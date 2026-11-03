"""Publish a new version of the app. Installed copies update themselves.

    python -I tools/release.py                 # 0.1.0 -> 0.1.1
    python -I tools/release.py minor "Added a mini-map for caves"
    python -I tools/release.py 1.0.0 "..."         # an exact version

This starts the Release workflow on GitHub (same as pressing "Run workflow" on the
Actions tab): it runs the tests while it bumps the version and builds and signs the
installer, then publishes the release from the latest commit on master. Commit and
push your changes to master first. Needs the GitHub CLI, logged in.
"""
from __future__ import annotations

import re
import subprocess
import sys
import time

WORKFLOW = "release.yml"
DEFAULT_NOTES = "Bug fixes and improvements."
REQUIRED_SECRETS = {"TAURI_SIGNING_PRIVATE_KEY", "TAURI_SIGNING_PRIVATE_KEY_PASSWORD"}


def gh(*args: str) -> str:
    return subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout


def main() -> None:
    args = sys.argv[1:]
    bump = args[0] if args else "patch"
    if bump not in {"patch", "minor", "major"} and not re.fullmatch(r"\d+\.\d+\.\d+", bump):
        raise SystemExit(__doc__)
    notes = args[1] if len(args) > 1 else DEFAULT_NOTES

    missing = REQUIRED_SECRETS - set(gh("secret", "list", "--json", "name", "--jq", ".[].name").split())
    if missing:
        raise SystemExit(f"missing repository secrets: {', '.join(sorted(missing))} (see README, Publishing)")

    gh("workflow", "run", WORKFLOW, "--ref", "master", "-f", f"bump={bump}", "-f", f"notes={notes}")
    time.sleep(3)  # let GitHub register the run
    url = gh("run", "list", "--workflow", WORKFLOW, "--limit", "1", "--json", "url", "--jq", ".[0].url").strip()
    print(f"Release started ({bump}): {url}")
    print("It takes about 15 minutes; installed apps update within 30 minutes after that.")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as e:
        sys.exit(f"gh {' '.join(e.cmd[1:])} failed:\n{e.stderr}")
