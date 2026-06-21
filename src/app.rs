use std::collections::HashMap;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use ratatui::widgets::{ListState, TableState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    config::{ColumnConfig, ProfileManager},
    discord::DiscordClient,
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode { Normal, Editing }

/// Sub-mode within the Database screen.
#[derive(Debug, Clone, PartialEq)]
pub enum DbMode {
    Browse,
    ConfigureColumns,
    SwitchDatabase,
    EditCell,
}

// ── Async events (API → TUI) ──────────────────────────────────────────────────

pub enum AppEvent {
    SchemaLoaded(Schema),
    MembersLoaded(Vec<Page>),
    MemberCreated(Page),
    MemberUpdated(Page),
    MemberRemoved,
    DiscordInvite(String),          // url
    DiscordMembersLoaded(Vec<crate::discord::Member>),
    ActivityLoaded(Vec<LogEntry>),
    StatusMsg(String),
    ErrMsg(String),
    // ── Database tab ──────────────────────────────────────────────────────
    DbSchemaLoaded(Schema),
    DbRowsLoaded(Vec<Page>),
    DbDatabasesListed(Vec<DbRef>),
    DbRowUpdated(Page),
}

// ── Per-screen state structs ──────────────────────────────────────────────────

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
            key: p.name.clone(),
            label: p.name.clone(),
            value: String::new(),
            kind: p.kind.clone(),
            options: p.options.clone(),
            opt_idx: 0,
        }).collect();
        MemberForm { is_edit: false, page_id: None, fields, active: 0 }
    }

    pub fn for_edit(page: &Page, schema: &Schema) -> Self {
        let fields = schema.editable().map(|p| {
            let value = page.display(&p.name);
            let opt_idx = p.options.iter().position(|o| o == &value).unwrap_or(0);
            FormField {
                key: p.name.clone(), label: p.name.clone(),
                value, kind: p.kind.clone(), options: p.options.clone(), opt_idx,
            }
        }).collect();
        MemberForm { is_edit: true, page_id: Some(page.id.clone()), fields, active: 0 }
    }

    pub fn to_props(&self) -> HashMap<String, serde_json::Value> {
        self.fields.iter().map(|f| {
            let val = if f.kind == "select" || f.kind == "multi_select" || f.kind == "status" {
                f.options.get(f.opt_idx).cloned().unwrap_or_else(|| f.value.clone())
            } else {
                f.value.clone()
            };
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
            is_edit: false, original_name: None,
            name: String::new(), notion_key: String::new(), notion_db: String::new(),
            discord_token: String::new(), discord_guild: String::new(), discord_channel: String::new(),
            active: 0,
        }
    }
    pub fn from_profile(p: &crate::config::Profile) -> Self {
        ProfileForm {
            is_edit: true, original_name: Some(p.name.clone()),
            name: p.name.clone(), notion_key: p.notion_api_key.clone(),
            notion_db: p.notion_database_id.clone(), discord_token: p.discord_bot_token.clone(),
            discord_guild: p.discord_guild_id.clone(), discord_channel: p.discord_default_channel_id.clone(),
            active: 0,
        }
    }
    pub fn field_labels() -> &'static [&'static str] {
        &["Profile name","Notion API key","Notion database ID","Discord bot token","Discord guild ID","Discord default channel ID"]
    }
    pub fn field_values(&self) -> Vec<&str> {
        vec![&self.name,&self.notion_key,&self.notion_db,&self.discord_token,&self.discord_guild,&self.discord_channel]
    }
    pub fn field_value_mut(&mut self, i: usize) -> Option<&mut String> {
        match i {
            0 => Some(&mut self.name), 1 => Some(&mut self.notion_key), 2 => Some(&mut self.notion_db),
            3 => Some(&mut self.discord_token), 4 => Some(&mut self.discord_guild), 5 => Some(&mut self.discord_channel),
            _ => None,
        }
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    // Core
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
    pub d_action_input: String,   // user_id for kick/ban
    pub d_reason: String,
    pub d_active_field: usize,
    pub d_section: usize,         // 0=invite 1=members 2=kick

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

    // ── Database screen (generic, configurable) ─────────────────────────────
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
    // column configurator overlay
    pub db_cfg_sel: usize,
    // database switcher overlay
    pub db_picker_input: String,
    pub db_picker_sel: usize,
    pub db_available: Vec<DbRef>,
    // cell editor overlay
    pub db_edit_buf: String,
    pub db_edit_opt_idx: usize,
}

impl App {
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let faq = FaqManager::load()?;
        let logger = ActivityLogger::open()?;
        let profiles = ProfileManager::load()?;

