import { useState } from "react";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import * as ipc from "../../lib/ipc";
import { useInstancesStore } from "../../stores/instancesStore";
import { StatePill } from "../../components/StatePill";

export function InstancePicker() {
  const instances = useInstancesStore((s) => s.instances);
  const selectedId = useInstancesStore((s) => s.selectedId);
  const select = useInstancesStore((s) => s.select);
  const pullProgress = useInstancesStore((s) => s.pullProgress);
  const refresh = useInstancesStore((s) => s.refresh);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [newVersion, setNewVersion] = useState("2022");
  const [versions, setVersions] = useState<ipc.SqlVersionInfo[]>([]);
  const [logsFor, setLogsFor] = useState<{ id: string; text: string } | null>(null);
  const [settingsFor, setSettingsFor] = useState<ipc.InstanceInfo | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const flash = (msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 4000);
  };

  const exportCompose = async (inst: ipc.InstanceInfo) => {
    const dir = await open({ directory: true, multiple: false, title: "Export compose file to…" });
    if (typeof dir !== "string") return;
    const includePassword = await ask(
      "Include the sa password in a .env file next to the compose file?",
      { title: "Bakehouse", okLabel: "Include it", cancelLabel: "Leave it out" },
    );
    const files = await ipc.instanceExportCompose(inst.id, dir, includePassword);
    flash(`Exported ${files.join(", ")}`);
  };

  const copyPassword = async (inst: ipc.InstanceInfo) => {
    const pw = await ipc.instanceRevealPassword(inst.id);
    await writeText(pw);
    flash("sa password copied to clipboard");
  };

  const copyConnectionInfo = async (inst: ipc.InstanceInfo) => {
    if (inst.state.kind !== "running" || !inst.state.ip) {
      flash("Start the instance first — the host IP only exists while it runs");
      return;
    }
    const ip = inst.state.ip;
    const pw = await ipc.instanceRevealPassword(inst.id);
    await writeText(
      [
        `# ${inst.name} (Bakehouse) — valid while the instance is running; IP changes on restart`,
        `Server: ${ip},1433`,
        `User: sa`,
        `Password: ${pw}`,
        `Encrypt: trust server certificate (self-signed)`,
        ``,
        `Connection string: Server=${ip},1433;User Id=sa;Password=${pw};TrustServerCertificate=True`,
        `sqlcmd: sqlcmd -S ${ip},1433 -U sa -P '${pw}' -C`,
        ``,
        `Machine-readable (all instances, kept current): ~/.bakehouse/connections.json`,
      ].join("\n"),
    );
    flash("Connection info copied to clipboard");
  };

  const toggle = (inst: ipc.InstanceInfo) => {
    if (inst.state.kind === "stopped" || inst.state.kind === "failed") {
      void ipc.instanceStart(inst.id);
    } else if (inst.state.kind === "running") {
      void ipc.instanceStop(inst.id);
    }
  };

  const startCreating = async () => {
    setCreating(true);
    if (versions.length === 0) {
      await ipc.sqlVersions().then(setVersions).catch(() => {});
    }
  };

  const create = async () => {
    const name = newName.trim();
    if (!name) return;
    await ipc.instanceCreate(name, undefined, newVersion);
    setNewName("");
    setCreating(false);
    await refresh();
  };

  const remove = async (inst: ipc.InstanceInfo) => {
    const ok = await ask(`Delete instance "${inst.name}" and its databases? This cannot be undone.`, {
      title: "Bakehouse",
      kind: "warning",
      okLabel: "Delete",
      cancelLabel: "Cancel",
    });
    if (!ok) return;
    await ipc.instanceDelete(inst.id, true);
    await refresh();
  };

  const showLogs = async (id: string) => {
    try {
      setLogsFor({ id, text: await ipc.diagInstanceLogs(id) });
    } catch (e) {
      setLogsFor({ id, text: ipc.errorMessage(e) });
    }
  };

  return (
    <div className="instance-picker">
      {instances.map((inst) => (
        <div
          key={inst.id}
          className={`instance-row ${inst.id === selectedId ? "selected" : ""}`}
          onClick={() => select(inst.id)}
        >
          <div className="instance-row-main">
            <span className="instance-name">{inst.name}</span>
            <StatePill state={inst.state} />
            {inst.state.kind === "pulling" && pullProgress[inst.id] && (
              <span className="instance-progress">{pullProgress[inst.id]}</span>
            )}
            {inst.state.kind === "failed" && (
              <button className="btn-link" onClick={(e) => { e.stopPropagation(); void showLogs(inst.id); }}>
                Show logs
              </button>
            )}
          </div>
          <div className="instance-row-actions">
            <button
              className="btn-ghost"
              title={inst.state.kind === "running" ? "Stop" : "Start"}
              disabled={!["stopped", "failed", "running"].includes(inst.state.kind)}
              onClick={(e) => {
                e.stopPropagation();
                toggle(inst);
              }}
            >
              {inst.state.kind === "running" ? "◼" : "▶"}
            </button>
            <button
              className="btn-ghost"
              title="Instance settings"
              onClick={(e) => {
                e.stopPropagation();
                setSettingsFor(inst);
              }}
            >
              ⋯
            </button>
          </div>
        </div>
      ))}

      {creating ? (
        <div className="instance-create">
          <input
            autoFocus
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void create();
              if (e.key === "Escape") setCreating(false);
            }}
            placeholder="Instance name"
            spellCheck={false}
          />
          {versions.length > 0 && (
            <select
              className="db-select instance-version"
              value={newVersion}
              onChange={(e) => setNewVersion(e.target.value)}
            >
              {versions.map((v) => (
                <option key={v.id} value={v.id}>
                  {v.label}
                  {v.recommended ? " — recommended" : ""}
                </option>
              ))}
            </select>
          )}
        </div>
      ) : (
        <button className="btn-ghost instance-add" onClick={() => void startCreating()}>
          + New instance
        </button>
      )}

      {logsFor && (
        <div className="modal-overlay" onClick={() => setLogsFor(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2>Container logs</h2>
            <pre className="modal-logs">{logsFor.text || "(no output)"}</pre>
            <button className="btn-primary" onClick={() => setLogsFor(null)}>
              Close
            </button>
          </div>
        </div>
      )}

      {settingsFor && (
        <InstanceSettings
          inst={settingsFor}
          onClose={() => setSettingsFor(null)}
          onExport={() => void exportCompose(settingsFor)}
          onCopyPassword={() => void copyPassword(settingsFor)}
          onCopyConnection={() => void copyConnectionInfo(settingsFor)}
          onDelete={() => {
            setSettingsFor(null);
            void remove(settingsFor);
          }}
          onSaved={async () => {
            await refresh();
            setSettingsFor(null);
          }}
        />
      )}

      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

function InstanceSettings({
  inst,
  onClose,
  onExport,
  onCopyPassword,
  onCopyConnection,
  onDelete,
  onSaved,
}: {
  inst: ipc.InstanceInfo;
  onClose: () => void;
  onExport: () => void;
  onCopyPassword: () => void;
  onCopyConnection: () => void;
  onDelete: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState(inst.name);
  const [memory, setMemory] = useState(String(inst.memoryMb));

  const save = async () => {
    const memoryMb = Math.max(2048, parseInt(memory, 10) || inst.memoryMb);
    await ipc.instanceUpdate(inst.id, { name: name.trim() || inst.name, memoryMb });
    onSaved();
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Instance settings</h2>
        <label className="field">
          Name
          <input value={name} onChange={(e) => setName(e.target.value)} spellCheck={false} />
        </label>
        <label className="field">
          Container memory (MB) — applies on next start
          <input value={memory} onChange={(e) => setMemory(e.target.value)} inputMode="numeric" />
        </label>
        <div className="settings-meta mono">
          {inst.image.split("@")[0]}
          <br />
          SQL memory limit: {inst.sqlMemoryMb} MB · connect as sa
          {inst.state.kind === "running" && inst.state.ip && (
            <>
              <br />
              host: {inst.state.ip},1433 · external tools: ~/.bakehouse/connections.json
            </>
          )}
        </div>
        <div className="settings-actions">
          <button className="btn-ghost" onClick={onCopyConnection} disabled={inst.state.kind !== "running"}>
            Copy connection info
          </button>
          <button className="btn-ghost" onClick={onCopyPassword}>
            Copy sa password
          </button>
          <button className="btn-ghost" onClick={onExport}>
            Export as compose file…
          </button>
          <button className="btn-ghost settings-delete" onClick={onDelete}>
            Delete instance…
          </button>
        </div>
        <button className="btn-primary" onClick={() => void save()}>
          Save
        </button>
      </div>
    </div>
  );
}
