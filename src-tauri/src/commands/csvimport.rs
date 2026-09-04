use crate::csvimport::{self, CsvFileInfo, ImportSpec};
use crate::error::Result;
use crate::events;
use crate::AppState;
use tauri::{Emitter, State};

#[tauri::command]
pub async fn csv_scan_dir(path: String) -> Result<Vec<String>> {
    tauri::async_runtime::spawn_blocking(move || csvimport::scan_dir(std::path::Path::new(&path)))
        .await
        .map_err(|e| crate::AppError::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn csv_inspect(paths: Vec<String>) -> Result<Vec<CsvFileInfo>> {
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .iter()
            .map(|p| csvimport::inspect_file(std::path::Path::new(p)))
            .collect::<Result<Vec<_>>>()
    })
    .await
    .map_err(|e| crate::AppError::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn csv_reinspect(
    path: String,
    delimiter: String,
    has_header: bool,
) -> Result<CsvFileInfo> {
    tauri::async_runtime::spawn_blocking(move || {
        csvimport::inspect_file_with(
            std::path::Path::new(&path),
            delimiter.as_bytes().first().copied(),
            Some(has_header),
        )
    })
    .await
    .map_err(|e| crate::AppError::Internal(e.to_string()))?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn csv_import(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    database: String,
    schema: String,
    replace: bool,
    all_text: bool,
    files: Vec<ImportSpec>,
) -> Result<String> {
    let ip = state.instances.running_ip(&instance_id).await?;
    let password = state.sessions.password(&instance_id)?;
    let job_id = format!("csvimport-{}", uuid::Uuid::new_v4().simple());
    let jid = job_id.clone();
    tauri::async_runtime::spawn(async move {
        let result = async {
            let mut conn = crate::sql::conn::SqlConn::connect(&ip, &password, None).await?;
            csvimport::run_import(&app, &mut conn, &database, &schema, &files, replace, all_text, &jid)
                .await
        }
        .await;
        let _ = app.emit(
            events::JOB_DONE,
            events::JobDonePayload {
                job_id: jid,
                ok: result.is_ok(),
                error: result.err().map(|e| e.to_string()),
            },
        );
    });
    Ok(job_id)
}
