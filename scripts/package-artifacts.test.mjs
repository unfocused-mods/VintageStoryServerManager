import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { spawnSync } from "node:child_process";
import { collect, verifyAgent } from "./package-artifacts.mjs";

async function fixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), "vsm-package-test-"));
  const bytes = Buffer.alloc(64);
  bytes.write("7f454c46", 0, "hex");
  bytes[4] = 2;
  bytes[5] = 1;
  bytes.writeUInt16LE(62, 18);
  for (const dir of [".vsm/artifacts", "docs", "apps/ui/dist"])
    await mkdir(path.join(root, dir), { recursive: true });
  await writeFile(path.join(root, ".vsm/artifacts/vsm-agent"), bytes);
  await writeFile(
    path.join(root, ".vsm/artifacts/SHA256SUMS"),
    `${createHash("sha256").update(bytes).digest("hex")}  vsm-agent\n`,
  );
  await writeFile(path.join(root, ".vsm/owner.token"), "must-not-be-packaged");
  await writeFile(path.join(root, "LICENSE"), "license");
  await writeFile(path.join(root, "docs/installers.md"), "instructions");
  await writeFile(path.join(root, "apps/ui/dist/index.html"), "dashboard");
  return root;
}

test("rejects changed or non-Linux agent binaries", async () => {
  const root = await fixture();
  await verifyAgent(root);
  await writeFile(path.join(root, ".vsm/artifacts/vsm-agent"), "changed");
  await assert.rejects(verifyAgent(root), /checksum/);
  const wrongArchitecture = Buffer.alloc(64);
  await writeFile(
    path.join(root, ".vsm/artifacts/vsm-agent"),
    wrongArchitecture,
  );
  await writeFile(
    path.join(root, ".vsm/artifacts/SHA256SUMS"),
    `${createHash("sha256").update(wrongArchitecture).digest("hex")}  vsm-agent\n`,
  );
  await assert.rejects(verifyAgent(root), /Linux x64 ELF/);
});

test("collects expected installers, CLI, UI, and agent without inventory data; refuses stale output", async () => {
  const root = await fixture();
  const target = "x86_64-unknown-linux-gnu";
  const release = path.join(root, "target", target, "release");
  for (const dir of ["deb", "appimage"])
    await mkdir(path.join(release, "bundle", dir), { recursive: true });
  await writeFile(path.join(release, "bundle/deb/manager.deb"), "deb");
  await writeFile(
    path.join(release, "bundle/appimage/manager.AppImage"),
    "appimage",
  );
  await writeFile(path.join(release, "vsm"), "cli");
  await collect(target, root);
  const output = path.join(root, "dist/installers", target);
  const sums = await readFile(path.join(output, "SHA256SUMS"), "utf8");
  assert.match(sums, /manager\.deb/);
  assert.match(sums, /manager\.AppImage/);
  const archive = path.join(output, `vsm-cli-${target}.tar.gz`);
  const listing = spawnSync("tar", ["-tzf", archive], { encoding: "utf8" });
  assert.equal(listing.status, 0, listing.stderr);
  assert.match(listing.stdout, /ui\/index\.html/);
  assert.match(listing.stdout, /artifacts\/vsm-agent/);
  assert.doesNotMatch(listing.stdout, /owner\.token|\.vsm/);
  await assert.rejects(collect(target, root), /EEXIST/);
});

test("fails when a required installer is absent", async () => {
  const root = await fixture();
  const release = path.join(
    root,
    "target/x86_64-unknown-linux-gnu/release/bundle",
  );
  await mkdir(path.join(release, "deb"), { recursive: true });
  await mkdir(path.join(release, "appimage"), { recursive: true });
  await writeFile(path.join(release, "deb/manager.deb"), "deb");
  await assert.rejects(
    collect("x86_64-unknown-linux-gnu", root),
    /Missing installers: .AppImage/,
  );
});
