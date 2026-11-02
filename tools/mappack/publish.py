"""Publish map data so every installed Game Maps picks it up automatically.

A map pack is everything under games/<id>/ that is generated from the game (tiles,
icons, pins), zipped. The app ships game.json itself. Packs live on the `maps` GitHub
release (a pre-release, so it never becomes the app's "latest" release), next to
maps.json, which lists the current pack per game. Installed apps check it every 30
minutes and whenever a game starts, and install new packs on their own.

Usage (needs the GitHub CLI, logged in):
    python -I tools/mappack/publish.py sons-of-the-forest
    python -I tools/mappack/publish.py --all
    python -I tools/mappack/publish.py sons-of-the-forest --dry-run   # build only

Packs are built reproducibly, so a game whose map data hasn't changed is skipped
(users don't re-download identical maps). Use --force to publish anyway.
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
GAMES_DIR = os.path.join(ROOT, "games")
DEFAULT_REPO = "HYPExMon5ter/game-maps"
RELEASE_TAG = "maps"
MANIFEST = "maps.json"
# Shipped with the app, or only used during development.
EXCLUDE = {"game.json", "preview.png", "preview-caves.png"}
# Already compressed; storing them avoids wasted CPU for no gain.
STORED_EXTENSIONS = {".webp", ".png", ".jpg", ".jpeg"}
# Fixed entry timestamp, so identical files always produce an identical zip.
ZIP_DATE = (1980, 1, 1, 0, 0, 0)


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
            info = zipfile.ZipInfo(rel, date_time=ZIP_DATE)
            info.compress_type = zipfile.ZIP_STORED if ext in STORED_EXTENSIONS else zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            with open(path, "rb") as f:
                zf.writestr(info, f.read())
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
           "--notes", "Map data downloaded automatically by the Game Maps app. Managed by tools/mappack/publish.py.")
        return {"packs": {}}
    res = gh("release", "download", RELEASE_TAG, "--repo", repo, "--pattern", MANIFEST, "--output", "-", check=False)
    if res.returncode != 0 or not res.stdout.strip():
        return {"packs": {}}
    return json.loads(res.stdout)


def upload_manifest(repo: str, manifest: dict) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, MANIFEST)
        with open(path, "w", encoding="utf-8") as f:
            json.dump(manifest, f, indent=2)
        gh("release", "upload", RELEASE_TAG, path, "--repo", repo, "--clobber")


def prune_old_packs(repo: str, game: str, keep: set[str]) -> None:
    """Deletes a game's older pack zips. The one replaced last is kept, so apps that
    were mid-download when the manifest changed can still finish."""
    names = gh("release", "view", RELEASE_TAG, "--repo", repo, "--json", "assets", "--jq", ".assets[].name").stdout.split()
    for name in names:
        if name.startswith(f"{game}-") and name.endswith(".zip") and name not in keep:
            gh("release", "delete-asset", RELEASE_TAG, name, "--repo", repo, "--yes")
            print(f"  removed old pack {name}")


def all_games() -> list[str]:
    return sorted(d for d in os.listdir(GAMES_DIR) if os.path.isfile(os.path.join(GAMES_DIR, d, "game.json")))


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("games", nargs="*", help="game ids (folders under games/)")
    ap.add_argument("--all", action="store_true", help="publish every game that has map data")
    ap.add_argument("--repo", default=DEFAULT_REPO)
    ap.add_argument("--force", action="store_true", help="publish even if the map data is unchanged")
    ap.add_argument("--dry-run", action="store_true", help="build packs without uploading")
    args = ap.parse_args()

    if args.all:
        games = [g for g in all_games() if os.path.isdir(os.path.join(GAMES_DIR, g, "tiles"))]
    else:
        games = args.games
        if not games:
            ap.error(f"name a game ({', '.join(all_games())}) or pass --all")
        unknown = [g for g in games if g not in all_games()]
        if unknown:
            raise SystemExit(f"unknown game: {', '.join(unknown)} (known: {', '.join(all_games())})")

    out_dir = os.path.join(ROOT, "target", "mappacks")
    os.makedirs(out_dir, exist_ok=True)
    manifest = None if args.dry_run else fetch_manifest(args.repo)
    version = dt.datetime.now(dt.timezone.utc).strftime("%Y.%m.%d.%H%M%S")

    for game in games:
        zip_name = f"{game}-{version}.zip"
        zip_path = os.path.join(out_dir, zip_name)
        info = build_pack(os.path.join(GAMES_DIR, game), zip_path)
        print(f"{game}: {info['files']} files, {info['size'] / 1e6:.1f} MB")
        if manifest is None:
            print(f"  built {zip_path}")
            continue
        current = manifest.setdefault("packs", {}).get(game)
        if current and current.get("sha256") == info["sha256"] and not args.force:
            print("  unchanged since the last publish, skipped")
            os.remove(zip_path)
            continue
        # Upload the pack before the manifest that points to it.
        gh("release", "upload", RELEASE_TAG, zip_path, "--repo", args.repo, "--clobber")
        manifest["packs"][game] = {
            "version": version,
            "url": f"https://github.com/{args.repo}/releases/download/{RELEASE_TAG}/{zip_name}",
            "sha256": info["sha256"],
            "size": info["size"],
        }
        upload_manifest(args.repo, manifest)
        keep = {zip_name} | ({current["url"].rsplit("/", 1)[-1]} if current else set())
        prune_old_packs(args.repo, game, keep)
        print(f"  published version {version}")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as e:
        sys.exit(f"gh {' '.join(e.cmd[1:])} failed:\n{e.stderr}")
