// ── Automation module ─────────────────────────────────────────────────────────
//
// Reads AutomationRule definitions from config (types live in config.rs) and
// executes them against live Notion + Discord state.
// Import: `mod automation;` in main.rs.

use crate::{
    config::{AutomationRule, RuleActionType, RuleConditionOp, RuleConditionSource},
    discord::DiscordClient,
    error::Result,
    notion::{build_prop, NotionClient},
};
use std::collections::HashMap;

// ── Rule execution ─────────────────────────────────────────────────────────────

/// Run one automation rule.
/// Returns a human-readable result string (never Err – errors surface in the string).
pub async fn run_rule(
    rule: &AutomationRule,
    notion: &NotionClient,
    discord: &DiscordClient,
) -> String {
    if !rule.enabled {
        return "skipped (disabled)".into();
    }

    let pages = match notion.query(None).await {
        Ok(p) => p,
        Err(e) => return format!("fetch error: {e}"),
    };

    let mut matched = 0usize;
    let mut errors: Vec<String> = vec![];

    for page in &pages {
        let field_val = match rule.condition.source {
            RuleConditionSource::Notion => page.display(&rule.condition.field),
            RuleConditionSource::Discord => continue, // Discord-sourced conditions not yet supported
        };

        let hit = match rule.condition.op {
            RuleConditionOp::Equals    => field_val == rule.condition.value,
            RuleConditionOp::NotEquals => field_val != rule.condition.value,
            RuleConditionOp::Contains  => field_val.to_lowercase()
                                            .contains(&rule.condition.value.to_lowercase()),
        };
        if !hit { continue; }
        matched += 1;

        // Resolve Discord user ID from common field names on the Notion page
        let discord_uid = {
            let v = page.display("Discord ID");
            if v.is_empty() { page.display("User ID") } else { v }
        };

        let res: std::result::Result<(), String> = match rule.action.action_type {
            RuleActionType::AddDiscordRole => {
                if discord_uid.is_empty() {
                    Err("no Discord ID field on page".into())
                } else {
                    discord.assign_role(&discord_uid, &rule.action.target).await
                        .map_err(|e| e.to_string())
                }
            }
            RuleActionType::RemoveDiscordRole => {
                if discord_uid.is_empty() {
                    Err("no Discord ID field on page".into())
                } else {
                    discord.remove_role(&discord_uid, &rule.action.target).await
                        .map_err(|e| e.to_string())
                }
            }
            RuleActionType::SendDiscordMessage => {
                discord.broadcast_message(&rule.action.target, &rule.action.value)
                    .await.map_err(|e| e.to_string())
            }
            RuleActionType::SetNotionField => {
                let kind = if rule.action.field_kind.is_empty() {
                    "rich_text"
                } else {
                    &rule.action.field_kind
                };
                let mut props = HashMap::new();
                props.insert(rule.action.target.clone(), build_prop(kind, &rule.action.value));
                notion.update_page(&page.id, props).await
                    .map(|_| ()).map_err(|e| e.to_string())
            }
            RuleActionType::LogActivity => Ok(()), // handled by caller
        };

        if let Err(e) = res { errors.push(e); }
    }

    if errors.is_empty() {
        format!("matched {matched} page(s), 0 errors")
    } else {
        format!("matched {matched}, {} error(s): {}", errors.len(), errors.join("; "))
    }
}

// ── Onboarding pipeline ───────────────────────────────────────────────────────

/// Create a Notion page for a new member and generate a Discord invite.
/// Returns a summary string with the page ID and invite URL.
pub async fn run_onboarding(
    notion: &NotionClient,
    discord: &DiscordClient,
    name: &str,
    email: &str,
    channel_id: &str,
) -> Result<String> {
    let mut props = HashMap::new();
    props.insert("Name".to_string(), build_prop("title", name));
    if !email.is_empty() {
        props.insert("Email".to_string(), build_prop("email", email));
    }
    let page = notion.create_page(props).await?;
    let invite = discord.create_invite(channel_id, 48, 1).await?;
    Ok(format!("page:{} | invite:{}", page.id, invite.url))
}
