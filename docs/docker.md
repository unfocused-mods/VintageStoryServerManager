# Docker deployments

The desktop app includes the Linux x64 agent and builds its container image locally. No source checkout, prepublished VSM image, cloud account, or separately running manager is required.

Choose **Local Docker** for Docker Desktop with Linux x64 containers. Install and start Docker Desktop first. The selected Docker context must use a local Unix socket or Windows named pipe; remote Docker contexts are rejected. ARM Docker engines are not supported yet.

Choose **Docker over SSH** for an existing Linux x64 machine with Docker Engine. Supply its SSH address, username and private key. The app asks you to verify the SSH fingerprint before installing. The SSH user must already have permission to run Docker without an interactive sudo prompt. Docker installation on that host is currently a prerequisite.

The app builds the image using its bundled agent and official Microsoft .NET 8/10 runtime images. Internet access is required for the first image build and game download. A deployment initially installs the selected game release and leaves the game stopped; choose Start when ready.

## Storage and networking

Each server gets a container and named volume, both tagged with its deployment ID. Game files, configuration, mods and recovery copies persist in the volume. The agent runs inside the container; game processes use the unprivileged `vintagestory` account. The Docker socket is never mounted into the container.

Only the selected game port (default TCP/UDP 42420) is published. Choose a different host port for each server. Local join address is `127.0.0.1:PORT`; other players use the host's reachable network address. Your host firewall/router may require a rule for that game port. The agent API stays bound to loopback inside the container. Management uses `docker exec`; remote management carries these commands through authenticated SSH. No insecure Docker TCP endpoint or public management port is opened.

Deleting a server removes its container and retains its data volume. Select the delete-data checkbox in the deletion dialog, or explicitly send `delete_data: true` through the API/CLI, to also remove the owned volume. Ownership mismatches block mutation; partial deployment records remain available for inspection. Shared built images are retained. Docker restart policies restart the agent after Docker starts, and the agent runs configured schedules independently of the desktop.

## CLI

Add an account with `vsm account-add examples/docker-account.json`, then put the returned account ID in the deployment file. Run `vsm plan examples/docker-server.json`, review the plan, and `vsm apply PLAN_ID --yes`. Standard `vsm action SERVER_ID start --wait`, logs, files, mods and backup commands work unchanged.

For remote Docker, set the account provider to `ssh_docker`; populate `existing_host`, `ssh_user` and `ssh_private_key` in the deployment file. After confirming the SSH host fingerprint, use `vsm action SERVER_ID bootstrap --wait`. OpenSSH strict host-key checking remains enabled.

Game stop differs from stopping the container. Infrastructure start/restart/deallocate actions start/restart/stop the container. Resize is unavailable; resource-limit controls are deferred. Off-host backups still require a configured repository and secret on the Docker host/container; the container's data volume alone does not protect against host loss.

## Verification

`python scripts/smoke-docker.py` exercises manager plan/apply, official game installation, a custom published port, console, backup/restore and ownership-safe cleanup against Docker Desktop. It requires a built manager executable and `.vsm/artifacts/vsm-agent`. No cloud account is used. SSH Docker uses the same command backend over OpenSSH; end-to-end verification requires a separately provisioned SSH host.
