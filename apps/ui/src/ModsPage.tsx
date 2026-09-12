import { useCallback, useEffect, useRef, useState } from "react";
import { Loader2, Package, RefreshCw, Upload } from "lucide-react";
import { Api, type Operation, type Server } from "./api";
import ModBrowser from "./ModBrowser";
import OperationDetails from "./OperationDetails";
type Pending = { operation: Operation; name?: string; version?: string };
const active = (op: Operation) => ["queued", "running"].includes(op.status);
const modOperation = (op: Operation) => op.action.startsWith("mod-");
async function bounded<T>(
  api: Api,
  path: string,
  signal: AbortSignal,
): Promise<T> {
  if (signal.aborted) throw new Error("Request cancelled");
  const controller = new AbortController();
  const cancel = () => controller.abort();
  signal.addEventListener("abort", cancel, { once: true });
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      api.request<T>(path, "GET", undefined, controller.signal),
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => {
          reject(new Error("The request timed out. Try refreshing."));
          controller.abort();
        }, 15000);
      }),
    ]);
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", cancel);
  }
}
export default function ModsPage({
  api,
  server,
  operations,
}: {
  api: Api;
  server: Server;
  operations: Operation[];
}) {
  const [mods, setMods] = useState<any[]>([]),
    [pending, setPending] = useState<Pending[]>([]),
    [error, setError] = useState(""),
    [loading, setLoading] = useState(true),
    [queuing, setQueuing] = useState(false),
    [ack, setAck] = useState(false),
    [pollAttempt, setPollAttempt] = useState(0);
  const scope = useRef(new AbortController()),
    queueLock = useRef(false),
    pendingIds = useRef(new Set<string>()),
    refreshSequence = useRef(0),
    upload = useRef<HTMLInputElement>(null);
  const refresh = useCallback(async () => {
    const sequence = ++refreshSequence.current;
    const context = scope.current;
    setLoading(true);
    try {
      const value = await bounded<any[]>(
        api,
        `/servers/${server.id}/agent/mods`,
        context.signal,
      );
      if (!context.signal.aborted && sequence === refreshSequence.current) {
        setMods(Array.isArray(value) ? value : []);
        setError("");
      }
    } catch (e) {
      if (!context.signal.aborted && sequence === refreshSequence.current)
        setError(String(e));
    } finally {
      if (!context.signal.aborted && sequence === refreshSequence.current)
        setLoading(false);
    }
  }, [api, server.id]);
  useEffect(() => {
    scope.current = new AbortController();
    void refresh();
    return () => scope.current.abort();
  }, [refresh]);
  useEffect(() => {
    setPending((old) => {
      const additions = operations.filter(
        (op) =>
          op.server_id === server.id &&
          modOperation(op) &&
          active(op) &&
          !old.some((p) => p.operation.id === op.id),
      );
      additions.forEach((op) => pendingIds.current.add(op.id));
      return additions.length
        ? [...old, ...additions.map((operation) => ({ operation }))]
        : old;
    });
  }, [operations, server.id]);
  const waiting = pending.filter((p) => active(p.operation));
  const waitKey = waiting
    .map((p) => p.operation.id)
    .sort()
    .join(",");
  useEffect(() => {
    if (!waitKey) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const values: Operation[] = [];
        for (const id of waitKey.split(","))
          values.push(
            await bounded<Operation>(
              api,
              `/operations/${id}`,
              controller.signal,
            ),
          );
        if (controller.signal.aborted) return;
        if (values.some((op) => !active(op))) await refresh();
        if (controller.signal.aborted) return;
        values
          .filter((op) => !active(op))
          .forEach((op) => pendingIds.current.delete(op.id));
        setPending((old) =>
          old.map((p) => ({
            ...p,
            operation:
              values.find((op) => op.id === p.operation.id) || p.operation,
          })),
        );
      } catch (e) {
        if (!controller.signal.aborted) setError(String(e));
      } finally {
        if (!controller.signal.aborted)
          timer = setTimeout(() => void poll(), 3000);
      }
    };
    void poll();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [api, waitKey, refresh, pollAttempt]);
  const blocked = queuing || waiting.length > 0;
  async function queue(
    action: string,
    body: unknown,
    label?: { name: string; version?: string },
    custom = false,
  ) {
    if (queueLock.current || pendingIds.current.size > 0)
      throw new Error("Wait for the current mod operation to finish.");
    const context = scope.current;
    queueLock.current = true;
    setQueuing(true);
    setError("");
    try {
      const operation = custom
        ? await api.request<Operation>(
            `/servers/${server.id}/agent/mods`,
            "POST",
            body,
          )
        : await api.action(server.id, action, body);
      if (!operation?.id)
        throw new Error(
          "The Manager did not return an operation. Refresh Activity before retrying.",
        );
      if (!context.signal.aborted && active(operation))
        pendingIds.current.add(operation.id);
      if (!context.signal.aborted)
        setPending((old) => [
          ...old.filter((p) => p.operation.id !== operation.id),
          { operation, ...label },
        ]);
    } finally {
      queueLock.current = false;
      if (!context.signal.aborted) setQueuing(false);
    }
  }
  return (
    <>
      <section className="panel detail-panel">
        <div className="panel-title">
          <h3>Installed mods</h3>
          <button
            type="button"
            className="icon-button"
            aria-label="Refresh installed mods"
            disabled={loading}
            onClick={() => {
              void refresh();
              setPollAttempt((n) => n + 1);
            }}
          >
            <RefreshCw size={16} />
          </button>
        </div>
        {pending
          .filter((p) => p.operation.status !== "succeeded")
          .map((p) => (
            <div className="review-card" key={p.operation.id}>
              <strong>
                {p.name || "Mod operation"}
                {p.version ? ` ${p.version}` : ""}
              </strong>
              <p role={active(p.operation) ? "status" : "alert"}>
                {active(p.operation) ? (
                  <>
                    <Loader2 size={14} className="spin" />{" "}
                    {p.operation.action === "mod-install" ||
                    p.operation.action === "mod-upload"
                      ? "Installing…"
                      : "Applying mod change…"}
                  </>
                ) : (
                  `${p.operation.status === "cancelled" ? "Cancelled" : p.operation.status === "interrupted" ? "Interrupted" : "Failed"}: ${p.operation.message}`
                )}
              </p>
              {!active(p.operation) && (
                <p className="muted small">
                  Review the details, then select the mod or ZIP again to retry.
                </p>
              )}
              <OperationDetails api={api} operation={p.operation} />
            </div>
          ))}
        {error && (
          <p role="alert" className="inline-error">
            {error}
          </p>
        )}
        {error && pending.some((p) => p.operation.status === "succeeded") && (
          <p role="status">
            The mod operation completed. Refresh the installed list to check its
            current contents.
          </p>
        )}
        {loading && !mods.length ? (
          <p role="status">Loading installed mods…</p>
        ) : !mods.length ? (
          <p className="muted">No mods installed.</p>
        ) : (
          <div className="mod-list">
            {mods.map((mod) => (
              <div key={mod.modid}>
                <ModIcon source={mod.icon_data_url} />
                <span>
                  <strong>{mod.modid}</strong>
                  <small>{mod.filename}</small>
                </span>
                <span className="tiny-tag">{mod.version}</span>
                <span className="muted">
                  {mod.compatibility === "declared_compatible"
                    ? "Compatible (author)"
                    : "Unconfirmed"}
                </span>
                <button
                  className="button compact secondary"
                  disabled={blocked}
                  onClick={() =>
                    void queue(
                      mod.enabled === false ? "mod-enable" : "mod-disable",
                      { modid: mod.modid },
                      { name: mod.modid },
                    ).catch((e) => setError(String(e)))
                  }
                >
                  {mod.enabled === false ? "Enable" : "Disable"}
                </button>
                <button
                  className="button compact secondary"
                  disabled={blocked}
                  onClick={() =>
                    void queue(
                      "mod-remove",
                      { modid: mod.modid },
                      { name: mod.modid },
                    ).catch((e) => setError(String(e)))
                  }
                >
                  Remove
                </button>
              </div>
            ))}
          </div>
        )}
        <h3>Install a mod ZIP</h3>
        <p className="muted">
          The game stops for a recovery copy before installation.
        </p>
        <label className="checkbox">
          <input
            type="checkbox"
            checked={ack}
            disabled={blocked}
            onChange={(e) => setAck(e.target.checked)}
          />
          I reviewed game and mod compatibility. Unknown compatibility is not a
          guarantee.
        </label>
        <label
          className={`button secondary upload-button ${!ack || blocked ? "disabled" : ""}`}
        >
          <Upload size={16} />
          Choose mod ZIP
          <input
            ref={upload}
            type="file"
            accept=".zip"
            disabled={!ack || blocked}
            onChange={async (e) => {
              const file = e.target.files?.[0];
              if (!file) return;
              const context = scope.current;
              try {
                if (file.size > 64 * 1024 * 1024)
                  throw new Error("Mod ZIP limit is 64 MiB.");
                const bytes = new Uint8Array(await file.arrayBuffer());
                if (context.signal.aborted) return;
                let encoded = "";
                for (let offset = 0; offset < bytes.length; offset += 32768)
                  encoded += String.fromCharCode(
                    ...bytes.subarray(offset, offset + 32768),
                  );
                await queue(
                  "mod-upload",
                  {
                    filename: file.name,
                    content_base64: btoa(encoded),
                    acknowledge_unknown_compatibility: ack,
                  },
                  { name: file.name },
                  true,
                );
                if (!context.signal.aborted) {
                  setAck(false);
                  if (upload.current) upload.current.value = "";
                }
              } catch (e) {
                if (!context.signal.aborted) setError(String(e));
              }
            }}
          />
        </label>
      </section>
      <ModBrowser
        api={api}
        server={server}
        disabled={blocked}
        install={(body, label) => queue("mod-install", body, label)}
      />
    </>
  );
}
function ModIcon({ source }: { source?: string | null }) {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [source]);
  return !failed && source?.startsWith("data:image/png;base64,") ? (
    <img
      src={source}
      width={32}
      height={32}
      alt=""
      onError={() => setFailed(true)}
    />
  ) : (
    <Package size={23} />
  );
}
