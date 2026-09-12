#!/usr/bin/env python3
"""Generate distribution notices from locked, installed dependencies.

Run npm ci and cargo fetch --locked first. Generation uses offline Cargo
metadata and vendored, provenance-bearing supplements for crates that omit
license files. It never substitutes a guessed license for a missing notice.
"""
import argparse
import hashlib
from html.parser import HTMLParser
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
PREFIXES = ("license", "licence", "copying", "copyright", "notice", "authors")


class NoticeHTML(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.parts = []

    def handle_data(self, data):
        self.parts.append(data)

    def handle_starttag(self, tag, attrs):
        if tag in ("p", "div", "pre", "h1", "h2", "h3", "li", "br"):
            self.parts.append("\n")
        if tag == "a":
            href = dict(attrs).get("href", "")
            if href.startswith(("https://", "http://")):
                self.parts.append(f" [{href}] ")


def read(path):
    return path.read_text(encoding="utf-8-sig").replace("\r\n", "\n").strip()


def license_files(base):
    return sorted((p for p in base.rglob("*") if p.is_file()
                   and p.name.lower().startswith(PREFIXES)
                   and p.suffix.lower() not in (".rs", ".c", ".h", ".py", ".json")),
                  key=lambda p: p.relative_to(base).as_posix())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--offline", "--locked", "--format-version", "1"],
        cwd=ROOT, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)))
    supplements = json.loads(read(ROOT / "scripts/notice-supplements.json"))
    entries, texts, missing = [], {}, []

    def add(name, version, license_name, source, records, authors=None):
        if not records:
            missing.append(f"{name}@{version}")
            return
        lines = [f"{name} {version}", f"Declared license: {license_name or 'not declared'}",
                 f"Source: {source}"]
        if authors:
            lines.append("Authors: " + "; ".join(authors))
        for record in records:
            content = record["text"].replace("\r\n", "\n").strip()
            if not content:
                missing.append(f"{name}@{version}: empty {record['path']}")
                continue
            key = hashlib.sha256(content.encode()).hexdigest()
            texts[key] = content
            lines.append(f"Notice: {record['path']} -> text {key}")
            if record.get("url"):
                lines.append("Notice source: " + record["url"])
        entries.append("\n".join(lines))

    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if not package.get("source"):
            continue
        base = pathlib.Path(package["manifest_path"]).parent
        key = f"{package['name']}@{package['version']}"
        records = [{"path": p.relative_to(base).as_posix(), "text": read(p)}
                   for p in license_files(base)]
        records += supplements.get(key, [])
        add(package["name"], package["version"], package.get("license"),
            f"https://crates.io/crates/{package['name']}/{package['version']}",
            records, package.get("authors"))

    pending = [(name, ROOT / "apps/ui") for name in
               json.loads(read(ROOT / "apps/ui/package.json"))["dependencies"]]
    visited = set()
    while pending:
        name, origin = pending.pop()
        base = next((parent / "node_modules" / name for parent in [origin, *origin.parents]
                     if (parent / "node_modules" / name / "package.json").is_file()), None)
        if base is None:
            missing.append("npm package " + name)
            continue
        manifest = json.loads(read(base / "package.json"))
        key = (manifest["name"], manifest["version"])
        if key in visited:
            continue
        visited.add(key)
        records = [{"path": p.relative_to(base).as_posix(), "text": read(p)}
                   for p in license_files(base) if "node_modules" not in p.relative_to(base).parts]
        add("npm:" + key[0], key[1], manifest.get("license"),
            f"https://www.npmjs.com/package/{key[0]}/v/{key[1]}", records)
        pending += [(dependency, base) for dependency in manifest.get("dependencies", {})]

    fonts = ROOT / "apps/ui/public/fonts"
    for name, version in [("IBM Plex Sans", "1.1.0"), ("IBM Plex Mono", "2.5.0")]:
        path = fonts / (name.replace(" ", "-") + "-LICENSE.txt")
        add(name, version, "OFL-1.1", "https://github.com/IBM/plex",
            [{"path": path.name, "text": read(path)}])

    rust = subprocess.check_output(["rustc", "--version", "--verbose"], text=True,
                                   creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    rust_info = dict(line.split(": ", 1) for line in rust.splitlines() if ": " in line)
    sysroot = pathlib.Path(subprocess.check_output(
        ["rustc", "--print", "sysroot"], text=True,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)).strip())
    stdlib_notice = sysroot / "share/doc/rust/COPYRIGHT-library.html"
    if stdlib_notice.is_file():
        html = NoticeHTML()
        html.feed(read(stdlib_notice))
        add("Rust standard library", rust_info["release"],
            "MIT OR Apache-2.0; dependency exceptions listed in notice",
            "https://github.com/rust-lang/rust/tree/" + rust_info["commit-hash"] + "/library",
            [{"path": "COPYRIGHT-library.html (text content)", "text": "".join(html.parts)}])
    else:
        missing.append("Rust standard-library notice (install rust-docs with rustup)")

    if missing:
        print("Missing notices; output not updated:\n" + "\n".join(missing), file=sys.stderr)
        return 1
    header = """THIRD-PARTY NOTICES — Vintage Story Server Manager

Generated by scripts/generate-notices.py from resolved Cargo.lock crates,
installed UI runtime npm dependencies, bundled fonts and the current Rust
toolchain's standard library. Regenerate when dependencies or the toolchain
change. The Cargo inventory
is deliberately overinclusive: it covers platform-specific, build and test
dependencies, some of which are not included in a particular binary.

Third-party code retains its respective licenses and copyright notices.
An OR expression offers alternative licenses; including upstream texts does
not change that choice. Source links identify the unmodified upstream release.
For MPL-2.0 components, corresponding source is available from those links;
the application does not modify those dependencies. This application's
Apache-2.0 license does not relicense third-party components.

Identical notice texts are stored once and referenced by their SHA-256 below.
Supplemental texts retain their original upstream source URL. No credentials,
local machine paths, or generated timestamps are included.

DEPENDENCY INVENTORY
====================
"""
    output = header + "\n\n".join(sorted(entries)) + "\n\nLICENSE AND NOTICE TEXTS\n=======================\n"
    output += "\n\n".join(f"TEXT {key}\n{'-' * 72}\n{value}" for key, value in sorted(texts.items())) + "\n"
    destination = ROOT / "THIRD-PARTY-NOTICES.txt"
    if args.check:
        if not destination.exists() or read(destination) != output.strip():
            print("THIRD-PARTY-NOTICES.txt is stale", file=sys.stderr)
            return 1
    else:
        destination.write_text(output, encoding="utf-8", newline="\n")
    print(f"Verified {len(entries)} dependency entries and {len(texts)} distinct notice texts")
    return 0


if __name__ == "__main__":
    sys.exit(main())
