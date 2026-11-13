// The full map's underground 3D view (see `cave3d.ts`): fed from the same store as the 2D
// map, and opening the same menus. Shown over the 2D map while 3D is on underground.

import { useEffect, useRef, useState } from 'react'
import { shownPins } from '../lib/filters'
import { selectAllPins, useStore } from '../state/store'
import { usePinFog } from '../state/fog'
import { loadMesh, meshAt, meshContains, type Map3D, type MeshInfo } from '../lib/map3d'
import { wholeMapTile } from '../lib/terrain'
import { bearingOf, toLngLat, toWorld } from '../lib/projection'
import { movers, findMover } from '../lib/movers'
import { resolveTracked } from '../lib/tracking'
import { trackPath } from '../lib/route'
import { logUiError } from '../lib/errors'
import { IS_MAP_WINDOW } from '../lib/view'
import type { Vec3 } from '../lib/protocol'
import type { BadgeShape, GameDefinition } from '../lib/game'
import { clipHeight, useView3D } from '../state/view3d'
import { badgeKey, renderBadge } from './badges'
import { mapHandle, view3dHandle } from './handle'
import type { MapContextMenuEvent } from './MapView'
import type { Cave3D, PinMarker } from './cave3d'

/** Camera distance when looking at the player (meters). */
const PLAYER_VIEW_DISTANCE = 90
/** Camera tilt when looking at the player (radians from straight down). */
const PLAYER_VIEW_TILT = 0.8
/** Furthest the camera stays when bringing a spot into view (meters). */
const SHOW_DISTANCE = 120
/** Pointer travel that turns a click into a drag (CSS pixels). */
const CLICK_SLOP = 4

const badges = new Map<string, Promise<ImageData>>()

function badge(gameId: string, key: string, color: string, icon: string, shape?: BadgeShape): Promise<ImageData> {
  const id = `${gameId}/${key}`
  let b = badges.get(id)
  if (!b) {
    b = renderBadge(gameId, color, icon, shape)
    badges.set(id, b)
    b.catch(() => badges.delete(id))
  }
  return b
}

/** The underground pins the 3D view shows: those inside a cave or bunker model. */
function cavePins(game: GameDefinition, meshes: MeshInfo[], pins: ReturnType<typeof shownPins>): ReturnType<typeof shownPins> {
  const entrances = new Set(game.categories.filter((c) => c.entrance).map((c) => c.id))
  return pins.filter(({ pin }) => (pin.layer === 'caves' || entrances.has(pin.category)) && meshes.some((m) => meshContains(m, pin.position, 10)))
}

