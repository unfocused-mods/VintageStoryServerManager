import { useId } from "react";
type Field = {
  key: string;
  label: string;
  type: "text" | "password" | "number" | "boolean" | "select";
  min?: number;
  integer?: boolean;
  nullable?: boolean;
  choices?: [string, string][];
  hint?: string;
};
const text = (key: string, label: string, nullable = false): Field => ({
  key,
  label,
  type: "text",
  nullable,
});
const number = (
  key: string,
  label: string,
  min = 0,
  integer = true,
  nullable = false,
): Field => ({ key, label, type: "number", min, integer, nullable });
const flag = (key: string, label: string): Field => ({
  key,
  label,
  type: "boolean",
});
export const configGroups: { title: string; fields: Field[] }[] = [
  {
    title: "Server identity",
    fields: [
      text("ServerName", "Server name"),
      text("ServerDescription", "Description", true),
      text("ServerUrl", "Website", true),
      text("WelcomeMessage", "Welcome message"),
      text("ServerLanguage", "Language code"),
      flag("AdvertiseServer", "Public server listing"),
    ],
  },
  {
    title: "Players and access",
    fields: [
      number("MaxClients", "Player limit", 1),
      number("MaxClientsInQueue", "Queue limit"),
      {
        ...text("Password", "Server password (optional)", true),
        type: "password",
      },
      {
        key: "WhitelistMode",
        label: "Whitelist",
        type: "select",
        choices: [
          ["0", "Default (on for dedicated servers)"],
          ["1", "Off"],
          ["2", "On"],
        ],
      },
      flag("VerifyPlayerAuth", "Verify game accounts"),
      { key: "DefaultRoleCode", label: "Default player role", type: "select" },
      {
        ...number("WarnClientsAfterAfkSeconds", "AFK warning (seconds)"),
        hint: "0 disables warnings.",
      },
      {
        ...number("KickClientsAfterAfkSeconds", "AFK kick (seconds)"),
        hint: "0 disables kicking.",
      },
      number("ChatRateLimitMs", "Minimum chat interval (ms)"),
      number("MaxOwnedGroupChannelsPerUser", "Group channels per player"),
      flag("LoginFloodProtection", "Limit repeated login attempts"),
    ],
  },
  {
    title: "Game rules",
    fields: [
      flag("AllowPvP", "Player combat"),
      flag("AllowFireSpread", "Fire spread"),
      flag("AllowFallingBlocks", "Falling blocks"),
      flag("PassTimeWhenEmpty", "Time passes without players"),
    ],
  },
  {
    title: "Connection and performance",
    fields: [
      flag("CompressPackets", "Compress network traffic"),
      number("ClientConnectionTimeout", "Connection timeout (seconds)", 1),
      {
        ...number("MaxChunkRadius", "Maximum view distance (chunks)", 1),
        hint: "Larger distances require more memory.",
      },
      number("TickTime", "Server tick interval (ms)", 0, false),
      number(
        "SpawnCapPlayerScaling",
        "Creature cap scaling per player",
        0,
        false,
      ),
      number("BlockTickChunkRange", "Block simulation range (chunks)"),
      number("MaxMainThreadBlockTicks", "Maximum block updates per tick"),
      number("RandomBlockTicksPerChunk", "Random block updates per chunk"),
      number("BlockTickInterval", "Random block interval (ms)", 1),
    ],
  },
  {
    title: "Logging and protection",
    fields: [
      flag("LogBlockBreakPlace", "Log block placement and removal"),
      {
        ...number("LogFileSplitAfterLine", "Lines per log file", 1, true, true),
        hint: "Blank disables splitting.",
      },
      flag("CorruptionProtection", "World corruption detection"),
      number("DieBelowDiskSpaceMb", "Stop below free disk space (MB)"),
      number("DieAboveMemoryUsageMb", "Stop above memory usage (MB)", 1),
      {
        ...number(
          "DieAboveErrorCount",
          "Stop after error count",
          1,
          true,
          true,
        ),
        hint: "Blank disables this limit.",
      },
    ],
  },
];
export function parseServerConfig(
  value: string,
): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(value);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? parsed
      : null;
  } catch {
    return null;
  }
}
export function validateServerConfig(value: string): string[] {
  const config = parseServerConfig(value);
  if (!config) return ["Configuration must be a valid JSON object."];
  const errors: string[] = [];
  for (const group of configGroups)
    for (const field of group.fields) {
      const value = config[field.key];
      if (value === undefined || (value === null && field.nullable)) continue;
      if (field.type === "number") {
        if (
          typeof value !== "number" ||
          !Number.isFinite(value) ||
          value < (field.min || 0) ||
          (field.key === "TickTime" && value === 0) ||
          (field.integer && (!Number.isInteger(value) || value > 2147483647))
        )
          errors.push(
            `${field.label}: enter ${field.integer ? "a whole number" : "a number"} ${field.key === "TickTime" ? "greater than 0" : `of at least ${field.min || 0}`}.`,
          );
      } else if (field.type === "boolean") {
        if (typeof value !== "boolean")
          errors.push(`${field.label}: choose On or Off.`);
      } else if (field.key === "WhitelistMode") {
        if (![0, 1, 2].includes(value as number))
          errors.push("Whitelist: choose Default, Off or On.");
      } else if (value !== null && typeof value !== "string")
        errors.push(`${field.label}: enter text.`);
    }
  if (typeof config.ServerUrl === "string" && config.ServerUrl) {
    try {
      if (!["http:", "https:"].includes(new URL(config.ServerUrl).protocol))
        throw new Error();
    } catch {
      errors.push("Website: enter a full http or https address.");
    }
  }
  return errors;
}
export default function ServerConfigForm({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  const prefix = useId();
  const config = parseServerConfig(value);
  const errors = validateServerConfig(value);
  if (!config)
    return (
      <p role="alert" className="inline-error">
        Correct the JSON below to use the settings form.
      </p>
    );
  const set = (field: Field, value: string) => {
    const next = { ...config };
    if (value === "" && field.nullable) next[field.key] = null;
    else if (value === "") next[field.key] = "";
    else
      next[field.key] =
        field.type === "number" || field.key === "WhitelistMode"
          ? Number(value)
          : field.type === "boolean"
            ? value === "true"
            : value;
    onChange(JSON.stringify(next, null, 2));
  };
  return (
    <div className="form server-config-form">
      {errors.length > 0 && (
        <div role="alert" className="inline-error">
          {errors.map((error) => (
            <p key={error}>{error}</p>
          ))}
        </div>
      )}
      {configGroups.map((group, index) => (
        <details key={group.title} open={index === 0}>
          <summary>{group.title}</summary>
          <div className="form-grid">
            {group.fields.map((field) => {
              const current = config[field.key];
              const choices =
                field.type === "boolean"
                  ? [
                      ["true", "On"],
                      ["false", "Off"],
                    ]
                  : field.key === "DefaultRoleCode"
                    ? Array.isArray(config.Roles)
                      ? config.Roles.flatMap((role: any) =>
                          typeof role?.Code === "string"
                            ? [
                                [
                                  role.Code,
                                  typeof role.Name === "string"
                                    ? role.Name
                                    : role.Code,
                                ],
                              ]
                            : [],
                        )
                      : []
                    : field.choices || [];
              const input =
                current === undefined || current === null
                  ? ""
                  : String(current);
              return (
                <label className="field" key={field.key}>
                  <span>{field.label}</span>
                  {["boolean", "select"].includes(field.type) ? (
                    <select
                      aria-label={field.label}
                      aria-describedby={
                        field.hint ? `${prefix}-${field.key}` : undefined
                      }
                      value={input}
                      onChange={(e) => set(field, e.target.value)}
                    >
                      <option value="" disabled>
                        Not set
                      </option>
                      {input && !choices.some(([v]) => v === input) && (
                        <option value={input}>{input} (current)</option>
                      )}
                      {choices.map(([v, label]) => (
                        <option key={v} value={v}>
                          {label}
                        </option>
                      ))}
                    </select>
                  ) : (
                    <input
                      aria-label={field.label}
                      aria-describedby={
                        field.hint ? `${prefix}-${field.key}` : undefined
                      }
                      type={field.type}
                      value={input}
                      min={field.min}
                      max={field.integer ? 2147483647 : undefined}
                      step={field.integer ? 1 : "any"}
                      placeholder={field.nullable ? "None" : "Not set"}
                      autoComplete={
                        field.type === "password" ? "new-password" : undefined
                      }
                      onChange={(e) => set(field, e.target.value)}
                    />
                  )}
                  {field.hint && (
                    <small id={`${prefix}-${field.key}`} className="muted">
                      {field.hint}
                    </small>
                  )}
                </label>
              );
            })}
          </div>
        </details>
      ))}
      <p className="muted small">
        Connection address and port, storage paths and world creation settings
        are managed through deployment and world tools. Other settings remain
        available in advanced JSON. Omitted properties keep their saved values.
      </p>
    </div>
  );
}
