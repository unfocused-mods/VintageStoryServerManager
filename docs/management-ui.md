# Managing a server

The sidebar and top bar stay in place while the main content scrolls. Long dialogs scroll inside their content area, keeping the title and close button visible. Use Tab to reach controls; Escape closes a dialog and returns focus to its opening control.

## Starting and stopping

Overview shows actions appropriate to the game state. A running game offers **Stop** and **Restart**; a stopped or sleeping game offers **Start server**. A deallocated cloud VM offers **Start VM**. Incomplete deployments link to **Infrastructure** to finish setup.

Infrastructure shows provider-reported VM power separately from Remote installation and game state. Azure starts a new VM during deployment. Verify its SSH fingerprint, then choose **Install Remote and game**; creating the VM alone does not install the Remote.

## Configuration and worlds

Stop the game before saving server configuration. Settings groups cover server identity, player access, game rules, connection and performance, and logging and protection. Invalid values block saving. Empty optional passwords and nullable limits are stored explicitly as `null`; an empty required numeric value must be corrected.

Advanced JSON retains unknown settings. Submitted properties are merged with the saved configuration: omitting a property does not reset or remove it. Deployment owns connection addresses, ports and mod paths; use the world tools to select a different save file.

World creation options come from the installed game's built-in metadata. Available presets and options depend on the installed release; this is not a catalog of every mod-added world setting. Creating a world is separate from changing the server configuration. Review the selected settings before creating it.

## Activity details

Expand **Operation details** in Activity to read recorded steps and failures. Running operations refresh while expanded. **Copy details** copies the displayed text; **Download details** saves it as a log file. Loading failures provide a retry.

Older operations may contain only their original summary. The interface identifies these cases rather than inventing missing steps. Recorded diagnostics are bounded and redact recognized credential-bearing content.

## Updating the Remote

**Update Remote** in Infrastructure explicitly updates the host's management software. It is available for installed Remotes, including local Docker and Docker over SSH. This is separate from **Install selected release**, which changes the Vintage Story game version. Neither action runs automatically.

The Remote update stops the game safely before replacing management software and checks the updated Remote. If the game was running, the update attempts to resume it. Check Activity if an update fails before retrying; the log records the failed stage and recovery outcome.

## Reducing Azure idle costs

Install the Remote and game first, then enable **Reduce costs when empty**. The default delay is 15 minutes without players. The Remote saves and stops the game, and the Manager deallocates the VM. Azure compute charges stop after deallocation; disks and public IPs continue to incur charges.

The Manager must run outside that VM and remain online. Keep the desktop application open or use a separately hosted Manager. Use **Start server** in the cost-saving panel to wake the VM and game; joining a deallocated server does not wake it automatically.
