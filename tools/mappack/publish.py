"""Build a game's map pack and publish it for the app to download.

A map pack is everything under games/<id>/ that is generated from the game (tiles,
icons, pins), zipped. The app ships game.json itself. Packs live on the `maps` GitHub
release (a pre-release, so it never becomes the app's "latest" release), next to
maps.json, which lists the current pack per game.

Usage (needs the GitHub CLI, logged in):
    python -I tools/mappack/publish.py --game sons-of-the-forest
    python -I tools/mappack/publish.py --game sons-of-the-forest --dry-run   # build only

Installed apps pick up the new pack within a few hours (or at their next start).
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import zipfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DEFAULT_REPO = "HYPExMon5ter/game-maps"
RELEASE_TAG = "maps"
MANIFEST = "maps.json"
# Shipped with the app, or only used during development.
EXCLUDE = {"game.json", "preview.png", "preview-caves.png"}
# Already compressed; storing them avoids wasted CPU for no gain.
STORED_EXTENSIONS = {".webp", ".png", ".jpg", ".jpeg"}


def pack_files(game_dir: str) -> list[tuple[str, str]]:
    """(absolute path, archive name) for every file that belongs in the pack."""
    files = []
    for dirpath, _, names in os.walk(game_dir):
        for name in names:
            path = os.path.join(dirpath, name)
            rel = os.path.relpath(path, game_dir).replace(os.sep, "/")
            if rel in EXCLUDE:
                continue
            files.append((path, rel))
    files.sort(key=lambda f: f[1])
    return files


def build_pack(game_dir: str, out_path: str) -> dict:
    files = pack_files(game_dir)
    if not any(rel.startswith("tiles/") for _, rel in files):
        raise SystemExit(f"{game_dir} has no tiles/; generate them with tools/mapgen first")
    with zipfile.ZipFile(out_path, "w") as zf:
        for path, rel in files:
            ext = os.path.splitext(rel)[1].lower()
            compress = zipfile.ZIP_STORED if ext in STORED_EXTENSIONS else zipfile.ZIP_DEFLATED
            zf.write(path, rel, compress_type=compress)
    digest = hashlib.sha256()
    with open(out_path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return {"sha256": digest.hexdigest(), "size": os.path.getsize(out_path), "files": len(files)}


def gh(*args: str, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(["gh", *args], check=check, capture_output=True, text=True)


def fetch_manifest(repo: str) -> dict:
    """Current maps.json, creating the `maps` release on first use."""
    if gh("release", "view", RELEASE_TAG, "--repo", repo, check=False).returncode != 0:
        print(f"creating release '{RELEASE_TAG}' on {repo}")
        gh("release", "create", RELEASE_TAG, "--repo", repo, "--prerelease", "--title", "Map data",
           "--notes", "Map packs downloaded by the Game Maps app. Managed by tools/mappack/publish.py.")
        return {"packs": {}}
    res = gh("release", "download", RELEASE_TAG, "--repo", repo, "--pattern", MANIFEST, "--output", "-", check=False)
    if res.returncode != 0 or not res.stdout.strip():
        return {"packs": {}}
    return json.loads(res.stdout)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--game", required=True, help="game id (folder under games/)")
    ap.add_argument("--version", help="pack version (default: current UTC time, e.g. 2026.11.02.1530)")
    ap.add_argument("--repo", default=DEFAULT_REPO)
    ap.add_argument("--dry-run", action="store_true", help="build the pack without uploading")
    args = ap.parse_args()

    game_dir = os.path.join(ROOT, "games", args.game)
    if not os.path.isfile(os.path.join(game_dir, "game.json")):
        raise SystemExit(f"unknown game: {args.game}")
    version = args.version or dt.datetime.now(dt.timezone.utc).strftime("%Y.%m.%d.%H%M")
    out_dir = os.path.join(ROOT, "target", "mappacks")
    os.makedirs(out_dir, exist_ok=True)
    zip_name = f"{args.game}-{version}.zip"
    zip_path = os.path.join(out_dir, zip_name)

    info = build_pack(game_dir, zip_path)
    print(f"built {zip_path}: {info['files']} files, {info['size'] / 1e6:.1f} MB")
    if args.dry_run:
        return

    manifest = fetch_manifest(args.repo)
    manifest.setdefault("packs", {})[args.game] = {
        "version": version,
        "url": f"https://github.com/{args.repo}/releases/download/{RELEASE_TAG}/{zip_name}",
        "sha256": info["sha256"],
        "size": info["size"],
    }
    # Upload the pack before the manifest that points to it.
    gh("release", "upload", RELEASE_TAG, zip_path, "--repo", args.repo, "--clobber")
    with tempfile.TemporaryDirectory() as tmp:
        manifest_path = os.path.join(tmp, MANIFEST)
        with open(manifest_path, "w", encoding="utf-8") as f:
            json.dump(manifest, f, indent=2)
        gh("release", "upload", RELEASE_TAG, manifest_path, "--repo", args.repo, "--clobber")
    print(f"published {args.game} {version}")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as e:
        sys.exit(f"gh {' '.join(e.cmd[1:])} failed:\n{e.stderr}")
