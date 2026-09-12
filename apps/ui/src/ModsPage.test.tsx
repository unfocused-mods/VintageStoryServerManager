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
import ModsPage from "./ModsPage";
import { Api, type Operation, type Server } from "./api";
afterEach(cleanup);
it("keeps the terminal installed list when an older list request resolves late", async () => {
  let finishOld: (value: unknown[]) => void = () => {};
  const old = new Promise<unknown[]>((resolve) => {
    finishOld = resolve;
  });
  let lists = 0;
  const request = vi.fn(async (path: string) =>
    path.startsWith("/operations/")
      ? { ...operation, status: "succeeded" }
      : ++lists === 1
        ? old
        : [{ modid: "vigor", version: "122.7.4" }],
  );
  render(
    <ModsPage
      api={{ request } as unknown as Api}
      server={server}
      operations={[operation]}
    />,
  );
  await screen.findByRole("button", { name: "Disable" });
  await act(async () => finishOld([]));
  expect(screen.getByRole("button", { name: "Disable" })).toBeVisible();
});
it("queues custom ZIP asynchronously and resets its acknowledgement only after acceptance", async () => {
  let accept: (value: Operation) => void = () => {};
  const accepted = new Promise<Operation>((resolve) => {
    accept = resolve;
  });
  const request = vi.fn(async (_path: string, method?: string) =>
    method === "POST"
      ? accepted
      : _path.startsWith("/operations/")
        ? new Promise(() => {})
        : [],
  );
  const { container } = render(
    <ModsPage
      api={{ request } as unknown as Api}
      server={server}
      operations={[]}
    />,
  );
  const ack = screen.getByLabelText(/I reviewed game and mod compatibility/);
  fireEvent.click(ack);
  const file = new File(["zip"], "vigor.zip", { type: "application/zip" });
  Object.defineProperty(file, "arrayBuffer", {
    value: async () => new Uint8Array([1, 2, 3]).buffer,
  });
  fireEvent.change(container.querySelector('input[type="file"]')!, {
    target: { files: [file] },
  });
  await waitFor(() =>
    expect(request.mock.calls.some(([, method]) => method === "POST")).toBe(
      true,
    ),
  );
  expect(ack).toBeChecked();
  await act(async () => accept({ ...operation, action: "mod-upload" }));
  await screen.findByText("Installing…");
  expect(screen.getByText("vigor.zip")).toBeVisible();
  expect(ack).not.toBeChecked();
});
it("does not enqueue a ZIP that finishes reading after leaving the server", async () => {
  let finish: (value: ArrayBuffer) => void = () => {};
  const reading = new Promise<ArrayBuffer>((resolve) => {
    finish = resolve;
  });
  const request = vi.fn().mockResolvedValue([]);
  const { container, unmount } = render(
    <ModsPage
      api={{ request } as unknown as Api}
      server={server}
      operations={[]}
    />,
  );
  fireEvent.click(
    screen.getByLabelText(/I reviewed game and mod compatibility/),
  );
  const file = new File(["zip"], "vigor.zip");
  Object.defineProperty(file, "arrayBuffer", { value: () => reading });
  fireEvent.change(container.querySelector('input[type="file"]')!, {
    target: { files: [file] },
  });
  unmount();
  await act(async () => finish(new ArrayBuffer(1)));
  expect(request.mock.calls.some(([, method]) => method === "POST")).toBe(
    false,
  );
});
const server = { id: "one", game_version: "1.22.7" } as Server;
const operation = {
  id: "install-one",
  server_id: "one",
  action: "mod-install",
  status: "queued",
  message: "Queued",
} as Operation;
const mod = {
  modid: "vigor",
  name: "Vigor",
  releases: [{ releaseid: 1, modversion: "122.7.4", tags: ["1.22.7"] }],
};
async function choose() {
  fireEvent.submit(screen.getByLabelText("Search ModDB").closest("form")!);
  fireEvent.click(await screen.findByRole("button", { name: "Vigor" }));
  await screen.findByLabelText("Release");
  fireEvent.click(
    screen.getByLabelText(
      "I trust this mod and have reviewed its requirements.",
    ),
  );
}
it("shows Installing and clears the accepted form before completion, then refreshes the installed list", async () => {
  let finish: (value: Operation) => void = () => {};
  const terminal = new Promise<Operation>((resolve) => {
    finish = resolve;
  });
  let installed = false;
  const request = vi.fn(async (path: string) =>
    path === "/moddb?vigor" || path === "/moddb?q="
      ? { mods: [mod] }
      : path === "/moddb/vigor"
        ? mod
        : path === "/operations/install-one"
          ? terminal
          : path.endsWith("/agent/mods")
            ? installed
              ? [{ modid: "vigor", version: "122.7.4" }]
              : []
            : {},
  );
  const action = vi.fn().mockResolvedValue(operation);
  render(
    <ModsPage
      api={{ request, action } as unknown as Api}
      server={server}
      operations={[]}
    />,
  );
  await choose();
  fireEvent.click(screen.getByRole("button", { name: "Install release" }));
  await screen.findByText("Installing…");
  expect(screen.getByText("Vigor 122.7.4")).toBeVisible();
  expect(screen.queryByLabelText("Release")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Vigor" }));
  await screen.findByLabelText("Release");
  expect(
    screen.getByRole("button", { name: "Install release" }),
  ).toBeDisabled();
  expect(action).toHaveBeenCalledTimes(1);
  installed = true;
  await act(async () =>
    finish({ ...operation, status: "succeeded", message: "Installed" }),
  );
  await screen.findByRole("button", { name: "Disable" });
  expect(screen.queryByText("Installing…")).not.toBeInTheDocument();
  expect(
    request.mock.calls.filter(([path]) => path.endsWith("/agent/mods")),
  ).toHaveLength(2);
});
it("preserves the form on enqueue failure and shows terminal failure without pretending installation succeeded", async () => {
  const request = vi.fn(async (path: string) =>
    path.startsWith("/moddb?")
      ? { mods: [mod] }
      : path === "/moddb/vigor"
        ? mod
        : path.startsWith("/operations/")
          ? { ...operation, status: "failed", message: "Archive rejected" }
          : [],
  );
  const action = vi
    .fn()
    .mockRejectedValueOnce(new Error("Connection failed"))
    .mockResolvedValueOnce(operation);
  render(
    <ModsPage
      api={{ request, action } as unknown as Api}
      server={server}
      operations={[]}
    />,
  );
  await choose();
  fireEvent.click(screen.getByRole("button", { name: "Install release" }));
  await screen.findByText("Error: Connection failed");
  expect(screen.getByLabelText("Release")).toHaveValue("1");
  expect(
    screen.getByLabelText(
      "I trust this mod and have reviewed its requirements.",
    ),
  ).toBeChecked();
  fireEvent.click(screen.getByRole("button", { name: "Install release" }));
  await screen.findByText("Failed: Archive rejected");
  expect(
    screen.getByText(/select the mod or ZIP again to retry/),
  ).toBeVisible();
  expect(
    screen.queryByRole("button", { name: "Disable" }),
  ).not.toBeInTheDocument();
});
it("recovers pending operations on return and uses safe icons with a broken-image fallback", async () => {
  const request = vi.fn(async (path: string) =>
    path.startsWith("/operations/")
      ? new Promise(() => {})
      : [
          {
            modid: "iconmod",
            icon_data_url: "data:image/png;base64,fixture",
            version: "1.0",
          },
        ],
  );
  const { container } = render(
    <ModsPage
      api={{ request } as unknown as Api}
      server={server}
      operations={[operation]}
    />,
  );
  await screen.findByText("Installing…");
  await waitFor(() =>
    expect(container.querySelector(".mod-list img")).not.toBeNull(),
  );
  fireEvent.error(container.querySelector(".mod-list img")!);
  expect(container.querySelector(".mod-list img")).toBeNull();
});
