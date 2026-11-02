"""Bump the app version, commit, and tag a release.

    python -I tools/release.py 0.2.0
    git push --follow-tags

Pushing the tag runs .github/workflows/release.yml, which builds the installer and
publishes it; installed copies then update themselves.
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


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def set_json_version(path: str, version: str) -> None:
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    data["version"] = version
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(data, f, indent=2)
        f.write("\n")


def set_cargo_version(version: str) -> None:
    with open(CARGO_TOML, encoding="utf-8") as f:
        text = f.read()
    text, n = re.subn(r'(\[workspace\.package\][^\[]*?\nversion = )"[^"]*"', rf'\g<1>"{version}"', text, count=1)
    if n != 1:
        raise SystemExit("could not find [workspace.package] version in Cargo.toml")
    with open(CARGO_TOML, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def main() -> None:
    if len(sys.argv) != 2 or not re.fullmatch(r"\d+\.\d+\.\d+", sys.argv[1]):
        raise SystemExit(__doc__)
    version = sys.argv[1]
    if git("status", "--porcelain").strip():
        raise SystemExit("commit or stash your changes first")

    set_json_version(TAURI_CONF, version)
    set_json_version(PACKAGE_JSON, version)
    set_cargo_version(version)
    # Refresh Cargo.lock with the new workspace version.
    subprocess.run(["cargo", "metadata", "--format-version", "1", "--quiet"], cwd=ROOT, check=True, stdout=subprocess.DEVNULL)

    git("add", TAURI_CONF, PACKAGE_JSON, CARGO_TOML, os.path.join(ROOT, "Cargo.lock"))
    git("commit", "-m", f"Release {version}")
    # Annotated, so `git push --follow-tags` sends it.
    git("tag", "-a", f"v{version}", "-m", f"Game Maps {version}")
    print(f"Tagged v{version}. Publish it with: git push --follow-tags")


if __name__ == "__main__":
    main()
