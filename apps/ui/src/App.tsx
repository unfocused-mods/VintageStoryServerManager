import ModsPage from "./ModsPage";
import AzureDiscovery from "./AzureDiscovery";
import { useCloudCatalog } from "./useCloudCatalog";
import InstanceSizePicker from "./InstanceSizePicker";
import { useInstancePrices } from "./useInstancePrices";
import GameVersionPicker from "./GameVersionPicker";
import BrandIcon from "./BrandIcon";
import ThemeSelect from "./ThemeSelect";
import DeployTarget from "./DeployTarget";
import ServerConfigForm, { validateServerConfig } from "./ServerConfigForm";
import Advisor from "./Advisor";
import { randomServerName } from "./serverName";
import Workflows from "./Workflows";
import HostSetup from "./HostSetup";
import LifecycleControls from "./LifecycleControls";
import OperationDetails from "./OperationDetails";
import { useInfrastructureStatus } from "./useInfrastructureStatus";
import { catalogOptions } from "./catalog";
import {
  useEffect,
  useState,
  useCallback,
  useRef,
  type ReactNode,
} from "react";
import {
  Activity,
  ArrowDownToLine,
  ArrowRight,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Cloud,
  Code2,
  Copy,
  Database,
  ExternalLink,
  FileText,
  Folder,
  FolderOpen,
  Globe2,
  HardDrive,
  KeyRound,
  Layers,
  Loader2,
  LogOut,
  Menu,
  Monitor,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Search,
  Server as ServerIcon,
  Settings,
  ShieldCheck,
  Terminal,
  Trash2,
  Upload,
  Users,
  X,
  AlertTriangle,
  MapPin,
  Package,
  Clock,
  ArrowUpRight,
} from "lucide-react";
import {
  Api,
  type Account,
  type Operation,
  type Provider,
  type Server,
  type Summary,
  providerNames,
  stateLabel,
  download,
  decode64,
  encode64,
} from "./api";

type Page =
  | "Overview"
  | "Console"
  | "Players"
  | "Settings"
  | "Mods"
  | "Files"
  | "Backups"
  | "Activity"
  | "Infrastructure";
