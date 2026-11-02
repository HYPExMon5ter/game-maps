"""Set the app version everywhere it is recorded and print it.

    python -I tools/version.py patch|minor|major|X.Y.Z

Used by the Release workflow; run tools/release.py to publish a release.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
TAURI_CONF = os.path.join(ROOT, "apps", "overlay", "src-tauri", "tauri.conf.json")
PACKAGE_JSON = os.path.join(ROOT, "apps", "overlay", "package.json")
CARGO_TOML = os.path.join(ROOT, "Cargo.toml")
FILES = [TAURI_CONF, PACKAGE_JSON, CARGO_TOML, os.path.join(ROOT, "Cargo.lock")]


def current() -> str:
    with open(TAURI_CONF, encoding="utf-8") as f:
        return json.load(f)["version"]


def next_version(version: str, bump: str) -> str:
    if re.fullmatch(r"\d+\.\d+\.\d+", bump):
        return bump
    major, minor, patch = (int(p) for p in version.split("."))
    if bump == "major":
        return f"{major + 1}.0.0"
    if bump == "minor":
        return f"{major}.{minor + 1}.0"
    if bump == "patch":
        return f"{major}.{minor}.{patch + 1}"
    raise SystemExit(f"unknown bump '{bump}': use patch, minor, major or X.Y.Z")


def set_json_version(path: str, version: str) -> None:
    """Replaces the top-level "version" in place, keeping the file's formatting."""
    with open(path, encoding="utf-8") as f:
        text = f.read()
    text, n = re.subn(r'^(  "version": )"[^"]*"', rf'\g<1>"{version}"', text, count=1, flags=re.MULTILINE)
    if n != 1:
        raise SystemExit(f"could not find the version in {path}")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def set_cargo_version(version: str) -> None:
    with open(CARGO_TOML, encoding="utf-8") as f:
        text = f.read()
    text, n = re.subn(r'(\[workspace\.package\][^\[]*?\nversion = )"[^"]*"', rf'\g<1>"{version}"', text, count=1)
    if n != 1:
        raise SystemExit("could not find [workspace.package] version in Cargo.toml")
    with open(CARGO_TOML, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    version = next_version(current(), sys.argv[1])
    set_json_version(TAURI_CONF, version)
    set_json_version(PACKAGE_JSON, version)
    set_cargo_version(version)
    # Record the new workspace version in Cargo.lock.
    subprocess.run(["cargo", "update", "--workspace", "--offline", "--quiet"], cwd=ROOT, check=True)
    print(version)


if __name__ == "__main__":
    main()
