// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { Accounts } from "./App";
import { Api } from "./api";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
afterEach(() => {
  cleanup();
  invoke.mockReset();
});
const account = {
  id: "one",
  name: "My Azure",
  provider: "azure",
  credential_ref: "",
  settings: { subscription_id: "sub", auth: "azure_cli" },
};
function setup(fail = false, existing = false) {
  const request = vi.fn(async (path: string, method?: string, body?: any) => {
    if (path.endsWith("/validate")) {
      if (fail) throw new Error("Session expired");
      return {};
    }
    if (method === "POST") return { ...body, id: body.id || "new" };
    if (method === "DELETE") return {};
    return existing ? [account] : [];
  });
  render(<Accounts api={{ request } as unknown as Api} onError={vi.fn()} />);
  return request;
}
it("automatically verifies saved credentials and replaces the form with neutral success", async () => {
  const request = setup();
  await waitFor(() => expect(request).toHaveBeenCalledWith("/accounts"));
  fireEvent.change(screen.getByLabelText("Display name"), {
    target: { value: "New Azure" },
  });
  fireEvent.change(screen.getByLabelText("Subscription ID"), {
    target: { value: "sub" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save account" }));
  await screen.findByText("Credentials verified");
  expect(screen.getByRole("status")).not.toHaveClass("inline-error");
  expect(
    screen.queryByRole("button", { name: "Save account" }),
  ).not.toBeInTheDocument();
  expect(
    request.mock.calls.filter((c) => c[0] === "/accounts" && c[1] === "POST"),
  ).toHaveLength(1);
});
it("keeps a saved account editable when automatic verification fails without creating another", async () => {
  const request = setup(true);
  await waitFor(() => expect(request).toHaveBeenCalledWith("/accounts"));
  fireEvent.change(screen.getByLabelText("Subscription ID"), {
    target: { value: "sub" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save account" }));
  await screen.findByText(/Account saved. Verification failed/);
  fireEvent.click(screen.getByRole("button", { name: "Retry verification" }));
  await waitFor(() =>
    expect(
      request.mock.calls.filter((c) => c[0].endsWith("/validate")),
    ).toHaveLength(2),
  );
  expect(
    request.mock.calls.filter((c) => c[0] === "/accounts" && c[1] === "POST"),
  ).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Edit Microsoft Azure" }));
  expect(screen.getByLabelText("Subscription ID")).toHaveValue("sub");
});
it("edits the existing account identity and confirms removal", async () => {
  const request = setup(false, true);
  fireEvent.click(await screen.findByRole("button", { name: "Edit My Azure" }));
  expect(screen.getByLabelText("Provider")).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Display name"), {
    target: { value: "Renamed" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await screen.findByText("Credentials verified");
  expect(request).toHaveBeenCalledWith(
    "/accounts",
    "POST",
    expect.objectContaining({ id: "one", name: "Renamed" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Delete Renamed" }));
  expect(request).not.toHaveBeenCalledWith("/accounts/one", "DELETE");
  fireEvent.click(screen.getByRole("button", { name: "Remove account" }));
  await waitFor(() =>
    expect(request).toHaveBeenCalledWith("/accounts/one", "DELETE"),
  );
  expect(screen.queryByText("Renamed")).not.toBeInTheDocument();
});

it("stores desktop tokens in native credential storage and sends only references to the Manager", async () => {
  invoke.mockResolvedValue("keyring:cloud-private");
  const request = vi.fn(async (path: string, method?: string, body?: any) =>
    method === "POST" ? (path.endsWith("validate") ? {} : body) : [],
  );
  render(
    <Accounts
      api={{ request } as unknown as Api}
      onError={vi.fn()}
      localDesktop
    />,
  );
  await waitFor(() => expect(request).toHaveBeenCalledWith("/accounts"));
  fireEvent.change(screen.getByLabelText("Provider"), {
    target: { value: "vultr" },
  });
  fireEvent.change(screen.getByLabelText("API token"), {
    target: { value: "private-fixture-token" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save account" }));
  await screen.findByText("Credentials verified");
  expect(invoke).toHaveBeenCalledWith("store_cloud_credential", {
    secret: "private-fixture-token",
  });
  const saved = request.mock.calls.find(
    (c) => c[0] === "/accounts" && c[1] === "POST",
  )![2];
  expect(saved.credential_ref).toBe("keyring:cloud-private");
  expect(JSON.stringify(saved)).not.toContain("private-fixture-token");
});

it("keeps an account visible when removal is blocked and distinguishes removal errors from verification", async () => {
  const request = setup(false, true);
  await screen.findByRole("button", { name: "Edit My Azure" });
  request.mockRejectedValueOnce(
    new Error("This account is used by a game server."),
  );
  fireEvent.click(screen.getByRole("button", { name: "Delete My Azure" }));
  fireEvent.click(screen.getByRole("button", { name: "Remove account" }));
  expect(
    await screen.findByText("Error: This account is used by a game server."),
  ).toBeVisible();
  expect(screen.getByRole("button", { name: "Edit My Azure" })).toBeVisible();
  expect(
    screen.queryByRole("button", { name: "Retry verification" }),
  ).not.toBeInTheDocument();
});
