<div align="center">

<img src="assets/icon.svg" width="88" height="88" alt="Game Maps logo">

# Game Maps

**An in-game map overlay for PC games: searchable locations, your live position,
other players, and teleport. Press a key and it's there.**

[![Latest release](https://img.shields.io/github/v/release/HYPExMon5ter/game-maps?label=download&color=ffd166)](https://github.com/HYPExMon5ter/game-maps/releases/latest)
[![CI](https://github.com/HYPExMon5ter/game-maps/actions/workflows/ci.yml/badge.svg)](https://github.com/HYPExMon5ter/game-maps/actions/workflows/ci.yml)
![Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078d4)

<img src="docs/images/map.png" alt="The map overlay in Sons of the Forest" width="900">

</div>

## Features

- **Interactive map** of the whole world, built from the game's own files, with every
  cave, bunker, weapon, tool, key item and collectible pinned. Search and filter
  by category, and mark pins as found.
- **Live location** of you, other players in multiplayer, and everything the in-game
  GPS shows (companions, placed locators, moved vehicles).
- **Teleport** anywhere on the map, to any pin, or to another player, including into
  caves and bunkers.
- **Track** a pin or player with an on-screen compass, and keep a click-through
  **mini-map** in a corner while you play.
- **Custom pins** with your own names, colors and icons.
- **No mod loader and no game files changed.** Works whether you host or join.
- **Always current.** The app and its maps update themselves.

## Supported games

| Game | Map | Live location | Teleport | Other players |
|---|:---:|:---:|:---:|:---:|
| Sons of the Forest | ✓ | ✓ | ✓ | ✓ |

More games can be added; see [Adding a game](docs/development.md#adding-a-game).

## Install

1. Download **`Game.Maps_x.y.z_x64-setup.exe`** from the
   [latest release](https://github.com/HYPExMon5ter/game-maps/releases/latest) and run it.
   No administrator rights are needed.
2. If Windows shows *"Windows protected your PC"*, click **More info**, then
   **Run anyway**. The installer isn't code-signed yet.

That's it. Game Maps downloads each game's map by itself and keeps both the app and
the maps up to date.

## Quick start

1. In the game's display settings, set the display mode to **Fullscreen Window**
   (borderless). The map is drawn over the game and can't appear over exclusive
   fullscreen.
2. Open **Game Maps** and click **Play**, or start the game as usual.
3. In game, press **M** to open or close the map.

<img src="docs/images/launcher.png" alt="The Game Maps window" width="420" align="right">

The **Game Maps window** lists your games, one line each, with their status. Search
the list, or show only games with live features (**Live**) or without (**Map only**).
On each game:

- **Play:** start the game through Steam. **Connect** attaches to it when it's running.
- **Map** (map icon): open the map without the game.
- The **arrow** shows more options:
  - **Connect to game:** *Automatically* (as soon as it starts) or *Manually* (only
    when you click Connect).
  - **Key to open the map:** pick any key or combination. You can also change it from
    the map's settings.
  - **Map data:** whether the map is downloaded and up to date.

The **gear** has **Start with Windows**, **Install updates automatically**, and update
and log buttons.

Closing the window keeps Game Maps running in the system tray. Quit it from the tray
icon.

<br clear="right">

## Using the map

| Action | How |
|---|---|
| Open / close the map | **M** (or your key) in game; **Esc** also closes it |
| Zoom | Mouse wheel, or **Num +** / **Num -** (also **=** / **-**). The keys zoom the mini-map while you play too |
| Search | **/** or **Ctrl+F** |
| Add a pin, teleport, copy coordinates | Right-click the map |
| Track, teleport to, or mark a pin found | Click the pin |
| Switch between surface and caves | **Surface / Underground** in the top bar (follows you automatically) |
| Find other players and companions | **Players** in the top bar |
| Mini-map while playing | **Picture-in-picture** in the top bar |
| Opacity, size, mini-map size and corner, zoom keys | **Settings** (gear icon) |

The map and zoom keys still reach the game, so the in-game GPS keeps working. Each
zoom key can be changed or turned off in **Settings**.

## Updates

Game Maps checks for new versions and new map data every 30 minutes and whenever a
game starts.

- **Maps** update on their own. A map that's open on screen switches over when you
  close it.
- **The app** installs new versions on its own as soon as no game is running. If you
  turn off *Install updates automatically*, it asks first instead.

## Troubleshooting

<details>
<summary><b>The map doesn't appear when I press M</b></summary>

- Check that the game uses **Fullscreen Window** (borderless). The Game Maps window
  warns you if it detects exclusive fullscreen.
- Check the game's status in the Game Maps window. It should say **Connected**. If
  hooking is set to *Manually*, click **Connect**.
- If you changed the key, use the one shown under *Key to open the map*.
</details>

<details>
<summary><b>"Game Maps can't connect" or it says the game runs as administrator</b></summary>

Game Maps can't attach to a game that runs with more rights than it has. Quit Game
Maps from the tray, then right-click it and choose **Run as administrator**, or stop
running the game as administrator.
</details>

<details>
<summary><b>The map is empty</b></summary>

The map downloads the first time it's needed. Check the **Map data** line in the
Game Maps window, which shows progress or any error. You need to be online for the
first download.
</details>

<details>
<summary><b>My antivirus flagged Game Maps</b></summary>

Game Maps attaches to the running game to read your position, the same way other
map and trainer tools do, and some antivirus products flag that behavior. The full
source code is in this repository, and every release is built from it by
[GitHub Actions](https://github.com/HYPExMon5ter/game-maps/actions/workflows/release.yml).
</details>

<details>
<summary><b>Reporting a problem</b></summary>

[Open an issue](https://github.com/HYPExMon5ter/game-maps/issues/new/choose) and
attach the log from **Open logs** in the Game Maps window.
</details>

## How it works

Game Maps is a [Tauri](https://tauri.app) app (Rust + React +
[MapLibre](https://maplibre.org)). When a supported game starts, it loads a small
Rust agent into the game process. The agent reads the player's position through
Unity's IL2CPP runtime and streams it to the app over a local named pipe. The map
is a separate transparent window drawn over the game, so it costs the game almost
nothing to render.

Map tiles, icons and pins are generated from the game's own files, so they line up
exactly with live positions. They're published separately from the app, so map
fixes reach everyone without an app update.

More detail: [Architecture](docs/architecture.md) ·
[Development](docs/development.md) · [Releasing](docs/releasing.md)

## Disclaimer

Game Maps is a fan project. It isn't affiliated with or endorsed by the developers
or publishers of the supported games, and all game names and assets belong to their
owners. Use it in single player or with friends who are fine with it. Teleporting in
someone else's multiplayer session changes their game too.
