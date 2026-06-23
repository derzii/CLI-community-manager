use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use crate::error::{AppError, Result};

// ── Theme ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum Theme {
    #[default]
    Dark,
    Solarized,
    Light,
}

impl Theme {
    pub fn cycle(&self) -> Theme {
        match self { Theme::Dark => Theme::Solarized, Theme::Solarized => Theme::Light, Theme::Light => Theme::Dark }
    }
    pub fn label(&self) -> &'static str {
        match self { Theme::Dark => "Dark", Theme::Solarized => "Solarized", Theme::Light => "Light" }
    }
}

// ── Automation rule types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RuleConditionSource { Notion, Discord }

impl RuleConditionSource {
    pub fn label(&self) -> &'static str {
        match self { Self::Notion => "Notion", Self::Discord => "Discord" }
    }
    pub fn cycle(&self) -> Self {
        match self { Self::Notion => Self::Discord, Self::Discord => Self::Notion }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RuleConditionOp { Equals, NotEquals, Contains }

impl RuleConditionOp {
    pub fn label(&self) -> &'static str {
        match self { Self::Equals => "=", Self::NotEquals => "≠", Self::Contains => "contains" }
    }
    pub fn cycle(&self) -> Self {
        match self {
            Self::Equals => Self::NotEquals,
            Self::NotEquals => Self::Contains,
            Self::Contains => Self::Equals,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleCondition {
    pub source: RuleConditionSource,
    pub field: String,
    pub op: RuleConditionOp,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RuleActionType {
    AddDiscordRole,
    RemoveDiscordRole,
    SetNotionField,
    SendDiscordMessage,
    LogActivity,
}

impl RuleActionType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::AddDiscordRole    => "Add Discord Role",
            Self::RemoveDiscordRole => "Remove Discord Role",
            Self::SetNotionField    => "Set Notion Field",
            Self::SendDiscordMessage => "Send Discord Message",
            Self::LogActivity       => "Log Activity",
        }
    }
    pub fn cycle(&self) -> Self {
        match self {
            Self::AddDiscordRole    => Self::RemoveDiscordRole,
            Self::RemoveDiscordRole => Self::SetNotionField,
            Self::SetNotionField    => Self::SendDiscordMessage,
            Self::SendDiscordMessage => Self::LogActivity,
            Self::LogActivity       => Self::AddDiscordRole,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleAction {
    pub action_type: RuleActionType,
    /// role_id / channel_id / notion field name
    pub target: String,
    /// role_id (unused for some types) / message text / field value
    pub value: String,
    /// Only for SetNotionField: the Notion property kind ("rich_text", "select", …)
    #[serde(default)]
    pub field_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub condition: RuleCondition,
    pub action: RuleAction,
    pub last_run: Option<String>,
    pub last_result: Option<String>,
}

impl AutomationRule {
    pub fn new(name: impl Into<String>) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        AutomationRule {
            id: format!("rule_{}", t.as_millis()),
            name: name.into(),
            enabled: true,
            condition: RuleCondition {
                source: RuleConditionSource::Notion,
                field: String::new(),
                op: RuleConditionOp::Equals,
                value: String::new(),
            },
            action: RuleAction {
                action_type: RuleActionType::AddDiscordRole,
                target: String::new(),
                value: String::new(),
                field_kind: String::new(),
            },
            last_run: None,
            last_result: None,
        }
    }
}

// ── Named filter preset ───────────────────────────────────────────────────────

/// A saved Notion filter the user can recall by name on the Database tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterPreset {
    pub name: String,
    pub field: String,
    pub op: String,    // "equals" | "contains" | "is_empty" | "is_not_empty"
    pub value: String,
    pub prop_type: String, // "title" | "rich_text" | "select" | "checkbox" | …
}

impl FilterPreset {
    /// Convert to a Notion filter JSON value.
    pub fn to_notion_filter(&self) -> serde_json::Value {
        use serde_json::json;
        match self.prop_type.as_str() {
            "checkbox" => json!({
                "property": self.field,
                "checkbox": { self.op.clone(): true }
            }),
            "select" => json!({
                "property": self.field,
                "select": { self.op.clone(): self.value }
            }),
            "status" => json!({
                "property": self.field,
                "status": { self.op.clone(): self.value }
            }),
            _ => json!({
                "property": self.field,
                self.prop_type.clone(): { self.op.clone(): self.value }
            }),
        }
    }
}

// ── Profile ───────────────────────────────────────────────────────────────────

/// One named profile holds all credentials + preferences for a single community.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
    pub name: String,
    pub notion_api_key: String,
    pub notion_database_id: String,
    pub discord_bot_token: String,
    pub discord_guild_id: String,
    pub discord_default_channel_id: String,
    #[serde(default)]
    pub extra: HashMap<String, String>,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub database_tab: DatabaseTabConfig,
    #[serde(default)]
    pub automation_rules: Vec<AutomationRule>,
}

impl Profile {
    pub fn new(name: impl Into<String>) -> Self {
        Profile { name: name.into(), ..Default::default() }
    }
    pub fn has_notion(&self) -> bool {
        !self.notion_api_key.is_empty() && !self.notion_database_id.is_empty()
    }
    pub fn has_discord(&self) -> bool {
        !self.discord_bot_token.is_empty() && !self.discord_guild_id.is_empty()
    }
}

// ── Database tab config ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DatabaseTabConfig {
    pub active_database_id: Option<String>,
    #[serde(default)]
    pub columns: HashMap<String, Vec<ColumnConfig>>,
    #[serde(default)]
    pub filter_presets: HashMap<String, Vec<FilterPreset>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnConfig {
    pub property_name: String,
    pub visible: bool,
    pub editable: bool,
}

// ── ProfileManager ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct RootConfig {
    active: Option<String>,
    profiles: Vec<Profile>,
}

