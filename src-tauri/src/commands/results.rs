use crate::error::Result;
use crate::results::export::CsvOptions;
use crate::results::{export, window};
use crate::AppState;
use tauri::State;

#[tauri::command]
pub fn results_window(
    state: State<'_, AppState>,
    result_set_id: u64,
    start_row: u32,
    count: u32,
) -> Result<tauri::ipc::Response> {
    let buf = state.results.get(result_set_id)?;
    let buf = buf.lock();
    Ok(tauri::ipc::Response::new(window::serialize_window(&buf, start_row, count.min(2000))))
}

#[tauri::command]
pub fn results_cell(
    state: State<'_, AppState>,
    result_set_id: u64,
    row: u32,
    col: u32,
) -> Result<serde_json::Value> {
    let buf = state.results.get(result_set_id)?;
    let buf = buf.lock();
    let phys = buf
        .physical_row(row)
        .ok_or_else(|| crate::AppError::Internal("row out of range".into()))?;
    let cell = buf.rows[phys]
        .get(col as usize)
        .ok_or_else(|| crate::AppError::Internal("col out of range".into()))?;
    Ok(serde_json::json!({ "kind": cell.kind, "display": cell.display }))
}

#[tauri::command]
pub fn results_sort(
    state: State<'_, AppState>,
    result_set_id: u64,
    col: Option<u32>,
    descending: bool,
) -> Result<()> {
    use crate::results::cell::CellKind;
    let buf = state.results.get(result_set_id)?;
    let mut buf = buf.lock();
    match col {
        None => buf.sort_perm = None,
        Some(col) => {
            let col = col as usize;
            let mut perm: Vec<u32> = (0..buf.rows.len() as u32).collect();
            perm.sort_by(|&a, &b| {
                let ca = &buf.rows[a as usize][col];
                let cb = &buf.rows[b as usize][col];
                // NULLs first (ascending), numeric compare when both numeric.
                let ord = match (ca.kind, cb.kind) {
                    (CellKind::Null, CellKind::Null) => std::cmp::Ordering::Equal,
                    (CellKind::Null, _) => std::cmp::Ordering::Less,
                    (_, CellKind::Null) => std::cmp::Ordering::Greater,
                    (CellKind::Number, CellKind::Number) => ca
                        .display
                        .parse::<f64>()
                        .unwrap_or(f64::NAN)
                        .partial_cmp(&cb.display.parse::<f64>().unwrap_or(f64::NAN))
                        .unwrap_or(std::cmp::Ordering::Equal),
                    _ => ca.display.cmp(&cb.display),
                };
                if descending { ord.reverse() } else { ord }
            });
            buf.sort_perm = Some(perm);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn results_export_csv(
    state: State<'_, AppState>,
    result_set_id: u64,
    path: String,
    options: Option<CsvOptions>,
) -> Result<u64> {
    let buf = state.results.get(result_set_id)?;
    let opts = options.unwrap_or_default();
    let path = std::path::PathBuf::from(path);
    // Export can be large: do the work off the IPC thread.
    tauri::async_runtime::spawn_blocking(move || {
        let buf = buf.lock();
        export::export_csv(&buf, &path, &opts)
    })
    .await
    .map_err(|e| crate::AppError::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn results_copy_tsv(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    result_set_id: u64,
    rect: Option<export::CopyRect>,
    include_header: Option<bool>,
) -> Result<u64> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let buf = state.results.get(result_set_id)?;
    let (tsv, rows) = {
        let buf = buf.lock();
        (
            export::to_tsv(&buf, rect, include_header.unwrap_or(rect.is_none())),
            buf.rows.len() as u64,
        )
    };
    app.clipboard()
        .write_text(tsv)
        .map_err(|e| crate::AppError::Internal(e.to_string()))?;
    Ok(rows)
}

#[tauri::command]
pub fn results_release(state: State<'_, AppState>, execution_id: String) -> Result<()> {
    state.results.release_execution(&execution_id);
    Ok(())
}
