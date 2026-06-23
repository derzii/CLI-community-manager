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
    pub role_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Role {
    pub id: String,
    pub name: String,
    /// Discord colour integer (0 = no colour).
    pub color: u32,
    pub position: i64,
}

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub id: String,
    pub actor_id: String,
    pub target_id: String,
    pub action_type: u32,
    pub action_label: &'static str,
    pub reason: String,
}

impl DiscordClient {
    pub fn new(
        token: impl Into<String>,
        guild_id: impl Into<String>,
        channel: impl Into<String>,
    ) -> Self {
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

    /// Create a single-use invite. Uses the default channel if `channel_id` is empty.
    pub async fn create_invite(
        &self,
        channel_id: &str,
        max_age_hours: u64,
        max_uses: u32,
    ) -> Result<Invite> {
        let ch = if channel_id.is_empty() { &self.default_channel } else { channel_id };
        let body = json!({ "max_age": max_age_hours * 3600, "max_uses": max_uses, "unique": true });
        let v = self.post_json(&format!("/channels/{ch}/invites"), &body).await?;
        let code = v["code"].as_str().unwrap_or("").to_string();
        Ok(Invite { url: format!("https://discord.gg/{code}"), code })
    }

    // ── Members ───────────────────────────────────────────────────────────────

    pub async fn search_members(&self, query: &str) -> Result<Vec<Member>> {
        let v = self.get(&format!(
            "/guilds/{}/members/search?query={query}&limit=10",
            self.guild_id
        )).await?;
        Ok(v.as_array().unwrap_or(&vec![]).iter().map(parse_member).collect())
    }

    pub async fn get_members(&self, limit: u32) -> Result<Vec<Member>> {
        let lim = limit.min(1000);
        let v = self.get(&format!("/guilds/{}/members?limit={lim}", self.guild_id)).await?;
        Ok(v.as_array().unwrap_or(&vec![]).iter().map(parse_member).collect())
    }

    pub async fn kick(&self, user_id: &str, reason: &str) -> Result<()> {
        let r = self.http
            .delete(format!("{BASE}/guilds/{}/members/{user_id}", self.guild_id))
            .header("Authorization", self.auth())
            .header("X-Audit-Log-Reason", reason)
            .send().await?;
        if !r.status().is_success() {
            return Err(AppError::Discord(r.text().await.unwrap_or_default()));
        }
        Ok(())
    }

    pub async fn ban(&self, user_id: &str, reason: &str) -> Result<()> {
        let r = self.http
            .put(format!("{BASE}/guilds/{}/bans/{user_id}", self.guild_id))
            .header("Authorization", self.auth())
            .header("X-Audit-Log-Reason", reason)
            .json(&json!({"delete_message_seconds": 0}))
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

    // ── Roles ─────────────────────────────────────────────────────────────────

    /// List all roles in the guild, sorted by position (highest first).
    pub async fn list_roles(&self) -> Result<Vec<Role>> {
        let v = self.get(&format!("/guilds/{}/roles", self.guild_id)).await?;
        let mut roles: Vec<Role> = v.as_array().unwrap_or(&vec![]).iter().map(|r| Role {
            id:       r["id"].as_str().unwrap_or("").to_string(),
            name:     r["name"].as_str().unwrap_or("").to_string(),
            color:    r["color"].as_u64().unwrap_or(0) as u32,
            position: r["position"].as_i64().unwrap_or(0),
        }).collect();
        roles.sort_by(|a, b| b.position.cmp(&a.position));
        Ok(roles)
    }

    /// Assign a role to a guild member.
    pub async fn assign_role(&self, user_id: &str, role_id: &str) -> Result<()> {
        let r = self.http
            .put(format!("{BASE}/guilds/{}/members/{user_id}/roles/{role_id}", self.guild_id))
            .header("Authorization", self.auth())
            .header("Content-Length", "0")
            .send().await?;
        if !r.status().is_success() {
            return Err(AppError::Discord(r.text().await.unwrap_or_default()));
        }
        Ok(())
    }

    /// Remove a role from a guild member.
    pub async fn remove_role(&self, user_id: &str, role_id: &str) -> Result<()> {
        let r = self.http
            .delete(format!("{BASE}/guilds/{}/members/{user_id}/roles/{role_id}", self.guild_id))
            .header("Authorization", self.auth())
            .send().await?;
        if !r.status().is_success() {
            return Err(AppError::Discord(r.text().await.unwrap_or_default()));
        }
        Ok(())
    }

    // ── Broadcast / DM ────────────────────────────────────────────────────────

    /// Post a message to a channel. Uses the default channel if `channel_id` is empty.
    pub async fn broadcast_message(&self, channel_id: &str, content: &str) -> Result<()> {
        let ch = if channel_id.is_empty() { &self.default_channel } else { channel_id };
        let body = json!({ "content": content });
        self.post_json(&format!("/channels/{ch}/messages"), &body).await?;
        Ok(())
    }

    /// Open (or return existing) DM channel with a user and send a message.
    pub async fn send_dm(&self, user_id: &str, content: &str) -> Result<()> {
        // 1. Create DM channel
        let ch_body = json!({ "recipient_id": user_id });
        let ch = self.post_json("/users/@me/channels", &ch_body).await?;
        let ch_id = ch["id"].as_str().unwrap_or("").to_string();
        if ch_id.is_empty() {
            return Err(AppError::Discord("Could not open DM channel".into()));
        }
        // 2. Send message
        let body = json!({ "content": content });
        self.post_json(&format!("/channels/{ch_id}/messages"), &body).await?;
        Ok(())
    }

    // ── Audit log ─────────────────────────────────────────────────────────────

    /// Fetch recent audit log entries (max 100).
    pub async fn get_audit_log(&self, limit: u32) -> Result<Vec<AuditEntry>> {
        let lim = limit.min(100).max(1);
        let v = self.get(&format!(
            "/guilds/{}/audit-logs?limit={lim}", self.guild_id
        )).await?;
        let entries = v["audit_log_entries"].as_array().unwrap_or(&vec![]).iter().map(|e| {
            let action_type = e["action_type"].as_u64().unwrap_or(0) as u32;
            AuditEntry {
                id:           e["id"].as_str().unwrap_or("").to_string(),
                actor_id:     e["user_id"].as_str().unwrap_or("").to_string(),
                target_id:    e["target_id"].as_str().unwrap_or("").to_string(),
                action_type,
                action_label: audit_label(action_type),
                reason:       e["reason"].as_str().unwrap_or("").to_string(),
            }
        }).collect();
        Ok(entries)
    }
}

fn audit_label(t: u32) -> &'static str {
    match t {
        1  => "GUILD_UPDATE",
        10 => "CHANNEL_CREATE",
        11 => "CHANNEL_UPDATE",
        12 => "CHANNEL_DELETE",
        20 => "MEMBER_KICK",
        21 => "MEMBER_PRUNE",
        22 => "MEMBER_BAN_ADD",
        23 => "MEMBER_BAN_REMOVE",
        24 => "MEMBER_ROLE_UPDATE",
        25 => "MEMBER_MOVE",
        28 => "ROLE_CREATE",
        29 => "ROLE_UPDATE",
        30 => "ROLE_DELETE",
        72 => "MESSAGE_DELETE",
        _ => "OTHER",
    }
}

fn parse_member(m: &Value) -> Member {
    let username = m["user"]["username"].as_str().unwrap_or("").to_string();
    let display = m["nick"].as_str()
        .or_else(|| m["user"]["global_name"].as_str())
        .unwrap_or(&username)
        .to_string();
    let role_ids = m["roles"].as_array()
        .map(|a| a.iter().filter_map(|r| r.as_str()).map(String::from).collect())
        .unwrap_or_default();
    Member {
        user_id:  m["user"]["id"].as_str().unwrap_or("").into(),
        username,
        display,
        joined_at: m["joined_at"].as_str().unwrap_or("").into(),
        role_ids,
    }
}
