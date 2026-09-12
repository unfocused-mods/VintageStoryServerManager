import { useEffect, useState } from "react";
import { Api } from "./api";
type CatalogData = { regions?: unknown[]; sizes?: unknown[] };
type Snapshot = {
  key: string;
  data: CatalogData;
  loading: boolean;
  error: string;
};
async function fetchCatalog(
  api: Api,
  path: string,
  controller: AbortController,
): Promise<CatalogData> {
  let timer: ReturnType<typeof setTimeout>;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      controller.abort();
      reject(new Error("Catalog request timed out. Try again."));
    }, 30000);
  });
  let abort: () => void = () => {};
  const cancelled = new Promise<never>((_, reject) => {
    abort = () => reject(new Error("Catalog request cancelled"));
    controller.signal.addEventListener("abort", abort, { once: true });
  });
  try {
    return await Promise.race([
      api.request<CatalogData>(path, "GET", undefined, controller.signal),
      timeout,
      cancelled,
    ]);
  } finally {
    clearTimeout(timer!);
    controller.signal.removeEventListener("abort", abort);
  }
}
export function useCloudCatalog(
  api: Api,
  accountId: string,
  provider: string,
  region: string,
) {
  const [attempt, setAttempt] = useState(0);
  const accountKey = JSON.stringify([api.url, accountId, provider, attempt]);
  const [regions, setRegions] = useState<Snapshot>({
    key: "",
    data: {},
    loading: false,
    error: "",
  });
  const [sizes, setSizes] = useState<Snapshot>({
    key: "",
    data: {},
    loading: false,
    error: "",
  });
  useEffect(() => {
    const controller = new AbortController();
    let active = true;
    setRegions({ key: accountKey, data: {}, loading: !!accountId, error: "" });
    if (!accountId) return;
    void fetchCatalog(
      api,
      `/accounts/${accountId}/${provider === "azure" ? "regions" : "catalog"}`,
      controller,
    )
      .then((data) => {
        if (active)
          setRegions({ key: accountKey, data, loading: false, error: "" });
      })
      .catch((e) => {
        if (active)
          setRegions({
            key: accountKey,
            data: {},
            loading: false,
            error: String(e),
          });
      });
    return () => {
      active = false;
      controller.abort();
    };
  }, [api, accountKey]);
  const currentRegions =
    regions.key === accountKey
      ? regions
      : { key: accountKey, data: {}, loading: !!accountId, error: "" };
  const requestRegion = provider === "azure" ? region.toLowerCase() : region;
  const regionReady =
    !!accountId &&
    !currentRegions.loading &&
    !currentRegions.error &&
    !!region &&
    Array.isArray(currentRegions.data.regions) &&
    currentRegions.data.regions.some((r) => {
      const name = (r as { name?: string }).name;
      return (
        typeof name === "string" &&
        (provider === "azure" ? name.toLowerCase() : name) === requestRegion
      );
    });
  const sizeKey = JSON.stringify([accountKey, requestRegion]);
  useEffect(() => {
    const controller = new AbortController();
    let active = true;
    if (provider !== "azure" || !regionReady) return;
    setSizes({ key: sizeKey, data: {}, loading: true, error: "" });
    void fetchCatalog(
      api,
      `/accounts/${accountId}/sizes?region=${encodeURIComponent(requestRegion)}`,
      controller,
    )
      .then((data) => {
        if (active) setSizes({ key: sizeKey, data, loading: false, error: "" });
      })
      .catch((e) => {
        if (active)
          setSizes({
            key: sizeKey,
            data: {},
            loading: false,
            error: String(e),
          });
      });
    return () => {
      active = false;
      controller.abort();
    };
  }, [api, sizeKey, regionReady, provider]);
  const currentSizes: Pick<Snapshot, "data" | "loading" | "error"> =
    sizes.key === sizeKey
      ? sizes
      : { data: {}, loading: regionReady, error: "" };
  return {
    catalog:
      provider === "azure"
        ? {
            regions: currentRegions.data.regions || [],
            sizes: regionReady ? currentSizes.data.sizes || [] : [],
          }
        : currentRegions.data,
    loading:
      currentRegions.loading ||
      (provider === "azure" && regionReady && currentSizes.loading),
    regionsLoading: currentRegions.loading,
    error:
      currentRegions.error || (provider === "azure" ? currentSizes.error : ""),
    retry: () => setAttempt((n) => n + 1),
  };
}
