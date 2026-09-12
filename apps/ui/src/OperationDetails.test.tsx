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
import OperationDetails from "./OperationDetails";
import { Api, type Operation } from "./api";
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
const operation = {
  id: "one",
  action: "mod-install",
  status: "failed",
  message: "Failed to install",
} as Operation;
it("loads detailed steps only when expanded and copies their exact text", async () => {
  const request = vi
    .fn()
    .mockResolvedValue({
      available: true,
      summary: "Archive rejected",
      entries: [
        {
          seq: 1,
          at: "2026-09-06",
          level: "error",
          stage: "validate",
          message: "modinfo.json was not found",
        },
      ],
      retention: { days: 30, max_entries: 200 },
    });
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  render(
    <OperationDetails
      api={{ request } as unknown as Api}
      operation={operation}
    />,
  );
  expect(request).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("Operation details"));
  await screen.findByLabelText("Operation log");
  expect(screen.getByLabelText("Operation log")).toHaveTextContent(
    "validate: modinfo.json was not found",
  );
  fireEvent.click(screen.getByRole("button", { name: "Copy details" }));
  await screen.findByRole("button", { name: "Copied" });
  expect(writeText).toHaveBeenCalledWith(
    "mod-install — failed\nArchive rejected\n2026-09-06 [error] validate: modinfo.json was not found",
  );
});
it("supports retry and identifies legacy summary-only operations honestly", async () => {
  const request = vi
    .fn()
    .mockRejectedValueOnce(new Error("Connection interrupted"))
    .mockResolvedValueOnce({
      available: false,
      summary: "Original failure summary",
      entries: [],
      retention: { days: 30, max_entries: 200 },
    });
  render(
    <OperationDetails
      api={{ request } as unknown as Api}
      operation={operation}
    />,
  );
  fireEvent.click(screen.getByText("Operation details"));
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  await screen.findByText(/Detailed logs were not recorded/);
  expect(screen.getByLabelText("Operation log")).toHaveTextContent(
    "Original failure summary",
  );
  await waitFor(() =>
    expect(screen.queryByRole("alert")).not.toBeInTheDocument(),
  );
});
