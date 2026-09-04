import { ask } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import * as ipc from "../../lib/ipc";
import { useInstancesStore } from "../../stores/instancesStore";
import { useTabsStore, type Tab } from "../../stores/tabsStore";
import { CodeEditor } from "./CodeEditor";

export function EditorTabs({ gotoLine, onGotoHandled }: { gotoLine: number | null; onGotoHandled: () => void }) {
  const tabs = useTabsStore((s) => s.tabs);
  const activeTabId = useTabsStore((s) => s.activeTabId);
  const openTab = useTabsStore((s) => s.openTab);
  const closeTab = useTabsStore((s) => s.closeTab);
  const activate = useTabsStore((s) => s.activate);
  const active = tabs.find((t) => t.tabId === activeTabId) ?? null;

  return (
    <div className="editor-tabs">
      <div className="tab-strip" data-tauri-drag-region="">
        {tabs.map((t) => (
          <div
            key={t.tabId}
            className={`tab ${t.tabId === activeTabId ? "active" : ""}`}
            onClick={() => activate(t.tabId)}
          >
            <span className="tab-title">
              {t.dirty ? "● " : ""}
              {t.title}
            </span>
            <button
              className="btn-ghost tab-close"
              onClick={(e) => {
                e.stopPropagation();
                void (async () => {
                  if (t.trancount > 0) {
                    const ok = await ask(
                      `"${t.title}" has an open transaction (@@TRANCOUNT = ${t.trancount}). Close anyway and roll back?`,
                      { title: "Bakehouse", kind: "warning", okLabel: "Close tab", cancelLabel: "Keep open" },
                    );
                    if (!ok) return;
                  }
                  closeTab(t.tabId);
                })();
              }}
            >
              ✕
            </button>
          </div>
        ))}
        <button className="btn-ghost tab-add" onClick={() => openTab()}>
          +
        </button>
      </div>
      {active ? (
        <TabEditor key={active.tabId} tab={active} gotoLine={gotoLine} onGotoHandled={onGotoHandled} />
      ) : (
        <div className="editor-empty">
          <button className="btn-primary" onClick={() => openTab()}>
            New query tab
          </button>
        </div>
      )}
    </div>
  );
}

function TabEditor({ tab, gotoLine, onGotoHandled }: { tab: Tab; gotoLine: number | null; onGotoHandled: () => void }) {
  const patch = useTabsStore((s) => s.patch);
  const instances = useInstancesStore((s) => s.instances);
  const connectTab = useTabsStore((s) => s.connectTab);
  const [databases, setDatabases] = useState<string[]>([]);
  const [schema, setSchema] = useState<Record<string, string[]> | null>(null);
  const selectedId = useInstancesStore((s) => s.selectedId);
  const contentsRef = useRef(tab.contents);

  // Which instance this tab should connect to:
  //  - a tab that already names an instance (context-menu tabs, restored
  //    buffers, reconnections) sticks to it — never silently another server;
  //  - otherwise the instance selected in the sidebar, if it's running;
  //  - otherwise any running instance as a convenience.
  const targetInstance = tab.instanceId
    ? (instances.find((i) => i.id === tab.instanceId) ?? null)
    : (() => {
        const selected = instances.find((i) => i.id === selectedId);
        if (selected?.state.kind === "running") return selected;
        return instances.find((i) => i.state.kind === "running") ?? null;
      })();
  const targetRunning = targetInstance?.state.kind === "running";

  // Auto-connect when the tab has no session, honoring the database the tab
  // was opened with (context-menu tabs).
  useEffect(() => {
    if (!tab.sessionId && targetInstance && targetRunning) {
      const id = targetInstance.id;
      void connectTab(tab.tabId, id, tab.database).catch(() =>
        // The tab's database may not exist (stale autosaved buffer) — fall
        // back to a default connection instead of a dead tab.
        connectTab(tab.tabId, id).catch(() => {}),
      );
    }
  }, [tab.sessionId, tab.tabId, tab.database, targetInstance, targetRunning, connectTab]);

  useEffect(() => {
    if (tab.instanceId && tab.sessionId) {
      ipc.dbList(tab.instanceId).then(setDatabases).catch(() => setDatabases([]));
      ipc
        .metaCompletionSchema(tab.instanceId, tab.database)
        .then(setSchema)
        .catch(() => setSchema(null));
    }
  }, [tab.instanceId, tab.sessionId, tab.database]);

  // Debounced autosave of the buffer (crash recovery).
  useEffect(() => {
    const interval = setInterval(() => {
      if (contentsRef.current !== null) {
        void ipc.buffersSave({
          tabId: tab.tabId,
          title: tab.title,
          instanceId: tab.instanceId,
          database: tab.database,
          contents: contentsRef.current,
          filePath: tab.filePath,
        });
      }
    }, 3000);
    return () => clearInterval(interval);
  }, [tab.tabId, tab.title, tab.instanceId, tab.database, tab.filePath]);

  const run = useCallback(
    async (sql: string) => {
      if (!tab.sessionId || tab.runningExecutionId || !sql.trim()) return;
      if (tab.executionId) void ipc.resultsRelease(tab.executionId);
      const executionId = await ipc.queryRun(tab.sessionId, sql);
      patch(tab.tabId, { runningExecutionId: executionId, executionId, lastSql: sql });
    },
    [tab.sessionId, tab.runningExecutionId, tab.executionId, tab.tabId, patch],
  );

  const cancel = useCallback(() => {
    if (tab.runningExecutionId) void ipc.queryCancel(tab.runningExecutionId);
  }, [tab.runningExecutionId]);

  const changeDatabase = async (db: string) => {
    if (!tab.sessionId) return;
    const info = await ipc.sessionSetDatabase(tab.sessionId, db);
    patch(tab.tabId, { database: info.database });
  };

  return (
    <div className="tab-editor">
      <div className="tab-toolbar">
        <button
          className="btn-primary btn-run"
          disabled={!tab.sessionId || !!tab.runningExecutionId}
          title="Run (⌘↵) — selection if any, else the whole buffer"
          onClick={() => {
            // Toolbar run always runs the whole buffer.
            void run(contentsRef.current);
          }}
        >
          ▶ Run
        </button>
        <button className="btn-ghost" disabled={!tab.runningExecutionId} onClick={cancel}>
          ◼ Cancel
        </button>
        <select
          className="db-select"
          value={tab.database}
          disabled={!tab.sessionId}
          onChange={(e) => void changeDatabase(e.target.value)}
        >
          {!databases.includes(tab.database) && <option value={tab.database}>{tab.database}</option>}
          {databases.map((db) => (
            <option key={db} value={db}>
              {db}
            </option>
          ))}
        </select>
        <span className="toolbar-session" title={targetInstance ? `Connected to ${targetInstance.name}` : undefined}>
          {tab.sessionId
            ? `${targetInstance?.name ?? "?"} · spid ${tab.spid}`
            : targetRunning
              ? "connecting…"
              : targetInstance
                ? `${targetInstance.name} is not running`
                : "no running instance"}
        </span>
        {tab.trancount > 0 && (
          <span className="txn-badge" title="Open transaction — COMMIT or ROLLBACK before closing">
            ⚠ open transaction ({tab.trancount})
          </span>
        )}
      </div>
      <CodeEditor
        value={tab.contents}
        onChange={(v) => {
          contentsRef.current = v;
          patch(tab.tabId, { contents: v, dirty: true });
        }}
        onRun={(sql) => void run(sql)}
        schema={schema}
        gotoLine={gotoLine}
        onGotoHandled={onGotoHandled}
      />
    </div>
  );
}
