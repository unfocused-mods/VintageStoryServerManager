# Recovering Azure deployments

An existing Azure deployment can be rediscovered from a new computer without a backup of the old Manager database. Discovery reads Azure resource metadata. Adoption adds the deployment to the current Manager's inventory; it does not provision resources, reinstall the Remote, or replace game files.

Stop using the old Manager for the deployment before managing it from the replacement. Discovery cannot establish whether another computer is still issuing commands, and it does not revoke that computer's access.

## Discover and adopt

1. Start the desktop app, or run a Manager and connect the CLI to it.
2. Add the Azure account again, signing in with access to the deployment's subscription. The old Manager's account record and login session are not needed.
3. Discover deployments, inspect the returned resource IDs and warnings, then adopt the intended VM.

In the desktop app, open **Accounts → Find existing servers → Add to Manager** for the Azure account. Then open the server's **Infrastructure → Reconnect** flow to configure SSH access, verify the host identity, and reconnect to its existing Remote.

For the CLI, use the account ID from `vsm accounts`:

```sh
vsm account-discover ACCOUNT_ID --json
vsm account-adopt ACCOUNT_ID --vm-id "/subscriptions/SUBSCRIPTION/resourceGroups/GROUP/providers/Microsoft.Compute/virtualMachines/VM" --ssh-private-key "/home/me/.ssh/vsm" --json
```

`--name` optionally changes the name shown in this Manager. The `--ssh-private-key` value is a path on the **Manager's machine**, not the path on a separate CLI client. The key contents are not sent in this request. Discovery and adoption also work with a deallocated VM; they do not start it.

Only deployments identified by the app's Azure ownership metadata are candidates. A VM with missing or conflicting metadata may need manual investigation. Discovery is not a general-purpose import of arbitrary Azure VMs. Review warnings before taking infrastructure actions.

## SSH access on a new machine

Azure sign-in restores access to cloud resource information. It does not recover the SSH private key used to access the Remote. Copy the original private key securely to the replacement Manager machine, or restore it from a protected backup.

You can omit `--ssh-private-key` when adopting to restore the cloud inventory first. Game management remains unavailable until SSH access is restored. Set the path later with:

```sh
vsm server-connection SERVER_ID --ssh-private-key "/home/me/.ssh/vsm" --json
```

Verify the SSH host identity when connecting from a new Manager, ensuring its trusted host key is recorded in the Manager's SSH `known_hosts` before reconnecting. If the VM is deallocated, **Start VM** is available after adoption. Once it is reachable, reconnect:

```sh
vsm server-reconnect SERVER_ID --json
```

Reconnect uses the existing Remote's status endpoint to recover its actual game version and state. It does not install or restart the game. Planned disruptive cloud operations, including restart and deletion, require reconnecting first so the game can be stopped gracefully. Do not choose **Install Remote** as a recovery shortcut: an existing installation should be reconnected to, not reinstalled.

If the original key is lost, use Azure's access-recovery facilities to authorize a replacement key on the existing VM, then configure that key in the Manager. Discovery does not perform that cloud write. An SSH firewall rule restricted to the old computer's public IP may also need updating before the new computer can connect.

## What is recovered

Discovery restores the server inventory and the Azure resource IDs and ownership information available from the deployment. Worlds, mods, configuration, game logs, and backups remain on the game host; the Manager reads them once the Remote is reachable.

Azure metadata does not contain the old Manager's users, sessions, activity history, credentials, or cloud cost-saving policies. Review cost-saving settings after recovery; restoring an inventory entry does not recover the old Manager's pending wake/deallocation operations. Retain backups of the Manager data directory if you need its historical records.

An encrypted inventory export remains useful for moving a complete inventory. See [Manager export and import](hosting.md). Keep SSH keys and backup passwords separately: neither cloud discovery nor an inventory export can recover a lost backup encryption password.
