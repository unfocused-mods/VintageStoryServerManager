// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { Api } from "./api";
import { useCloudCatalog } from "./useCloudCatalog";
afterEach(cleanup);
it("loads New Zealand North when Azure returns a mixed-case region name, without refetching for casing changes", async () => {
  const request = vi.fn(async (path: string) =>
    path.endsWith("/regions")
      ? {
          regions: [
            { name: "NewZealandNorth", displayName: "New Zealand North" },
          ],
        }
      : {
          sizes: [{ name: "Standard_D2s_v5", locations: ["NewZealandNorth"] }],
        },
  );
  const api = { url: "local", request } as unknown as Api;
  const { result, rerender } = renderHook(
    ({ region }) => useCloudCatalog(api, "one", "azure", region),
    {
      initialProps: { region: "newzealandnorth" },
    },
  );
  await waitFor(() =>
    expect(result.current.catalog.sizes).toEqual([
      { name: "Standard_D2s_v5", locations: ["NewZealandNorth"] },
    ]),
  );
  expect(request.mock.calls[1][0]).toBe(
    "/accounts/one/sizes?region=newzealandnorth",
  );
  rerender({ region: "NewZealandNorth" });
  expect(request).toHaveBeenCalledTimes(2);
  expect(result.current.loading).toBe(false);
});
it("loads Azure regions before requesting sizes for just the selected region and does not refetch on rerender", async () => {
  const request = vi.fn(async (path: string) =>
    path.endsWith("/regions")
      ? { regions: [{ name: "east" }, { name: "west" }] }
      : { sizes: [{ name: "size-east" }] },
  );
  const api = { url: "local", request } as unknown as Api;
  const { result, rerender } = renderHook(
    ({ region }) => useCloudCatalog(api, "one", "azure", region),
    { initialProps: { region: "" } },
  );
  await waitFor(() => expect(result.current.regionsLoading).toBe(false));
  expect(request).toHaveBeenCalledTimes(1);
  expect(request.mock.calls[0][0]).toBe("/accounts/one/regions");
  rerender({ region: "east" });
  await waitFor(() =>
    expect(result.current.catalog.sizes).toEqual([{ name: "size-east" }]),
  );
  expect(request.mock.calls[1][0]).toBe("/accounts/one/sizes?region=east");
  rerender({ region: "east" });
  expect(request).toHaveBeenCalledTimes(2);
});
it("keeps the newer region's sizes and cancels an older in-flight request", async () => {
  let release: (value: unknown) => void = () => {};
  const old = new Promise((resolve) => {
    release = resolve;
  });
  const request = vi.fn(
    async (
      path: string,
      _method?: string,
      _body?: unknown,
      _signal?: AbortSignal,
    ) =>
      path.endsWith("/regions")
        ? { regions: [{ name: "east" }, { name: "west" }] }
        : path.endsWith("=east")
          ? old
          : { sizes: [{ name: "size-west" }] },
  );
  const api = { url: "local", request } as unknown as Api;
  const { result, rerender } = renderHook(
    ({ region }) => useCloudCatalog(api, "one", "azure", region),
    { initialProps: { region: "east" } },
  );
  await waitFor(() => expect(request).toHaveBeenCalledTimes(2));
  rerender({ region: "west" });
  await waitFor(() =>
    expect(result.current.catalog.sizes).toEqual([{ name: "size-west" }]),
  );
  expect(request.mock.calls[1][3]?.aborted).toBe(true);
  await act(async () => {
    release({ sizes: [{ name: "stale-east" }] });
  });
  expect(result.current.catalog.sizes).toEqual([{ name: "size-west" }]);
});
