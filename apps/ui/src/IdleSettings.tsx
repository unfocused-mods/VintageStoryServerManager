import { useEffect, useState } from "react";
import { Api } from "./api";
interface Idle {
  policy: { enabled: boolean; minutes: number };
  sleeping: boolean;
  idle_seconds: number | null;
  error: string | null;
}
export default function IdleSettings({
  api,
  serverId,
}: {
  api: Api;
  serverId: string;
}) {
  const [status, setStatus] = useState<Idle | null>(null),
    [enabled, setEnabled] = useState(false),
    [minutes, setMinutes] = useState(15),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [saved, setSaved] = useState(false);
  useEffect(() => {
    let active = true;
    api
      .request<Idle>(`/servers/${serverId}/agent/idle-policy`)
      .then((value) => {
        if (active) {
          setStatus(value);
          setEnabled(value.policy.enabled);
          setMinutes(value.policy.minutes);
        }
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [api, serverId]);
  return (
    <section className="panel detail-panel">
      <h3>Game-only sleep</h3>
      <p>
        The Remote saves and stops the game when nobody is connected. The
        computer stays on and cloud VM charges continue.
      </p>
      <p className="small">
        Joining starts the game again while the host is running. The first
        player may need to retry while the world loads.
      </p>
      <form
        className="form"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          setSaved(false);
          setError("");
          try {
            setStatus(
              await api.request<Idle>(
                `/servers/${serverId}/agent/idle-policy`,
                "PUT",
                { enabled, minutes },
              ),
            );
            setSaved(true);
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
            disabled={!status || busy}
            onChange={(e) => {
              setEnabled(e.target.checked);
              setSaved(false);
            }}
          />
          Enable automatic sleep and wake
        </label>
        <label className="field">
          <span>Minutes without players</span>
          <input
            type="number"
            min={1}
            max={1440}
            value={minutes}
            required
            disabled={!enabled || busy}
            onChange={(e) => {
              setMinutes(Number(e.target.value));
              setSaved(false);
            }}
          />
        </label>
        <p className="muted small">
          Runs on the Remote, even with this app closed. Server-list checks and
          port scans can also wake the game. Manual Stop disables wake until you
          start it again.
        </p>
        {status?.sleeping && (
          <p role="status">Sleeping. Ready to wake on a connection.</p>
        )}
        {status?.error && (
          <p role="alert" className="inline-error">
            {status.error}
          </p>
        )}
        {error && (
          <p role="alert" className="inline-error">
            {error}
          </p>
        )}
        {saved && <p role="status">Sleep settings saved.</p>}
        <button className="button secondary" disabled={!status || busy}>
          {busy ? "Saving…" : "Save sleep settings"}
        </button>
      </form>
    </section>
  );
}
