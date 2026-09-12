// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import CloudIdleSettings from "./CloudIdleSettings";
import { Api, type Server } from "./api";
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
const server = {
  id: "azure-one",
  provider: "azure",
  state: "attention",
} as Server;
const policy = {
  enabled: false,
  minutes: 15,
  external_manager_confirmed: false,
  wake_pending: false,
};
it("explains installation prerequisites and blocks enabling before Remote setup", async () => {
  const request = vi.fn().mockResolvedValue(policy);
  render(
    <CloudIdleSettings
      api={{ request } as unknown as Api}
      server={{ ...server, state: "awaiting_bootstrap" }}
      localManager
    />,
  );
  await waitFor(() => expect(request).toHaveBeenCalled());
  expect(
    screen.getByText(/Install the Remote and game in Infrastructure first/),
  ).toBeVisible();
  expect(
    screen.getByLabelText("Deallocate when no players are connected"),
  ).toBeDisabled();
});
it("still allows an existing cost-saving policy to be disabled while setup is incomplete", async () => {
  const request = vi.fn(
    async (_path: string, method?: string, body?: unknown) =>
      method === "PUT"
        ? body
        : { ...policy, enabled: true, external_manager_confirmed: true },
  );
  render(
    <CloudIdleSettings
      api={{ request } as unknown as Api}
      server={{ ...server, state: "awaiting_bootstrap" }}
      localManager
    />,
  );
  const enabled = screen.getByLabelText(
    "Deallocate when no players are connected",
  );
  await waitFor(() => expect(enabled).toBeEnabled());
  fireEvent.click(enabled);
  fireEvent.click(
    screen.getByRole("button", { name: "Save cost-saving settings" }),
  );
  await screen.findByText("Cost-saving settings saved.");
  expect(request).toHaveBeenCalledWith(
    "/servers/azure-one/cloud-idle-policy",
    "PUT",
    { enabled: false, minutes: 15, external_manager_confirmed: true },
  );
});
it("defaults off and requires an external Manager acknowledgement before enabling", async () => {
  const request = vi.fn(
    async (_path: string, method?: string, body?: unknown) =>
      method === "PUT" ? body : policy,
  );
  render(
    <CloudIdleSettings api={{ request } as unknown as Api} server={server} />,
  );
  const enabled = await screen.findByLabelText(
    "Deallocate when no players are connected",
  );
  await waitFor(() => expect(enabled).toBeEnabled());
  expect(enabled).not.toBeChecked();
  expect(screen.getByLabelText("Minutes without players")).toHaveValue(15);
  fireEvent.click(enabled);
  expect(
    screen.getByRole("button", { name: "Save cost-saving settings" }),
  ).toBeDisabled();
  fireEvent.click(
    screen.getByLabelText("This Manager runs outside the game server’s VM."),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Save cost-saving settings" }),
  );
  await screen.findByText("Cost-saving settings saved.");
  expect(request).toHaveBeenCalledWith(
    "/servers/azure-one/cloud-idle-policy",
    "PUT",
    { enabled: true, minutes: 15, external_manager_confirmed: true },
  );
  expect(screen.getByText(/disk and public IP charges continue/)).toBeVisible();
  expect(
    screen.getByText(/joining the game cannot wake a deallocated VM/),
  ).toBeVisible();
});
it("keeps recovery available after partial failure and reports running only after the operation succeeds", async () => {
  let complete: (value: unknown) => void = () => {};
  const finished = new Promise((resolve) => {
    complete = resolve;
  });
  const request = vi.fn(async (path: string) =>
    path.startsWith("/operations/")
      ? finished
      : { ...policy, last_error: "Deallocation timed out", wake_pending: true },
  );
  const action = vi.fn(async () => ({ id: "wake" }));
  render(
    <CloudIdleSettings
      api={{ request, action } as unknown as Api}
      server={server}
    />,
  );
  expect(await screen.findByText("Deallocation timed out")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Start server" }));
  await waitFor(() =>
    expect(action).toHaveBeenCalledWith("azure-one", "cloud-start"),
  );
  expect(
    screen.getByRole("button", { name: "Starting server…" }),
  ).toBeDisabled();
  expect(screen.queryByText("Server is running.")).not.toBeInTheDocument();
  complete({
    id: "wake",
    status: "succeeded",
    message: "Game health verified",
  });
  expect(await screen.findByText("Server is running.")).toBeVisible();
  expect(
    screen.queryByRole("button", { name: "Start server" }),
  ).not.toBeInTheDocument();
});
it("keeps a failed wake retryable and does not claim success", async () => {
  const request = vi.fn(async (path: string) =>
    path.startsWith("/operations/")
      ? { status: "failed", message: "Game did not become healthy" }
      : { ...policy, wake_pending: true },
  );
  const action = vi.fn(async () => ({ id: "wake" }));
  render(
    <CloudIdleSettings
      api={{ request, action } as unknown as Api}
      server={server}
    />,
  );
  fireEvent.click(await screen.findByRole("button", { name: "Start server" }));
  expect(await screen.findByText("Game did not become healthy")).toBeVisible();
  expect(screen.getByRole("button", { name: "Start server" })).toBeEnabled();
  expect(screen.queryByText("Server is running.")).not.toBeInTheDocument();
});

it("refreshes automatic sleep intent without discarding unsaved settings", async () => {
  vi.useFakeTimers();
  let current = { ...policy };
  const request = vi.fn(async () => current);
  await act(async () => {
    render(
      <CloudIdleSettings api={{ request } as unknown as Api} server={server} />,
    );
  });
  fireEvent.click(
    screen.getByLabelText("Deallocate when no players are connected"),
  );
  fireEvent.change(screen.getByLabelText("Minutes without players"), {
    target: { value: 30 },
  });
  current = { ...policy, wake_pending: true };
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5000);
  });
  expect(screen.getByRole("button", { name: "Start server" })).toBeVisible();
  expect(
    screen.getByLabelText("Deallocate when no players are connected"),
  ).toBeChecked();
  expect(screen.getByLabelText("Minutes without players")).toHaveValue(30);
});

it("uses the known local desktop Manager without asking users to confirm its location", async () => {
  const request = vi.fn(
    async (_path: string, method?: string, body?: unknown) =>
      method === "PUT" ? body : policy,
  );
  render(
    <CloudIdleSettings
      api={{ request } as unknown as Api}
      server={server}
      localManager
    />,
  );
  expect(
    screen.getByText(/Keep this app open to deallocate the VM/),
  ).toHaveTextContent("the VM stays allocated and billable");
  expect(
    screen.getByText(/joining the game cannot wake a deallocated VM/),
  ).toBeInTheDocument();
  expect(screen.getByText(/The Remote checks for players/)).toBeInTheDocument();
  expect(
    screen.getByText(/This Manager deallocates the VM/),
  ).toBeInTheDocument();
  const enabled = screen.getByLabelText(
    "Deallocate when no players are connected",
  );
  await waitFor(() => expect(enabled).toBeEnabled());
  fireEvent.click(enabled);
  expect(
    screen.queryByLabelText("This Manager runs outside the game server’s VM."),
  ).not.toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "Save cost-saving settings" }),
  );
  await screen.findByText("Cost-saving settings saved.");
  expect(request).toHaveBeenCalledWith(
    "/servers/azure-one/cloud-idle-policy",
    "PUT",
    expect.objectContaining({
      enabled: true,
      external_manager_confirmed: true,
    }),
  );
});
