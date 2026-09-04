//! .bak restore/backup flows. See spike/findings.md for the verified mechanics
//! (copy via `exec -i cat`, chown 10001:0, WITH MOVE for every logical file,
//! percent_complete polling on a second connection).

use crate::error::{AppError, Result};
use crate::events;
use crate::runtime::ContainerRuntime;
use crate::sql::session::SessionManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::Emitter;

pub const BACKUP_DIR: &str = "/var/opt/mssql/backup";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BackupSet {
    pub file_number: i32,
    pub database_name: String,
    pub backup_type: String,
    pub finish_date: String,
    pub software_version_major: i32,
    pub files: Vec<BakFile>,
    pub has_filestream: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BakFile {
    pub logical_name: String,
    pub physical_name: String,
    pub file_type: String,
    /// Suggested in-container restore target.
    pub suggested_target: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedBak {
    pub staged_path: String,
    pub server_version_major: i32,
    pub sets: Vec<BackupSet>,
}

fn quote_ident(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

fn quote_str(s: &str) -> String {
    format!("N'{}'", s.replace('\'', "''"))
}

/// Sanitize a database name into a filesystem-safe fragment for MOVE targets.
fn fs_fragment(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

pub async fn stage_and_inspect(
    runtime: &dyn ContainerRuntime,
    sessions: &SessionManager,
    instance_id: &str,
    ip: &str,
    host_path: &str,
) -> Result<StagedBak> {
    // Copy into the container (lands owned by mssql since exec runs as mssql;
    // chown as belt-and-braces).
    let file_name = format!("staged-{}.bak", uuid::Uuid::new_v4().simple());
    let staged_path = format!("{BACKUP_DIR}/{file_name}");
    runtime.exec(instance_id, &["mkdir", "-p", BACKUP_DIR], false).await?;
    runtime.copy_in(instance_id, std::path::Path::new(host_path), &staged_path).await?;
    runtime
        .exec(instance_id, &["chown", "-R", "10001:0", BACKUP_DIR], true)
        .await?;

    let mut conn = sessions.utility_checkout(instance_id, ip).await?;

    let server_version: String = conn
        .query_rows("SELECT CAST(SERVERPROPERTY('ProductVersion') AS nvarchar(30))")
        .await?
        .first()
        .and_then(|r| r.first())
        .map(|c| c.display.clone())
        .unwrap_or_default();
    let server_version_major: i32 =
        server_version.split('.').next().and_then(|v| v.parse().ok()).unwrap_or(0);

    // HEADERONLY: one row per backup set in the file.
    let header_sql = format!(
        "RESTORE HEADERONLY FROM DISK = {}",
        quote_str(&staged_path)
    );
    let mut header_cols: Vec<String> = vec![];
    let mut header_rows: Vec<Vec<crate::results::cell::Cell>> = vec![];
    conn.execute_streaming(&header_sql, |e| match e {
        crate::sql::conn::StreamEvent::ResultSet(cols) => {
            header_cols = cols.into_iter().map(|c| c.name).collect();
        }
        crate::sql::conn::StreamEvent::Row(cells) => header_rows.push(cells),
        _ => {}
    })
    .await?;
    if header_rows.is_empty() {
        // Clean up the staged file before failing.
        let _ = runtime.exec(instance_id, &["rm", "-f", &staged_path], true).await;
        return Err(AppError::runtime(
            "This file doesn't look like a SQL Server backup (RESTORE HEADERONLY returned nothing)",
            None,
        ));
    }
    let col = |name: &str| header_cols.iter().position(|c| c == name);
    let (pos_i, db_i, type_i, date_i, ver_i) = (
        col("Position"),
        col("DatabaseName"),
        col("BackupType"),
        col("BackupFinishDate"),
        col("SoftwareVersionMajor"),
    );

    let mut sets = vec![];
    for row in &header_rows {
        let get = |i: Option<usize>| i.and_then(|i| row.get(i)).map(|c| c.display.clone()).unwrap_or_default();
        let file_number: i32 = get(pos_i).parse().unwrap_or(1);
        let database_name = get(db_i);
        let backup_type = match get(type_i).as_str() {
            "1" => "Full".to_string(),
            "5" => "Differential".to_string(),
            "2" => "Log".to_string(),
            other => format!("Type {other}"),
        };

        // FILELISTONLY per set.
        let fl_sql = format!(
            "RESTORE FILELISTONLY FROM DISK = {} WITH FILE = {}",
            quote_str(&staged_path),
            file_number
        );
        let mut fl_cols: Vec<String> = vec![];
        let mut fl_rows: Vec<Vec<crate::results::cell::Cell>> = vec![];
        conn.execute_streaming(&fl_sql, |e| match e {
            crate::sql::conn::StreamEvent::ResultSet(cols) => {
                fl_cols = cols.into_iter().map(|c| c.name).collect();
            }
            crate::sql::conn::StreamEvent::Row(cells) => fl_rows.push(cells),
            _ => {}
        })
        .await?;
        let flc = |name: &str| fl_cols.iter().position(|c| c == name);
        let (ln_i, pn_i, ft_i) = (flc("LogicalName"), flc("PhysicalName"), flc("Type"));

        let db_frag = fs_fragment(&database_name);
        let mut files = vec![];
        let mut has_filestream = false;
        for (n, frow) in fl_rows.iter().enumerate() {
            let get = |i: Option<usize>| i.and_then(|i| frow.get(i)).map(|c| c.display.clone()).unwrap_or_default();
            let file_type = get(ft_i);
            if file_type == "S" {
                has_filestream = true;
            }
            let logical = get(ln_i);
            let ext = match file_type.as_str() {
                "L" => "ldf",
                "D" if n == 0 => "mdf",
                _ => "ndf",
            };
            files.push(BakFile {
                suggested_target: format!(
                    "/var/opt/mssql/data/{db_frag}_{}.{ext}",
                    fs_fragment(&logical)
                ),
                logical_name: logical,
                physical_name: get(pn_i),
                file_type,
            });
        }

        sets.push(BackupSet {
            file_number,
            database_name,
            backup_type,
            finish_date: get(date_i),
            software_version_major: get(ver_i).parse().unwrap_or(0),
            files,
            has_filestream,
        });
    }

    sessions.utility_return(instance_id, conn).await;
    Ok(StagedBak { staged_path, server_version_major, sets })
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RestorePlan {
    pub staged_path: String,
    pub file_number: i32,
    pub database_name: String,
    pub moves: Vec<MoveSpec>,
    pub replace: bool,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MoveSpec {
    pub logical_name: String,
    pub target: String,
}

/// Runs the restore in a spawned task; progress via `restore:progress`,
/// completion via `job:done` with jobId = the returned id.
#[allow(clippy::too_many_arguments)]
pub fn spawn_restore(
    app: tauri::AppHandle,
    runtime: Arc<dyn ContainerRuntime>,
    sessions: Arc<SessionManager>,
    instance_id: String,
    ip: String,
    plan: RestorePlan,
) -> String {
    let job_id = format!("restore-{}", uuid::Uuid::new_v4().simple());
    let jid = job_id.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_restore(&app, &*runtime, &sessions, &instance_id, &ip, &plan, &jid).await;
        let _ = app.emit(
            events::JOB_DONE,
            events::JobDonePayload {
                job_id: jid,
                ok: result.is_ok(),
                error: result.err().map(|e| e.to_string()),
            },
        );
    });
    job_id
}

async fn run_restore(
    app: &tauri::AppHandle,
    runtime: &dyn ContainerRuntime,
    sessions: &SessionManager,
    instance_id: &str,
    ip: &str,
    plan: &RestorePlan,
    job_id: &str,
) -> Result<()> {
    let emit = |stage: &str, percent: Option<f64>| {
        let _ = app.emit(
            events::RESTORE_PROGRESS,
            serde_json::json!({ "jobId": job_id, "stage": stage, "percent": percent }),
        );
    };
    emit("restoring", Some(0.0));

    let moves: Vec<String> = plan
        .moves
        .iter()
        .map(|m| format!("MOVE {} TO {}", quote_str(&m.logical_name), quote_str(&m.target)))
        .collect();
    let restore_sql = format!(
        "RESTORE DATABASE {} FROM DISK = {} WITH FILE = {}, {}, {}RECOVERY, STATS = 5",
        quote_ident(&plan.database_name),
        quote_str(&plan.staged_path),
        plan.file_number,
        moves.join(", "),
        if plan.replace { "REPLACE, " } else { "" },
    );

    // The restore occupies one connection; progress polls on a second one.
    let mut restore_conn = sessions.utility_checkout(instance_id, ip).await?;
    let poll_sessions = ip.to_string();
    let poll_instance = instance_id.to_string();
    let app2 = app.clone();
    let job2 = job_id.to_string();
    let password = crate::instances::keychain::get(instance_id)?;
    let poll_handle = tauri::async_runtime::spawn(async move {
        let Ok(mut poll_conn) =
            crate::sql::conn::SqlConn::connect(&poll_sessions, &password, None).await
        else {
            return;
        };
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let rows = poll_conn
                .query_rows(
                    "SELECT CAST(percent_complete AS float) FROM sys.dm_exec_requests WHERE command IN ('RESTORE DATABASE','RESTORE HEADERONLY','BACKUP DATABASE')",
                )
                .await;
            match rows {
                Ok(rows) => {
                    if let Some(p) = rows.first().and_then(|r| r.first()) {
                        let pct: f64 = p.display.parse().unwrap_or(0.0);
                        let stage = if pct >= 100.0 { "recovering" } else { "restoring" };
                        let _ = app2.emit(
                            events::RESTORE_PROGRESS,
                            serde_json::json!({ "jobId": job2, "stage": stage, "percent": pct }),
                        );
                    }
                }
                Err(_) => break,
            }
        }
        let _ = poll_instance;
    });

    let mut server_errors = vec![];
    let run = restore_conn
        .execute_streaming(&restore_sql, |e| {
            if let crate::sql::conn::StreamEvent::ServerError { number, message, .. } = e {
                server_errors.push(format!("Error {number}: {message}"));
            }
        })
        .await;
    poll_handle.abort();

    if let Err(e) = run {
        return Err(e);
    }
    if !server_errors.is_empty() {
        return Err(AppError::runtime(server_errors.join("\n"), None));
    }

    // Wait for ONLINE (recovery phase after 100%).
    emit("recovering", None);
    for _ in 0..150 {
        let state = restore_conn
            .query_rows(&format!(
                "SELECT state_desc FROM sys.databases WHERE name = {}",
                quote_str(&plan.database_name)
            ))
            .await?;
        if state.first().and_then(|r| r.first()).map(|c| c.display.as_str()) == Some("ONLINE") {
            sessions.utility_return(instance_id, restore_conn).await;
            emit("done", Some(100.0));
            // Remove the staged file; it has served its purpose.
            let _ = runtime.exec(instance_id, &["rm", "-f", &plan.staged_path], true).await;
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    Err(AppError::runtime("database did not come ONLINE after restore", None))
}

/// Backup a database and copy the .bak out to a host path.
pub fn spawn_backup(
    app: tauri::AppHandle,
    runtime: Arc<dyn ContainerRuntime>,
    sessions: Arc<SessionManager>,
    instance_id: String,
    ip: String,
    database: String,
    host_dest: String,
) -> String {
    let job_id = format!("backup-{}", uuid::Uuid::new_v4().simple());
    let jid = job_id.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_backup(&app, &*runtime, &sessions, &instance_id, &ip, &database, &host_dest, &jid).await;
        let _ = app.emit(
            events::JOB_DONE,
            events::JobDonePayload {
                job_id: jid,
                ok: result.is_ok(),
                error: result.err().map(|e| e.to_string()),
            },
        );
    });
    job_id
}

#[allow(clippy::too_many_arguments)]
async fn run_backup(
    app: &tauri::AppHandle,
    runtime: &dyn ContainerRuntime,
    sessions: &SessionManager,
    instance_id: &str,
    ip: &str,
    database: &str,
    host_dest: &str,
    job_id: &str,
) -> Result<()> {
    let file = format!("{BACKUP_DIR}/export-{}.bak", uuid::Uuid::new_v4().simple());
    runtime.exec(instance_id, &["mkdir", "-p", BACKUP_DIR], false).await?;

    let _ = app.emit(
        events::BACKUP_PROGRESS,
        serde_json::json!({ "jobId": job_id, "stage": "backing-up", "percent": 0.0 }),
    );
    let sql = format!(
        "BACKUP DATABASE {} TO DISK = {} WITH INIT, COMPRESSION, STATS = 10",
        quote_ident(database),
        quote_str(&file)
    );
    let mut conn = sessions.utility_checkout(instance_id, ip).await?;
    let mut server_errors = vec![];
    conn.execute_streaming(&sql, |e| {
        if let crate::sql::conn::StreamEvent::ServerError { number, message, .. } = e {
            server_errors.push(format!("Error {number}: {message}"));
        }
    })
    .await?;
    if !server_errors.is_empty() {
        return Err(AppError::runtime(server_errors.join("\n"), None));
    }
    sessions.utility_return(instance_id, conn).await;

    let _ = app.emit(
        events::BACKUP_PROGRESS,
        serde_json::json!({ "jobId": job_id, "stage": "copying-out", "percent": null }),
    );
    runtime.copy_out(instance_id, &file, std::path::Path::new(host_dest)).await?;
    let _ = runtime.exec(instance_id, &["rm", "-f", &file], true).await;
    Ok(())
}
