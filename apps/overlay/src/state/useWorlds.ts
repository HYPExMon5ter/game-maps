// Which world's data the map shows: the world the game reports, or (with no game
// running) the last world played, which the user can change in the world picker.

import { emit, listen } from '@tauri-apps/api/event'
import { useEffect } from 'react'
import { logUiError } from '../lib/errors'
import { loadWorldPins, type GameDefinition, type Pin } from '../lib/game'
import type { GameWorld } from '../lib/protocol'
import { IS_MAP_WINDOW } from '../lib/view'
import { adoptUnsaved, discardUnsavedWorlds, discardWorld, enterWorld, loadPrefs, loadSession, loadWorldData, loadWorldIndex, rememberLastWorld, userData } from './persist'
import { gameSettings } from '../settings/model'
import { useSettings } from '../settings/store'
import { dropPendingFog, flushFog, recordedFog } from './fog'
import { useStore, type GameUserData, type MapSession } from './store'
import { DEFAULT_WORLD, DEFAULT_WORLD_INFO, isUnsaved, type WorldIndex, type WorldInfo } from './worlds'

/** Everything the map needs to show one world of a game. */
export async function loadWorldView(
  game: GameDefinition,
  worldId?: string,
): Promise<{ user: GameUserData; world: WorldInfo; index: WorldIndex; session: MapSession; pins?: Pin[] | null }> {
  const gameId = game.id
  const index = await loadWorldIndex(gameId)
  const id = worldId ?? index.last ?? DEFAULT_WORLD
  const unsaved = isUnsaved(id) ? { id, name: null, kind: null, lastPlayed: 0 } : null
  const world = unsaved ?? index.worlds.find((w) => w.id === id) ?? DEFAULT_WORLD_INFO
  const [prefs, data, session, pins] = await Promise.all([
    loadPrefs(gameId),
    loadWorldData(gameId, world.id),
    loadSession(gameId, world.id),
    game.worldMaps ? loadWorldPins(gameId, world.id, gameSettings(useSettings.getState().ui, gameId).showSilver) : undefined,
  ])
  return { user: userData(prefs, data), world, index, session, pins }
}

// --- Either window: showing a world ------------------------------------------------------

/** Latest wins when several overlap. */
let viewing = 0

/** Shows a world of `game` as it's saved. Changes nothing on disk. */
async function viewWorld(game: GameDefinition, worldId: string): Promise<void> {
  const seq = ++viewing
  const view = await loadWorldView(game, worldId)
  if (seq === viewing && useStore.getState().game?.id === game.id) {
    useStore.getState().setWorld(view.world, view.index, view.user, view.session, view.pins)
  }
}

/** The overlay changed which world is shown, or what's recorded about it (see `useWorldTracking`). */
const ENTERED = 'world-entered'
/** The map window's world picker asks the overlay for a world. */
const PICK = 'world-pick'

interface WorldEvent {
  gameId: string
  worldId: string
}

const announce = (gameId: string, worldId: string) => void emit(ENTERED, { gameId, worldId } satisfies WorldEvent)

/** How long the overlay gets to answer the map window's picker, and how often it's asked. */
const PICK_WAIT_MS = 2000
const PICK_TRIES = 3

/** The world the map window's picker last asked for. */
let asked: string | null = null

/**
 * The world picker: shows another world of the current game, in both windows, and again
 * next time. Only the overlay writes which worlds there are, so the map window asks it.
 */
export function pickWorld(worldId: string): void {
  const game = useStore.getState().game
  if (!game) return
  if (!IS_MAP_WINDOW) return run(() => pick(game, worldId))
  asked = worldId
  void (async () => {
    // The overlay may be reloading (after a map update) and miss one.
    for (let i = 0; i < PICK_TRIES && asked === worldId && useStore.getState().world.id !== worldId; i++) {
      await emit(PICK, { gameId: game.id, worldId } satisfies WorldEvent)
      await new Promise((resolve) => setTimeout(resolve, PICK_WAIT_MS))
    }
  })()
}

// --- The overlay: recording worlds -----------------------------------------------------------
//
// Everything that writes which worlds there are, or moves a world's data, runs here, one
// step at a time in the order it was asked for: two at once would each write over what the
// other recorded.

let steps: Promise<unknown> = Promise.resolve()
let waiting = 0
/** The world the last step asked for leads to; `null` with none waiting (the shown one). */
let heading: string | null = null

function run(step: () => Promise<void>): void {
  waiting++
  steps = steps
    .then(step)
    .catch((e: unknown) => logUiError(`could not switch worlds: ${String(e)}`))
    .finally(() => {
      if (--waiting === 0) heading = null
    })
}

/** The game loaded `world`. */
async function enter(game: GameDefinition, world: GameWorld) {
  const shown = useStore.getState().world
  if (useStore.getState().game?.id !== game.id) return
  if (isUnsaved(shown.id) && shown.id !== world.id) {
    // Leaving a new game: its first save keeps what was recorded, otherwise it goes.
    // What was explored goes with it as it is now, even if it couldn't be saved just then.
    const fog = (await flushFog()) ? null : recordedFog(game.id, shown.id)
    // Its files are about to go: nothing may write them again afterwards.
    await dropPendingFog(game.id, shown.id)
    if (isUnsaved(world.id)) await discardWorld(game.id, shown.id)
    else await adoptUnsaved(game.id, shown.id, world.id, fog)
  }
  await enterWorld(game.id, world, await loadWorldIndex(game.id))
  if (!isUnsaved(world.id)) await discardUnsavedWorlds(game.id)
  await viewWorld(game, world.id)
  announce(game.id, world.id)
}

