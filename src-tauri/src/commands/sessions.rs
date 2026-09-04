use crate::error::Result;
use crate::sql::session::SessionInfo;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn session_open(
    state: State<'_, AppState>,
    instance_id: String,
    database: Option<String>,
) -> Result<SessionInfo> {
    let ip = state.instances.running_ip(&instance_id).await?;
    state.sessions.open(&instance_id, &ip, database.as_deref()).await
}

#[tauri::command]
pub async fn session_close(state: State<'_, AppState>, session_id: String) -> Result<()> {
    state.sessions.close(&session_id);
    Ok(())
}

#[tauri::command]
pub async fn session_set_database(
    state: State<'_, AppState>,
    session_id: String,
    database: String,
) -> Result<SessionInfo> {
    let session = state.sessions.get(&session_id)?;
    let mut session = session.lock().await;
    // Bracket-quote; ] escaping per T-SQL.
    let quoted = format!("USE [{}]", database.replace(']', "]]"));
    session.conn.exec_simple(&quoted).await?;
    session.database = database;
    Ok(SessionInfo {
        session_id: session.id.clone(),
        spid: session.conn.spid(),
        database: session.database.clone(),
        trancount: session.trancount,
    })
}

#[tauri::command]
pub async fn session_state(state: State<'_, AppState>, session_id: String) -> Result<SessionInfo> {
    let session = state.sessions.get(&session_id)?;
    let session = session.lock().await;
    Ok(SessionInfo {
        session_id: session.id.clone(),
        spid: session.conn.spid(),
        database: session.database.clone(),
        trancount: session.trancount,
    })
}

#[tauri::command]
pub async fn db_list(state: State<'_, AppState>, instance_id: String) -> Result<Vec<String>> {
    let ip = state.instances.running_ip(&instance_id).await?;
    let mut conn = state.sessions.utility_checkout(&instance_id, &ip).await?;
    let rows = conn
        .query_rows(
            "SELECT name FROM sys.databases WHERE state = 0 ORDER BY CASE WHEN database_id <= 4 THEN 0 ELSE 1 END, name",
        )
        .await?;
    state.sessions.utility_return(&instance_id, conn).await;
    Ok(rows.into_iter().filter_map(|r| r.into_iter().next().map(|c| c.display)).collect())
}
