// Generated worlds (`GameDefinition.worldMaps`): their maps are made from the running game
// (`src-tauri/src/worldmaps.rs`), and the game's map can carry the player's pins.

import { useEffect } from 'react'
import { DEATH_PIN } from '../lib/actions'
import { backend, events, type WorldMapReady } from '../lib/backend'
import { logUiError } from '../lib/errors'
import { gamePinsOf } from '../lib/gamePins'
import { loadWorldPins, worldMapBase } from '../lib/game'
import { gameSettings } from '../settings/model'
import { useGameHud, useGameSetting, useSettings } from '../settings/store'
import { mergeGameExplored, useFogStore } from './fog'
import { useStore } from './store'
import { keys } from './worlds'
import { discoveredPins, enteredDungeon, regrowthTimers } from '../lib/worldExtras'
import type { Vec3 } from '../lib/protocol'

/** A new world's map can arrive while the map is still switching to that world. */
const SWITCH_WAIT_MS = 500
const SWITCH_TRIES = 20

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

async function waitFor(ready: () => boolean): Promise<boolean> {
  for (let i = 0; i < SWITCH_TRIES && !ready(); i++) await sleep(SWITCH_WAIT_MS)
  return ready()
}

async function showWorldMap({ game_id: gameId, world_id: worldId, tiles, explored }: WorldMapReady, record: boolean) {
  const shown = () => {
    const s = useStore.getState()
    return s.game?.id === gameId && s.world.id === worldId
  }
  if (!(await waitFor(shown))) return
  const pins = await loadWorldPins(gameId, worldId, gameSettings(useSettings.getState().ui, gameId).showSilver)
  if (pins && shown()) useStore.getState().setWorldMap(pins, tiles)
  if (!record || !explored) return
  // What the player explored before Game Maps, or without it running.
  const key = keys.fog(gameId, worldId)
  if (!(await waitFor(() => useFogStore.getState().key === key && !!useFogStore.getState().fog))) return
  const res = await fetch(`${worldMapBase(gameId, worldId)}/explored.bin`)
  if (res.ok) mergeGameExplored(new Uint8Array(await res.arrayBuffer()))
}

/** Shows a world's map when it's ready. Only the overlay (`record`) saves what's explored. */
export function useWorldMaps(record: boolean) {
  useHiddenPins()
  useEffect(() => {
    const unlisten = events.worldMapReady((ready) => {
      showWorldMap(ready, record).catch((e: unknown) => logUiError(`could not show the world's map: ${String(e)}`))
    })
    return () => void unlisten.then((f) => f())
  }, [record])
}

/** Shows or hides silver not found with a wishbone as the option changes. */
function useHiddenPins() {
  const worldMaps = useStore((s) => !!s.game?.worldMaps)
  const gameId = useStore((s) => s.game?.id)
  const worldId = useStore((s) => s.world.id)
  const showSilver = useGameSetting(gameId, 'showSilver')
  useEffect(() => {
    if (!worldMaps || !gameId) return
    let current = true
    loadWorldPins(gameId, worldId, showSilver)
      .then((pins) => {
        if (pins && current) useStore.getState().setWorldMap(pins, false)
      })
      .catch((e: unknown) => logUiError(`could not load the world's pins: ${String(e)}`))
    return () => {
      current = false
    }
  }, [worldMaps, gameId, worldId, showSilver])
}

/** Starts a pin's timer when what's on it was picked and is growing back. */
function useRegrowthTimers() {
  const gameId = useStore((s) => s.game?.id)
  const enabled = useGameSetting(gameId, 'resourceTimers')
  const pins = useStore((s) => s.gamePins)
  useEffect(() => {
    if (!enabled) return
    const { user, setPinTimers } = useStore.getState()
    const started = regrowthTimers(pins, user.timers, Date.now())
    if (started) setPinTimers(started)
  }, [enabled, pins])
}

/** Tells the agent the options it acts on. */
function useGameOptions() {
  const gameId = useStore((s) => s.game?.id)
  const connected = useStore((s) => s.status?.state === 'connected')
  const teleportRestricted = useGameSetting(gameId, 'teleportRestricted')
  const hideGameMinimap = useGameHud(gameId).minimap
  useEffect(() => {
    if (!connected) return
    backend.setGameOptions({ teleportRestricted, hideGameMinimap }).catch((e: unknown) => logUiError(`could not set the game's options: ${String(e)}`))
  }, [connected, teleportRestricted, hideGameMinimap])
}

/** Marks a dungeon found when the player goes inside it. */
function useClearedDungeons() {
  useEffect(() => {
    let surface: Vec3 | null = null
    return useStore.subscribe((s, prev) => {
      const snapshot = s.snapshot
      if (snapshot === prev.snapshot || !snapshot?.local || !s.game) return
      if (!snapshot.indoors) {
        surface = snapshot.local.position
        return
      }
      if (prev.snapshot?.indoors) return
      const categories = new Set(s.game.categories.filter((c) => c.visitInside).map((c) => c.id))
      const pin = enteredDungeon(s.gamePins.filter((p) => categories.has(p.category)), surface, snapshot.local.position)
      if (pin) s.markFound([pin.id])
    })
  }, [])
}

/** Marks places found when the game discovers them. */
function useDiscoveredPlaces() {
  useEffect(() => {
    return useStore.subscribe((s, prev) => {
      const visited = s.snapshot?.visited
      if (!visited?.length || !s.game || visited.length === prev.snapshot?.visited?.length) return
      const categories = new Set(s.game.categories.filter((c) => c.visitInside).map((c) => c.id))
      const known = new Set(s.user.found)
      const pins = discoveredPins(s.gamePins.filter((p) => categories.has(p.category)), visited).filter((p) => !known.has(p.id))
      if (pins.length) s.markFound(pins.map((p) => p.id))
    })
  }, [])
}

/** What only the overlay does, for what games report about the world. */
export function useWorldExtras() {
  useRegrowthTimers()
  useGameOptions()
  useClearedDungeons()
  useDiscoveredPlaces()
}

/** Keeps the game's own map in line with the player's pins in the world they're in. */
export function useGamePins() {
  const gameId = useStore((s) => s.game?.id)
  const enabled = useGameSetting(gameId, 'gameMap')
  const inWorld = useStore((s) => !!s.status?.capabilities?.game_map && !!s.snapshot?.world && s.snapshot.world.id === s.world.id)
  const pins = useStore((s) => s.user.customPins)
  useEffect(() => {
    if (!enabled || !inWorld) return
    backend.setGamePins(gamePinsOf(pins, DEATH_PIN)).catch((e: unknown) => logUiError(`could not update the game's map: ${String(e)}`))
  }, [enabled, inWorld, pins])
}
