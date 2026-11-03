import { create } from 'zustand'
import type { ZoomKeys } from '../lib/backend'
import type { Category, GameDefinition, GameMap, Pin } from '../lib/game'
import type { Corner, Hotkey, LinkStatus, OverlayMode, WorldLayer, WorldSnapshot } from '../lib/protocol'

export interface HudSettings {
  /** Keep a mini-map on screen while playing. */
  minimap: boolean
  corner: Corner
  /** Mini-map size in logical pixels (capped at half the screen, see `hudSize`). */
  width: number
  height: number
  /** Mini-map zoom in levels relative to the default (about 1 screen pixel per world unit). */
  zoom: number
  opacity: number
  /** Rotate the mini-map so the player's view direction points up. */
  rotateWithPlayer: boolean
}

export interface Settings {
  /** Opacity of the full map (the game shows through below 1). */
  opacity: number
  /** Fraction of the screen the full map panel uses. */
  panelScale: number
  /** Interface scale; null picks one from the screen size. */
  uiScale: number | null
  followPlayer: boolean
  hud: HudSettings
}

export interface GameUserData {
  customPins: Pin[]
  found: string[]
  hiddenCategories: string[]
  shownCategories: string[]
}

export type TrackTarget = { kind: 'pin'; id: string } | { kind: 'player'; id: string }

export const DEFAULT_SETTINGS: Settings = {
  opacity: 0.96,
  panelScale: 1,
  uiScale: null,
  followPlayer: false,
  hud: { minimap: false, corner: 'top_right', width: 300, height: 300, zoom: 0, opacity: 0.92, rotateWithPlayer: false },
}

export const EMPTY_USER_DATA: GameUserData = { customPins: [], found: [], hiddenCategories: [], shownCategories: [] }

interface AppState {
  status: LinkStatus | null
  mode: OverlayMode
  snapshot: WorldSnapshot | null
  game: GameDefinition | null
  map: GameMap | null
  gamePins: Pin[]
  user: GameUserData
  settings: Settings
  /** Per-game map hotkey overrides (owned by the backend, see `set_map_hotkey`). */
  mapHotkeys: Record<string, Hotkey>
  /** Map zoom keys for all games (owned by the backend, see `set_zoom_keys`). */
  zoomKeys: ZoomKeys | null
  selectedPinId: string | null
  tracked: TrackTarget | null
  query: string
  showFound: boolean
  /** Layer the map shows; null follows the player's current layer. */
  viewLayerOverride: WorldLayer | null
  toast: { id: number; text: string; tone: 'info' | 'error' } | null

  setStatus(s: LinkStatus): void
  setMode(m: OverlayMode): void
  setSnapshot(s: WorldSnapshot | null): void
  setGame(game: GameDefinition, pins: Pin[], user: GameUserData): void
  setSettings(patch: Partial<Settings>): void
  setMapHotkeys(mapHotkeys: Record<string, Hotkey>): void
  setZoomKeys(zoomKeys: ZoomKeys): void
  setHud(patch: Partial<HudSettings>): void
  selectPin(id: string | null): void
  setTracked(t: TrackTarget | null): void
  setQuery(q: string): void
  setShowFound(v: boolean): void
  setViewLayer(layer: WorldLayer | null): void
  toggleCategory(id: string): void
  setAllCategories(visible: boolean): void
  toggleFound(id: string): void
  upsertCustomPin(pin: Pin): void
  deleteCustomPin(id: string): void
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
  user: EMPTY_USER_DATA,
  settings: DEFAULT_SETTINGS,
  mapHotkeys: {},
  zoomKeys: null,
  selectedPinId: null,
  tracked: null,
  query: '',
  showFound: true,
  viewLayerOverride: null,
  toast: null,

  setStatus: (status) => set({ status }),
  setMode: (mode) => set({ mode }),
  setSnapshot: (snapshot) => set({ snapshot }),
  setGame: (game, gamePins, user) => set({ game, map: game.maps[0] ?? null, gamePins, user, selectedPinId: null }),
  setSettings: (patch) => set((s) => ({ settings: { ...s.settings, ...patch } })),
  setMapHotkeys: (mapHotkeys) => set({ mapHotkeys }),
  setZoomKeys: (zoomKeys) => set({ zoomKeys }),
  setHud: (patch) => set((s) => ({ settings: { ...s.settings, hud: { ...s.settings.hud, ...patch } } })),
  selectPin: (selectedPinId) => set({ selectedPinId }),
  setTracked: (tracked) => set({ tracked }),
  setQuery: (query) => set({ query }),
  setShowFound: (showFound) => set({ showFound }),
  setViewLayer: (viewLayerOverride) => set({ viewLayerOverride }),
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
  notify: (text, tone = 'info') => set({ toast: { id: ++toastSeq, text, tone } }),
  dismissToast: (id) => set((s) => (s.toast?.id === id ? { toast: null } : {})),
}))

export function isCategoryVisible(category: Category | undefined, user: GameUserData): boolean {
  if (!category) return true
  if (user.hiddenCategories.includes(category.id)) return false
  if (user.shownCategories.includes(category.id)) return true
  return category.visible
}

export function selectViewLayer(s: Pick<AppState, 'viewLayerOverride' | 'snapshot'>): WorldLayer {
  return s.viewLayerOverride ?? (s.snapshot?.local?.layer === 'caves' ? 'caves' : 'surface')
}

/** The key that opens the current game's map: the user's choice or the game's default. */
export function selectMapHotkey(s: Pick<AppState, 'game' | 'mapHotkeys'>): Hotkey | null {
  if (!s.game) return null
  return s.mapHotkeys[s.game.id] ?? s.game.defaultHotkeys[0] ?? null
}

/** Game pins plus the user's custom pins. */
export function selectAllPins(s: Pick<AppState, 'gamePins' | 'user'>): Pin[] {
  return s.user.customPins.length ? [...s.gamePins, ...s.user.customPins] : s.gamePins
}
