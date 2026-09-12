# Alpha releases and development installers

Download published binaries from [GitHub Releases](https://github.com/cjohnsto-nz/VintageStoryServerManager/releases). The initial alpha release includes Windows x64 NSIS/MSI installers, the Windows CLI archive and a Linux x64 Remote agent. The release asset list is authoritative; macOS and Linux desktop packages remain workflow-buildable targets until published and tested. Read the [cloud-cost responsibility notice](../README.md#cloud-costs-and-responsibility) before deploying resources.

The **Prepare installers** GitHub Actions workflow runs only from **Actions → Prepare installers → Run workflow**. Select the branch to build. It does not publish a release or deploy a server. Existing test workflows still run on pushes and pull requests.

After a successful run, download the artifact for your computer:

| Computer            | Artifact                       | Desktop installer       |
| ------------------- | ------------------------------ | ----------------------- |
| Windows x64         | `vsm-x86_64-pc-windows-msvc`   | `.exe` (NSIS) or `.msi` |
| macOS Apple Silicon | `vsm-aarch64-apple-darwin`     | `.dmg`                  |
| macOS Intel         | `vsm-x86_64-apple-darwin`      | `.dmg`                  |
| Linux x64           | `vsm-x86_64-unknown-linux-gnu` | `.deb` or `.AppImage`   |

Open the installer and launch **Vintage Story Server Manager**. The desktop app starts its local management service automatically. You do not need to start a separate service or enter a local access token.

These are development alpha builds. Windows and Linux packages are unsigned. macOS uses an ad-hoc signature without an Apple developer identity and is not notarized. Operating-system security prompts are expected. The workflow requires no signing secrets. Native signing and notarization remain stable-release goals; an alpha release is not a signed or independently audited distribution.

Linux desktop builds use Ubuntu 22.04 as their build baseline. The agent is built on Ubuntu 24.04 for Ubuntu 24.04 x64 game hosts. macOS builds are separate native architectures. ARM game hosts are not supported.

## Contents and verification

Each workflow platform artifact contains desktop installers, a CLI archive, `build.json` recording the source commit, and `SHA256SUMS`. The separate `vsm-agent-linux-x64` artifact contains the Linux agent and its checksum. Workflow artifacts expire after 30 days; published release assets are separate downloads.

Verify a downloaded file on Windows with `Get-FileHash -Algorithm SHA256 FILE`, on macOS with `shasum -a 256 FILE`, or on Linux with `sha256sum FILE`, then compare it with the corresponding line in `SHA256SUMS`. Checksums detect damaged or mismatched downloads; they are not publisher signatures.

The desktop installers include the Linux agent. It is copied into the application's private data directory for SSH installation on a game host. It is not executed on Windows or macOS.

The CLI archive includes the native `vsm` executable, `artifacts/vsm-agent`, and dashboard assets in `ui/`. With the desktop app open, run `vsm --desktop servers --json` to use its inventory; `vsm --desktop mcp` starts the MCP stdio server. See [CLI/MCP administration](administration.md) and the [Codex plugin](../plugins/vintage-story-server-manager/README.md).

For a separate local service, extract the archive, open a terminal in that directory, set `VSM_AGENT_BINARY` to the absolute path of the agent file, then run `./vsm serve --ui ui` (`.\vsm.exe serve --ui ui` in PowerShell). For a hosted service, set `VSM_URL` and `VSM_TOKEN_FILE` to its URL and a protected local token file; do not combine these with `--desktop`. Run `vsm --help` for commands. Linux CLI users need the system D-Bus library; local credential storage also needs a functioning keyring session. SSH operations require OpenSSH. Azure CLI authentication requires Azure CLI.

## Build locally

Install [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), Rust 1.96.0, and Node.js 22. Run `npm ci`. Source development uses `npm run desktop`.

Published builds pin Rust 1.96.0 for both the agent and desktop/CLI so their standard-library license notices match `THIRD-PARTY-NOTICES.txt`. Use that toolchain for local release builds too (`rustup override set 1.96.0`). When changing the release toolchain, regenerate the notices with `scripts/generate-notices.py` before building. The separate Checks workflow continues to test the current stable Rust toolchain.

To create a complete installer, download the agent artifact from the same source revision into `.vsm/artifacts/` (including `SHA256SUMS`), then run:

```sh
node scripts/package-artifacts.mjs verify-agent
node scripts/desktop.mjs build --config tauri.packaging.conf.json -- --locked
```

Builds must run on the target operating system with its native toolchain. CI passes an explicit Rust target and bundle formats, then collects only the expected formats and fails if one is missing. To produce the matching CLI and checksums locally, follow the build and collection commands in `.github/workflows/installers.yml` for your target.

The collection script refuses an existing `dist/installers/TARGET` directory to prevent old installers from being included in a new build. Move the previous output elsewhere before collecting again. Staging directories under `target/package/` are unique per run.

The workflow follows [Tauri's resource mapping](https://v2.tauri.app/develop/resources/), [Linux AppImage baseline guidance](https://v2.tauri.app/distribute/appimage/), and [GitHub's artifact workflow](https://docs.github.com/actions/using-workflows/storing-workflow-data-as-artifacts). Windows/macOS/Linux installer execution still needs manual testing on clean machines.
