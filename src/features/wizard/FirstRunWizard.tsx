import { useEffect, useState } from "react";
import * as ipc from "../../lib/ipc";
import { useInstancesStore } from "../../stores/instancesStore";
import { Mark } from "../../components/Mark";

type Step = "eula" | "rosetta" | "create" | "firstStart";

/**
 * First run: accept the SQL Server EULA, verify Rosetta, create the first
 * instance, then watch it pull + start. The instance pull/start progress is
 * rendered by the instance list once the wizard hands off.
 */
export function FirstRunWizard({
  status,
  onDone,
}: {
  status: ipc.SetupStatus;
  onDone: () => void;
}) {
  const [step, setStep] = useState<Step>(() =>
    !status.eulaAccepted ? "eula" : !status.rosettaInstalled ? "rosetta" : "create",
  );
  const [name, setName] = useState("Local SQL Server");
  const [versions, setVersions] = useState<ipc.SqlVersionInfo[]>([]);
  const [version, setVersion] = useState("2022");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    ipc.sqlVersions().then(setVersions).catch(() => {});
  }, []);
  const instances = useInstancesStore((s) => s.instances);
  const pullProgress = useInstancesStore((s) => s.pullProgress);
  const refresh = useInstancesStore((s) => s.refresh);

  const created = instances[0];
  const state = created?.state;

  useEffect(() => {
    if (step === "firstStart" && state?.kind === "running") onDone();
  }, [step, state?.kind, onDone]);

  const acceptEula = async () => {
    setBusy(true);
    try {
      await ipc.setupAcceptEula();
      setStep(status.rosettaInstalled ? "create" : "rosetta");
    } catch (e) {
      setError(ipc.errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const recheckRosetta = async () => {
    const s = await ipc.setupStatus();
    if (s.rosettaInstalled) setStep("create");
  };

  const createAndStart = async () => {
    setBusy(true);
    setError(null);
    try {
      const info = await ipc.instanceCreate(name.trim() || "Local SQL Server", undefined, version);
      await refresh();
      await ipc.instanceStart(info.id);
      setStep("firstStart");
    } catch (e) {
      setError(ipc.errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="wizard" data-tauri-drag-region="">
      <div className="wizard-card">
        <div className="wizard-brand">
          <span style={{ color: "var(--bh-accent)" }}>
            <Mark size={28} />
          </span>
        </div>

        {step === "eula" && (
          <>
            <h1>Warm up the oven.</h1>
            <p>
              Bakehouse runs Microsoft SQL Server (Developer Edition) in a local container. Using it
              requires accepting Microsoft's license terms.
            </p>
            <p className="wizard-fine">
              Developer Edition is free for development and testing, not production. Read the{" "}
              <a
                href="https://go.microsoft.com/fwlink/?linkid=857698"
                target="_blank"
                rel="noreferrer"
              >
                license terms
              </a>
              .
            </p>
            <button className="btn-primary" disabled={busy} onClick={() => void acceptEula()}>
              Accept and continue
            </button>
          </>
        )}

        {step === "rosetta" && (
          <>
            <h1>Rosetta is required.</h1>
            <p>
              SQL Server ships only for x86-64, so this Mac needs Apple's Rosetta 2 translation
              layer. Install it from Terminal, then come back:
            </p>
            <pre className="wizard-code">softwareupdate --install-rosetta --agree-to-license</pre>
            <button className="btn-primary" onClick={() => void recheckRosetta()}>
              I've installed it — check again
            </button>
          </>
        )}

        {step === "create" && (
          <>
            <h1>Name your first instance.</h1>
            <p>
              Bakehouse will download SQL Server 2022 (~1.6 GB, one time) and start it in an
              isolated container. Nothing keeps running after you quit the app.
            </p>
            <input
              className="wizard-input"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Instance name"
              spellCheck={false}
            />
            {versions.length > 0 && (
              <label className="field">
                SQL Server version
                <select value={version} onChange={(e) => setVersion(e.target.value)}>
                  {versions.map((v) => (
                    <option key={v.id} value={v.id}>
                      {v.label}
                      {v.recommended ? " — recommended" : ""}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {error && <pre className="wizard-error">{error}</pre>}
            <button className="btn-primary" disabled={busy} onClick={() => void createAndStart()}>
              Create instance
            </button>
          </>
        )}

        {step === "firstStart" && created && (
          <>
            <h1>Proofing…</h1>
            <p>
              {state?.kind === "pulling"
                ? "Downloading the SQL Server image. This happens once."
                : state?.kind === "failed"
                  ? "The instance failed to start."
                  : "Starting SQL Server."}
            </p>
            {state?.kind === "pulling" && pullProgress[created.id] && (
              <pre className="wizard-code">{pullProgress[created.id]}</pre>
            )}
            {state?.kind === "failed" && (
              <>
                <pre className="wizard-error">{state.detail}</pre>
                <button
                  className="btn-primary"
                  onClick={() => void ipc.instanceStart(created.id)}
                >
                  Retry
                </button>
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}
