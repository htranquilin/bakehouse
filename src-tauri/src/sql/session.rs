//! Editor-tab sessions (one dedicated connection each) and a small per-instance
//! utility pool for KILL / metadata / progress polling — never user queries.

use super::conn::SqlConn;
use crate::error::{AppError, Result};
use crate::instances::keychain;
use dashmap::DashMap;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Session {
    pub id: String,
    pub instance_id: String,
    pub ip: String,
    pub database: String,
    pub conn: SqlConn,
    pub trancount: i32,
}

/// Bookkeeping for a running execution, readable while the session is locked.
#[derive(Clone)]
pub struct RunningExec {
    pub session_id: String,
    pub instance_id: String,
    pub ip: String,
    pub spid: i16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: String,
    pub spid: i16,
    pub database: String,
    pub trancount: i32,
}

const UTILITY_POOL_MAX: usize = 3;

/// Resolves an instance id to its sa password. The app wires this to the macOS
/// Keychain; headless smoke tests inject an in-memory source (unsigned test
/// binaries would otherwise trigger Keychain authorization dialogs).
pub type PasswordSource = Arc<dyn Fn(&str) -> Result<String> + Send + Sync>;

pub struct SessionManager {
    sessions: DashMap<String, Arc<Mutex<Session>>>,
    running: DashMap<String, RunningExec>,
    utility: DashMap<String, Arc<Mutex<Vec<SqlConn>>>>,
    passwords: PasswordSource,
}

impl SessionManager {
    pub fn new(passwords: PasswordSource) -> Self {
        Self {
            sessions: DashMap::new(),
            running: DashMap::new(),
            utility: DashMap::new(),
            passwords,
        }
    }

    /// Production source: the macOS Keychain.
    pub fn with_keychain() -> Self {
        Self::new(Arc::new(keychain::get))
    }

    pub fn password(&self, instance_id: &str) -> Result<String> {
        (self.passwords)(instance_id)
    }

    pub async fn open(
        &self,
        instance_id: &str,
        ip: &str,
        database: Option<&str>,
    ) -> Result<SessionInfo> {
        let password = self.password(instance_id)?;
        let conn = SqlConn::connect(ip, &password, database).await?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let info = SessionInfo {
            session_id: id.clone(),
            spid: conn.spid(),
            database: database.unwrap_or("master").to_string(),
            trancount: 0,
        };
        let session = Session {
            id: id.clone(),
            instance_id: instance_id.to_string(),
            ip: ip.to_string(),
            database: info.database.clone(),
            conn,
            trancount: 0,
        };
        self.sessions.insert(id, Arc::new(Mutex::new(session)));
        Ok(info)
    }

    pub fn get(&self, session_id: &str) -> Result<Arc<Mutex<Session>>> {
        self.sessions
            .get(session_id)
            .map(|e| e.value().clone())
            .ok_or_else(|| AppError::Internal(format!("unknown session {session_id}")))
    }

    pub fn close(&self, session_id: &str) {
        self.sessions.remove(session_id);
        self.running.retain(|_, r| r.session_id != session_id);
    }

    /// Drop every session belonging to an instance (called on instance stop).
    pub fn close_for_instance(&self, instance_id: &str) {
        self.sessions.retain(|_, s| {
            // try_lock is fine: a busy session belongs to a running query on a
            // stopping instance; dropping the entry drops the Arc reference.
            match s.try_lock() {
                Ok(guard) => guard.instance_id != instance_id,
                Err(_) => true,
            }
        });
        self.utility.remove(instance_id);
    }

    pub fn mark_running(&self, execution_id: &str, exec: RunningExec) {
        self.running.insert(execution_id.to_string(), exec);
    }

    pub fn clear_running(&self, execution_id: &str) {
        self.running.remove(execution_id);
    }

    pub fn running(&self, execution_id: &str) -> Option<RunningExec> {
        self.running.get(execution_id).map(|e| e.value().clone())
    }

    /// Check a utility connection out of the per-instance pool (connecting if
    /// the pool is empty). Return it with [`utility_return`] on success; on
    /// error just drop it — it may be poisoned.
    pub async fn utility_checkout(&self, instance_id: &str, ip: &str) -> Result<SqlConn> {
        let pool = self
            .utility
            .entry(instance_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(Vec::new())))
            .value()
            .clone();
        if let Some(conn) = pool.lock().await.pop() {
            return Ok(conn);
        }
        let password = self.password(instance_id)?;
        SqlConn::connect(ip, &password, None).await
    }

    pub async fn utility_return(&self, instance_id: &str, conn: SqlConn) {
        if let Some(pool) = self.utility.get(instance_id).map(|e| e.value().clone()) {
            let mut pool = pool.lock().await;
            if pool.len() < UTILITY_POOL_MAX {
                pool.push(conn);
            }
        }
    }
}
