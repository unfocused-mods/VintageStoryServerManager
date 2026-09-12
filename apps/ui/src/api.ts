import type { components } from "./schema";
export type Provider = components["schemas"]["Provider"];
export type Account = Omit<components["schemas"]["Account"], "settings"> & {
  settings: Record<string, string>;
};
export type Server = components["schemas"]["Server"];
export type Operation = components["schemas"]["Operation"];
export interface Summary {
  servers: Server[];
  operations: Operation[];
  user: { name: string; role: string };
  version: string;
  providers: string[];
}
export const providerNames: Record<Provider, string> = {
  azure: "Microsoft Azure",
  hetzner: "Hetzner",
  digital_ocean: "DigitalOcean",
  vultr: "Vultr",
  ssh: "Remote over SSH",
  docker: "Docker",
  ssh_docker: "Docker over SSH",
};
export class Api {
  constructor(
    public url: string,
    private token: string,
  ) {}
  async request<T = unknown>(
    path: string,
    method = "GET",
    body?: unknown,
    signal?: AbortSignal,
  ): Promise<T> {
    let res: Response;
    try {
      res = await fetch(`${this.url.replace(/\/$/, "")}/api/v1${path}`, {
        method,
        signal,
        headers: {
          Authorization: `Bearer ${this.token}`,
          ...(body !== undefined
            ? {
                "Content-Type": "application/json",
                "Idempotency-Key": crypto.randomUUID(),
              }
            : {}),
        },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch {
      throw new Error(
        "The Manager could not be reached. Check the Manager connection and try again.",
      );
    }
    const value = await res
      .json()
      .catch(() => ({ error: "Server Manager returned an invalid response" }));
    if (!res.ok)
      throw new Error(value.error || `Request failed (${res.status})`);
    return value as T;
  }
  async action(id: string, action: string, body: unknown = {}) {
    return this.request<Operation>(
      `/servers/${id}/actions/${action}`,
      "POST",
      body,
    );
  }
}
export function stateLabel(state: string) {
  if (state === "awaiting_bootstrap") return "Setup required";
  if (state === "bootstrap") return "Install server software";
  return state.replaceAll("_", " ").replace(/^./, (c) => c.toUpperCase());
}
export function download(
  name: string,
  content: string | Uint8Array<ArrayBuffer>,
  type = "application/json",
) {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
export function decode64(base64: string) {
  const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
}
export function encode64(text: string) {
  const bytes = new TextEncoder().encode(text);
  let result = "";
  for (const b of bytes) result += String.fromCharCode(b);
  return btoa(result);
}