pub struct ProfileManager {
    path: PathBuf,
    pub profiles: Vec<Profile>,
    pub active_idx: usize,
}

impl ProfileManager {
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        let mut m = ProfileManager { path, profiles: vec![], active_idx: 0 };
        m.reload()?;
        Ok(m)
    }

    fn config_path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .ok_or_else(|| AppError::Config("No config dir".into()))?
            .join("theblackroom");
        std::fs::create_dir_all(&dir)?;
        Ok(dir.join("profiles.toml"))
    }

    pub fn reload(&mut self) -> Result<()> {
        if !self.path.exists() { return Ok(()); }
        let raw = std::fs::read_to_string(&self.path)?;
        let cfg: RootConfig = toml::from_str(&raw).unwrap_or_default();
        self.active_idx = cfg.active.as_deref()
            .and_then(|n| cfg.profiles.iter().position(|p| p.name == n))
            .unwrap_or(0);
        self.profiles = cfg.profiles;
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        let active = self.profiles.get(self.active_idx).map(|p| p.name.clone());
        let cfg = RootConfig { active, profiles: self.profiles.clone() };
        let text = toml::to_string_pretty(&cfg)?;
        std::fs::write(&self.path, text)?;
        Ok(())
    }

    pub fn active(&self) -> Option<&Profile> {
        self.profiles.get(self.active_idx)
    }

    pub fn active_mut(&mut self) -> Option<&mut Profile> {
        self.profiles.get_mut(self.active_idx)
    }

    pub fn upsert(&mut self, p: Profile) -> Result<()> {
        match self.profiles.iter().position(|x| x.name == p.name) {
            Some(i) => self.profiles[i] = p,
            None    => self.profiles.push(p),
        }
        self.save()
    }

    pub fn delete(&mut self, idx: usize) -> Result<()> {
        if idx < self.profiles.len() {
            self.profiles.remove(idx);
            if self.active_idx >= self.profiles.len() && !self.profiles.is_empty() {
                self.active_idx = self.profiles.len() - 1;
            }
            self.save()?;
        }
        Ok(())
    }

    pub fn set_active(&mut self, idx: usize) -> Result<()> {
        if idx < self.profiles.len() {
            self.active_idx = idx;
            self.save()?;
        }
        Ok(())
    }
}
