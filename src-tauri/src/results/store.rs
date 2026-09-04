use super::buffer::ResultSetBuffer;
use crate::error::{AppError, Result};
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// In-memory result buffers, keyed by result-set id. Rows never cross IPC as
/// JSON — the frontend fetches binary windows via `results_window`.
pub struct ResultStore {
    next_id: AtomicU64,
    buffers: DashMap<u64, Arc<parking_lot_shim::Mutex<ResultSetBuffer>>>,
    /// executionId -> result set ids (for release on re-run/tab close).
    executions: DashMap<String, Vec<u64>>,
}

/// std Mutex wrapper so callers don't unwrap poisoned locks everywhere.
mod parking_lot_shim {
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {
        pub fn new(v: T) -> Self {
            Self(std::sync::Mutex::new(v))
        }
        pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
            self.0.lock().unwrap_or_else(|p| p.into_inner())
        }
    }
}
pub use parking_lot_shim::Mutex as BufferMutex;

impl ResultStore {
    pub fn new() -> Self {
        Self { next_id: AtomicU64::new(1), buffers: DashMap::new(), executions: DashMap::new() }
    }

    pub fn insert(&self, execution_id: &str, buffer: ResultSetBuffer) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.buffers.insert(id, Arc::new(BufferMutex::new(buffer)));
        self.executions.entry(execution_id.to_string()).or_default().push(id);
        id
    }

    pub fn get(&self, id: u64) -> Result<Arc<BufferMutex<ResultSetBuffer>>> {
        self.buffers
            .get(&id)
            .map(|e| e.value().clone())
            .ok_or_else(|| AppError::Internal(format!("result set {id} released")))
    }

    pub fn release_execution(&self, execution_id: &str) {
        if let Some((_, ids)) = self.executions.remove(execution_id) {
            for id in ids {
                self.buffers.remove(&id);
            }
        }
    }
}
