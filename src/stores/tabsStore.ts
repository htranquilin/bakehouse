import { create } from "zustand";
import * as ipc from "../lib/ipc";

export interface Tab {
  tabId: string;
  title: string;
  instanceId: string | null;
  sessionId: string | null;
  spid: number | null;
  database: string;
  contents: string;
  dirty: boolean;
  filePath: string | null;
  trancount: number;
  runningExecutionId: string | null;
  /** Last execution whose results this tab shows. */
  executionId: string | null;
  /** Exact SQL of the last run (selection or buffer) for uncapped re-runs. */
  lastSql: string | null;
}

let tabCounter = 0;
const newTabId = () => `tab-${Date.now()}-${tabCounter++}`;

interface TabsStore {
  tabs: Tab[];
  activeTabId: string | null;

  openTab: (partial?: Partial<Tab>) => Tab;
  closeTab: (tabId: string) => void;
  activate: (tabId: string) => void;
  patch: (tabId: string, patch: Partial<Tab>) => void;
  patchBySession: (sessionId: string, patch: Partial<Tab>) => void;
  connectTab: (tabId: string, instanceId: string, database?: string) => Promise<void>;
}

export const useTabsStore = create<TabsStore>((set, get) => ({
  tabs: [],
  activeTabId: null,

  openTab: (partial) => {
    const tab: Tab = {
      tabId: newTabId(),
      title: `Query ${get().tabs.length + 1}`,
      instanceId: null,
      sessionId: null,
      spid: null,
      database: "master",
      contents: "",
      dirty: false,
      filePath: null,
      trancount: 0,
      runningExecutionId: null,
      executionId: null,
      lastSql: null,
      ...partial,
    };
    set((s) => ({ tabs: [...s.tabs, tab], activeTabId: tab.tabId }));
    return tab;
  },

  closeTab: (tabId) => {
    const tab = get().tabs.find((t) => t.tabId === tabId);
    if (tab?.sessionId) void ipc.sessionClose(tab.sessionId);
    if (tab?.executionId) void ipc.resultsRelease(tab.executionId);
    void ipc.buffersDelete(tabId);
    set((s) => {
      const tabs = s.tabs.filter((t) => t.tabId !== tabId);
      return {
        tabs,
        activeTabId: s.activeTabId === tabId ? (tabs[tabs.length - 1]?.tabId ?? null) : s.activeTabId,
      };
    });
  },

  activate: (tabId) => set({ activeTabId: tabId }),

  patch: (tabId, patch) =>
    set((s) => ({ tabs: s.tabs.map((t) => (t.tabId === tabId ? { ...t, ...patch } : t)) })),

  patchBySession: (sessionId, patch) =>
    set((s) => ({ tabs: s.tabs.map((t) => (t.sessionId === sessionId ? { ...t, ...patch } : t)) })),

  connectTab: async (tabId, instanceId, database) => {
    const info = await ipc.sessionOpen(instanceId, database);
    get().patch(tabId, {
      instanceId,
      sessionId: info.sessionId,
      spid: info.spid,
      database: info.database,
      trancount: info.trancount,
    });
  },
}));
