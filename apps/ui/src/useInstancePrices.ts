import { useEffect, useState } from "react";
import { Api } from "./api";
export function useInstancePrices(
  api: Api,
  accountId: string,
  region: string,
  sizeIds: string[],
  enabled: boolean,
): {
  prices: Record<string, { price: number; currency: string }>;
  errors: string[];
  loading: boolean;
} {
  const ids = [...new Set(sizeIds)].slice(0, 4);
  const key = JSON.stringify([api.url, accountId, region, enabled, ids]);
  const [state, setState] = useState<{
    key: string;
    prices: Record<string, { price: number; currency: string }>;
    errors: string[];
    loading: boolean;
  }>({ key: "", prices: {}, errors: [], loading: false });
  useEffect(() => {
    let active = true;
    let pending = ids.length;
    const controllers: AbortController[] = [];
    const timers: ReturnType<typeof setTimeout>[] = [];
    setState({
      key,
      prices: {},
      errors: [],
      loading: enabled && !!accountId && !!region && ids.length > 0,
    });
    if (!enabled || !accountId || !region || !ids.length) return;
    for (const id of ids) {
      const controller = new AbortController();
      controllers.push(controller);
      let timer: ReturnType<typeof setTimeout>;
      const timeout = new Promise<never>((_, reject) => {
        timer = setTimeout(() => {
          controller.abort();
          reject(new Error("Price lookup timed out"));
        }, 15000);
        timers.push(timer);
      });
      void Promise.race([
        api.request<{ monthly_compute: number; currency: string }>(
          `/accounts/${accountId}/price?region=${encodeURIComponent(region)}&size=${encodeURIComponent(id)}`,
          "GET",
          undefined,
          controller.signal,
        ),
        timeout,
      ])
        .then((quote) => {
          if (
            !Number.isFinite(quote.monthly_compute) ||
            quote.monthly_compute <= 0 ||
            !quote.currency
          )
            throw new Error("Price unavailable");
          if (active)
            setState((s) =>
              s.key === key
                ? {
                    ...s,
                    prices: {
                      ...s.prices,
                      [id]: {
                        price: quote.monthly_compute,
                        currency: quote.currency,
                      },
                    },
                  }
                : s,
            );
        })
        .catch(() => {
          if (active)
            setState((s) =>
              s.key === key ? { ...s, errors: [...s.errors, id] } : s,
            );
        })
        .finally(() => {
          clearTimeout(timer);
          pending--;
          if (active && pending === 0)
            setState((s) => (s.key === key ? { ...s, loading: false } : s));
        });
    }
    return () => {
      active = false;
      controllers.forEach((c) => c.abort());
      timers.forEach(clearTimeout);
    };
  }, [api, key]);
  return state.key === key
    ? state
    : {
        prices: {},
        errors: [],
        loading: enabled && !!accountId && !!region && ids.length > 0,
      };
}
