// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  render,
  screen,
  fireEvent,
  cleanup,
  waitFor,
} from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import App from "./App";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const summary = {
  servers: [],
  operations: [],
  user: { name: "Owner", role: "owner" },
  version: "0.1.0",
  providers: ["azure"],
};
function mockApi() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => ({
      ok: true,
      json: async () => (url.endsWith("/accounts") ? [] : summary),
    })),
  );
}
it("shows a running VM separately from uninstalled Remote and never polls its agent during setup", async () => {
  (window as any).__TAURI_INTERNALS__ = {};
  invoke.mockResolvedValue({ url: "http://127.0.0.1:1234", token: "private" });
  const server = {
    id: "cloud",
    name: "Cloud",
    provider: "azure",
    state: "awaiting_bootstrap",
    region: "newzealandnorth",
    size: "Standard_D2s_v5",
    host: "192.0.2.1",
    game_version: "1.22.7",
  };
  const fetch = vi.fn(async (url: string) => ({
    ok: true,
    json: async () =>
      url.endsWith("/summary")
        ? { ...summary, servers: [server] }
        : url.endsWith("/infrastructure/status")
          ? {
              power_state: "running",
              provisioning_state: "succeeded",
              checked_at: "2026-09-06T00:00:00Z",
            }
          : url.endsWith("/cloud-idle-policy")
            ? { enabled: false, minutes: 15, external_manager_confirmed: false }
            : url.endsWith("/schedule")
              ? { enabled: false, hour_utc: 4 }
              : {},
  }));
  vi.stubGlobal("fetch", fetch);
  render(<App />);
  await screen.findByRole("button", { name: "Infrastructure" });
  fireEvent.click(screen.getByRole("button", { name: "Infrastructure" }));
  await screen.findByText("succeeded");
  expect(screen.getByText("VM power").nextElementSibling).toHaveTextContent(
    "running",
  );
  expect(
    screen.getByText("Remote installation").nextElementSibling,
  ).toHaveTextContent("Not installed");
  expect(screen.getByRole("button", { name: "Start VM" })).toBeDisabled();
  expect(
    screen.getByRole("button", { name: "Check Remote identity" }),
  ).toBeEnabled();
  expect(fetch.mock.calls.some(([url]) => url.includes("/agent/status"))).toBe(
    false,
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  invoke.mockReset();
  delete (window as any).__TAURI_INTERNALS__;
});
describe("server setup", () => {
  it("shows browser sign-in without promotional metrics or controller jargon", () => {
    render(<App />);
    expect(
      screen.getByRole("heading", { name: "Sign in to Server Manager" }),
    ).toBeVisible();
    expect(screen.queryByText("CLOUD PROVIDERS")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Create server" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Manager connection" }),
    ).not.toHaveTextContent("LO");
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(screen.getByLabelText("Username")).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Disconnect" }),
    ).not.toBeInTheDocument();
  });
  it("keeps failed login open with its error", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 401,
        json: async () => ({ error: "Invalid credentials" }),
      }),
    );
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    fireEvent.change(screen.getByLabelText("Username"), {
      target: { value: "owner" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: "wrong" },
    });
    fireEvent.submit(screen.getByLabelText("Username").closest("form")!);
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Invalid credentials",
      ),
    );
    expect(screen.getByRole("dialog")).toBeVisible();
  });
  it("starts desktop management automatically and offers local and remote setup without requiring cloud credentials", async () => {
    (window as any).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      url: "http://127.0.0.1:1234",
      token: "private",
    });
    mockApi();
    render(<App />);
    expect(
      screen.getByRole("heading", { name: "Starting Server Manager…" }),
    ).toBeVisible();
    await screen.findByRole("heading", { name: "Create your first server" });
    expect(invoke).toHaveBeenCalledWith("connection");
    fireEvent.click(screen.getByRole("button", { name: "Set up server" }));
    expect(screen.getByRole("button", { name: "Local Docker" })).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Install a Remote" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Cloud deployment" }));
    await screen.findByRole("heading", { name: "Add a cloud account" });
    expect(
      screen.getByRole("button", { name: "Sign in to Azure" }),
    ).toBeVisible();
    expect(screen.queryByLabelText("Access token")).not.toBeInTheDocument();
  });
  it("retries a failed desktop startup without requesting credentials", async () => {
    (window as any).__TAURI_INTERNALS__ = {};
    invoke
      .mockRejectedValueOnce(new Error("Storage unavailable"))
      .mockResolvedValue({ url: "http://127.0.0.1:1234", token: "private" });
    mockApi();
    render(<App />);
    await screen.findByRole("button", { name: "Try again" });
    expect(screen.getByRole("alert")).toHaveTextContent("Storage unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await screen.findByRole("heading", { name: "Create your first server" });
    expect(invoke).toHaveBeenCalledTimes(2);
  });
  it.each(["awaiting_bootstrap", "provisioning"])(
    "shows the next setup action for %s instead of game controls",
    async (state) => {
      (window as any).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue({
        url: "http://127.0.0.1:1234",
        token: "private",
      });
      vi.stubGlobal(
        "fetch",
        vi.fn(async (url: string) => ({
          ok: true,
          json: async () =>
            url.endsWith("/cloud-idle-policy")
              ? {
                  enabled: false,
                  minutes: 15,
                  external_manager_confirmed: false,
                  wake_pending: false,
                }
              : {
                  ...summary,
                  servers: [
                    {
                      id: "server-1",
                      name: "Test server",
                      state,
                      provider: "azure",
                      host: "192.0.2.1",
                      region: "eastus",
                      size: "Standard_D4s_v5",
                      game_version: "1.22.0",
                    },
                  ],
                },
        })),
      );
      render(<App />);
      if (state === "awaiting_bootstrap") {
        const action = await screen.findByRole("button", {
          name: "Finish server setup",
        });
        expect(
          screen.queryByRole("button", { name: "Start server" }),
        ).not.toBeInTheDocument();
        fireEvent.click(action);
        await screen.findByRole("heading", { name: "Install Remote" });
      } else {
        expect(
          await screen.findByRole("button", { name: "Preparing server…" }),
        ).toBeDisabled();
        expect(
          screen.queryByRole("button", { name: "Start server" }),
        ).not.toBeInTheDocument();
        expect(
          screen.queryByRole("button", { name: "Copy join address" }),
        ).not.toBeInTheDocument();
      }
    },
  );
  it("preserves keyboard access to sign-in and returns focus after Escape", () => {
    render(<App />);
    const open = screen.getByRole("button", { name: "Sign in" });
    open.focus();
    fireEvent.click(open);
    expect(screen.getByRole("button", { name: "Close dialog" })).toHaveFocus();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(open).toHaveFocus();
  });
});

