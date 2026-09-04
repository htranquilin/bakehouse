// Single place where all app-level Tauri event listeners are registered.
// Called once from App.tsx.

import { listen } from "@tauri-apps/api/event";
import { EVENTS, type InstanceStatePayload } from "./events";
import { useInstancesStore } from "../stores/instancesStore";
import { useResultsStore } from "../stores/resultsStore";
import { useTabsStore } from "../stores/tabsStore";
import type { InstanceStateKind } from "./ipc";

let started = false;

export function bootstrapEvents() {
  if (started) return;
  started = true;

  void listen<InstanceStatePayload>(EVENTS.instanceState, (e) => {
    useInstancesStore
      .getState()
      .applyStateEvent(e.payload.instanceId, e.payload.state as InstanceStateKind, e.payload.detail);
  });

  void listen<{ jobId: string; line: string }>(EVENTS.imagePullProgress, (e) => {
    useInstancesStore.getState().applyPullProgress(e.payload.jobId, e.payload.line);
  });

  // ---- Query lifecycle ----

  void listen<{ executionId: string; sessionId: string }>(EVENTS.queryStarted, (e) => {
    useResultsStore.getState().start(e.payload.executionId, e.payload.sessionId);
  });

  void listen<{ executionId: string; resultSetId: number; columns: { name: string; sqlType: string }[] }>(
    EVENTS.queryResultSet,
    (e) => {
      useResultsStore
        .getState()
        .addResultSet(e.payload.executionId, e.payload.resultSetId, e.payload.columns);
    },
  );

  void listen<{ executionId: string; resultSetId: number; rowCount: number; truncated: boolean }>(
    EVENTS.queryRows,
    (e) => {
      useResultsStore
        .getState()
        .setRowCount(e.payload.resultSetId, e.payload.executionId, e.payload.rowCount, e.payload.truncated);
    },
  );

  void listen<{ executionId: string } & import("../stores/resultsStore").QueryMessage>(
    EVENTS.queryMessage,
    (e) => {
      const { executionId, ...msg } = e.payload;
      useResultsStore.getState().addMessage(executionId, msg);
    },
  );

  void listen<{
    executionId: string;
    elapsedMs: number;
    totalRows: number;
    trancount: number;
    database: string;
    success: boolean;
  }>(EVENTS.queryFinished, (e) => {
    const p = e.payload;
    useResultsStore.getState().finish(p.executionId, p.elapsedMs, p.totalRows, p.success);
    const tabs = useTabsStore.getState();
    const tab = tabs.tabs.find((t) => t.runningExecutionId === p.executionId);
    if (tab) {
      tabs.patch(tab.tabId, {
        runningExecutionId: null,
        trancount: p.trancount,
        // A batch may have run USE; keep the dropdown in sync.
        ...(p.database ? { database: p.database } : {}),
      });
    }
  });

  void listen<{ sessionId: string; newSpid: number }>(EVENTS.sessionReset, (e) => {
    useTabsStore.getState().patchBySession(e.payload.sessionId, {
      spid: e.payload.newSpid,
      trancount: 0,
      runningExecutionId: null,
    });
  });
}
