import WorldCreator from "./WorldCreator";
import IdleSettings from "./IdleSettings";
import CloudIdleSettings from "./CloudIdleSettings";
import { useEffect, useState } from "react";
import {
  ArrowDownToLine,
  Clock,
  Database,
  FolderPlus,
  Upload,
  RefreshCw,
} from "lucide-react";
import { Api, type Server } from "./api";
type Props = {
  page: string;
  localManager?: boolean;
  server: Server;
  api: Api;
  act: (action: string, body?: unknown) => void;
  perform: (fn: () => Promise<unknown>, message?: string) => Promise<void>;
};
export default function Workflows({
  page,
  server,
  api,
  act,
  perform,
  localManager = false,
}: Props) {
  const [worlds, setWorlds] = useState<any[]>([]),
    [world, setWorld] = useState(""),
    [path, setPath] = useState(""),
    [destination, setDestination] = useState(""),
    [schedule, setSchedule] = useState<any>({ enabled: false, hour_utc: 4 }),
    [size, setSize] = useState(server.size),
    [snapshot, setSnapshot] = useState(""),
    [resizeQuote, setResizeQuote] = useState<any>(null),
    [resizeError, setResizeError] = useState(""),
    [resizeAccepted, setResizeAccepted] = useState(false);
  useEffect(() => {
    if (page === "Settings") {
      api
        .request(`/servers/${server.id}/agent/schedule`)
        .then(setSchedule)
        .catch(() => {});
      api
        .request<any[]>(`/servers/${server.id}/agent/worlds`)
        .then(setWorlds)
        .catch(() => {});
    }
  }, [page, api, server.id]);
  if (page === "Mods") return null;
  if (page === "Files")
    return (
      <section className="panel detail-panel">
        <h3>File operations</h3>
        <p>Stop the game before changing files. Deleted files move to trash.</p>
        <div className="form-grid">
          <label className="field">
            <span>Relative path</span>
            <input
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder="Saves/my-world.vcdbs"
            />
          </label>
          <label className="field">
            <span>Destination (move or extract)</span>
            <input
              value={destination}
              onChange={(e) => setDestination(e.target.value)}
              placeholder="Saves/renamed-world.vcdbs"
            />
          </label>
        </div>
        <div className="button-row">
          <button
            className="button secondary"
            disabled={!path}
            onClick={() => act("file-mkdir", { path })}
          >
            <FolderPlus size={15} />
            Create folder
          </button>
          <button
            className="button secondary"
            disabled={!path || !destination}
            onClick={() => act("file-move", { path, destination })}
          >
            Move / rename
          </button>
          <button
            className="button secondary"
            disabled={!path || !destination}
            onClick={() => act("file-extract", { path, destination })}
          >
            Extract ZIP
          </button>
          <button
            className="button danger"
            disabled={!path}
            onClick={() => act("file-delete", { path })}
          >
            Move to trash
          </button>
          <label className="button secondary upload-button">
            <Upload size={15} />
            Upload file
            <input
              type="file"
              onChange={async (e) => {
                const file = e.target.files?.[0];
                if (!file) return;
                await perform(async () => {
                  const bytes = new Uint8Array(await file.arrayBuffer());
                  if (bytes.length > 64 * 1024 * 1024)
                    throw new Error("Transfer limit is 64 MiB");
                  let content = "";
                  for (const b of bytes) content += String.fromCharCode(b);
                  await api.request(
                    `/servers/${server.id}/agent/files`,
                    "POST",
                    { path: path || file.name, content_base64: btoa(content) },
                  );
                }, "File uploaded; refresh the directory to see it");
              }}
            />
          </label>
        </div>
      </section>
    );
  if (page === "Settings")
    return (
      <div className="two-column">
        <section className="panel detail-panel">
          <h3>World selection</h3>
          <p>
            Import a .vcdbs file into Saves using Files, then select it here.
            Switching worlds preserves a recovery copy.
          </p>
          <label className="field">
            <span>World</span>
            <select value={world} onChange={(e) => setWorld(e.target.value)}>
              <option value="">Select an imported world</option>
              {worlds.map((w) => (
                <option key={w.name}>{w.name}</option>
              ))}
            </select>
          </label>
          <button
            className="button secondary"
            disabled={!world}
            onClick={() => act("world-switch", { filename: world })}
          >
            Switch world
          </button>
        </section>
        <WorldCreator api={api} server={server} act={act} />
        <section className="panel detail-panel">
          <h3>Scheduled restart</h3>
          <p>
            Players receive a 30-second warning. Runs even when the app is
            closed.
          </p>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={schedule.enabled}
              onChange={(e) =>
                setSchedule({ ...schedule, enabled: e.target.checked })
              }
            />
            Enable daily restart
          </label>
          <label className="field">
            <span>Hour (UTC)</span>
            <input
              type="number"
              min={0}
              max={23}
              value={schedule.hour_utc}
              onChange={(e) =>
                setSchedule({ ...schedule, hour_utc: Number(e.target.value) })
              }
            />
          </label>
          <button
            className="button secondary"
            onClick={() =>
              void perform(
                () =>
                  api.request(
                    `/servers/${server.id}/agent/schedule`,
                    "PUT",
                    schedule,
                  ),
                "Restart schedule saved",
              )
            }
          >
            <Clock size={15} />
            Save schedule
          </button>
        </section>
        {server.provider === "azure" ? (
          <CloudIdleSettings
            api={api}
            server={server}
            localManager={localManager}
          />
        ) : (
          <IdleSettings api={api} serverId={server.id} />
        )}
      </div>
    );
  if (page === "Backups")
    return (
      <section className="panel detail-panel">
        <h3>Backup repository tools</h3>
        <p>Add backup credentials on the host before using these tools.</p>
        <div className="button-row">
          <button
            className="button secondary"
            onClick={() => act("backup-init")}
          >
            Initialize repository
          </button>
          <button
            className="button secondary"
            onClick={() => act("backup-check")}
          >
            Check repository
          </button>
          <button
            className="button secondary"
            onClick={() => act("backup-snapshots")}
          >
            List snapshots in Activity
          </button>
        </div>
        <div className="divider" />
        <label className="field">
          <span>Snapshot ID to restore</span>
          <input
            value={snapshot}
            onChange={(e) => setSnapshot(e.target.value)}
            placeholder="Exact restic snapshot ID"
          />
        </label>
        <button
          className="button secondary"
          disabled={!snapshot}
          onClick={() => {
            if (
              confirm(
                "Restore this backup snapshot? The game server will stop and current data will be preserved.",
              )
            )
              act("backup-restore", { snapshot });
          }}
        >
          <ArrowDownToLine size={15} />
          Restore backup snapshot
        </button>
      </section>
    );
  if (
    page === "Infrastructure" &&
    !["ssh", "docker", "ssh_docker"].includes(server.provider)
  )
    return (
      <>
        {server.provider === "azure" && (
          <CloudIdleSettings
            api={api}
            server={server}
            localManager={localManager}
          />
        )}
        <section className="panel detail-panel">
          <h3>Resize instance</h3>
          <p>Resizing stops the game and may require VM deallocation.</p>
          <label className="field">
            <span>New size identifier</span>
            <input
              value={size}
              onChange={(e) => {
                setSize(e.target.value);
                setResizeQuote(null);
                setResizeAccepted(false);
              }}
            />
          </label>
          <button
            className="button secondary"
            disabled={!size || size === server.size}
            onClick={async () => {
              setResizeError("");
              const requestedSize = size;
              try {
                const quote = await api.request<any>(
                  `/accounts/${server.account_id}/quote?region=${encodeURIComponent(server.region)}&size=${encodeURIComponent(requestedSize)}`,
                );
                setResizeQuote({ ...quote, requestedSize });
                setResizeAccepted(false);
              } catch (e) {
                setResizeError(String(e));
              }
            }}
          >
            <RefreshCw size={15} />
            Review resize cost
          </button>
          {resizeQuote && resizeQuote.requestedSize === size && (
            <div className="form review-card">
              <p>
                {resizeQuote.currency}{" "}
                {resizeQuote.monthly_compute == null
                  ? "Unknown"
                  : Number(resizeQuote.monthly_compute).toFixed(2)}{" "}
                / month compute
              </p>
              <p>
                Disk, networking, backup, and tax charges are additional. This
                is not a complete bill estimate.
              </p>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={resizeAccepted}
                  onChange={(e) => setResizeAccepted(e.target.checked)}
                />
                I accept the changed recurring cost and downtime.
              </label>
              <button
                className="button primary"
                disabled={!resizeAccepted}
                onClick={() => {
                  act("resize", { size, accept_cost_change: true });
                  setResizeQuote(null);
                  setResizeAccepted(false);
                }}
              >
                Resize instance
              </button>
            </div>
          )}
          {resizeError && (
            <p className="inline-error" role="alert">
              {resizeError}
            </p>
          )}
        </section>
      </>
    );
  return null;
}
