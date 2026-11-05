// Which world's data the map shows: the world the game reports, or (with no game
// running) the last world played, which the user can change in the world picker.

import { emit, listen } from '@tauri-apps/api/event'
import { useEffect } from 'react'
import type { GameWorld } from '../lib/protocol'
import { adoptUnsaved, discardUnsavedWorlds, discardWorld, enterWorld, loadPrefs, loadSession, loadWorldData, loadWorldIndex, rememberLastWorld, userData } from './persist'
import { flushFog } from './fog'
import { useStore, type GameUserData, type MapSession } from './store'
import { DEFAULT_WORLD, DEFAULT_WORLD_INFO, isUnsaved, type WorldIndex, type WorldInfo } from './worlds'

/** Everything the map needs to show one world of a game. */
export async function loadWorldView(gameId: string, worldId?: string): Promise<{ user: GameUserData; world: WorldInfo; index: WorldIndex; session: MapSession }> {
  const index = await loadWorldIndex(gameId)
  const id = worldId ?? index.last ?? DEFAULT_WORLD
  const unsaved = isUnsaved(id) ? { id, name: null, kind: null, lastPlayed: 0 } : null
  const world = unsaved ?? index.worlds.find((w) => w.id === id) ?? DEFAULT_WORLD_INFO
  const [prefs, data, session] = await Promise.all([loadPrefs(gameId), loadWorldData(gameId, world.id), loadSession(gameId, world.id)])
  return { user: userData(prefs, data), world, index, session }
}

/** Latest switch wins when several overlap. */
let switching = 0

/**
 * Shows another world of the current game: from the world picker (`remember`: shown
 * again next time), or in the map window following the game.
 */
export async function showWorld(worldId: string, remember = true): Promise<void> {
  const game = useStore.getState().game
  if (!game || loading === worldId) return
  loading = worldId
  const seq = ++switching
  try {
    const view = await loadWorldView(game.id, worldId)
    const index = remember ? await rememberLastWorld(game.id, view.index, view.world.id) : view.index
    if (seq === switching && useStore.getState().game?.id === game.id) {
      useStore.getState().setWorld(view.world, index, view.user, view.session)
    }
  } finally {
    if (loading === worldId) loading = null
  }
}

/** The world `showWorld` is loading. */
let loading: string | null = null

/** The world being switched to (snapshots keep arriving meanwhile). */
let entering: string | null = null

async function followGameWorld(world: GameWorld) {
  const { game, worldIndex, world: shown } = useStore.getState()
  if (!game || entering === world.id) return
  entering = world.id
  const seq = ++switching
  try {
    if (isUnsaved(shown.id)) {
      // Leaving a new game: its first save keeps what was recorded, otherwise it goes.
      await flushFog()
      if (isUnsaved(world.id)) await discardWorld(game.id, shown.id)
      else await adoptUnsaved(game.id, shown.id, world.id)
    }
    const index = await enterWorld(game.id, world, worldIndex)
    if (!isUnsaved(world.id)) await discardUnsavedWorlds(game.id)
    const view = await loadWorldView(game.id, world.id)
    if (seq === switching && useStore.getState().game?.id === game.id) {
      useStore.getState().setWorld(view.world, index, view.user, view.session)
    }
    void emit(ENTERED, { gameId: game.id, worldId: world.id } satisfies Entered)
  } finally {
    entering = null
  }
}

async function leaveUnsaved(gameId: string, unsaved: string, next: string) {
  await flushFog()
  await showWorld(next, false)
  await discardWorld(gameId, unsaved)
  void emit(ENTERED, { gameId, worldId: next } satisfies Entered)
}

/** How long a new game's world may be gone before it counts as left without saving. */
const LEFT_WORLD_MS = 10_000

/** The overlay recorded a newly loaded world (see `useWorldTracking`). */
const ENTERED = 'world-entered'

interface Entered {
  gameId: string
  worldId: string
}

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
    const unsubscribe = useStore.subscribe((s, prev) => {
      const world = s.snapshot?.world
      if (world && s.game && world !== prev.snapshot?.world && world.id !== s.world.id) void showWorld(world.id, false)
    })
    const unlisten = listen<Entered>(ENTERED, ({ payload }) => {
      if (useStore.getState().game?.id === payload.gameId) void showWorld(payload.worldId, false)
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
        // Left a new game without saving: it's gone, back to the last world. Only if that's
        // what happened: a moment without a world, a lost connection or the player
        // disconnecting the app mustn't delete a game they're still in.
        clearTimeout(leaving)
        leaving = setTimeout(() => {
          const now = useStore.getState()
          const state = now.status?.state
          if (now.snapshot?.world || !now.game || !isUnsaved(now.world.id) || (state !== 'connected' && state !== 'waiting_for_game')) return
          void leaveUnsaved(now.game.id, now.world.id, now.worldIndex.last ?? DEFAULT_WORLD)
        }, LEFT_WORLD_MS)
        return
      }
      if (!world || !s.game || world === prev.snapshot?.world) return
      if (world.id === s.world.id && world.name === s.world.name) return
      if (world.id === s.world.id) {
        // Same world, new name (renamed in the game): just record it.
        useStore.setState({ world: { ...s.world, name: world.name } })
        void enterWorld(s.game.id, world, s.worldIndex).then((index) => useStore.getState().setWorldIndex(index))
        return
      }
      void followGameWorld(world)
    })
    return () => {
      clearTimeout(leaving)
      stop()
    }
  }, [lead])
}
