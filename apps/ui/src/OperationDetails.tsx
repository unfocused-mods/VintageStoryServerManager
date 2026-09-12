import { useEffect, useState } from "react";
import { Api, type Operation } from "./api";
interface Log {
  available: boolean;
  entries: {
    seq: number;
    at: string;
    level: string;
    stage: string;
    message: string;
  }[];
  summary: string;
  retention: { days: number; max_entries: number };
}
export default function OperationDetails({
  api,
  operation,
}: {
  api: Api;
  operation: Operation;
}) {
  const [open, setOpen] = useState(false),
    [log, setLog] = useState<Log | null>(null),
    [error, setError] = useState(""),
    [loading, setLoading] = useState(false),
    [attempt, setAttempt] = useState(0),
    [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!open) return;
    let active = true;
    let next: ReturnType<typeof setTimeout> | undefined;
    let controller: AbortController | undefined;
    const refresh = async () => {
      setLoading(true);
      controller = new AbortController();
      let timeout: ReturnType<typeof setTimeout> | undefined;
      try {
        const value = await Promise.race([
          api.request<Log>(
            `/operations/${operation.id}/logs`,
            "GET",
            undefined,
            controller.signal,
          ),
          new Promise<never>((_, reject) => {
            timeout = setTimeout(() => {
              reject(new Error("Loading operation details timed out."));
              controller?.abort();
            }, 15000);
          }),
        ]);
        if (active) {
          setLog(value);
          setError("");
        }
      } catch (e) {
        if (active) setError(String(e));
      } finally {
        clearTimeout(timeout);
        if (active) {
          setLoading(false);
          if (["queued", "running"].includes(operation.status))
            next = setTimeout(() => void refresh(), 3000);
        }
      }
    };
    void refresh();
    return () => {
      active = false;
      clearTimeout(next);
      controller?.abort();
    };
  }, [api, operation.id, operation.status, open, attempt]);
  const text = log
    ? [
        `${operation.action} — ${operation.status}`,
        log.summary,
        ...log.entries.map(
          (entry) =>
            `${entry.at} [${entry.level}] ${entry.stage}: ${entry.message}`,
        ),
      ].join("\n")
    : "";
  return (
    <details
      className="operation-details"
      onToggle={(e) => setOpen(e.currentTarget.open)}
    >
      <summary>Operation details</summary>
      {open && (
        <>
          {loading && !log && <p role="status">Loading details…</p>}
          {error && (
            <p className="inline-error" role="alert">
              {error}{" "}
              <button
                type="button"
                className="text-button"
                disabled={loading}
                onClick={() => setAttempt((n) => n + 1)}
              >
                Retry
              </button>
            </p>
          )}
          {log && (
            <>
              {!log.available && (
                <p>
                  Detailed logs were not recorded for this operation. Only its
                  summary is available.
                </p>
              )}
              <pre
                className="operation-log"
                tabIndex={0}
                aria-label="Operation log"
              >
                {text}
              </pre>
              <div className="button-row">
                <button
                  type="button"
                  className="text-button"
                  onClick={async () => {
                    try {
                      await navigator.clipboard.writeText(text);
                      setCopied(true);
                    } catch {
                      setError(
                        "Could not copy the log. Use Download details instead.",
                      );
                    }
                  }}
                >
                  {copied ? "Copied" : "Copy details"}
                </button>
                <button
                  type="button"
                  className="text-button"
                  onClick={() => {
                    const url = URL.createObjectURL(
                      new Blob([text], { type: "text/plain;charset=utf-8" }),
                    );
                    const a = document.createElement("a");
                    a.href = url;
                    a.download = `operation-${operation.id}.log`;
                    a.click();
                    setTimeout(() => URL.revokeObjectURL(url), 1000);
                  }}
                >
                  Download details
                </button>
              </div>
            </>
          )}
        </>
      )}
    </details>
  );
}
