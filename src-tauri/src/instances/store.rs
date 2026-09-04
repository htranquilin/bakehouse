//! instances.json persistence (atomic write: temp file + rename).

use super::model::Instance;
use crate::error::{AppError, Result};
use std::path::PathBuf;

pub struct InstanceStore {
    path: PathBuf,
}

impl InstanceStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Vec<Instance>> {
        match std::fs::read_to_string(&self.path) {
            Ok(json) => serde_json::from_str(&json)
                .map_err(|e| AppError::Internal(format!("instances.json is corrupt: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, instances: &[Instance]) -> Result<()> {
        let json = serde_json::to_string_pretty(instances)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
