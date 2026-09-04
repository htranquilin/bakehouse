import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { EVENTS, type JobDonePayload } from "../../lib/events";
import * as ipc from "../../lib/ipc";

type Step =
  | { kind: "pick" }
  | { kind: "inspecting"; path: string }
  | { kind: "options"; staged: ipc.StagedBak }
  | { kind: "restoring"; jobId: string; stage: string; percent: number | null }
  | { kind: "done"; database: string }
  | { kind: "failed"; error: string };

export function RestoreWizard({
  instanceId,
  onClose,
  onRestored,
}: {
  instanceId: string;
  onClose: () => void;
  onRestored: () => void;
}) {
  const [step, setStep] = useState<Step>({ kind: "pick" });
  const [dbName, setDbName] = useState("");
  const [setIndex, setSetIndex] = useState(0);
  const [replace, setReplace] = useState(false);
  const stagedRef = useRef<string | null>(null);
  const dbNameRef = useRef("");
  dbNameRef.current = dbName;

  // Job events
  useEffect(() => {
    let unlistens: UnlistenFn[] = [];
    void Promise.all([
      listen<{ jobId: string; stage: string; percent: number | null }>(
        EVENTS.restoreProgress,
        (e) => {
          setStep((s) =>
            s.kind === "restoring" && s.jobId === e.payload.jobId
              ? { ...s, stage: e.payload.stage, percent: e.payload.percent }
              : s,
          );
        },
      ),
      listen<JobDonePayload>(EVENTS.jobDone, (e) => {
        setStep((s) => {
          if (s.kind !== "restoring" || s.jobId !== e.payload.jobId) return s;
          if (e.payload.ok) {
            onRestored();
            return { kind: "done", database: dbNameRef.current };
          }
          return { kind: "failed", error: e.payload.error ?? "restore failed" };
        });
      }),
    ]).then((u) => {
      unlistens = u;
    });
    return () => unlistens.forEach((u) => u());
  }, [onRestored]);

  const pickFile = async () => {
    const path = await open({
      multiple: false,
      filters: [{ name: "SQL Server backup", extensions: ["bak", "bck", "backup"] }],
    });
    if (typeof path !== "string") return;
    setStep({ kind: "inspecting", path });
    try {
      const staged = await ipc.bakInspect(instanceId, path);
      stagedRef.current = staged.stagedPath;
      setStep({ kind: "options", staged });
      setDbName(staged.sets[0]?.databaseName ?? "restored_db");
      setSetIndex(0);
    } catch (e) {
      setStep({ kind: "failed", error: ipc.errorMessage(e) });
    }
  };

  const startRestore = async (staged: ipc.StagedBak) => {
    const set = staged.sets[setIndex];
    try {
      const jobId = await ipc.bakRestore(instanceId, {
        stagedPath: staged.stagedPath,
        fileNumber: set.fileNumber,
        databaseName: dbName.trim(),
        moves: set.files
          .filter((f) => f.fileType !== "S")
          .map((f) => ({ logicalName: f.logicalName, target: f.suggestedTarget })),
        replace,
      });
      setStep({ kind: "restoring", jobId, stage: "restoring", percent: 0 });
    } catch (e) {
      setStep({ kind: "failed", error: ipc.errorMessage(e) });
    }
  };

  const close = () => {
    // Discard the staged copy if we never restored it.
    if (stagedRef.current && (step.kind === "options" || step.kind === "failed")) {
      void ipc.bakDiscardStaged(instanceId, stagedRef.current);
    }
    onClose();
  };

  return (
    <div className="modal-overlay" onClick={step.kind === "restoring" ? undefined : close}>
      <div className="modal restore-modal" onClick={(e) => e.stopPropagation()}>
        <h2>Restore a backup</h2>

        {step.kind === "pick" && (
          <>
            <p>Pick a .bak file. Bakehouse copies it into the instance and inspects it.</p>
            <button className="btn-primary" onClick={() => void pickFile()}>
              Choose .bak file…
            </button>
          </>
        )}

        {step.kind === "inspecting" && (
          <p className="bh-proofing">Copying backup into the container and reading its header…</p>
        )}

        {step.kind === "options" && (
          <OptionsStep
            staged={step.staged}
            dbName={dbName}
            setDbName={setDbName}
            setIndex={setIndex}
            setSetIndex={setSetIndex}
            replace={replace}
            setReplace={setReplace}
            onRestore={() => void startRestore(step.staged)}
          />
        )}

        {step.kind === "restoring" && (
          <>
            <p className="bh-proofing">
              {step.stage === "recovering"
                ? "Recovering the database (this stage has no percentage)…"
                : `Restoring… ${step.percent?.toFixed(0) ?? 0}%`}
            </p>
            <div className="progress-track">
              <div
                className={`progress-fill ${step.stage === "recovering" ? "indeterminate" : ""}`}
                style={{ width: `${step.stage === "recovering" ? 100 : (step.percent ?? 0)}%` }}
              />
            </div>
          </>
        )}

        {step.kind === "done" && (
          <>
            <p>
              <strong>{step.database}</strong> is restored and online.
            </p>
            <button className="btn-primary" onClick={onClose}>
              Close
            </button>
          </>
        )}

        {step.kind === "failed" && (
          <>
            <pre className="wizard-error">{step.error}</pre>
            <button className="btn-primary" onClick={close}>
              Close
            </button>
          </>
        )}
      </div>
    </div>
  );
}