        let mut app = App {
            screen: Screen::Dashboard,
            input_mode: InputMode::Normal,
            status: "Ready – press ? for help".into(),
            should_quit: false,
            tx, rx,
            notion: None, discord: None,
            faq, logger, profiles,
            // members
            m_pages: vec![], m_schema: None,
            m_list: TableState::default(), m_sel: 0,
            m_search: String::new(), m_search_mode: false,
            m_form: None, m_loading: false,
            // discord
            d_channel: String::new(), d_hours: 24, d_uses: 1,
            d_invite_result: String::new(), d_user_query: String::new(),
            d_discord_members: vec![], d_members_list: ListState::default(),
            d_sel: 0, d_action_input: String::new(), d_reason: String::new(),
            d_active_field: 0, d_section: 0,
            // faq
            f_filtered: vec![], f_list: ListState::default(), f_sel: 0,
            f_search: String::new(), f_search_mode: false,
            f_form: None, f_copied: false, f_show_preview: false,
            // payments
            p_search: String::new(), p_search_mode: false,
            p_results: vec![], p_res_list: TableState::default(), p_sel: 0,
            p_status_val: String::new(), p_notes: String::new(), p_active_field: 0,
            // activity
            a_logs: vec![], a_list: ListState::default(), a_sel: 0,
            a_filter: String::new(), a_filter_mode: false,
            // settings
            s_list: ListState::default(), s_sel: 0, s_form: None,
            // database tab
            db_database_id: None, db_db_name: String::new(),
            db_schema: None, db_columns: vec![], db_rows: vec![],
            db_table: TableState::default(), db_row_sel: 0, db_col_sel: 0,
            db_mode: DbMode::Browse, db_loading: false,
            db_search: String::new(), db_search_mode: false,
            db_cfg_sel: 0,
            db_picker_input: String::new(), db_picker_sel: 0, db_available: vec![],
            db_edit_buf: String::new(), db_edit_opt_idx: 0,
        };

