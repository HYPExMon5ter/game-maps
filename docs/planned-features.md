# Planned features

Ideas for Game Maps that aren't built yet, split into features for every game and
features for one game. Notes on effort and approach are rough; anything that runs in
the game is measured before it ships (see the frame-time budget in
`docs/architecture.md`).

Status: **Built** (in the next release), **Planned**, **Idea** (worth doing, not designed
yet), **Deferred** (wanted, waiting on something).

## Every game

### Map and navigation

| Feature | Status | Notes |
|---|---|---|
| Per-world data | Built | The agent reports which world or save is loaded (single player, hosting, or joining someone); found items, completion, fog of war, custom pins and the map state are kept per world. Games without detection use one world per game. |
| Fog of war | Built | Only what you've explored is revealed: unexplored land is covered and game pins there are hidden (each can be turned off). Recorded from position updates; a bitmap per world. |
| Completion tracker | Built | Progress per collectible category with what's left, per world. |
| Map on a second monitor | Built | An always-on, interactive map window on a chosen monitor that follows you live while you play on the main screen. |
| Path history | Idea | A trail of where you walked this session ("where was I five minutes ago"). Shares the recording with fog of war. |
| Death marker | Idea | A pin where you last died, to recover your gear. Death detection is per game; the pin is shared code. |
| Collection routes | Idea | Visit what's left in a category in an efficient order (nearest-neighbor tour over the route planner). |
| Measure tool | Idea | Right-click to measure straight-line and walking distance between points. |
| Map window follows the game's screen | Idea | Put the map window on whichever monitor the game isn't on, when the game moves. |
| Corner-drag resize | Deferred | Resize the full map and mini-map by dragging their corners. |
| Mini-map edit mode | Deferred | Place the mini-map anywhere on screen, not just in a corner. |

### Pins and progress

| Feature | Status | Notes |
|---|---|---|
| Notes and screenshots on pins | Idea | Free text and an image per pin ("needs the shovel"). |
| Pin timers | Idea | A countdown on a pin (resource respawns, crops), with a notice when it's up. |
| Import and export pins | Idea | Share a pin set as a file or link, outside multiplayer. |
| Manage worlds | Idea | Rename, merge or delete a world's data from the world picker. |
| Copy pins between worlds | Planned | Comes with per-world data: copy a custom pin to every world, or to one. |

### Project

| Feature | Status | Notes |
|---|---|---|
| Usage analytics | Deferred | See how many people use the app and what they play, with no paid hosting: anonymous events to a free service (PostHog's free tier looks like the best fit: 1M events a month, unique users by a random install ID; Aptabase is an alternative at 20k events), plus GitHub release download counts for the app and each map pack. Needs a switch to turn it off per user. |

### While playing

| Feature | Status | Notes |
|---|---|---|
| Danger alert | Built | Edge-of-screen indicators toward hostile creatures within a set distance (games with creature tracking). |
| Pings in the game | Built | Friends' map pings drawn over the game like the waypoint, for 30 seconds. |
| Quick-mark key | Idea | A key that drops a pin where you stand without opening the map. |
| Streamer mode | Idea | Hide coordinates, session details and player names when capturing. |
| Controller support | Idea | Open, pan and zoom the map with a gamepad. |

### Playing together

| Feature | Status | Notes |
|---|---|---|
| Shared fog of war and found items | Idea | Optionally merge friends' explored areas and found collectibles in the same world. |
| Relay for strict NATs | Idea | A free TURN option for players who can't connect directly. |

## Sons of the Forest

| Feature | Status | Notes |
|---|---|---|
| World detection | Built | `GameSetupManager.GetSelectedSaveId()` and `GetSaveGameType()` (single player, hosting, client). |
| Companion status | Idea | Kelvin's and Virginia's health and whereabouts; possibly giving orders. |
| "Where's my stuff?" | Idea | Search the contents of your storage and get the container on the map. Needs research into how storage is saved and loaded. |
| Progress checklist | Idea | Keycards, shovel, rebreather, golden armor and other key items, linked to their pins. |
| Time, day and weather | Idea | Readout of the in-game time, day count, season and weather. |
| Your structures | Idea | Mark the bases and shelters you've built automatically. |

## Sunkenland

| Feature | Status | Notes |
|---|---|---|
| World detection | Built | `Mainframe.SaveManager.CurrentWorldGuid` / `CurrentWorldName`; clients use the world identifier from the session. |
| Creature tracking | Built | Nearby enemies, enemy ships, sharks, animals and bosses on the map, with the danger alert. Ordinary enemies show as "Enemy": their faction's name is localized and not read yet. |
| Playing together | Built | Sessions come from Photon Fusion; not tested with a second player yet. |
| Boat and base markers | Idea | Your boat and bases on the map. |
| Depth and oxygen | Idea | A readout while diving. |
| Cleared areas | Idea | Wrecks and loot areas you've emptied, like collectibles marked found. |
