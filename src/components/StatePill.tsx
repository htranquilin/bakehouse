import type { InstanceState } from "../lib/ipc";

/** Brand rule: state is always color + glyph, never color alone. */
const STATES: Record<
  string,
  { label: string; varName: string; glyph: "dot" | "ring" | "hollow" | "triangle" }
> = {
  running: { label: "Running", varName: "--bh-running", glyph: "dot" },
  starting: { label: "Starting", varName: "--bh-starting", glyph: "ring" },
  pulling: { label: "Pulling image", varName: "--bh-pulling", glyph: "ring" },
  waitingForSql: { label: "Waiting for SQL", varName: "--bh-starting", glyph: "ring" },
  stopping: { label: "Stopping", varName: "--bh-stopped", glyph: "ring" },
  stopped: { label: "Stopped", varName: "--bh-stopped", glyph: "hollow" },
  failed: { label: "Failed", varName: "--bh-failed", glyph: "triangle" },
};

export function StatePill({ state }: { state: InstanceState }) {
  const s = STATES[state.kind] ?? STATES.stopped;
  const color = `var(${s.varName})`;
  const animated = s.glyph === "ring";
  return (
    <span className="state-pill" style={{ color }}>
      <span className={`state-glyph ${animated ? "bh-proofing" : ""}`} aria-hidden>
        {s.glyph === "dot" && <span className="glyph-dot" style={{ background: color }} />}
        {s.glyph === "ring" && <span className="glyph-ring" style={{ borderColor: color }} />}
        {s.glyph === "hollow" && <span className="glyph-ring" style={{ borderColor: color }} />}
        {s.glyph === "triangle" && <span className="glyph-triangle" style={{ borderBottomColor: color }} />}
      </span>
      {s.label}
    </span>
  );
}
