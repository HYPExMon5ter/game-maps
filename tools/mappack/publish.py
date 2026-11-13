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

Each pack records the Steam build it was made from, so the app can tell players when the
game has moved on, and its pin counts: a generator that silently lost a category after a
game update stops the publish instead of reaching players.

A map's optional parts (EXTRAS: `3d/`, the 3D view's data) are left out of its pack and
published as packs of their own (`<game>-<part>-<version>.zip`, listed under the game's
`extras` in maps.json). The app downloads one only when the player asks for it, so the
map stays small for everyone else.
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
# Generator inputs that one tools/mapgen step leaves for another.
EXCLUDE_DIRS = ("build/",)
# Optional parts of a map: folders published as packs of their own (see the docstring).
EXTRAS = ("3d",)
# Already compressed; storing them avoids wasted CPU for no gain.
STORED_EXTENSIONS = {".webp", ".png", ".jpg", ".jpeg"}
# Fixed entry timestamp, so identical files always produce an identical zip.
ZIP_DATE = (1980, 1, 1, 0, 0, 0)


def pack_files(game_dir: str, extra: str | None = None) -> list[tuple[str, str]]:
    """(absolute path, archive name) for every file that belongs in the map's pack, or
    in the pack of one of its optional parts."""
    files = []
    for dirpath, _, names in os.walk(game_dir):
        for name in names:
            path = os.path.join(dirpath, name)
            rel = os.path.relpath(path, game_dir).replace(os.sep, "/")
            if rel in EXCLUDE or rel.startswith(EXCLUDE_DIRS):
                continue
            part = next((e for e in EXTRAS if rel.startswith(e + "/")), None)
            if part == extra:
                files.append((path, rel))
    files.sort(key=lambda f: f[1])
    return files


def build_pack(game_dir: str, out_path: str, extra: str | None = None) -> dict:
    files = pack_files(game_dir, extra)
    if extra is None and not any(rel.startswith("tiles/") for _, rel in files):
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


# A category that loses more than this share of its pins stops the publish.
MAX_PIN_DROP = 0.3
STEAM_LIBRARIES = (r"D:\SteamLibrary\steamapps", r"C:\Program Files (x86)\Steam\steamapps")


def game_build(game_dir: str) -> str | None:
    """The installed Steam build of a game, from its appmanifest."""
    with open(os.path.join(game_dir, "game.json"), encoding="utf-8") as f:
        app_id = json.load(f).get("steamAppId")
    for library in os.environ.get("STEAM_LIBRARIES", os.pathsep.join(STEAM_LIBRARIES)).split(os.pathsep):
        path = os.path.join(library, f"appmanifest_{app_id}.acf")
        if os.path.exists(path):
            with open(path, encoding="utf-8") as f:
                for line in f:
                    parts = [p for p in line.split('"') if p.strip()]
                    if len(parts) >= 2 and parts[0] == "buildid":
                        return parts[1]
    return None


def pin_counts(game_dir: str) -> dict[str, int]:
    """Pins per category."""
    with open(os.path.join(game_dir, "pins.json"), encoding="utf-8") as f:
        pins = json.load(f).get("pins", [])
    counts: dict[str, int] = {}
    for pin in pins:
        counts[pin["category"]] = counts.get(pin["category"], 0) + 1
    return counts


def lost_pins(before: dict[str, int], after: dict[str, int]) -> list[str]:
    """Categories that shrank too much to be a real change to the game."""
    return [f"{c}: {n} -> {after.get(c, 0)}" for c, n in sorted(before.items()) if after.get(c, 0) < n * (1 - MAX_PIN_DROP)]


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


def published(repo: str, zip_path: str, info: dict) -> dict:
    """Uploads a pack; returns its manifest fields."""
    gh("release", "upload", RELEASE_TAG, zip_path, "--repo", repo, "--clobber")
    name = os.path.basename(zip_path)
    return {"url": f"https://github.com/{repo}/releases/download/{RELEASE_TAG}/{name}", "sha256": info["sha256"], "size": info["size"]}


def zip_name(entry: dict | None) -> set[str]:
    return {entry["url"].rsplit("/", 1)[-1]} if entry else set()


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
        game_dir = os.path.join(GAMES_DIR, game)
        zip_path = os.path.join(out_dir, f"{game}-{version}.zip")
        info = build_pack(game_dir, zip_path)
        print(f"{game}: {info['files']} files, {info['size'] / 1e6:.1f} MB")
        extras = {}
        for extra in EXTRAS:
            if pack_files(game_dir, extra):
                path = os.path.join(out_dir, f"{game}-{extra}-{version}.zip")
                extras[extra] = (path, build_pack(game_dir, path, extra))
                print(f"  {extra}: {extras[extra][1]['files']} files, {extras[extra][1]['size'] / 1e6:.1f} MB")
        if manifest is None:
            for path in [zip_path] + [p for p, _ in extras.values()]:
                print(f"  built {path}")
            continue
        current = manifest.setdefault("packs", {}).get(game)
        counts = pin_counts(game_dir)
        lost = lost_pins((current or {}).get("pinCounts", {}), counts)
        if lost and not args.force:
            raise SystemExit(f"{game}: pins went missing, check the generators (--force to publish anyway):\n  " + "\n  ".join(lost))
        # Upload each pack before the manifest that points to it.
        entry = dict(current or {})
        changed = False
        if not current or current.get("sha256") != info["sha256"] or args.force:
            entry.update(version=version, **published(args.repo, zip_path, info), gameBuild=game_build(game_dir), pinCounts=counts)
            changed = True
            print(f"  map: published version {version}")
        else:
            print("  map: unchanged since the last publish, skipped")
        old_extras = (current or {}).get("extras", {})
        entry["extras"] = {}
        for extra, (path, extra_info) in extras.items():
            old = old_extras.get(extra)
            if old and old.get("sha256") == extra_info["sha256"] and not args.force:
                entry["extras"][extra] = old
                print(f"  {extra}: unchanged since the last publish, skipped")
                continue
            entry["extras"][extra] = {"version": version, **published(args.repo, path, extra_info)}
            changed = True
            print(f"  {extra}: published version {version}")
        if not entry["extras"]:
            del entry["extras"]
        changed |= set(entry.get("extras", {})) != set(old_extras)
        if not changed:
            continue
        manifest["packs"][game] = entry
        upload_manifest(args.repo, manifest)
        # The ones replaced last are kept, so apps that were mid-download can finish.
        keep = zip_name(entry) | zip_name(current)
        for extra in set(entry.get("extras", {})) | set(old_extras):
            keep |= zip_name(entry.get("extras", {}).get(extra)) | zip_name(old_extras.get(extra))
        prune_old_packs(args.repo, game, keep)

if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as e:
        sys.exit(f"gh {' '.join(e.cmd[1:])} failed:\n{e.stderr}")
