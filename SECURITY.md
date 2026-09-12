# Security

This is pre-release software. Public hosting and sensitive production worlds require review of the limitations in `docs/status.md`.

Report security vulnerabilities through [GitHub's private vulnerability reporting form](https://github.com/cjohnsto-nz/VintageStoryServerManager/security/advisories/new). Do not put credentials, private keys, tokens or exploitable vulnerability details in public issues. Remove secrets from reports and attachments before sharing them, including private reports.

## Trust boundaries

- Cloud credentials stay in the controller and use OS keyring, environment, or encrypted-vault references.
- The controller binds to loopback by default. Public deployment requires a TLS reverse proxy and TOTP-enrolled write users.
- Agents bind to loopback and use an authenticated SSH tunnel. SSH host keys are checked strictly.
- Game binaries and mods are executable third-party code. Install only trusted releases.
- The supplied Linux service runs the supervisor as root with a restricted systemd capability set and launches the game as a separate unprivileged user with no capabilities. Agent tokens and backup credentials remain outside the game account. This is not a hostile-mod or multi-tenant sandbox.
- Recovery copies are local unless off-host backup is enabled. Keep encryption keys outside the server.
- Protect controller and agent data directories with filesystem permissions. Do not expose database files through web hosting.

The project does not collect telemetry or automatically update game/mod code.
