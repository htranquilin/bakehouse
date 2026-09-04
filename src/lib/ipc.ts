// Typed wrappers over Tauri invoke — one function per #[tauri::command].
// Errors arrive as { code, message, detail? } (see src-tauri/src/error.rs).

import { invoke } from "@tauri-apps/api/core";

export interface AppErrorPayload {
  code: string;
  message: string;
  detail?: string;
}

export interface AppInfo {
  version: string;
  appSupport: string;
}

export interface SetupStatus {
  rosettaInstalled: boolean;
  eulaAccepted: boolean;
  hasInstances: boolean;
}

export type InstanceStateKind =
  | "stopped"
  | "pulling"
  | "starting"
  | "waitingForSql"
  | "running"
  | "stopping"
  | "failed";

export interface InstanceState {
  kind: InstanceStateKind;
  ip?: string;
  stage?: string;
  detail?: string;
}

export interface InstanceInfo {
  id: string;
  name: string;
  image: string;
  memoryMb: number;
  sqlMemoryMb: number;
  createdAt: string;
  state: InstanceState;
}

export const appInfo = () => invoke<AppInfo>("app_info");

// Setup / diagnostics
export const setupStatus = () => invoke<SetupStatus>("setup_status");
export const setupAcceptEula = () => invoke<void>("setup_accept_eula");
export const runtimeShutdown = () => invoke<void>("runtime_shutdown");
export const diagInstanceLogs = (instanceId: string) =>
  invoke<string>("diag_instance_logs", { instanceId });
export const diagAppLog = () => invoke<string>("diag_app_log");

// Instances
export interface SqlVersionInfo {
  id: string;
  label: string;
  image: string;
  recommended: boolean;
}
export const sqlVersions = () => invoke<SqlVersionInfo[]>("sql_versions");
export const instanceList = () => invoke<InstanceInfo[]>("instance_list");
export const instanceCreate = (name: string, memoryMb?: number, sqlVersion?: string) =>
  invoke<InstanceInfo>("instance_create", { name, memoryMb, sqlVersion });
export const instanceStart = (instanceId: string) =>
  invoke<void>("instance_start", { instanceId });
export const instanceStop = (instanceId: string) => invoke<void>("instance_stop", { instanceId });
export const instanceUpdate = (instanceId: string, patch: { name?: string; memoryMb?: number }) =>
  invoke<InstanceInfo>("instance_update", { instanceId, ...patch });
export const instanceDelete = (instanceId: string, deleteVolume: boolean) =>
  invoke<void>("instance_delete", { instanceId, deleteVolume });
export const instanceRevealPassword = (instanceId: string) =>
  invoke<string>("instance_reveal_password", { instanceId });
export const instanceExportCompose = (instanceId: string, destDir: string, includePassword: boolean) =>
  invoke<string[]>("instance_export_compose", { instanceId, destDir, includePassword });

// Sessions & queries
export interface SessionInfo {
  sessionId: string;
  spid: number;
  database: string;
  trancount: number;
}

export const sessionOpen = (instanceId: string, database?: string) =>
  invoke<SessionInfo>("session_open", { instanceId, database });
export const sessionClose = (sessionId: string) => invoke<void>("session_close", { sessionId });
export const sessionSetDatabase = (sessionId: string, database: string) =>
  invoke<SessionInfo>("session_set_database", { sessionId, database });
export const sessionState = (sessionId: string) =>
  invoke<SessionInfo>("session_state", { sessionId });
export const dbList = (instanceId: string) => invoke<string[]>("db_list", { instanceId });

export const queryRun = (sessionId: string, sql: string) =>
  invoke<string>("query_run", { sessionId, sql });
export const queryRunUncapped = (sessionId: string, sql: string) =>
  invoke<string>("query_run_uncapped", { sessionId, sql });
export const queryCancel = (executionId: string) => invoke<void>("query_cancel", { executionId });

// Results
export const resultsWindow = (resultSetId: number, startRow: number, count: number) =>
  invoke<ArrayBuffer>("results_window", { resultSetId, startRow, count });
export const resultsCell = (resultSetId: number, row: number, col: number) =>
  invoke<{ kind: string; display: string }>("results_cell", { resultSetId, row, col });
export const resultsSort = (resultSetId: number, col: number | null, descending: boolean) =>
  invoke<void>("results_sort", { resultSetId, col, descending });
export interface CsvOptions {
  separator?: string;
  quote?: string;
  includeHeader?: boolean;
  encoding?: string;
  nullAs?: string;
}
export const resultsExportCsv = (resultSetId: number, path: string, options?: CsvOptions) =>
  invoke<number>("results_export_csv", { resultSetId, path, options });
export interface CopyRect {
  rowStart: number;
  rowEnd: number;
  colStart: number;
  colEnd: number;
}
export const resultsCopyTsv = (resultSetId: number, rect?: CopyRect, includeHeader?: boolean) =>
  invoke<number>("results_copy_tsv", { resultSetId, rect, includeHeader });
