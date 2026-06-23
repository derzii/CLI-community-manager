use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use ratatui::widgets::{ListState, TableState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    analytics::AnalyticsSummary,
    config::{AutomationRule, ColumnConfig, FilterPreset, ProfileManager, RuleActionType,
             RuleConditionOp, RuleConditionSource},
    discord::{AuditEntry, DiscordClient, Role},
    error::Result,
    faq::FaqManager,
    logger::{ActivityLogger, LogEntry},
    notion::{build_prop, DbRef, NotionClient, Page, Schema},
};

// ── Screens ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Dashboard,
    Members,
    Discord,
    Faq,
    Payments,
    Activity,
    Settings,
    Database,
    Analytics,  // key 9 — Track D
    Automation, // key 0 — Track F
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode { Normal, Editing }

#[derive(Debug, Clone, PartialEq)]
pub enum DbMode {
    Browse,
    ConfigureColumns,
    SwitchDatabase,
    EditCell,
    ViewBody,       // Track C: inline page-body viewer
    FilterPresets,  // Track C: named filter picker
}

// ── Async events ──────────────────────────────────────────────────────────────

pub enum AppEvent {
    // Members / shared Notion
    SchemaLoaded(Schema),
    MembersLoaded(Vec<Page>),
    MemberCreated(Page),
    MemberUpdated(Page),
    MemberRemoved,
    // Discord
    DiscordInvite(String),
    DiscordMembersLoaded(Vec<crate::discord::Member>),
    DiscordRolesLoaded(Vec<Role>),
    DiscordAuditLoaded(Vec<AuditEntry>),
    // Activity
    ActivityLoaded(Vec<LogEntry>),
    // Database tab
    DbSchemaLoaded(Schema),
    DbRowsLoaded(Vec<Page>),
    DbDatabasesListed(Vec<DbRef>),
    DbRowUpdated(Page),
    DbPageBodyLoaded(String),
    // Analytics
    AnalyticsSummaryLoaded(AnalyticsSummary),
    // Automation
    AutomationRuleResult { rule_id: String, result: String },
    // Global search (Track G)
    GlobalSearchResults(Vec<SearchResult>),
    // Generic
    StatusMsg(String),
    ErrMsg(String),
}

// ── Search types (Track G) ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SearchSource { Notion, Discord, Faq, Activity }

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub source: SearchSource,
    pub id: String,
    pub title: String,
    pub preview: String,
}

// ── Undo stack (Track A) ──────────────────────────────────────────────────────

pub enum UndoAction {
    UpdatePage { page_id: String, old_props: HashMap<String, serde_json::Value> },
    CreatePage { page_id: String },
}

// ── Command palette (Track A) ─────────────────────────────────────────────────

pub struct PaletteCmd {
    pub keys: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

pub const PALETTE_CMDS: &[PaletteCmd] = &[
    PaletteCmd { keys: "1 dash",       label: "Dashboard",      description: "Go to Dashboard" },
    PaletteCmd { keys: "2 mem",        label: "Members",        description: "Go to Members (2)" },
    PaletteCmd { keys: "3 dis",        label: "Discord",        description: "Go to Discord (3)" },
    PaletteCmd { keys: "4 faq",        label: "FAQ",            description: "Go to FAQ (4)" },
    PaletteCmd { keys: "5 pay",        label: "Payments",       description: "Go to Payments (5)" },
    PaletteCmd { keys: "6 log act",    label: "Activity Log",   description: "Go to Activity (6)" },
    PaletteCmd { keys: "7 set",        label: "Settings",       description: "Go to Settings (7)" },
    PaletteCmd { keys: "8 db database",label: "Database",       description: "Go to Database (8)" },
    PaletteCmd { keys: "9 an stat",    label: "Analytics",      description: "Go to Analytics (9)" },
    PaletteCmd { keys: "0 auto rule",  label: "Automation",     description: "Go to Automation (0)" },
    PaletteCmd { keys: "search glob",  label: "Global Search",  description: "Open global search (`)" },
    PaletteCmd { keys: "refresh ref",  label: "Refresh",        description: "Refresh current view (r/F5)" },
    PaletteCmd { keys: "export csv",   label: "Export CSV",     description: "Export log as CSV" },
    PaletteCmd { keys: "onboard new",  label: "Onboard Member", description: "Run onboarding pipeline" },
    PaletteCmd { keys: "run rules",    label: "Run All Rules",  description: "Execute all automation rules" },
    PaletteCmd { keys: "quit q",       label: "Quit",           description: "Quit the application" },
];

// ── Per-screen form types ─────────────────────────────────────────────────────

pub struct FormField {
    pub key: String,
    pub label: String,
    pub value: String,
    pub kind: String,
    pub options: Vec<String>,
    pub opt_idx: usize,
}

pub struct MemberForm {
    pub is_edit: bool,
    pub page_id: Option<String>,
    pub fields: Vec<FormField>,
    pub active: usize,
}

impl MemberForm {
    pub fn for_schema(schema: &Schema) -> Self {
        let fields = schema.editable().map(|p| FormField {
            key: p.name.clone(), label: p.name.clone(), value: String::new(),
            kind: p.kind.clone(), options: p.options.clone(), opt_idx: 0,
        }).collect();
        MemberForm { is_edit: false, page_id: None, fields, active: 0 }
    }

    pub fn for_edit(page: &Page, schema: &Schema) -> Self {
        let fields = schema.editable().map(|p| {
            let value = page.display(&p.name);
            let opt_idx = p.options.iter().position(|o| o == &value).unwrap_or(0);
            FormField {
                key: p.name.clone(), label: p.name.clone(), value,
                kind: p.kind.clone(), options: p.options.clone(), opt_idx,
            }
        }).collect();
        MemberForm { is_edit: true, page_id: Some(page.id.clone()), fields, active: 0 }
    }

    pub fn to_props(&self) -> HashMap<String, serde_json::Value> {
        self.fields.iter().map(|f| {
            let val = if matches!(f.kind.as_str(), "select" | "multi_select" | "status") {
                f.options.get(f.opt_idx).cloned().unwrap_or_else(|| f.value.clone())
            } else { f.value.clone() };
            (f.key.clone(), build_prop(&f.kind, &val))
        }).collect()
    }
}

pub struct SnippetForm {
    pub id: Option<String>,
    pub title: String,
    pub content: String,
    pub tags: String,
    pub active: usize,
}

pub struct ProfileForm {
    pub is_edit: bool,
    pub original_name: Option<String>,
    pub name: String,
    pub notion_key: String,
    pub notion_db: String,
    pub discord_token: String,
    pub discord_guild: String,
    pub discord_channel: String,
    pub active: usize,
}

impl ProfileForm {
    pub fn new() -> Self {
        ProfileForm {
            is_edit: false, original_name: None, name: String::new(),
            notion_key: String::new(), notion_db: String::new(),
            discord_token: String::new(), discord_guild: String::new(),
            discord_channel: String::new(), active: 0,
        }
    }
    pub fn from_profile(p: &crate::config::Profile) -> Self {
        ProfileForm {
            is_edit: true, original_name: Some(p.name.clone()),
            name: p.name.clone(), notion_key: p.notion_api_key.clone(),
            notion_db: p.notion_database_id.clone(),
            discord_token: p.discord_bot_token.clone(),
            discord_guild: p.discord_guild_id.clone(),
            discord_channel: p.discord_default_channel_id.clone(), active: 0,
        }
    }
    pub fn field_labels() -> &'static [&'static str] {
        &["Profile name","Notion API key","Notion database ID",
          "Discord bot token","Discord guild ID","Discord default channel ID"]
    }
    pub fn field_values(&self) -> Vec<&str> {
        vec![&self.name, &self.notion_key, &self.notion_db,
             &self.discord_token, &self.discord_guild, &self.discord_channel]
    }
    pub fn field_value_mut(&mut self, i: usize) -> Option<&mut String> {
        match i {
            0 => Some(&mut self.name),         1 => Some(&mut self.notion_key),
            2 => Some(&mut self.notion_db),    3 => Some(&mut self.discord_token),
            4 => Some(&mut self.discord_guild), 5 => Some(&mut self.discord_channel),
            _ => None,
        }
    }
}

/// Form for creating / editing an automation rule.
pub struct AutoRuleForm {
    pub is_edit: bool,
    pub original_id: Option<String>,
    // fields (active index 0-9)
    pub name: String,
    pub cond_source: RuleConditionSource,
    pub cond_field: String,
    pub cond_op: RuleConditionOp,
    pub cond_value: String,
    pub action_type: RuleActionType,
    pub action_target: String,
    pub action_value: String,
    pub action_field_kind: String,
    pub enabled: bool,
    pub active: usize,
}

impl AutoRuleForm {
    pub fn new() -> Self {
        AutoRuleForm {
            is_edit: false, original_id: None, name: String::new(),
            cond_source: RuleConditionSource::Notion, cond_field: String::new(),
            cond_op: RuleConditionOp::Equals, cond_value: String::new(),
            action_type: RuleActionType::AddDiscordRole, action_target: String::new(),
            action_value: String::new(), action_field_kind: String::new(), enabled: true, active: 0,
        }
    }
    pub fn from_rule(r: &AutomationRule) -> Self {
        AutoRuleForm {
            is_edit: true, original_id: Some(r.id.clone()), name: r.name.clone(),
            cond_source: r.condition.source.clone(), cond_field: r.condition.field.clone(),
            cond_op: r.condition.op.clone(), cond_value: r.condition.value.clone(),
            action_type: r.action.action_type.clone(), action_target: r.action.target.clone(),
            action_value: r.action.value.clone(), action_field_kind: r.action.field_kind.clone(),
            enabled: r.enabled, active: 0,
        }
    }
    pub fn field_count() -> usize { 10 }
    pub fn field_label(i: usize) -> &'static str {
        match i {
            0 => "Rule name",          1 => "Condition source (←/→)",
            2 => "Condition field",    3 => "Condition op (←/→)",
            4 => "Condition value",    5 => "Action type (←/→)",
            6 => "Action target",      7 => "Action value",
            8 => "Field kind (if SetNotionField)", 9 => "Enabled (←/→)",
            _ => "",
        }
    }
    pub fn field_display(&self, i: usize) -> String {
        match i {
            0 => self.name.clone(),
            1 => self.cond_source.label().into(),
            2 => self.cond_field.clone(),
            3 => self.cond_op.label().into(),
            4 => self.cond_value.clone(),
            5 => self.action_type.label().into(),
            6 => self.action_target.clone(),
            7 => self.action_value.clone(),
            8 => self.action_field_kind.clone(),
            9 => if self.enabled { "yes" } else { "no" }.into(),
            _ => String::new(),
        }
    }
    pub fn field_value_mut(&mut self, i: usize) -> Option<&mut String> {
        match i {
            0 => Some(&mut self.name),           2 => Some(&mut self.cond_field),
            4 => Some(&mut self.cond_value),     6 => Some(&mut self.action_target),
            7 => Some(&mut self.action_value),   8 => Some(&mut self.action_field_kind),
            _ => None,
        }
    }
    pub fn cycle_left(&mut self, i: usize) {
        match i {
            1 => self.cond_source = self.cond_source.cycle(),
            3 => self.cond_op = self.cond_op.cycle(),
            5 => self.action_type = self.action_type.cycle(),
            9 => self.enabled = !self.enabled,
            _ => {}
        }
    }
    pub fn to_rule(&self) -> AutomationRule {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let id = self.original_id.clone()
            .unwrap_or_else(|| format!("rule_{}", t.as_millis()));
        AutomationRule {
            id, name: self.name.clone(), enabled: self.enabled,
            condition: crate::config::RuleCondition {
                source: self.cond_source.clone(), field: self.cond_field.clone(),
                op: self.cond_op.clone(), value: self.cond_value.clone(),
            },
            action: crate::config::RuleAction {
                action_type: self.action_type.clone(), target: self.action_target.clone(),
                value: self.action_value.clone(), field_kind: self.action_field_kind.clone(),
            },
            last_run: None, last_result: None,
        }
    }
}

