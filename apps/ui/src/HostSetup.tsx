import { useEffect, useRef, useState } from "react";
import { type Server } from "./api";
export default function HostSetup({
  server,
  local,
  act,
  powerState,
  remoteInstalled,
  reconnect,
}: {
  server: Server;
  local: boolean;
  act: (action: string) => void;
  powerState?: string;
  remoteInstalled?: boolean | null;
  reconnect?: (keyPath: string) => Promise<unknown>;
}) {
  const recovered = server.state === "recovered";
  const [privateKey, setPrivateKey] = useState(server.ssh_key || "");
  const [key, setKey] = useState<{ keys: string; fingerprint: string } | null>(
      null,
    ),
    [verified, setVerified] = useState(false),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const requestId = useRef(0);
  useEffect(() => {
    requestId.current += 1;
    setKey(null);
    setVerified(false);
    setError("");
    setBusy(false);
    setPrivateKey(server.ssh_key || "");
    return () => {
      requestId.current += 1;
    };
  }, [server.id, server.host]);
  const docker = server.provider === "docker",
    remoteDocker = server.provider === "ssh_docker";
  const hostOff = ["stopped", "deallocated"].includes(powerState || "");
  const hostStarting = ["starting", "stopping", "deallocating"].includes(
    powerState || "",
  );
  const blocked = hostOff || hostStarting;
  async function scan(accept: boolean) {
    const request = ++requestId.current;
    setBusy(true);
    setError("");
    if (!accept) {
      setVerified(false);
      setKey(null);
    }
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke<{ keys: string; fingerprint: string }>(
        "host_key",
        { host: server.host, accepted: accept ? key?.keys : null },
      );
      if (request !== requestId.current) return;
      setKey(result);
      if (accept) setVerified(true);
    } catch (e) {
      if (request !== requestId.current) return;
      setVerified(false);
      if (accept) setKey(null);
      setError(String(e));
    } finally {
      if (request === requestId.current) setBusy(false);
    }
  }
  if (
    !["awaiting_bootstrap", "attention", "recovered"].includes(server.state) &&
    !(
      remoteInstalled === false &&
      !["provisioning", "deleted"].includes(server.state)
    )
  )
    return (
      <section className="panel detail-panel">
        <h3>{docker ? "Local Docker" : "Remote"}</h3>
        <p role="status">
          {server.state === "provisioning"
            ? "Preparing the host. Remote setup becomes available when deployment finishes. See Activity for progress."
            : ["running", "starting", "stopped", "sleeping"].includes(
                  server.state,
                )
              ? `Game ${server.state}. Use Overview to start or stop it.`
              : `Host state: ${server.state.replaceAll("_", " ")}. Check Activity for details.`}
        </p>
      </section>
    );
  return (
    <section className="panel detail-panel">
      <h3>{recovered ? "Reconnect Remote" : docker ? "Install local game server" : "Install Remote"}</h3>
      {recovered && <>
        <p>Infrastructure recovered. Reconnect to read the existing game version and status.</p>
        <label className="field">SSH private key path
          <input value={privateKey} disabled={busy} onChange={(e) => setPrivateKey(e.target.value)} />
        </label>
        <p className="muted small">Use the existing key on the computer running this Manager. Allow SSH access from its current address in Azure. Reconnecting does not install software or change game files.</p>
      </>}
      {server.provider === "azure" && server.state === "awaiting_bootstrap" && (
        <p className="muted small">
          Azure starts the VM during deployment. Installing the Remote and game
          is the next step.
        </p>
      )}
      {hostOff && (
        <p role="status">
          The VM is {powerState}. Start the VM below, then check its identity.
        </p>
      )}
      {hostStarting && (
        <p role="status">
          The VM is {powerState}. Wait for it to finish before connecting.
        </p>
      )}
      {server.state === "attention" && (
        <p role="status">
          Setup needs attention. Check Activity for the failure before retrying.
        </p>
      )}
      {!recovered && <ol className="setup-steps">
        {!docker && <li>Verify the host's SSH fingerprint.</li>}
        <li>
          {remoteDocker || docker
            ? "Build the Docker image and create persistent game storage."
            : "Upload the bundled agent and install the game runtime."}
        </li>
        <li>
          Install Vintage Story {server.game_version} and check the agent
          connection.
        </li>
        <li>Start the game from Overview.</li>
      </ol>}
      {!docker &&
        (local ? (
          <>
            {!server.host ? (
              <p role="status">
                Waiting for the host address. Setup will be available when it is
                assigned.
              </p>
            ) : !key && !verified && !error && !busy ? (
              <p className="muted small">
                Connect to {server.host} to check its identity before
                {recovered ? "reconnecting." : "installing. A newly created host may need a few minutes to start."}
              </p>
            ) : null}
            <button
              type="button"
              className="button secondary"
              disabled={!server.host || busy || blocked}
              onClick={() => void scan(false)}
            >
              {busy
                ? "Checking connection…"
                : error
                  ? "Retry connection"
                  : "Check Remote identity"}
            </button>
            {busy && (
              <p role="status">
                Connecting to {server.host} and reading its SSH fingerprint…
              </p>
            )}
            {key && !verified && (
              <div className="form">
                <code style={{ overflowWrap: "anywhere" }}>
                  {key.fingerprint}
                </code>
                <p>
                  Open this host’s terminal in your cloud account or on the host
                  itself. Run the command below and compare its SHA256
                  fingerprint before trusting this connection.
                </p>
                <code>ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</code>
                <button
                  type="button"
                  className="button secondary"
                  disabled={busy || blocked}
                  onClick={() => void scan(true)}
                >
                  Fingerprint matches — trust Remote
                </button>
              </div>
            )}
            {verified && <p role="status">Remote identity saved.</p>}
          </>
        ) : (
          <p>
            Verify this host in the SSH known_hosts file on the computer running
            this Manager, or use the desktop app for guided fingerprint
            verification.
          </p>
        ))}
      <button
        type="button"
        className="button primary"
        disabled={
          busy || blocked || !server.host || (!docker && local && !verified) || (recovered && (!privateKey.trim() || !reconnect))
        }
        onClick={async () => {
          if (!recovered) { act("bootstrap"); return; }
          const request = ++requestId.current;
          setBusy(true); setError("");
          try { await reconnect?.(privateKey.trim()); }
          catch (e) { if (request === requestId.current) setError(String(e)); }
          finally { if (request === requestId.current) setBusy(false); }
        }}
      >
        {recovered ? busy ? "Reconnecting…" : "Reconnect Remote" : docker ? "Install locally" : "Install Remote and game"}
      </button>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
