import { useEffect, useRef, useState } from "react";
import { Search, Package, ArrowDownToLine, Loader2 } from "lucide-react";
import { Api, type Server } from "./api";
function parseVersion(value: unknown) {
  const match =
    /^v?(\d+(?:\.\d+)*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z.-]+)?$/.exec(
      String(value || "").trim(),
    );
  return match
    ? {
        parts: match[1].split(".").map(BigInt),
        prerelease: match[2]?.split("."),
      }
    : null;
}
function compareVersions(
  a: NonNullable<ReturnType<typeof parseVersion>>,
  b: NonNullable<ReturnType<typeof parseVersion>>,
) {
  for (let i = 0; i < Math.max(a.parts.length, b.parts.length); i++) {
    const x = a.parts[i] || 0n,
      y = b.parts[i] || 0n;
    if (x !== y) return x > y ? 1 : -1;
  }
  if (!a.prerelease || !b.prerelease)
    return !a.prerelease && !b.prerelease ? 0 : !a.prerelease ? 1 : -1;
  for (let i = 0; i < Math.max(a.prerelease.length, b.prerelease.length); i++) {
    const x = a.prerelease[i],
      y = b.prerelease[i];
    if (x === y) continue;
    if (x === undefined || y === undefined) return x === undefined ? -1 : 1;
    const xn = /^\d+$/.test(x),
      yn = /^\d+$/.test(y);
    if (xn && yn) {
      if (BigInt(x) === BigInt(y)) continue;
      return BigInt(x) > BigInt(y) ? 1 : -1;
    }
    if (xn !== yn) return xn ? -1 : 1;
    return x > y ? 1 : -1;
  }
  return 0;
}
export function versionSortedReleases(releases: any[]): any[] {
  const time = (r: any) =>
    Date.parse(String(r.created || "").replace(" ", "T")) || 0;
  return [...releases]
    .filter(
      (r) =>
        Number.isSafeInteger(Number(r.releaseid)) && Number(r.releaseid) > 0,
    )
    .sort((a, b) => {
      const av = parseVersion(a.modversion),
        bv = parseVersion(b.modversion);
      const version = av && bv ? compareVersions(bv, av) : av ? -1 : bv ? 1 : 0;
      return (
        version ||
        time(b) - time(a) ||
        Number(b.releaseid) - Number(a.releaseid)
      );
    });
}
export function compatibleRelease(releases: any[], gameVersion: string) {
  return versionSortedReleases(releases).find(
    (r) =>
      parseVersion(r.modversion) &&
      Array.isArray(r.tags) &&
      r.tags.includes(gameVersion),
  );
}
export default function ModBrowser({
  api,
  server,
  install,
  disabled = false,
}: {
  api: Api;
  server: Server;
  install: (
    body: unknown,
    label: { name: string; version: string },
  ) => Promise<void>;
  disabled?: boolean;
}) {
  const [q, setQ] = useState(""),
    [mods, setMods] = useState<any[]>([]),
    [detail, setDetail] = useState<any>(null),
    [release, setRelease] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [ack, setAck] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [installing, setInstalling] = useState(false);
  const searchRequest = useRef<AbortController | null>(null);
  const detailRequest = useRef<AbortController | null>(null);
  const selection = useRef({ mod: "", release: "" });
  useEffect(() => {
    searchRequest.current?.abort();
    detailRequest.current?.abort();
    setMods([]);
    setDetail(null);
    setRelease("");
    setAck(false);
    setError("");
    setBusy(false);
    setLoadingDetail(false);
    setInstalling(false);
    selection.current = { mod: "", release: "" };
    return () => {
      searchRequest.current?.abort();
      detailRequest.current?.abort();
    };
  }, [api, server.id, server.game_version]);
  async function selectMod(mod: any) {
    detailRequest.current?.abort();
    const controller = new AbortController();
    detailRequest.current = controller;
    const id = String(mod.urlalias || mod.modid);
    if (selection.current.mod !== id)
      selection.current = { mod: id, release: "" };
    setDetail(null);
    setAck(false);
    setError("");
    setLoadingDetail(true);
    try {
      const value: any = await api.request(
        `/moddb/${encodeURIComponent(id)}`,
        "GET",
        undefined,
        controller.signal,
      );
      if (controller.signal.aborted) return;
      const releases = versionSortedReleases(
        Array.isArray(value.releases) ? value.releases : [],
      );
      const chosen = releases.some(
        (r) => String(r.releaseid) === selection.current.release,
      )
        ? selection.current.release
        : String(
            compatibleRelease(releases, server.game_version)?.releaseid || "",
          );
      selection.current = { mod: id, release: chosen };
      setDetail({ ...value, releases });
      setRelease(chosen);
    } catch (e) {
      if (!controller.signal.aborted) setError(String(e));
    } finally {
      if (!controller.signal.aborted) setLoadingDetail(false);
    }
  }
  const selected = detail?.releases?.find(
    (r: any) => String(r.releaseid) === release,
  );
  const compatible =
    Array.isArray(selected?.tags) &&
    selected.tags.includes(server.game_version);
  const recommended = compatibleRelease(
    detail?.releases || [],
    server.game_version,
  );
  return (
    <section className="panel detail-panel">
      <h3>Browse mods</h3>
      <form
        className="button-row"
        onSubmit={async (e) => {
          e.preventDefault();
          searchRequest.current?.abort();
          const controller = new AbortController();
          searchRequest.current = controller;
          setBusy(true);
          try {
            const data: any = await api.request(
              `/moddb?q=${encodeURIComponent(q)}`,
              "GET",
              undefined,
              controller.signal,
            );
            if (controller.signal.aborted) return;
            setMods(Array.isArray(data.mods) ? data.mods : []);
            setError("");
          } catch (e) {
            if (!controller.signal.aborted) setError(String(e));
          } finally {
            if (!controller.signal.aborted) setBusy(false);
          }
        }}
      >
        <input
          aria-label="Search ModDB"
          style={{ flex: 1 }}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="Search mods…"
        />
        <button className="button secondary" disabled={busy}>
          {busy ? <Loader2 className="spin" size={16} /> : <Search size={16} />}
          Search
        </button>
      </form>
      <div className="file-list">
        {mods.map((m) => (
          <button
            key={m.modid}
            disabled={installing}
            onClick={() => void selectMod(m)}
          >
            <Package size={18} />
            <span>
              {m.name}
              <small style={{ display: "block" }}>{m.summary}</small>
            </span>
            <small>{m.side}</small>
          </button>
        ))}
      </div>
      {loadingDetail && <p role="status">Loading releases…</p>}
      {detail && (
        <div className="review-card">
          <h3>{detail.name}</h3>
          <p>
            By {detail.author} · Game {server.game_version}
          </p>
          <label className="field">
            <span>Release</span>
            <select
              disabled={installing}
              value={release}
              onChange={(e) => {
                selection.current.release = e.target.value;
                setRelease(e.target.value);
                setAck(false);
              }}
            >
              <option value="">
                {detail.releases.length
                  ? "Select a release"
                  : "No releases available"}
              </option>
              {detail.releases?.map((r: any) => (
                <option key={r.releaseid} value={String(r.releaseid)}>
                  {r.modversion}
                  {r.releaseid === recommended?.releaseid
                    ? ` — Latest for ${server.game_version}`
                    : ""}
                  {r.created ? ` · ${String(r.created).slice(0, 10)}` : ""}
                </option>
              ))}
            </select>
          </label>
          {!recommended && !selected && (
            <p className="muted">
              No comparable release declares support for game{" "}
              {server.game_version}. Select a release to review its
              compatibility.
            </p>
          )}
          {selected && (
            <>
              <p className={compatible ? "muted" : "inline-error"}>
                {compatible
                  ? "Compatible with this game version, according to the author."
                  : "Compatibility with this game version is unconfirmed."}
              </p>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={ack}
                  disabled={installing || disabled}
                  onChange={(e) => setAck(e.target.checked)}
                />
                I trust this mod and have reviewed its requirements.
              </label>
              <button
                className="button primary"
                disabled={!ack || disabled || installing}
                onClick={async () => {
                  setInstalling(true);
                  setError("");
                  const scope = detailRequest.current;
                  try {
                    await install(
                      {
                        modid: detail.urlalias || String(detail.modid),
                        releaseid: Number(release),
                        acknowledge_unknown_compatibility: ack,
                      },
                      { name: detail.name, version: selected.modversion },
                    );
                    if (scope?.signal.aborted) return;
                    setDetail(null);
                    setRelease("");
                    setAck(false);
                    selection.current = { mod: "", release: "" };
                  } catch (e) {
                    if (!scope?.signal.aborted) setError(String(e));
                  } finally {
                    if (!scope?.signal.aborted) setInstalling(false);
                  }
                }}
              >
                <ArrowDownToLine size={16} />
                {installing ? "Queuing installation…" : "Install release"}
              </button>
            </>
          )}
        </div>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
