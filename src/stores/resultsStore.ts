import { create } from "zustand";

export interface ResultSetMeta {
  resultSetId: number;
  columns: { name: string; sqlType: string }[];
  rowCount: number;
  finished: boolean;
  truncated?: boolean;
}

export interface QueryMessage {
  kind: "info" | "error" | "rowcount";
  number?: number;
  severity?: number;
  line?: number;
  procedure?: string;
  text: string;
}

export interface ExecutionMeta {
  executionId: string;
  sessionId: string;
  running: boolean;
  resultSets: ResultSetMeta[];
  messages: QueryMessage[];
  elapsedMs: number | null;
  totalRows: number | null;
  success: boolean | null;
}

interface ResultsStore {
  executions: Record<string, ExecutionMeta>;

  start: (executionId: string, sessionId: string) => void;
  addResultSet: (executionId: string, resultSetId: number, columns: ResultSetMeta["columns"]) => void;
  setRowCount: (resultSetId: number, executionId: string, rowCount: number, truncated?: boolean) => void;
  addMessage: (executionId: string, msg: QueryMessage) => void;
  finish: (executionId: string, elapsedMs: number, totalRows: number, success: boolean) => void;
  drop: (executionId: string) => void;
}

export const useResultsStore = create<ResultsStore>((set) => ({
  executions: {},

  start: (executionId, sessionId) =>
    set((s) => ({
      executions: {
        ...s.executions,
        [executionId]: {
          executionId,
          sessionId,
          running: true,
          resultSets: [],
          messages: [],
          elapsedMs: null,
          totalRows: null,
          success: null,
        },
      },
    })),

  addResultSet: (executionId, resultSetId, columns) =>
    set((s) => {
      const exec = s.executions[executionId];
      if (!exec) return s;
      return {
        executions: {
          ...s.executions,
          [executionId]: {
            ...exec,
            resultSets: [...exec.resultSets, { resultSetId, columns, rowCount: 0, finished: false }],
          },
        },
      };
    }),

  setRowCount: (resultSetId, executionId, rowCount, truncated) =>
    set((s) => {
      const exec = s.executions[executionId];
      if (!exec) return s;
      return {
        executions: {
          ...s.executions,
          [executionId]: {
            ...exec,
            resultSets: exec.resultSets.map((r) =>
              r.resultSetId === resultSetId
                ? { ...r, rowCount, truncated: truncated ?? r.truncated }
                : r,
            ),
          },
        },
      };
    }),

  addMessage: (executionId, msg) =>
    set((s) => {
      const exec = s.executions[executionId];
      if (!exec) return s;
      return {
        executions: {
          ...s.executions,
          [executionId]: { ...exec, messages: [...exec.messages, msg] },
        },
      };
    }),

  finish: (executionId, elapsedMs, totalRows, success) =>
    set((s) => {
      const exec = s.executions[executionId];
      if (!exec) return s;
      return {
        executions: {
          ...s.executions,
          [executionId]: { ...exec, running: false, elapsedMs, totalRows, success },
        },
      };
    }),

  drop: (executionId) =>
    set((s) => {
      const { [executionId]: _, ...rest } = s.executions;
      return { executions: rest };
    }),
}));
