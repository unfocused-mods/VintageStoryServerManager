// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { Api } from "./api";
import { useInfrastructureStatus } from "./useInfrastructureStatus";
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
it("reads provider power independently of game setup and only uses the status endpoint", async () => {
  const request = vi
    .fn()
    .mockResolvedValue({
      power_state: "running",
      provisioning_state: "Succeeded",
      checked_at: "2026-09-06T00:00:00Z",
    });
  const api = { request } as unknown as Api;
  const { result } = renderHook(() =>
    useInfrastructureStatus(api, "one", true, "awaiting_bootstrap"),
  );
  await waitFor(() =>
    expect(result.current.status?.power_state).toBe("running"),
  );
  expect(request).toHaveBeenCalledTimes(1);
  expect(request.mock.calls[0][0]).toBe("/servers/one/infrastructure/status");
});
it("does not overlap polls, bounds a stalled lookup, and retries after the interval", async () => {
  vi.useFakeTimers();
  const request = vi.fn().mockImplementation(() => new Promise(() => {}));
  const api = { request } as unknown as Api;
  const { result } = renderHook(() =>
    useInfrastructureStatus(api, "one", true, "awaiting_bootstrap"),
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(29999);
  });
  expect(request).toHaveBeenCalledTimes(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
  expect(result.current.loading).toBe(false);
  expect(result.current.error).toContain("timed out");
  expect(request.mock.calls[0][3].aborted).toBe(true);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30000);
  });
  expect(request).toHaveBeenCalledTimes(2);
});
it("does not request provider status on other pages", () => {
  const request = vi.fn();
  renderHook(() =>
    useInfrastructureStatus(
      { request } as unknown as Api,
      "one",
      false,
      "running",
    ),
  );
  expect(request).not.toHaveBeenCalled();
});
