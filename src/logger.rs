use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: i64,
    pub ts: String,
    pub kind: String,
    pub target: String,
    pub detail: String,
    pub ok: bool,
}

pub struct ActivityLogger {
    path: PathBuf,
}

impl ActivityLogger {
    pub fn open() -> Result<Self> {
        let dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("theblackroom");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("activity.db");
        let logger = ActivityLogger { path };
        logger.init()?;
        Ok(logger)
    }

    /// pub(crate) so analytics.rs can run its own queries.
    pub(crate) fn conn(&self) -> Result<Connection> {
        Ok(Connection::open(&self.path)?)
    }

    fn init(&self) -> Result<()> {
        self.conn()?.execute_batch(
            "CREATE TABLE IF NOT EXISTS log (
                id     INTEGER PRIMARY KEY AUTOINCREMENT,
                ts     TEXT    NOT NULL,
                kind   TEXT    NOT NULL,
                target TEXT    NOT NULL DEFAULT '',
                detail TEXT    NOT NULL DEFAULT '',
                ok     INTEGER NOT NULL DEFAULT 1
            );",
        )?;
        Ok(())
    }

    pub fn write(&self, kind: &str, target: &str, detail: &str, ok: bool) -> Result<()> {
        let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
        self.conn()?.execute(
            "INSERT INTO log (ts,kind,target,detail,ok) VALUES (?1,?2,?3,?4,?5)",
            params![ts, kind, target, detail, ok as i64],
        )?;
        Ok(())
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<LogEntry>> {
        let conn = self.conn()?;
        let mut s = conn.prepare(
            "SELECT id,ts,kind,target,detail,ok FROM log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = s.query_map(params![limit as i64], |r| {
            Ok(LogEntry {
                id:     r.get(0)?,
                ts:     r.get(1)?,
                kind:   r.get(2)?,
                target: r.get(3)?,
                detail: r.get(4)?,
                ok:     r.get::<_, i64>(5)? != 0,
            })
        })?
        .filter_map(|x| x.ok())
        .collect();
        Ok(rows)
    }

    pub fn search(&self, q: &str) -> Result<Vec<LogEntry>> {
        let conn = self.conn()?;
        let like = format!("%{q}%");
        let mut s = conn.prepare(
            "SELECT id,ts,kind,target,detail,ok FROM log
             WHERE kind LIKE ?1 OR target LIKE ?1 OR detail LIKE ?1
             ORDER BY id DESC LIMIT 500",
        )?;
        let rows = s.query_map(params![like], |r| {
            Ok(LogEntry {
                id:     r.get(0)?,
                ts:     r.get(1)?,
                kind:   r.get(2)?,
                target: r.get(3)?,
                detail: r.get(4)?,
                ok:     r.get::<_, i64>(5)? != 0,
            })
        })?
        .filter_map(|x| x.ok())
        .collect();
        Ok(rows)
    }
}
