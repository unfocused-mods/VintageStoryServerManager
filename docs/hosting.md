# Optional hosted dashboard

The hosted dashboard serves the same UI and API as the desktop controller. Use one authoritative controller for each inventory; desktop and CLI clients can connect to it remotely.

## Domain and HTTPS

`deploy/compose.yaml` builds the controller and runs Caddy. Point your domain at the host, allow inbound TCP 80/443, set `VSM_DOMAIN`, and run:

```sh
docker compose -f deploy/compose.yaml up -d --build
```

Use a managed identity on an Azure-hosted controller or service-principal credentials in an encrypted vault/environment elsewhere. Do not rely on a developer's interactive Azure CLI cache for unattended hosting. Mount SSH keys and known_hosts read-only, and use their **container** paths when creating servers.

The public controller refuses the filesystem bootstrap token. Create the initial owner from a private JSON input using `vsm user-add` inside the controller container:

```sh
docker compose -f deploy/compose.yaml exec -T controller vsm user-add --data /var/lib/vsm < owner.json
```

The input object contains `name`, `password` (at least 12 characters), `role: "owner"`, `servers: []`, and a base32 `totp_secret` enrolled in your authenticator. Keep this input private, remove it after enrollment, and log in through the HTTPS dashboard. There is currently no recovery-code or self-service password-reset flow; retain secure local controller access for recovery.

## No domain

Run the controller bound to loopback on your host and tunnel it over SSH:

```sh
ssh -N -L 4317:127.0.0.1:4317 user@controller-host
```

Visit http://127.0.0.1:4317 locally. This does not expose an unauthenticated public dashboard. Existing private-network access can be used with an appropriate TLS reverse proxy as well.

## Encrypted hosted credentials

Set `VSM_VAULT_FILE` to a protected vault path and `VSM_VAULT_KEY` to a separate strong passphrase. Pipe a secret into `vsm secret-set NAME --vault`, then use `vault:NAME` in the provider account. Vault values are protected with Argon2-derived AES-256-GCM encryption. Keep the vault key outside the encrypted vault and outside game processes.

OS credential storage uses `vsm secret-set NAME` and `keyring:NAME`; environment references use `env:NAME`. Secret input is read from stdin, not command-line arguments.

## Moving a controller

POST `/api/v1/recovery/export` with a strong passphrase. The response is an encrypted inventory file encoded as base64. Stop the source controller, then POST `/api/v1/recovery/import` to an empty target with the file, passphrase, and `source_stopped: true`.

Transfer provider credentials, SSH keys, known_hosts, and backup keys separately. Users and sessions are intentionally not included in this inventory export. Keep a separate protected backup of the stopped controller data directory if you need to recover users and audit history.

Never delete a source game server during migration until a restore on the destination has passed operational checks. Cross-provider world migration currently uses these backup/restore tools rather than an automated cutover wizard.

## Game downloads

Version selection uses the official public stable server catalog; no Vintage Story account login is required. `vsm versions --json` and `GET /api/v1/game/releases` list supported Linux x64 releases. Stable 1.21 and 1.22 use the bundled .NET 8 and .NET 10 runtimes. New deployments select an exact release; updates remain explicit. Existing cached releases can be restored offline. See [agent installation and integrity checks](agent.md) for the download and checksum policy.

## Azure idle compute suspension

Game sleep and VM suspension are separate controls. Game sleep keeps the host running and can wake on a game-port connection. Azure compute suspension deallocates the VM, which stops compute charges and also stops its agent and game-port listeners. Both policies are disabled by default. Automatic player detection is verified for Vintage Story 1.22.0 and 1.22.7 only; unknown versions or response formats cannot trigger deallocation.

Azure's `Stopped`/allocated state still incurs compute charges. Use deallocation and verify the provider reports `Deallocated`. Managed disks, public IP addresses and other retained resources can continue to cost money. Retail estimates exclude account-specific discounts and taxes. See [Azure VM billing states](https://learn.microsoft.com/en-us/azure/virtual-machines/states-billing).

The Manager performing suspension must run **outside the VM being suspended** and remain available. The desktop's built-in Manager runs only while the desktop application is open; it does not become an unattended hosted service. A dashboard installed on the game VM cannot wake that same VM after deallocation. For unattended operation, host the Manager separately and use Azure managed identity or an explicitly configured service principal with access to that VM. Keep these credentials outside the game process.

Before enabling compute suspension:

1. Confirm the game server is managed by the separate Manager and can be started through its Azure account.
2. Enable and test idle detection with the game running.
3. Enable the separate Azure compute-suspension policy after reviewing retained charges and the external-Manager requirement.
4. Keep the Manager available so it can observe idle state and finish the recorded deallocation operation.

The Manager rechecks the agent's sleep generation under its mutation lock and disarms local game-port wake before deallocating. This avoids acting on an old sleep report after a player has already woken the game. Operations are recorded and provider state must be verified; an interrupted or failed operation is not reported as a successful suspension.

Wake a deallocated VM from the external Manager or CLI. Waking requires Azure VM start, agent connectivity, game start and a game health check. A player connecting directly to the deallocated VM's old game address cannot wake it. Automatic game-connection wake would require a separate always-on gateway; a gateway is not included in this policy.

### Policy and wake commands

Read the current policy:

```sh
vsm request GET /servers/SERVER_ID/cloud-idle-policy
```

Save a JSON file containing:

```json
{"enabled":true,"minutes":15,"external_manager_confirmed":true}
```

Only set `external_manager_confirmed` after verifying the Manager is outside the target VM. Save with:

```sh
vsm request PUT /servers/SERVER_ID/cloud-idle-policy --body cloud-idle-policy.json
```

This also enables the agent's idle-game policy with the same timeout. The response includes `last_error` and `wake_pending`; these are operational status, not editable settings. Disabling the cloud policy stops new deallocation decisions and attempts to disable the remote idle-game policy. If contact is attempted and fails, the response reports that the remote policy could not be changed. A deallocated VM retains its agent settings until it is reachable again; the Manager applies the current policy before resuming the game.

For a VM suspended by this policy, use **Start VM** in Infrastructure, or:

```sh
vsm action SERVER_ID cloud-start --wait
```

The recorded wake intent tells the Manager to resume the game after the VM starts. Successful completion requires game readiness. On an uncertain ARM failure the Manager disables automatic deallocation and retains wake intent for recovery. Inspect Infrastructure and Activity, then use Start VM after resolving credentials, availability or connectivity. A failed wake may leave an allocated VM incurring charges; inspect the provider state before retrying.

### Other providers

| Host type | Does stopping the game or powering off the host stop compute billing? |
| --- | --- |
| Azure VM | Only VM deallocation stops compute billing; retained resources still cost money. |
| Hetzner Cloud | No. Servers remain billable until deleted. |
| DigitalOcean Droplet | No. Powered-off Droplets remain billable until destroyed. |
| Vultr instance | No. Stopped instances remain billable until destroyed. |
| Local Docker / existing SSH host | The app cannot determine or change the host's electricity or hosting bill. |

These policies follow [Hetzner's billing FAQ](https://docs.hetzner.com/cloud/billing/faq/), [DigitalOcean Droplet pricing](https://docs.digitalocean.com/products/droplets/details/pricing/) and [Vultr's stopped-instance billing policy](https://docs.vultr.com/support/platform/billing/are-stopped-instances-still-billed-on-vultr). The app does not automatically destroy and recreate servers to avoid these charges. Snapshot-and-recreate workflows would need separate recovery, addressing and capacity guarantees.
