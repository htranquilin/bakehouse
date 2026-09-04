// Typed Tauri event names + payloads. Mirror of src-tauri/src/events.rs.

export const EVENTS = {
  instanceState: "instance:state",
  imagePullProgress: "image:pull-progress",
  queryStarted: "query:started",
  queryResultSet: "query:result-set",
  queryRows: "query:rows",
  queryMessage: "query:message",
  queryFinished: "query:finished",
  sessionReset: "session:reset",
  restoreProgress: "restore:progress",
  importProgress: "import:progress",
  backupProgress: "backup:progress",
  jobDone: "job:done",
  runtimeFatal: "runtime:fatal",
} as const;

export interface InstanceStatePayload {
  instanceId: string;
  state: string;
  detail?: string;
}

export interface JobDonePayload {
  jobId: string;
  ok: boolean;
  error?: string;
}
