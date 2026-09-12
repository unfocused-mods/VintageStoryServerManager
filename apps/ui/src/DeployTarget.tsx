import GameVersionPicker from "./GameVersionPicker";
import { useState, type ReactNode } from "react";
import { Api, type Account } from "./api";
import { randomServerName } from "./serverName";

type Target = "docker" | "ssh" | "ssh_docker" | "cloud";
export default function DeployTarget({
  api,
  localDesktop,
  onDone,
  cloud,
}: {
  api: Api;
  localDesktop: boolean;
  onDone: (id?: string, remote?: boolean) => void;
  cloud: (done: (id?: string, remote?: boolean) => void) => ReactNode;
}) {
  const [target, setTarget] = useState<Target | null>(null);
  if (!target)
    return (
      <div className="form">
        <h3>Where will the game run?</h3>
        <div className="form-grid">
          <button
            className="button secondary"
            onClick={() => setTarget("docker")}
          >
            {localDesktop ? "Local Docker" : "Docker on this Manager"}
          </button>
          <button className="button secondary" onClick={() => setTarget("ssh")}>
            Install a Remote
          </button>
          <button
            className="button secondary"
            onClick={() => setTarget("ssh_docker")}
          >
            Docker over SSH
          </button>
          <button
            className="button secondary"
            onClick={() => setTarget("cloud")}
          >
            Cloud deployment
          </button>
        </div>
        <p className="muted small">
          A Remote runs the game and its management agent on another computer.
          This Manager remains your control panel.
        </p>
      </div>
    );
  return (
    <div className="form">
      <button
        type="button"
        className="text-button deployment-back"
        onClick={() => setTarget(null)}
      >
        Change deployment target
      </button>
      {target === "cloud" ? (
        cloud(onDone)
      ) : (
        <HostDeploy
          key={target}
          api={api}
          target={target}
          localDesktop={localDesktop}
          onDone={onDone}
        />
      )}
    </div>
  );
}
function HostDeploy({
  api,
  target,
  localDesktop,
  onDone,
}: {
  api: Api;
  target: Exclude<Target, "cloud">;
  localDesktop: boolean;
  onDone: (id?: string, remote?: boolean) => void;
}) {
  const remote = target !== "docker";
  const [name, setName] = useState(randomServerName),
    [host, setHost] = useState(""),
    [user, setUser] = useState(""),
    [key, setKey] = useState(""),
    [publicKey, setPublicKey] = useState(""),
    [version, setVersion] = useState(""),
    [versionReady, setVersionReady] = useState(false),
    [port, setPort] = useState("42420"),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [plan, setPlan] = useState<any>(null),
    [accountId, setAccountId] = useState("");
  async function createKey() {
    setBusy(true);
    setError("");
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const keys = await invoke<{ public_key: string; private_key: string }>(
        "create_ssh_key",
      );
      setKey(keys.private_key);
      setPublicKey(keys.public_key);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <form
      className="form"
      onSubmit={async (e) => {
        e.preventDefault();
        if (!plan && !versionReady) return;
        setBusy(true);
        setError("");
        try {
          if (plan) {
            await api.request("/deployments", "POST", {
              plan_id: plan.id,
              accept_incomplete_estimate: true,
            });
            onDone(plan.id, remote);
            return;
          }
          let id = accountId;
          if (!id) {
            const account = await api.request<Account>("/accounts", "POST", {
              id: "",
              name: remote
                ? `${host} (${target === "ssh" ? "Remote" : "Docker"})`
                : "Local Docker",
              provider: target,
              credential_ref: "",
              settings: { game_port: Number(port) },
            });
            id = account.id;
            setAccountId(id);
          }
          await api.request(`/accounts/${id}/validate`, "POST", {});
          setPlan(
            await api.request("/plans", "POST", {
              name,
              account_id: id,
              region: remote ? host : "local",
              size: "existing",
              game_version: version,
              ssh_public_key: publicKey,
              ssh_private_key: key,
              admin_cidr: "",
              existing_host: host,
              ssh_user: user,
            }),
          );
        } catch (e) {
          setError(String(e));
        } finally {
          setBusy(false);
        }
      }}
    >
      <h3>
        {target === "docker"
          ? localDesktop
            ? "Local Docker"
            : "Docker on this Manager"
          : target === "ssh"
            ? "Install a Remote"
            : "Docker over SSH"}
      </h3>
      {plan ? (
        <>
          <h4>{name}</h4>
          <p>
            {remote
              ? host
              : localDesktop
                ? "This computer"
                : "The computer running this Manager"}{" "}
            · Vintage Story {version}
          </p>
          <ul>
            {plan.resources?.map((resource: string) => (
              <li key={resource}>{resource}</li>
            ))}
          </ul>
          {plan.warnings?.length > 0 && (
            <ul aria-label="Deployment warnings">
              {plan.warnings.map((warning: string) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          )}
          <p>
            {remote
              ? "Next: verify the host fingerprint, then install the Remote."
              : "Creates persistent Docker storage, builds the game image, and installs the selected game version. Downloads may take several minutes."}
          </p>
          <div className="button-row">
            <button
              type="button"
              className="button secondary"
              onClick={() => setPlan(null)}
            >
              Back
            </button>
            <button className="button primary" disabled={busy}>
              {busy ? "Setting up…" : remote ? "Add Remote" : "Install locally"}
            </button>
          </div>
        </>
      ) : (
        <>
          <p>
            {target === "docker"
              ? localDesktop
                ? "Requires an x64 computer. Install Docker Desktop, open it, and select Linux containers before continuing. Game data stays in a persistent volume."
                : "Uses Docker on the computer running this Manager. Docker must be running with Linux x64 containers."
              : target === "ssh"
                ? "Requires Ubuntu 24.04 x64, SSH access, and a user with passwordless sudo. The app uploads the agent and installs the game runtime for you."
                : "Requires a Linux x64 host with Docker installed, SSH access, and permission to run Docker without a password."}
          </p>
          <label>
            Game server name
            <input
              required
              pattern="[a-z0-9-]{3,40}"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="our-world"
            />
            <button
              type="button"
              className="text-button"
              onClick={() => setName(randomServerName())}
            >
              Suggest a name
            </button>
          </label>
          <GameVersionPicker
            api={api}
            value={version}
            onChange={setVersion}
            onReady={setVersionReady}
            disabled={busy}
          />
          {target !== "ssh" && (
            <label>
              Game port
              <input
                required
                type="number"
                min="1024"
                max="65535"
                value={port}
                onChange={(e) => {
                  setPort(e.target.value);
                  setAccountId("");
                }}
              />
            </label>
          )}
          {remote && (
            <>
              <div className="form-grid">
                <label>
                  Remote address
                  <input
                    required
                    value={host}
                    onChange={(e) => {
                      setHost(e.target.value);
                      setAccountId("");
                    }}
                    placeholder="192.168.1.20"
                  />
                </label>
                <label>
                  SSH username
                  <input
                    required
                    value={user}
                    onChange={(e) => setUser(e.target.value)}
                    placeholder="ubuntu"
                  />
                </label>
              </div>
              {localDesktop && (
                <button
                  type="button"
                  className="button secondary"
                  disabled={busy}
                  onClick={() => void createKey()}
                >
                  Create SSH key
                </button>
              )}
              {publicKey && (
                <>
                  <label>
                    Public key
                    <textarea readOnly value={publicKey} />
                  </label>
                  <p className="muted small">
                    Add this public key to ~/.ssh/authorized_keys for the SSH
                    user on the Remote before continuing.
                  </p>
                </>
              )}
              <label>
                SSH private key file
                <input
                  required
                  value={key}
                  onChange={(e) => setKey(e.target.value)}
                  placeholder={
                    localDesktop
                      ? "Choose an existing key path or create one above"
                      : "Path on the Manager computer"
                  }
                />
              </label>
              <p className="muted small">
                The next step checks the host fingerprint before uploading any
                software.
              </p>
            </>
          )}
          <button className="button primary" disabled={busy || !versionReady}>
            {busy ? "Checking…" : "Review setup"}
          </button>
        </>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </form>
  );
}
