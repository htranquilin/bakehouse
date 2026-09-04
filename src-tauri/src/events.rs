//! Typed event names + payloads emitted to the frontend.
//! Mirror any change here in src/lib/events.ts.
#![allow(dead_code)] // consumed starting M2

use serde::Serialize;

pub const INSTANCE_STATE: &str = "instance:state";
pub const IMAGE_PULL_PROGRESS: &str = "image:pull-progress";
pub const QUERY_STARTED: &str = "query:started";
pub const QUERY_RESULT_SET: &str = "query:result-set";
pub const QUERY_ROWS: &str = "query:rows";
pub const QUERY_MESSAGE: &str = "query:message";
pub const QUERY_FINISHED: &str = "query:finished";
pub const SESSION_RESET: &str = "session:reset";
pub const RESTORE_PROGRESS: &str = "restore:progress";
pub const IMPORT_PROGRESS: &str = "import:progress";
pub const BACKUP_PROGRESS: &str = "backup:progress";
pub const JOB_DONE: &str = "job:done";
pub const RUNTIME_FATAL: &str = "runtime:fatal";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstanceStatePayload {
    pub instance_id: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JobDonePayload {
    pub job_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
