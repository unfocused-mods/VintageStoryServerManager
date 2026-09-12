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
import DeployTarget from "./DeployTarget";
import HostSetup from "./HostSetup";
import { Api, type Server } from "./api";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  invoke.mockReset();
});
const server = {
  id: "one",
  host: "192.168.1.2",
  provider: "ssh",
  state: "awaiting_bootstrap",
  game_version: "1.22.0",
} as Server;
describe("deployment targets", () => {
  it("offers all targets without requiring a cloud account", () => {
    render(
      <DeployTarget
        api={new Api("", "")}
        localDesktop
        onDone={() => {}}
        cloud={() => null}
      />,
    );
    for (const name of [
      "Local Docker",
      "Install a Remote",
      "Docker over SSH",
      "Cloud deployment",
    ])
      expect(screen.getByRole("button", { name })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Local Docker" }));
    expect(screen.getByText(/Install Docker Desktop/)).toBeVisible();
    expect(
      screen.queryByLabelText("SSH private key file"),
    ).not.toBeInTheDocument();
  });
  it("labels browser Docker deployment as running on the Manager computer", () => {
    render(
      <DeployTarget
        api={new Api("", "")}
        localDesktop={false}
        onDone={() => {}}
        cloud={() => null}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Docker on this Manager" }),
    );
    expect(
      screen.getByText(/Uses Docker on the computer running this Manager/),
    ).toBeVisible();
  });
  it("does not install a Remote until a fingerprint is explicitly accepted", async () => {
    const act = vi.fn();
    invoke.mockResolvedValue({
      keys: "host key",
      fingerprint: "SHA256:example",
    });
    render(<HostSetup server={server} local act={act} />);
    const install = screen.getByRole("button", {
      name: "Install Remote and game",
    });
    expect(install).toBeDisabled();
    fireEvent.click(
      screen.getByRole("button", { name: "Check Remote identity" }),
    );
    await screen.findByText("SHA256:example");
    expect(install).toBeDisabled();
    expect(act).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Fingerprint matches — trust Remote",
      }),
    );
    await waitFor(() => expect(install).toBeEnabled());
    fireEvent.click(install);
    expect(act).toHaveBeenCalledWith("bootstrap");
    expect(invoke).toHaveBeenLastCalledWith("host_key", {
      host: server.host,
      accepted: "host key",
    });
  });
  it("does not report failed setup as installed", () => {
    render(
      <HostSetup
        server={{ ...server, state: "attention" }}
        local
        act={vi.fn()}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Setup needs attention",
    );
    expect(screen.queryByText(/Remote installed/)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Install Remote and game" }),
    ).toBeDisabled();
  });
  it("waits for a stopped cloud VM before checking its identity", () => {
    render(
      <HostSetup
        server={{ ...server, provider: "azure" }}
        local
        act={vi.fn()}
        powerState="deallocated"
      />,
    );
    expect(screen.getByText(/Start the VM below/)).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Check Remote identity" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Install Remote and game" }),
    ).toBeDisabled();
    expect(invoke).not.toHaveBeenCalled();
  });
  it("keeps Remote installation available when VM power previously overwrote the setup state", () => {
    render(
      <HostSetup
        server={{ ...server, provider: "azure", state: "stopped" }}
        local
        act={vi.fn()}
        powerState="running"
        remoteInstalled={false}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Check Remote identity" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Install Remote and game" }),
    ).toBeDisabled();
    expect(screen.queryByText(/Remote installed/)).not.toBeInTheDocument();
  });
  it("keeps installation blocked when identity verification fails", async () => {
    invoke.mockRejectedValue(new Error("The host key changed"));
    render(<HostSetup server={server} local act={vi.fn()} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Check Remote identity" }),
    );
    await screen.findByRole("alert");
    expect(
      screen.getByRole("button", { name: "Install Remote and game" }),
    ).toBeDisabled();
  });
  it("shows connection progress and a retry without trusting an unreachable host", async () => {
    let reject: (reason: Error) => void = () => {};
    invoke.mockImplementationOnce(
      () =>
        new Promise((_resolve, fail) => {
          reject = fail;
        }),
    );
    render(<HostSetup server={server} local act={vi.fn()} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Check Remote identity" }),
    );
    expect(
      screen.getByRole("button", { name: "Checking connection…" }),
    ).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Connecting to 192.168.1.2",
    );
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    reject(
      new Error("SSH connection timed out. Check the allowed access address."),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "SSH connection timed out",
    );
    expect(
      screen.getByRole("button", { name: "Install Remote and game" }),
    ).toBeDisabled();
    invoke.mockResolvedValueOnce({
      keys: "host key",
      fingerprint: "SHA256:retry",
    });
    fireEvent.click(screen.getByRole("button", { name: "Retry connection" }));
    await screen.findByText("SHA256:retry");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Install Remote and game" }),
    ).toBeDisabled();
  });
  it("does not accept a completed identity check for a previously selected host", async () => {
    let resolve: (value: unknown) => void = () => {};
    invoke.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const { rerender } = render(
      <HostSetup server={server} local act={vi.fn()} />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Check Remote identity" }),
    );
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    rerender(
      <HostSetup
        server={{ ...server, id: "two", host: "192.168.1.3" }}
        local
        act={vi.fn()}
      />,
    );
    resolve({ keys: "old key", fingerprint: "SHA256:old" });
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Check Remote identity" }),
      ).toBeEnabled(),
    );
    expect(screen.queryByText("SHA256:old")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Install Remote and game" }),
    ).toBeDisabled();
  });
  it("queues local deployment only after its resource review", async () => {
    const api = new Api("", "");
    const done = vi.fn();
    const request = vi.spyOn(api, "request").mockImplementation(async (path) =>
      path === "/game/releases"
        ? {
            versions: [
              { version: "1.22.7", latest: true, automatic_idle: false },
            ],
            latest: "1.22.7",
            status: "current",
          }
        : path === "/accounts"
          ? { id: "account" }
          : path === "/plans"
            ? {
                id: "plan",
                resources: ["Docker volume"],
                warnings: ["Game port binds on all interfaces"],
              }
            : {},
    );
    render(
      <DeployTarget api={api} localDesktop onDone={done} cloud={() => null} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Local Docker" }));
    fireEvent.change(screen.getByLabelText("Game server name"), {
      target: { value: "test-world" },
    });
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Review setup" }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Review setup" }));
    await screen.findByText("Docker volume");
    expect(request).toHaveBeenCalledWith(
      "/plans",
      "POST",
      expect.objectContaining({ game_version: "1.22.7" }),
    );
    expect(screen.getByText("Game port binds on all interfaces")).toBeVisible();
    expect(request.mock.calls.some(([path]) => path === "/deployments")).toBe(
      false,
    );
    fireEvent.click(screen.getByRole("button", { name: "Install locally" }));
    await waitFor(() => expect(done).toHaveBeenCalled());
    expect(request).toHaveBeenLastCalledWith("/deployments", "POST", {
      plan_id: "plan",
      accept_incomplete_estimate: true,
    });
  });
});
