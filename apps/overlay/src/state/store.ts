import { create } from 'zustand'
import type { FilterSet } from '../lib/filterSets'
import type { Category, GameDefinition, GameMap, Pin } from '../lib/game'
import type { LinkStatus, OverlayMode, PathInfo, Vec3, WorldLayer, WorldSnapshot } from '../lib/protocol'
import type { PinTimers } from '../lib/timers'
import { DEFAULT_WORLD_INFO, EMPTY_INDEX, type WorldIndex, type WorldInfo } from './worlds'

export interface GameUserData {
  customPins: Pin[]
  found: string[]
  hiddenCategories: string[]
  shownCategories: string[]
  /** Saved category filters, the same in every world. */
  filterSets: FilterSet[]
  /** The player's notes, by pin id (any pin, not just their own). */
  notes: Record<string, string>
  /** When each pin's countdown ends (ms since the epoch), by pin id. */
  timers: PinTimers
}

/**
 * A pin, a live entity (player, companion, live marker; see `lib/movers`), or a spot
 * someone asked you to track.
 */
export type TrackTarget =
  | { kind: 'pin'; id: string }
  | { kind: 'live'; id: string }
  | { kind: 'point'; name: string; position: Vec3; layer?: WorldLayer }

/** Where the full map was looking. */
export interface SavedMapView {
  center: [number, number]
  zoom: number
}

/**
 * The map as it was left, per game: restored whenever that game's map loads (app
 * start, game start, reconnects), so nothing is lost between sessions.
 */
export interface MapSession {
  tracked: TrackTarget | null
  selectedPinId: string | null
  selectedLiveId: string | null
  followId: string | null
  /** The full map keeps the player centered. */
  followPlayer: boolean
  viewLayerOverride: WorldLayer | null
  query: string
  showFound: boolean
  mapView: SavedMapView | null
}

export const EMPTY_SESSION: MapSession = {
  tracked: null,
  selectedPinId: null,
  selectedLiveId: null,
  followId: null,
  followPlayer: false,
  viewLayerOverride: null,
  query: '',
  showFound: true,
  mapView: null,
}

/** The current map session (see `MapSession`). */
export function selectSession(s: MapSession): MapSession {
  return {
    tracked: s.tracked,
    selectedPinId: s.selectedPinId,
    selectedLiveId: s.selectedLiveId,
    followId: s.followId,
    followPlayer: s.followPlayer,
    viewLayerOverride: s.viewLayerOverride,
    query: s.query,
    showFound: s.showFound,
    mapView: s.mapView,
  }
}

export const EMPTY_USER_DATA: GameUserData = { customPins: [], found: [], hiddenCategories: [], shownCategories: [], filterSets: [], notes: {}, timers: {} }

interface AppState {
  status: LinkStatus | null
  mode: OverlayMode
  snapshot: WorldSnapshot | null
  game: GameDefinition | null
  map: GameMap | null
  gamePins: Pin[]
  /** Pins other players in the session share (see `lib/sync`). */
  friendPins: Pin[]
  /** Category filters for the game plus the current world's pins and found items. */
  user: GameUserData
  /** The world whose data is shown (see `state/worlds`). */
  world: WorldInfo
  /** The game's known worlds. */
  worldIndex: WorldIndex
  /** The shown generated world has a map (see `GameDefinition.worldMaps`). */
  worldMapReady: boolean
  /** Bumped when a generated world's map files change. */
  worldMapVersion: number
  /** How many teleports "teleport back" can undo. */
  backSteps: number
  /** Walking route to the tracked target, refreshed while tracking. */
  route: PathInfo | null
  selectedPinId: string | null
  /** Selected live entity (its details card is open). */
  selectedLiveId: string | null
  /** Live entity the full map keeps centered on (excludes `followPlayer`). */
  followId: string | null
  followPlayer: boolean
  tracked: TrackTarget | null
  query: string
  showFound: boolean
  /** Layer the map shows; null follows the player's current layer. */
  viewLayerOverride: WorldLayer | null
  /** Where the full map was last looking (restored when it opens). */
  mapView: SavedMapView | null
  toast: { id: number; text: string; tone: 'info' | 'error' } | null
  /** Moving and resizing the map and mini-map (see `ui/LayoutEditor`). */
  editingLayout: boolean

