#!/usr/bin/env python3
"""Prepare a local plugin installation; no marketplace or credential edits."""
import argparse
import json
import shutil
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, required=True)
parser.add_argument("--destination", type=Path, required=True)
args = parser.parse_args()
source = Path(__file__).resolve().parents[1]
binary = args.binary.resolve(strict=True)
destination = args.destination.resolve()
if not binary.is_file():
    parser.error("--binary must identify the vsm executable")
if destination == source or source in destination.parents or destination in source.parents:
    parser.error("choose a destination outside the repository plugin source")
destination.mkdir(parents=True, exist_ok=True)
for name in (".codex-plugin", "skills", "scripts"):
    shutil.copytree(source / name, destination / name, dirs_exist_ok=True)
shutil.copy2(source / "README.md", destination / "README.md")
exe = destination / "bin" / ("vsm.exe" if binary.suffix.lower() == ".exe" else "vsm")
exe.parent.mkdir(exist_ok=True)
if exe.resolve() != binary:
    shutil.copy2(binary, exe)
config = {"mcpServers": {"vintage-story-server-manager": {"command": str(exe), "args": ["--desktop", "mcp"]}}}
(destination / ".mcp.json").write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")
print(f"Prepared {destination}; executable {exe}")
