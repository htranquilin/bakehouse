use crate::error::Result;
use crate::sql::executor;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn query_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    sql: String,
) -> Result<String> {
    let execution_id = uuid::Uuid::new_v4().simple().to_string();
    // Prior results of this tab stay alive until the frontend releases them;
    // each execution has its own id.
    let sessions = state.sessions.clone();
    let results = state.results.clone();
    let history = state.history.clone();
    let exec_id = execution_id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = executor::run_query(
            app, sessions, results, history, session_id, exec_id, sql, executor::ROW_CAP,
        )
        .await
        {
            tracing::error!("query execution failed: {e}");
        }
    });
    Ok(execution_id)
}

#[tauri::command]
pub async fn query_run_uncapped(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    sql: String,
) -> Result<String> {
    let execution_id = uuid::Uuid::new_v4().simple().to_string();
    let sessions = state.sessions.clone();
    let results = state.results.clone();
    let history = state.history.clone();
    let exec_id = execution_id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = executor::run_query(
            app, sessions, results, history, session_id, exec_id, sql, usize::MAX,
        )
        .await
        {
            tracing::error!("query execution failed: {e}");
        }
    });
    Ok(execution_id)
}

#[tauri::command]
pub async fn query_cancel(state: State<'_, AppState>, execution_id: String) -> Result<()> {
    executor::cancel_query(&state.sessions, &execution_id).await
}
