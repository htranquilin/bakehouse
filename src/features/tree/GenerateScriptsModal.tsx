import { open, save } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
import * as ipc from "../../lib/ipc";

type Kind = "table" | "view" | "proc" | "fn";
const GROUPS: { kind: Kind; label: string }[] = [
  { kind: "table", label: "Tables" },
  { kind: "view", label: "Views" },
  { kind: "proc", label: "Stored Procedures" },
  { kind: "fn", label: "Functions" },
];

interface Obj extends ipc.ObjInfo {
  kind: Kind;
}

export function GenerateScriptsModal({
  instanceId,
  database,
  onClose,
  onDone,
}: {
  instanceId: string;
  database: string;
  onClose: () => void;
  onDone: (message: string) => void;
}) {
  const [objects, setObjects] = useState<Obj[] | null>(null);
  const [checked, setChecked] = useState<Set<number>>(new Set());
  const [filter, setFilter] = useState("");
  const [includeDrop, setIncludeDrop] = useState(true);
  const [includeUse, setIncludeUse] = useState(true);
  const [perObject, setPerObject] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void Promise.all(
      GROUPS.map((g) =>
        ipc.metaObjects(instanceId, database, g.kind).then((objs) =>
          objs.map((o) => ({ ...o, kind: g.kind }) as Obj),
        ),
      ),
    )
      .then((groups) => {
        const all = groups.flat();
        setObjects(all);
        // Views pre-checked — that's the everyday export.
        setChecked(new Set(all.filter((o) => o.kind === "view").map((o) => o.objectId)));
      })
      .catch((e) => setError(ipc.errorMessage(e)));
  }, [instanceId, database]);

  const visible = useMemo(
    () =>
      (objects ?? []).filter(
        (o) => !filter || `${o.schema}.${o.name}`.toLowerCase().includes(filter.toLowerCase()),
      ),
    [objects, filter],
  );

  const toggle = (id: number) =>
    setChecked((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const toggleGroup = (kind: Kind, on: boolean) =>
    setChecked((s) => {
      const next = new Set(s);
      for (const o of visible.filter((o) => o.kind === kind)) {
        if (on) next.add(o.objectId);
        else next.delete(o.objectId);
      }
      return next;
    });

  const generate = async () => {
    const selected = (objects ?? []).filter((o) => checked.has(o.objectId));
    if (selected.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      if (perObject) {
        const dir = await open({ directory: true, multiple: false, title: "Write .sql files to…" });
        if (typeof dir !== "string") {
          setBusy(false);
          return;
        }
        for (const o of selected) {
          const sql = await ipc.scriptGenerate(instanceId, database, [o.objectId], includeDrop, includeUse);
          await ipc.fileWriteSql(`${dir}/${o.schema}.${o.name}.sql`, sql);
        }
        onDone(`Wrote ${selected.length} .sql files`);
      } else {
        const path = await save({
          defaultPath: `${database}-scripts.sql`,
          filters: [{ name: "SQL", extensions: ["sql"] }],
        });
        if (!path) {
          setBusy(false);
          return;
        }
        const sql = await ipc.scriptGenerate(
          instanceId,
          database,
          selected.map((o) => o.objectId),
          includeDrop,
          includeUse,
        );
        await ipc.fileWriteSql(path, sql);
        onDone(`Scripted ${selected.length} objects to ${path.split("/").pop()}`);
      }
    } catch (e) {
      setError(ipc.errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal scripts-modal" onClick={(e) => e.stopPropagation()}>
        <h2>Generate scripts — {database}</h2>
        {!objects && !error && <p className="bh-proofing">Loading objects…</p>}
        {objects && (
          <>
            <input
              className="tree-filter"
              placeholder="Filter objects…"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              spellCheck={false}
            />
            <div className="scripts-list">
              {GROUPS.map((g) => {
                const group = visible.filter((o) => o.kind === g.kind);
                if (group.length === 0) return null;
                const checkedCount = group.filter((o) => checked.has(o.objectId)).length;
                return (
                  <div key={g.kind}>
                    <label className="field-check scripts-group">
                      <input
                        type="checkbox"
                        checked={checkedCount === group.length}
                        ref={(el) => {
                          if (el) el.indeterminate = checkedCount > 0 && checkedCount < group.length;
                        }}
                        onChange={(e) => toggleGroup(g.kind, e.target.checked)}
                      />
                      {g.label} ({checkedCount}/{group.length})
                    </label>
                    {group.map((o) => (
                      <label key={o.objectId} className="field-check scripts-item">
                        <input
                          type="checkbox"
                          checked={checked.has(o.objectId)}
                          onChange={() => toggle(o.objectId)}
                        />
                        {o.schema !== "dbo" ? `${o.schema}.` : ""}
                        {o.name}
                      </label>
                    ))}
                  </div>
                );
              })}
            </div>
            <div className="scripts-options">
              <label className="field-check">
                <input type="checkbox" checked={includeDrop} onChange={(e) => setIncludeDrop(e.target.checked)} />
                Include DROP … IF EXISTS
              </label>
              <label className="field-check">
                <input type="checkbox" checked={includeUse} onChange={(e) => setIncludeUse(e.target.checked)} />
                Include USE [{database}]
              </label>
              <label className="field-check">
                <input type="checkbox" checked={perObject} onChange={(e) => setPerObject(e.target.checked)} />
                One .sql file per object
              </label>
            </div>
          </>
        )}
        {error && <pre className="wizard-error">{error}</pre>}
        <button className="btn-primary" disabled={busy || checked.size === 0} onClick={() => void generate()}>
          {busy ? "Generating…" : `Generate (${checked.size} selected)`}
        </button>
      </div>
    </div>
  );
}
