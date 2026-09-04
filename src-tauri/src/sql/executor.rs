//! Query execution: GO-split, stream per batch into ResultStore, emit events.
//! SSMS semantics: a server error in one batch does not stop later batches;
//! a transport error (killed connection) does.

use super::batch::split_batches;
use super::conn::StreamEvent;
use super::session::{RunningExec, SessionManager};
use crate::error::{AppError, Result};
use crate::events;
use crate::history::HistoryStore;
use crate::results::buffer::ResultSetBuffer;
use crate::results::store::ResultStore;
use std::sync::Arc;
use tauri::Emitter;

pub const ROW_CAP: usize = 10_000;

pub struct ExecOutcome {
    pub success: bool,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_query(
    app: tauri::AppHandle,
    sessions: Arc<SessionManager>,
    results: Arc<ResultStore>,
    history: Arc<HistoryStore>,
    session_id: String,
    execution_id: String,
    sql: String,
    row_cap: usize,
) -> Result<ExecOutcome> {
    let session = sessions.get(&session_id)?;
    let mut session = session.lock().await;

    sessions.mark_running(
        &execution_id,
        RunningExec {
            session_id: session_id.clone(),
            instance_id: session.instance_id.clone(),
            ip: session.ip.clone(),
            spid: session.conn.spid(),
        },
    );

    let batches = split_batches(&sql);
    let _ = app.emit(
        events::QUERY_STARTED,
        serde_json::json!({ "executionId": execution_id, "sessionId": session_id, "batchCount": batches.len() }),
    );

    let started = std::time::Instant::now();
    let mut any_error = false;
    let mut transport_error: Option<AppError> = None;
    let mut total_rows: u64 = 0;

    'batches: for batch in &batches {
        for _ in 0..batch.repeat {
            // Per-batch streaming state.
            let mut current_set: Option<(u64, usize)> = None; // (result_set_id, buffered)
            let mut emitted_rows: u64 = 0;

            let emit_rows = |app: &tauri::AppHandle, rs: u64, n: u64, truncated: bool| {
                let _ = app.emit(
                    events::QUERY_ROWS,
                    serde_json::json!({ "executionId": execution_id, "resultSetId": rs, "rowCount": n, "truncated": truncated }),
                );
            };

            let run_result = session.conn.execute_streaming(&batch.sql, |event| match event {
                StreamEvent::ResultSet(cols) => {
                    if let Some((rs, _)) = current_set.take() {
                        let buf = results.get(rs).ok();
                        if let Some(buf) = buf {
                            let b = buf.lock();
                            emit_rows(&app, rs, b.total_seen, b.truncated);
                        }
                    }
                    let names = cols.clone();
                    let id = results.insert(&execution_id, ResultSetBuffer::new(cols));
                    current_set = Some((id, 0));
                    emitted_rows = 0;
                    let _ = app.emit(
                        events::QUERY_RESULT_SET,
                        serde_json::json!({ "executionId": execution_id, "resultSetId": id, "columns": names }),
                    );
                }
                StreamEvent::Row(cells) => {
                    if let Some((rs, buffered)) = &mut current_set {
                        if let Ok(buf) = results.get(*rs) {
                            let mut b = buf.lock();
                            b.total_seen += 1;
                            total_rows += 1;
                            if *buffered < row_cap {
                                b.rows.push(cells);
                                *buffered += 1;
                            } else {
                                b.truncated = true;
                            }
                            if b.total_seen - emitted_rows >= 1000 {
                                emitted_rows = b.total_seen;
                                emit_rows(&app, *rs, b.total_seen, b.truncated);
                            }
                        }
                    }
                }
                StreamEvent::Info { number, line, message } => {
                    let _ = app.emit(
                        events::QUERY_MESSAGE,
                        serde_json::json!({
                            "executionId": execution_id, "kind": "info", "number": number,
                            "line": line + batch.start_line.saturating_sub(1), "text": message
                        }),
                    );
                }
                StreamEvent::ServerError { number, class, line, message, procedure } => {
                    any_error = true;
                    let editor_line = if procedure.is_empty() {
                        line + batch.start_line.saturating_sub(1)
                    } else {
                        line // error inside a proc: line is proc-relative
                    };
                    let _ = app.emit(
                        events::QUERY_MESSAGE,
                        serde_json::json!({
                            "executionId": execution_id, "kind": "error", "number": number,
                            "severity": class, "line": editor_line, "procedure": procedure,
                            "text": message
                        }),
                    );
                }
                StreamEvent::RowsAffected(n) => {
                    let _ = app.emit(
                        events::QUERY_MESSAGE,
                        serde_json::json!({
                            "executionId": execution_id, "kind": "rowcount",
                            "text": format!("({n} row{} affected)", if n == 1 { "" } else { "s" })
                        }),
                    );
                }
            })
            .await;

            // Close out the last result set of the batch.
            if let Some((rs, _)) = current_set.take() {
                if let Ok(buf) = results.get(rs) {
                    let b = buf.lock();
                    emit_rows(&app, rs, b.total_seen, b.truncated);
                }
            }

            if let Err(e) = run_result {
                transport_error = Some(e);
                any_error = true;
                break 'batches;
            }
        }
    }

    // Refresh trancount + current database (a batch may have run USE). A
    // failure here means the connection died mid-execution (e.g. KILL delivers
    // a server error, then the socket is dead) — treat it as a transport error
    // so the session gets a fresh connection below.
    let trancount = if transport_error.is_none() {
        match session.conn.probe_state().await {
            Ok((tc, db)) => {
                if !db.is_empty() {
                    session.database = db;
                }
                tc
            }
            Err(e) => {
                transport_error = Some(e);
                0
            }
        }
    } else {
        session.trancount
    };
    session.trancount = trancount;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    sessions.clear_running(&execution_id);

    history.record(&session.instance_id, &session.database, &sql, !any_error, elapsed_ms);

    // A killed/poisoned connection gets replaced so the tab stays usable.
    if let Some(te) = &transport_error {
        let _ = app.emit(
            events::QUERY_MESSAGE,
            serde_json::json!({ "executionId": execution_id, "kind": "error", "text": te.to_string() }),
        );
        let password = sessions.password(&session.instance_id)?;
        match super::conn::SqlConn::connect(&session.ip, &password, Some(&session.database)).await {
            Ok(fresh) => {
                session.conn = fresh;
                session.trancount = 0;
                let _ = app.emit(
                    events::SESSION_RESET,
                    serde_json::json!({ "sessionId": session_id, "newSpid": session.conn.spid() }),
                );
            }
            Err(e) => tracing::warn!("session {session_id} reconnect failed: {e}"),
        }
    }

    let _ = app.emit(
        events::QUERY_FINISHED,
        serde_json::json!({
            "executionId": execution_id, "elapsedMs": elapsed_ms, "totalRows": total_rows,
            "trancount": trancount, "database": session.database, "success": !any_error
        }),
    );

    Ok(ExecOutcome { success: !any_error })
}

/// Cancel = KILL the session's spid from a utility connection. The victim's
/// stream dies with a transport error; run_query then reconnects the tab.
pub async fn cancel_query(sessions: &SessionManager, execution_id: &str) -> Result<()> {
    let Some(exec) = sessions.running(execution_id) else {
        return Ok(()); // already finished
    };
    let mut conn = sessions.utility_checkout(&exec.instance_id, &exec.ip).await?;
    conn.exec_simple(&format!("KILL {}", exec.spid)).await?;
    sessions.utility_return(&exec.instance_id, conn).await;
    Ok(())
}