  setEditingLayout(editing: boolean): void
  setStatus(s: LinkStatus): void
  setMode(m: OverlayMode): void
  setSnapshot(s: WorldSnapshot | null): void
  setGame(game: GameDefinition, pins: Pin[], user: GameUserData, world: WorldInfo, index: WorldIndex, session: MapSession, worldMapReady?: boolean): void
  /** Shows another world of the current game. */
  /** `worldPins`: for generated worlds, the world's pins (null if it has no map yet). */
  setWorld(world: WorldInfo, index: WorldIndex, user: GameUserData, session: MapSession, worldPins?: Pin[] | null): void
  /** A generated world's map files changed; `tiles` if not only its pins did. */
  setWorldMap(pins: Pin[], tiles: boolean): void
  setWorldIndex(index: WorldIndex): void
  setBackSteps(steps: number): void
  setRoute(route: PathInfo | null): void
  selectPin(id: string | null): void
  selectLive(id: string | null): void
  setFollow(id: string | null): void
  setFollowPlayer(follow: boolean): void
  setTracked(t: TrackTarget | null): void
  setQuery(q: string): void
  setShowFound(v: boolean): void
  setViewLayer(layer: WorldLayer | null): void
  setFriendPins(pins: Pin[]): void
  setMapView(view: SavedMapView): void
  toggleCategory(id: string): void
  setAllCategories(visible: boolean): void
  toggleFound(id: string): void
  markFound(ids: string[]): void
  upsertCustomPin(pin: Pin): void
  deleteCustomPin(id: string): void
  /** Sets a pin's note; an empty one removes it. */
  setPinNote(id: string, note: string): void
  /** Starts a pin's countdown, or stops it (`null`). */
  setPinTimer(id: string, endsAt: number | null): void
  setPinTimers(timers: PinTimers): void
  notify(text: string, tone?: 'info' | 'error'): void
  dismissToast(id: number): void
}

let toastSeq = 0

