// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import AzureDiscovery from "./AzureDiscovery";
import HostSetup from "./HostSetup";
import { type Account, type Api, type Server } from "./api";
afterEach(cleanup);
const account = { id: "a", name: "Azure", provider: "azure" } as Account;
const server = { id: "s", name: "Oak", region: "newzealandnorth", size: "D2s", state: "recovered", host: "1.2.3.4", ssh_key: "", provider: "azure" } as Server;
const found = { deployments: [{ server, vm_id: "vm", resource_group: "vsm-oak", already_managed: false, warnings: [] }], warnings: [] };
it("only adopts an explicitly selected server, preserving optional key and custom name", async () => {
  const onAdded = vi.fn();
  const request = vi.fn(async (_path: string, method?: string) => method === "POST" ? { ...server, recovery_warnings: ["An untagged disk was left unmanaged."] } : found);
  render(<AzureDiscovery api={{ request } as unknown as Api} account={account} onAdded={onAdded} onClose={vi.fn()} />);
  await screen.findByText("vsm-oak");
  expect(request).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("button", { name: "Add to Manager" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("radio"));
  fireEvent.change(screen.getByLabelText("Server name"), { target: { value: "My world" } });
  fireEvent.change(screen.getByLabelText("SSH private key path (optional)"), { target: { value: "C:/keys/world" } });
  fireEvent.click(screen.getByRole("button", { name: "Add to Manager" }));
  await screen.findByText(/Oak added/);
  expect(screen.getByText("An untagged disk was left unmanaged.")).toBeInTheDocument();
  expect(screen.queryByLabelText("Server name")).not.toBeInTheDocument();
  expect(request).toHaveBeenCalledWith("/accounts/a/adopt", "POST", { vm_id: "vm", name: "My world", ssh_private_key: "C:/keys/world" });
  expect(onAdded).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("radio")).toBeDisabled();
  expect(request.mock.calls.some(([path]) => /deploy|bootstrap/.test(path))).toBe(false);
});
it("supports infrastructure recovery without a key and retains selection on failure", async () => {
  const request = vi.fn(async (_path: string, method?: string) => { if (method === "POST") throw new Error("Ownership changed"); return found; });
  render(<AzureDiscovery api={{ request } as unknown as Api} account={account} onClose={vi.fn()} />);
  fireEvent.click(await screen.findByRole("radio"));
  fireEvent.click(screen.getByRole("button", { name: "Add to Manager" }));
  await screen.findByRole("alert");
  expect(request).toHaveBeenLastCalledWith("/accounts/a/adopt", "POST", { vm_id: "vm", name: "Oak" });
  expect(screen.getByRole("radio")).toBeChecked();
});
it("ignores stale discovery responses after switching accounts", async () => {
  let finish!: (value: unknown) => void;
  const request = vi.fn((path: string) => path.includes("/a/") ? new Promise((resolve) => { finish = resolve; }) : Promise.resolve({ deployments: [], warnings: [] }));
  const api = { request } as unknown as Api;
  const view = render(<AzureDiscovery api={api} account={account} onClose={vi.fn()} />);
  view.rerender(<AzureDiscovery api={api} account={{ ...account, id: "b" }} onClose={vi.fn()} />);
  await screen.findByText(/No Server Manager deployments/);
  finish(found);
  await waitFor(() => expect(screen.queryByText("Oak")).not.toBeInTheDocument());
});
it("recovered hosts offer reconnect rather than installation, requiring a key", async () => {
  const reconnect = vi.fn().mockResolvedValue({}); const act = vi.fn();
  render(<HostSetup server={server} local={false} act={act} reconnect={reconnect} remoteInstalled={false} />);
  expect(screen.queryByRole("button", { name: /Install/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Reconnect Remote" })).toBeDisabled();
  fireEvent.change(screen.getByLabelText("SSH private key path"), { target: { value: "/keys/old" } });
  fireEvent.click(screen.getByRole("button", { name: "Reconnect Remote" }));
  await waitFor(() => expect(reconnect).toHaveBeenCalledWith("/keys/old"));
  expect(act).not.toHaveBeenCalled();
});
