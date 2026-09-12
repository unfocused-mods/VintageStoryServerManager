import { useEffect, useState } from "react";
import { Api } from "./api";

export interface InfrastructureStatus {
  power_state: string | null;
  provisioning_state: string | null;
  checked_at: string;
  remote_installed?: boolean | null;
  error?: string | null;
}

export function useInfrastructureStatus(
  api: Api,
  serverId: string,
  enabled: boolean,
  serverState: string,
) {
  const [status, setStatus] = useState<InfrastructureStatus | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    let next: ReturnType<typeof setTimeout> | undefined;
    let controller: AbortController | undefined;
    setStatus(null);
    setError("");
    setLoading(enabled);
    if (!enabled) return;
    const refresh = async () => {
      controller = new AbortController();
      let timeout: ReturnType<typeof setTimeout> | undefined;
      setLoading(true);
      try {
        const value = await Promise.race([
          api.request<InfrastructureStatus>(
            `/servers/${serverId}/infrastructure/status`,
            "GET",
            undefined,
            controller.signal,
          ),
          new Promise<never>((_, reject) => {
            timeout = setTimeout(() => {
              reject(
                new Error(
                  "Checking VM status timed out. Retry to get its current state.",
                ),
              );
              controller?.abort();
            }, 30000);
          }),
        ]);
        if (active) {
          setStatus(value);
          setError(value.error || "");
        }
      } catch (e) {
        if (active) {
          setStatus(null);
          setError(String(e));
        }
      } finally {
        clearTimeout(timeout);
        if (active) {
          setLoading(false);
          next = setTimeout(() => void refresh(), 30000);
        }
      }
    };
    void refresh();
    return () => {
      active = false;
      clearTimeout(next);
      controller?.abort();
    };
  }, [api, serverId, enabled, serverState, attempt]);
  return { status, error, loading, refresh: () => setAttempt((n) => n + 1) };
}
