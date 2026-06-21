use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use crate::error::{AppError, Result};

/// One named profile holds all credentials for a single community setup.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
    pub name: String,
    pub notion_api_key: String,
    pub notion_database_id: String,
    pub discord_bot_token: String,
    pub discord_guild_id: String,
    pub discord_default_channel_id: String,
    /// Arbitrary extra key=value pairs so power users can store anything.
    #[serde(default)]
    pub extra: HashMap<String, String>,
    /// Per-profile configuration for the generic "Database" tab.
    /// `#[serde(default)]` keeps existing profiles.toml files (saved before
    /// this field existed) loading correctly.
    #[serde(default)]
    pub database_tab: DatabaseTabConfig,
}

/// Persisted state for the Database tab: which database it's currently
/// pointed at, and the column layout chosen for each database it's ever
/// been pointed at (keyed by Notion database id).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DatabaseTabConfig {
    pub active_database_id: Option<String>,
    #[serde(default)]
    pub columns: HashMap<String, Vec<ColumnConfig>>,
}

/// One column's display/edit configuration in the Database tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnConfig {
    pub property_name: String,
    pub visible: bool,
    pub editable: bool,
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
        if !self.path.exists() {
            return Ok(());
        }
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

    pub fn upsert(&mut self, p: Profile) -> Result<()> {
        match self.profiles.iter().position(|x| x.name == p.name) {
            Some(i) => self.profiles[i] = p,
            None => self.profiles.push(p),
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
