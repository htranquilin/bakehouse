import { useCallback, useEffect, useState } from "react";
import * as ipc from "../../lib/ipc";
import { useInstancesStore } from "../../stores/instancesStore";
import { useTabsStore } from "../../stores/tabsStore";

type Kind = "table" | "view" | "proc" | "fn";
const FOLDERS: { kind: Kind; label: string }[] = [
  { kind: "table", label: "Tables" },
  { kind: "view", label: "Views" },
  { kind: "proc", label: "Stored Procedures" },
  { kind: "fn", label: "Functions" },
];

interface Menu {
  x: number;
  y: number;
  items: { label: string; action: () => void }[];
}

export function ObjectTree({
  onRestoreRequest,
  onBackupRequest,
  onGenerateScripts,
  onImportCsv,
  refreshToken,
}: {
  onRestoreRequest: () => void;
  onBackupRequest: (database: string) => void;
  onGenerateScripts: (database: string) => void;
  onImportCsv: (database?: string) => void;
  refreshToken: number;
}) {
  const instances = useInstancesStore((s) => s.instances);
  const selectedId = useInstancesStore((s) => s.selectedId);
  const instance = instances.find((i) => i.id === selectedId);
  const running = instance?.state.kind === "running";

  const [databases, setDatabases] = useState<string[]>([]);
  const [filter, setFilter] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [children, setChildren] = useState<Record<string, ipc.ObjInfo[]>>({});
  const [columns, setColumns] = useState<Record<string, ipc.ColInfo[]>>({});
  const [menu, setMenu] = useState<Menu | null>(null);
  const openTab = useTabsStore((s) => s.openTab);

  const loadDatabases = useCallback(() => {
    if (instance && running) {
      ipc.dbList(instance.id).then(setDatabases).catch(() => setDatabases([]));
    } else {
      setDatabases([]);
    }
  }, [instance, running]);

  useEffect(loadDatabases, [loadDatabases, refreshToken]);

  useEffect(() => {
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, []);

  if (!instance) return <div className="tree-empty">No instance selected.</div>;
  if (!running) return <div className="tree-empty">Start the instance to browse objects.</div>;

  const toggle = (key: string, load?: () => void) => {
    setExpanded((s) => {
      const next = new Set(s);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
        load?.();
      }
      return next;
    });
  };

  const loadObjects = (db: string, kind: Kind) => {
    const key = `${db}/${kind}`;
    if (children[key]) return;
    void ipc.metaObjects(instance.id, db, kind).then((objs) =>
      setChildren((s) => ({ ...s, [key]: objs })),
    );
  };

  const loadColumns = (db: string, obj: ipc.ObjInfo) => {
    const key = `${db}/${obj.objectId}`;
    if (columns[key]) return;
    void ipc.metaColumns(instance.id, db, obj.objectId).then((cols) =>
      setColumns((s) => ({ ...s, [key]: cols })),
    );
  };

  const openSqlTab = (title: string, contents: string, database: string) => {
    // Stamp the instance so the tab connects to THIS instance even when
    // several are running and another one is first in the list.
    openTab({ title, contents, database, instanceId: instance.id });
  };

  const objectMenu = (e: React.MouseEvent, db: string, kind: Kind, obj: ipc.ObjInfo) => {
    e.preventDefault();
    e.stopPropagation();
    const fq = `[${obj.schema}].[${obj.name}]`;
    const items: Menu["items"] = [];
    if (kind === "table" || kind === "view") {
      items.push({
        label: "Select Top 1000",
        action: () =>
          openSqlTab(
            obj.name,
            `SELECT TOP (1000) *\nFROM ${fq};`,
            db,
          ),
      });
    }
    items.push({
      label: "Script as CREATE",
      action: () =>
        void ipc
          .metaScriptObject(instance.id, db, obj.objectId, false)
          .then((sql) => openSqlTab(`${obj.name} (create)`, sql, db)),
    });
    if (kind !== "table") {
      items.push({
        label: "Script as ALTER",
        action: () =>
          void ipc
            .metaScriptObject(instance.id, db, obj.objectId, true)
            .then((sql) => openSqlTab(`${obj.name} (alter)`, sql, db)),
      });
    }
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  const dbMenu = (e: React.MouseEvent, db: string) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "New query on this database", action: () => openSqlTab(`Query — ${db}`, "", db) },
        { label: "Import CSV files…", action: () => onImportCsv(db) },
        { label: "Generate scripts…", action: () => onGenerateScripts(db) },
        { label: "Backup database…", action: () => onBackupRequest(db) },
        {
          label: "Refresh",
          action: () => {
            setChildren({});
            setColumns({});
            loadDatabases();
          },
        },
      ],
    });
  };

  const match = (name: string) => name.toLowerCase().includes(filter.toLowerCase());

  return (
    <div className="object-tree">
      <div className="tree-actions">
        <input
          className="tree-filter"
          placeholder="Filter objects…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <button className="btn-ghost" title="Restore a .bak backup" onClick={onRestoreRequest}>
          ⤓ .bak
        </button>
        <button className="btn-ghost" title="Import CSV files as tables" onClick={() => onImportCsv()}>
          ⇪ CSV
        </button>
      </div>
      <div className="tree-scroll">
        {databases.map((db) => (
          <div key={db}>
            <div
              className="tree-node tree-db"
              onClick={() => toggle(`db/${db}`)}
              onContextMenu={(e) => dbMenu(e, db)}
            >
              <span className="tree-arrow">{expanded.has(`db/${db}`) ? "▾" : "▸"}</span> {db}
            </div>
            {expanded.has(`db/${db}`) &&
              FOLDERS.map((f) => {
                const key = `${db}/${f.kind}`;
                const objs = (children[key] ?? []).filter((o) => !filter || match(o.name));
                return (
                  <div key={key}>
                    <div
                      className="tree-node tree-folder"
                      onClick={() => toggle(`f/${key}`, () => loadObjects(db, f.kind))}
                    >
                      <span className="tree-arrow">{expanded.has(`f/${key}`) ? "▾" : "▸"}</span>{" "}
                      {f.label}
                      {children[key] ? ` (${objs.length})` : ""}
                    </div>
                    {expanded.has(`f/${key}`) &&
                      objs.map((o) => {
                        const okey = `${db}/${o.objectId}`;
                        return (
                          <div key={o.objectId}>
                            <div
                              className="tree-node tree-object"
                              onClick={() =>
                                f.kind === "table" || f.kind === "view"
                                  ? toggle(`o/${okey}`, () => loadColumns(db, o))
                                  : undefined
                              }
                              onDoubleClick={() =>
                                (f.kind === "table" || f.kind === "view") &&
                                openSqlTab(o.name, `SELECT TOP (1000) *\nFROM [${o.schema}].[${o.name}];`, db)
                              }
                              onContextMenu={(e) => objectMenu(e, db, f.kind, o)}
                              title={`${o.schema}.${o.name}`}
                            >
                              {(f.kind === "table" || f.kind === "view") && (
                                <span className="tree-arrow">
                                  {expanded.has(`o/${okey}`) ? "▾" : "▸"}
                                </span>
                              )}{" "}
                              {o.schema !== "dbo" ? `${o.schema}.` : ""}
                              {o.name}
                            </div>
                            {expanded.has(`o/${okey}`) &&
                              (columns[okey] ?? []).map((c) => (
                                <div key={c.name} className="tree-node tree-column">
                                  {c.isPk ? "🔑 " : ""}
                                  {c.name}{" "}
                                  <span className="tree-type">
                                    {c.dataType}
                                    {c.nullable ? ", null" : ""}
                                  </span>
                                </div>
                              ))}
                          </div>
                        );
                      })}
                  </div>
                );
              })}
          </div>
        ))}
      </div>
      {menu && (
        <div className="context-menu" style={{ left: menu.x, top: menu.y }}>
          {menu.items.map((item) => (
            <button
              key={item.label}
              className="context-menu-item"
              onClick={() => {
                item.action();
                setMenu(null);
              }}
            >
              {item.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