it("resets the update selection to the newly selected server's installed version", async () => {
  (window as any).__TAURI_INTERNALS__ = {};
  invoke.mockResolvedValue({ url: "http://127.0.0.1:1234", token: "private" });
  const servers = [
    {
      id: "first",
      name: "First",
      provider: "docker",
      state: "stopped",
      region: "local",
      size: "existing",
      host: "127.0.0.1",
      game_version: "1.22.0",
    },
    {
      id: "second",
      name: "Second",
      provider: "docker",
      state: "stopped",
      region: "local",
      size: "existing",
      host: "127.0.0.1",
      game_version: "1.21.6",
    },
  ];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => ({
      ok: true,
      json: async () => {
        if (url.endsWith("/summary")) return { ...summary, servers };
        if (url.endsWith("/game/releases"))
          return {
            versions: ["1.22.7", "1.22.0", "1.21.6"].map((version) => ({
              version,
              latest: version === "1.22.7",
              automatic_idle: false,
            })),
            latest: "1.22.7",
            status: "current",
          };
        if (url.endsWith("/worlds")) return [];
        if (url.endsWith("/schedule")) return { enabled: false, hour_utc: 4 };
        if (url.endsWith("/idle-policy"))
          return { policy: { enabled: false, minutes: 15 }, sleeping: false };
        if (url.endsWith("/config")) return { value: {}, etag: "fixture" };
        return {};
      },
    })),
  );
  render(<App />);
  await screen.findByRole("button", { name: "First Docker" });
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  await screen.findByRole("option", { name: "1.22.7 — Latest stable" });
  fireEvent.change(screen.getByLabelText("Vintage Story version"), {
    target: { value: "1.22.7" },
  });
  expect(
    screen.getByRole("button", { name: "Install selected release" }),
  ).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: "First Docker" }));
  fireEvent.click(screen.getByRole("button", { name: /^Second Docker/ }));
  await waitFor(() =>
    expect(screen.getByLabelText("Vintage Story version")).toHaveValue(
      "1.21.6",
    ),
  );
  expect(
    screen.getByRole("button", { name: "Install selected release" }),
  ).toBeDisabled();
});

