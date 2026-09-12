// @vitest-environment jsdom
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import GameVersionPicker from "./GameVersionPicker";
import { Api } from "./api";
afterEach(cleanup);
const catalog = {
  versions: [
    { version: "1.22.7", latest: true, automatic_idle: false },
    { version: "1.22.0", latest: false, automatic_idle: true },
  ],
  latest: "1.22.7",
  status: "current",
};
function Form({ api, installed }: { api: Api; installed?: string }) {
  const [version, setVersion] = useState(installed || "");
  const [ready, setReady] = useState(false);
  return (
    <>
      <GameVersionPicker
        api={api}
        value={version}
        installedVersion={installed}
        onChange={setVersion}
        onReady={setReady}
      />
      <button disabled={!ready || version === installed}>Install</button>
    </>
  );
}
it("selects the latest downloadable stable release for new servers", async () => {
  const request = vi.fn(async () => catalog);
  render(<Form api={{ request } as unknown as Api} />);
  expect(screen.getByRole("button", { name: "Install" })).toBeDisabled();
  await waitFor(() =>
    expect(screen.getByLabelText("Vintage Story version")).toHaveValue(
      "1.22.7",
    ),
  );
  expect(request).toHaveBeenCalledWith("/game/releases");
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Install" })).toBeEnabled(),
  );
  expect(screen.getByText(/Automatic sleep is not yet tested/)).toBeVisible();
  fireEvent.change(screen.getByLabelText("Vintage Story version"), {
    target: { value: "1.22.0" },
  });
  expect(
    screen.queryByText(/Automatic sleep is not yet tested/),
  ).not.toBeInTheDocument();
});
it("preserves an installed version without automatically selecting an update", async () => {
  render(
    <Form
      api={{ request: vi.fn(async () => catalog) } as unknown as Api}
      installed="1.22.0"
    />,
  );
  await screen.findByRole("option", { name: "1.22.7 — Latest stable" });
  expect(screen.getByLabelText("Vintage Story version")).toHaveValue("1.22.0");
  expect(screen.getByRole("button", { name: "Install" })).toBeDisabled();
});
it("keeps an unlisted installed release visible but unavailable for download", async () => {
  render(
    <Form
      api={{ request: vi.fn(async () => catalog) } as unknown as Api}
      installed="1.20.9"
    />,
  );
  expect(
    await screen.findByRole("option", {
      name: "1.20.9 — Installed — unavailable",
    }),
  ).toBeDisabled();
  expect(screen.getByLabelText("Vintage Story version")).toHaveValue("1.20.9");
  expect(screen.getByRole("button", { name: "Install" })).toBeDisabled();
});
it("does not guess a version when the catalog fails and can retry", async () => {
  const request = vi
    .fn()
    .mockRejectedValueOnce(new Error("Downloads unavailable"))
    .mockResolvedValue(catalog);
  render(<Form api={{ request } as unknown as Api} />);
  expect(await screen.findByText("Downloads unavailable")).toBeVisible();
  expect(screen.getByLabelText("Vintage Story version")).toHaveValue("");
  expect(screen.getByRole("button", { name: "Install" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Retry release list" }));
  await waitFor(() =>
    expect(screen.getByLabelText("Vintage Story version")).toHaveValue(
      "1.22.7",
    ),
  );
});

it("does not change the installed selection when a stale catalog is refreshed", async () => {
  const request = vi
    .fn()
    .mockResolvedValueOnce({ ...catalog, status: "stale" })
    .mockResolvedValue({
      ...catalog,
      latest: "1.22.8",
      versions: [
        { version: "1.22.8", latest: true, automatic_idle: false },
        ...catalog.versions.map((v) => ({ ...v, latest: false })),
      ],
    });
  render(<Form api={{ request } as unknown as Api} installed="1.22.0" />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Retry release list" }),
  );
  await screen.findByRole("option", { name: "1.22.8 — Latest stable" });
  expect(screen.getByLabelText("Vintage Story version")).toHaveValue("1.22.0");
  expect(screen.getByRole("button", { name: "Install" })).toBeDisabled();
});