function OptionsStep({
  staged,
  dbName,
  setDbName,
  setIndex,
  setSetIndex,
  replace,
  setReplace,
  onRestore,
}: {
  staged: ipc.StagedBak;
  dbName: string;
  setDbName: (v: string) => void;
  setIndex: number;
  setSetIndex: (v: number) => void;
  replace: boolean;
  setReplace: (v: boolean) => void;
  onRestore: () => void;
}) {
  const set = staged.sets[setIndex];
  const versionBlocked = set.softwareVersionMajor > staged.serverVersionMajor;
  const blocked = set.hasFilestream || versionBlocked || !dbName.trim();

  return (
    <>
      {staged.sets.length > 1 && (
        <label className="field">
          Backup set
          <select value={setIndex} onChange={(e) => setSetIndex(Number(e.target.value))}>
            {staged.sets.map((s, i) => (
              <option key={s.fileNumber} value={i}>
                #{s.fileNumber} — {s.databaseName} ({s.backupType}, {s.finishDate})
              </option>
            ))}
          </select>
        </label>
      )}
      <label className="field">
        Restore as database
        <input value={dbName} onChange={(e) => setDbName(e.target.value)} spellCheck={false} />
      </label>
      <label className="field-check">
        <input type="checkbox" checked={replace} onChange={(e) => setReplace(e.target.checked)} />
        Overwrite if it already exists (WITH REPLACE)
      </label>
      <div className="restore-files mono">
        {set.files.map((f) => (
          <div key={f.logicalName}>
            {f.logicalName} → {f.suggestedTarget}
          </div>
        ))}
      </div>
      {set.hasFilestream && (
        <pre className="wizard-error">
          This backup contains a FILESTREAM or memory-optimized filegroup. SQL Server on Linux
          cannot restore it — restore this backup on a Windows SQL Server instead.
        </pre>
      )}
      {versionBlocked && (
        <pre className="wizard-error">
          This backup was taken on SQL Server major version {set.softwareVersionMajor}, newer than
          this instance ({staged.serverVersionMajor}). SQL Server cannot restore backups to older
          versions.
        </pre>
      )}
      {set.backupType !== "Full" && (
        <pre className="wizard-error">
          This is a {set.backupType} backup. v1 restores full backups only.
        </pre>
      )}
      <button className="btn-primary" disabled={blocked || set.backupType !== "Full"} onClick={onRestore}>
        Restore
      </button>
    </>
  );
}
