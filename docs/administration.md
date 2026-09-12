# Administration with MCP and the CLI



The `vsm` binary provides one administration catalog used by both the CLI and MCP. It calls the existing authenticated Manager REST API. It does not independently SSH into servers, store another inventory, or bypass roles, mutation locks, recovery checks or filesystem restrictions.



## Connect



For the desktop app, keep an updated desktop build open:



```sh

vsm --desktop servers --json

vsm --desktop status --json

vsm --desktop mcp

```



`--desktop` reads the app's connection descriptor and local token, verifies the running Manager instance, then uses its API. It never guesses the desktop port. Older desktop versions must be updated before they can publish this descriptor. `--desktop-data PATH` overrides the desktop data directory. Combining desktop discovery with an explicit URL or token is rejected to prevent targeting a different inventory accidentally.



For a hosted Manager, configure `VSM_URL` and `VSM_TOKEN_FILE` in the client environment, or pass `--url https://manager.example.com --token-file /path/to/token`. Use a scoped token; the token file lives on the CLI/MCP client machine. Do not place token values in commands, plugin manifests or shared logs. Non-loopback connections require HTTPS. An authenticated local tunnel can expose a hosted Manager on loopback HTTP.



For compatibility, running the CLI without an explicit URL or desktop mode can still use `.vsm/owner.token` against `http://127.0.0.1:4317`. This fallback is never used for an explicitly selected URL.



## CLI workflows



Global options work before or after subcommands. `--json` produces compact JSON on stdout; API and operation errors are JSON on stderr. Argument parsing retains standard CLI help/error output. `--idempotency-key KEY` deduplicates supported durable operations when retrying the same request after an uncertain response. Other endpoints, such as account creation and configuration writes, may not consume that key: inspect inventory/activity and the current state before retrying any mutation. Reuse a key only for the same request. Successful acceptance is not completion: use `--wait`, or inspect the returned operation ID. `--wait-timeout SECONDS` is bounded to 1-600 seconds, default 300. A timeout does not cancel the operation.



```sh

vsm --desktop start SERVER_ID --wait --json

vsm --desktop stop SERVER_ID --wait

vsm --desktop restart SERVER_ID --wait

vsm --desktop mods list SERVER_ID

vsm --desktop mods search vigor

vsm --desktop mods detail 4425

vsm --desktop mods install SERVER_ID 4425 55847 --wait

vsm --desktop whitelist SERVER_ID PlayerName --wait

vsm --desktop whitelist SERVER_ID PlayerName --remove --wait

vsm --desktop config SERVER_ID

vsm --desktop config SERVER_ID --file config-change.json

vsm --desktop files list SERVER_ID

vsm --desktop files read SERVER_ID serverconfig.json

vsm --desktop files write SERVER_ID file-change.json

vsm --desktop backups list SERVER_ID

vsm --desktop backups create SERVER_ID --wait

vsm --desktop activity

vsm --desktop activity OPERATION_ID --logs

vsm --desktop activity OPERATION_ID --wait --wait-timeout 120

```



Mod installs use an exact release ID. First inspect release metadata and choose the highest numeric version explicitly compatible with the installed game, not the latest publication timestamp. `--acknowledge-unknown-compatibility` is only for a deliberate install whose compatibility is unknown; it is not a general trust checkbox and does not make incompatible releases compatible.



Configuration writes use the API body `{ "value": { "ChangedSetting": "new value" }, "etag": "current tag" }`. Read configuration first and pass its current concurrency tag. Omitted fields are preserved. Passwords in reads are redacted; never submit `[redacted]` as a new password. File writes use `{ "path": "relative/path", "content_base64": "...", "etag": "current tag" }`; use the current file's tag for existing files. The Remote enforces safe paths, stopped-game requirements and concurrent edit protection.



All named tools are available without remembering endpoints:



```sh

vsm admin list --json

vsm admin schema world_create --json

vsm --desktop admin run world_options --file server.json --json

vsm --desktop admin run world_create --file new-world.json --wait --json

vsm --desktop admin run console_command --file command.json --wait --json

vsm --desktop admin run infrastructure_status --file server.json --json

```



`server.json` contains `{ "server_id": "SERVER_ID" }`. Other input files follow the tool schema. The catalog covers lifecycle, mod management, players/console, files, worlds, game/Remote installation and updates, backups, schedules, cost-saving policies, Azure discovery/adoption, infrastructure and deployment. For less common workflows, `api_schema` and `api_request` provide the versioned API contract and constrained HTTP access. Existing CLI commands remain available. Shell completion includes the new commands: `vsm completions powershell`, `bash`, `zsh`, or `fish`.



Exit codes:



| Code | Meaning |

| --- | --- |

| 0 | Request completed, or operation accepted when not waiting |

| 1 | Other API, response or local error |

| 2 | Usage, input or connection configuration error |

| 3 | Authentication or permission denied |

| 4 | Resource not found |

| 5 | Conflict, including concurrent mutation |

| 6 | Request or operation wait timed out; work may still be running |

| 7 | Transport/connection failure |

| 8 | Waited operation failed, was cancelled or interrupted |



A failed waited operation includes its operation object in the JSON error. Use `activity ID --logs` for detailed diagnostics. Success should be verified with a live status/read after the operation completes.



## MCP and the Codex plugin



The stdio entry point is `vsm mcp`, with the same connection options as the CLI. It serves protocol messages only on stdout and creates no additional public listening port. Clients discover the same named tools and JSON input schemas as `vsm admin list`. Mutations accept an `idempotency_key`; `operation_wait` bounds waiting, and operation/log tools allow resuming after disconnection. Tools return errors distinctly from successful results. Tool annotations describe read-only and potentially destructive behavior; the Manager remains the authority for permissions.



The repository plugin is in `plugins/vintage-story-server-manager`. Its source configuration expects `vsm` on PATH and uses `--desktop`. To avoid PATH setup, its `scripts/package.py` helper copies the chosen platform's executable into a local plugin installation and writes that absolute executable path. It does not copy tokens, keys or app data. A hosted MCP client instead uses arguments `["mcp"]` and the `VSM_URL`/`VSM_TOKEN_FILE` environment variables.



The plugin skill explains compatibility selection, operation tracking, safe editing and diagnostics. It treats server output, game chat and ModDB content as untrusted data. It does not require repeated confirmation for already authorized management tasks. Test destructive workflows on disposable servers; read-only checks are sufficient to verify a client connection.

