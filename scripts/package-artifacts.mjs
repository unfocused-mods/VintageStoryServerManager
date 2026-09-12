import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  chmod,
  copyFile,
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const digest = async (file) =>
  createHash("sha256")
    .update(await readFile(file))
    .digest("hex");

export async function verifyAgent(workspace = root) {
  const agent = path.join(workspace, ".vsm/artifacts/vsm-agent");
  const manifest = await readFile(
    path.join(workspace, ".vsm/artifacts/SHA256SUMS"),
    "utf8",
  );
  const expected = manifest.match(/^([a-f0-9]{64})\s+\*?vsm-agent\s*$/m)?.[1];
  if (!expected || (await digest(agent)) !== expected)
    throw new Error("Bundled agent checksum does not match");
  const bytes = await readFile(agent);
  if (
    bytes.length < 20 ||
    bytes.subarray(0, 4).toString("hex") !== "7f454c46" ||
    bytes[4] !== 2 ||
    bytes[5] !== 1 ||
    bytes.readUInt16LE(18) !== 62
  ) {
    throw new Error("Bundled agent must be a Linux x64 ELF executable");
  }
  if (process.platform !== "win32") await chmod(agent, 0o755);
}

function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(`${command} failed (${result.status})`);
}

export async function collect(target, workspace = root) {
  const expected = {
    "x86_64-pc-windows-msvc": [".msi", ".exe"],
    "aarch64-apple-darwin": [".dmg"],
    "x86_64-apple-darwin": [".dmg"],
    "x86_64-unknown-linux-gnu": [".deb", ".AppImage"],
  }[target];
  if (!expected) throw new Error("Unsupported target");
  await verifyAgent(workspace);
  const agent = path.join(workspace, ".vsm/artifacts/vsm-agent");
  const release = path.join(workspace, "target", target, "release");
  const output = path.join(workspace, "dist/installers", target);
  await mkdir(path.dirname(output), { recursive: true });
  // Refuse to mix this build with an older local collection.
  await mkdir(output);
  const found = new Set();
  async function visit(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const source = path.join(directory, entry.name);
      if (entry.isFile() && expected.includes(path.extname(entry.name))) {
        found.add(path.extname(entry.name));
        await copyFile(source, path.join(output, entry.name));
      }
    }
  }
  const folders = {
    ".msi": "msi",
    ".exe": "nsis",
    ".dmg": "dmg",
    ".deb": "deb",
    ".AppImage": "appimage",
  };
  for (const extension of expected)
    await visit(path.join(release, "bundle", folders[extension]));
  if (expected.some((extension) => !found.has(extension)))
    throw new Error(
      `Missing installers: ${expected.filter((e) => !found.has(e)).join(", ")}`,
    );
  const archiveName = `vsm-cli-${target}`;
  const stagingParent = path.join(workspace, "target/package");
  await mkdir(stagingParent, { recursive: true });
  const stagingRoot = await mkdtemp(path.join(stagingParent, "build-"));
  const stage = path.join(stagingRoot, archiveName);
  await mkdir(path.join(stage, "artifacts"), { recursive: true });
  await copyFile(
    path.join(release, target.includes("windows") ? "vsm.exe" : "vsm"),
    path.join(stage, target.includes("windows") ? "vsm.exe" : "vsm"),
  );
  await copyFile(agent, path.join(stage, "artifacts/vsm-agent"));
  await copyFile(path.join(workspace, "LICENSE"), path.join(stage, "LICENSE"));
  await copyFile(
    path.join(workspace, "docs/installers.md"),
    path.join(stage, "README.md"),
  );
  await cp(path.join(workspace, "apps/ui/dist"), path.join(stage, "ui"), {
    recursive: true,
  });
  if (target.includes("windows")) {
    // Paths are supplied through environment variables, never interpolated into shell code.
    const result = spawnSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Compress-Archive -LiteralPath $env:VSM_PACKAGE_SOURCE -DestinationPath $env:VSM_PACKAGE_DESTINATION -Force",
      ],
      {
        stdio: "inherit",
        env: {
          ...process.env,
          VSM_PACKAGE_SOURCE: stage,
          VSM_PACKAGE_DESTINATION: path.join(output, `${archiveName}.zip`),
        },
      },
    );
    if (result.error || result.status !== 0)
      throw result.error ?? new Error("ZIP packaging failed");
  } else {
    await chmod(path.join(stage, "vsm"), 0o755);
    await chmod(path.join(stage, "artifacts/vsm-agent"), 0o755);
    run("tar", [
      "-czf",
      path.join(output, `${archiveName}.tar.gz`),
      "-C",
      path.dirname(stage),
      archiveName,
    ]);
  }
  const metadata = {
    target,
    commit: process.env.GITHUB_SHA ?? "local",
    run: process.env.GITHUB_RUN_ID ?? null,
    desktop_signing: target.includes("apple")
      ? "ad-hoc; not notarized"
      : "unsigned",
    cli_signing: "not publisher-signed",
  };
  await writeFile(
    path.join(output, "build.json"),
    `${JSON.stringify(metadata, null, 2)}\n`,
  );
  const files = (await readdir(output))
    .filter((name) => name !== "SHA256SUMS")
    .sort();
  await writeFile(
    path.join(output, "SHA256SUMS"),
    (
      await Promise.all(
        files.map(
          async (name) => `${await digest(path.join(output, name))}  ${name}\n`,
        ),
      )
    ).join(""),
  );
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const [command, target] = process.argv.slice(2);
  if (command === "verify-agent") await verifyAgent();
  else if (command === "collect") await collect(target);
  else
    throw new Error(
      "Usage: node scripts/package-artifacts.mjs verify-agent | collect TARGET",
    );
}
