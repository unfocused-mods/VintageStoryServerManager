import { useEffect, useState } from "react";
import { Api, type Server } from "./api";
export type WorldField = {
  code: string;
  label: string;
  description?: string;
  category: string;
  categoryLabel: string;
  dataType: "dropdown" | "bool";
  values?: string[];
  names?: string[];
  default: string | boolean;
  onlyDuringWorldCreate?: boolean;
};
export type WorldPreset = {
  code: string;
  label: string;
  worldType: string;
  worldConfig: Record<string, string | boolean>;
};
export type WorldOptions = {
  version: string;
  presets: WorldPreset[];
  fields: WorldField[];
  source: string;
};
export function worldHelpText(value: string): string {
  return value.replace(/<br\s*\/?>(?:\r?\n)?/gi, " ").replace(/<[^>]*>/g, "");
}
export default function WorldCreator({
  api,
  server,
  act,
}: {
  api: Api;
  server: Server;
  act: (action: string, body?: unknown) => void;
}) {
  const [schema, setSchema] = useState<WorldOptions | null>(null),
    [error, setError] = useState(""),
    [name, setName] = useState(""),
    [seed, setSeed] = useState(""),
    [style, setStyle] = useState(""),
    [values, setValues] = useState<Record<string, string | boolean>>({}),
    [search, setSearch] = useState(""),
    [height, setHeight] = useState(256);
  const [reload, setReload] = useState(0);
  const load = () => setReload((value) => value + 1);
  useEffect(() => {
    const controller = new AbortController();
    let active = true;
    setSchema(null);
    setError("");
    setStyle("");
    setValues({});
    const timeout = setTimeout(() => {
      if (active) {
        controller.abort();
        setError(
          "World options took too long to load. Retry when the Remote is reachable.",
        );
      }
    }, 30000);
    void api
      .request<WorldOptions>(
        `/servers/${server.id}/agent/world-options`,
        "GET",
        undefined,
        controller.signal,
      )
      .then((value) => {
        if (!active || controller.signal.aborted) return;
        if (
          !Array.isArray(value?.presets) ||
          !value.presets.length ||
          !Array.isArray(value?.fields) ||
          !value.fields.length
        )
          throw new Error(
            "World options are unavailable from this Remote. Update Remote in Infrastructure, then retry.",
          );
        setSchema(value);
        setStyle(value.presets[0].code);
        setValues({});
      })
      .catch((e) => {
        if (active && !controller.signal.aborted) setError(String(e));
      })
      .finally(() => clearTimeout(timeout));
    return () => {
      active = false;
      controller.abort();
      clearTimeout(timeout);
    };
  }, [api, server.id, server.game_version, reload]);
  const preset = schema?.presets.find((p) => p.code === style);
  const current = (field: WorldField) =>
    values[field.code] ?? preset?.worldConfig[field.code] ?? field.default;
  const categories = [...new Set(schema?.fields.map((f) => f.category) || [])];
  return (
    <section className="panel detail-panel">
      <h3>Create a world</h3>
      <p className="muted small">
        Previous worlds are kept. Creating a world stops the game and saves a
        recovery copy.
      </p>
      {error && (
        <p role="alert">
          {error}{" "}
          <button className="text-button" onClick={load}>
            Retry
          </button>
        </p>
      )}
      {!schema && !error && (
        <p role="status">Loading options from the installed game…</p>
      )}
      {schema && (
        <>
          <div className="form-grid">
            <label className="field">
              <span>World name</span>
              <input
                value={name}
                maxLength={99}
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            <label className="field">
              <span>Seed (optional)</span>
              <input
                value={seed}
                maxLength={100}
                placeholder="Random"
                onChange={(e) => setSeed(e.target.value)}
              />
            </label>
            <label className="field">
              <span>Play style</span>
              <select
                value={style}
                onChange={(e) => {
                  setStyle(e.target.value);
                  setValues({});
                }}
              >
                {schema.presets.map((p) => (
                  <option key={p.code} value={p.code}>
                    {p.label}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              <span>World height (blocks)</span>
              <input
                type="number"
                min={128}
                max={2048}
                step={32}
                value={height}
                onChange={(e) => setHeight(Number(e.target.value))}
              />
              <small>
                128–2048 in steps of 32. Taller worlds use more memory.
              </small>
            </label>
          </div>
          <details>
            <summary>Customize world settings</summary>
            <label className="field">
              <span>Find a world setting</span>
              <input
                type="search"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </label>
            {categories.map((category) => {
              const fields = schema.fields.filter(
                (f) =>
                  f.category === category &&
                  `${f.label} ${f.description || ""}`
                    .toLowerCase()
                    .includes(search.toLowerCase()),
              );
              if (!fields.length) return null;
              return (
                <fieldset key={category}>
                  <legend>{fields[0].categoryLabel}</legend>
                  <div className="form-grid">
                    {fields.map((field) =>
                      field.dataType === "bool" ? (
                        <label className="checkbox" key={field.code}>
                          <input
                            type="checkbox"
                            aria-label={field.label}
                            aria-describedby={
                              field.description
                                ? `world-help-${field.code}`
                                : undefined
                            }
                            checked={
                              current(field) === true ||
                              current(field) === "true"
                            }
                            onChange={(e) =>
                              setValues({
                                ...values,
                                [field.code]: e.target.checked,
                              })
                            }
                          />
                          <span>
                            {field.label}
                            {field.description && (
                              <small id={`world-help-${field.code}`}>
                                {worldHelpText(field.description)}
                              </small>
                            )}
                          </span>
                        </label>
                      ) : (
                        <label className="field" key={field.code}>
                          <span>{field.label}</span>
                          <select
                            aria-describedby={
                              field.description
                                ? `world-help-${field.code}`
                                : undefined
                            }
                            value={String(current(field))}
                            onChange={(e) =>
                              setValues({
                                ...values,
                                [field.code]: e.target.value,
                              })
                            }
                          >
                            {field.values?.map((value, index) => (
                              <option key={value} value={value}>
                                {field.names?.[index] || value}
                              </option>
                            ))}
                          </select>
                          {field.description && (
                            <small id={`world-help-${field.code}`}>
                              {worldHelpText(field.description)}
                            </small>
                          )}
                        </label>
                      ),
                    )}
                  </div>
                </fieldset>
              );
            })}
          </details>
          <button
            className="button secondary"
            disabled={
              !name.trim() ||
              !preset ||
              !Number.isInteger(height) ||
              height < 128 ||
              height > 2048 ||
              height % 32 !== 0 ||
              [
                "provisioning",
                "awaiting_bootstrap",
                "deallocated",
                "deleted",
              ].includes(server.state)
            }
            onClick={() =>
              act("world-create", {
                name: name.trim(),
                seed: seed || null,
                play_style: style,
                height,
                world_configuration: values,
              })
            }
          >
            Create and select world
          </button>
        </>
      )}
    </section>
  );
}
