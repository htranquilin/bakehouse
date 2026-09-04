use crate::error::Result;
use crate::sql::meta::{self, ColInfo, ObjInfo};
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn meta_objects(
    state: State<'_, AppState>,
    instance_id: String,
    database: String,
    kind: String,
) -> Result<Vec<ObjInfo>> {
    let ip = state.instances.running_ip(&instance_id).await?;
    let mut conn = state.sessions.utility_checkout(&instance_id, &ip).await?;
    let out = meta::objects(&mut conn, &database, &kind).await?;
    state.sessions.utility_return(&instance_id, conn).await;
    Ok(out)
}

#[tauri::command]
pub async fn meta_columns(
    state: State<'_, AppState>,
    instance_id: String,
    database: String,
    object_id: i64,
) -> Result<Vec<ColInfo>> {
    let ip = state.instances.running_ip(&instance_id).await?;
    let mut conn = state.sessions.utility_checkout(&instance_id, &ip).await?;
    let out = meta::columns(&mut conn, &database, object_id).await?;
    state.sessions.utility_return(&instance_id, conn).await;
    Ok(out)
}

#[tauri::command]
pub async fn meta_script_object(
    state: State<'_, AppState>,
    instance_id: String,
    database: String,
    object_id: i64,
    alter: bool,
) -> Result<String> {
    let ip = state.instances.running_ip(&instance_id).await?;
    let mut conn = state.sessions.utility_checkout(&instance_id, &ip).await?;
    let out = meta::script_object(&mut conn, &database, object_id, alter).await?;
    state.sessions.utility_return(&instance_id, conn).await;
    Ok(out)
}

#[tauri::command]
pub async fn script_generate(
    state: State<'_, AppState>,
    instance_id: String,
    database: String,
    object_ids: Vec<i64>,
    include_drop: bool,
    include_use: bool,
) -> Result<String> {
    let ip = state.instances.running_ip(&instance_id).await?;
    let mut conn = state.sessions.utility_checkout(&instance_id, &ip).await?;
    let out =
        meta::generate_scripts(&mut conn, &database, &object_ids, include_drop, include_use).await?;
    state.sessions.utility_return(&instance_id, conn).await;
    Ok(out)
}

#[tauri::command]
pub async fn meta_completion_schema(
    state: State<'_, AppState>,
    instance_id: String,
    database: String,
) -> Result<serde_json::Value> {
    let ip = state.instances.running_ip(&instance_id).await?;
    let mut conn = state.sessions.utility_checkout(&instance_id, &ip).await?;
    let out = meta::completion_schema(&mut conn, &database).await?;
    state.sessions.utility_return(&instance_id, conn).await;
    Ok(out)
}
