# Vintage Story Server Manager

An open-source desktop app, CLI, web dashboard and Linux agent for running your own Vintage Story servers. Deploy to Azure, Hetzner, DigitalOcean or Vultr, connect an existing Linux host over SSH, or use local Docker or Docker over SSH.

**Development alpha.** Windows desktop installation and live Azure management have been tested, including Vintage Story 1.22.7 startup, restart, mod installation and VM deallocation. This is not a claim that every provider, platform or recovery workflow has passed acceptance testing. See [status and limitations](docs/status.md).

## Cloud costs and responsibility

**This is server-management software, not a hosting service. You use your own cloud accounts and pay your providers directly.** The project does not sell hosting, provide cloud accounts, handle provider billing or act as your cloud provider's agent.

You are responsible for **all costs incurred through your use of this tool**, including resources created by a failed, interrupted or repeated deployment. You are also responsible for your cloud-provider agreements, subscriptions, commitments, acceptable-use requirements and other obligations. The project and its contributors are not parties to those agreements and do not assume your payment or contractual obligations.

- Estimates are informational, can be incomplete or stale, and are not quotes, spending limits or guarantees of savings. Actual charges depend on your provider, account terms, usage, discounts, currency, taxes and fees.
- Stopping the game does not stop VM billing. Azure deallocation stops compute billing for the VM, but managed disks, public IP addresses, backups and other retained resources can still incur charges. Network usage and other services may be billed separately. Other providers may charge for stopped VMs.
- Closing or uninstalling the app, removing a cloud account, or losing the Manager's configuration does not delete your cloud resources or cancel provider agreements.
- Monitor billing in your provider's portal, configure provider budgets and alerts, and verify cleanup there. You are responsible for deleting resources you no longer need. Ownership checks deliberately leave unowned or ambiguous resources untouched; Azure cleanup retains the resource group.

Except where required by applicable law or agreed in writing, the project and its contributors accept no liability for cloud charges, financial losses or obligations arising from your use of this software or your agreements with a cloud provider. The software is provided **“AS IS,” without warranties**, under the [Apache License 2.0](LICENSE), including its warranty disclaimer and limitation of liability in sections 7 and 8.

## Desktop app

