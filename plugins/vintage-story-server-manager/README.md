# Vintage Story Server Manager plugin

Provides the Manager's named administration tools over MCP stdio, plus a skill for administration and troubleshooting. The same operations and schemas are available through `vsm admin`; the Manager enforces authentication, roles, filesystem restrictions, mutation locks and durable activities.

## Desktop

Use the current `vsm` build and keep the desktop app open. The source `.mcp.json` runs `vsm --desktop mcp`, so `vsm` must be on PATH. Desktop discovery is explicit and uses connection metadata created by current desktop builds; it does not guess a port or use another inventory.

For a self-contained local installation, run the packaging helper with the executable for the local platform:

```text
python scripts/package.py --binary /absolute/path/to/vsm --destination /absolute/path/to/personal/plugin
```

On Windows use `vsm.exe`. The helper copies the executable into the destination and writes its absolute path to that installation's MCP config, so PATH setup is unnecessary. Rerun it after moving the installation. It never copies credentials or Manager data. A package built this way is specific to the chosen machine and operating system; prepare a fresh package for each target. The repository source remains portable.

## Hosted Manager and other MCP clients

Configure your client with an absolute `vsm` executable path, arguments `["mcp"]`, and environment variables `VSM_URL` and `VSM_TOKEN_FILE`. Keep the token in a protected file on the client machine. Use a scoped CLI token appropriate for the servers and role needed. Do not combine those variables with `--desktop`: ambiguous destinations are rejected. HTTPS is required for non-loopback endpoints; use an authenticated local tunnel if needed.

Generic stdio MCP clients can run the same command without installing the Codex skill. The executable serves only MCP protocol messages on stdout. No new public listening port is created. Start a fresh Codex task after installation so the tools and skill are picked up.

For command examples, operation semantics and exit codes, see `docs/administration.md` in the project.
