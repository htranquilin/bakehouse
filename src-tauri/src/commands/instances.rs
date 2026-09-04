use crate::error::Result;
use crate::instances::keychain;
use crate::instances::manager::InstanceInfo;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn instance_list(state: State<'_, AppState>) -> Result<Vec<InstanceInfo>> {
    Ok(state.instances.list().await)
}

#[tauri::command]
pub fn sql_versions() -> Vec<crate::instances::model::SqlVersionInfo> {
    crate::instances::model::SQL_VERSIONS.to_vec()
}

#[tauri::command]
pub async fn instance_create(
    state: State<'_, AppState>,
    name: String,
    memory_mb: Option<u32>,
    sql_version: Option<String>,
) -> Result<InstanceInfo> {
    let image = match sql_version.as_deref() {
        None => crate::instances::model::DEFAULT_IMAGE,
        Some(v) => crate::instances::model::image_for_version(v).ok_or_else(|| {
            crate::AppError::Internal(format!("unknown SQL Server version '{v}'"))
        })?,
    };
    state
        .instances
        .create(
            name,
            memory_mb.unwrap_or(crate::instances::model::DEFAULT_MEMORY_MB),
            image.to_string(),
        )
        .await
}

#[tauri::command]
pub async fn instance_start(state: State<'_, AppState>, instance_id: String) -> Result<()> {
    state.instances.start(instance_id).await
}

#[tauri::command]
pub async fn instance_stop(state: State<'_, AppState>, instance_id: String) -> Result<()> {
    state.sessions.close_for_instance(&instance_id);
    state.instances.stop(&instance_id).await
}

#[tauri::command]
pub async fn instance_update(
    state: State<'_, AppState>,
    instance_id: String,
    name: Option<String>,
    memory_mb: Option<u32>,
) -> Result<InstanceInfo> {
    state.instances.update(&instance_id, name, memory_mb).await
}

#[tauri::command]
pub async fn instance_delete(
    state: State<'_, AppState>,
    instance_id: String,
    delete_volume: bool,
) -> Result<()> {
    state.instances.delete(&instance_id, delete_volume).await
}

#[tauri::command]
pub fn instance_reveal_password(instance_id: String) -> Result<String> {
    keychain::get(&instance_id)
}

#[tauri::command]
pub async fn instance_export_compose(
    state: State<'_, AppState>,
    instance_id: String,
    dest_dir: String,
    include_password: bool,
) -> Result<Vec<String>> {
    let instance = state.instances.get(&instance_id).await?;
    let password = if include_password { Some(keychain::get(&instance_id)?) } else { None };
    crate::instances::export::write_compose(
        &instance,
        std::path::Path::new(&dest_dir),
        password.as_deref(),
    )
}
