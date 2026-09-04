import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { EVENTS, type JobDonePayload } from "../../lib/events";
import * as ipc from "../../lib/ipc";

interface FileConfig extends ipc.CsvFileInfo {
  table: string;
  expanded: boolean;
}

type Step =
  | { kind: "pick" }
  | { kind: "inspecting" }
  | { kind: "configure" }
  | { kind: "importing"; jobId: string; fileIndex: number; totalFiles: number; rowsDone: number; fileName: string }
  | { kind: "done"; summary: string }
  | { kind: "failed"; error: string };

export function ImportCsvWizard({
  instanceId,
  presetDatabase,
  onClose,
  onImported,
}: {
  instanceId: string;
  presetDatabase?: string;
  onClose: () => void;
  onImported: () => void;
}) {
  const [step, setStep] = useState<Step>({ kind: "pick" });
  const [files, setFiles] = useState<FileConfig[]>([]);
  const [databases, setDatabases] = useState<string[]>([]);
  const [database, setDatabase] = useState(presetDatabase ?? "");
  const [schema, setSchema] = useState("dbo");
  const [replace, setReplace] = useState(false);
  const [allText, setAllText] = useState(false);

  useEffect(() => {
    void ipc.dbList(instanceId).then((dbs) => {
      const user = dbs.filter((d) => !["master", "tempdb", "model", "msdb"].includes(d));
      setDatabases(dbs);
      setDatabase((cur) => cur || user[0] || dbs[0] || "");
    });
  }, [instanceId]);

  useEffect(() => {
    let unlistens: UnlistenFn[] = [];
    void Promise.all([
      listen<{ jobId: string; fileIndex: number; totalFiles: number; fileName: string; rowsDone: number }>(
        EVENTS.importProgress,
        (e) => {
          setStep((s) =>
            s.kind === "importing" && s.jobId === e.payload.jobId
              ? { ...s, ...e.payload }
              : s,
          );
        },
      ),
      listen<JobDonePayload>(EVENTS.jobDone, (e) => {
        setStep((s) => {
          if (s.kind !== "importing" || s.jobId !== e.payload.jobId) return s;
          if (e.payload.ok) {
            onImported();
            return { kind: "done", summary: `Imported ${s.totalFiles} file${s.totalFiles === 1 ? "" : "s"}.` };
          }
          return { kind: "failed", error: e.payload.error ?? "import failed" };
        });
      }),
    ]).then((u) => {
      unlistens = u;
    });
    return () => unlistens.forEach((u) => u());
  }, [onImported]);

  const inspect = async (paths: string[]) => {
    if (paths.length === 0) return;
    setStep({ kind: "inspecting" });
    try {
      const infos = await ipc.csvInspect(paths);
      setFiles(infos.map((f) => ({ ...f, table: f.suggestedTable, expanded: false })));
      setStep({ kind: "configure" });
    } catch (e) {
      setStep({ kind: "failed", error: ipc.errorMessage(e) });
    }
  };

  const pickFiles = async () => {
    const picked = await open({
      multiple: true,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!picked) return;
    await inspect(Array.isArray(picked) ? picked : [picked]);
  };

  const pickFolder = async () => {
    const dir = await open({ directory: true, multiple: false, title: "Import every .csv in a folder" });
    if (typeof dir !== "string") return;
    try {
      const paths = await ipc.csvScanDir(dir);
      if (paths.length === 0) {
        setStep({ kind: "failed", error: `No .csv files found in ${dir}` });
        return;
      }
      await inspect(paths);
    } catch (e) {
      setStep({ kind: "failed", error: ipc.errorMessage(e) });
    }
  };

  const patchFile = (path: string, patch: Partial<FileConfig>) =>
    setFiles((fs) => fs.map((f) => (f.path === path ? { ...f, ...patch } : f)));

  // Changing delimiter/header invalidates the inferred columns — re-inspect.
  const reinspectFile = async (f: FileConfig, patch: { delimiter?: string; hasHeader?: boolean }) => {
    const delimiter = patch.delimiter ?? f.delimiter;
    const hasHeader = patch.hasHeader ?? f.hasHeader;
    patchFile(f.path, { delimiter, hasHeader });
    try {
      const fresh = await ipc.csvReinspect(f.path, delimiter, hasHeader);
      patchFile(f.path, { columns: fresh.columns, sampleRows: fresh.sampleRows });
    } catch {
      // keep the previous inference on failure
    }
  };

  const startImport = async () => {
    try {
      const jobId = await ipc.csvImport(
        instanceId,
        database,
        schema.trim() || "dbo",
        replace,
        allText,
        files.map((f) => ({
          path: f.path,
          table: f.table.trim() || f.suggestedTable,
          delimiter: f.delimiter,
          hasHeader: f.hasHeader,
          columns: f.columns,
        })),
      );
      setStep({ kind: "importing", jobId, fileIndex: 0, totalFiles: files.length, rowsDone: 0, fileName: "" });
    } catch (e) {
      setStep({ kind: "failed", error: ipc.errorMessage(e) });
    }
  };

  const tableNamesValid =
    new Set(files.map((f) => f.table.trim().toLowerCase())).size === files.length &&
    files.every((f) => f.table.trim());

  return (
    <div className="modal-overlay" onClick={step.kind === "importing" ? undefined : onClose}>
      <div className="modal import-modal" onClick={(e) => e.stopPropagation()}>
        <h2>Import CSV files</h2>

        {step.kind === "pick" && (
          <>
            <p>
              Each file becomes a table (named after the file). Bakehouse detects the delimiter,
              header row, and column types — you can adjust before importing.
            </p>
            <div className="settings-actions">
              <button className="btn-primary" onClick={() => void pickFiles()}>
                Choose CSV files…
              </button>
              <button className="btn-ghost" onClick={() => void pickFolder()}>
                Import a whole folder…
              </button>
            </div>
          </>
        )}

        {step.kind === "inspecting" && <p className="bh-proofing">Reading files and inferring column types…</p>}

        {step.kind === "configure" && (
          <>
            <div className="csv-grid">
              <label className="field">
                Target database
                <select value={database} onChange={(e) => setDatabase(e.target.value)}>
                  {databases.map((db) => (
                    <option key={db} value={db}>
                      {db}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                Schema
                <input value={schema} onChange={(e) => setSchema(e.target.value)} spellCheck={false} />
              </label>
            </div>
            <div className="scripts-options">
              <label className="field-check">
                <input type="checkbox" checked={replace} onChange={(e) => setReplace(e.target.checked)} />
                Replace existing tables
              </label>
              <label className="field-check">
                <input type="checkbox" checked={allText} onChange={(e) => setAllText(e.target.checked)} />
                Import all columns as text
              </label>
            </div>
            <div className="scripts-list import-list">
              {files.map((f) => (
                <div key={f.path} className="import-file">
                  <div className="import-file-row">
                    <button className="btn-ghost" onClick={() => patchFile(f.path, { expanded: !f.expanded })}>
                      {f.expanded ? "▾" : "▸"}
                    </button>
                    <span className="import-file-name" title={f.path}>
                      {f.fileName}
                    </span>
                    <input
                      className="import-table-input"
                      value={f.table}
                      onChange={(e) => patchFile(f.path, { table: e.target.value })}
                      spellCheck={false}
                      title="Table name"
                    />
                    <select
                      value={f.delimiter}
                      title="Delimiter"
                      onChange={(e) => void reinspectFile(f, { delimiter: e.target.value })}
                    >
                      <option value=",">,</option>
                      <option value=";">;</option>
                      <option value="&#9;">⇥</option>
                      <option value="|">|</option>
                    </select>
                    <label className="field-check" title="First row is a header">
                      <input
                        type="checkbox"
                        checked={f.hasHeader}
                        onChange={(e) => void reinspectFile(f, { hasHeader: e.target.checked })}
                      />
                      header
                    </label>
                  </div>
                  {f.expanded && (
                    <div className="import-preview mono">
                      <div className="import-cols">
                        {f.columns.map((c) => `${c.name} ${allText ? "NVARCHAR(MAX)" : c.sqlType}`).join(" · ")}
                      </div>
                      <table>
                        <tbody>
                          {f.sampleRows.map((r, i) => (
                            <tr key={i}>
                              {f.columns.map((_, ci) => (
                                <td key={ci}>{r[ci] ?? ""}</td>
                              ))}
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                </div>
              ))}
            </div>
            {!tableNamesValid && (
              <pre className="wizard-error">Table names must be non-empty and unique.</pre>
            )}
            <button
              className="btn-primary"
              disabled={!database || files.length === 0 || !tableNamesValid}
              onClick={() => void startImport()}
            >
              Import {files.length} file{files.length === 1 ? "" : "s"} into [{database}]
            </button>
          </>
        )}

        {step.kind === "importing" && (
          <>
            <p className="bh-proofing">
              File {Math.min(step.fileIndex + 1, step.totalFiles)} of {step.totalFiles}
              {step.fileName ? ` — ${step.fileName}` : ""} · {step.rowsDone.toLocaleString()} rows
            </p>
            <div className="progress-track">
              <div
                className="progress-fill"
                style={{ width: `${((step.fileIndex + 0.5) / step.totalFiles) * 100}%` }}
              />
            </div>
          </>
        )}

        {step.kind === "done" && (
          <>
            <p>{step.summary}</p>
            <button className="btn-primary" onClick={onClose}>
              Close
            </button>
          </>
        )}

        {step.kind === "failed" && (
          <>
            <pre className="wizard-error">{step.error}</pre>
            <button className="btn-primary" onClick={() => setStep(files.length ? { kind: "configure" } : { kind: "pick" })}>
              Back
            </button>
          </>
        )}
      </div>
    </div>
  );
}
