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
import ModBrowser, {
  versionSortedReleases,
  compatibleRelease,
} from "./ModBrowser";
import { Api, type Server } from "./api";
afterEach(cleanup);
it("locks mod selection while enqueueing and unlocks after acceptance", async () => {
  let accept: () => void = () => {};
  const install = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        accept = resolve;
      }),
  );
  const request = vi.fn(async (path: string) =>
    path.startsWith("/moddb?")
      ? {
          mods: [
            { modid: 1, name: "First" },
            { modid: 2, name: "Second" },
          ],
        }
      : {
          modid: 1,
          name: "First",
          releases: [{ releaseid: 1, modversion: "122.7.4", tags: ["1.22.7"] }],
        },
  );
  render(
    <ModBrowser
      api={{ request } as unknown as Api}
      server={server}
      install={install}
    />,
  );
  await search();
  fireEvent.click(screen.getByRole("button", { name: "First" }));
  await screen.findByLabelText("Release");
  fireEvent.click(
    screen.getByLabelText(
      "I trust this mod and have reviewed its requirements.",
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "Install release" }));
  expect(screen.getByRole("button", { name: "Second" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Second" }));
  expect(request.mock.calls.some(([path]) => path === "/moddb/2")).toBe(false);
  await act(async () => accept());
  expect(screen.queryByLabelText("Release")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Second" })).toBeEnabled();
});
it("requires an explicit choice when no release declares support for the exact game version", async () => {
  const request = vi.fn(async (path: string) =>
    path.startsWith("/moddb?")
      ? { mods: [{ modid: 1, name: "First" }] }
      : {
          modid: 1,
          name: "First",
          releases: [{ releaseid: 1, modversion: "121.9.0", tags: ["1.21.6"] }],
        },
  );
  render(
    <ModBrowser
      api={{ request } as unknown as Api}
      server={server}
      install={vi.fn()}
    />,
  );
  await search();
  fireEvent.click(screen.getByRole("button", { name: "First" }));
  await screen.findByLabelText("Release");
  expect(screen.getByLabelText("Release")).toHaveValue("");
  expect(
    screen.getByText(/No comparable release declares support/),
  ).toBeVisible();
  fireEvent.change(screen.getByLabelText("Release"), {
    target: { value: "1" },
  });
  expect(
    screen.getByText("Compatibility with this game version is unconfirmed."),
  ).toBeVisible();
});
it("compares numeric segments, stable versions and prereleases without ranking unknown versions", () => {
  const versions = [
    "122.7.9",
    "122.7.10-beta.2",
    "unknown",
    "122.7.10-beta.10",
    "122.7.10",
  ].map((modversion, i) => ({
    modversion,
    releaseid: i + 1,
    tags: ["1.22.7"],
  }));
  expect(versionSortedReleases(versions).map((r) => r.modversion)).toEqual([
    "122.7.10",
    "122.7.10-beta.10",
    "122.7.10-beta.2",
    "122.7.9",
    "unknown",
  ]);
  expect(
    compatibleRelease(
      [{ releaseid: 100, modversion: "unknown", tags: ["1.22.7"] }],
      "1.22.7",
    ),
  ).toBeUndefined();
});
const server = { id: "one", game_version: "1.22.7" } as Server;
const releases = [
  {
    releaseid: 30,
    modversion: "122.7.4",
    created: "2026-04-01 10:00:00",
    tags: ["1.22.7"],
  },
  {
    releaseid: 20,
    modversion: "121.99.9",
    created: "2026-05-01 10:00:00",
    tags: ["1.21.6"],
  },
  {
    releaseid: 10,
    modversion: "122.7.3",
    created: "2026-03-01 10:00:00",
    tags: ["1.22.7"],
  },
];
async function search() {
  fireEvent.submit(screen.getByLabelText("Search ModDB").closest("form")!);
  await screen.findByRole("button", { name: "First" });
}
it("selects the highest compatible branch despite later incompatible publication and preserves a manual choice", async () => {
  const request = vi.fn(async (path: string) =>
    path.startsWith("/moddb?")
      ? { mods: [{ modid: 1, name: "First" }] }
      : { modid: 1, name: "First", releases },
  );
  const action = vi.fn().mockResolvedValue(undefined);
  render(
    <ModBrowser
      api={{ request } as unknown as Api}
      server={server}
      install={action}
    />,
  );
  await search();
  fireEvent.click(screen.getByRole("button", { name: "First" }));
  await waitFor(() =>
    expect(screen.getByLabelText("Release")).toHaveValue("30"),
  );
  expect(
    screen.getByText(
      "Compatible with this game version, according to the author.",
    ),
  ).toBeVisible();
  expect(
    screen.getByRole("button", { name: "Install release" }),
  ).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Release"), {
    target: { value: "20" },
  });
  fireEvent.click(screen.getByRole("button", { name: "First" }));
  await waitFor(() =>
    expect(screen.getByLabelText("Release")).toHaveValue("20"),
  );
  fireEvent.click(
    screen.getByLabelText(
      "I trust this mod and have reviewed its requirements.",
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "Install release" }));
  expect(action).toHaveBeenCalledWith(
    { modid: "1", releaseid: 20, acknowledge_unknown_compatibility: true },
    { name: "First", version: "121.99.9" },
  );
  await waitFor(() =>
    expect(screen.queryByLabelText("Release")).not.toBeInTheDocument(),
  );
});
it("uses release IDs as a deterministic fallback for missing or equal dates", () => {
  expect(
    versionSortedReleases([
      { releaseid: 1 },
      { releaseid: 3 },
      { releaseid: 2 },
    ]).map((r) => r.releaseid),
  ).toEqual([3, 2, 1]);
});
it("ignores late mod detail responses and clears selection when the server changes", async () => {
  let finishFirst: (value: unknown) => void = () => {};
  const first = new Promise((resolve) => {
    finishFirst = resolve;
  });
  const request = vi.fn(async (path: string) =>
    path.startsWith("/moddb?")
      ? {
          mods: [
            { modid: 1, name: "First" },
            { modid: 2, name: "Second" },
          ],
        }
      : path === "/moddb/1"
        ? first
        : { modid: 2, name: "Second", releases: [releases[2]] },
  );
  const api = { request } as unknown as Api;
  const { rerender } = render(
    <ModBrowser
      api={api}
      server={server}
      install={vi.fn().mockResolvedValue(undefined)}
    />,
  );
  await search();
  fireEvent.click(screen.getByRole("button", { name: "First" }));
  fireEvent.click(screen.getByRole("button", { name: "Second" }));
  await waitFor(() =>
    expect(screen.getByLabelText("Release")).toHaveValue("10"),
  );
  await act(async () => {
    finishFirst({ modid: 1, name: "First", releases });
  });
  expect(screen.getByLabelText("Release")).toHaveValue("10");
  fireEvent.click(screen.getByRole("button", { name: "First" }));
  rerender(
    <ModBrowser
      api={api}
      server={{ ...server, id: "two" }}
      install={vi.fn().mockResolvedValue(undefined)}
    />,
  );
  await act(async () => {});
  expect(screen.queryByLabelText("Release")).not.toBeInTheDocument();
});
