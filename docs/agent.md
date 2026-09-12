# Linux agent and game runtime

New cloud hosts use Ubuntu 24.04 LTS x64. The controller and game host are different execution environments; a Windows `.exe` cannot be installed as the Linux agent.

Desktop [installers](installers.md) include the Linux agent and copy it into the app's data directory automatically. The build steps below are for source builds and independently hosted management services.

## Build the Linux artifact

```sh
docker build --target builder -t vsm-builder .
docker create --name vsm-artifacts vsm-builder
docker cp vsm-artifacts:/src/target/release/vsm-agent ./vsm-agent
docker rm vsm-artifacts
```

Place the executable at `CONTROLLER_DATA/artifacts/vsm-agent`, or set `VSM_AGENT_BINARY` to its absolute path before starting the controller. A Linux artifact built during development is also available in `.vsm/artifacts` when the local smoke workflow has been run.

Verify the host key against the provider console before accepting it into OpenSSH's `known_hosts`. Bootstrap intentionally uses `StrictHostKeyChecking=yes`. The selected SSH user must have passwordless sudo for installation.

Bootstrap uploads the agent, installs .NET 8 and 10 from Microsoft's Ubuntu package source, installs restic, and enables `vsm-agent.service`. The agent runs as root inside a restricted systemd service. Before launch it drops the game process to the separate `vintagestory` account with no capabilities or supplementary groups, with cloud credentials excluded from its environment. The private agent token and recovery directory remain root-owned. The agent listens on 127.0.0.1:4318 and is reached using authenticated SSH tunnels. Do not expose this port publicly.

Available stable Linux x64 game versions come from the [official public download feed](https://api.vintagestory.at/stable.json); downloading a dedicated server does not require a game account login. Fresh downloads must match the feed's canonical CDN URL and publisher MD5 before extraction. MD5 provides archive integrity checking, not a cryptographic publisher signature. Downloads are verified before a running world is stopped for an update. The catalog supports stable 1.21 (.NET 8) and 1.22 (.NET 10) releases; unfamiliar runtime requirements remain unavailable until supported. On 6 September 2026 the feed contained 16 supported Linux server downloads, with 1.22.7 the latest. This count is a dated check, not a fixed catalog.

Use `vsm versions --json` or public `GET /api/v1/game/releases` to inspect available releases. Deployment plans pin an exact version and validate it before provisioning. New installations suggest the latest supported release; existing servers never update automatically. Previously cached game releases remain usable for offline restores, without being reported as freshly checksum-verified.

## Data layout

```text
/var/lib/vsm/
  agent.token          private management token
  version              explicitly selected game release
  desired              desired process state across agent restarts
  releases/            game runtimes downloaded from the official CDN
  data/                serverconfig.json, Saves, Mods, Logs, etc.
  mods.json            exact installed mod manifest
  recovery/            latest three complete local recovery points
  trash/               explicitly removed files/mods
  backup.json          optional repository policy (no plaintext password)
  schedule.json        host-side daily restart schedule
```

Full recovery copies require a stopped server and sufficient free disk. The default maximum source size is 20 GiB; set `VSM_RECOVERY_LIMIT_GIB` in the systemd environment to increase it. Interrupted staging directories are not advertised as valid recovery points. Trash and failed staging directories are retained for manual inspection and can consume disk.

## Optional off-host backups

Configure storage credentials and a repository password **on the agent host**, using a protected systemd EnvironmentFile. Restart the agent after changing its environment. The game subprocess does not inherit these variables.

Example variable names:

```text
RESTIC_PASSWORD=<a strong unique backup password>
AWS_ACCESS_KEY_ID=<S3-compatible storage access key>
AWS_SECRET_ACCESS_KEY=<S3-compatible storage secret>
AWS_DEFAULT_REGION=<storage region>
```

In Backups, set a repository such as `s3:https://storage.example.com/bucket`, keep the policy disabled, save, and initialize the repository. Then enable the daily schedule. Defaults are 03:00 UTC, seven daily copies, and four weekly copies. Repository passwords and cloud storage keys are never collected by the game configuration editor.

The backup workflow stops the game, stages a complete recovery point, restarts the game if it was running, then uploads using restic. Failed backup attempts are recorded in agent logs and retried at most once per 15 minutes during the selected hour. Snapshots carry a persistent per-agent identity tag; retention only touches that agentâ€™s tagged snapshots, even when several servers share a repository. Retention removes snapshot references; run `restic prune` separately during a maintenance window to reclaim repository storage.

Restores download into a staging directory first and retain current data before switching. Store the repository password and cloud credentials outside the game host: losing them can make encrypted backups unrecoverable.

## Local real-game smoke test

```sh
docker build -t vsm:local .
docker run -d --name vsm-agent-smoke --entrypoint vsm-agent vsm:local --root /var/lib/vsm
python scripts/smoke-agent.py vsm-agent-smoke
docker stop vsm-agent-smoke
docker rm vsm-agent-smoke
```

This downloads Vintage Story 1.22.0 into a disposable Linux container and checks startup, console submission, local backup, graceful stop, restore, game-token isolation, and encrypted restic repository backup/check/restore. No ports are published and no paid cloud resources are used. It is not a multiplayer client-join test.

## Optional idle game sleep

Idle game sleep is disabled by default. When enabled, the agent waits for the configured idle period (15 minutes by default; 1â€“1440 minutes supported), confirms an empty player list and no active game TCP connections, saves the world, and stops the game process. The agent keeps the game's TCP and UDP ports available for wake requests. A new connection starts the game; the player may need to retry joining after startup.

This releases game CPU and memory use. It does **not** stop a cloud VM or reduce its compute bill. Azure compute suspension is a separate, explicit policy described in [hosted management](hosting.md#azure-idle-compute-suspension).

A manual Stop disarms wake listeners. Start enables normal operation again; changing configuration or preparing cloud suspension also releases wake ports. Unknown player-list formats, unreadable socket activity, or missing listener information pause automatic sleep rather than assuming the server is empty. Automatic player detection is enabled for Vintage Story 1.22.0 and 1.22.7, whose unmodified English console formats have been checked. Other versions and unknown formats pause automatic sleep; this is not a general player-count API.

The agent stores its settings in `/var/lib/vsm/idle-policy.json`. Through the Manager API:

```sh
vsm request GET /servers/SERVER_ID/agent/idle-policy
vsm request PUT /servers/SERVER_ID/agent/idle-policy --body idle-policy.json
```

Example `idle-policy.json`:

```json
{"enabled":true,"minutes":15}
```

Set `enabled` to `false` to disable automatic sleep. Status includes the saved policy, sleep state, elapsed idle time and any monitoring error. The agent handles this policy while the desktop is closed, provided the game host and agent remain running.

Scheduled backups can still create a consistent copy while the game is sleeping. Once the entire VM is deallocated, its agent, backups and restart schedules cannot run until the VM starts again; wake the VM through an external Manager before the required backup window.
