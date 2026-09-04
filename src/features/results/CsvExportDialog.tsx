import { save } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import * as ipc from "../../lib/ipc";

interface Opts {
  separator: string;
  quote: string;
  includeHeader: boolean;
  encoding: string;
  nullAs: string;
}

const DEFAULTS: Opts = {
  separator: ",",
  quote: '"',
  includeHeader: true,
  encoding: "utf-8",
  nullAs: "",
};

const STORAGE_KEY = "bh-csv-options";

function loadOpts(): Opts {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") };
  } catch {
    return DEFAULTS;
  }
}

export function CsvExportDialog({
  resultSetId,
  onClose,
  onDone,
}: {
  resultSetId: number;
  onClose: () => void;
  onDone: (message: string) => void;
}) {
  const [opts, setOpts] = useState<Opts>(loadOpts);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<Opts>) => setOpts((o) => ({ ...o, ...patch }));

  const doExport = async () => {
    setBusy(true);
    setError(null);
    try {
      const path = await save({
        defaultPath: "results.csv",
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
      if (!path) {
        setBusy(false);
        return;
      }
      try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(opts));
      } catch {
        // storage unavailable; export proceeds with the chosen options anyway
      }
      const rows = await ipc.resultsExportCsv(resultSetId, path, opts);
      onDone(`Exported ${rows.toLocaleString()} rows to ${path.split("/").pop()}`);
    } catch (e) {
      setError(ipc.errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal csv-modal" onClick={(e) => e.stopPropagation()}>
        <h2>Export CSV</h2>
        <div className="csv-grid">
          <label className="field">
            Separator
            <select value={opts.separator} onChange={(e) => set({ separator: e.target.value })}>
              <option value=",">Comma ( , )</option>
              <option value=";">Semicolon ( ; )</option>
              <option value="&#9;">Tab</option>
              <option value="|">Pipe ( | )</option>
            </select>
          </label>
          <label className="field">
            Quote character
            <select value={opts.quote} onChange={(e) => set({ quote: e.target.value })}>
              <option value='"'>Double quote ( " )</option>
              <option value="'">Single quote ( ' )</option>
            </select>
          </label>
          <label className="field">
            Encoding
            <select value={opts.encoding} onChange={(e) => set({ encoding: e.target.value })}>
              <option value="utf-8">UTF-8</option>
              <option value="utf-8-bom">UTF-8 with BOM (Excel-friendly)</option>
              <option value="windows-1252">Windows-1252</option>
            </select>
          </label>
          <label className="field">
            Write NULL as
            <input
              value={opts.nullAs}
              placeholder="(empty)"
              onChange={(e) => set({ nullAs: e.target.value })}
              spellCheck={false}
            />
          </label>
        </div>
        <label className="field-check">
          <input
            type="checkbox"
            checked={opts.includeHeader}
            onChange={(e) => set({ includeHeader: e.target.checked })}
          />
          Include header row
        </label>
        {error && <pre className="wizard-error">{error}</pre>}
        <button className="btn-primary" disabled={busy} onClick={() => void doExport()}>
          {busy ? "Exporting…" : "Choose file and export"}
        </button>
      </div>
    </div>
  );
}
