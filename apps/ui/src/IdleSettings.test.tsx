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
import IdleSettings from "./IdleSettings";
import { Api } from "./api";
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
it("saves optional sleep policy and makes wake and billing limits explicit", async () => {
  const api = new Api("http://localhost", "");
  const request = vi
    .spyOn(api, "request")
    .mockResolvedValue({
      policy: { enabled: false, minutes: 15 },
      sleeping: false,
      error: null,
    });
  render(<IdleSettings api={api} serverId="one" />);
  await waitFor(() =>
    expect(
      screen.getByLabelText("Enable automatic sleep and wake"),
    ).toBeEnabled(),
  );
  fireEvent.click(screen.getByLabelText("Enable automatic sleep and wake"));
  fireEvent.change(screen.getByLabelText("Minutes without players"), {
    target: { value: "30" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save sleep settings" }));
  await waitFor(() =>
    expect(request).toHaveBeenCalledWith(
      "/servers/one/agent/idle-policy",
      "PUT",
      { enabled: true, minutes: 30 },
    ),
  );
  expect(
    screen.getByText(/Runs on the Remote, even with this app closed/),
  ).toBeInTheDocument();
  expect(
    screen.getByText(/Joining starts the game again while the host is running/),
  ).toBeInTheDocument();
  expect(screen.getByText(/cloud VM charges continue/)).toBeInTheDocument();
  expect(
    screen.getByText(/first player may need to retry/),
  ).toBeInTheDocument();
});
