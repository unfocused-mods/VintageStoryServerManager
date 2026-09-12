import { describe, it, expect, vi, afterEach } from "vitest";
import { Api, decode64, encode64, stateLabel } from "./api";
afterEach(() => vi.unstubAllGlobals());
describe("controller client", () => {
  it("preserves Unicode file content", () => {
    const text = "A world for everyone 🌲 — 雪";
    expect(decode64(encode64(text))).toBe(text);
  });
  it("sends authenticated mutations with unique idempotency keys", async () => {
    const fetch = vi
      .fn()
      .mockResolvedValue({ ok: true, json: async () => ({ id: "operation" }) });
    vi.stubGlobal("fetch", fetch);
    const api = new Api("http://localhost:4317/", "secret");
    await api.action("one", "start");
    await api.action("one", "start");
    const first = fetch.mock.calls[0][1];
    expect(first.headers.Authorization).toBe("Bearer secret");
    expect(first.headers["Idempotency-Key"]).not.toBe(
      fetch.mock.calls[1][1].headers["Idempotency-Key"],
    );
    expect(fetch.mock.calls[0][0]).toBe(
      "http://localhost:4317/api/v1/servers/one/actions/start",
    );
  });
  it("surfaces API errors rather than reporting success", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 401,
        json: async () => ({ error: "Authentication required" }),
      }),
    );
    await expect(new Api("", "bad").request("/summary")).rejects.toThrow(
      "Authentication required",
    );
  });
  it("explains network failures without exposing a raw browser TypeError", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockRejectedValue(new TypeError("Failed to fetch")),
    );
    await expect(
      new Api("http://localhost:4317", "secret").request(
        "/accounts/one",
        "DELETE",
      ),
    ).rejects.toThrow(
      "The Manager could not be reached. Check the Manager connection and try again.",
    );
  });
  it("renders explicit unknown states", () => {
    expect(stateLabel("awaiting_bootstrap")).toBe("Setup required");
  });
});