it("uses editable player and mod requirements for cloud size suggestions while keeping the chosen size", async () => {
  (window as any).__TAURI_INTERNALS__ = {};
  invoke.mockResolvedValue({ url: "http://127.0.0.1:1234", token: "private" });
  const sizes = [
    { slug: "small", vcpus: 2, memory: 4096, price_monthly: 24 },
    { slug: "medium", vcpus: 4, memory: 8192, price_monthly: 48 },
    { slug: "large", vcpus: 4, memory: 16384, price_monthly: 80 },
  ].map((size) => ({ ...size, available: true, regions: ["syd1"] }));
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => ({
      ok: true,
      json: async () => {
        if (url.endsWith("/accounts"))
          return [
            {
              id: "do",
              name: "My cloud",
              provider: "digital_ocean",
              credential_ref: "keyring:fixture",
              settings: {},
            },
          ];
        if (url.endsWith("/catalog"))
          return {
            regions: [{ slug: "syd1", name: "Sydney", available: true }],
            sizes,
          };
        if (url.endsWith("/game/releases"))
          return {
            versions: [
              { version: "1.22.7", latest: true, automatic_idle: true },
            ],
            latest: "1.22.7",
            status: "current",
          };
        return summary;
      },
    })),
  );
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Set up server" }));
  fireEvent.click(screen.getByRole("button", { name: "Cloud deployment" }));
  await screen.findByRole("option", { name: "Sydney" });
  fireEvent.change(screen.getByLabelText("Region"), {
    target: { value: "syd1" },
  });
  await waitFor(() =>
    expect(screen.getByLabelText("Instance size")).toHaveValue("medium"),
  );
  expect(
    screen.getByRole("option", { name: /Economy — small/ }),
  ).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Expected players"), {
    target: { value: 20 },
  });
  fireEvent.change(screen.getByLabelText("Mods"), {
    target: { value: "heavy" },
  });
  expect(
    screen.getByRole("option", { name: /Recommended — large/ }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("option", { name: /Economy — small/ }),
  ).not.toBeInTheDocument();
  expect(screen.getByLabelText("Instance size")).toHaveValue("medium");
  expect(screen.getByText(/less than the estimated 9.5 GiB/)).toBeVisible();
});

it("keeps deployment drafts on blank form and backdrop clicks while explicit back and close still work", async () => {
  (window as any).__TAURI_INTERNALS__ = {};
  invoke.mockResolvedValue({ url: "http://127.0.0.1:1234", token: "private" });
  mockApi();
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Set up server" }));
  fireEvent.click(screen.getByRole("button", { name: "Local Docker" }));
  const input = screen.getByLabelText("Game server name");
  fireEvent.change(input, { target: { value: "keep-my-draft" } });
  const back = screen.getByRole("button", { name: "Change deployment target" });
  expect(back).toHaveAttribute("type", "button");
  expect(back).toHaveClass("deployment-back");
  fireEvent.click(back.parentElement!);
  fireEvent.mouseDown(input.closest("form")!);
  fireEvent.click(input.closest("form")!);
  fireEvent.click(screen.getByRole("dialog").parentElement!);
  expect(screen.getByLabelText("Game server name")).toHaveValue(
    "keep-my-draft",
  );
  expect(
    screen.queryByRole("heading", { name: "Where will the game run?" }),
  ).not.toBeInTheDocument();
  fireEvent.click(back);
  expect(
    screen.getByRole("heading", { name: "Where will the game run?" }),
  ).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Close dialog" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