/** The shown world was renamed in the game. */
async function rename(game: GameDefinition, world: GameWorld) {
  const s = useStore.getState()
  if (s.game?.id !== game.id || s.world.id !== world.id || s.world.name === world.name) return
  const index = await enterWorld(game.id, world, await loadWorldIndex(game.id))
  // The map may show another game or world by now.
  const now = useStore.getState()
  if (now.game?.id !== game.id || now.world.id !== world.id) return
  useStore.setState({ world: { ...now.world, name: world.name } })
  now.setWorldIndex(index)
  announce(game.id, world.id)
}

async function pick(game: GameDefinition, worldId: string) {
  if (useStore.getState().game?.id !== game.id) return
  await rememberLastWorld(game.id, await loadWorldIndex(game.id), worldId)
  await viewWorld(game, worldId)
  announce(game.id, worldId)
}

/** Whether a new game was left without saving: checked again when its turn comes. */
function leftUnsaved(): GameDefinition | null {
  const now = useStore.getState()
  const state = now.status?.state
  const left = !now.snapshot?.world && isUnsaved(now.world.id) && (state === 'connected' || state === 'waiting_for_game')
  return left ? now.game : null
}

/** A new game was left without saving: it's gone, back to the last world. */
async function leaveUnsaved() {
  const game = leftUnsaved()
  if (!game) return
  const unsaved = useStore.getState().world.id
  const next = (await loadWorldIndex(game.id)).last ?? DEFAULT_WORLD
  await viewWorld(game, next)
  await dropPendingFog(game.id, unsaved)
  await discardWorld(game.id, unsaved)
  announce(game.id, next)
}

/** How long a new game's world may be gone before it counts as left without saving. */
const LEFT_WORLD_MS = 10_000

/**
 * Switches the map to the world the game reports whenever it changes. Only the overlay
 * (`lead`) records worlds (and moves data into the first one); the map window follows
 * once that's done.
 */
export function useWorldTracking(lead: boolean) {
  useEffect(() => {
    if (lead) return
    // The world the game reports, right away (possibly before the overlay moved data
    // into it), then again once the overlay has recorded it.
    const show = (game: GameDefinition, worldId: string) => {
      viewWorld(game, worldId).catch((e: unknown) => logUiError(`could not show the world: ${String(e)}`))
    }
    const unsubscribe = useStore.subscribe((s, prev) => {
      const world = s.snapshot?.world
      if (world && s.game && world !== prev.snapshot?.world && world.id !== s.world.id) show(s.game, world.id)
    })
    const unlisten = listen<WorldEvent>(ENTERED, ({ payload }) => {
      const { game, world } = useStore.getState()
      if (game?.id !== payload.gameId) return
      if (world.id !== payload.worldId) return show(game, payload.worldId)
      // Already shown (it was renamed, say): what's recorded about it, not the view the
      // player has moved since.
      loadWorldIndex(game.id)
        .then((index) => {
          const now = useStore.getState()
          if (now.game?.id !== game.id || now.world.id !== payload.worldId) return
          now.setWorldIndex(index)
          const recorded = index.worlds.find((w) => w.id === payload.worldId)
          if (recorded) useStore.setState({ world: recorded })
        })
        .catch((e: unknown) => logUiError(`could not load the worlds: ${String(e)}`))
    })
    return () => {
      unsubscribe()
      void unlisten.then((f) => f())
    }
  }, [lead])

  useEffect(() => {
    if (!lead) return
    let leaving: ReturnType<typeof setTimeout> | undefined
    const stop = useStore.subscribe((s, prev) => {
      const world = s.snapshot?.world
      if (world) {
        clearTimeout(leaving)
        leaving = undefined
      }
      if (!world && s.game && prev.snapshot?.world && isUnsaved(s.world.id)) {
        // Only if that's what happened: a moment without a world, a lost connection or the
        // player disconnecting the app mustn't delete a game they're still in.
        clearTimeout(leaving)
        leaving = setTimeout(() => leftUnsaved() && run(leaveUnsaved), LEFT_WORLD_MS)
        return
      }
      const game = s.game
      if (!world || !game || world === prev.snapshot?.world) return
      // Against where the map is heading, not only what it shows: going back to the shown
      // world while another is still loading has to be followed too.
      if (world.id !== (heading ?? s.world.id)) {
        heading = world.id
        run(() => enter(game, world))
      } else if (heading !== null || world.name !== s.world.name) {
        run(() => rename(game, world))
      }
    })
    const unlisten = listen<WorldEvent>(PICK, ({ payload }) => {
      const game = useStore.getState().game
      if (game?.id === payload.gameId) run(() => pick(game, payload.worldId))
    })
    return () => {
      clearTimeout(leaving)
      stop()
      void unlisten.then((f) => f())
    }
  }, [lead])
}