        // Try to load clients from active profile
        app.apply_active_profile();
        // Refresh FAQ filter
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
        }
    }

    // ── Async event processing ────────────────────────────────────────────────

    pub fn drain_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            self.process(ev);
        }
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
                self.m_pages = pages;
                self.m_loading = false;
                let _ = self.logger.write("notion_query", "database", "members loaded", true);
            }
            AppEvent::MemberCreated(p) => {
                self.status = format!("Added: {}", p.id);
                self.m_form = None;
                self.m_loading = false;
                let _ = self.logger.write("notion_add", &p.id, "page created", true);
                self.trigger_load_members();
            }
            AppEvent::MemberUpdated(p) => {
                self.status = format!("Updated: {}", p.id);
                self.m_form = None;
                self.m_loading = false;
                let _ = self.logger.write("notion_update", &p.id, "page updated", true);
                self.trigger_load_members();
            }
            AppEvent::MemberRemoved => {
                self.status = "Member removed".into();
                self.m_loading = false;
                let _ = self.logger.write("notion_remove", "page", "archived", true);
                self.trigger_load_members();
            }
            AppEvent::DiscordInvite(url) => {
                self.d_invite_result = url.clone();
                self.status = format!("Invite created: {url}");
                let _ = self.logger.write("discord_invite", &url, "", true);
                crate::faq::copy_to_clipboard(&url);
            }
            AppEvent::DiscordMembersLoaded(members) => {
                self.status = format!("Discord: {} members", members.len());
                self.d_sel = 0;
                if !members.is_empty() { self.d_members_list.select(Some(0)); }
                self.d_discord_members = members;
            }
            AppEvent::ActivityLoaded(logs) => {
                self.a_sel = 0;
                if !logs.is_empty() { self.a_list.select(Some(0)); }
                self.a_logs = logs;
            }
            AppEvent::StatusMsg(s) => self.status = s,
            AppEvent::ErrMsg(e) => {
                self.status = format!("Error: {e}");
                self.m_loading = false;
                self.db_loading = false;
            }
            AppEvent::DbSchemaLoaded(s) => {
                // First time we see this database: build a default column
                // layout (everything visible, in schema order). If a layout
                // was already loaded from profiles.toml, keep it but append
                // any schema properties it doesn't know about yet, so newly
                // added Notion properties show up without losing the saved
                // order/visibility/editable choices for existing ones.
                if self.db_columns.is_empty() {
                    self.db_columns = s.props.iter().map(|p| ColumnConfig {
                        property_name: p.name.clone(),
                        visible: true,
                        editable: p.editable,
                    }).collect();
                } else {
                    for p in &s.props {
                        if !self.db_columns.iter().any(|c| c.property_name == p.name) {
                            self.db_columns.push(ColumnConfig {
                                property_name: p.name.clone(),
                                visible: true,
                                editable: p.editable,
                            });
                        }
                    }
                }
                self.status = format!("Database schema: {} properties", s.props.len());
                self.db_schema = Some(s);
            }
            AppEvent::DbRowsLoaded(rows) => {
                self.status = format!("Loaded {} rows", rows.len());
                self.db_row_sel = 0;
                if !rows.is_empty() { self.db_table.select(Some(0)); } else { self.db_table.select(None); }
                self.db_rows = rows;
                self.db_loading = false;
            }
            AppEvent::DbDatabasesListed(dbs) => {
                self.status = format!("Found {} databases", dbs.len());
                self.db_picker_sel = 0;
                self.db_available = dbs;
            }
            AppEvent::DbRowUpdated(p) => {
                self.status = format!("Row updated: {}", p.id);
                self.trigger_db_load_rows();
            }
        }
    }

    // ── Spawn helpers ─────────────────────────────────────────────────────────

    fn trigger_load_members(&mut self) {
        if let Some(n) = self.notion.clone() {
            self.m_loading = true;
            let tx = self.tx.clone();
            tokio::spawn(async move {
                match n.query(None).await {
                    Ok(p) => { let _ = tx.send(AppEvent::MembersLoaded(p)); }
                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                }
            });
        } else {
            self.status = "No Notion profile active. Go to Settings (7).".into();
        }
    }

    fn trigger_load_schema(&mut self) {
        if let Some(n) = self.notion.clone() {
            let tx = self.tx.clone();
            tokio::spawn(async move {
                match n.fetch_schema().await {
                    Ok(s) => { let _ = tx.send(AppEvent::SchemaLoaded(s)); }
                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                }
            });
        }
    }

    fn trigger_load_activity(&mut self) {
        match self.logger.recent(200) {
            Ok(logs) => { let _ = self.tx.send(AppEvent::ActivityLoaded(logs)); }
            Err(e) => self.status = format!("Log error: {e}"),
        }
    }

    fn trigger_discord_invite(&mut self) {
        if let Some(d) = self.discord.clone() {
            let ch = self.d_channel.clone();
            let h = self.d_hours;
            let u = self.d_uses;
            let tx = self.tx.clone();
            tokio::spawn(async move {
                match d.create_invite(&ch, h, u).await {
                    Ok(inv) => { let _ = tx.send(AppEvent::DiscordInvite(inv.url)); }
                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                }
            });
        } else {
            self.status = "No Discord profile. Configure in Settings (7).".into();
        }
    }

    fn trigger_discord_members(&mut self) {
        if let Some(d) = self.discord.clone() {
            let tx = self.tx.clone();
            tokio::spawn(async move {
                match d.get_members(100).await {
                    Ok(m) => { let _ = tx.send(AppEvent::DiscordMembersLoaded(m)); }
                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                }
            });
        }
    }

    // ── Database tab spawn helpers ────────────────────────────────────────────

    fn trigger_db_load_schema(&mut self) {
        if let (Some(n), Some(db_id)) = (self.notion.clone(), self.db_database_id.clone()) {
            let tx = self.tx.clone();
            tokio::spawn(async move {
                match n.fetch_schema_of(&db_id).await {
                    Ok(s) => { let _ = tx.send(AppEvent::DbSchemaLoaded(s)); }
                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                }
            });
        } else {
            self.status = "No Notion profile active. Go to Settings (7).".into();
        }
    }

    fn trigger_db_load_rows(&mut self) {
        if let (Some(n), Some(db_id)) = (self.notion.clone(), self.db_database_id.clone()) {
            self.db_loading = true;
            let tx = self.tx.clone();
            tokio::spawn(async move {
                match n.query_of(&db_id, None).await {
                    Ok(p) => { let _ = tx.send(AppEvent::DbRowsLoaded(p)); }
                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                }
            });
        }
    }

    fn trigger_db_list_databases(&mut self) {
        if let Some(n) = self.notion.clone() {
            let tx = self.tx.clone();
            tokio::spawn(async move {
                match n.list_databases().await {
                    Ok(dbs) => { let _ = tx.send(AppEvent::DbDatabasesListed(dbs)); }
                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                }
            });
        } else {
            self.status = "No Notion profile active. Go to Settings (7).".into();
        }
    }

    /// Default the Database tab to the profile's saved active database (or
    /// the Members database if none was saved yet), and load that
    /// database's saved column layout. Idempotent — only runs once until
    /// `switch_database` clears `db_database_id` again.
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

    /// Point the Database tab at a different database: saves the current
    /// column layout to the still-active profile first (so it isn't lost),
    /// then resets and reloads for the new database.
    fn switch_database(&mut self, id: String, title: String) {
        self.save_db_config();
        self.db_database_id = Some(id.clone());
        self.db_db_name = title;
        self.db_schema = None;
        self.db_rows = vec![];
        self.db_row_sel = 0;
        self.db_col_sel = 0;
        self.db_table = TableState::default();
        self.db_columns = self.profiles.active()
            .and_then(|p| p.database_tab.columns.get(&id).cloned())
            .unwrap_or_default();
        self.db_mode = DbMode::Browse;
        self.trigger_db_load_schema();
        self.trigger_db_load_rows();
    }

    /// Persist the current column layout (visibility/order/editable) for the
    /// active database into the active profile's profiles.toml.
    fn save_db_config(&mut self) {
        let Some(db_id) = self.db_database_id.clone() else { return };
        if let Some(profile) = self.profiles.profiles.get_mut(self.profiles.active_idx) {
            profile.database_tab.active_database_id = Some(db_id.clone());
            profile.database_tab.columns.insert(db_id, self.db_columns.clone());
        } else {
            return;
        }
        match self.profiles.save() {
            Ok(_) => self.status = "Column layout saved.".into(),
            Err(e) => self.status = format!("Save error: {e}"),
        }
    }

    // ── Database tab helpers (used by both app.rs and ui/screens.rs) ─────────

    pub fn db_visible_columns(&self) -> Vec<&ColumnConfig> {
        self.db_columns.iter().filter(|c| c.visible).collect()
    }

    /// Notion property kind ("select", "checkbox", "rich_text", …) for the
    /// currently selected visible column, looked up from the loaded schema.
    pub fn db_edit_kind(&self) -> Option<String> {
        let cols = self.db_visible_columns();
        let col = cols.get(self.db_col_sel)?;
        self.db_schema.as_ref()?.props.iter()
            .find(|p| p.name == col.property_name)
            .map(|p| p.kind.clone())
    }

    /// Select/status options for the currently selected visible column.
    pub fn db_edit_options(&self) -> Vec<String> {
        let cols = self.db_visible_columns();
        cols.get(self.db_col_sel)
            .and_then(|col| self.db_schema.as_ref()?.props.iter().find(|p| p.name == col.property_name))
            .map(|p| p.options.clone())
            .unwrap_or_default()
    }

    /// Indices into `db_available` matching the current picker filter text.
    pub fn db_filtered_indices(&self) -> Vec<usize> {
        let q = self.db_picker_input.to_lowercase();
        self.db_available.iter().enumerate()
            .filter(|(_, d)| q.is_empty() || d.title.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    // ── Key handling ──────────────────────────────────────────────────────────

    /// Returns true → quit.
    pub async fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Close any active form with Esc
        if key.code == KeyCode::Esc {
            if self.m_form.is_some() { self.m_form = None; self.input_mode = InputMode::Normal; return false; }
            if self.f_form.is_some() { self.f_form = None; self.input_mode = InputMode::Normal; return false; }
            if self.s_form.is_some() { self.s_form = None; self.input_mode = InputMode::Normal; return false; }
            if self.db_mode != DbMode::Browse {
                self.db_mode = DbMode::Browse;
                self.input_mode = InputMode::Normal;
                return false;
            }
            if self.input_mode == InputMode::Editing {
                self.input_mode = InputMode::Normal;
                self.m_search_mode = false;
                self.f_search_mode = false;
                self.a_filter_mode = false;
                self.p_search_mode = false;
                self.db_search_mode = false;
                return false;
            }
        }

        // In editing mode, route to the active screen's text handler
        if self.input_mode == InputMode::Editing {
            self.handle_editing(key);
            return false;
        }

        // Global screen switches (1-7)
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
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
                    return true;
                }
            }
            _ => {}
        }

        // Per-screen keys
        match self.screen.clone() {
            Screen::Members  => self.key_members(key).await,
            Screen::Discord  => self.key_discord(key).await,
            Screen::Faq      => self.key_faq(key),
            Screen::Payments => self.key_payments(key).await,
            Screen::Activity => self.key_activity(key),
            Screen::Settings => self.key_settings(key),
            Screen::Database => self.key_database(key),
            Screen::Dashboard => {}
        }
        false
    }

    // ── Editing mode (text input routing) ─────────────────────────────────────

    fn handle_editing(&mut self, key: KeyEvent) {
        // The Database tab's EditCell and SwitchDatabase modes need
        // immediate Left/Right/Up/Down handling (live select-cycling, live
        // list navigation while typing a filter) rather than the generic
        // text-field router below, which only reacts to Tab/Char/Backspace/
        // Enter and defers everything else to the per-screen key handler —
        // a handler that, by construction, is never reached while
        // `input_mode == Editing` (see `handle_key` above).
        if self.screen == Screen::Database {
            match self.db_mode {
                DbMode::EditCell => { self.handle_db_edit_cell_key(key); return; }
                DbMode::SwitchDatabase => { self.handle_db_switch_key(key); return; }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.m_search_mode = false;
                self.f_search_mode = false;
                self.a_filter_mode = false;
                self.p_search_mode = false;
            }
            KeyCode::Tab => {
                // advance active field inside forms
                if let Some(form) = &mut self.m_form {
                    form.active = (form.active + 1) % form.fields.len().max(1);
                } else if let Some(form) = &mut self.f_form {
                    form.active = (form.active + 1) % 3;
                } else if let Some(form) = &mut self.s_form {
                    form.active = (form.active + 1) % 6;
                } else if self.screen == Screen::Discord {
                    self.d_active_field = (self.d_active_field + 1) % 5;
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
                } else if self.screen == Screen::Discord {
                    if self.d_active_field > 0 { self.d_active_field -= 1; }
                } else if self.screen == Screen::Payments {
                    if self.p_active_field > 0 { self.p_active_field -= 1; }
                }
            }
            KeyCode::Char(c) => self.editing_char(c),
            KeyCode::Backspace => self.editing_backspace(),
            KeyCode::Enter => {
                // Confirm form or search
                self.input_mode = InputMode::Normal;
                // screens handle the submit separately via their own Enter logic
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
        if let Some(form) = &mut self.m_form {
            return form.fields.get_mut(form.active).map(|f| &mut f.value);
        }
        if let Some(form) = &mut self.f_form {
            return match form.active {
                0 => Some(&mut form.title),
                1 => Some(&mut form.content),
                _ => Some(&mut form.tags),
            };
        }
        if let Some(form) = &mut self.s_form {
            let idx = form.active;
            return form.field_value_mut(idx);
        }
        match self.screen {
            Screen::Discord => match self.d_active_field {
                0 => Some(&mut self.d_channel),
                2 => Some(&mut self.d_user_query),
                3 => Some(&mut self.d_action_input),
                4 => Some(&mut self.d_reason),
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
        // For select fields in member form, cycle options instead of typing
        if let Some(form) = &mut self.m_form {
            let f = &mut form.fields[form.active];
            if (f.kind == "select" || f.kind == "multi_select" || f.kind == "status") && !f.options.is_empty() {
                return; // handled by left/right
            }
        }
        if let Some(buf) = self.active_buf() { buf.push(c); }
        // Live-update FAQ filter
        if self.f_search_mode {
            let q = self.f_search.clone();
            self.f_filtered = self.faq.filtered(&q);
            self.f_sel = 0;
            if !self.f_filtered.is_empty() { self.f_list.select(Some(0)); } else { self.f_list.select(None); }
        }
    }

    fn editing_backspace(&mut self) {
        if let Some(buf) = self.active_buf() { buf.pop(); }
        if self.f_search_mode {
            let q = self.f_search.clone();
            self.f_filtered = self.faq.filtered(&q);
            self.f_sel = 0;
            if !self.f_filtered.is_empty() { self.f_list.select(Some(0)); } else { self.f_list.select(None); }
        }
    }

    // ── Database tab: EditCell overlay key handling ───────────────────────────

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

    /// Left/Right inside the cell editor: toggles a checkbox, or cycles
    /// through select/status options — mirroring how the Members form's
    /// `←/→` field cycling is meant to behave.
    fn db_cycle_edit_value(&mut self, forward: bool) {
        let kind = self.db_edit_kind().unwrap_or_default();
        if kind == "checkbox" {
            self.db_edit_buf = if self.db_edit_buf == "true" || self.db_edit_buf == "✓" {
                "false".into()
            } else {
                "true".into()
            };
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
        let cols = self.db_visible_columns();
        if let Some(col) = cols.get(self.db_col_sel).cloned() {
            if let Some(page) = self.db_rows.get(self.db_row_sel).cloned() {
                let kind = self.db_edit_kind().unwrap_or_else(|| "rich_text".into());
                let value = self.db_edit_buf.clone();
                let mut props = HashMap::new();
                props.insert(col.property_name.clone(), build_prop(&kind, &value));
                if let Some(n) = self.notion.clone() {
                    let tx = self.tx.clone();
                    let pid = page.id.clone();
                    tokio::spawn(async move {
                        match n.update_page(&pid, props).await {
                            Ok(p) => { let _ = tx.send(AppEvent::DbRowUpdated(p)); }
                            Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                        }
                    });
                    let _ = self.logger.write("db_cell_update", &page.id, &format!("{}={value}", col.property_name), true);
                }
            }
        }
        self.db_mode = DbMode::Browse;
        self.input_mode = InputMode::Normal;
    }

    // ── Database tab: SwitchDatabase overlay key handling ─────────────────────

    fn handle_db_switch_key(&mut self, key: KeyEvent) {
        let filtered = self.db_filtered_indices();
        match key.code {
            KeyCode::Down => { if self.db_picker_sel + 1 < filtered.len() { self.db_picker_sel += 1; } }
            KeyCode::Up => { if self.db_picker_sel > 0 { self.db_picker_sel -= 1; } }
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

    // ── Members screen keys ───────────────────────────────────────────────────

    async fn key_members(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if self.m_sel + 1 < self.m_pages.len() {
                    self.m_sel += 1;
                    self.m_list.select(Some(self.m_sel));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.m_sel > 0 {
                    self.m_sel -= 1;
                    self.m_list.select(Some(self.m_sel));
                }
            }
            KeyCode::Char('r') | KeyCode::F(5) => self.trigger_load_members(),
            KeyCode::Char('/') => {
                self.m_search_mode = true;
                self.input_mode = InputMode::Editing;
            }
            KeyCode::Enter if self.m_search_mode => {
                self.m_search_mode = false;
                self.input_mode = InputMode::Normal;
                let q = self.m_search.clone();
                if let Some(n) = self.notion.clone() {
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        match n.search_title(&q).await {
                            Ok(p) => { let _ = tx.send(AppEvent::MembersLoaded(p)); }
                            Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                        }
                    });
                }
            }
            KeyCode::Char('a') => {
                if let Some(schema) = &self.m_schema {
                    self.m_form = Some(MemberForm::for_schema(schema));
                    self.input_mode = InputMode::Editing;
                } else {
                    self.status = "Schema not loaded yet. Press r to refresh.".into();
                }
            }
            KeyCode::Char('e') => {
                if let Some(page) = self.m_pages.get(self.m_sel).cloned() {
                    if let Some(schema) = &self.m_schema {
                        self.m_form = Some(MemberForm::for_edit(&page, schema));
                        self.input_mode = InputMode::Editing;
                    }
                }
            }
            KeyCode::Char('d') => {
                if let Some(page) = self.m_pages.get(self.m_sel).cloned() {
                    if let Some(n) = self.notion.clone() {
                        let tx = self.tx.clone();
                        self.m_loading = true;
                        tokio::spawn(async move {
                            match n.archive_page(&page.id).await {
                                Ok(_) => { let _ = tx.send(AppEvent::MemberRemoved); }
                                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                            }
                        });
                    }
                }
            }
            KeyCode::Char('u') => {
                // Unarchive / restore
                if let Some(page) = self.m_pages.get(self.m_sel).cloned() {
                    if let Some(n) = self.notion.clone() {
                        let tx = self.tx.clone();
                        tokio::spawn(async move {
                            match n.unarchive_page(&page.id).await {
                                Ok(_) => { let _ = tx.send(AppEvent::StatusMsg("Unarchived".into())); }
                                Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                            }
                        });
                    }
                }
            }
            KeyCode::Enter if self.m_form.is_some() => {
                if let Some(form) = &self.m_form {
                    let props = form.to_props();
                    let is_edit = form.is_edit;
                    let page_id = form.page_id.clone();
                    if let Some(n) = self.notion.clone() {
                        let tx = self.tx.clone();
                        self.m_loading = true;
                        tokio::spawn(async move {
                            if is_edit {
                                match n.update_page(&page_id.unwrap(), props).await {
                                    Ok(p) => { let _ = tx.send(AppEvent::MemberUpdated(p)); }
                                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                                }
                            } else {
                                match n.create_page(props).await {
                                    Ok(p) => { let _ = tx.send(AppEvent::MemberCreated(p)); }
                                    Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                                }
                            }
                        });
                    }
                }
                self.input_mode = InputMode::Normal;
            }
            // Cycle select options with Left/Right
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
        match key.code {
            KeyCode::Tab => {
                self.d_section = (self.d_section + 1) % 3;
                if self.d_section == 1 { self.trigger_discord_members(); }
            }
            KeyCode::Char('i') if self.d_section == 0 => self.trigger_discord_invite(),
            KeyCode::Char('c') if self.d_section == 0 => {
                if !self.d_invite_result.is_empty() {
                    if crate::faq::copy_to_clipboard(&self.d_invite_result) {
                        self.status = "Invite URL copied to clipboard!".into();
                    }
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') if self.d_section == 0 => {
                self.d_hours = (self.d_hours + 1).min(168);
            }
            KeyCode::Char('-') if self.d_section == 0 => {
                if self.d_hours > 1 { self.d_hours -= 1; }
            }
            KeyCode::Down | KeyCode::Char('j') if self.d_section == 1 => {
                if self.d_sel + 1 < self.d_discord_members.len() {
                    self.d_sel += 1;
                    self.d_members_list.select(Some(self.d_sel));
                }
            }
            KeyCode::Up | KeyCode::Char('k') if self.d_section == 1 => {
                if self.d_sel > 0 {
                    self.d_sel -= 1;
                    self.d_members_list.select(Some(self.d_sel));
                }
            }
            KeyCode::Char('/') => {
                self.d_active_field = 2;
                self.input_mode = InputMode::Editing;
            }
            KeyCode::Char('e') => {
                self.input_mode = InputMode::Editing;
                self.d_active_field = 0;
            }
            KeyCode::Char('k') if self.d_section == 2 => {
                if let Some(d) = self.discord.clone() {
                    let uid = self.d_action_input.clone();
                    let reason = self.d_reason.clone();
                    let tx = self.tx.clone();
                    let task_uid = uid.clone();
                    tokio::spawn(async move {
                        let r = d.kick(&task_uid, &reason).await;
                        let _ = tx.send(match r {
                            Ok(_) => AppEvent::StatusMsg(format!("Kicked {task_uid}")),
                            Err(e) => AppEvent::ErrMsg(e.to_string()),
                        });
                    });
                    let _ = self.logger.write("discord_kick", &uid, &self.d_reason, true);
                }
            }
            KeyCode::Char('b') if self.d_section == 2 => {
                if let Some(d) = self.discord.clone() {
                    let uid = self.d_action_input.clone();
                    let reason = self.d_reason.clone();
                    let tx = self.tx.clone();
                    let task_uid = uid.clone();
                    tokio::spawn(async move {
                        let r = d.ban(&task_uid, &reason).await;
                        let _ = tx.send(match r {
                            Ok(_) => AppEvent::StatusMsg(format!("Banned {task_uid}")),
                            Err(e) => AppEvent::ErrMsg(e.to_string()),
                        });
                    });
                    let _ = self.logger.write("discord_ban", &uid, &self.d_reason, true);
                }
            }
            _ => {}
        }
    }

    // ── FAQ screen keys ───────────────────────────────────────────────────────

    fn key_faq(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if self.f_sel + 1 < self.f_filtered.len() {
                    self.f_sel += 1;
                    self.f_list.select(Some(self.f_sel));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.f_sel > 0 {
                    self.f_sel -= 1;
                    self.f_list.select(Some(self.f_sel));
                }
            }
            KeyCode::Char('/') => {
                self.f_search_mode = true;
                self.input_mode = InputMode::Editing;
            }
            KeyCode::Char('c') | KeyCode::Enter => {
                // Copy selected snippet to clipboard
                if let Some(&idx) = self.f_filtered.get(self.f_sel) {
                    if let Some(s) = self.faq.snippets.get(idx) {
                        self.f_copied = crate::faq::copy_to_clipboard(&s.content);
                        self.status = if self.f_copied {
                            format!("Copied '{}' to clipboard!", s.title)
                        } else {
                            format!("Clipboard failed – xclip/xsel not found. Content shown in preview.")
                        };
                        let _ = self.logger.write("faq_copy", &s.title, "copied to clipboard", self.f_copied);
                    }
                }
            }
            KeyCode::Char('p') => self.f_show_preview = !self.f_show_preview,
            KeyCode::Char('a') => {
                self.f_form = Some(SnippetForm {
                    id: None,
                    title: String::new(), content: String::new(), tags: String::new(), active: 0,
                });
                self.input_mode = InputMode::Editing;
            }
            KeyCode::Char('e') => {
                if let Some(&idx) = self.f_filtered.get(self.f_sel) {
                    if let Some(s) = self.faq.snippets.get(idx) {
                        self.f_form = Some(SnippetForm {
                            id: Some(s.id.clone()),
                            title: s.title.clone(),
                            content: s.content.clone(),
                            tags: s.tags.join(", "),
                            active: 0,
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
                            self.status = "Snippet saved.".into();
                            self.f_form = None;
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
                        let id = s.id.clone();
                        let _ = self.faq.delete(&id);
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
                self.p_search_mode = true;
                self.p_active_field = 0;
                self.input_mode = InputMode::Editing;
            }
            KeyCode::Enter if self.p_active_field == 0 => {
                // Search Notion for member
                let q = self.p_search.clone();
                if let Some(n) = self.notion.clone() {
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        match n.search_title(&q).await {
                            Ok(p) => { let _ = tx.send(AppEvent::MembersLoaded(p)); }
                            Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                        }
                    });
                }
                self.input_mode = InputMode::Normal;
                self.p_search_mode = false;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.p_sel + 1 < self.p_results.len() {
                    self.p_sel += 1;
                    self.p_res_list.select(Some(self.p_sel));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.p_sel > 0 {
                    self.p_sel -= 1;
                    self.p_res_list.select(Some(self.p_sel));
                }
            }
            KeyCode::Char('v') => {
                // Mark payment as verified – update a checkbox/select on the selected member
                if let Some(page) = self.m_pages.get(self.p_sel).cloned() {
                    if let Some(schema) = &self.m_schema {
                        // Find a "payment" or "paid" field in the schema
                        let pay_field = schema.props.iter().find(|p| {
                            let n = p.name.to_lowercase();
                            n.contains("pay") || n.contains("paid") || n.contains("status")
                        });
                        if let Some(field) = pay_field {
                            let mut props = HashMap::new();
                            let val = if field.kind == "checkbox" { "true" } else { self.p_status_val.as_str() };
                            props.insert(field.name.clone(), build_prop(&field.kind, val));
                            if let Some(n) = self.notion.clone() {
                                let pid = page.id.clone();
                                let tx = self.tx.clone();
                                let fname = field.name.clone();
                                tokio::spawn(async move {
                                    match n.update_page(&pid, props).await {
                                        Ok(p) => {
                                            let _ = tx.send(AppEvent::MemberUpdated(p));
                                            let _ = tx.send(AppEvent::StatusMsg(format!("Payment verified for {pid}")));
                                        }
                                        Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                                    }
                                });
                                let _ = self.logger.write("payment_check", &page.id, &format!("{fname}={val} notes={}", self.p_notes), true);
                            }
                        } else {
                            self.status = "No payment field found in schema. Edit member manually (2 → e).".into();
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
                if self.a_sel + 1 < self.a_logs.len() {
                    self.a_sel += 1;
                    self.a_list.select(Some(self.a_sel));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.a_sel > 0 {
                    self.a_sel -= 1;
                    self.a_list.select(Some(self.a_sel));
                }
            }
            KeyCode::Char('r') | KeyCode::F(5) => self.trigger_load_activity(),
            KeyCode::Char('/') => {
                self.a_filter_mode = true;
                self.input_mode = InputMode::Editing;
            }
            KeyCode::Enter if self.a_filter_mode => {
                self.a_filter_mode = false;
                self.input_mode = InputMode::Normal;
                let q = self.a_filter.clone();
                match if q.is_empty() { self.logger.recent(200) } else { self.logger.search(&q) } {
                    Ok(logs) => {
                        self.a_sel = 0;
                        if !logs.is_empty() { self.a_list.select(Some(0)); }
                        self.a_logs = logs;
                    }
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
                if self.s_sel + 1 < self.profiles.profiles.len() {
                    self.s_sel += 1;
                    self.s_list.select(Some(self.s_sel));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.s_sel > 0 {
                    self.s_sel -= 1;
                    self.s_list.select(Some(self.s_sel));
                }
            }
            KeyCode::Char('a') => {
                self.s_form = Some(ProfileForm::new());
                self.input_mode = InputMode::Editing;
            }
            KeyCode::Char('e') => {
                if let Some(p) = self.profiles.profiles.get(self.s_sel) {
                    self.s_form = Some(ProfileForm::from_profile(p));
                    self.input_mode = InputMode::Editing;
                }
            }
            KeyCode::Char('d') => {
                let _ = self.profiles.delete(self.s_sel);
                if self.s_sel > 0 { self.s_sel -= 1; }
                self.s_list.select(Some(self.s_sel));
                self.apply_active_profile();
                self.status = "Profile deleted.".into();
            }
            KeyCode::Enter if self.s_form.is_none() => {
                let _ = self.profiles.set_active(self.s_sel);
                self.apply_active_profile();
                self.status = format!("Active profile: {}", self.profiles.active().map(|p| p.name.as_str()).unwrap_or("none"));
            }
            KeyCode::F(2) | KeyCode::Char('s') if self.s_form.is_some() => {
                if let Some(form) = &self.s_form {
                    // Carry over the existing database_tab config (column
                    // layouts, active database) when editing a profile —
                    // ProfileForm doesn't surface it, so without this an
                    // edit-and-save from Settings would silently wipe out
                    // anything configured on the Database tab (8).
                    let existing_db_tab = form.original_name.as_ref()
                        .and_then(|name| self.profiles.profiles.iter().find(|p| &p.name == name))
                        .map(|p| p.database_tab.clone())
                        .unwrap_or_default();
                    let p = crate::config::Profile {
                        name: form.name.clone(),
                        notion_api_key: form.notion_key.clone(),
                        notion_database_id: form.notion_db.clone(),
                        discord_bot_token: form.discord_token.clone(),
                        discord_guild_id: form.discord_guild.clone(),
                        discord_default_channel_id: form.discord_channel.clone(),
                        extra: Default::default(),
                        database_tab: existing_db_tab,
                    };
                    match self.profiles.upsert(p) {
                        Ok(_) => {
                            self.status = "Profile saved.".into();
                            self.s_form = None;
                            self.apply_active_profile();
                        }
                        Err(e) => self.status = format!("Save error: {e}"),
                    }
                }
                self.input_mode = InputMode::Normal;
            }
            _ => {}
        }
    }

    // ── Database screen keys ──────────────────────────────────────────────────
    // EditCell and SwitchDatabase sub-modes are handled in handle_editing()
    // (see comment there) since they run while input_mode == Editing.
    // Browse and ConfigureColumns run in input_mode == Normal and land here.

    fn key_database(&mut self, key: KeyEvent) {
        match self.db_mode {
            DbMode::Browse => self.key_database_browse(key),
            DbMode::ConfigureColumns => self.key_database_configure(key),
            DbMode::EditCell | DbMode::SwitchDatabase => {}
        }
    }

    fn key_database_browse(&mut self, key: KeyEvent) {
        let visible_count = self.db_visible_columns().len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if self.db_row_sel + 1 < self.db_rows.len() {
                    self.db_row_sel += 1;
                    self.db_table.select(Some(self.db_row_sel));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.db_row_sel > 0 {
                    self.db_row_sel -= 1;
                    self.db_table.select(Some(self.db_row_sel));
                }
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
                if self.db_schema.is_some() {
                    self.db_cfg_sel = 0;
                    self.db_mode = DbMode::ConfigureColumns;
                } else {
                    self.status = "Schema not loaded yet. Press r to refresh.".into();
                }
            }
            KeyCode::Char('D') => {
                self.db_mode = DbMode::SwitchDatabase;
                self.db_picker_input.clear();
                self.db_picker_sel = 0;
                self.input_mode = InputMode::Editing;
                self.trigger_db_list_databases();
            }
            KeyCode::Char('/') => {
                self.db_search_mode = true;
                self.input_mode = InputMode::Editing;
            }
            KeyCode::Enter if self.db_search_mode => {
                self.db_search_mode = false;
                self.input_mode = InputMode::Normal;
                let q = self.db_search.clone();
                if let (Some(n), Some(db_id)) = (self.notion.clone(), self.db_database_id.clone()) {
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        match n.search_title_of(&db_id, &q).await {
                            Ok(p) => { let _ = tx.send(AppEvent::DbRowsLoaded(p)); }
                            Err(e) => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
                        }
                    });
                }
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                let cols = self.db_visible_columns();
                let Some(col) = cols.get(self.db_col_sel) else { return };
                if !col.editable {
                    self.status = "Column is read-only (toggle editable with 'c' → x).".into();
                    return;
                }
                let property_name = col.property_name.clone();
                if let Some(page) = self.db_rows.get(self.db_row_sel).cloned() {
                    let current = page.display(&property_name);
                    self.db_edit_buf = current.clone();
                    let opts = self.db_edit_options();
                    self.db_edit_opt_idx = opts.iter().position(|o| o == &current).unwrap_or(0);
                    self.db_mode = DbMode::EditCell;
                    self.input_mode = InputMode::Editing;
                } else {
                    self.status = "No row selected.".into();
                }
            }
            KeyCode::Char('s') => self.save_db_config(),
            _ => {}
        }
    }

    fn key_database_configure(&mut self, key: KeyEvent) {
        let n = self.db_columns.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => { if self.db_cfg_sel + 1 < n { self.db_cfg_sel += 1; } }
            KeyCode::Up | KeyCode::Char('k') => { if self.db_cfg_sel > 0 { self.db_cfg_sel -= 1; } }
            KeyCode::Char(' ') => {
                if let Some(c) = self.db_columns.get_mut(self.db_cfg_sel) { c.visible = !c.visible; }
            }
            KeyCode::Char('x') => {
                if let Some(c) = self.db_columns.get_mut(self.db_cfg_sel) { c.editable = !c.editable; }
            }
            KeyCode::Char('J') => {
                if self.db_cfg_sel + 1 < n {
                    self.db_columns.swap(self.db_cfg_sel, self.db_cfg_sel + 1);
                    self.db_cfg_sel += 1;
                }
            }
            KeyCode::Char('K') => {
                if self.db_cfg_sel > 0 {
                    self.db_columns.swap(self.db_cfg_sel, self.db_cfg_sel - 1);
                    self.db_cfg_sel -= 1;
                }
            }
            KeyCode::Char('s') => self.save_db_config(),
            KeyCode::Enter | KeyCode::Esc => {
                self.db_mode = DbMode::Browse;
                self.db_col_sel = 0;
            }
            _ => {}
        }
    }
}
