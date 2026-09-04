import { create } from "zustand";
import * as ipc from "../lib/ipc";
import type { InstanceInfo, InstanceStateKind } from "../lib/ipc";

interface InstancesStore {
  instances: InstanceInfo[];
  selectedId: string | null;
  pullProgress: Record<string, string>; // instanceId -> last progress line
  loaded: boolean;

  refresh: () => Promise<void>;
  select: (id: string | null) => void;
  applyStateEvent: (instanceId: string, kind: InstanceStateKind, detail?: string) => void;
  applyPullProgress: (instanceId: string, line: string) => void;
}

export const useInstancesStore = create<InstancesStore>((set, get) => ({
  instances: [],
  selectedId: null,
  pullProgress: {},
  loaded: false,

  refresh: async () => {
    const instances = await ipc.instanceList();
    set((s) => ({
      instances,
      loaded: true,
      selectedId: s.selectedId ?? instances[0]?.id ?? null,
    }));
  },

  select: (id) => set({ selectedId: id }),

  applyStateEvent: (instanceId, kind, detail) => {
    set((s) => ({
      instances: s.instances.map((i) =>
        i.id === instanceId
          ? {
              ...i,
              state:
                kind === "running"
                  ? { kind, ip: detail }
                  : kind === "failed"
                    ? { kind, detail }
                    : { kind },
            }
          : i,
      ),
    }));
    if (kind !== "pulling") {
      const { pullProgress } = get();
      if (pullProgress[instanceId]) {
        set((s) => {
          const next = { ...s.pullProgress };
          delete next[instanceId];
          return { pullProgress: next };
        });
      }
    }
  },

  applyPullProgress: (instanceId, line) =>
    set((s) => ({ pullProgress: { ...s.pullProgress, [instanceId]: line } })),
}));
