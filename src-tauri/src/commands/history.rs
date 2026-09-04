use crate::error::Result;
use crate::history::{HistoryEntry, SavedBuffer};
use crate::AppState;
use tauri::State;

#[tauri::command]
pub fn history_list(
    state: State<'_, AppState>,
    filter: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<HistoryEntry>> {
    state.history.list(filter.as_deref(), limit.unwrap_or(100), offset.unwrap_or(0))
}

#[tauri::command]
pub fn buffers_save(state: State<'_, AppState>, buffer: SavedBuffer) -> Result<()> {
    state.history.save_buffer(&buffer)
}

#[tauri::command]
pub fn buffers_load_all(state: State<'_, AppState>) -> Result<Vec<SavedBuffer>> {
    state.history.load_buffers()
}

#[tauri::command]
pub fn buffers_delete(state: State<'_, AppState>, tab_id: String) -> Result<()> {
    state.history.delete_buffer(&tab_id)
}

#[tauri::command]
pub async fn file_read_sql(path: String) -> Result<String> {
    Ok(tokio::fs::read_to_string(&path).await?)
}

#[tauri::command]
pub async fn file_write_sql(path: String, contents: String) -> Result<()> {
    Ok(tokio::fs::write(&path, contents).await?)
}
