//! Query history + autosaved buffers (rusqlite, bundled).

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

pub struct HistoryStore {
    db: Mutex<rusqlite::Connection>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub instance_id: String,
    pub database: String,
    pub sql: String,
    pub success: bool,
    pub elapsed_ms: u64,
    pub ran_at: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedBuffer {
    pub tab_id: String,
    pub title: String,
    pub instance_id: Option<String>,
    pub database: Option<String>,
    pub contents: String,
    pub file_path: Option<String>,
}

impl HistoryStore {
    pub fn open(path: &Path) -> Result<Self> {
        let db = rusqlite::Connection::open(path)
            .map_err(|e| AppError::Internal(format!("cannot open history db: {e}")))?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS query_history (
                id INTEGER PRIMARY KEY,
                instance_id TEXT NOT NULL,
                database_name TEXT NOT NULL,
                sql TEXT NOT NULL,
                success INTEGER NOT NULL,
                elapsed_ms INTEGER NOT NULL,
                ran_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_history_ran_at ON query_history(ran_at DESC);
            CREATE TABLE IF NOT EXISTS autosaved_buffers (
                tab_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                instance_id TEXT,
                database_name TEXT,
                contents TEXT NOT NULL,
                file_path TEXT,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .map_err(|e| AppError::Internal(format!("history schema: {e}")))?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// Best-effort — history failures never fail a query.
    pub fn record(&self, instance_id: &str, database: &str, sql: &str, success: bool, elapsed_ms: u64) {
        let db = self.db.lock().unwrap();
        let _ = db.execute(
            "INSERT INTO query_history (instance_id, database_name, sql, success, elapsed_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![instance_id, database, sql, success, elapsed_ms as i64],
        );
    }

    pub fn list(&self, filter: Option<&str>, limit: u32, offset: u32) -> Result<Vec<HistoryEntry>> {
        let db = self.db.lock().unwrap();
        let like = format!("%{}%", filter.unwrap_or(""));
        let mut stmt = db
            .prepare(
                "SELECT id, instance_id, database_name, sql, success, elapsed_ms, ran_at
                 FROM query_history WHERE sql LIKE ?1
                 ORDER BY id DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![like, limit, offset], |r| {
                Ok(HistoryEntry {
                    id: r.get(0)?,
                    instance_id: r.get(1)?,
                    database: r.get(2)?,
                    sql: r.get(3)?,
                    success: r.get::<_, i64>(4)? != 0,
                    elapsed_ms: r.get::<_, i64>(5)? as u64,
                    ran_at: r.get(6)?,
                })
            })
            .map_err(|e| AppError::Internal(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn save_buffer(&self, buf: &SavedBuffer) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO autosaved_buffers (tab_id, title, instance_id, database_name, contents, file_path, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
             ON CONFLICT(tab_id) DO UPDATE SET
               title=?2, instance_id=?3, database_name=?4, contents=?5, file_path=?6, updated_at=datetime('now')",
            rusqlite::params![buf.tab_id, buf.title, buf.instance_id, buf.database, buf.contents, buf.file_path],
        )
        .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(())
    }

    pub fn load_buffers(&self) -> Result<Vec<SavedBuffer>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db
            .prepare(
                "SELECT tab_id, title, instance_id, database_name, contents, file_path
                 FROM autosaved_buffers ORDER BY updated_at ASC",
            )
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SavedBuffer {
                    tab_id: r.get(0)?,
                    title: r.get(1)?,
                    instance_id: r.get(2)?,
                    database: r.get(3)?,
                    contents: r.get(4)?,
                    file_path: r.get(5)?,
                })
            })
            .map_err(|e| AppError::Internal(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn delete_buffer(&self, tab_id: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM autosaved_buffers WHERE tab_id = ?1", [tab_id])
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(())
    }
}
