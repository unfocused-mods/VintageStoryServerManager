# Architecture

The Rust `vsm` crate contains the domain contracts, recommendation engine, SQLite operation store, authentication, provider adapters, controller service, SSH transport, and agent. `vsm` and `vsm-agent` are separate binaries; the Tauri application embeds the controller library and passes a local, in-memory connection to the shared React application.

## Control flow

1. An owner adds a provider account using a credential reference.
2. Planning validates the specification and retrieves live provider data. Azure additionally checks registration, region/SKU restrictions, and quota.
3. Applying a short-lived plan creates a durable operation and server inventory entry before calling a provider.
4. Resource identifiers are persisted during provisioning. Interrupted operations become `interrupted`, never falsely successful or silently replayed.
5. An owner verifies the SSH host key, supplies a Linux agent artifact, and bootstraps the host.
6. The controller creates short-lived SSH tunnels to the authenticated loopback agent. The agent owns game execution and schedules.

## State and credentials

An exclusive process lock allows only one controller to own an inventory. Encrypted export/import requires an empty target and acknowledgment that the source is stopped. Exports include inventory, not cloud credentials, user sessions, SSH keys, or off-host backup passwords.

Cloud API credentials use `keyring:NAME`, `env:NAME`, or encrypted `vault:NAME`. Azure additionally supports CLI identity, VM managed identity, and service-principal OAuth. No cloud secret is included in cloud-init or inherited by the game process.

## APIs

Existing tagged Azure deployments can also be [rediscovered and adopted](recovery.md) after losing an inventory or moving to a new machine. Adoption reconstructs Azure resource ownership metadata without reinstalling the Remote. SSH keys and Manager history are not recovered from Azure.

`/api/v1` is authenticated using bearer tokens. OpenAPI components are derived from Rust types; route descriptions and generated TypeScript declarations are under `docs/openapi.json` and `apps/ui/src/schema.d.ts`.

Server actions return operation IDs. Poll `/operations/{id}` or use `/events` with `Last-Event-ID`. Durable actions and deployment apply require an `Idempotency-Key`; direct agent configuration/file writes use agent serialization and file etags. Agent file endpoints are JSON/base64 and currently limit transfers to 64 MiB.

## Boundaries

Provider acceptance and observed completion are different states. Game launch is reported as `starting`, not a verified player join. Costs with missing meters are incomplete and cannot win automatic cost ranking. Existing SSH hosts have no discoverable provider bill.

See `status.md` for the release gates that remain before stable community use.