export function Cave3DView({ data, onContextMenu }: { data: Map3D; onContextMenu?: (e: MapContextMenuEvent) => void }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [cave, setCave] = useState<Cave3D | null>(null)
  const [tip, setTip] = useState<{ x: number; y: number; text: string } | null>(null)
  const active = useStore((s) => IS_MAP_WINDOW || s.mode === 'open')
  const game = useStore((s) => s.game)
  const gameMap = useStore((s) => s.map)
  const menuRef = useRef(onContextMenu)
  useEffect(() => {
    menuRef.current = onContextMenu
  }, [onContextMenu])

  // The scene, with three.js loaded only now.
  useEffect(() => {
    const el = containerRef.current
    if (!el || !gameMap) return
    let disposed = false
    let scene: Cave3D | null = null
    let ro: ResizeObserver | null = null
    void import('./cave3d').then(({ Cave3D }) => {
      if (disposed) return
      scene = new Cave3D(el, gameMap.projection)
      ro = new ResizeObserver(() => scene?.resize(el.clientWidth, el.clientHeight))
      ro.observe(el)
      scene.resize(el.clientWidth, el.clientHeight)
      setCave(scene)
    }, (e) => logUiError(`3D view: ${String(e)}`))
    return () => {
      disposed = true
      ro?.disconnect()
      scene?.dispose()
      setCave(null)
    }
  }, [gameMap])

  // Models: the one to look at first, then the rest.
  useEffect(() => {
    if (!cave || !game || !gameMap) return
    const abort = new AbortController()
    const { snapshot } = useStore.getState()
    const local = snapshot?.local
    const underground = local?.layer === 'caves'
    const center = mapHandle.current?.getCenter()
    // Where the player is underground, else under the middle of the 2D map.
    const first = underground && local
      ? meshAt(data.meshes, local.position, gameMap.projection)
      : center
        ? meshAt(data.meshes, toWorld(gameMap.projection, center), gameMap.projection, false)
        : data.meshes[0]
    const order = first ? [first, ...data.meshes.filter((m) => m !== first)] : data.meshes
    let framed = false
    const frame = () => {
      if (framed || !first || !cave.hasMesh(first.id)) return
      framed = true
      if (underground && local) cave.lookAt(cave.toScene(local.position), PLAYER_VIEW_DISTANCE, PLAYER_VIEW_TILT)
      else cave.frameMesh(first.id)
      useView3D.getState().setCurrent(first.id)
    }
    void (async () => {
      for (const info of order) {
        if (cave.hasMesh(info.id)) continue
        try {
          cave.addMesh(info, await loadMesh(game.id, info.file, abort.signal))
          if (info.water) cave.addWater(info.id, await loadMesh(game.id, info.water, abort.signal))
        } catch (e) {
          if (!abort.signal.aborted) logUiError(`3D view: ${String(e)}`)
        }
        if (abort.signal.aborted) return
        frame()
      }
    })()
    if (data.terrain) {
      void wholeMapTile(game.id, data.terrain, gameMap.projection).then((g) => {
        const p = gameMap.projection
        if (g && !abort.signal.aborted) cave.setGround(g.heights, g.size, p.minEast, p.maxEast, p.minNorth, p.maxNorth)
      }, () => {})
    }
    frame()
    return () => abort.abort()
  }, [cave, game, gameMap, data])

  // Actions elsewhere (center on me, a pin picked in a list, zoom keys) move this view.
  useEffect(() => {
    if (!cave) return
    const handle = {
      show: (position: Vec3) => {
        const target = cave.toScene(position)
        // Close enough to see where it is, never further than now.
        const now = cave.distance
        cave.lookAt(target, Math.min(now, SHOW_DISTANCE))
      },
      zoom: (steps: number) => cave.zoomBy(steps),
    }
    view3dHandle.current = handle
    return () => {
      if (view3dHandle.current === handle) view3dHandle.current = null
    }
  }, [cave])

  // A cave picked from the list.
  const focus = useView3D((s) => s.focus)
  useEffect(() => {
    if (cave && focus) {
      cave.frameMesh(focus.id)
      useView3D.getState().setCurrent(focus.id)
    }
  }, [cave, focus])

  // A camera angle picked in the controls.
  const tilt = useView3D((s) => s.tilt)
  useEffect(() => {
    if (cave && tilt) cave.setTilt(tilt.view)
  }, [cave, tilt])

  // Which cave is in view, for the controls.
  useEffect(() => {
    if (!cave || !gameMap) return
    cave.setHandlers({
      moveEnd: () => useView3D.getState().setCurrent(meshAt(data.meshes, cave.toWorld(cave.target), gameMap.projection)?.id ?? null),
      interact: () => {
        const s = useStore.getState()
        if (s.followPlayer) s.setFollowPlayer(false)
        if (s.followId) s.setFollow(null)
      },
    })
  }, [cave, gameMap, data])

  // Players, the route and following, with each snapshot.
  useEffect(() => {
    if (!cave || !gameMap || !active) return
    const p = gameMap.projection
    const apply = () => {
      const { snapshot, followPlayer, followId } = useStore.getState()
      const local = snapshot?.local
      cave.setLocal(local ? { position: local.position, heading: local.forward ? bearingOf(p, local.forward) : null } : null)
      cave.setPlayers(movers(snapshot).map((m) => ({ id: m.id, name: m.name, color: m.color, position: m.position })))
      const { tracked, route, game: g } = useStore.getState()
      const to = tracked ? resolveTracked(tracked, snapshot).position : undefined
      const path = local && to ? trackPath(local.position, to, route, g?.unitsPerMeter) : null
      cave.setRoute(path)
      cave.setTarget(to ?? null)
      const target = followId ? findMover(snapshot, followId)?.position : followPlayer ? local?.position : undefined
      if (target) cave.follow(cave.toScene(target))
    }
    apply()
    return useStore.subscribe((s, prev) => {
      if (s.snapshot !== prev.snapshot || s.tracked !== prev.tracked || s.route !== prev.route || s.followPlayer !== prev.followPlayer || s.followId !== prev.followId) apply()
    })
  }, [cave, gameMap, active])

  // The cut: the player's level, a height picked on the slider, or none.
  const clip = useView3D((s) => s.clip)
  const playerUp = useStore((s) => {
    const local = s.snapshot?.local
    if (!local || local.layer !== 'caves' || !s.map) return null
    // Whole meters: finer would redo this with every step.
    return Math.round(local.position[s.map.projection.up])
  })
  const unitsPerMeter = game?.unitsPerMeter ?? 1
  useEffect(() => {
    cave?.setClip(clipHeight(clip, playerUp, unitsPerMeter))
  }, [cave, clip, playerUp, unitsPerMeter])

  const ground = useView3D((s) => s.ground)
  useEffect(() => cave?.setGroundVisible(ground), [cave, ground])

  // Pins, through the same filters as the 2D map.
  const gamePins = useStore((s) => s.gamePins)
  const user = useStore((s) => s.user)
  const friendPins = useStore((s) => s.friendPins)
  const selected = useStore((s) => s.selectedPinId)
  const showFound = useStore((s) => s.showFound)
  const hiddenByFog = usePinFog()
  useEffect(() => {
    if (!cave || !game || !active) return
    let cancelled = false
    const categories = new Map(game.categories.map((c) => [c.id, c]))
    const found = new Set(user.found)
    const all = selectAllPins({ gamePins, user, friendPins })
    const shown = cavePins(game, data.meshes, shownPins(all, categories, user, found, { selected, showFound, hiddenByFog }))
    const markers: PinMarker[] = shown.map((v) => ({
      id: v.pin.id,
      name: v.pin.name,
      position: v.pin.position,
      badge: badgeKey(v.color, v.icon, v.shape),
      found: found.has(v.pin.id),
      selected: v.pin.id === selected,
    }))
    const needed = new Map(shown.map((v) => [badgeKey(v.color, v.icon, v.shape), v]))
    void Promise.all(
      [...needed].map(([key, v]) => badge(game.id, key, v.color, v.icon, v.shape).then((img) => cave.setBadge(key, img), () => {})),
    ).then(() => {
      if (!cancelled) cave.setPins(markers)
    })
    return () => {
      cancelled = true
    }
  }, [cave, game, data, gamePins, user, friendPins, selected, showFound, hiddenByFog, active])

  // Clicks, the right-click menu and the hover tip.
  useEffect(() => {
    if (!cave || !gameMap) return
    const canvas = cave.canvas
    let down: { x: number; y: number; button: number } | null = null
    let moved = false
    let hover = 0
    const onDown = (e: PointerEvent) => {
      down = { x: e.offsetX, y: e.offsetY, button: e.button }
      moved = false
    }
    const onMove = (e: PointerEvent) => {
      if (down && Math.hypot(e.offsetX - down.x, e.offsetY - down.y) > CLICK_SLOP) moved = true
      if (down) return setTip(null)
      const { offsetX: x, offsetY: y } = e
      cancelAnimationFrame(hover)
      hover = requestAnimationFrame(() => {
        const name = cave.hoverName(x, y)
        canvas.style.cursor = name ? 'pointer' : ''
        setTip(name ? { x, y, text: name } : null)
      })
    }
    const onUp = (e: PointerEvent) => {
      const click = down && down.button === 0 && e.button === 0 && !moved
      down = null
      if (!click) return
      const hit = cave.pick(e.offsetX, e.offsetY, true, false)
      const s = useStore.getState()
      if (hit?.kind === 'pin' && hit.id) s.selectPin(hit.id)
      else if (hit?.kind === 'player' && hit.id) s.selectLive(hit.id)
      else {
        s.selectPin(null)
        s.selectLive(null)
      }
    }
    const onDouble = (e: MouseEvent) => {
      const hit = cave.pick(e.offsetX, e.offsetY, false)
      if (hit) cave.lookAt(cave.toScene(hit.position))
    }
    const onMenu = (e: MouseEvent) => {
      e.preventDefault()
      if (moved) return
      const hit = cave.pick(e.offsetX, e.offsetY)
      if (!hit) return
      const lngLat = toLngLat(gameMap.projection, hit.position)
      if (hit.kind === 'pin') menuRef.current?.({ lngLat, x: e.offsetX, y: e.offsetY, pinId: hit.id })
      else if (hit.kind === 'ground') menuRef.current?.({ lngLat, x: e.offsetX, y: e.offsetY, world: hit.position })
    }
    const onLeave = () => setTip(null)
    canvas.addEventListener('pointerdown', onDown)
    canvas.addEventListener('pointermove', onMove)
    canvas.addEventListener('pointerup', onUp)
    canvas.addEventListener('dblclick', onDouble)
    canvas.addEventListener('contextmenu', onMenu)
    canvas.addEventListener('pointerleave', onLeave)
    return () => {
      cancelAnimationFrame(hover)
      canvas.removeEventListener('pointerdown', onDown)
      canvas.removeEventListener('pointermove', onMove)
      canvas.removeEventListener('pointerup', onUp)
      canvas.removeEventListener('dblclick', onDouble)
      canvas.removeEventListener('contextmenu', onMenu)
      canvas.removeEventListener('pointerleave', onLeave)
    }
  }, [cave, gameMap])

  // Back in view: draw what changed while hidden.
  useEffect(() => {
    if (active) cave?.requestRender()
  }, [cave, active])

  return (
    <div className="absolute inset-0">
      <div ref={containerRef} className="h-full w-full" />
      {tip && (
        <div
          className="glass pointer-events-none absolute z-10 -translate-x-1/2 rounded-lg px-2 py-1 text-[12px] whitespace-nowrap"
          style={{ left: tip.x, top: tip.y + 18 }}
        >
          {tip.text}
        </div>
      )}
    </div>
  )
}
