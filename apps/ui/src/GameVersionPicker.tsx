import { useEffect, useState } from "react";
import { Api } from "./api";
export interface GameRelease {
  version: string;
  latest: boolean;
  automatic_idle: boolean;
  runtime: string;
  tested: boolean;
}
interface Releases {
  versions: GameRelease[];
  latest: string | null;
  status: "current" | "stale" | "unavailable";
  error?: string;
}
export default function GameVersionPicker({
  api,
  value,
  onChange,
  onReady,
  installedVersion,
  disabled = false,
}: {
  api: Api;
  value: string;
  onChange: (version: string) => void;
  onReady?: (ready: boolean) => void;
  installedVersion?: string;
  disabled?: boolean;
}) {
  const [catalog, setCatalog] = useState<Releases | null>(null),
    [loading, setLoading] = useState(true),
    [error, setError] = useState(""),
    [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    setCatalog(null);
    api
      .request<Releases>("/game/releases")
      .then((result) => {
        if (!active) return;
        if (
          !Array.isArray(result.versions) ||
          !result.versions.length ||
          result.status === "unavailable"
        )
          throw new Error(
            result.error || "No stable downloads are available right now.",
          );
        setCatalog(result);
      })
      .catch((e) => {
        if (active) setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [api, attempt]);
  const selected = catalog?.versions.find(
    (release) => release.version === value,
  );
  const ready = !!selected && !loading;
  useEffect(() => {
    onReady?.(ready);
  }, [ready, onReady]);
  useEffect(() => {
    if (
      !value &&
      !installedVersion &&
      catalog?.latest &&
      catalog.versions.some((release) => release.version === catalog.latest)
    )
      onChange(catalog.latest);
  }, [catalog, value, installedVersion, onChange]);
  return (
    <div className="form">
      <label className="field">
        <span>Vintage Story version</span>
        <select
          required
          value={value}
          disabled={disabled || loading || !catalog}
          onChange={(e) => onChange(e.target.value)}
        >
          {!value && (
            <option value="">
              {loading ? "Loading releases…" : "Select a release"}
            </option>
          )}
          {value && !selected && (
            <option value={value} disabled>
              {value}
              {value === installedVersion ? " — Installed" : ""}
              {catalog ? " — unavailable" : ""}
            </option>
          )}
          {catalog?.versions.map((release) => (
            <option key={release.version} value={release.version}>
              {release.version}
              {release.version === installedVersion ? " — Installed" : ""}
              {release.latest
                ? catalog.status === "stale"
                  ? " — Latest known"
                  : " — Latest stable"
                : ""}
            </option>
          ))}
        </select>
      </label>
      {loading && (
        <p role="status" className="muted small">
          Loading stable releases…
        </p>
      )}
      {error && (
        <p role="alert" className="inline-error">
          {error}
        </p>
      )}
      {catalog?.status === "stale" && (
        <p role="status" className="muted small">
          Release list may be out of date.
        </p>
      )}
      {(error || catalog?.status === "stale") && (
        <button
          type="button"
          className="text-button"
          disabled={disabled || loading}
          onClick={() => setAttempt((n) => n + 1)}
        >
          Retry release list
        </button>
      )}
      {selected && !selected.automatic_idle && (
        <p className="muted small">
          Automatic sleep is not yet tested for this release.
        </p>
      )}
    </div>
  );
}