// ── Onboarding form ───────────────────────────────────────────────────────────

pub struct OnboardForm {
    pub name: String,
    pub email: String,
    pub channel: String,
    pub active: usize,
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    // ── Core ─────────────────────────────────────────────────────────────────
    pub screen: Screen,
    pub input_mode: InputMode,
    pub status: String,
    pub should_quit: bool,

    // Async bridge
    pub tx: UnboundedSender<AppEvent>,
    pub rx: UnboundedReceiver<AppEvent>,

    // Services
    pub notion: Option<NotionClient>,
    pub discord: Option<DiscordClient>,
    pub faq: FaqManager,
    pub logger: ActivityLogger,
    pub profiles: ProfileManager,

    // ── Track A: UX ──────────────────────────────────────────────────────────
    pub show_help: bool,
    pub show_palette: bool,
    pub palette_input: String,
    pub palette_sel: usize,
    pub undo_stack: Vec<UndoAction>,

    // ── Track G: Global search overlay ───────────────────────────────────────
    pub gs_open: bool,
    pub gs_input: String,
    pub gs_results: Vec<SearchResult>,
    pub gs_sel: usize,
    pub gs_loading: bool,

    // ── Members screen ────────────────────────────────────────────────────────
    pub m_pages: Vec<Page>,
    pub m_schema: Option<Schema>,
    pub m_list: TableState,
    pub m_sel: usize,
    pub m_search: String,
    pub m_search_mode: bool,
    pub m_form: Option<MemberForm>,
    pub m_loading: bool,

    // ── Discord screen ────────────────────────────────────────────────────────
    pub d_channel: String,
    pub d_hours: u64,
    pub d_uses: u32,
    pub d_invite_result: String,
    pub d_user_query: String,
    pub d_discord_members: Vec<crate::discord::Member>,
    pub d_members_list: ListState,
    pub d_sel: usize,
    pub d_action_input: String,
    pub d_reason: String,
    pub d_active_field: usize,
    pub d_section: usize, // 0=invite 1=members 2=kick/ban 3=roles 4=broadcast
    // Track B additions
    pub d_roles: Vec<Role>,
    pub d_roles_list: ListState,
    pub d_role_sel: usize,
    pub d_assign_user: String,   // user_id for role assign/remove
    pub d_broadcast_channel: String,
    pub d_broadcast_msg: String,
    pub d_dm_user: String,
    pub d_dm_msg: String,
    pub d_audit_log: Vec<AuditEntry>,

    // ── FAQ screen ────────────────────────────────────────────────────────────
    pub f_filtered: Vec<usize>,
    pub f_list: ListState,
    pub f_sel: usize,
    pub f_search: String,
    pub f_search_mode: bool,
    pub f_form: Option<SnippetForm>,
    pub f_copied: bool,
    pub f_show_preview: bool,

    // ── Payments screen ───────────────────────────────────────────────────────
    pub p_search: String,
    pub p_search_mode: bool,
    pub p_results: Vec<Page>,
    pub p_res_list: TableState,
    pub p_sel: usize,
    pub p_status_val: String,
    pub p_notes: String,
    pub p_active_field: usize,

    // ── Activity screen ───────────────────────────────────────────────────────
    pub a_logs: Vec<LogEntry>,
    pub a_list: ListState,
    pub a_sel: usize,
    pub a_filter: String,
    pub a_filter_mode: bool,

    // ── Settings screen ───────────────────────────────────────────────────────
    pub s_list: ListState,
    pub s_sel: usize,
    pub s_form: Option<ProfileForm>,

    // ── Database screen ────────────────────────────────────────────────────────
    pub db_database_id: Option<String>,
    pub db_db_name: String,
    pub db_schema: Option<Schema>,
    pub db_columns: Vec<ColumnConfig>,
    pub db_rows: Vec<Page>,
    pub db_table: TableState,
    pub db_row_sel: usize,
    pub db_col_sel: usize,
    pub db_mode: DbMode,
    pub db_loading: bool,
    pub db_search: String,
    pub db_search_mode: bool,
    pub db_cfg_sel: usize,
    pub db_picker_input: String,
    pub db_picker_sel: usize,
    pub db_available: Vec<DbRef>,
    pub db_edit_buf: String,
    pub db_edit_opt_idx: usize,
    // Track C additions
    pub db_page_body: String,
    pub db_body_loading: bool,
    pub db_body_append_buf: String,
    pub db_filter_presets: Vec<FilterPreset>,
    pub db_preset_sel: usize,
    pub db_bulk_sel: HashSet<usize>,
    pub db_bulk_mode: bool,

    // ── Analytics screen (Track D) ─────────────────────────────────────────
    pub an_summary: Option<AnalyticsSummary>,
    pub an_loading: bool,
    pub an_export_msg: String,

    // ── Automation screen (Track F) ────────────────────────────────────────
    pub auto_rules: Vec<AutomationRule>,
    pub auto_list: ListState,
    pub auto_sel: usize,
    pub auto_form: Option<AutoRuleForm>,
    pub auto_running: bool,
    pub auto_last_result: String,
    pub auto_onboard_form: Option<OnboardForm>,
}

impl App {
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let faq     = FaqManager::load()?;
        let logger  = ActivityLogger::open()?;
        let profiles = ProfileManager::load()?;

        let auto_rules = profiles.active()
            .map(|p| p.automation_rules.clone())
            .unwrap_or_default();

