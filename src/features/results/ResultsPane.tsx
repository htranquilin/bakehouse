import { useState } from "react";
import * as ipc from "../../lib/ipc";
import { useResultsStore, type ExecutionMeta, type QueryMessage } from "../../stores/resultsStore";
import { CsvExportDialog } from "./CsvExportDialog";
import { ResultsGrid } from "./ResultsGrid";

export function ResultsPane({
  executionId,
  onGotoLine,
  onRunUncapped,
}: {
  executionId: string | null;
  onGotoLine: (line: number) => void;
  onRunUncapped?: () => void;
}) {
  const exec = useResultsStore((s) => (executionId ? s.executions[executionId] : undefined));
  const [activeView, setActiveView] = useState<string>("auto");
  const [csvFor, setCsvFor] = useState<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  if (!exec) {
    return <div className="results-empty">Run a query (⌘↵) to see results here.</div>;
  }

  const hasError = exec.messages.some((m) => m.kind === "error");
  const view =
    activeView === "auto"
      ? hasError && !exec.running
        ? "messages"
        : exec.resultSets.length > 0
          ? `rs-${exec.resultSets[0].resultSetId}`
          : "messages"
      : activeView;

  return (
    <div className="results-pane">
      <div className="results-tabs">
        {exec.resultSets.map((rs, i) => (
          <button
            key={rs.resultSetId}
            className={`results-tab ${view === `rs-${rs.resultSetId}` ? "active" : ""}`}
            onClick={() => setActiveView(`rs-${rs.resultSetId}`)}
          >
            Results{exec.resultSets.length > 1 ? ` ${i + 1}` : ""}
          </button>
        ))}
        <button
          className={`results-tab ${view === "messages" ? "active" : ""}`}
          onClick={() => setActiveView("messages")}
        >
          Messages{hasError ? " ⚠" : ""}
        </button>
        <StatusBar exec={exec} view={view} onRunUncapped={onRunUncapped} onExportCsv={setCsvFor} />
      </div>
      {view === "messages" ? (
        <MessagesView messages={exec.messages} onGotoLine={onGotoLine} />
      ) : (
        (() => {
          const rs = exec.resultSets.find((r) => `rs-${r.resultSetId}` === view);
          return rs ? <ResultsGrid key={rs.resultSetId} meta={rs} /> : null;
        })()
      )}
      {csvFor != null && (
        <CsvExportDialog
          resultSetId={csvFor}
          onClose={() => setCsvFor(null)}
          onDone={(msg) => {
            setCsvFor(null);
            setToast(msg);
            setTimeout(() => setToast(null), 5000);
          }}
        />
      )}
      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

function StatusBar({
  exec,
  view,
  onRunUncapped,
  onExportCsv,
}: {
  exec: ExecutionMeta;
  view: string;
  onRunUncapped?: () => void;
  onExportCsv: (resultSetId: number) => void;
}) {
  const rs = exec.resultSets.find((r) => `rs-${r.resultSetId}` === view);

  return (
    <span className="results-status">
      {exec.running ? (
        <span className="bh-proofing">running…</span>
      ) : (
        <>
          {exec.totalRows != null && `${exec.totalRows.toLocaleString()} rows`}
          {exec.elapsedMs != null && ` · ${formatMs(exec.elapsedMs)}`}
        </>
      )}
      {rs?.truncated && !exec.running && (
        <span className="truncated-badge" title="The grid holds the first 10,000 rows">
          capped at 10,000
          {onRunUncapped && (
            <button className="btn-link" onClick={onRunUncapped}>
              run uncapped
            </button>
          )}
        </span>
      )}
      {rs && !exec.running && (
        <>
          <button className="btn-ghost" title="Copy as TSV with headers" onClick={() => void ipc.resultsCopyTsv(rs.resultSetId)}>
            Copy
          </button>
          <button className="btn-ghost" title="Export CSV…" onClick={() => onExportCsv(rs.resultSetId)}>
            CSV
          </button>
        </>
      )}
    </span>
  );
}

function MessagesView({
  messages,
  onGotoLine,
}: {
  messages: QueryMessage[];
  onGotoLine: (line: number) => void;
}) {
  if (messages.length === 0) {
    return <div className="messages-view mono">Commands completed successfully.</div>;
  }
  return (
    <div className="messages-view mono">
      {messages.map((m, i) => (
        <div key={i} className={`message message-${m.kind}`}>
          {m.kind === "error" ? (
            <>
              <span
                className="message-error-head"
                onClick={() => m.line && onGotoLine(m.line)}
                title="Jump to line"
              >
                Msg {m.number}, Severity {m.severity}
                {m.procedure ? `, Procedure ${m.procedure}` : ""}, Line {m.line}
              </span>
              <div>{m.text}</div>
            </>
          ) : (
            m.text
          )}
        </div>
      ))}
    </div>
  );
}

function formatMs(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(2)} s`;
  const m = Math.floor(ms / 60_000);
  return `${m}:${String(Math.floor((ms % 60_000) / 1000)).padStart(2, "0")} min`;
}
