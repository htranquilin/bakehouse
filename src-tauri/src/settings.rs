use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// Timestamp of the user accepting the Microsoft SQL Server EULA in the wizard.
    pub eula_accepted_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct SettingsStore {
    path: PathBuf,
    cached: Mutex<AppSettings>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let cached = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, cached: Mutex::new(cached) }
    }

    pub fn get(&self) -> AppSettings {
        self.cached.lock().unwrap().clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut AppSettings)) -> Result<()> {
        let mut cached = self.cached.lock().unwrap();
        f(&mut cached);
        let json = serde_json::to_string_pretty(&*cached)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }
}