const pages: [Page, typeof Activity][] = [
  ["Overview", Layers],
  ["Console", Terminal],
  ["Players", Users],
  ["Settings", Settings],
  ["Mods", Package],
  ["Files", FolderOpen],
  ["Backups", Database],
  ["Activity", Activity],
  ["Infrastructure", Cloud],
];
const EMPTY: Summary = {
  servers: [],
  operations: [],
  user: { name: "Local owner", role: "owner" },
  version: "0.1.0",
  providers: [],
};
export default function App() {
  const [api, setApi] = useState<Api | null>(null),
    [summary, setSummary] = useState<Summary>(EMPTY),
    [page, setPage] = useState<Page>("Overview"),
    [selected, setSelected] = useState(""),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [modal, setModal] = useState(""),
    [advisorProvider, setAdvisorProvider] = useState<Provider | undefined>(),
    [busy, setBusy] = useState(false),
    [mobile, setMobile] = useState(false);
  const server = summary.servers.find((s) => s.id === selected);
  const apiRef = useRef<Api | null>(null);
  const refresh = useCallback(async () => {
    if (!api) return;
    try {
      const s = await api.request<Summary>("/summary");
      if (apiRef.current !== api) return;
      setSummary(s);
      setError("");
    } catch (e) {
      if (apiRef.current !== api) return;
      setError(String(e));
    }
  }, [api]);
  const desktop = "__TAURI_INTERNALS__" in window;
  const [localConnection, setLocalConnection] = useState(false);
  const [startup, setStartup] = useState(desktop ? "starting" : "ready");
  const connectLocal = useCallback(async () => {
    setStartup("starting");
    setError("");
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const c = await invoke<{ url: string; token: string }>("connection");
      const local = new Api(c.url, c.token);
      const inventory = await local.request<Summary>("/summary");
      apiRef.current = local;
      setSelected("");
      setSummary(inventory);
      setApi(local);
      setLocalConnection(true);
      setStartup("ready");
    } catch (e) {
      setError(
        `Could not start Server Manager. ${e instanceof Error ? e.message : String(e)}`,
      );
      setStartup("failed");
    }
  }, []);
  useEffect(() => {
    if (desktop) void connectLocal();
  }, [desktop, connectLocal]);
  useEffect(() => {
    void refresh();
    if (api) {
      const id = setInterval(() => void refresh(), 5000);
      return () => clearInterval(id);
    }
  }, [api, refresh]);
  useEffect(() => {
    if (!selected && summary.servers.length) setSelected(summary.servers[0].id);
  }, [summary.servers, selected]);
  const perform = async (fn: () => Promise<unknown>, message = "Saved") => {
    setBusy(true);
    setError("");
    try {
      await fn();
      setNotice(message);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const act = (action: string, body: unknown = {}) => {
    if (server && api)
      void perform(
        () => api.action(server.id, action, body),
        "Operation queued",
      );
  };
  return (
    <div className="app-shell">
      <aside className={`sidebar ${mobile ? "mobile-open" : ""}`}>
        <a className="brand" href="#" onClick={() => setPage("Overview")}>
          <div className="brand-mark">
            <BrandIcon />
          </div>
          <div>
            Vintage Story<span>Server Manager</span>
          </div>
        </a>
        <button className="server-select" onClick={() => setModal("servers")}>
          <div className="server-mini">
            <ServerIcon size={19} />
          </div>
          <span>
            {server?.name || "No server selected"}
            <small>
              {server
                ? providerNames[server.provider]
                : "Select or create a server"}
            </small>
          </span>
          <ChevronDown size={15} />
        </button>
        <nav aria-label="Server navigation">
          {pages.map(([name, Icon]) => (
            <button
              key={name}
              aria-current={page === name ? "page" : undefined}
              className={page === name ? "active" : ""}
              onClick={() => {
                setPage(name);
                setMobile(false);
              }}
            >
              <Icon size={18} />
              {name}
              {name === "Activity" &&
                summary.operations.some((o) => o.status === "running") && (
                  <span className="nav-dot" />
                )}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <button onClick={() => setModal("accounts")}>
            <Cloud size={17} />
            Cloud accounts
            <ArrowUpRight size={14} />
          </button>
          <button onClick={() => setModal("team")}>
            <Users size={17} />
            Team & access
            <ArrowUpRight size={14} />
          </button>
          <div className="sidebar-footer">
            <span>
              v{summary.version} <span className="alpha">ALPHA</span>
            </span>
            <ThemeSelect />
          </div>
        </div>
      </aside>
      <div className="main-shell">
        <header className="topbar">
          <div className="breadcrumbs">
            <button
              className="mobile-toggle"
              aria-label="Open navigation"
              onClick={() => setMobile(!mobile)}
            >
              <Menu size={20} />
            </button>
            <span>Workspace</span>
            <ChevronRight size={14} />
            <strong>{page}</strong>
          </div>
          <div className="top-actions">
            <span className={`connection ${api && !error ? "connected" : ""}`}>
              <i />
              {api
                ? localConnection
                  ? "Local Manager"
                  : summary.user.name
                : startup === "starting"
                  ? "Starting…"
                  : "Not signed in"}
            </span>
            <button
              className="icon-button"
              aria-label="Refresh inventory"
              onClick={() => void refresh()}
            >
              <RefreshCw size={16} />
            </button>
            <button
              className="avatar"
              onClick={() => setModal("connect")}
              title="Manager connection"
              aria-label="Manager connection"
            >
              {api ? (
                summary.user.name.slice(0, 2).toUpperCase()
              ) : (
                <KeyRound size={16} />
              )}
            </button>
          </div>
        </header>
        <main tabIndex={0}>
          {error && (
            <div className="alert error" role="alert">
              <AlertTriangle size={18} />
              <span>{error}</span>
              {localConnection && (
                <button
                  className="button secondary"
                  onClick={() => void connectLocal()}
                >
                  Reconnect
                </button>
              )}
              <button aria-label="Dismiss error" onClick={() => setError("")}>
                <X size={16} />
              </button>
            </div>
          )}
          {notice && (
            <div className="alert success" role="status">
              <CheckCircle2 size={18} />
              <span>{notice}</span>
              <button aria-label="Dismiss notice" onClick={() => setNotice("")}>
                <X size={16} />
              </button>
            </div>
          )}
          <div className="page-heading">
            <div>
              <h1>{page === "Overview" ? "Servers" : page}</h1>
            </div>
            <div className="heading-actions">
              {page === "Overview" && (
                <button
                  className="button secondary"
                  onClick={() => setModal("advisor")}
                >
                  <Globe2 size={16} />
                  Compare costs
                </button>
              )}
              {server && page === "Overview" && (
                <button
                  className="button secondary"
                  onClick={() => setModal("servers")}
                >
                  <ServerIcon size={16} />
                  Switch server
                </button>
              )}
              {api && (
                <button
                  className="button primary"
                  disabled={startup === "starting" || (desktop && !api)}
                  onClick={() => setModal(api ? "deploy" : "connect")}
                >
                  <Plus size={17} />
                  Create server
                </button>
              )}
            </div>
          </div>
          {page === "Overview" ? (
            <>
              {server ? (
                <>
                  <section className="panel world-panel">
                    <div>
                      <h2>{server.name}</h2>
                      <div className="inline-meta">
                        <Badge state={server.state} />
                        <span>
                          <MapPin size={14} />
                          {server.region || "Your host"}
                        </span>
                        <span>Vintage Story {server.game_version}</span>
                      </div>
                      {!["awaiting_bootstrap", "provisioning"].includes(
                        server.state,
                      ) && (
                        <div className="join-address">
                          <code>
                            {server.host
                              ? `${server.host}:${server.resources?.find((r) => r.kind === "docker_game_port")?.id || "42420"}`
                              : "Waiting for an address"}
                          </code>
                          <button
                            aria-label="Copy join address"
                            disabled={!server.host}
                            onClick={() =>
                              void navigator.clipboard
                                .writeText(
                                  `${server.host}:${server.resources?.find((r) => r.kind === "docker_game_port")?.id || "42420"}`,
                                )
                                .then(() => setNotice("Join address copied"))
                            }
                          >
                            <Copy size={16} />
                          </button>
                        </div>
                      )}
                      <div className="button-row">
                        <LifecycleControls
                          state={server.state}
                          busy={busy}
                          act={act}
                          setup={() => setPage("Infrastructure")}
                        />
                      </div>
                    </div>
                  </section>
                  <div>
                    <section className="panel">
                      <PanelTitle
                        icon={<Activity size={17} />}
                        title="Recent activity"
                        action={
                          <button
                            className="text-button"
                            onClick={() => setPage("Activity")}
                          >
                            View all <ArrowRight size={14} />
                          </button>
                        }
                      />
                      <OperationList
                        api={api}
                        operations={summary.operations
                          .filter((o) => o.server_id === server.id)
                          .slice(0, 4)}
                      />
                    </section>
                  </div>
                </>
              ) : (
                <>
                  <section className="panel setup-panel">
                    {startup === "starting" ? (
                      <>
                        <Loader2 className="spin" size={20} />
                        <h2 aria-live="polite">Starting Server Manager…</h2>
                      </>
                    ) : startup === "failed" ? (
                      <>
                        <h2>Server Manager could not start</h2>
                        <button
                          className="button primary"
                          onClick={() => void connectLocal()}
                        >
                          Try again
                        </button>
                      </>
                    ) : api ? (
                      <>
                        <h2>Create your first server</h2>
                        <p>
                          Run locally with Docker, install a Remote on another
                          computer, or deploy to the cloud.
                        </p>
                        <button
                          className="button primary"
                          onClick={() => setModal("deploy")}
                        >
                          <Plus size={16} />
                          Set up server
                        </button>
                      </>
                    ) : (
                      <>
                        <h2>Sign in to Server Manager</h2>
                        <p>
                          Sign in to this Manager, or use the desktop app to set
                          up a game server on this computer or a Remote.
                        </p>
                        <button
                          className="button primary"
                          onClick={() => setModal("connect")}
                        >
                          Sign in
                        </button>
                      </>
                    )}
                  </section>
                </>
              )}
            </>
          ) : page === "Activity" ? (
            <section className="panel">
              <PanelTitle
                title="Operation history"
                icon={<Activity size={18} />}
              />
              <OperationList api={api} operations={summary.operations} />
            </section>
          ) : server && api ? (
            page === "Mods" ? (
              <ModsPage
                key={`${api.url}:${summary.user.name}:${server.id}`}
                api={api}
                server={server}
                operations={summary.operations}
              />
            ) : (
              <ServerPage
                key={`${server.id}-${page}`}
                page={page}
                server={server}
                api={api}
                act={act}
                perform={perform}
                busy={busy}
                localDesktop={localConnection}
              />
            )
          ) : (
            <div className="panel empty">
              <ServerIcon size={36} />
              <h2>No server selected</h2>
              <p>Select a server from the sidebar.</p>
              <button
                className="button primary"
                disabled={startup === "starting" || (desktop && !api)}
                onClick={() => setModal(api ? "deploy" : "connect")}
              >
                {api ? "Create server" : "Sign in"}
                <ArrowRight size={16} />
              </button>
            </div>
          )}
          {server && api && (
            <Workflows
              localManager={localConnection}
              page={page}
              server={server}
              api={api}
              act={act}
              perform={perform}
            />
          )}
        </main>
      </div>
      {modal && (
        <Modal
          title={modalTitles[modal] || modal}
          close={() => setModal("")}
          wide={modal === "deploy" || modal === "advisor"}
          dismissOnBackdrop={modal !== "deploy"}
        >
          {modal === "connect" && (
            <ConnectionForm
              desktop={desktop}
              onLocal={() => {
                setModal("");
                void connectLocal();
              }}
              onConnect={(a) => {
                apiRef.current = a;
                setSummary(EMPTY);
                setSelected("");
                setApi(a);
                setLocalConnection(false);
                setModal("");
                setNotice("");
              }}
              onDisconnect={() => {
                if (desktop) {
                  setModal("");
                  void connectLocal();
                  return;
                }
                apiRef.current = null;
                setApi(null);
                setSummary(EMPTY);
                setSelected("");
                setModal("");
              }}
              connected={!!api}
            />
          )}
          {modal === "accounts" &&
            (api ? (
              <Accounts
                key={advisorProvider || "accounts"}
                initialProvider={advisorProvider}
                api={api}
                onError={setError}
                localDesktop={localConnection}
                onRecovered={() => void refresh()}
              />
            ) : (
              <ConnectNotice onClick={() => setModal("connect")} />
            ))}
          {modal === "advisor" && (
            <Advisor
              api={api || new Api(window.location.origin, "")}
              onChoose={(provider) => {
                setAdvisorProvider(provider);
                setModal("accounts");
              }}
            />
          )}
          {modal === "deploy" && api && (
            <DeployTarget
              cloud={(done) => (
                <CloudDeploy
                  api={api}
                  localDesktop={localConnection}
                  onDone={done}
                />
              )}
              api={api}
              localDesktop={localConnection}
              onDone={(id, remote) => {
                setModal("");
                if (id) setSelected(id);
                if (remote) setPage("Infrastructure");
                void refresh();
                setNotice("Deployment queued");
              }}
            />
          )}
          {modal === "servers" && (
            <div className="server-list">
              {summary.servers.length ? (
                summary.servers.map((s) => (
                  <button
                    key={s.id}
                    onClick={() => {
                      setSelected(s.id);
                      setModal("");
                    }}
                  >
                    <ServerIcon />
                    <span>
                      <strong>{s.name}</strong>
                      <small>
                        {providerNames[s.provider]} · {s.region}
                      </small>
                    </span>
                    <Badge state={s.state} />
                  </button>
                ))
              ) : (
                <button
                  className="button primary"
                  onClick={() => setModal(api ? "deploy" : "connect")}
                >
                  Set up server
                </button>
              )}
            </div>
          )}
          {modal === "team" &&
            (api ? (
              <Team api={api} servers={summary.servers} />
            ) : (
              <ConnectNotice onClick={() => setModal("connect")} />
            ))}
        </Modal>
      )}
    </div>
  );
}
const modalTitles: Record<string, string> = {
  connect: "Manager connection",
  accounts: "Cloud accounts",
  deploy: "Set up server",
  servers: "Your servers",
  team: "Team & access",
  advisor: "Compare hosting",
};
function Badge({ state }: { state: string }) {
  return (
    <span className={`badge ${state}`}>
      <i />
      {stateLabel(state)}
    </span>
  );
}
function PanelTitle({
  title,
  icon,
  action,
}: {
  title: string;
  icon?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="panel-title">
      <h3>
        {icon}
        {title}
      </h3>
      {action}
    </div>
  );
}
function OperationList({
  operations,
  api,
}: {
  operations: Operation[];
  api: Api | null;
}) {
  return operations.length ? (
    <div className="operation-list">
      {operations.map((o) => (
        <div className="operation" key={o.id}>
          <div className={`operation-icon ${o.status}`}>
            {["running", "queued"].includes(o.status) ? (
              <Loader2 size={17} className="spin" />
            ) : o.status === "succeeded" ? (
              <Check size={17} />
            ) : (
              <Activity size={17} />
            )}
          </div>
          <div>
            <strong>{stateLabel(o.action)}</strong>
            <p>{o.message}</p>
            <small>{new Date(o.created_at).toLocaleString()}</small>
            {api && <OperationDetails api={api} operation={o} />}
          </div>
          <Badge state={o.status} />
        </div>
      ))}
    </div>
  ) : (
    <div className="empty-inline">
      <Clock size={23} />
      <p>No operations yet.</p>
    </div>
  );
}
function Modal({
  title,
  close,
  children,
  wide = false,
  dismissOnBackdrop = true,
}: {
  title: string;
  close: () => void;
  children: ReactNode;
  wide?: boolean;
  dismissOnBackdrop?: boolean;
}) {
  const closeRef = useRef(close);
  closeRef.current = close;
  const dialogRef = useRef<HTMLElement>(null);
  useEffect(() => {
    const fn = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeRef.current();
      if (e.key === "Tab") {
        const elements = Array.from(
          dialogRef.current?.querySelectorAll<HTMLElement>(
            "button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),a[href],summary,[tabindex='0']",
          ) || [],
        ).filter((el) => el.getClientRects().length > 0);
        const first = elements[0],
          last = elements[elements.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last?.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first?.focus();
        }
      }
    };
    window.addEventListener("keydown", fn);
    const previous = document.activeElement as HTMLElement | null;
    document.querySelector<HTMLButtonElement>(".modal-close")?.focus();
    return () => {
      window.removeEventListener("keydown", fn);
      previous?.focus();
    };
  }, []);
  return (
    <div
      className="modal-backdrop"
      onClick={(e) => {
        if (dismissOnBackdrop && e.target === e.currentTarget) close();
      }}
    >
      <section
        ref={dialogRef}
        className={`modal ${wide ? "wide" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <header>
          <h2>{title}</h2>
          <button
            className="icon-button modal-close"
            aria-label="Close dialog"
            onClick={close}
          >
            <X size={20} />
          </button>
        </header>
        <div className="modal-body" tabIndex={0}>
          {children}
        </div>
      </section>
    </div>
  );
}
function ConnectNotice({ onClick }: { onClick: () => void }) {
  return (
    <div className="empty-inline">
      <p>Sign in to manage accounts.</p>
      <button className="button primary" onClick={onClick}>
        Sign in
      </button>
    </div>
  );
}
function Field({
  label,
  children,
  hint,
  hintId,
}: {
  label: string;
  children: ReactNode;
  hint?: string;
  hintId?: string;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
      {hint && <small id={hintId}>{hint}</small>}
    </label>
  );
}
function ConnectionForm({
  onConnect,
  onDisconnect,
  desktop,
  onLocal,
  connected,
}: {
  connected: boolean;
  desktop: boolean;
  onLocal: () => void;
  onConnect: (a: Api) => void;
  onDisconnect: () => void;
}) {
  const [url, setUrl] = useState(
      location.port === "1420" ? "" : location.origin,
    ),
    [token, setToken] = useState(""),
    [username, setUsername] = useState(""),
    [password, setPassword] = useState(""),
    [code, setCode] = useState(""),
    [mode, setMode] = useState("login"),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  return (
    <form
      className="form"
      onSubmit={async (e) => {
        e.preventDefault();
        setBusy(true);
        try {
          let value = token;
          if (mode === "login") {
            const res = await fetch(`${url}/api/v1/login`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ name: username, password, code }),
            });
            const data = await res.json();
            if (!res.ok) throw new Error(data.error);
            value = data.token;
          }
          const a = new Api(url, value);
          await a.request("/summary");
          onConnect(a);
        } catch (e) {
          setError(String(e));
        } finally {
          setBusy(false);
        }
      }}
    >
      {desktop && (
        <button type="button" className="button secondary" onClick={onLocal}>
          Use Local Manager
        </button>
      )}
      <details>
        <summary>Advanced: connect to another Manager</summary>
        <Field label="Manager address">
          <input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://manager.example.com"
          />
        </Field>
      </details>
      <div className="segmented">
        <button
          type="button"
          className={mode === "token" ? "selected" : ""}
          onClick={() => setMode("token")}
        >
          Access token
        </button>
        <button
          type="button"
          className={mode === "login" ? "selected" : ""}
          onClick={() => setMode("login")}
        >
          Username and password
        </button>
      </div>
      {mode === "token" ? (
        <Field
          label="Access token"
          hint="For local web access, use owner.token in the Manager�s data folder."
        >
          <input
            autoComplete="off"
            type="password"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            required
          />
        </Field>
      ) : (
        <>
          <Field label="Username">
            <input
              autoComplete="username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
            />
          </Field>
          <Field label="Password">
            <input
              autoComplete="current-password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
          </Field>
          <Field label="Authenticator code">
            <input
              autoComplete="one-time-code"
              value={code}
              onChange={(e) => setCode(e.target.value)}
            />
          </Field>
        </>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      <div className="button-row">
        <button disabled={busy} className="button primary">
          {busy ? (
            <Loader2 className="spin" size={16} />
          ) : (
            <KeyRound size={16} />
          )}
          Sign in
        </button>
        {connected && (
          <button
            type="button"
            className="button secondary"
            onClick={onDisconnect}
          >
            <LogOut size={15} />
            Disconnect
          </button>
        )}
      </div>
    </form>
  );
}
export function Accounts({
  api,
  localDesktop = false,
  initialProvider,
  onSaved,
  onRecovered,
}: {
  api: Api;
  onError: (s: string) => void;
  localDesktop?: boolean;
  initialProvider?: Provider;
  onSaved?: () => void;
  onRecovered?: () => void;
}) {
  const [discoveryAccount, setDiscoveryAccount] = useState<Account | null>(null);
  const [accounts, setAccounts] = useState<Account[]>([]),
    [provider, setProvider] = useState<Provider>(initialProvider || "azure"),
    [name, setName] = useState(""),
    [subscription, setSubscription] = useState(""),
    [auth, setAuth] = useState("azure_cli"),
    [tenant, setTenant] = useState(""),
    [client, setClient] = useState(""),
    [ref, setRef] = useState(""),
    [secret, setSecret] = useState(""),
    [advanced, setAdvanced] = useState(!localDesktop),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [subscriptions, setSubscriptions] = useState<any[]>([]),
    [editing, setEditing] = useState<Account | null>(null),
    [showForm, setShowForm] = useState(true),
    [deleting, setDeleting] = useState<string | null>(null),
    [statuses, setStatuses] = useState<
      Record<string, { error: boolean; text: string; retry?: boolean }>
    >({});
  const saving = useRef(false);
  const draftId = useRef(crypto.randomUUID());
  const verify = async (account: Account) => {
    setStatuses((s) => ({
      ...s,
      [account.id]: { error: false, text: "Checking credentials…" },
    }));
    try {
      await api.request(`/accounts/${account.id}/validate`, "POST", {});
      setStatuses((s) => ({
        ...s,
        [account.id]: { error: false, text: "Credentials verified" },
      }));
      return true;
    } catch (e) {
      setStatuses((s) => ({
        ...s,
        [account.id]: {
          error: true,
          retry: true,
          text: `Account saved. Verification failed: ${String(e)}`,
        },
      }));
      return false;
    }
  };
  const edit = (account: Account | null) => {
    draftId.current = crypto.randomUUID();
    setEditing(account);
    setShowForm(true);
    setError("");
    setDeleting(null);
    setProvider(account?.provider || initialProvider || "azure");
    setName(account?.name || "");
    setSubscription(account?.settings.subscription_id || "");
    setAuth(account?.settings.auth || "azure_cli");
    setTenant(account?.settings.tenant_id || "");
    setClient(account?.settings.client_id || "");
    setRef(account?.credential_ref || "");
    setSubscriptions([]);
    setSecret("");
    setAdvanced(!localDesktop);
  };
  const azureSignIn = async (login: boolean) => {
    setBusy(true);
    setError("");
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const list = await invoke<any[]>("azure_accounts", { login });
      const available = list.filter((a) => a.state === "Enabled");
      setSubscriptions(available);
      if (available.length) {
        setSubscription(available[0].id);
        setTenant(available[0].tenantId);
      } else
        setError(
          "No active subscriptions found. Sign in with another Azure account.",
        );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => {
    void api
      .request<Account[]>("/accounts")
      .then((list) => {
        setAccounts(list);
        setShowForm(
          !!initialProvider ||
            !list.some(
              (a) => !["ssh", "docker", "ssh_docker"].includes(a.provider),
            ),
        );
      })
      .catch((e) => setError(String(e)));
  }, [api]);
  return (
    <div className="form">
      <div className="account-list">
        {accounts
          .filter((a) => !["ssh", "docker", "ssh_docker"].includes(a.provider))
          .map((a) => (
            <div key={a.id}>
              <Cloud size={19} />
              <span>
                <strong>{a.name}</strong>
                <small>{providerNames[a.provider]}</small>
                {statuses[a.id] && (
                  <small
                    role={statuses[a.id].error ? "alert" : "status"}
                    className={
                      statuses[a.id].error ? "inline-error" : "account-status"
                    }
                  >
                    {statuses[a.id].text}
                  </small>
                )}
                {deleting === a.id && (
                  <span>
                    <small>
                      Remove this account from the Manager? Cloud resources and
                      stored credentials are kept.
                    </small>
                    <button
                      type="button"
                      className="button compact danger"
                      disabled={busy}
                      onClick={async () => {
                        setBusy(true);
                        try {
                          await api.request(`/accounts/${a.id}`, "DELETE");
                          setAccounts((list) =>
                            list.filter((item) => item.id !== a.id),
                          );
                          setDeleting(null);
                        } catch (e) {
                          setStatuses((s) => ({
                            ...s,
                            [a.id]: { error: true, text: String(e) },
                          }));
                        } finally {
                          setBusy(false);
                        }
                      }}
                    >
                      Remove account
                    </button>
                    <button
                      type="button"
                      className="text-button"
                      disabled={busy}
                      onClick={() => setDeleting(null)}
                    >
                      Keep account
                    </button>
                  </span>
                )}
              </span>
              <button
                type="button"
                className="button compact secondary"
                disabled={busy}
                aria-label={`Edit ${a.name}`}
                onClick={() => edit(a)}
              >
                Edit
              </button>
              {a.provider === "azure" && <button type="button" className="button compact secondary" disabled={busy}
                aria-label={`Discover servers in ${a.name}`} onClick={() => setDiscoveryAccount(a)}>Discover servers</button>}
              <button
                type="button"
                className="button compact secondary"
                disabled={busy}
                aria-label={`Delete ${a.name}`}
                onClick={() => setDeleting(a.id)}
              >
                Delete
              </button>
              {statuses[a.id]?.retry && (
                <button
                  type="button"
                  className="button compact secondary"
                  disabled={busy}
                  onClick={async () => {
                    setBusy(true);
                    try {
                      await verify(a);
                    } finally {
                      setBusy(false);
                    }
                  }}
                >
                  Retry verification
                </button>
              )}
            </div>
          ))}
      </div>
      {discoveryAccount && <AzureDiscovery key={discoveryAccount.id} api={api} account={discoveryAccount}
        onAdded={onRecovered} onClose={() => setDiscoveryAccount(null)} />}
      {!showForm && (
        <button
          type="button"
          className="button secondary"
          disabled={busy}
          onClick={() => edit(null)}
        >
          <Plus size={16} />
          Add cloud account
        </button>
      )}
      {!showForm && error && (
        <p role="alert" className="inline-error">
          {error}
        </p>
      )}
      {showForm && (
        <form
          className="form"
          onSubmit={async (e) => {
            e.preventDefault();
            if (saving.current) return;
            saving.current = true;
            setBusy(true);
            setError("");
            try {
              let credentialRef = ref;
              if (secret) {
                const { invoke } = await import("@tauri-apps/api/core");
                credentialRef = await invoke<string>("store_cloud_credential", {
                  secret,
                });
                setRef(credentialRef);
                setSecret("");
              }
              const saved = await api.request<Account>("/accounts", "POST", {
                id: editing?.id || draftId.current,
                name: name || providerNames[provider],
                provider,
                credential_ref: credentialRef,
                settings: {
                  ...editing?.settings,
                  subscription_id: subscription,
                  auth,
                  tenant_id: tenant,
                  client_id: client,
                },
              });
              setAccounts((list) => [
                ...list.filter((a) => a.id !== saved.id),
                saved,
              ]);
              setEditing(saved);
              setShowForm(false);
              if (await verify(saved)) onSaved?.();
            } catch (e) {
              setError(String(e));
            } finally {
              saving.current = false;
              setBusy(false);
            }
          }}
        >
          <h3>{editing ? "Edit cloud account" : "Add a cloud account"}</h3>
          <div className="form-grid">
            <Field label="Provider">
              <select
                disabled={busy || !!editing}
                value={provider}
                onChange={(e) => setProvider(e.target.value as Provider)}
              >
                {Object.entries(providerNames)
                  .filter(
                    ([id]) => !["ssh", "docker", "ssh_docker"].includes(id),
                  )
                  .map(([id, name]) => (
                    <option key={id} value={id}>
                      {name}
                    </option>
                  ))}
              </select>
            </Field>
            <Field label="Display name">
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="My community account"
              />
            </Field>
          </div>
          {provider === "azure" && (
            <>
              {localDesktop && auth === "azure_cli" && (
                <>
                  <div className="button-row">
                    <button
                      type="button"
                      className="button secondary"
                      disabled={busy}
                      onClick={() => void azureSignIn(true)}
                    >
                      Sign in to Azure
                    </button>
                    <button
                      type="button"
                      className="text-button"
                      disabled={busy}
                      onClick={() => void azureSignIn(false)}
                    >
                      Use existing sign-in
                    </button>
                  </div>
                  <p>
                    Sign-in opens in your browser.{" "}
                    <a
                      href="https://learn.microsoft.com/cli/azure/install-azure-cli"
                      onClick={async (e) => {
                        e.preventDefault();
                        try {
                          const { invoke } =
                            await import("@tauri-apps/api/core");
                          await invoke("open_azure_help");
                        } catch (error) {
                          setError(String(error));
                        }
                      }}
                      target="_blank"
                      rel="noreferrer"
                    >
                      Install Azure CLI
                    </a>{" "}
                    if prompted.
                  </p>
                </>
              )}
              <Field
                label={
                  subscriptions.length ? "Subscription" : "Subscription ID"
                }
              >
                {subscriptions.length ? (
                  <select
                    value={subscription}
                    onChange={(e) => {
                      setSubscription(e.target.value);
                      setTenant(
                        subscriptions.find((a) => a.id === e.target.value)
                          ?.tenantId || "",
                      );
                    }}
                  >
                    {subscriptions.map((a) => (
                      <option key={a.id} value={a.id}>
                        {a.name} · {a.tenantDisplayName || a.tenantId}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    value={subscription}
                    onChange={(e) => setSubscription(e.target.value)}
                    placeholder="00000000-0000-0000-0000-000000000000"
                    required
                  />
                )}
              </Field>
              <Field label="Authentication">
                <select value={auth} onChange={(e) => setAuth(e.target.value)}>
                  <option value="azure_cli">
                    {localDesktop
                      ? "Azure account sign-in"
                      : "Azure CLI session"}
                  </option>
                  <option value="managed_identity">
                    Managed identity — Azure-hosted Manager
                  </option>
                  <option value="service_principal">
                    Service principal — unattended Manager
                  </option>
                </select>
              </Field>
              {auth === "service_principal" && (
                <div className="form-grid">
                  <Field label="Tenant ID">
                    <input
                      value={tenant}
                      onChange={(e) => setTenant(e.target.value)}
                      required
                    />
                  </Field>
                  <Field label="Client ID">
                    <input
                      value={client}
                      onChange={(e) => setClient(e.target.value)}
                      required
                    />
                  </Field>
                </div>
              )}
            </>
          )}
          {provider !== "ssh" &&
            (provider !== "azure" || auth === "service_principal") && (
              <>
                {localDesktop && !advanced ? (
                  <Field
                    label={provider === "azure" ? "Client secret" : "API token"}
                    hintId={`${draftId.current}-credential-hint`}
                    hint={
                      ref
                        ? "Leave blank to keep the saved credential."
                        : "Saved in your computer’s credential storage."
                    }
                  >
                    <input
                      type="password"
                      aria-label={
                        provider === "azure" ? "Client secret" : "API token"
                      }
                      aria-describedby={`${draftId.current}-credential-hint`}
                      autoComplete="new-password"
                      value={secret}
                      onChange={(e) => setSecret(e.target.value)}
                      required={!ref}
                    />
                  </Field>
                ) : (
                  <Field
                    label="Credential reference"
                    hint="Use a credential already stored for this Manager: keyring:name, vault:name, or env:VARIABLE."
                  >
                    <input
                      value={ref}
                      onChange={(e) => setRef(e.target.value)}
                      placeholder="keyring:my-cloud-token"
                      required
                    />
                  </Field>
                )}
                {localDesktop && (
                  <button
                    type="button"
                    className="text-button"
                    onClick={() => {
                      setAdvanced(!advanced);
                      setSecret("");
                    }}
                  >
                    {advanced
                      ? "Enter a credential"
                      : "Use a stored credential reference"}
                  </button>
                )}
              </>
            )}
          <p className="muted small">
            Adding an account does not create billable resources.
          </p>
          {error && (
            <p role="alert" className="inline-error">
              {error}
            </p>
          )}
          <button disabled={busy} className="button primary">
            <Plus size={16} />
            {busy
              ? "Saving and verifying…"
              : editing
                ? "Save changes"
                : "Save account"}
          </button>
          <button
            type="button"
            className="text-button"
            disabled={busy}
            onClick={() => {
              setShowForm(false);
              setError("");
            }}
          >
            Cancel
          </button>
        </form>
      )}
    </div>
  );
}
function CloudDeploy({
  api,
  onDone,
  localDesktop,
}: {
  api: Api;
  onDone: (id?: string, remote?: boolean) => void;
  localDesktop: boolean;
}) {
  const [versionReady, setVersionReady] = useState(false);
  const [accounts, setAccounts] = useState<Account[]>([]),
    [spec, setSpec] = useState({
      name: randomServerName(),
      account_id: "",
      region: "australiaeast",
      size: "",
      game_version: "",
      ssh_public_key: "",
      ssh_private_key: "",
      admin_cidr: "",
      existing_host: "",
      ssh_user: "vsm",
    }),
    [plan, setPlan] = useState<any>(null),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [ack, setAck] = useState(false),
    [addingAccount, setAddingAccount] = useState(false),
    [manual, setManual] = useState(false),
    [showAllSizes, setShowAllSizes] = useState(false),
    [expectedPlayers, setExpectedPlayers] = useState(6),
    [modIntensity, setModIntensity] = useState("light");
  const loadAccounts = () => {
    api
      .request<Account[]>("/accounts")
      .then((a) => {
        setAccounts(a);
        const cloudAccounts = a.filter(
          (item) => !["ssh", "docker", "ssh_docker"].includes(item.provider),
        );
        setAccounts(cloudAccounts);
        if (cloudAccounts.length)
          setSpec((s) => ({ ...s, account_id: cloudAccounts[0].id }));
      })
      .catch((e) => setError(String(e)));
  };
  useEffect(loadAccounts, [api]);
  const account = accounts.find((a) => a.id === spec.account_id);
  const {
    catalog,
    loading: catalogLoading,
    regionsLoading,
    error: catalogError,
    retry: retryCatalog,
  } = useCloudCatalog(
    api,
    account?.id || "",
    account?.provider || "",
    spec.region,
  );
  useEffect(() => {
    setManual(false);
    setShowAllSizes(false);
  }, [account?.id]);
  const regionKey = catalogOptions(account?.provider || "", catalog, "")
    .regions.map((r) => r.value)
    .join(",");
  useEffect(() => {
    if (regionsLoading || catalogError) return;
    const regions = catalogOptions(
      account?.provider || "",
      catalog,
      "",
    ).regions;
    setSpec((s) => {
      const match = regions.find((r) =>
        account?.provider === "azure"
          ? r.value.toLowerCase() === s.region.toLowerCase()
          : r.value === s.region,
      );
      return match
        ? match.value === s.region
          ? s
          : { ...s, region: match.value }
        : { ...s, region: "", size: "" };
    });
  }, [regionKey, regionsLoading, account?.id]);
  const options = catalogOptions(
    account?.provider || "azure",
    catalog,
    spec.region,
    { players: expectedPlayers, mods: modIntensity },
  );
  const selectedSize = options.allSizes.find((s) => s.value === spec.size);
  const priceCandidates = [
    ...options.sizes,
    ...(selectedSize ? [selectedSize] : []),
  ].filter((s) => s.monthlyPrice === undefined);
  const {
    prices: regionalPrices,
    loading: pricingLoading,
    errors: priceErrors,
  } = useInstancePrices(
    api,
    account?.id || "",
    spec.region,
    priceCandidates.map((s) => s.value),
    account?.provider === "azure",
  );
  const pricingUnavailable = priceErrors.length > 0;
  const sizeKey = options.allSizes.map((s) => s.value).join(",");
  useEffect(() => {
    if (manual || catalogLoading) return;
    setSpec((s) =>
      options.allSizes.some((option) => option.value === s.size)
        ? s
        : {
            ...s,
            size:
              options.sizes.find(
                (option) => option.recommendation === "recommended",
              )?.value || "",
          },
    );
  }, [sizeKey, manual, catalogLoading]);
  const field =
    (key: keyof typeof spec) =>
    (
      e: React.ChangeEvent<
        HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement
      >,
    ) =>
      setSpec({ ...spec, [key]: e.target.value });
  if (addingAccount || !accounts.length)
    return (
      <div className="form">
        <div className="wizard-steps">
          <span className="current">1. Account</span>
          <ChevronRight size={15} />
          <span>2. Server</span>
          <ChevronRight size={15} />
          <span>3. Review</span>
        </div>
        <Accounts
          api={api}
          localDesktop={localDesktop}
          onError={setError}
          onSaved={() => {
            setAddingAccount(false);
            loadAccounts();
          }}
        />
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
      </div>
    );
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
              accept_incomplete_estimate: ack,
            });
            onDone(plan.id, true);
          } else {
            setPlan(await api.request("/plans", "POST", spec));
          }
        } catch (e) {
          setError(String(e));
        } finally {
          setBusy(false);
        }
      }}
    >
      <div className="wizard-steps">
        <span>1. Account</span>
        <ChevronRight size={15} />
        <span className={!plan ? "current" : ""}>2. Server</span>
        <ChevronRight size={15} />
        <span className={plan ? "current" : ""}>3. Review</span>
      </div>
      {plan ? (
        <>
          <div className="review-card">
            <h3>{spec.name}</h3>
            <p>
              {account && providerNames[account.provider]} · {spec.region} ·{" "}
              {spec.size}
            </p>
            {plan.quote && (
              <>
                <div className="price">
                  {plan.quote.currency}{" "}
                  {plan.quote.monthly_compute == null
                    ? "Unknown"
                    : Number(plan.quote.monthly_compute).toFixed(2)}
                  <small>/ month compute</small>
                </div>
                <p>
                  Additional storage, networking, taxes and backups may apply.
                  This is not a complete bill estimate.
                </p>
              </>
            )}
            <ul>
              {plan.resources.map((r: string) => (
                <li key={r}>
                  <Check size={14} />
                  {r}
                </li>
              ))}
            </ul>
          </div>
          <div className="callout">
            <AlertTriangle size={19} />
            <div>
              <strong>This creates billable resources.</strong>
              <p>
                Stopping the game does not stop cloud billing. Review your
                provider’s prices before deploying.
              </p>
            </div>
          </div>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={ack}
              onChange={(e) => setAck(e.target.checked)}
              required
            />
            I accept the resource plan and any explicitly incomplete price
            estimate.
          </label>
          <div className="button-row">
            <button
              type="button"
              className="button secondary"
              onClick={() => setPlan(null)}
            >
              Back to configuration
            </button>
            <button disabled={busy || !ack} className="button primary">
              {busy ? (
                <Loader2 className="spin" size={16} />
              ) : (
                <Cloud size={16} />
              )}
              Deploy server
            </button>
          </div>
        </>
      ) : (
        <>
          {!accounts.length && (
            <div className="callout">
              <Cloud />
              <p>
                Add a cloud account first using Cloud accounts in the sidebar.
              </p>
            </div>
          )}
          <div className="form-grid">
            <Field label="Server name">
              <input
                value={spec.name}
                onChange={field("name")}
                placeholder="our-quiet-valley"
                pattern="[a-z0-9-]{3,40}"
                required
              />
              <button
                type="button"
                className="text-button"
                onClick={() => setSpec({ ...spec, name: randomServerName() })}
              >
                Suggest a name
              </button>
            </Field>
            <Field label="Cloud account">
              <select
                value={spec.account_id}
                onChange={field("account_id")}
                required
              >
                <option value="">Select an account</option>
                {accounts.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="Region">
              {manual || account?.provider === "ssh" ? (
                <input
                  value={spec.region}
                  onChange={field("region")}
                  required
                />
              ) : (
                <select
                  value={spec.region}
                  onChange={field("region")}
                  disabled={regionsLoading}
                  required
                >
                  <option value="">
                    {regionsLoading ? "Loading regions…" : "Select a region"}
                  </option>
                  {options.regions.map((r) => (
                    <option key={r.value} value={r.value}>
                      {r.label}
                    </option>
                  ))}
                </select>
              )}
            </Field>
            <Field label="Expected players">
              <input
                type="number"
                min={1}
                max={200}
                required
                value={expectedPlayers}
                onChange={(e) =>
                  setExpectedPlayers(
                    Math.max(1, Math.min(200, Number(e.target.value) || 1)),
                  )
                }
              />
            </Field>
            <Field label="Mods">
              <select
                value={modIntensity}
                onChange={(e) => setModIntensity(e.target.value)}
              >
                <option value="none">None</option>
                <option value="light">A few mods</option>
                <option value="heavy">Heavily modded</option>
              </select>
            </Field>
            {manual || account?.provider === "ssh" ? (
              <Field label="Instance size">
                <input value={spec.size} onChange={field("size")} required />
              </Field>
            ) : (
              <InstanceSizePicker
                options={options}
                emptyMessage={
                  !catalogError && spec.region
                    ? "No matching sizes were returned for this region. Try another region or refresh the catalog."
                    : undefined
                }
                value={spec.size}
                onChange={(size) => setSpec((s) => ({ ...s, size }))}
                showAll={showAllSizes}
                onShowAll={setShowAllSizes}
                loading={catalogLoading}
                prices={regionalPrices}
                pricingLoading={pricingLoading}
                priceErrors={priceErrors}
                players={expectedPlayers}
                mods={modIntensity}
              />
            )}
          </div>
          {account?.provider !== "ssh" && (
            <>
              {catalogError && (
                <p className="inline-error" role="alert">
                  Could not load available regions and sizes: {catalogError}
                  <button
                    type="button"
                    className="text-button"
                    onClick={retryCatalog}
                  >
                    Retry catalog
                  </button>
                </p>
              )}
              {pricingUnavailable && account?.provider === "azure" && (
                <p className="muted small" role="status">
                  Regional prices could not be loaded. The provider will
                  re-check the price during review.
                </p>
              )}
              <details>
                <summary>Advanced cloud configuration</summary>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={manual}
                    onChange={(e) => setManual(e.target.checked)}
                  />
                  Enter region and size identifiers manually
                </label>
              </details>
            </>
          )}
          <button
            type="button"
            className="text-button"
            onClick={() => setAddingAccount(true)}
          >
            Add another account
          </button>
          <div className="form-grid">
            <GameVersionPicker
              api={api}
              value={spec.game_version}
              onChange={(game_version) =>
                setSpec((s) => ({ ...s, game_version }))
              }
              onReady={setVersionReady}
              disabled={busy}
            />
            <Field
              label="Allow SSH access from (IPv4)"
              hint="Your public IPv4 address followed by /32. This limits who can administer the server."
            >
              <input
                value={spec.admin_cidr}
                onChange={field("admin_cidr")}
                placeholder="203.0.113.10/32"
                required
              />
            </Field>
          </div>
          {account?.provider === "ssh" && (
            <div className="form-grid">
              <Field label="Existing host">
                <input
                  value={spec.existing_host}
                  onChange={field("existing_host")}
                  required
                />
              </Field>
              <Field label="SSH user">
                <input
                  value={spec.ssh_user}
                  onChange={field("ssh_user")}
                  required
                />
              </Field>
            </div>
          )}
          {localDesktop && (
            <button
              type="button"
              className="button secondary"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                setError("");
                try {
                  const { invoke } = await import("@tauri-apps/api/core");
                  const key = await invoke<{
                    public_key: string;
                    private_key: string;
                  }>("create_ssh_key");
                  setSpec((s) => ({
                    ...s,
                    ssh_public_key: key.public_key,
                    ssh_private_key: key.private_key,
                  }));
                } catch (e) {
                  setError(String(e));
                } finally {
                  setBusy(false);
                }
              }}
            >
              Create server access key
            </button>
          )}
          <Field label="SSH public key">
            <textarea
              rows={2}
              value={spec.ssh_public_key}
              onChange={field("ssh_public_key")}
              placeholder="ssh-ed25519 AAAA…"
              required
            />
          </Field>
          <Field
            label="SSH private key file"
            hint={
              localDesktop
                ? "Stored on this computer."
                : "Path on the computer running Server Manager."
            }
          >
            <input
              value={spec.ssh_private_key}
              onChange={field("ssh_private_key")}
              placeholder="C:\Users\you\.ssh\id_ed25519 or /home/you/.ssh/id_ed25519"
              required
            />
          </Field>
          <button
            disabled={busy || !accounts.length || !versionReady}
            className="button primary"
          >
            {busy ? (
              <Loader2 className="spin" size={16} />
            ) : (
              <ArrowRight size={16} />
            )}
            Review deployment
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
function Team({ api, servers }: { api: Api; servers: Server[] }) {
  const [name, setName] = useState(""),
    [password, setPassword] = useState(""),
    [role, setRole] = useState("owner"),
    [totp, setTotp] = useState(""),
    [ids, setIds] = useState<string[]>([]),
    [message, setMessage] = useState("");
  return (
    <form
      className="form"
      onSubmit={async (e) => {
        e.preventDefault();
        try {
          await api.request("/users", "POST", {
            name,
            password,
            role,
            servers: ids,
            totp_secret: totp || null,
          });
          setMessage(
            "User created. Share credentials privately and enroll their authenticator.",
          );
          setPassword("");
        } catch (e) {
          setMessage(String(e));
        }
      }}
    >
      <p className="muted">
        Owners control cloud resources. Administrators manage assigned servers.
        Viewers can see status and sanitized logs.
      </p>
      <div className="form-grid">
        <Field label="Username">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
          />
        </Field>
        <Field label="Role">
          <select value={role} onChange={(e) => setRole(e.target.value)}>
            <option value="owner">Administrator</option>
            <option value="viewer">Viewer</option>
            <option value="owner">Owner</option>
          </select>
        </Field>
      </div>
      <Field label="Password">
        <input
          type="password"
          minLength={12}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          autoComplete="new-password"
          required
        />
      </Field>
      <Field
        label="TOTP secret (base32)"
        hint="Required for public dashboard write access. Enroll this secret in the user’s authenticator."
      >
        <input
          value={totp}
          onChange={(e) => setTotp(e.target.value)}
          autoComplete="off"
        />
      </Field>
      <div className="field">
        <span>Assigned servers</span>
        {servers.map((s) => (
          <label className="checkbox" key={s.id}>
            <input
              type="checkbox"
              checked={ids.includes(s.id)}
              onChange={(e) =>
                setIds(
                  e.target.checked
                    ? [...ids, s.id]
                    : ids.filter((id) => id !== s.id),
                )
              }
            />
            {s.name}
          </label>
        ))}
      </div>
      {message && <p role="status">{message}</p>}
      <button className="button primary">
        <Users size={16} />
        Create user
      </button>
    </form>
  );
}

type ServerPageProps = {
  page: Page;
  server: Server;
  api: Api;
  act: (action: string, body?: unknown) => void;
  perform: (fn: () => Promise<unknown>, message?: string) => Promise<void>;
  busy: boolean;
  localDesktop: boolean;
};
function ServerPage({
  page,
  server,
  api,
  act,
  perform,
  busy,
  localDesktop,
}: ServerPageProps) {
  const [data, setData] = useState<any>(null),
    [error, setError] = useState(""),
    [text, setText] = useState(""),
    [path, setPath] = useState(""),
    [file, setFile] = useState<{ path: string; etag: string } | null>(null),
    [command, setCommand] = useState(""),
    [search, setSearch] = useState(""),
    [version, setVersion] = useState(server.game_version),
    [versionReady, setVersionReady] = useState(false),
    [player, setPlayer] = useState(""),
    [deleteData, setDeleteData] = useState(false),
    [ack, setAck] = useState(false);
  const endpoint = (
    {
      Console: "logs",
      Players: "status",
      Settings: "config",
      Mods: "mods",
      Files: "files",
      Backups: "backups",
      Infrastructure: "status",
    } as Record<string, string>
  )[page];
  const cloudHost = !["ssh", "docker", "ssh_docker"].includes(server.provider);
  const infrastructure = useInfrastructureStatus(
    api,
    server.id,
    page === "Infrastructure" && cloudHost && server.state !== "deleted",
    server.state,
  );
  const vmPower = infrastructure.error
    ? "unknown"
    : infrastructure.status?.power_state || "unknown";
  const load = useCallback(async () => {
    try {
      const value = await api.request<any>(
        `/servers/${server.id}/agent/${endpoint}${page === "Files" ? `?path=${encodeURIComponent(path)}` : ""}`,
      );
      setData(value);
      if (page === "Settings") setText(JSON.stringify(value.value, null, 2));
      setError("");
    } catch (e) {
      setError(String(e));
    }
  }, [api, server.id, page, endpoint, path]);
  useEffect(() => {
    if (page === "Infrastructure") return;
    void load();
    if (["Console", "Players"].includes(page)) {
      const timer = setInterval(() => void load(), 5000);
      return () => clearInterval(timer);
    }
  }, [load, page]);
  const send = async () => {
    if (!command.trim()) return;
    await perform(
      () => api.action(server.id, "console", { command }),
      "Console command queued",
    );
    setCommand("");
  };
  if (page === "Infrastructure")
    return (
      <>
        <div className="two-column">
          <section className="panel detail-panel">
            <PanelTitle title="Host resources" icon={<Cloud size={18} />} />
            <dl>
              <dt>Provider</dt>
              <dd>{providerNames[server.provider]}</dd>
              <dt>Region</dt>
              <dd>{server.region}</dd>
              <dt>Instance</dt>
              <dd>{server.size}</dd>
              <dt>Public address</dt>
              <dd>{server.host || "Not assigned"}</dd>
              {cloudHost && (
                <>
                  <dt>VM power</dt>
                  <dd>
                    {infrastructure.loading && !infrastructure.status
                      ? "Checking…"
                      : vmPower.replaceAll("_", " ")}
                  </dd>
                  <dt>Cloud provisioning</dt>
                  <dd>
                    {infrastructure.status?.provisioning_state || "Unknown"}
                  </dd>
                </>
              )}
              <dt>Remote installation</dt>
              <dd>
                {server.state === "recovered" ? "Reconnect to check" : infrastructure.status?.remote_installed === true
                  ? "Installed"
                  : infrastructure.status?.remote_installed === false ||
                      server.state === "awaiting_bootstrap"
                    ? "Not installed"
                    : ["running", "sleeping"].includes(server.state)
                      ? "Installed"
                      : "Not confirmed"}
              </dd>
              <dt>Game</dt>
              <dd>
                {server.state === "recovered" ? "Reconnect to check" : infrastructure.status?.remote_installed === false ||
                server.state === "awaiting_bootstrap" ? (
                  "Not installed"
                ) : server.state === "attention" ? (
                  "Not confirmed — setup needs attention"
                ) : (
                  <Badge state={server.state} />
                )}
              </dd>
            </dl>
            {(infrastructure.status?.remote_installed === true ||
              ["running", "sleeping"].includes(server.state) ||
              (!cloudHost && server.state === "stopped")) && (
              <div className="form">
                <button
                  type="button"
                  className="button secondary"
                  disabled={
                    busy ||
                    [
                      "stopped",
                      "deallocated",
                      "starting",
                      "stopping",
                      "deallocating",
                    ].includes(vmPower)
                  }
                  onClick={() => act("agent-update")}
                >
                  Update Remote
                </button>
                <p className="muted small">
                  Updates the management software. The game stops safely and
                  resumes if it was running.
                </p>
              </div>
            )}
            {cloudHost && (
              <>
                {infrastructure.error && (
                  <p role="alert" className="inline-error">
                    Could not check VM status. {infrastructure.error}
                  </p>
                )}
                {infrastructure.status?.checked_at && (
                  <p className="muted small">
                    Checked{" "}
                    {new Date(
                      infrastructure.status.checked_at,
                    ).toLocaleTimeString()}
                  </p>
                )}
                <button
                  type="button"
                  className="text-button"
                  disabled={infrastructure.loading}
                  onClick={infrastructure.refresh}
                >
                  {infrastructure.loading
                    ? "Checking VM status…"
                    : "Refresh VM status"}
                </button>
              </>
            )}
            <button
              className="button secondary"
              onClick={() =>
                void perform(async () => {
                  const info = await api.request(
                    `/servers/${server.id}/infrastructure`,
                  );
                  download(
                    `${server.name}-infrastructure.json`,
                    JSON.stringify(info, null, 2),
                  );
                }, "Infrastructure inspection downloaded")
              }
            >
              Download resource details
              <ArrowDownToLine size={15} />
            </button>
          </section>
          <HostSetup
            server={server}
            local={localDesktop}
            act={act}
            powerState={cloudHost ? vmPower : undefined}
            remoteInstalled={infrastructure.status?.remote_installed}
            reconnect={async (keyPath) => {
              await api.request(`/servers/${server.id}/connection`, "PATCH", { ssh_private_key: keyPath });
              await api.request(`/servers/${server.id}/reconnect`, "POST", {});
              await perform(async () => { await infrastructure.refresh(); }, "Remote reconnected");
            }}
          />
        </div>
        {!["ssh", "docker", "ssh_docker"].includes(server.provider) && (
          <section className="panel detail-panel">
            <PanelTitle
              title="Power & billing"
              icon={<HardDrive size={18} />}
            />
            <div className="callout">
              <AlertTriangle size={18} />
              <p>
                VM shutdown can leave compute billing active. Azure deallocation
                releases compute; disks and public IPs can still incur charges.
                Other providers may bill stopped VMs.
              </p>
            </div>
            <div className="button-row">
              <button
                className="button secondary"
                disabled={
                  busy ||
                  ["running", "starting", "stopping", "deallocating"].includes(
                    vmPower,
                  )
                }
                onClick={() => act("cloud-start")}
              >
                Start VM
              </button>
              <button
                className="button secondary"
                disabled={
                  busy ||
                  [
                    "stopped",
                    "deallocated",
                    "starting",
                    "stopping",
                    "deallocating",
                  ].includes(vmPower) || server.state === "recovered"
                }
                onClick={() => act("cloud-restart")}
              >
                Restart VM
              </button>
              <button
                className="button secondary"
                disabled={
                  busy ||
                  [
                    "deallocated",
                    "starting",
                    "stopping",
                    "deallocating",
                  ].includes(vmPower) || server.state === "recovered"
                }
                onClick={() => act("deallocate")}
              >
                {server.provider === "azure" ? "Deallocate VM" : "Shut down VM"}
              </button>
            </div>
          </section>
        )}
        {["docker", "ssh_docker"].includes(server.provider) &&
          server.state !== "deleted" && (
            <section className="panel detail-panel danger-zone">
              <h3>Remove Docker deployment</h3>
              <p>
                Stops the game and removes its container. The persistent volume
                is kept unless you choose to delete game data.
              </p>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={deleteData}
                  onChange={(e) => setDeleteData(e.target.checked)}
                />
                Also permanently delete the game data volume
              </label>
              {deleteData && (
                <p>
                  Worlds, settings, mods, and local backups in this volume will
                  be deleted. Export a backup before continuing.
                </p>
              )}
              <Field label="Type the server name to confirm">
                <input
                  value={command}
                  onChange={(e) => setCommand(e.target.value)}
                  placeholder={server.name}
                />
              </Field>
              <button
                className="button danger"
                disabled={command !== server.name || busy}
                onClick={() =>
                  act("delete", {
                    confirm_name: command,
                    delete_data: deleteData,
                  })
                }
              >
                Remove container{deleteData ? " and data" : ""}
              </button>
            </section>
          )}
        {!["ssh", "docker", "ssh_docker"].includes(server.provider) && (
          <section className="panel detail-panel danger-zone">
            <h3>Delete infrastructure</h3>
            <p>
              Stops the server and deletes verified owned resources. Export your
              world and verify your off-host backups first. The Azure resource
              group is retained.
            </p>
            <Field label="Type the server name to confirm">
              <input
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                placeholder={server.name}
              />
            </Field>
            <button
              className="button danger"
              disabled={command !== server.name || busy || server.state === "recovered"}
              onClick={() => act("delete", { confirm_name: command })}
            >
              <Trash2 size={16} />
              Delete resources
            </button>
          </section>
        )}
      </>
    );
  return (
    <>
      {error && (
        <div className="callout">
          <AlertTriangle size={18} />
          <div>
            <strong>Agent connection needs attention</strong>
            <p>{error}</p>
            <p>Check the host connection in Infrastructure.</p>
          </div>
        </div>
      )}
      {page === "Console" && (
        <section className="panel console-panel">
          <PanelTitle
            title="Server console"
            icon={<Terminal size={18} />}
            action={
              <div className="toolbar">
                <input
                  aria-label="Filter logs"
                  placeholder="Filter output…"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                />
                <button
                  className="icon-button"
                  aria-label="Download logs"
                  onClick={() =>
                    download(
                      "console.json",
                      JSON.stringify(data || [], null, 2),
                    )
                  }
                >
                  <ArrowDownToLine size={17} />
                </button>
              </div>
            }
          />
          <div className="console-output" role="log" aria-live="off">
            {Array.isArray(data) && data.length ? (
              data
                .filter((l: any) =>
                  l.message.toLowerCase().includes(search.toLowerCase()),
                )
                .map((l: any, i: number) => (
                  <div className={l.level} key={i}>
                    <time>{new Date(l.at).toLocaleTimeString()}</time>
                    <span>{l.message}</span>
                  </div>
                ))
            ) : (
              <div className="console-placeholder">
                <Terminal size={26} />
                <p>Server output will appear here.</p>
              </div>
            )}
          </div>
          <form
            className="console-input"
            onSubmit={(e) => {
              e.preventDefault();
              void send();
            }}
          >
            <ChevronRight size={17} />
            <input
              aria-label="Server command"
              placeholder="Enter a server command…"
              value={command}
              onChange={(e) => setCommand(e.target.value)}
            />
            <button
              className="button compact primary"
              disabled={busy || !command.trim()}
            >
              Send
              <ArrowRight size={14} />
            </button>
          </form>
        </section>
      )}
      {page === "Players" && (
        <div className="two-column">
          <section className="panel detail-panel">
            <PanelTitle title="Player access" icon={<Users size={18} />} />
            <p>Enter the exact Vintage Story username.</p>
            <Field label="Player username">
              <input
                value={player}
                onChange={(e) => setPlayer(e.target.value)}
                placeholder="Exact in-game username"
              />
            </Field>
            <div className="button-row">
              <button
                className="button primary"
                disabled={!/^[a-zA-Z0-9_-]+$/.test(player)}
                onClick={() =>
                  act("console", { command: `player ${player} whitelist on` })
                }
              >
                <Plus size={15} />
                Whitelist
              </button>
              <button
                className="button secondary"
                disabled={!/^[a-zA-Z0-9_-]+$/.test(player)}
                onClick={() =>
                  act("console", { command: `player ${player} whitelist off` })
                }
              >
                Remove
              </button>
            </div>
            <button
              className="button secondary"
              onClick={() => act("console", { command: "list clients" })}
            >
              List connected players in console
            </button>
          </section>
          <section className="panel detail-panel">
            <PanelTitle title="Announcements" icon={<Globe2 size={18} />} />
            <Field label="Message to all players">
              <textarea
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                placeholder="Server restarting in five minutes…"
                rows={4}
              />
            </Field>
            <button
              className="button primary"
              disabled={!command.trim()}
              onClick={() => act("console", { command: `announce ${command}` })}
            >
              Send announcement
              <ArrowRight size={15} />
            </button>
          </section>
        </div>
      )}
      {page === "Settings" && (
        <>
          <section className="panel detail-panel">
            <PanelTitle
              title="Server configuration"
              icon={<Settings size={18} />}
            />
            <p className="muted">Stop the server before editing.</p>
            <ServerConfigForm value={text} onChange={setText} />
            <details>
              <summary>Advanced configuration (JSON)</summary>
              <textarea
                aria-label="Server configuration JSON"
                className="code-editor"
                value={text}
                onChange={(e) => setText(e.target.value)}
                spellCheck={false}
              />
            </details>
            <div className="button-row">
              <button
                className="button primary"
                disabled={
                  !data ||
                  busy ||
                  ["running", "starting"].includes(server.state) ||
                  validateServerConfig(text).length > 0
                }
                onClick={() =>
                  void perform(async () => {
                    const issues = validateServerConfig(text);
                    if (issues.length) throw new Error(issues.join(" "));
                    await api.request(
                      `/servers/${server.id}/agent/config`,
                      "PUT",
                      { value: JSON.parse(text), etag: data.etag },
                    );
                    await load();
                  }, "Configuration saved")
                }
              >
                <Check size={16} />
                Save configuration
              </button>
              <button className="button secondary" onClick={() => void load()}>
                Reload
              </button>
            </div>
          </section>
          <section className="panel detail-panel">
            <PanelTitle title="Game version" icon={<Package size={18} />} />
            <p>
              Updating stops the server and creates a recovery copy. Check mod
              compatibility first.
            </p>
            <GameVersionPicker
              api={api}
              value={version}
              installedVersion={server.game_version}
              onChange={setVersion}
              onReady={setVersionReady}
              disabled={busy}
            />
            <button
              className="button secondary"
              disabled={
                busy || !versionReady || version === server.game_version
              }
              onClick={() => act("update", { version })}
            >
              Install selected release
              <ArrowRight size={15} />
            </button>
          </section>
        </>
      )}
      {page === "Files" && (
        <section className="panel detail-panel">
          <PanelTitle
            title="Server files"
            icon={<FolderOpen size={18} />}
            action={
              <button
                className="icon-button"
                aria-label="Refresh files"
                onClick={() => void load()}
              >
                <RefreshCw size={16} />
              </button>
            }
          />
          <form
            className="path-bar"
            onSubmit={(e) => {
              e.preventDefault();
              void load();
            }}
          >
            <Folder size={16} />
            <span>data /</span>
            <input
              aria-label="Directory path"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder="Root directory"
            />
          </form>
          {path && (
            <button
              className="text-button"
              onClick={() => setPath(path.split("/").slice(0, -1).join("/"))}
            >
              ↑ Parent folder
            </button>
          )}
          <div className="file-list">
            {Array.isArray(data) &&
              data.map((f: any) => (
                <button
                  key={f.name}
                  onClick={async () => {
                    const next = [path, f.name].filter(Boolean).join("/");
                    if (f.directory) {
                      setPath(next);
                      return;
                    }
                    await perform(async () => {
                      const content = await api.request<{
                        content_base64: string;
                        etag: string;
                      }>(
                        `/servers/${server.id}/agent/file?path=${encodeURIComponent(next)}`,
                      );
                      let decoded: string | undefined;
                      try {
                        decoded = decode64(content.content_base64);
                      } catch {
                        /* Binary download below. */
                      }
                      if (
                        decoded === undefined ||
                        /[\x00-\x08\x0e-\x1f]/.test(decoded) ||
                        /\.(vcdbs|zip|db|dll|png|jpe?g)$/i.test(next)
                      ) {
                        download(
                          f.name,
                          Uint8Array.from(atob(content.content_base64), (c) =>
                            c.charCodeAt(0),
                          ),
                          "application/octet-stream",
                        );
                        setFile(null);
                      } else {
                        setFile({ path: next, etag: content.etag });
                        setText(decoded);
                      }
                    }, "File opened");
                  }}
                >
                  {f.directory ? <Folder size={18} /> : <FileText size={18} />}
                  <span>{f.name}</span>
                  <small>
                    {f.directory
                      ? "Folder"
                      : `${(f.bytes / 1024).toFixed(1)} KB`}
                  </small>
                  <ChevronRight size={14} />
                </button>
              ))}
          </div>
          {file && (
            <>
              <h3>{file.path}</h3>
              <textarea
                aria-label="File contents"
                className="code-editor"
                value={text}
                onChange={(e) => setText(e.target.value)}
              />
              <div className="button-row">
                <button
                  className="button primary"
                  onClick={() =>
                    void perform(async () => {
                      const result = await api.request<{ etag: string }>(
                        `/servers/${server.id}/agent/files`,
                        "POST",
                        {
                          path: file.path,
                          content_base64: encode64(text),
                          etag: file.etag,
                        },
                      );
                      setFile({ ...file, etag: result.etag });
                    }, "File saved")
                  }
                >
                  Save changes
                </button>
                <button
                  className="button secondary"
                  onClick={() =>
                    download(file.path.split("/").pop()!, text, "text/plain")
                  }
                >
                  <ArrowDownToLine size={15} />
                  Download text
                </button>
              </div>
            </>
          )}
        </section>
      )}
      {page === "Backups" && (
        <>
          <div className="two-column">
            <section className="panel detail-panel">
              <PanelTitle
                title="Recovery copies"
                icon={<Database size={18} />}
              />
              <p>Keeps the latest three copies on the server disk.</p>
              <button
                className="button primary"
                disabled={busy}
                onClick={() => act("backup")}
              >
                <Plus size={16} />
                Create backup
              </button>
              <div className="recovery-list">
                {data?.recovery_points?.map((r: any) => (
                  <div key={r.name}>
                    <Database size={18} />
                    <span>{r.name}</span>
                    <button
                      className="button compact secondary"
                      onClick={() => {
                        if (
                          confirm(
                            `Restore ${r.name}? The server will stop and current data will be preserved first.`,
                          )
                        )
                          act("restore", { id: r.name });
                      }}
                    >
                      Restore
                    </button>
                  </div>
                ))}
              </div>
            </section>
            <section className="panel detail-panel">
              <PanelTitle
                title="Off-host backups"
                icon={<ShieldCheck size={18} />}
              />
              <Badge state={data?.policy?.enabled ? "enabled" : "disabled"} />
              <p>
                Local copies share your server’s disk. Encrypted off-host
                backups protect against losing the host.
              </p>
              {data?.policy && (
                <BackupForm
                  policy={data.policy}
                  save={(p) =>
                    perform(async () => {
                      await api.request(
                        `/servers/${server.id}/agent/backup-policy`,
                        "PUT",
                        p,
                      );
                      await load();
                    }, "Backup policy saved")
                  }
                />
              )}
            </section>
          </div>
        </>
      )}
    </>
  );
}
function BackupForm({
  policy,
  save,
}: {
  policy: any;
  save: (p: any) => Promise<void>;
}) {
  const [p, setP] = useState(policy);
  return (
    <form
      className="form"
      onSubmit={(e) => {
        e.preventDefault();
        void save(p);
      }}
    >
      <label className="checkbox">
        <input
          type="checkbox"
          checked={p.enabled}
          onChange={(e) => setP({ ...p, enabled: e.target.checked })}
        />
        Enable scheduled off-host backups
      </label>
      <Field label="Restic repository">
        <input
          value={p.repository}
          onChange={(e) => setP({ ...p, repository: e.target.value })}
          placeholder="s3:https://endpoint/bucket"
        />
      </Field>
      <Field label="Password environment variable on the host">
        <input
          value={p.password_env}
          onChange={(e) => setP({ ...p, password_env: e.target.value })}
        />
      </Field>
      <Field label="Daily backup hour (UTC)">
        <input
          type="number"
          min={0}
          max={23}
          value={p.hour_utc}
          onChange={(e) => setP({ ...p, hour_utc: Number(e.target.value) })}
        />
      </Field>
      <p className="muted small">
        Initialize the repository and configure storage credentials on the
        server host first. Retains 7 daily and 4 weekly copies.
      </p>
      <button className="button secondary">Save backup policy</button>
    </form>
  );
}
