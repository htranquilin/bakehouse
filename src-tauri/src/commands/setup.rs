use crate::error::Result;
use crate::runtime::ContainerRuntime;
use crate::AppState;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    pub rosetta_installed: bool,
    pub eula_accepted: bool,
    pub has_instances: bool,
}

fn rosetta_installed() -> bool {
    std::process::Command::new("/usr/bin/arch")
        .args(["-x86_64", "/usr/bin/true"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tauri::command]
pub async fn setup_status(state: State<'_, AppState>) -> Result<SetupStatus> {
    Ok(SetupStatus {
        rosetta_installed: rosetta_installed(),
        eula_accepted: state.settings.get().eula_accepted_at.is_some(),
        has_instances: !state.instances.list().await.is_empty(),
    })
}

#[tauri::command]
pub fn setup_accept_eula(state: State<'_, AppState>) -> Result<()> {
    state.settings.update(|s| s.eula_accepted_at = Some(chrono::Utc::now()))
}

#[tauri::command]
pub async fn runtime_shutdown(state: State<'_, AppState>) -> Result<()> {
    state.instances.stop_all().await;
    state.runtime.shutdown().await
}

#[tauri::command]
pub async fn diag_instance_logs(state: State<'_, AppState>, instance_id: String) -> Result<String> {
    state.runtime.logs(&instance_id, 500).await
}

#[tauri::command]
pub fn diag_app_log(state: State<'_, AppState>) -> Result<String> {
    let dir = state.paths.logs();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("bakehouse.log"))
        .collect();
    entries.sort_by_key(|e| e.file_name());
    match entries.last() {
        Some(e) => {
            let content = std::fs::read_to_string(e.path())?;
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(300);
            Ok(lines[start..].join("\n"))
        }
        None => Ok(String::new()),
    }
}
