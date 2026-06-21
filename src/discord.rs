use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::error::{AppError, Result};

const BASE: &str = "https://discord.com/api/v10";

#[derive(Clone)]
pub struct DiscordClient {
    http: Client,
    token: String,
    pub guild_id: String,
    pub default_channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    pub code: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct Member {
    pub user_id: String,
    pub username: String,
    pub display: String,
    pub joined_at: String,
}

impl DiscordClient {
    pub fn new(token: impl Into<String>, guild_id: impl Into<String>, channel: impl Into<String>) -> Self {
        DiscordClient {
            http: Client::new(),
            token: token.into(),
            guild_id: guild_id.into(),
            default_channel: channel.into(),
        }
    }

    fn auth(&self) -> String { format!("Bot {}", self.token) }

    async fn get(&self, path: &str) -> Result<Value> {
        let r = self.http.get(format!("{BASE}{path}"))
            .header("Authorization", self.auth())
            .send().await?;
        if !r.status().is_success() {
            return Err(AppError::Discord(r.text().await.unwrap_or_default()));
        }
        Ok(r.json().await?)
    }

    async fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        let r = self.http.post(format!("{BASE}{path}"))
            .header("Authorization", self.auth())
            .json(body).send().await?;
        if !r.status().is_success() {
            return Err(AppError::Discord(r.text().await.unwrap_or_default()));
        }
        Ok(r.json().await?)
    }

    // ── Invites ───────────────────────────────────────────────────────────────

    /// Create a single-use invite that expires after `max_age_hours` hours.
    /// Uses the default channel if `channel_id` is empty.
    pub async fn create_invite(&self, channel_id: &str, max_age_hours: u64, max_uses: u32) -> Result<Invite> {
        let ch = if channel_id.is_empty() { &self.default_channel } else { channel_id };
        let body = json!({
            "max_age": max_age_hours * 3600,
            "max_uses": max_uses,
            "unique": true
        });
        let v = self.post_json(&format!("/channels/{ch}/invites"), &body).await?;
        let code = v["code"].as_str().unwrap_or("").to_string();
        Ok(Invite { url: format!("https://discord.gg/{code}"), code })
    }

    // ── Members ───────────────────────────────────────────────────────────────

    pub async fn search_members(&self, query: &str) -> Result<Vec<Member>> {
        let v = self.get(&format!("/guilds/{}/members/search?query={query}&limit=10", self.guild_id)).await?;
        Ok(v.as_array().unwrap_or(&vec![]).iter().map(parse_member).collect())
    }

    pub async fn get_members(&self, limit: u32) -> Result<Vec<Member>> {
        let lim = limit.min(1000);
        let v = self.get(&format!("/guilds/{}/members?limit={lim}", self.guild_id)).await?;
        Ok(v.as_array().unwrap_or(&vec![]).iter().map(parse_member).collect())
    }

    pub async fn kick(&self, user_id: &str, reason: &str) -> Result<()> {
        let r = self.http.delete(format!("{BASE}/guilds/{}/members/{user_id}", self.guild_id))
            .header("Authorization", self.auth())
            .header("X-Audit-Log-Reason", reason)
            .send().await?;
        if !r.status().is_success() {
            return Err(AppError::Discord(r.text().await.unwrap_or_default()));
        }
        Ok(())
    }

    pub async fn ban(&self, user_id: &str, reason: &str) -> Result<()> {
        let r = self.http.put(format!("{BASE}/guilds/{}/bans/{user_id}", self.guild_id))
            .header("Authorization", self.auth())
            .header("X-Audit-Log-Reason", reason)
            .json(&json!({"delete_message_seconds":0}))
            .send().await?;
        if !r.status().is_success() {
            return Err(AppError::Discord(r.text().await.unwrap_or_default()));
        }
        Ok(())
    }

    pub async fn guild_name(&self) -> Result<String> {
        let v = self.get(&format!("/guilds/{}", self.guild_id)).await?;
        Ok(v["name"].as_str().unwrap_or("Unknown").to_string())
    }
}

fn parse_member(m: &Value) -> Member {
    let username = m["user"]["username"].as_str().unwrap_or("").to_string();
    let display = m["nick"].as_str()
        .or_else(|| m["user"]["global_name"].as_str())
        .unwrap_or(&username)
        .to_string();
    Member {
        user_id:    m["user"]["id"].as_str().unwrap_or("").into(),
        username,
        display,
        joined_at:  m["joined_at"].as_str().unwrap_or("").into(),
    }
}
