import { useEffect, useState } from "react";
import { Api, type Operation, type Server } from "./api";
interface CloudIdlePolicy {
  enabled: boolean;
  minutes: number;
  external_manager_confirmed: boolean;
  last_error?: string | null;
  wake_pending?: boolean;
}
export default function CloudIdleSettings({
  api,
  server,
  localManager = false,
}: {
  api: Api;
  server: Server;
  localManager?: boolean;
}) {
  const needsInstallation = ["awaiting_bootstrap", "provisioning"].includes(
    server.state,
  );
  const [policy, setPolicy] = useState<CloudIdlePolicy | null>(null),
    [enabled, setEnabled] = useState(false),
    [minutes, setMinutes] = useState(15),
    [confirmed, setConfirmed] = useState(false),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [message, setMessage] = useState(""),
    [operation, setOperation] = useState<string | null>(null),
    [starting, setStarting] = useState(false);
  useEffect(() => {
    let active = true;
    let initialized = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    setPolicy(null);
    setEnabled(false);
    setMinutes(15);
    setConfirmed(localManager);
    setError("");
    setMessage("");
    setOperation(null);
    setStarting(false);
    const refresh = async () => {
      try {
        const value = await api.request<CloudIdlePolicy>(
          `/servers/${server.id}/cloud-idle-policy`,
        );
        if (!active) return;
        setPolicy(value);
        // Refresh intent and errors without overwriting settings being edited.
        if (!initialized) {
          setEnabled(value.enabled);
          setMinutes(value.minutes);
          setConfirmed(localManager || value.external_manager_confirmed);
          setError("");
          initialized = true;
        }
      } catch (e) {
        if (active && !initialized) setError(String(e));
      }
      if (active) timer = setTimeout(() => void refresh(), 5000);
    };
    void refresh();
    return () => {
      active = false;
      if (timer) clearTimeout(timer);
    };
  }, [api, server.id, localManager]);
  useEffect(() => {
    if (!operation) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const check = async () => {
      try {
        const result = await api.request<Operation>(`/operations/${operation}`);
        if (!active) return;
        if (result.status === "succeeded") {
          setMessage("Server is running.");
          setPolicy((p) => (p ? { ...p, wake_pending: false } : p));
          setOperation(null);
          setStarting(false);
        } else if (
          ["failed", "interrupted", "cancelled"].includes(result.status)
        ) {
          setMessage("");
          setError(
            result.message ||
              "Server could not start. Check Activity and retry.",
          );
          setOperation(null);
          setStarting(false);
        } else {
          setMessage(result.message || "Starting the VM and game…");
          timer = setTimeout(() => void check(), 2000);
        }
      } catch (e) {
        if (active) {
          setMessage("");
          setError(
            `Could not check startup: ${String(e)}. Check Activity before retrying.`,
          );
          setOperation(null);
          setStarting(false);
        }
      }
    };
    void check();
    return () => {
      active = false;
      if (timer) clearTimeout(timer);
    };
  }, [api, operation]);
  return (
    <section className="panel detail-panel">
      <h3>Azure idle cost saving</h3>
      <p>This configures both the Remote and this Manager:</p>
      <ol className="small">
        <li>
          The Remote checks for players, saves the world and stops the game.
        </li>
        <li>
          This Manager deallocates the VM. Compute charges stop; disk and public
          IP charges continue.
        </li>
      </ol>
      <p className="muted small">
        {localManager
          ? "Keep this app open to deallocate the VM. If you close it, the Remote can still stop the game, but the VM stays allocated and billable."
          : "Keep this Manager running on a separate computer. If it is offline, the Remote can still stop the game, but cannot deallocate the VM."}
      </p>
      <p className="small">
        To play again, use Start server in this Manager; joining the game cannot
        wake a deallocated VM.
      </p>
      {needsInstallation && (
        <p className="muted small">
          Install the Remote and game in Infrastructure first. Then enable this
          setting to stop compute charges while nobody is playing.
        </p>
      )}
      <form
        className="form"
        onSubmit={async (e) => {
          e.preventDefault();
          if (
            enabled &&
            (!confirmed || (needsInstallation && !policy?.enabled))
          )
            return;
          setBusy(true);
          setError("");
          setMessage("");
          try {
            const value = await api.request<CloudIdlePolicy>(
              `/servers/${server.id}/cloud-idle-policy`,
              "PUT",
              { enabled, minutes, external_manager_confirmed: confirmed },
            );
            setPolicy(value);
            setMessage("Cost-saving settings saved.");
          } catch (e) {
            setError(String(e));
          } finally {
            setBusy(false);
          }
        }}
      >
        <label className="checkbox">
          <input
            type="checkbox"
            checked={enabled}
            disabled={
              !policy ||
              busy ||
              starting ||
              (needsInstallation && !policy.enabled)
            }
            onChange={(e) => {
              setEnabled(e.target.checked);
              setMessage("");
            }}
          />
          Deallocate when no players are connected
        </label>
        <label className="field">
          <span>Minutes without players</span>
          <input
            type="number"
            min={1}
            max={1440}
            required
            value={minutes}
            disabled={!enabled || busy || starting}
            onChange={(e) => {
              setMinutes(Number(e.target.value));
              setMessage("");
            }}
          />
        </label>
        {enabled && (
          <>
            {!localManager && (
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={confirmed}
                  disabled={busy || starting}
                  onChange={(e) => setConfirmed(e.target.checked)}
                />
                This Manager runs outside the game server’s VM.
              </label>
            )}
          </>
        )}
        {policy?.last_error && (
          <p role="alert" className="inline-error">
            {policy.last_error}
          </p>
        )}
        {error && (
          <p role="alert" className="inline-error">
            {error}
          </p>
        )}
        {message && <p role="status">{message}</p>}
        <div className="button-row">
          <button
            className="button secondary"
            disabled={
              !policy ||
              busy ||
              starting ||
              (enabled &&
                (!confirmed || (needsInstallation && !policy.enabled)))
            }
          >
            {busy ? "Saving…" : "Save cost-saving settings"}
          </button>
          {policy?.wake_pending && (
            <button
              type="button"
              className="button primary"
              disabled={busy || starting}
              onClick={async () => {
                setStarting(true);
                setError("");
                setMessage("Starting the VM and game…");
                try {
                  const op = await api.action(server.id, "cloud-start");
                  setOperation(op.id);
                } catch (e) {
                  setError(String(e));
                  setStarting(false);
                }
              }}
            >
              {starting ? "Starting server…" : "Start server"}
            </button>
          )}
        </div>
      </form>
    </section>
  );
}
