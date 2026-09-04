import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { Mark } from "./components/Mark";
import { EditorTabs } from "./features/editor/EditorTabs";
import { FirstRunWizard } from "./features/wizard/FirstRunWizard";
import { InstancePicker } from "./features/instances/InstancePicker";
import { ImportCsvWizard } from "./features/import/ImportCsvWizard";
import { GenerateScriptsModal } from "./features/tree/GenerateScriptsModal";
import { ObjectTree } from "./features/tree/ObjectTree";
import { RestoreWizard } from "./features/restore/RestoreWizard";
import { ResultsPane } from "./features/results/ResultsPane";
import { bootstrapEvents } from "./lib/bootstrap";
import { EVENTS, type JobDonePayload } from "./lib/events";
import * as ipc from "./lib/ipc";
import { useInstancesStore } from "./stores/instancesStore";
import { useTabsStore } from "./stores/tabsStore";

function App() {
  const [setup, setSetup] = useState<ipc.SetupStatus | null>(null);
  const [wizardDone, setWizardDone] = useState(false);
  const [gotoLine, setGotoLine] = useState<number | null>(null);
  const [showRestore, setShowRestore] = useState(false);
  const [scriptsFor, setScriptsFor] = useState<string | null>(null);
  const [importCsv, setImportCsv] = useState<{ database?: string } | null>(null);
  const [sidebarW, setSidebarW] = useState(() => {
    const v = Number(localStorage.getItem("bh-sidebar-w"));
    return v >= 180 && v <= 560 ? v : 260;
  });
  const [bottomH, setBottomH] = useState(() => {
    const v = Number(localStorage.getItem("bh-bottom-h"));
    return v >= 120 ? v : 280;
  });

  const dragSplitter = (dir: "v" | "h") => (e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startY = e.clientY;
    const w0 = sidebarW;
    const h0 = bottomH;
    const move = (ev: MouseEvent) => {
      if (dir === "v") {
        setSidebarW(Math.min(560, Math.max(180, w0 + ev.clientX - startX)));
      } else {
        setBottomH(Math.min(window.innerHeight - 180, Math.max(120, h0 + (startY - ev.clientY))));
      }
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.style.cursor = "";
      setSidebarW((w) => {
        localStorage.setItem("bh-sidebar-w", String(w));
        return w;
      });
      setBottomH((h) => {
        localStorage.setItem("bh-bottom-h", String(h));
        return h;
      });
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    document.body.style.cursor = dir === "v" ? "col-resize" : "row-resize";
  };
  const [treeRefresh, setTreeRefresh] = useState(0);
  const [backupToast, setBackupToast] = useState<string | null>(null);
  const refresh = useInstancesStore((s) => s.refresh);
  const instances = useInstancesStore((s) => s.instances);
  const selectedId = useInstancesStore((s) => s.selectedId);
  const tabs = useTabsStore((s) => s.tabs);
  const activeTabId = useTabsStore((s) => s.activeTabId);
  const activeTab = tabs.find((t) => t.tabId === activeTabId);
  const selectedInstance = instances.find((i) => i.id === selectedId);

  useEffect(() => {
    bootstrapEvents();
    void refresh().then(() => ipc.setupStatus().then(setSetup));
    void ipc.buffersLoadAll().then((buffers) => {
      const store = useTabsStore.getState();
      if (store.tabs.length > 0) return;
      for (const b of buffers) {
        store.openTab({
          tabId: b.tabId,
          title: b.title,
          contents: b.contents,
          filePath: b.filePath ?? null,
          database: b.database ?? "master",
          instanceId: b.instanceId ?? null,
          dirty: true,
        });
      }
    });
  }, [refresh]);

  // Warn before closing the window with open transactions or running queries.
  // NOTE: window.confirm is a no-op in wry's WKWebView — the decision must be
  // made with the dialog plugin, so we preventDefault synchronously and
  // destroy() explicitly once the user confirms.
  useEffect(() => {
    const unlisten = getCurrentWindow().onCloseRequested((event) => {
      const offenders = useTabsStore
        .getState()
        .tabs.filter((t) => t.trancount > 0 || t.runningExecutionId);
      if (offenders.length === 0) return; // close proceeds normally
      event.preventDefault();
      void (async () => {
        const ok = await ask(
          `${offenders.map((t) => `"${t.title}"`).join(", ")} ${
            offenders.length === 1 ? "has" : "have"
          } an open transaction or running query. Quit anyway? Open transactions will roll back.`,
          { title: "Bakehouse", kind: "warning", okLabel: "Quit", cancelLabel: "Stay" },
        );
        if (ok) await getCurrentWindow().destroy();
      })();
    });
    return () => void unlisten.then((u) => u());
  }, []);

  // ⌘O / ⌘S for .sql files on the active tab.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!e.metaKey) return;
      const store = useTabsStore.getState();
      const tab = store.tabs.find((t) => t.tabId === store.activeTabId);
      if (e.key === "o") {
        e.preventDefault();
        void (async () => {
          const path = await open({
            multiple: false,
            filters: [{ name: "SQL", extensions: ["sql"] }],
          });
          if (typeof path !== "string") return;
          const contents = await ipc.fileReadSql(path);
          store.openTab({
            title: path.split("/").pop() ?? "query.sql",
            contents,
            filePath: path,
          });
        })();
      } else if (e.key === "s" && tab) {
        e.preventDefault();
        void (async () => {
          let path = tab.filePath;
          if (!path) {
            const picked = await save({
              defaultPath: `${tab.title.replace(/[^\w.-]+/g, "_")}.sql`,
              filters: [{ name: "SQL", extensions: ["sql"] }],
            });
            if (!picked) return;
            path = picked;
          }
          await ipc.fileWriteSql(path, tab.contents);
          store.patch(tab.tabId, {
            filePath: path,
            dirty: false,
            title: path.split("/").pop() ?? tab.title,
          });
        })();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  // Backup job feedback.
  useEffect(() => {
    const unlisten = listen<JobDonePayload>(EVENTS.jobDone, (e) => {
      if (e.payload.jobId.startsWith("backup-")) {
        setBackupToast(e.payload.ok ? "Backup written." : `Backup failed: ${e.payload.error}`);
        setTimeout(() => setBackupToast(null), 6000);
      }
    });
    return () => void unlisten.then((u) => u());
  }, []);

  const onGotoLine = useCallback((line: number) => setGotoLine(line), []);
  const onGotoHandled = useCallback(() => setGotoLine(null), []);
  const onRestored = useCallback(() => setTreeRefresh((n) => n + 1), []);

  const startBackup = useCallback(
    async (database: string) => {
      if (!selectedInstance) return;
      const path = await save({
        defaultPath: `${database}.bak`,
        filters: [{ name: "SQL Server backup", extensions: ["bak"] }],
      });
      if (!path) return;
      setBackupToast(`Backing up ${database}…`);
      await ipc.bakBackup(selectedInstance.id, database, path);
    },
    [selectedInstance],
  );

  if (!setup) return null;

  const needsWizard =
    !wizardDone && (!setup.eulaAccepted || !setup.rosettaInstalled || !setup.hasInstances);
  if (needsWizard) {
    return <FirstRunWizard status={setup} onDone={() => setWizardDone(true)} />;
  }

  return (
    <div className="shell">
      <aside className="sidebar" style={{ width: sidebarW }}>
        <div className="sidebar-drag" data-tauri-drag-region="">
          <div className="sidebar-wordmark" data-tauri-drag-region="">
            <Mark />
            <span data-tauri-drag-region="">Bakehouse</span>
          </div>
        </div>
        <div className="sidebar-section">Instances</div>
        <InstancePicker />
        <div className="sidebar-section">Objects</div>
        <ObjectTree
          onRestoreRequest={() => setShowRestore(true)}
          onBackupRequest={(db) => void startBackup(db)}
          onGenerateScripts={setScriptsFor}
          onImportCsv={(database) => setImportCsv({ database })}
          refreshToken={treeRefresh}
        />
      </aside>
      <div className="splitter-v" onMouseDown={dragSplitter("v")} />
      <main className="main">
        <div className="editor-region">
          <EditorTabs gotoLine={gotoLine} onGotoHandled={onGotoHandled} />
        </div>
        <div className="splitter-h" onMouseDown={dragSplitter("h")} />
        <div className="bottom-pane" style={{ height: bottomH }}>
          <ResultsPane
            executionId={activeTab?.executionId ?? null}
            onGotoLine={onGotoLine}
            onRunUncapped={
              activeTab?.lastSql && activeTab.sessionId && !activeTab.runningExecutionId
                ? () => {
                    const tab = activeTab;
                    void (async () => {
                      if (tab.executionId) void ipc.resultsRelease(tab.executionId);
                      const executionId = await ipc.queryRunUncapped(tab.sessionId!, tab.lastSql!);
                      useTabsStore
                        .getState()
                        .patch(tab.tabId, { runningExecutionId: executionId, executionId });
                    })();
                  }
                : undefined
            }
          />
        </div>
      </main>
      {showRestore && selectedInstance && (
        <RestoreWizard
          instanceId={selectedInstance.id}
          onClose={() => setShowRestore(false)}
          onRestored={onRestored}
        />
      )}
      {importCsv && selectedInstance && (
        <ImportCsvWizard
          instanceId={selectedInstance.id}
          presetDatabase={importCsv.database}
          onClose={() => setImportCsv(null)}
          onImported={onRestored}
        />
      )}
      {scriptsFor && selectedInstance && (
        <GenerateScriptsModal
          instanceId={selectedInstance.id}
          database={scriptsFor}
          onClose={() => setScriptsFor(null)}
          onDone={(msg) => {
            setScriptsFor(null);
            setBackupToast(msg);
            setTimeout(() => setBackupToast(null), 6000);
          }}
        />
      )}
      {backupToast && <div className="toast">{backupToast}</div>}
    </div>
  );
}

export default App;