export const useStore = create<AppState>((set) => ({
  status: null,
  mode: 'hidden',
  snapshot: null,
  game: null,
  map: null,
  gamePins: [],
  friendPins: [],
  user: EMPTY_USER_DATA,
  world: DEFAULT_WORLD_INFO,
  worldIndex: EMPTY_INDEX,
  worldMapReady: false,
  worldMapVersion: 0,
  backSteps: 0,
  route: null,
  selectedPinId: null,
  selectedLiveId: null,
  followId: null,
  followPlayer: false,
  tracked: null,
  query: '',
  showFound: true,
  viewLayerOverride: null,
  mapView: null,
  toast: null,
  editingLayout: false,

  setEditingLayout: (editingLayout) => set({ editingLayout }),
  setStatus: (status) => set({ status }),
  setMode: (mode) => set({ mode }),
  // A pinned surface/underground view lasts until the player changes layer themselves
  // (enters or leaves a cave); then the map follows them again.
  setSnapshot: (snapshot) =>
    set((s) => {
      const layer = snapshot?.local?.layer
      const changed = layer !== undefined && s.snapshot?.local?.layer !== undefined && layer !== s.snapshot.local.layer
      return { snapshot, viewLayerOverride: changed ? null : s.viewLayerOverride }
    }),
  setGame: (game, gamePins, user, world, worldIndex, session, worldMapReady = false) =>
    set({ game, map: game.maps[0] ?? null, gamePins, worldMapReady, user, world, worldIndex, route: null, ...session }),
  setWorld: (world, worldIndex, user, session, worldPins) =>
    set({
      world,
      worldIndex,
      user,
      route: null,
      ...session,
      ...(worldPins === undefined ? {} : { gamePins: worldPins ?? [], worldMapReady: worldPins !== null }),
    }),
  setWorldMap: (gamePins, tiles) => set((s) => ({ gamePins, worldMapReady: true, worldMapVersion: s.worldMapVersion + (tiles ? 1 : 0) })),
  setWorldIndex: (worldIndex) => set({ worldIndex }),
  setBackSteps: (backSteps) => set({ backSteps }),
  setRoute: (route) => set({ route }),
  selectPin: (selectedPinId) => set((s) => ({ selectedPinId, selectedLiveId: selectedPinId ? null : s.selectedLiveId })),
  selectLive: (selectedLiveId) => set((s) => ({ selectedLiveId, selectedPinId: selectedLiveId ? null : s.selectedPinId })),
  // Following yourself and following someone else exclude each other.
  setFollow: (followId) => set((s) => ({ followId, followPlayer: followId ? false : s.followPlayer })),
  setFollowPlayer: (followPlayer) => set((s) => ({ followPlayer, followId: followPlayer ? null : s.followId })),
  setTracked: (tracked) => set({ tracked, route: null }),
  setQuery: (query) => set({ query }),
  setShowFound: (showFound) => set({ showFound }),
  setViewLayer: (viewLayerOverride) => set({ viewLayerOverride }),
  setMapView: (mapView) => set({ mapView }),
  setFriendPins: (friendPins) => set({ friendPins }),
  toggleCategory: (id) =>
    set((s) => {
      const visible = isCategoryVisible(s.game?.categories.find((c) => c.id === id), s.user)
      const hidden = new Set(s.user.hiddenCategories)
      const shown = new Set(s.user.shownCategories)
      if (visible) {
        hidden.add(id)
        shown.delete(id)
      } else {
        hidden.delete(id)
        shown.add(id)
      }
      return { user: { ...s.user, hiddenCategories: [...hidden], shownCategories: [...shown] } }
    }),
  setAllCategories: (visible) =>
    set((s) => {
      const ids = s.game?.categories.map((c) => c.id) ?? []
      return {
        user: { ...s.user, hiddenCategories: visible ? [] : ids, shownCategories: visible ? ids : [] },
      }
    }),
  markFound: (ids) =>
    set((s) => {
      const known = new Set(s.user.found)
      const added = ids.filter((id) => !known.has(id))
      // Nothing new: no change, so nothing to save or send.
      return added.length ? { user: { ...s.user, found: [...s.user.found, ...new Set(added)] } } : s
    }),
  toggleFound: (id) =>
    set((s) => {
      const found = new Set(s.user.found)
      if (found.has(id)) found.delete(id)
      else found.add(id)
      return { user: { ...s.user, found: [...found] } }
    }),
  upsertCustomPin: (pin) =>
    set((s) => {
      const rest = s.user.customPins.filter((p) => p.id !== pin.id)
      return { user: { ...s.user, customPins: [...rest, { ...pin, custom: true }] } }
    }),
  deleteCustomPin: (id) =>
    set((s) => ({
      user: { ...s.user, customPins: s.user.customPins.filter((p) => p.id !== id) },
      selectedPinId: s.selectedPinId === id ? null : s.selectedPinId,
      tracked: s.tracked?.kind === 'pin' && s.tracked.id === id ? null : s.tracked,
    })),
  setPinNote: (id, note) =>
    set((s) => {
      const { [id]: _old, ...rest } = s.user.notes
      return { user: { ...s.user, notes: note ? { ...rest, [id]: note } : rest } }
    }),
  setPinTimer: (id, endsAt) =>
    set((s) => {
      const { [id]: _old, ...rest } = s.user.timers
      return { user: { ...s.user, timers: endsAt === null ? rest : { ...rest, [id]: endsAt } } }
    }),
  setPinTimers: (timers) => set((s) => ({ user: { ...s.user, timers } })),
  notify: (text, tone = 'info') => set({ toast: { id: ++toastSeq, text, tone } }),
  dismissToast: (id) => set((s) => (s.toast?.id === id ? { toast: null } : {})),
}))

export function isCategoryVisible(category: Category | undefined, user: Pick<GameUserData, 'hiddenCategories' | 'shownCategories'>): boolean {
  if (!category) return true
  if (user.hiddenCategories.includes(category.id)) return false
  if (user.shownCategories.includes(category.id)) return true
  return category.visible
}

export function selectViewLayer(s: Pick<AppState, 'viewLayerOverride' | 'snapshot'>): WorldLayer {
  return s.viewLayerOverride ?? (s.snapshot?.local?.layer === 'caves' ? 'caves' : 'surface')
}

/** Game pins plus the user's custom pins. */
export function selectAllPins(s: Pick<AppState, 'gamePins' | 'user' | 'friendPins'>): Pin[] {
  if (!s.user.customPins.length && !s.friendPins.length) return s.gamePins
  // Asked for several times per snapshot while tracking; the lists rarely change.
  const last = mergedPins
  if (last && last.game === s.gamePins && last.custom === s.user.customPins && last.friends === s.friendPins) return last.all
  const all = [...s.gamePins, ...s.user.customPins, ...s.friendPins]
  mergedPins = { game: s.gamePins, custom: s.user.customPins, friends: s.friendPins, all }
  return all
}

let mergedPins: { game: Pin[]; custom: Pin[]; friends: Pin[]; all: Pin[] } | null = null