        let mut app = App {
            screen: Screen::Dashboard, input_mode: InputMode::Normal,
            status: "Ready – press ? for help\".into()" .into(), should_quit: false,
            tx, rx,
            notion: None, discord: None, faq, logger, profiles,
            // UX
            show_help: false, show_palette: false,
            palette_input: String::new(), palette_sel: 0,
            undo_stack: vec![],
            // Global search
            gs_open: false, gs_input: String::new(), gs_results: vec![],
            gs_sel: 0, gs_loading: false,
            // Members
            m_pages: vec![], m_schema: None,
            m_list: TableState::default(), m_sel: 0,
            m_search: String::new(), m_search_mode: false,
            m_form: None, m_loading: false,
            // Discord
            d_channel: String::new(), d_hours: 24, d_uses: 1,
            d_invite_result: String::new(), d_user_query: String::new(),
            d_discord_members: vec![], d_members_list: ListState::default(),
            d_sel: 0, d_action_input: String::new(), d_reason: String::new(),
            d_active_field: 0, d_section: 0,
            d_roles: vec![], d_roles_list: ListState::default(), d_role_sel: 0,
            d_assign_user: String::new(),
            d_broadcast_channel: String::new(), d_broadcast_msg: String::new(),
            d_dm_user: String::new(), d_dm_msg: String::new(),
            d_audit_log: vec![],
            // FAQ
            f_filtered: vec![], f_list: ListState::default(), f_sel: 0,
            f_search: String::new(), f_search_mode: false,
            f_form: None, f_copied: false, f_show_preview: false,
            // Payments
            p_search: String::new(), p_search_mode: false,
            p_results: vec![], p_res_list: TableState::default(), p_sel: 0,
            p_status_val: String::new(), p_notes: String::new(), p_active_field: 0,
            // Activity
            a_logs: vec![], a_list: ListState::default(), a_sel: 0,
            a_filter: String::new(), a_filter_mode: false,
            // Settings
            s_list: ListState::default(), s_sel: 0, s_form: None,
            // Database
            db_database_id: None, db_db_name: String::new(),
            db_schema: None, db_columns: vec![], db_rows: vec![],
            db_table: TableState::default(), db_row_sel: 0, db_col_sel: 0,
            db_mode: DbMode::Browse, db_loading: false,
            db_search: String::new(), db_search_mode: false,
            db_cfg_sel: 0, db_picker_input: String::new(), db_picker_sel: 0,
            db_available: vec![], db_edit_buf: String::new(), db_edit_opt_idx: 0,
            db_page_body: String::new(), db_body_loading: false,
            db_body_append_buf: String::new(),
            db_filter_presets: vec![], db_preset_sel: 0,
            db_bulk_sel: HashSet::new(), db_bulk_mode: false,
            // Analytics
            an_summary: None, an_loading: false, an_export_msg: String::new(),
            // Automation
            auto_rules, auto_list: ListState::default(), auto_sel: 0,
            auto_form: None, auto_running: false, auto_last_result: String::new(),
            auto_onboard_form: None,
        };
        app.apply_active_profile();
        app.f_filtered = app.faq.filtered("");
        if !app.f_filtered.is_empty() { app.f_list.select(Some(0)); }
        Ok(app)
    }

    pub fn apply_active_profile(&mut self) {
        if let Some(p) = self.profiles.active() {
            if p.has_notion() {
                self.notion = Some(NotionClient::new(&p.notion_api_key, &p.notion_database_id));
            }
            if p.has_discord() {
                self.discord = Some(DiscordClient::new(
                    &p.discord_bot_token, &p.discord_guild_id, &p.discord_default_channel_id,
                ));
            }
            self.auto_rules = p.automation_rules.clone();
        }
    }

    // ── Event processing ──────────────────────────────────────────────────────

    pub fn drain_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() { self.process(ev); }
    }

    fn process(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::SchemaLoaded(s) => {
                self.status = format!("Schema: {} props", s.props.len());
                self.m_schema = Some(s);
            }
            AppEvent::MembersLoaded(pages) => {
                self.status = format!("Loaded {} members", pages.len());
                self.m_sel = 0;
                if !pages.is_empty() { self.m_list.select(Some(0)); }
                self.m_pages = pages; self.m_loading = false;
                let _ = self.logger.write("notion_query", "database", "members loaded", true);
            }
            AppEvent::MemberCreated(p) => {
                self.status = format!("Added: {}", p.id);
                self.m_form = None; self.m_loading = false;
                let _ = self.logger.write("notion_add", &p.id, "page created", true);
                self.trigger_load_members();
            }
            AppEvent::MemberUpdated(p) => {
                self.status = format!("Updated: {}", p.id);
                self.m_form = None; self.m_loading = false;
                let _ = self.logger.write("notion_update", &p.id, "page updated", true);
                self.trigger_load_members();
            }
            AppEvent::MemberRemoved => {
                self.status = "Member removed".into(); self.m_loading = false;
                let _ = self.logger.write("notion_remove", "page", "archived", true);
                self.trigger_load_members();
            }
            AppEvent::DiscordInvite(url) => {
                self.d_invite_result = url.clone();
                self.status = format!("Invite: {url}");
                let _ = self.logger.write("discord_invite", &url, "", true);
                crate::faq::copy_to_clipboard(&url);
            }
            AppEvent::DiscordMembersLoaded(members) => {
                self.status = format!("Discord: {} members", members.len());
                self.d_sel = 0;
                if !members.is_empty() { self.d_members_list.select(Some(0)); }
                self.d_discord_members = members;
            }
            AppEvent::DiscordRolesLoaded(roles) => {
                self.status = format!("{} guild roles", roles.len());
                self.d_role_sel = 0;
                if !roles.is_empty() { self.d_roles_list.select(Some(0)); }
                self.d_roles = roles;
            }
            AppEvent::DiscordAuditLoaded(entries) => {
                self.status = format!("Audit log: {} entries", entries.len());
                self.d_audit_log = entries;
            }
            AppEvent::ActivityLoaded(logs) => {
                self.a_sel = 0;
                if !logs.is_empty() { self.a_list.select(Some(0)); }
                self.a_logs = logs;
            }
            AppEvent::StatusMsg(s) => self.status = s,
            AppEvent::ErrMsg(e) => {
                self.status = format!("Error: {e}");
                self.m_loading = false; self.db_loading = false;
                self.gs_loading = false; self.an_loading = false;
                self.auto_running = false;
            }
            // Database tab
            AppEvent::DbSchemaLoaded(s) => {
                if self.db_columns.is_empty() {
                    self.db_columns = s.props.iter().map(|p| ColumnConfig {
                        property_name: p.name.clone(), visible: true, editable: p.editable,
                    }).collect();
                } else {
                    for p in &s.props {
                        if !self.db_columns.iter().any(|c| c.property_name == p.name) {
                            self.db_columns.push(ColumnConfig {
                                property_name: p.name.clone(), visible: true, editable: p.editable,
                            });
                        }
                    }
                }
                self.status = format!("Database schema: {} properties", s.props.len());
                self.db_schema = Some(s);
                // Refresh filter presets from profile
                if let (Some(db_id), Some(p)) = (&self.db_database_id, self.profiles.active()) {
                    self.db_filter_presets = p.database_tab.filter_presets
                        .get(db_id).cloned().unwrap_or_default();
                }
            }
            AppEvent::DbRowsLoaded(rows) => {
                self.status = format!("Loaded {} rows", rows.len());
                self.db_row_sel = 0; self.db_bulk_sel.clear();
                if !rows.is_empty() { self.db_table.select(Some(0)); }
                else { self.db_table.select(None); }
                self.db_rows = rows; self.db_loading = false;
            }
            AppEvent::DbDatabasesListed(dbs) => {
                self.status = format!("Found {} databases", dbs.len());
                self.db_picker_sel = 0; self.db_available = dbs;
            }
            AppEvent::DbRowUpdated(p) => {
                self.status = format!("Row updated: {}", p.id);
                self.trigger_db_load_rows();
            }
            AppEvent::DbPageBodyLoaded(body) => {
                self.db_page_body = body;
                self.db_body_loading = false;
                self.status = "Page body loaded.".into();
            }
            // Analytics
            AppEvent::AnalyticsSummaryLoaded(s) => {
                self.an_summary = Some(s); self.an_loading = false;
                self.status = "Analytics refreshed.".into();
            }
            // Automation
            AppEvent::AutomationRuleResult { rule_id, result } => {
                self.auto_last_result = format!("[{rule_id}] {result}");
                self.auto_running = false;
                let _ = self.logger.write("automation_rule", &rule_id, &result, true);
                // Persist last_result into the rule
                let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
                if let Some(r) = self.auto_rules.iter_mut().find(|r| r.id == rule_id) {
                    r.last_run = Some(ts); r.last_result = Some(result);
                }
                self.save_automation_rules();
            }
            // Global search
            AppEvent::GlobalSearchResults(mut results) => {
                self.gs_results.append(&mut results);
                self.gs_loading = self.gs_loading; // last batch sets to false externally
                self.gs_loading = false;
                self.status = format!("Found {} result(s)", self.gs_results.len());
            }
        }
    }

    // ── Spawn helpers ─────────────────────────────────────────────────────────

    pub fn trigger_load_members(&mut self) {
        let Some(n) = self.notion.clone() else {
            self.status = "No Notion profile. Configure in Settings (7).".into();
            return;
        };
        self.m_loading = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match n.query(None).await {
                Ok(p)  => { let _ = tx.send(AppEvent::MembersLoaded(p)); }
                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    fn trigger_load_schema(&mut self) {
        let Some(n) = self.notion.clone() else { return; };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match n.fetch_schema().await {
                Ok(s)  => { let _ = tx.send(AppEvent::SchemaLoaded(s)); }
                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    fn trigger_load_activity(&mut self) {
        match self.logger.recent(200) {
            Ok(logs) => { let _ = self.tx.send(AppEvent::ActivityLoaded(logs)); }
            Err(e)   => self.status = format!("Log error: {e}"),
        }
    }

    fn trigger_discord_invite(&mut self) {
        let Some(d) = self.discord.clone() else {
            self.status = "No Discord profile. Configure in Settings (7).".into();
            return;
        };
        let (ch, h, u) = (self.d_channel.clone(), self.d_hours, self.d_uses);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match d.create_invite(&ch, h, u).await {
                Ok(inv) => { let _ = tx.send(AppEvent::DiscordInvite(inv.url)); }
                Err(e)  => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    fn trigger_discord_members(&mut self) {
        let Some(d) = self.discord.clone() else { return; };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match d.get_members(100).await {
                Ok(m)  => { let _ = tx.send(AppEvent::DiscordMembersLoaded(m)); }
                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    fn trigger_discord_roles(&mut self) {
        let Some(d) = self.discord.clone() else { return; };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match d.list_roles().await {
                Ok(r)  => { let _ = tx.send(AppEvent::DiscordRolesLoaded(r)); }
                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    fn trigger_discord_audit(&mut self) {
        let Some(d) = self.discord.clone() else { return; };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match d.get_audit_log(50).await {
                Ok(a)  => { let _ = tx.send(AppEvent::DiscordAuditLoaded(a)); }
                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    // ── Database tab spawn helpers ────────────────────────────────────────────

    fn trigger_db_load_schema(&mut self) {
        let (Some(n), Some(db_id)) = (self.notion.clone(), self.db_database_id.clone()) else {
            self.status = "No Notion profile. Configure in Settings (7).".into();
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match n.fetch_schema_of(&db_id).await {
                Ok(s)  => { let _ = tx.send(AppEvent::DbSchemaLoaded(s)); }
                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    fn trigger_db_load_rows(&mut self) {
        let (Some(n), Some(db_id)) = (self.notion.clone(), self.db_database_id.clone()) else { return; };
        self.db_loading = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match n.query_of(&db_id, None).await {
                Ok(p)  => { let _ = tx.send(AppEvent::DbRowsLoaded(p)); }
                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    fn trigger_db_list_databases(&mut self) {
        let Some(n) = self.notion.clone() else {
            self.status = "No Notion profile. Configure in Settings (7).".into();
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match n.list_databases().await {
                Ok(dbs) => { let _ = tx.send(AppEvent::DbDatabasesListed(dbs)); }
                Err(e)  => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    fn trigger_db_page_body(&mut self) {
        let Some(page) = self.db_rows.get(self.db_row_sel).cloned() else { return; };
        let Some(n) = self.notion.clone() else { return; };
        self.db_body_loading = true;
        let tx = self.tx.clone();
        let pid = page.id.clone();
        tokio::spawn(async move {
            match n.fetch_page_body(&pid).await {
                Ok(body) => { let _ = tx.send(AppEvent::DbPageBodyLoaded(body)); }
                Err(e)   => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    fn trigger_db_load_rows_filtered(&mut self, filter: serde_json::Value) {
        let (Some(n), Some(db_id)) = (self.notion.clone(), self.db_database_id.clone()) else { return; };
        self.db_loading = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match n.query_of(&db_id, Some(filter)).await {
                Ok(p)  => { let _ = tx.send(AppEvent::DbRowsLoaded(p)); }
                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    // ── Analytics spawn helpers ───────────────────────────────────────────────

    fn trigger_analytics(&mut self) {
        self.an_loading = true;
        match self.logger.analytics_summary() {
            Ok(s)  => { let _ = self.tx.send(AppEvent::AnalyticsSummaryLoaded(s)); }
            Err(e) => { self.status = format!("Analytics error: {e}"); self.an_loading = false; }
        }
    }

    fn trigger_export_csv(&mut self) {
        match self.logger.export_log_csv(1000) {
            Ok(csv) => {
                let path = "/tmp/theblackroom_export.csv";
                match std::fs::write(path, csv) {
                    Ok(_)  => self.an_export_msg = format!("Exported to {path}"),
                    Err(e) => self.an_export_msg = format!("Export failed: {e}"),
                }
            }
            Err(e) => self.an_export_msg = format!("Export error: {e}"),
        }
    }

    // ── Automation spawn helpers ──────────────────────────────────────────────

    fn trigger_run_rule(&mut self, rule: AutomationRule) {
        let (Some(n), Some(d)) = (self.notion.clone(), self.discord.clone()) else {
            self.status = "Automation needs both Notion and Discord configured.".into();
            return;
        };
        self.auto_running = true;
        let tx = self.tx.clone();
        let rule_id = rule.id.clone();
        tokio::spawn(async move {
            let result = crate::automation::run_rule(&rule, &n, &d).await;
            let _ = tx.send(AppEvent::AutomationRuleResult { rule_id, result });
        });
    }

    fn trigger_onboarding(&mut self, name: String, email: String, channel: String) {
        let (Some(n), Some(d)) = (self.notion.clone(), self.discord.clone()) else {
            self.status = "Onboarding needs both Notion and Discord configured.".into();
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match crate::automation::run_onboarding(&n, &d, &name, &email, &channel).await {
                Ok(msg)  => { let _ = tx.send(AppEvent::StatusMsg(format!("Onboarding: {msg}"))); }
                Err(e)   => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
            }
        });
    }

    fn save_automation_rules(&mut self) {
        let rules = self.auto_rules.clone();
        if let Some(p) = self.profiles.profiles.get_mut(self.profiles.active_idx) {
            p.automation_rules = rules;
        }
        let _ = self.profiles.save();
    }

    // ── Global search spawn (Track G) ────────────────────────────────────────

    pub fn trigger_global_search(&mut self) {
        let query = self.gs_input.clone();
        if query.is_empty() { return; }
        self.gs_results.clear();
        self.gs_loading = true;
        self.gs_sel = 0;

        // 1. Notion
        if let Some(n) = self.notion.clone() {
            let tx = self.tx.clone();
            let q = query.clone();
            tokio::spawn(async move {
                if let Ok(pages) = n.search_global(&q).await {
                    let results = pages.into_iter().map(|p| {
                        let title = p.title();
                        SearchResult {
                            source: SearchSource::Notion,
                            id: p.id.clone(),
                            title: if title.is_empty() { p.id.clone() } else { title },
                            preview: String::new(),
                        }
                    }).collect();
                    let _ = tx.send(AppEvent::GlobalSearchResults(results));
                }
            });
        }

        // 2. Discord members
        if let Some(d) = self.discord.clone() {
            let tx = self.tx.clone();
            let q = query.clone();
            tokio::spawn(async move {
                if let Ok(members) = d.search_members(&q).await {
                    let results = members.into_iter().map(|m| SearchResult {
                        source: SearchSource::Discord,
                        id: m.user_id.clone(),
                        title: format!("{} (@{})", m.display, m.username),
                        preview: format!("Joined: {}", m.joined_at),
                    }).collect();
                    let _ = tx.send(AppEvent::GlobalSearchResults(results));
                }
            });
        }

        // 3. FAQ snippets (local)
        {
            let q_lower = query.to_lowercase();
            let faq_results: Vec<SearchResult> = self.faq.snippets.iter()
                .filter(|s| s.title.to_lowercase().contains(&q_lower)
                         || s.content.to_lowercase().contains(&q_lower))
                .map(|s| SearchResult {
                    source: SearchSource::Faq,
                    id: s.id.clone(),
                    title: s.title.clone(),
                    preview: s.content.chars().take(60).collect(),
                })
                .collect();
            if !faq_results.is_empty() {
                let _ = self.tx.send(AppEvent::GlobalSearchResults(faq_results));
            }
        }

        // 4. Activity log (local)
        {
            let q_lower = query.to_lowercase();
            if let Ok(log_results) = self.logger.search(&q_lower) {
                let results: Vec<SearchResult> = log_results.into_iter().map(|e| SearchResult {
                    source: SearchSource::Activity,
                    id: e.id.to_string(),
                    title: format!("{} – {}", e.kind, e.target),
                    preview: format!("{}: {}", e.ts, e.detail),
                }).collect();
                if !results.is_empty() {
                    let _ = self.tx.send(AppEvent::GlobalSearchResults(results));
                }
            }
        }
    }

    // ── Database tab helpers ──────────────────────────────────────────────────

    pub fn db_visible_columns(&self) -> Vec<&ColumnConfig> {
        self.db_columns.iter().filter(|c| c.visible).collect()
    }

    pub fn db_edit_kind(&self) -> Option<String> {
        let cols = self.db_visible_columns();
        let col = cols.get(self.db_col_sel)?;
        self.db_schema.as_ref()?.props.iter()
            .find(|p| p.name == col.property_name)
            .map(|p| p.kind.clone())
    }

    pub fn db_edit_options(&self) -> Vec<String> {
        let cols = self.db_visible_columns();
        cols.get(self.db_col_sel)
            .and_then(|col| self.db_schema.as_ref()?.props.iter()
                .find(|p| p.name == col.property_name))
            .map(|p| p.options.clone())
            .unwrap_or_default()
    }

    pub fn db_filtered_indices(&self) -> Vec<usize> {
        let q = self.db_picker_input.to_lowercase();
        self.db_available.iter().enumerate()
            .filter(|(_, d)| q.is_empty() || d.title.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    fn ensure_db_tab_initialized(&mut self) {
        if self.db_database_id.is_some() { return; }
        if let Some(p) = self.profiles.active() {
            let id = p.database_tab.active_database_id.clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| p.notion_database_id.clone());
            if !id.is_empty() {
                self.db_columns = p.database_tab.columns.get(&id).cloned().unwrap_or_default();
                self.db_database_id = Some(id);
            }
        }
    }

    fn switch_database(&mut self, id: String, title: String) {
        self.save_db_config();
        self.db_database_id = Some(id.clone());
        self.db_db_name = title;
        self.db_schema = None; self.db_rows = vec![];
        self.db_row_sel = 0; self.db_col_sel = 0;
        self.db_table = TableState::default();
        self.db_bulk_sel.clear();
        self.db_columns = self.profiles.active()
            .and_then(|p| p.database_tab.columns.get(&id).cloned())
            .unwrap_or_default();
        self.db_mode = DbMode::Browse;
        self.trigger_db_load_schema();
        self.trigger_db_load_rows();
    }

    fn save_db_config(&mut self) {
        let Some(db_id) = self.db_database_id.clone() else { return; };
        if let Some(profile) = self.profiles.profiles.get_mut(self.profiles.active_idx) {
            profile.database_tab.active_database_id = Some(db_id.clone());
            profile.database_tab.columns.insert(db_id.clone(), self.db_columns.clone());
            profile.database_tab.filter_presets.insert(db_id, self.db_filter_presets.clone());
        } else { return; }
        match self.profiles.save() {
            Ok(_)  => self.status = "Column layout saved.".into(),
            Err(e) => self.status = format!("Save error: {e}"),
        }
    }

    // ── Palette helpers ───────────────────────────────────────────────────────

    pub fn palette_filtered(&self) -> Vec<usize> {
        let q = self.palette_input.to_lowercase();
        PALETTE_CMDS.iter().enumerate()
            .filter(|(_, c)| q.is_empty()
                || c.keys.contains(&q as &str)
                || c.label.to_lowercase().contains(&q)
                || c.description.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    fn execute_palette_cmd(&mut self, label: &str) {
        match label {
            "Dashboard"      => { self.screen = Screen::Dashboard; }
            "Members"        => { self.screen = Screen::Members; if self.m_schema.is_none() { self.trigger_load_schema(); } }
            "Discord"        => { self.screen = Screen::Discord; }
            "FAQ"            => { self.screen = Screen::Faq; }
            "Payments"       => { self.screen = Screen::Payments; }
            "Activity Log"   => { self.screen = Screen::Activity; self.trigger_load_activity(); }
            "Settings"       => { self.screen = Screen::Settings; }
            "Database"       => { self.screen = Screen::Database; self.ensure_db_tab_initialized(); }
            "Analytics"      => { self.screen = Screen::Analytics; self.trigger_analytics(); }
            "Automation"     => { self.screen = Screen::Automation; }
            "Global Search"  => { self.show_palette = false; self.gs_open = true; self.input_mode = InputMode::Editing; }
            "Refresh"        => self.refresh_current_screen(),
            "Export CSV"     => self.trigger_export_csv(),
            "Onboard Member" => { self.screen = Screen::Automation; self.auto_onboard_form = Some(OnboardForm { name: String::new(), email: String::new(), channel: String::new(), active: 0 }); self.input_mode = InputMode::Editing; }
            "Run All Rules"  => self.trigger_run_all_rules(),
            "Quit"           => self.should_quit = true,
            _ => {}
        }
    }

    fn refresh_current_screen(&mut self) {
        match self.screen {
            Screen::Members  => self.trigger_load_members(),
            Screen::Activity => self.trigger_load_activity(),
            Screen::Database => self.trigger_db_load_rows(),
            Screen::Analytics => self.trigger_analytics(),
            Screen::Discord  => {
                if self.d_section == 1 { self.trigger_discord_members(); }
                else if self.d_section == 3 { self.trigger_discord_roles(); }
            }
            _ => {}
        }
    }

    fn trigger_run_all_rules(&mut self) {
        for rule in self.auto_rules.clone() {
            if rule.enabled { self.trigger_run_rule(rule); }
        }
    }

    // ── Key handling ──────────────────────────────────────────────────────────

    pub async fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Global search overlay
        if self.gs_open {
            self.handle_global_search_key(key);
            return false;
        }

        // Command palette overlay
        if self.show_palette {
            self.handle_palette_key(key);
            return false;
        }

        // Help overlay dismiss
        if self.show_help {
            self.show_help = false;
            return false;
        }

        // Esc: close forms / modes
        if key.code == KeyCode::Esc {
            if self.m_form.is_some() { self.m_form = None; self.input_mode = InputMode::Normal; return false; }
            if self.f_form.is_some() { self.f_form = None; self.input_mode = InputMode::Normal; return false; }
            if self.s_form.is_some() { self.s_form = None; self.input_mode = InputMode::Normal; return false; }
            if self.auto_form.is_some() { self.auto_form = None; self.input_mode = InputMode::Normal; return false; }
            if self.auto_onboard_form.is_some() { self.auto_onboard_form = None; self.input_mode = InputMode::Normal; return false; }
            if self.db_mode != DbMode::Browse {
                self.db_mode = DbMode::Browse; self.input_mode = InputMode::Normal; return false;
            }
            if self.input_mode == InputMode::Editing {
                self.input_mode = InputMode::Normal;
                self.m_search_mode = false; self.f_search_mode = false;
                self.a_filter_mode = false; self.p_search_mode = false;
                self.db_search_mode = false;
                return false;
            }
        }

        // Editing mode: route to current screen's text handler
        if self.input_mode == InputMode::Editing {
            self.handle_editing(key);
            return false;
        }

        // Ctrl+Z: undo
        if key.code == KeyCode::Char('z') && key.modifiers == KeyModifiers::CONTROL {
            self.handle_undo();
            return false;
        }

        // Global overlays
        match key.code {
            KeyCode::Char('?') => { self.show_help = true; return false; }
            KeyCode::Char(':') => {
                self.show_palette = true;
                self.palette_input.clear();
                self.palette_sel = 0;
                self.input_mode = InputMode::Editing;
                return false;
            }
            KeyCode::Char('`') => {
                self.gs_open = !self.gs_open;
                if self.gs_open { self.gs_input.clear(); self.gs_results.clear(); self.gs_sel = 0; self.input_mode = InputMode::Editing; }
                return false;
            }
            _ => {}
        }

        // Screen switches
        match key.code {
            KeyCode::Char('1') => { self.screen = Screen::Dashboard; return false; }
            KeyCode::Char('2') => {
                self.screen = Screen::Members;
                if self.m_schema.is_none() { self.trigger_load_schema(); }
                if self.m_pages.is_empty() { self.trigger_load_members(); }
                return false;
            }
            KeyCode::Char('3') => { self.screen = Screen::Discord; return false; }
            KeyCode::Char('4') => { self.screen = Screen::Faq; return false; }
            KeyCode::Char('5') => { self.screen = Screen::Payments; return false; }
            KeyCode::Char('6') => { self.screen = Screen::Activity; self.trigger_load_activity(); return false; }
            KeyCode::Char('7') => { self.screen = Screen::Settings; return false; }
            KeyCode::Char('8') => {
                self.screen = Screen::Database;
                self.ensure_db_tab_initialized();
                if self.db_schema.is_none() { self.trigger_db_load_schema(); }
                if self.db_rows.is_empty() { self.trigger_db_load_rows(); }
                return false;
            }
            KeyCode::Char('9') => {
                self.screen = Screen::Analytics;
                if self.an_summary.is_none() { self.trigger_analytics(); }
                return false;
            }
            KeyCode::Char('0') => { self.screen = Screen::Automation; return false; }
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
                    return true;
                }
            }
            _ => {}
        }

        // Per-screen keys
        match self.screen.clone() {
            Screen::Members    => self.key_members(key).await,
            Screen::Discord    => self.key_discord(key).await,
            Screen::Faq        => self.key_faq(key),
            Screen::Payments   => self.key_payments(key).await,
            Screen::Activity   => self.key_activity(key),
            Screen::Settings   => self.key_settings(key),
            Screen::Database   => self.key_database(key),
            Screen::Analytics  => self.key_analytics(key),
            Screen::Automation => self.key_automation(key).await,
            Screen::Dashboard  => {}
        }
        false
    }

    // ── Undo ──────────────────────────────────────────────────────────────────

    fn handle_undo(&mut self) {
        let Some(action) = self.undo_stack.pop() else {
            self.status = "Nothing to undo.".into();
            return;
        };
        match action {
            UndoAction::UpdatePage { page_id, old_props } => {
                if let Some(n) = self.notion.clone() {
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        match n.update_page(&page_id, old_props).await {
                            Ok(p)  => { let _ = tx.send(AppEvent::DbRowUpdated(p)); }
                            Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                        }
                    });
                    self.status = "Undoing last cell update…".into();
                }
            }
            UndoAction::CreatePage { page_id } => {
                if let Some(n) = self.notion.clone() {
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        match n.archive_page(&page_id).await {
                            Ok(_)  => { let _ = tx.send(AppEvent::MemberRemoved); }
                            Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                        }
                    });
                    self.status = "Undoing create – archiving page…".into();
                }
            }
        }
    }

    // ── Editing mode routing ──────────────────────────────────────────────────

    fn handle_editing(&mut self, key: KeyEvent) {
        // Command palette receives keys while show_palette = true
        if self.show_palette {
            match key.code {
                KeyCode::Esc => { self.show_palette = false; self.input_mode = InputMode::Normal; }
                KeyCode::Down => {
                    let n = self.palette_filtered().len();
                    if self.palette_sel + 1 < n { self.palette_sel += 1; }
                }
                KeyCode::Up => { if self.palette_sel > 0 { self.palette_sel -= 1; } }
                KeyCode::Enter => {
                    let filtered = self.palette_filtered();
                    if let Some(&idx) = filtered.get(self.palette_sel) {
                        let label = PALETTE_CMDS[idx].label;
                        self.show_palette = false;
                        self.input_mode = InputMode::Normal;
                        self.execute_palette_cmd(label);
                    }
                }
                KeyCode::Char(c) => { self.palette_input.push(c); self.palette_sel = 0; }
                KeyCode::Backspace => { self.palette_input.pop(); self.palette_sel = 0; }
                _ => {}
            }
            return;
        }

        // Database special modes
        if self.screen == Screen::Database {
            match self.db_mode {
                DbMode::EditCell      => { self.handle_db_edit_cell_key(key); return; }
                DbMode::SwitchDatabase => { self.handle_db_switch_key(key); return; }
                DbMode::ViewBody      => { self.handle_db_body_key(key); return; }
                DbMode::FilterPresets => { self.handle_db_preset_key(key); return; }
                _ => {}
            }
        }

        // Automation onboarding form
        if self.auto_onboard_form.is_some() {
            self.handle_onboard_editing(key);
            return;
        }

        // Automation rule form
        if self.auto_form.is_some() {
            self.handle_auto_form_editing(key);
            return;
        }

        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.m_search_mode = false; self.f_search_mode = false;
                self.a_filter_mode = false; self.p_search_mode = false;
                self.db_search_mode = false;
            }
            KeyCode::Tab => {
                if let Some(form) = &mut self.m_form {
                    form.active = (form.active + 1) % form.fields.len().max(1);
                } else if let Some(form) = &mut self.f_form {
                    form.active = (form.active + 1) % 3;
                } else if let Some(form) = &mut self.s_form {
                    form.active = (form.active + 1) % 6;
                } else if self.screen == Screen::Discord {
                    // Section-aware Tab
                    let max = match self.d_section {
                        0 => 2, 2 => 2, 3 => 2, 4 => 2, _ => 1,
                    };
                    self.d_active_field = (self.d_active_field + 1) % max;
                } else if self.screen == Screen::Payments {
                    self.p_active_field = (self.p_active_field + 1) % 3;
                }
            }
            KeyCode::BackTab => {
                if let Some(form) = &mut self.m_form {
                    if form.active > 0 { form.active -= 1; }
                } else if let Some(form) = &mut self.f_form {
                    if form.active > 0 { form.active -= 1; }
                } else if let Some(form) = &mut self.s_form {
                    if form.active > 0 { form.active -= 1; }
                }
            }
            KeyCode::Char(c) => self.editing_char(c),
            KeyCode::Backspace => self.editing_backspace(),
            KeyCode::Enter => { self.input_mode = InputMode::Normal; }
            _ => {}
        }
    }

    fn handle_auto_form_editing(&mut self, key: KeyEvent) {
        let n_fields = AutoRuleForm::field_count();
        match key.code {
            KeyCode::Esc => { self.auto_form = None; self.input_mode = InputMode::Normal; }
            KeyCode::Tab => {
                if let Some(f) = &mut self.auto_form {
                    f.active = (f.active + 1) % n_fields;
                }
            }
            KeyCode::BackTab => {
                if let Some(f) = &mut self.auto_form {
                    if f.active > 0 { f.active -= 1; }
                }
            }
            KeyCode::Left => {
                if let Some(f) = &mut self.auto_form { f.cycle_left(f.active); }
            }
            KeyCode::Right => {
                if let Some(f) = &mut self.auto_form { f.cycle_left(f.active); }
            }
            KeyCode::Backspace => {
                if let Some(f) = &mut self.auto_form {
                    let ai = f.active;
                    if let Some(buf) = f.field_value_mut(ai) { buf.pop(); }
                }
            }
            // Save – must appear BEFORE the general Char(c) arm
            KeyCode::F(2) | KeyCode::Char('s') => {
                if let Some(f) = &self.auto_form {
                    let rule = f.to_rule();
                    let id = rule.id.clone();
                    match self.auto_rules.iter().position(|r| r.id == id) {
                        Some(i) => self.auto_rules[i] = rule,
                        None    => self.auto_rules.push(rule),
                    }
                    self.save_automation_rules();
                    self.status = "Rule saved.".into();
                }
                self.auto_form = None;
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char(c) => {
                if let Some(f) = &mut self.auto_form {
                    let ai = f.active;
                    if let Some(buf) = f.field_value_mut(ai) { buf.push(c); }
                }
            }
            _ => {}
        }
    }

    fn handle_onboard_editing(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => { self.auto_onboard_form = None; self.input_mode = InputMode::Normal; }
            KeyCode::Tab => {
                if let Some(f) = &mut self.auto_onboard_form {
                    f.active = (f.active + 1) % 3;
                }
            }
            KeyCode::Char(c) => {
                if let Some(f) = &mut self.auto_onboard_form {
                    match f.active {
                        0 => f.name.push(c),
                        1 => f.email.push(c),
                        _ => f.channel.push(c),
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(f) = &mut self.auto_onboard_form {
                    match f.active {
                        0 => { f.name.pop(); }
                        1 => { f.email.pop(); }
                        _ => { f.channel.pop(); }
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(f) = &self.auto_onboard_form {
                    let (name, email, ch) = (f.name.clone(), f.email.clone(), f.channel.clone());
                    self.auto_onboard_form = None;
                    self.input_mode = InputMode::Normal;
                    self.trigger_onboarding(name, email, ch);
                }
            }
            _ => {}
        }
    }

    fn active_buf(&mut self) -> Option<&mut String> {
        if self.m_search_mode { return Some(&mut self.m_search); }
        if self.f_search_mode { return Some(&mut self.f_search); }
        if self.a_filter_mode { return Some(&mut self.a_filter); }
        if self.p_search_mode { return Some(&mut self.p_search); }
        if self.db_search_mode { return Some(&mut self.db_search); }
        if self.gs_open { return Some(&mut self.gs_input); }
        if let Some(form) = &mut self.m_form {
            return form.fields.get_mut(form.active).map(|f| &mut f.value);
        }
        if let Some(form) = &mut self.f_form {
            return match form.active {
                0 => Some(&mut form.title), 1 => Some(&mut form.content), _ => Some(&mut form.tags),
            };
        }
        if let Some(form) = &mut self.s_form {
            let idx = form.active;
            return form.field_value_mut(idx);
        }
        match self.screen {
            Screen::Discord => match self.d_section {
                0 => Some(&mut self.d_channel),
                2 => Some(&mut self.d_action_input),
                3 => Some(&mut self.d_assign_user),
                4 => Some(&mut self.d_broadcast_msg),
                _ => None,
            },
            Screen::Payments => match self.p_active_field {
                0 => Some(&mut self.p_search),
                1 => Some(&mut self.p_status_val),
                _ => Some(&mut self.p_notes),
            },
            _ => None,
        }
    }

    fn editing_char(&mut self, c: char) {
        if let Some(form) = &mut self.m_form {
            let f = &mut form.fields[form.active];
            if matches!(f.kind.as_str(), "select" | "multi_select" | "status") && !f.options.is_empty() {
                return;
            }
        }
        if let Some(buf) = self.active_buf() { buf.push(c); }
        if self.f_search_mode {
            let q = self.f_search.clone();
            self.f_filtered = self.faq.filtered(&q);
            self.f_sel = 0;
            if !self.f_filtered.is_empty() { self.f_list.select(Some(0)); }
            else { self.f_list.select(None); }
        }
    }

    fn editing_backspace(&mut self) {
        if let Some(buf) = self.active_buf() { buf.pop(); }
        if self.f_search_mode {
            let q = self.f_search.clone();
            self.f_filtered = self.faq.filtered(&q);
            self.f_sel = 0;
            if !self.f_filtered.is_empty() { self.f_list.select(Some(0)); }
            else { self.f_list.select(None); }
        }
    }

    // ── Global search key handler ─────────────────────────────────────────────

    fn handle_global_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => { self.gs_open = false; self.input_mode = InputMode::Normal; }
            KeyCode::Char(c) => {
                self.gs_input.push(c);
                self.gs_results.clear(); self.gs_sel = 0;
                self.trigger_global_search();
            }
            KeyCode::Backspace => {
                self.gs_input.pop();
                if self.gs_input.is_empty() { self.gs_results.clear(); self.gs_loading = false; }
                else { self.gs_results.clear(); self.trigger_global_search(); }
            }
            KeyCode::Down => {
                if self.gs_sel + 1 < self.gs_results.len() { self.gs_sel += 1; }
            }
            KeyCode::Up => { if self.gs_sel > 0 { self.gs_sel -= 1; } }
            KeyCode::Enter => {
                // Navigate to the result's source screen
                if let Some(r) = self.gs_results.get(self.gs_sel).cloned() {
                    self.gs_open = false; self.input_mode = InputMode::Normal;
                    match r.source {
                        SearchSource::Notion   => { self.screen = Screen::Members; }
                        SearchSource::Discord  => { self.screen = Screen::Discord; }
                        SearchSource::Faq      => { self.screen = Screen::Faq; }
                        SearchSource::Activity => { self.screen = Screen::Activity; }
                    }
                    self.status = format!("Jumped to {} ({})", r.title, r.id);
                }
            }
            _ => {}
        }
    }

    fn handle_palette_key(&mut self, key: KeyEvent) {
        // Delegated to handle_editing since show_palette is checked there
        self.handle_editing(key);
    }

    // ── Database cell edit keys ───────────────────────────────────────────────

    fn handle_db_edit_cell_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Right => self.db_cycle_edit_value(key.code == KeyCode::Right),
            KeyCode::Enter => self.submit_db_cell_edit(),
            KeyCode::Char(c) => {
                let kind = self.db_edit_kind().unwrap_or_default();
                if !matches!(kind.as_str(), "select" | "status" | "multi_select" | "checkbox") {
                    self.db_edit_buf.push(c);
                }
            }
            KeyCode::Backspace => {
                let kind = self.db_edit_kind().unwrap_or_default();
                if !matches!(kind.as_str(), "select" | "status" | "checkbox") {
                    self.db_edit_buf.pop();
                }
            }
            _ => {}
        }
    }

    fn db_cycle_edit_value(&mut self, forward: bool) {
        let kind = self.db_edit_kind().unwrap_or_default();
        if kind == "checkbox" {
            self.db_edit_buf = if self.db_edit_buf == "true" || self.db_edit_buf == "✓" {
                "false".into()
            } else { "true".into() };
            return;
        }
        let opts = self.db_edit_options();
        if opts.is_empty() { return; }
        if forward {
            self.db_edit_opt_idx = (self.db_edit_opt_idx + 1) % opts.len();
        } else if self.db_edit_opt_idx > 0 {
            self.db_edit_opt_idx -= 1;
        } else {
            self.db_edit_opt_idx = opts.len() - 1;
        }
        self.db_edit_buf = opts[self.db_edit_opt_idx].clone();
    }

    fn submit_db_cell_edit(&mut self) {
        // Scope the immutable borrow so we can mutate undo_stack afterwards.
        let col_name = {
            let cols = self.db_visible_columns();
            cols.get(self.db_col_sel).map(|c| c.property_name.clone())
        };
        let Some(col_name) = col_name else {
            self.db_mode = DbMode::Browse;
            self.input_mode = InputMode::Normal;
            return;
        };

        if let Some(page) = self.db_rows.get(self.db_row_sel).cloned() {
            // Push undo before updating (borrow of undo_stack is now safe).
            let old_props = page.props.clone();
            if self.undo_stack.len() >= 20 { self.undo_stack.remove(0); }
            self.undo_stack.push(UndoAction::UpdatePage {
                page_id: page.id.clone(), old_props,
            });

            let kind  = self.db_edit_kind().unwrap_or_else(|| "rich_text".into());
            let value = self.db_edit_buf.clone();
            let mut props = HashMap::new();
            props.insert(col_name.clone(), build_prop(&kind, &value));
            if let Some(n) = self.notion.clone() {
                let tx = self.tx.clone(); let pid = page.id.clone();
                tokio::spawn(async move {
                    match n.update_page(&pid, props).await {
                        Ok(p)  => { let _ = tx.send(AppEvent::DbRowUpdated(p)); }
                        Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                    }
                });
                let _ = self.logger.write("db_cell_update", &page.id,
                    &format!("{}={value}", col_name), true);
            }
        }
        self.db_mode = DbMode::Browse;
        self.input_mode = InputMode::Normal;
    }

    fn handle_db_switch_key(&mut self, key: KeyEvent) {
        let filtered = self.db_filtered_indices();
        match key.code {
            KeyCode::Down => { if self.db_picker_sel + 1 < filtered.len() { self.db_picker_sel += 1; } }
            KeyCode::Up   => { if self.db_picker_sel > 0 { self.db_picker_sel -= 1; } }
            KeyCode::Enter => {
                if let Some(&idx) = filtered.get(self.db_picker_sel) {
                    if let Some(db) = self.db_available.get(idx).cloned() {
                        self.switch_database(db.id, db.title);
                    }
                }
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char(c) => { self.db_picker_input.push(c); self.db_picker_sel = 0; }
            KeyCode::Backspace => { self.db_picker_input.pop(); self.db_picker_sel = 0; }
            _ => {}
        }
    }

    fn handle_db_body_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => { self.db_mode = DbMode::Browse; self.input_mode = InputMode::Normal; }
            KeyCode::Char(c) => { self.db_body_append_buf.push(c); }
            KeyCode::Backspace => { self.db_body_append_buf.pop(); }
            KeyCode::Enter => {
                let content = self.db_body_append_buf.clone();
                if !content.is_empty() {
                    if let Some(page) = self.db_rows.get(self.db_row_sel).cloned() {
                        if let Some(n) = self.notion.clone() {
                            let tx = self.tx.clone(); let pid = page.id.clone();
                            tokio::spawn(async move {
                                match n.append_page_body(&pid, &content).await {
                                    Ok(_)  => { let _ = tx.send(AppEvent::StatusMsg("Body updated.".into())); }
                                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                                }
                            });
                            self.db_body_append_buf.clear();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_db_preset_key(&mut self, key: KeyEvent) {
        let n = self.db_filter_presets.len();
        match key.code {
            KeyCode::Esc => { self.db_mode = DbMode::Browse; self.input_mode = InputMode::Normal; }
            KeyCode::Down => { if self.db_preset_sel + 1 < n { self.db_preset_sel += 1; } }
            KeyCode::Up   => { if self.db_preset_sel > 0 { self.db_preset_sel -= 1; } }
            KeyCode::Enter => {
                if let Some(preset) = self.db_filter_presets.get(self.db_preset_sel).cloned() {
                    let filter = preset.to_notion_filter();
                    self.db_mode = DbMode::Browse;
                    self.input_mode = InputMode::Normal;
                    self.trigger_db_load_rows_filtered(filter);
                    self.status = format!("Filter applied: {}", preset.name);
                }
            }
            _ => {}
        }
    }

    // ── Members screen keys ───────────────────────────────────────────────────

    async fn key_members(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if self.m_sel + 1 < self.m_pages.len() { self.m_sel += 1; self.m_list.select(Some(self.m_sel)); }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.m_sel > 0 { self.m_sel -= 1; self.m_list.select(Some(self.m_sel)); }
            }
            KeyCode::Char('r') | KeyCode::F(5) => self.trigger_load_members(),
            KeyCode::Char('/') => { self.m_search_mode = true; self.input_mode = InputMode::Editing; }
            KeyCode::Enter if self.m_search_mode => {
                self.m_search_mode = false; self.input_mode = InputMode::Normal;
                let q = self.m_search.clone();
                if let Some(n) = self.notion.clone() {
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        match n.search_title(&q).await {
                            Ok(p)  => { let _ = tx.send(AppEvent::MembersLoaded(p)); }
                            Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                        }
                    });
                }
            }
            KeyCode::Char('a') => {
                if let Some(schema) = &self.m_schema {
                    self.m_form = Some(MemberForm::for_schema(schema));
                    self.input_mode = InputMode::Editing;
                } else { self.status = "Schema not loaded. Press r.".into(); }
            }
            KeyCode::Char('e') => {
                if let (Some(page), Some(schema)) = (self.m_pages.get(self.m_sel).cloned(), &self.m_schema) {
                    self.m_form = Some(MemberForm::for_edit(&page, schema));
                    self.input_mode = InputMode::Editing;
                }
            }
            KeyCode::Char('d') => {
                if let Some(page) = self.m_pages.get(self.m_sel).cloned() {
                    if let Some(n) = self.notion.clone() {
                        let tx = self.tx.clone(); self.m_loading = true;
                        tokio::spawn(async move {
                            match n.archive_page(&page.id).await {
                                Ok(_)  => { let _ = tx.send(AppEvent::MemberRemoved); }
                                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                            }
                        });
                    }
                }
            }
            KeyCode::Char('u') => {
                if let Some(page) = self.m_pages.get(self.m_sel).cloned() {
                    if let Some(n) = self.notion.clone() {
                        let tx = self.tx.clone();
                        tokio::spawn(async move {
                            match n.unarchive_page(&page.id).await {
                                Ok(_)  => { let _ = tx.send(AppEvent::StatusMsg("Unarchived.".into())); }
                                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                            }
                        });
                    }
                }
            }
            KeyCode::Enter if self.m_form.is_some() => {
                if let Some(form) = &self.m_form {
                    let props = form.to_props(); let is_edit = form.is_edit; let pid = form.page_id.clone();
                    if let Some(n) = self.notion.clone() {
                        let tx = self.tx.clone(); self.m_loading = true;
                        tokio::spawn(async move {
                            if is_edit {
                                match n.update_page(&pid.unwrap(), props).await {
                                    Ok(p)  => { let _ = tx.send(AppEvent::MemberUpdated(p)); }
                                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                                }
                            } else {
                                match n.create_page(props).await {
                                    Ok(p)  => { let _ = tx.send(AppEvent::MemberCreated(p)); }
                                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                                }
                            }
                        });
                    }
                }
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Right | KeyCode::Left => {
                if let Some(form) = &mut self.m_form {
                    let f = &mut form.fields[form.active];
                    if !f.options.is_empty() {
                        if key.code == KeyCode::Right {
                            f.opt_idx = (f.opt_idx + 1) % f.options.len();
                        } else if f.opt_idx > 0 {
                            f.opt_idx -= 1;
                        } else {
                            f.opt_idx = f.options.len() - 1;
                        }
                        f.value = f.options[f.opt_idx].clone();
                    }
                }
            }
            _ => {}
        }
    }

    // ── Discord screen keys ───────────────────────────────────────────────────

    async fn key_discord(&mut self, key: KeyEvent) {
        let max_sections = 5; // 0=invite 1=members 2=kick 3=roles 4=broadcast
        match key.code {
            KeyCode::Tab => {
                self.d_section = (self.d_section + 1) % max_sections;
                match self.d_section {
                    1 => self.trigger_discord_members(),
                    3 => self.trigger_discord_roles(),
                    _ => {}
                }
            }
            // Invite section
            KeyCode::Char('i') if self.d_section == 0 => self.trigger_discord_invite(),
            KeyCode::Char('c') if self.d_section == 0 => {
                if !self.d_invite_result.is_empty() {
                    if crate::faq::copy_to_clipboard(&self.d_invite_result) {
                        self.status = "Invite URL copied!".into();
                    }
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') if self.d_section == 0 => {
                self.d_hours = (self.d_hours + 1).min(168);
            }
            KeyCode::Char('-') if self.d_section == 0 => {
                if self.d_hours > 1 { self.d_hours -= 1; }
            }
            // Members section
            KeyCode::Down | KeyCode::Char('j') if self.d_section == 1 => {
                if self.d_sel + 1 < self.d_discord_members.len() {
                    self.d_sel += 1; self.d_members_list.select(Some(self.d_sel));
                }
            }
            KeyCode::Up | KeyCode::Char('k') if self.d_section == 1 => {
                if self.d_sel > 0 { self.d_sel -= 1; self.d_members_list.select(Some(self.d_sel)); }
            }
            // Kick/ban section
            KeyCode::Char('k') if self.d_section == 2 => {
                if let Some(d) = self.discord.clone() {
                    let (uid, reason) = (self.d_action_input.clone(), self.d_reason.clone());
                    let tx = self.tx.clone(); let uid2 = uid.clone();
                    tokio::spawn(async move {
                        let r = d.kick(&uid2, &reason).await;
                        let _ = tx.send(match r {
                            Ok(_)  => AppEvent::StatusMsg(format!("Kicked {uid2}")),
                            Err(e) => AppEvent::ErrMsg(e.to_string()),
                        });
                    });
                    let _ = self.logger.write("discord_kick", &uid, &self.d_reason, true);
                }
            }
            KeyCode::Char('b') if self.d_section == 2 => {
                if let Some(d) = self.discord.clone() {
                    let (uid, reason) = (self.d_action_input.clone(), self.d_reason.clone());
                    let tx = self.tx.clone(); let uid2 = uid.clone();
                    tokio::spawn(async move {
                        let r = d.ban(&uid2, &reason).await;
                        let _ = tx.send(match r {
                            Ok(_)  => AppEvent::StatusMsg(format!("Banned {uid2}")),
                            Err(e) => AppEvent::ErrMsg(e.to_string()),
                        });
                    });
                    let _ = self.logger.write("discord_ban", &uid, &self.d_reason, true);
                }
            }
            // Roles section
            KeyCode::Down | KeyCode::Char('j') if self.d_section == 3 => {
                if self.d_role_sel + 1 < self.d_roles.len() {
                    self.d_role_sel += 1; self.d_roles_list.select(Some(self.d_role_sel));
                }
            }
            KeyCode::Up | KeyCode::Char('k') if self.d_section == 3 => {
                if self.d_role_sel > 0 { self.d_role_sel -= 1; self.d_roles_list.select(Some(self.d_role_sel)); }
            }
            KeyCode::Char('a') if self.d_section == 3 => {
                // Assign selected role to user
                if let Some(d) = self.discord.clone() {
                    if let Some(role) = self.d_roles.get(self.d_role_sel).cloned() {
                        let uid   = self.d_assign_user.clone();
                        let uid2  = uid.clone(); // keep for logger after move
                        let rid   = role.id.clone();
                        let rname = role.name.clone();
                        let tx    = self.tx.clone();
                        tokio::spawn(async move {
                            let r = d.assign_role(&uid, &rid).await;
                            let _ = tx.send(match r {
                                Ok(_)  => AppEvent::StatusMsg(format!("Role '{rname}' assigned to {uid}")),
                                Err(e) => AppEvent::ErrMsg(e.to_string()),
                            });
                        });
                        let _ = self.logger.write("discord_role_add", &uid2, &role.name, true);
                    }
                }
            }
            KeyCode::Char('x') if self.d_section == 3 => {
                // Remove selected role from user
                if let Some(d) = self.discord.clone() {
                    if let Some(role) = self.d_roles.get(self.d_role_sel).cloned() {
                        let uid   = self.d_assign_user.clone();
                        let uid2  = uid.clone(); // keep for logger after move
                        let rid   = role.id.clone();
                        let rname = role.name.clone();
                        let tx    = self.tx.clone();
                        tokio::spawn(async move {
                            let r = d.remove_role(&uid, &rid).await;
                            let _ = tx.send(match r {
                                Ok(_)  => AppEvent::StatusMsg(format!("Role '{rname}' removed from {uid}")),
                                Err(e) => AppEvent::ErrMsg(e.to_string()),
                            });
                        });
                        let _ = self.logger.write("discord_role_remove", &uid2, &role.name, true);
                    }
                }
            }
            // Broadcast section
            KeyCode::Char('m') if self.d_section == 4 => {
                // Send broadcast message
                if let Some(d) = self.discord.clone() {
                    let ch = self.d_broadcast_channel.clone();
                    let msg = self.d_broadcast_msg.clone();
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        let r = d.broadcast_message(&ch, &msg).await;
                        let _ = tx.send(match r {
                            Ok(_)  => AppEvent::StatusMsg("Message sent!".into()),
                            Err(e) => AppEvent::ErrMsg(e.to_string()),
                        });
                    });
                    let _ = self.logger.write("discord_broadcast", &self.d_broadcast_channel, &self.d_broadcast_msg, true);
                }
            }
            // Audit log: press A to fetch (no separate section, shown in broadcast panel)
            KeyCode::Char('A') => self.trigger_discord_audit(),
            // Edit / search in any section
            KeyCode::Char('e') => { self.input_mode = InputMode::Editing; self.d_active_field = 0; }
            KeyCode::Char('/') => { self.d_active_field = 2; self.input_mode = InputMode::Editing; }
            // DM from members section
            KeyCode::Char('d') if self.d_section == 1 => {
                if let Some(member) = self.d_discord_members.get(self.d_sel).cloned() {
                    if let Some(d) = self.discord.clone() {
                        let uid = member.user_id.clone();
                        let msg = self.d_dm_msg.clone();
                        let tx = self.tx.clone(); let uid2 = uid.clone();
                        tokio::spawn(async move {
                            let r = d.send_dm(&uid2, &msg).await;
                            let _ = tx.send(match r {
                                Ok(_)  => AppEvent::StatusMsg(format!("DM sent to {uid2}")),
                                Err(e) => AppEvent::ErrMsg(e.to_string()),
                            });
                        });
                    }
                }
            }
            _ => {}
        }
    }

    // ── FAQ screen keys ───────────────────────────────────────────────────────

    fn key_faq(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if self.f_sel + 1 < self.f_filtered.len() { self.f_sel += 1; self.f_list.select(Some(self.f_sel)); }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.f_sel > 0 { self.f_sel -= 1; self.f_list.select(Some(self.f_sel)); }
            }
            KeyCode::Char('/') => { self.f_search_mode = true; self.input_mode = InputMode::Editing; }
            KeyCode::Char('c') | KeyCode::Enter => {
                if let Some(&idx) = self.f_filtered.get(self.f_sel) {
                    if let Some(s) = self.faq.snippets.get(idx) {
                        self.f_copied = crate::faq::copy_to_clipboard(&s.content);
                        self.status = if self.f_copied {
                            format!("Copied '{}' to clipboard!", s.title)
                        } else { "Clipboard unavailable – see preview.".into() };
                        let _ = self.logger.write("faq_copy", &s.title, "copied", self.f_copied);
                    }
                }
            }
            KeyCode::Char('p') => self.f_show_preview = !self.f_show_preview,
            KeyCode::Char('a') => {
                self.f_form = Some(SnippetForm { id: None, title: String::new(), content: String::new(), tags: String::new(), active: 0 });
                self.input_mode = InputMode::Editing;
            }
            KeyCode::Char('e') => {
                if let Some(&idx) = self.f_filtered.get(self.f_sel) {
                    if let Some(s) = self.faq.snippets.get(idx) {
                        self.f_form = Some(SnippetForm {
                            id: Some(s.id.clone()), title: s.title.clone(),
                            content: s.content.clone(), tags: s.tags.join(", "), active: 0,
                        });
                        self.input_mode = InputMode::Editing;
                    }
                }
            }
            KeyCode::F(2) | KeyCode::Char('s') if self.f_form.is_some() => {
                if let Some(form) = &self.f_form {
                    let tags: Vec<String> = form.tags.split(',')
                        .map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
                    let r = if let Some(id) = &form.id {
                        self.faq.update(id, form.title.clone(), form.content.clone(), tags)
                    } else {
                        self.faq.add(form.title.clone(), form.content.clone(), tags)
                    };
                    match r {
                        Ok(_) => {
                            self.status = "Snippet saved.".into(); self.f_form = None;
                            let q = self.f_search.clone();
                            self.f_filtered = self.faq.filtered(&q);
                        }
                        Err(e) => self.status = format!("Save error: {e}"),
                    }
                    self.input_mode = InputMode::Normal;
                }
            }
            KeyCode::Char('D') => {
                if let Some(&idx) = self.f_filtered.get(self.f_sel) {
                    if let Some(s) = self.faq.snippets.get(idx) {
                        let id = s.id.clone(); let _ = self.faq.delete(&id);
                        let q = self.f_search.clone();
                        self.f_filtered = self.faq.filtered(&q);
                        if self.f_sel > 0 { self.f_sel -= 1; }
                        self.f_list.select(Some(self.f_sel));
                        self.status = "Snippet deleted.".into();
                    }
                }
            }
            _ => {}
        }
    }

    // ── Payments screen keys ──────────────────────────────────────────────────

    async fn key_payments(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('/') | KeyCode::Char('s') => {
                self.p_search_mode = true; self.p_active_field = 0; self.input_mode = InputMode::Editing;
            }
            KeyCode::Enter if self.p_active_field == 0 => {
                let q = self.p_search.clone();
                if let Some(n) = self.notion.clone() {
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        match n.search_title(&q).await {
                            Ok(p)  => { let _ = tx.send(AppEvent::MembersLoaded(p)); }
                            Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                        }
                    });
                }
                self.input_mode = InputMode::Normal; self.p_search_mode = false;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.p_sel + 1 < self.p_results.len() { self.p_sel += 1; self.p_res_list.select(Some(self.p_sel)); }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.p_sel > 0 { self.p_sel -= 1; self.p_res_list.select(Some(self.p_sel)); }
            }
            KeyCode::Char('v') => {
                if let Some(page) = self.m_pages.get(self.p_sel).cloned() {
                    if let Some(schema) = &self.m_schema {
                        let pay_field = schema.props.iter().find(|p| {
                            let n = p.name.to_lowercase();
                            n.contains("pay") || n.contains("paid") || n.contains("status")
                        });
                        if let Some(field) = pay_field {
                            let mut props = HashMap::new();
                            let val = if field.kind == "checkbox" { "true" } else { self.p_status_val.as_str() };
                            props.insert(field.name.clone(), build_prop(&field.kind, val));
                            if let Some(n) = self.notion.clone() {
                                let pid = page.id.clone(); let tx = self.tx.clone();
                                let fname = field.name.clone();
                                tokio::spawn(async move {
                                    match n.update_page(&pid, props).await {
                                        Ok(p)  => { let _ = tx.send(AppEvent::MemberUpdated(p)); let _ = tx.send(AppEvent::StatusMsg(format!("Payment verified for {pid}"))); }
                                        Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                                    }
                                });
                                let _ = self.logger.write("payment_check", &page.id, &format!("{fname}={val}"), true);
                            }
                        } else {
                            self.status = "No payment field in schema. Edit manually (2 → e).".into();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // ── Activity screen keys ──────────────────────────────────────────────────

    fn key_activity(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if self.a_sel + 1 < self.a_logs.len() { self.a_sel += 1; self.a_list.select(Some(self.a_sel)); }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.a_sel > 0 { self.a_sel -= 1; self.a_list.select(Some(self.a_sel)); }
            }
            KeyCode::Char('r') | KeyCode::F(5) => self.trigger_load_activity(),
            KeyCode::Char('/') => { self.a_filter_mode = true; self.input_mode = InputMode::Editing; }
            KeyCode::Enter if self.a_filter_mode => {
                self.a_filter_mode = false; self.input_mode = InputMode::Normal;
                let q = self.a_filter.clone();
                match if q.is_empty() { self.logger.recent(200) } else { self.logger.search(&q) } {
                    Ok(logs) => { self.a_sel = 0; if !logs.is_empty() { self.a_list.select(Some(0)); } self.a_logs = logs; }
                    Err(e) => self.status = format!("Log error: {e}"),
                }
            }
            _ => {}
        }
    }

    // ── Settings screen keys ──────────────────────────────────────────────────

    fn key_settings(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if self.s_sel + 1 < self.profiles.profiles.len() { self.s_sel += 1; self.s_list.select(Some(self.s_sel)); }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.s_sel > 0 { self.s_sel -= 1; self.s_list.select(Some(self.s_sel)); }
            }
            KeyCode::Char('a') => { self.s_form = Some(ProfileForm::new()); self.input_mode = InputMode::Editing; }
            KeyCode::Char('e') => {
                if let Some(p) = self.profiles.profiles.get(self.s_sel) {
                    self.s_form = Some(ProfileForm::from_profile(p)); self.input_mode = InputMode::Editing;
                }
            }
            KeyCode::Char('d') => {
                let _ = self.profiles.delete(self.s_sel);
                if self.s_sel > 0 { self.s_sel -= 1; }
                self.s_list.select(Some(self.s_sel));
                self.apply_active_profile();
                self.status = "Profile deleted.".into();
            }
            // Cycle theme for active profile
            KeyCode::Char('t') => {
                if let Some(p) = self.profiles.active_mut() {
                    p.theme = p.theme.cycle();
                    self.status = format!("Theme: {}", p.theme.label());
                }
                let _ = self.profiles.save();
            }
            KeyCode::Enter if self.s_form.is_none() => {
                let _ = self.profiles.set_active(self.s_sel);
                self.apply_active_profile();
                self.status = format!("Active: {}", self.profiles.active().map(|p| p.name.as_str()).unwrap_or("none"));
            }
            KeyCode::F(2) | KeyCode::Char('s') if self.s_form.is_some() => {
                if let Some(form) = &self.s_form {
                    let existing_db_tab = form.original_name.as_ref()
                        .and_then(|n| self.profiles.profiles.iter().find(|p| &p.name == n))
                        .map(|p| p.database_tab.clone())
                        .unwrap_or_default();
                    let existing_theme = form.original_name.as_ref()
                        .and_then(|n| self.profiles.profiles.iter().find(|p| &p.name == n))
                        .map(|p| p.theme.clone())
                        .unwrap_or_default();
                    let existing_rules = form.original_name.as_ref()
                        .and_then(|n| self.profiles.profiles.iter().find(|p| &p.name == n))
                        .map(|p| p.automation_rules.clone())
                        .unwrap_or_default();
                    let p = crate::config::Profile {
                        name: form.name.clone(), notion_api_key: form.notion_key.clone(),
                        notion_database_id: form.notion_db.clone(),
                        discord_bot_token: form.discord_token.clone(),
                        discord_guild_id: form.discord_guild.clone(),
                        discord_default_channel_id: form.discord_channel.clone(),
                        extra: Default::default(),
                        theme: existing_theme, database_tab: existing_db_tab,
                        automation_rules: existing_rules,
                    };
                    match self.profiles.upsert(p) {
                        Ok(_) => { self.status = "Profile saved.".into(); self.s_form = None; self.apply_active_profile(); }
                        Err(e) => self.status = format!("Save error: {e}"),
                    }
                }
                self.input_mode = InputMode::Normal;
            }
            _ => {}
        }
    }

    // ── Database screen keys ──────────────────────────────────────────────────

    fn key_database(&mut self, key: KeyEvent) {
        match self.db_mode {
            DbMode::Browse           => self.key_database_browse(key),
            DbMode::ConfigureColumns => self.key_database_configure(key),
            _ => {}
        }
    }

    fn key_database_browse(&mut self, key: KeyEvent) {
        let visible_count = self.db_visible_columns().len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if self.db_row_sel + 1 < self.db_rows.len() { self.db_row_sel += 1; self.db_table.select(Some(self.db_row_sel)); }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.db_row_sel > 0 { self.db_row_sel -= 1; self.db_table.select(Some(self.db_row_sel)); }
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                if visible_count > 0 { self.db_col_sel = (self.db_col_sel + 1) % visible_count; }
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
                if visible_count > 0 {
                    self.db_col_sel = if self.db_col_sel == 0 { visible_count - 1 } else { self.db_col_sel - 1 };
                }
            }
            KeyCode::Char('r') | KeyCode::F(5) => self.trigger_db_load_rows(),
            KeyCode::Char('c') => {
                if self.db_schema.is_some() { self.db_cfg_sel = 0; self.db_mode = DbMode::ConfigureColumns; }
                else { self.status = "Schema not loaded. Press r.".into(); }
            }
            KeyCode::Char('D') => {
                self.db_mode = DbMode::SwitchDatabase;
                self.db_picker_input.clear(); self.db_picker_sel = 0;
                self.input_mode = InputMode::Editing;
                self.trigger_db_list_databases();
            }
            KeyCode::Char('/') => { self.db_search_mode = true; self.input_mode = InputMode::Editing; }
            KeyCode::Enter if self.db_search_mode => {
                self.db_search_mode = false; self.input_mode = InputMode::Normal;
                let q = self.db_search.clone();
                if let (Some(n), Some(db_id)) = (self.notion.clone(), self.db_database_id.clone()) {
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        match n.search_title_of(&db_id, &q).await {
                            Ok(p)  => { let _ = tx.send(AppEvent::DbRowsLoaded(p)); }
                            Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                        }
                    });
                }
            }
            // Open cell editor
            KeyCode::Char('e') | KeyCode::Enter => {
                let cols = self.db_visible_columns();
                let Some(col) = cols.get(self.db_col_sel) else { return; };
                if !col.editable {
                    self.status = "Column is read-only (toggle with c → x).".into(); return;
                }
                let property_name = col.property_name.clone();
                if let Some(page) = self.db_rows.get(self.db_row_sel).cloned() {
                    let current = page.display(&property_name);
                    self.db_edit_buf = current.clone();
                    let opts = self.db_edit_options();
                    self.db_edit_opt_idx = opts.iter().position(|o| o == &current).unwrap_or(0);
                    self.db_mode = DbMode::EditCell; self.input_mode = InputMode::Editing;
                }
            }
            // Open page body viewer (Track C)
            KeyCode::Char('b') => {
                self.db_page_body.clear(); self.db_body_append_buf.clear();
                self.db_mode = DbMode::ViewBody; self.input_mode = InputMode::Editing;
                self.trigger_db_page_body();
            }
            // Filter presets (Track C)
            KeyCode::Char('f') => {
                if self.db_filter_presets.is_empty() {
                    self.status = "No filter presets saved. Save presets via :auto-filter (coming soon).".into();
                } else {
                    self.db_preset_sel = 0;
                    self.db_mode = DbMode::FilterPresets;
                    self.input_mode = InputMode::Editing;
                }
            }
            // Bulk select toggle (Track C)
            KeyCode::Char(' ') => {
                self.db_bulk_mode = true;
                if self.db_bulk_sel.contains(&self.db_row_sel) {
                    self.db_bulk_sel.remove(&self.db_row_sel);
                } else {
                    self.db_bulk_sel.insert(self.db_row_sel);
                }
                self.status = format!("{} rows selected", self.db_bulk_sel.len());
            }
            // Bulk archive
            KeyCode::Char('X') if self.db_bulk_mode && !self.db_bulk_sel.is_empty() => {
                let ids: Vec<String> = self.db_bulk_sel.iter()
                    .filter_map(|&i| self.db_rows.get(i).map(|p| p.id.clone()))
                    .collect();
                if let Some(n) = self.notion.clone() {
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        for id in &ids { let _ = n.archive_page(id).await; }
                        let _ = tx.send(AppEvent::StatusMsg(format!("Archived {} rows", ids.len())));
                    });
                }
                self.db_bulk_sel.clear(); self.db_bulk_mode = false;
            }
            KeyCode::Char('s') => self.save_db_config(),
            _ => {}
        }
    }

    fn key_database_configure(&mut self, key: KeyEvent) {
        let n = self.db_columns.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => { if self.db_cfg_sel + 1 < n { self.db_cfg_sel += 1; } }
            KeyCode::Up | KeyCode::Char('k')   => { if self.db_cfg_sel > 0 { self.db_cfg_sel -= 1; } }
            KeyCode::Char(' ') => { if let Some(c) = self.db_columns.get_mut(self.db_cfg_sel) { c.visible = !c.visible; } }
            KeyCode::Char('x') => { if let Some(c) = self.db_columns.get_mut(self.db_cfg_sel) { c.editable = !c.editable; } }
            KeyCode::Char('J') => {
                if self.db_cfg_sel + 1 < n { self.db_columns.swap(self.db_cfg_sel, self.db_cfg_sel + 1); self.db_cfg_sel += 1; }
            }
            KeyCode::Char('K') => {
                if self.db_cfg_sel > 0 { self.db_columns.swap(self.db_cfg_sel, self.db_cfg_sel - 1); self.db_cfg_sel -= 1; }
            }
            KeyCode::Char('s') => self.save_db_config(),
            KeyCode::Enter | KeyCode::Esc => { self.db_mode = DbMode::Browse; self.db_col_sel = 0; }
            _ => {}
        }
    }

    // ── Analytics screen keys (Track D) ──────────────────────────────────────

    fn key_analytics(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('r') | KeyCode::F(5) => self.trigger_analytics(),
            KeyCode::Char('e') => self.trigger_export_csv(),
            _ => {}
        }
    }

    // ── Automation screen keys (Track F) ─────────────────────────────────────

    async fn key_automation(&mut self, key: KeyEvent) {
        let n = self.auto_rules.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if self.auto_sel + 1 < n { self.auto_sel += 1; self.auto_list.select(Some(self.auto_sel)); }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.auto_sel > 0 { self.auto_sel -= 1; self.auto_list.select(Some(self.auto_sel)); }
            }
            KeyCode::Char('a') => {
                self.auto_form = Some(AutoRuleForm::new());
                self.input_mode = InputMode::Editing;
            }
            KeyCode::Char('e') => {
                if let Some(rule) = self.auto_rules.get(self.auto_sel).cloned() {
                    self.auto_form = Some(AutoRuleForm::from_rule(&rule));
                    self.input_mode = InputMode::Editing;
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if self.auto_sel < self.auto_rules.len() {
                    self.auto_rules.remove(self.auto_sel);
                    if self.auto_sel > 0 { self.auto_sel -= 1; }
                    self.auto_list.select(Some(self.auto_sel));
                    self.save_automation_rules();
                    self.status = "Rule deleted.".into();
                }
            }
            KeyCode::Enter => {
                if let Some(rule) = self.auto_rules.get(self.auto_sel).cloned() {
                    self.trigger_run_rule(rule);
                }
            }
            // Run all
            KeyCode::Char('R') => self.trigger_run_all_rules(),
            // Toggle enabled
            KeyCode::Char(' ') => {
                if let Some(rule) = self.auto_rules.get_mut(self.auto_sel) {
                    rule.enabled = !rule.enabled;
                    self.save_automation_rules();
                }
            }
            // Open onboarding form
            KeyCode::Char('o') => {
                self.auto_onboard_form = Some(OnboardForm {
                    name: String::new(), email: String::new(), channel: String::new(), active: 0,
                });
                self.input_mode = InputMode::Editing;
            }
            _ => {}
        }
    }
}
