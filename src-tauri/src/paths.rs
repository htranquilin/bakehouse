use std::path::PathBuf;
use tauri::path::BaseDirectory;
use tauri::Manager;

/// All Bakehouse state lives under ~/Library/Application Support/com.bakehouse.app/.
pub struct AppPaths {
    pub app_support: PathBuf,
}

impl AppPaths {
    pub fn new(app: &tauri::AppHandle) -> tauri::Result<Self> {
        let app_support = app.path().app_data_dir()?;
        Ok(Self { app_support })
    }

    /// `--app-root` for the bundled container runtime (images, containers, volumes).
    pub fn container_root(&self) -> PathBuf {
        self.app_support.join("container-root")
    }

    pub fn container_logs(&self) -> PathBuf {
        self.app_support.join("container-logs")
    }

    pub fn logs(&self) -> PathBuf {
        self.app_support.join("logs")
    }

    pub fn instances_file(&self) -> PathBuf {
        self.app_support.join("instances.json")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.app_support.join("settings.json")
    }

    pub fn history_db(&self) -> PathBuf {
        self.app_support.join("history.db")
    }

    pub fn staging(&self) -> PathBuf {
        self.app_support.join("staging")
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            &self.app_support,
            &self.container_root(),
            &self.container_logs(),
            &self.logs(),
            &self.staging(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// Resolve a binary inside the bundled container-runtime resource tree.
/// Layout must stay exactly as Apple ships it: bin/ + libexec/container/plugins/.
#[allow(dead_code)] // consumed starting M2
pub fn runtime_binary(app: &tauri::AppHandle, name: &str) -> tauri::Result<PathBuf> {
    app.path()
        .resolve(format!("resources/container-runtime/bin/{name}"), BaseDirectory::Resource)
}