export const resultsRelease = (executionId: string) =>
  invoke<void>("results_release", { executionId });

// History & buffers
export interface HistoryEntry {
  id: number;
  instanceId: string;
  database: string;
  sql: string;
  success: boolean;
  elapsedMs: number;
  ranAt: string;
}
export interface SavedBuffer {
  tabId: string;
  title: string;
  instanceId?: string | null;
  database?: string | null;
  contents: string;
  filePath?: string | null;
}
export const historyList = (filter?: string, limit?: number, offset?: number) =>
  invoke<HistoryEntry[]>("history_list", { filter, limit, offset });
export const buffersSave = (buffer: SavedBuffer) => invoke<void>("buffers_save", { buffer });
export const buffersLoadAll = () => invoke<SavedBuffer[]>("buffers_load_all");
export const buffersDelete = (tabId: string) => invoke<void>("buffers_delete", { tabId });
export const fileReadSql = (path: string) => invoke<string>("file_read_sql", { path });
export const fileWriteSql = (path: string, contents: string) =>
  invoke<void>("file_write_sql", { path, contents });

// Metadata / object tree
export interface ObjInfo {
  schema: string;
  name: string;
  objectId: number;
}
export interface ColInfo {
  name: string;
  dataType: string;
  nullable: boolean;
  isPk: boolean;
}
export const metaObjects = (instanceId: string, database: string, kind: "table" | "view" | "proc" | "fn") =>
  invoke<ObjInfo[]>("meta_objects", { instanceId, database, kind });
export const metaColumns = (instanceId: string, database: string, objectId: number) =>
  invoke<ColInfo[]>("meta_columns", { instanceId, database, objectId });
export const metaScriptObject = (instanceId: string, database: string, objectId: number, alter: boolean) =>
  invoke<string>("meta_script_object", { instanceId, database, objectId, alter });
export const metaCompletionSchema = (instanceId: string, database: string) =>
  invoke<Record<string, string[]>>("meta_completion_schema", { instanceId, database });
export const scriptGenerate = (
  instanceId: string,
  database: string,
  objectIds: number[],
  includeDrop: boolean,
  includeUse: boolean,
) => invoke<string>("script_generate", { instanceId, database, objectIds, includeDrop, includeUse });

// .bak flows
export interface BakFile {
  logicalName: string;
  physicalName: string;
  fileType: string;
  suggestedTarget: string;
}
export interface BackupSet {
  fileNumber: number;
  databaseName: string;
  backupType: string;
  finishDate: string;
  softwareVersionMajor: number;
  files: BakFile[];
  hasFilestream: boolean;
}
export interface StagedBak {
  stagedPath: string;
  serverVersionMajor: number;
  sets: BackupSet[];
}
export interface RestorePlan {
  stagedPath: string;
  fileNumber: number;
  databaseName: string;
  moves: { logicalName: string; target: string }[];
  replace: boolean;
}
export const bakInspect = (instanceId: string, hostPath: string) =>
  invoke<StagedBak>("bak_inspect", { instanceId, hostPath });
export const bakDiscardStaged = (instanceId: string, stagedPath: string) =>
  invoke<void>("bak_discard_staged", { instanceId, stagedPath });
export const bakRestore = (instanceId: string, plan: RestorePlan) =>
  invoke<string>("bak_restore", { instanceId, plan });
export const bakBackup = (instanceId: string, database: string, hostDest: string) =>
  invoke<string>("bak_backup", { instanceId, database, hostDest });

// CSV import
export interface CsvColumn {
  name: string;
  sqlType: string;
}
export interface CsvFileInfo {
  path: string;
  fileName: string;
  suggestedTable: string;
  delimiter: string;
  hasHeader: boolean;
  columns: CsvColumn[];
  sampleRows: string[][];
  sizeBytes: number;
}
export interface CsvImportSpec {
  path: string;
  table: string;
  delimiter: string;
  hasHeader: boolean;
  columns: CsvColumn[];
}
export const csvScanDir = (path: string) => invoke<string[]>("csv_scan_dir", { path });
export const csvInspect = (paths: string[]) => invoke<CsvFileInfo[]>("csv_inspect", { paths });
export const csvReinspect = (path: string, delimiter: string, hasHeader: boolean) =>
  invoke<CsvFileInfo>("csv_reinspect", { path, delimiter, hasHeader });
export const csvImport = (
  instanceId: string,
  database: string,
  schema: string,
  replace: boolean,
  allText: boolean,
  files: CsvImportSpec[],
) => invoke<string>("csv_import", { instanceId, database, schema, replace, allText, files });

export function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) {
    const p = e as AppErrorPayload;
    return p.detail ? `${p.message}\n${p.detail}` : p.message;
  }
  return String(e);
}