Download the Windows x64 desktop installer from [Releases](https://github.com/cjohnsto-nz/VintageStoryServerManager/releases). The initial alpha also includes the Windows CLI and Linux x64 Remote agent. macOS Intel/Apple Silicon and Linux x64 desktop packages can be built using the manual **Prepare installers** workflow; they have not yet passed installed-platform acceptance testing. See [installer formats and checksum verification](docs/installers.md).

These alpha packages are unsigned or ad-hoc signed, and macOS packages are not notarized. Operating-system security prompts are expected. Checksums verify download integrity; they are not publisher signatures.

The desktop app starts its local management service automatically and keeps the server inventory in your OS application-data directory. No separate service or local access token is needed.

Choose **Create server**, then select local Docker, Remote over SSH, Docker over SSH, or a cloud deployment. Follow the setup steps and review the resource plan before applying it. Cloud deployment creates billable resources. For an SSH or cloud host, verify its SSH fingerprint, then choose **Install Remote and game**. Start the game from **Overview**, check its status and join address, and configure player access.

Game hosts must be Linux x64. SSH hosts use Ubuntu 24.04; Docker requires a Linux x64 engine. Local Docker on Windows or macOS requires Docker Desktop running Linux containers. Keep your Manager recovery export and SSH keys backed up separately from the game host.

To run the desktop app from source, install [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/), Rust, Node.js 22+, and OpenSSH:

```sh
git clone https://github.com/cjohnsto-nz/VintageStoryServerManager.git
cd VintageStoryServerManager
npm ci
npm run desktop
```

## Browser development

Requires Rust, Node.js 22+, and OpenSSH. On Linux, install `libdbus-1-dev` and `pkg-config`.

```sh
npm ci
npm run build
cargo build -p vsm --bins
cargo run -p vsm --bin vsm -- serve
```

Open **http://127.0.0.1:4317** and connect to the local service using the token from `.vsm/owner.token`. Credentials stay in the browser's memory, not localStorage. This manual setup is for browser development; the desktop app handles it automatically.

For frontend development, run the controller and `npm run dev` in separate terminals. The Vite frontend runs at http://127.0.0.1:1420 and proxies the API to the controller.

### Azure

1. Open **Cloud accounts**, choose Azure, and use **Sign in to Azure** in the desktop app. If Azure CLI is missing, use the installation link, then retry sign-in.
2. Choose your subscription and save the account. Hosted services can use managed identity in Azure or a service principal elsewhere.
3. Use **Compare costs** to review live compute prices, additional-cost allowances, resource requirements, and imported latency measurements.
4. Choose **Create server**, select a region and size, create a server access key in the desktop app, and enter a restricted administrator source CIDR. Review the resource plan.
5. Apply the plan. **This creates billable resources.** Incomplete estimates require explicit acknowledgment.
6. Follow the server setup steps to compare the SSH fingerprint with a trusted provider source and trust the host.
7. Choose **Install Remote and game**, then start the game. Desktop installers include the required Linux agent. See [agent installation](docs/agent.md) for source builds and hosted services.

Azure creates a dedicated resource group, VM, 64 GiB Standard SSD managed OS disk, VNet/subnet, NSG, NIC, and Standard static IPv4. Cleanup verifies ownership and removes individual resources; it retains the resource group. Deallocation is distinct from stopping the game, and storage/IP charges may remain.

Optional **reduce cost when empty** uses a Manager outside the game VM to deallocate it after inactivity. The desktop app must remain open, or a separately hosted Manager must remain available. Joining a deallocated VM does not wake it; start the VM in the Manager and, if needed, start the game. Game sleep alone leaves the VM allocated and does not reduce compute charges.

Examples are in `examples/azure-account.json` and `examples/server.json`. Replace **all** placeholder values before use; the example size is illustrative, not a recommendation for every community.

## CLI and MCP

Extract the CLI archive for your platform. With the desktop app open, `--desktop` connects to its Local Manager and existing inventory:

```sh
vsm --desktop servers --json
vsm --desktop start SERVER_ID --wait
vsm --desktop restart SERVER_ID --wait
vsm --desktop mods list SERVER_ID
vsm --desktop whitelist SERVER_ID PlayerName --wait
vsm --desktop activity
vsm --desktop activity OPERATION_ID --logs
vsm completions powershell
```

The CLI and MCP share 77 named administration tools for lifecycle, mods, players, configuration, files, worlds, backups, infrastructure and diagnostics. Use the catalog for additional workflows:

```sh
vsm admin list --json
vsm admin schema mod_upload --json
vsm --desktop admin run mod_upload --file upload.json --wait --json
vsm --desktop mcp
```

`vsm --desktop mcp` provides a stdio MCP server for compatible clients. A [Codex plugin and administration skill](plugins/vintage-story-server-manager/README.md) are included in the repository. CLI/MCP clients use the Manager's authentication and safety checks; they do not create a separate inventory.

For a hosted Manager, use `VSM_URL` and `VSM_TOKEN_FILE` or `--url` and `--token-file` instead of `--desktop`. Non-loopback connections require HTTPS. Keep token values out of command arguments and shared configuration. See [administration documentation](docs/administration.md) for schemas, stable exit codes and operation tracking. An accepted operation is not completed work: use `--wait`, inspect activity logs and verify the resulting state.

`vsm plan examples/server.json` and `vsm apply PLAN_ID` provide deployment plan/apply workflows. Apply requires interactive confirmation unless `--yes` is specified. Default builds and tests do not provision cloud resources; live cloud tests require an explicitly supplied account and incur costs.

`vsm probe HOST --quote-id azure:australiaeast:Standard_D4s_v5 --location Auckland --players 4` measures TCP connection latency from the current computer. Import its JSON output into the advisor. It is a network estimate, not a measurement of in-game tick performance.

## Management

- Start, stop, restart, console, bounded crash recovery, and host-side daily restart schedules.
- Searchable, sanitized console history with rotation and downloads.
- Configuration editor with common settings controls, preserving unknown JSON fields.
- Whitelist commands, connected-player listing, and announcements.
- ModDB search, exact release installation, uploaded mod ZIPs, dependency checks, enable/disable/remove, and manifests.
- Confined file browsing/editing, binary upload, rename, directory creation, safe ZIP extraction, trash, and world switching.
- Local recovery copies and optional scheduled encrypted restic backups, repository checks, remote snapshot restore, and retention.
- Owner/admin/viewer roles, TOTP-gated public write access, token revocation, OS keyring/environment/encrypted-vault credential references.
- Encrypted inventory export/import with explicit controller ownership transfer.

Game and mod updates are explicit. There is no mandatory telemetry. Native mods execute code on your host; install only mods you trust. Local recovery copies do not protect against host or disk loss: configure off-host backups for worlds you want to keep.

## Verification and hosting

```sh
cargo test -p vsm
cargo clippy -p vsm --all-targets -- -D warnings
npm test
npm run build
npm run typegen
```

[Hosted dashboard](docs/hosting.md) · [Agent setup and backups](docs/agent.md) · [Architecture](docs/architecture.md) · [Status and limitations](docs/status.md) · [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md)

Licensed under [Apache-2.0](LICENSE). Dependencies retain their own licenses; see [third-party notices](THIRD-PARTY-NOTICES.txt). This is an independent community project, not an official Anego Studios product. Game binaries and mods are downloaded from their respective sources and are not included in the source repository.
