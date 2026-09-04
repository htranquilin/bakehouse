use crate::bak::{self, RestorePlan, StagedBak};
use crate::error::Result;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn bak_inspect(
    state: State<'_, AppState>,
    instance_id: String,
    host_path: String,
) -> Result<StagedBak> {
    let ip = state.instances.running_ip(&instance_id).await?;
    bak::stage_and_inspect(&*state.runtime, &state.sessions, &instance_id, &ip, &host_path).await
}

#[tauri::command]
pub async fn bak_discard_staged(
    state: State<'_, AppState>,
    instance_id: String,
    staged_path: String,
) -> Result<()> {
    use crate::runtime::ContainerRuntime;
    // Only paths inside the backup dir may be removed.
    if !staged_path.starts_with(bak::BACKUP_DIR) || staged_path.contains("..") {
        return Err(crate::AppError::Internal("invalid staged path".into()));
    }
    state.runtime.exec(&instance_id, &["rm", "-f", &staged_path], true).await?;
    Ok(())
}

#[tauri::command]
pub async fn bak_restore(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    plan: RestorePlan,
) -> Result<String> {
    let ip = state.instances.running_ip(&instance_id).await?;
    Ok(bak::spawn_restore(
        app,
        state.runtime.clone(),
        state.sessions.clone(),
        instance_id,
        ip,
        plan,
    ))
}

#[tauri::command]
pub async fn bak_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    database: String,
    host_dest: String,
) -> Result<String> {
    let ip = state.instances.running_ip(&instance_id).await?;
    Ok(bak::spawn_backup(
        app,
        state.runtime.clone(),
        state.sessions.clone(),
        instance_id,
        ip,
        database,
        host_dest,
    ))
}
