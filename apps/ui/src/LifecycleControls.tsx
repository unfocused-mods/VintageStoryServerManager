import { Loader2, Play, RefreshCw, Square } from "lucide-react";
export default function LifecycleControls({
  state,
  busy,
  act,
  setup,
}: {
  state: string;
  busy: boolean;
  act: (action: string) => void;
  setup: () => void;
}) {
  if (state === "deleted") return null;
  if (state === "recovered") return <button className="button primary" onClick={setup}>Reconnect Remote</button>;
  if (state === "awaiting_bootstrap")
    return (
      <button className="button primary" onClick={setup}>
        Finish server setup
      </button>
    );
  const progress: Record<string, string> = {
    provisioning: "Preparing server…",
    starting: "Starting server…",
    stopping: "Stopping server…",
    restarting: "Restarting server…",
  };
  if (progress[state])
    return (
      <button className="button secondary" disabled>
        <Loader2 className="spin" size={15} />
        {progress[state]}
      </button>
    );
  if (state === "running")
    return (
      <>
        <button
          className="button secondary"
          disabled={busy}
          onClick={() => act("stop")}
        >
          <Square size={14} />
          Stop
        </button>
        <button
          className="button secondary"
          disabled={busy}
          onClick={() => act("restart")}
        >
          <RefreshCw size={14} />
          Restart
        </button>
      </>
    );
  if (["stopped", "sleeping"].includes(state))
    return (
      <button
        className="button primary"
        disabled={busy}
        onClick={() => act("start")}
      >
        <Play size={15} />
        Start server
      </button>
    );
  if (state === "deallocated")
    return (
      <button
        className="button primary"
        disabled={busy}
        onClick={() => act("cloud-start")}
      >
        <Play size={15} />
        Start VM
      </button>
    );
  return (
    <button className="button secondary" onClick={setup}>
      Check server setup
    </button>
  );
}
