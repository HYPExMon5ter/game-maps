# Releasing

Maps and the app are published separately, both through GitHub Releases. Installed
apps check for both every 30 minutes and whenever a game starts.

| You changed… | Publish with | Users get it |
|---|---|---|
| Map data: tiles, icons, pins | `python -I tools/mappack/publish.py <game>` | Automatically, within 30 minutes |
| Anything else, including live features | `python -I tools/release.py` | Automatically once no game is running, or after confirming if they turned automatic updates off |

## Map updates

After regenerating a game's map data (see [Development](development.md#map-data)):

```sh
python -I tools/mappack/publish.py sons-of-the-forest
python -I tools/mappack/publish.py --all        # every game with map data
python -I tools/mappack/publish.py --all --dry-run   # build only, to check sizes
```

This zips everything under `games/<id>/` except `game.json` and uploads it to the
[`maps` release](https://github.com/HYPExMon5ter/game-maps/releases/tag/maps), then
points `maps.json` there at the new pack. Packs are built reproducibly, so a game
whose data hasn't changed is skipped and nobody downloads it again. The previous pack
is kept for apps that were mid-download, and older ones are deleted.

Each pack records the Steam build it was made from, read from the installed game, and its
pin counts. A category that lost more than 30% of its pins stops the publish, since that
usually means an update broke a generator; check it, or use `--force` if the drop is real.

To see which maps a game update has left behind:

```sh
python -I tools/mappack/outdated.py
```

`maps` is a pre-release on purpose. It must never become the "latest" release,
because that's where the app looks for its own updates.

## App updates

Merge your changes into `master`, then:

```sh
python -I tools/release.py                        # 0.1.0 -> 0.1.1
python -I tools/release.py minor "What changed"   # 0.1.0 -> 0.2.0
python -I tools/release.py 1.0.0 "What changed"   # an exact version
```

You can also go to **Actions → Release → Run workflow** on GitHub. The notes appear in
the release and in the app's update prompt.

The [Release workflow](../.github/workflows/release.yml) then:

1. runs the [CI checks](../.github/workflows/ci.yml) and, at the same time, sets the
   version (`tools/version.py`) and builds and signs the installer into a draft release
   with `latest.json`, which the app's updater reads,
2. once both succeed, commits the version to `master`, tags it, and publishes the
   release as the latest. If anything fails, the draft is deleted and nothing is
   published.

It takes about 15 minutes. The workflow only runs on `master`, and two releases never
run at once.

## Signing key

Installed apps only accept updates signed with this project's updater key. Its
public half is in `apps/overlay/src-tauri/tauri.conf.json`. The private key and
its password are stored as the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and in `~/.tauri/` on the machine that created
them.

**Keep a backup of `~/.tauri/game-maps.key` and its password.** If they're lost,
existing installs can't receive updates and everyone has to reinstall from the
releases page.

To set the secrets again (PowerShell):

```powershell
gh secret set TAURI_SIGNING_PRIVATE_KEY --body (Get-Content ~/.tauri/game-maps.key -Raw).Trim()
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --body (Get-Content ~/.tauri/game-maps.key.password -Raw).Trim()
```

## Building an installer locally

With the two variables above set in your environment:

```sh
cd apps/overlay
bun run app:build    # output: target/release/bundle/nsis/
```

To try a build on your own machine, skip the update files and you don't need the key:

```sh
bun run tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

Unlike development builds, these send usage counts if you choose Share, so they show up
in PostHog alongside real players.
