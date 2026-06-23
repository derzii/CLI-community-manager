// ── Analytics module ──────────────────────────────────────────────────────────
//
// Data types + methods on ActivityLogger.
// Import: `mod analytics;` in main.rs.

use crate::{error::Result, logger::ActivityLogger};
use rusqlite::params;

// ── Summary types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct GrowthPoint {
    pub date: String, // YYYY-MM-DD
    pub count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct AnalyticsSummary {
    pub total_entries: i64,
    pub notion_adds: i64,
    pub notion_removes: i64,
    pub payment_checks: i64,
    pub discord_invites: i64,
    pub discord_kicks: i64,
    pub discord_bans: i64,
    pub db_cell_updates: i64,
    /// Member additions grouped by date (last 30 days with data).
    pub growth_by_day: Vec<GrowthPoint>,
    /// (adds - removes) / adds * 100, clamped 0..100.
    pub retention_pct: f64,
}

// ── ActivityLogger extensions ─────────────────────────────────────────────────

impl ActivityLogger {
    /// Compute aggregated analytics from the activity log.
    pub fn analytics_summary(&self) -> Result<AnalyticsSummary> {
        let conn = self.conn()?;

        let total_entries: i64 = conn
            .query_row("SELECT COUNT(*) FROM log", [], |r| r.get(0))
            .unwrap_or(0);

        let count_kind = |k: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM log WHERE kind = ?1",
                params![k],
                |r| r.get(0),
            )
            .unwrap_or(0)
        };

        let notion_adds    = count_kind("notion_add");
        let notion_removes = count_kind("notion_remove");
        let payment_checks = count_kind("payment_check");
        let discord_invites = count_kind("discord_invite");
        let discord_kicks  = count_kind("discord_kick");
        let discord_bans   = count_kind("discord_ban");
        let db_cell_updates = count_kind("db_cell_update");

        let mut stmt = conn.prepare(
            "SELECT substr(ts, 1, 10) AS day, COUNT(*) AS cnt
             FROM log WHERE kind = 'notion_add'
             GROUP BY day ORDER BY day ASC LIMIT 30",
        )?;
        let growth_by_day = stmt
            .query_map([], |r| Ok(GrowthPoint { date: r.get(0)?, count: r.get(1)? }))?
            .filter_map(|x| x.ok())
            .collect();

        let retention_pct = if notion_adds > 0 {
            ((notion_adds - notion_removes) as f64 / notion_adds as f64 * 100.0)
                .clamp(0.0, 100.0)
        } else {
            0.0
        };

        Ok(AnalyticsSummary {
            total_entries, notion_adds, notion_removes, payment_checks,
            discord_invites, discord_kicks, discord_bans, db_cell_updates,
            growth_by_day, retention_pct,
        })
    }

    /// Export the most-recent `limit` log entries as CSV text.
    pub fn export_log_csv(&self, limit: usize) -> Result<String> {
        let entries = self.recent(limit)?;
        let mut out = String::from("id,ts,kind,target,detail,ok\n");
        for e in entries {
            out.push_str(&format!(
                "{},{},{},{},{},{}\n",
                e.id,
                e.ts,
                e.kind,
                e.target.replace(',', ";"),
                e.detail.replace(',', ";"),
                if e.ok { 1 } else { 0 },
            ));
        }
        Ok(out)
    }
}

// ── Sparkline rendering ───────────────────────────────────────────────────────

/// Build a Unicode block-character sparkline `width` chars wide.
pub fn sparkline(data: &[GrowthPoint], width: usize) -> String {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if data.is_empty() || width == 0 {
        return " ".repeat(width);
    }
    let max_val = data.iter().map(|p| p.count).max().unwrap_or(1).max(1);
    let n = data.len();
    (0..width)
        .map(|i| {
            let idx = (i * n) / width;
            let val = data.get(idx).map(|p| p.count).unwrap_or(0);
            let level = ((val as f64 / max_val as f64) * 7.0).round() as usize;
            BLOCKS[level.min(7)]
        })
        .collect()
}
