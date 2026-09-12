// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { Api } from "./api";
import { useInstancePrices } from "./useInstancePrices";
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
it("publishes individual prices progressively and ends loading when a remaining request hangs", async () => {
  vi.useFakeTimers();
  const request = vi.fn(async (path: string) =>
    path.includes("size=fast")
      ? { monthly_compute: 42, currency: "USD" }
      : new Promise(() => {}),
  );
  const api = { url: "local", request } as unknown as Api;
  const { result } = renderHook(() =>
    useInstancePrices(api, "one", "east", ["fast", "slow"], true),
  );
  await act(async () => {});
  expect(result.current.prices.fast.price).toBe(42);
  expect(result.current.loading).toBe(true);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(15000);
  });
  expect(result.current.loading).toBe(false);
  expect(result.current.errors).toContain("slow");
  expect(result.current.prices.fast.price).toBe(42);
  expect(request.mock.calls.every((c) => c[0].includes("/price?"))).toBe(true);
});
it("aborts old-region work and never displays stale prices after a region change", async () => {
  let oldResolve: (v: unknown) => void = () => {};
  const old = new Promise((resolve) => {
    oldResolve = resolve;
  });
  const request = vi.fn(
    async (
      path: string,
      _method?: string,
      _body?: unknown,
      _signal?: AbortSignal,
    ) =>
      path.includes("region=east")
        ? old
        : { monthly_compute: 84, currency: "USD" },
  );
  const api = { url: "local", request } as unknown as Api;
  const { result, rerender } = renderHook(
    ({ region }) => useInstancePrices(api, "one", region, ["same"], true),
    { initialProps: { region: "east" } },
  );
  rerender({ region: "west" });
  await waitFor(() => expect(result.current.prices.same?.price).toBe(84));
  expect(request.mock.calls[0][3]?.aborted).toBe(true);
  await act(async () => {
    oldResolve({ monthly_compute: 1, currency: "USD" });
  });
  expect(result.current.prices.same.price).toBe(84);
});
