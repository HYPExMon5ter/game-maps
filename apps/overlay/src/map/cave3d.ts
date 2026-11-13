// The underground 3D view: caves and bunkers as the game's own collision meshes, with
// the player, other players, pins and the route at their real heights.
//
// Loaded on demand (three.js is only fetched once 3D is turned on) and drawn only when
// something changes: the camera moving, a snapshot, a setting. There is no render loop.
//
// Scene axes: x = east, y = up, z = south (three.js is right-handed with y up, so north
// is -z). Meshes keep the game's axes and get there by their object matrix; three.js
// turns the face culling around for the mirroring that involves.

import {
  BufferAttribute,
  BufferGeometry,
  CanvasTexture,
  Color,
  CurvePath,
  DoubleSide,
  LineCurve3,
  Matrix3,
  Matrix4,
  Mesh,
  MeshBasicMaterial,
  MOUSE,
  PerspectiveCamera,
  PlaneGeometry,
  Raycaster,
  Scene,
  ShaderMaterial,
  SRGBColorSpace,
  Sprite,
  SpriteMaterial,
  Texture,
  TubeGeometry,
  Vector2,
  Vector3,
  WebGLRenderer,
  type Object3D,
} from 'three'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'
import type { MeshData, MeshInfo } from '../lib/map3d'
import type { Projection } from '../lib/projection'
import type { Vec3 } from '../lib/protocol'
import { MOVE_COLORS, moveRuns, type TrackPath } from '../lib/route'

export interface PinMarker {
  id: string
  name: string
  position: Vec3
  /** Badge image (see `renderBadge`), shared by pins that look the same. */
  badge: string
  found: boolean
  selected: boolean
}

export interface PlayerMarker {
  id: string
  name: string
  color: string
  position: Vec3
}

export interface LocalMarker {
  position: Vec3
  /** Compass heading in degrees (clockwise from north), when known. */
  heading: number | null
}

export interface CameraHandlers {
  /** The user started moving the camera. */
  interact?: () => void
  /** The camera stopped moving. */
  moveEnd?: () => void
}

export interface Pick {
  kind: 'pin' | 'player' | 'local' | 'ground'
  id?: string
  /** World position (game axes). */
  position: Vec3
}

/** Heights the mesh colors run between (scene y), and the floor/wall look. */
const FLOOR_HIGH = new Color('#e8d2a8')
const FLOOR_MID = new Color('#c4a076')
const FLOOR_LOW = new Color('#74aebb')
const WALL = new Color('#7a6f66')
/** Meters between the floors' contour lines (as on the 2D map). */
const CONTOUR_STEP = 2.5
const BACKGROUND = '#0d1116'

/** Badge size on screen (CSS pixels), as on the 2D map at mid zoom. */
const PIN_PX = 26
const SELECTED_PX = 36
const PLAYER_PX = 34
const FRIEND_PX = 16
const TARGET_PX = 40
/** How steeply a cave picked from the list is shown (radians from straight down). */
const FRAME_TILT = 0.75
/** Camera distance factor per zoom key step. */
const ZOOM_STEP = 1.4
/** Route tube radius (meters) and lift above the floor. */
const ROUTE_RADIUS = 0.35
const ROUTE_LIFT = 0.6

const vertexShader = /* glsl */ `
  varying vec3 vWorld;
  void main() {
    vec4 w = modelMatrix * vec4(position, 1.0);
    vWorld = w.xyz;
    gl_Position = projectionMatrix * viewMatrix * w;
  }
`

/** Cave meshes: also their smooth normals, octahedral-encoded in the game's axes. */
const meshVertexShader = /* glsl */ `
  attribute vec2 octNormal;
  uniform mat3 uAxes;
  varying vec3 vWorld;
  varying vec3 vNormal;
  void main() {
    vec3 n = vec3(octNormal, 1.0 - abs(octNormal.x) - abs(octNormal.y));
    if (n.z < 0.0) n.xy = (1.0 - abs(n.yx)) * vec2(n.x >= 0.0 ? 1.0 : -1.0, n.y >= 0.0 ? 1.0 : -1.0);
    vNormal = uAxes * normalize(n);
    vec4 w = modelMatrix * vec4(position, 1.0);
    vWorld = w.xyz;
    gl_Position = projectionMatrix * viewMatrix * w;
  }
`

const fragmentShader = /* glsl */ `
  #define CONTOUR ${CONTOUR_STEP.toFixed(1)}
  uniform float uClip;
  uniform float uLow;
  uniform float uHigh;
  uniform vec3 uFloorHigh;
  uniform vec3 uFloorMid;
  uniform vec3 uFloorLow;
  uniform vec3 uWall;
  varying vec3 vWorld;
  varying vec3 vNormal;
  void main() {
    if (vWorld.y > uClip) discard;
    // The face's own normal, turned to face the camera (only front faces are drawn,
    // and they face into the cave)...
    vec3 face = normalize(cross(dFdx(vWorld), dFdy(vWorld)));
    if (dot(face, cameraPosition - vWorld) < 0.0) face = -face;
    // ...smoothed with the vertex normals, which hide the simplified mesh's facets.
    // Where the two differ a lot (a box's corner, a step) the edge stays sharp.
    vec3 smoothed = normalize(vNormal);
    if (dot(smoothed, face) < 0.0) smoothed = -smoothed;
    vec3 n = dot(smoothed, face) > 0.7 ? smoothed : face;
    // Height colors everything, walls included, so levels read from any angle.
    float t = clamp((vWorld.y - uLow) / max(uHigh - uLow, 1.0), 0.0, 1.0);
    vec3 heightColor = t > 0.5 ? mix(uFloorMid, uFloorHigh, t * 2.0 - 1.0) : mix(uFloorLow, uFloorMid, t * 2.0);
    float floorness = smoothstep(0.45, 0.8, n.y);
    // Walls a shade darker than the floors they rise from.
    vec3 base = mix(mix(uWall, heightColor, 0.7), heightColor, floorness);
    // Contour lines on the floors, every few meters, as on the 2D map.
    float d = abs(fract(vWorld.y / CONTOUR + 0.5) - 0.5) * CONTOUR;
    float w = max(fwidth(vWorld.y), 0.02);
    base *= 1.0 - 0.22 * floorness * (1.0 - smoothstep(w * 0.6, w * 1.6, d));
    float key = max(dot(n, normalize(vec3(0.35, 0.85, 0.4))), 0.0);
    float fill = max(dot(n, normalize(vec3(-0.5, 0.3, -0.6))), 0.0);
    float light = 0.5 + 0.45 * key + 0.15 * fill;
    // Faces just under the cut are lit a little, so the cut edge reads.
    float edge = 1.0 - smoothstep(0.0, 0.6, uClip - vWorld.y);
    gl_FragColor = vec4(base * light + edge * 0.18, 1.0);
  }
`

/** Water surfaces: translucent blue, brighter where seen at a glance (Fresnel). */
const waterFragmentShader = /* glsl */ `
  uniform float uClip;
  varying vec3 vWorld;
  void main() {
    if (vWorld.y > uClip) discard;
    float facing = abs(normalize(cameraPosition - vWorld).y);
    vec3 deep = vec3(0.12, 0.42, 0.66);
    vec3 sky = vec3(0.55, 0.8, 0.95);
    gl_FragColor = vec4(mix(sky, deep, facing), mix(0.85, 0.55, facing));
  }
`

/** The ground over the caves: faint, shaded so its hills and valleys read. */
const groundFragmentShader = /* glsl */ `
  varying vec3 vWorld;
  void main() {
    vec3 n = normalize(cross(dFdx(vWorld), dFdy(vWorld)));
    if (n.y < 0.0) n = -n;
    float light = 0.35 + 0.65 * max(dot(n, normalize(vec3(0.35, 0.85, 0.4))), 0.0);
    // Fainter where it's seen edge-on, where it would otherwise pile up into sheets.
    float facing = abs(dot(n, normalize(cameraPosition - vWorld)));
    gl_FragColor = vec4(vec3(0.62, 0.72, 0.64) * light, 0.16 * smoothstep(0.05, 0.5, facing));
  }
`

/** The scene axes for a game's world axes, as a matrix (game xyz -> scene xyz). */
function axesMatrix(p: Projection): Matrix4 {
  const col = { x: 0, y: 1, z: 2 } as const
  const m = new Matrix4().set(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1)
  const e = m.elements // column-major
  e[col[p.east] * 4 + 0] = 1 // scene x = east
  e[col[p.up] * 4 + 1] = 1 // scene y = up
  e[col[p.north] * 4 + 2] = -1 // scene z = -north
  return m
}

interface LoadedMesh {
  info: MeshInfo
  mesh: Mesh<BufferGeometry, ShaderMaterial>
  /** Scene y range of the mesh (its colors run across it). */
  low: number
  high: number
}

export class Cave3D {
  readonly canvas: HTMLCanvasElement
  #renderer: WebGLRenderer
  #scene = new Scene()
  #camera = new PerspectiveCamera(45, 1, 0.5, 12000)
  #controls: OrbitControls
  #projection: Projection
  #axes: Matrix4
  #meshes = new Map<string, LoadedMesh>()
  #clip = Infinity
  #frame = 0
  #raycaster = new Raycaster()
  #textures = new Map<string, Texture>()
  #pins = new Map<string, { sprite: Sprite; key: string; px: number }>()
  #players = new Map<string, { dot: Sprite; label: Sprite; key: string }>()
  #local: { arrow: Sprite; heading: number | null } | null = null
  #target: Sprite | null = null
  #route: Mesh[] = []
  #routeKey = ''
  #ground: Mesh | null = null
  #groundVisible = true
  #water = new Map<string, Mesh<BufferGeometry, ShaderMaterial>>()
  #disposed = false
  #handlers: CameraHandlers = {}

  constructor(container: HTMLElement, projection: Projection) {
    this.#projection = projection
    this.#axes = axesMatrix(projection)
    this.#renderer = new WebGLRenderer({ antialias: true, powerPreference: 'high-performance' })
    this.#renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
    this.#renderer.setClearColor(BACKGROUND)
    this.canvas = this.#renderer.domElement
    this.canvas.style.display = 'block'
    container.appendChild(this.canvas)

    const c = new OrbitControls(this.#camera, this.canvas)
    // Like most 3D viewers: drag turns around the middle of the view, right-drag (or
    // Shift-drag) moves, the wheel zooms to the cursor.
    c.mouseButtons = { LEFT: MOUSE.ROTATE, MIDDLE: MOUSE.DOLLY, RIGHT: MOUSE.PAN }
    c.screenSpacePanning = false
    c.zoomToCursor = true
    c.rotateSpeed = 0.5
    c.enableDamping = false
    c.maxPolarAngle = Math.PI * 0.49
    c.minDistance = 4
    c.maxDistance = 3000
    c.keys = { LEFT: '', UP: '', RIGHT: '', BOTTOM: '' }
    c.addEventListener('change', () => this.requestRender())
    c.addEventListener('start', () => this.#handlers.interact?.())
    c.addEventListener('end', () => this.#handlers.moveEnd?.())
    this.#controls = c
    this.#camera.position.set(0, 300, 300)
    c.target.set(0, 0, 0)
    c.update()
  }

  setHandlers(handlers: CameraHandlers) {
    this.#handlers = handlers
  }

  /** Scene position of a world position. */
  toScene(p: Vec3): Vector3 {
    const pr = this.#projection
    return new Vector3(p[pr.east], p[pr.up], -p[pr.north])
  }

  toWorld(v: Vector3): Vec3 {
    const pr = this.#projection
    const out: Vec3 = { x: 0, y: 0, z: 0 }
    out[pr.east] = v.x
    out[pr.up] = v.y
    out[pr.north] = -v.z
    return out
  }

  resize(width: number, height: number) {
    if (width <= 0 || height <= 0) return
    this.#renderer.setSize(width, height, false)
    this.canvas.style.width = `${width}px`
    this.canvas.style.height = `${height}px`
    this.#camera.aspect = width / height
    this.#camera.updateProjectionMatrix()
    this.#rescaleMarkers()
    this.requestRender()
  }

  hasMesh(id: string) {
    return this.#meshes.has(id)
  }

  addMesh(info: MeshInfo, data: MeshData) {
    if (this.#disposed || this.#meshes.has(info.id)) return
    const geometry = new BufferGeometry()
    geometry.setAttribute('position', new BufferAttribute(data.positions, 3, true))
    geometry.setAttribute('octNormal', new BufferAttribute(data.normals, 2, true))
    geometry.setIndex(new BufferAttribute(data.indices, 1))
    // Quantized positions span the unit cube; the object matrix places them.
    const span = data.max.map((v, i) => Math.max(v - data.min[i], 1e-6))
    const place = new Matrix4().makeTranslation(data.min[0], data.min[1], data.min[2]).multiply(new Matrix4().makeScale(span[0], span[1], span[2]))
    const lo = this.toScene({ x: data.min[0], y: data.min[1], z: data.min[2] })
    const hi = this.toScene({ x: data.max[0], y: data.max[1], z: data.max[2] })
    const low = Math.min(lo.y, hi.y)
    const high = Math.max(lo.y, hi.y)
    const material = new ShaderMaterial({
      vertexShader: meshVertexShader,
      fragmentShader,
      uniforms: {
        uAxes: { value: new Matrix3().setFromMatrix4(this.#axes) },
        uClip: { value: this.#clip },
        uLow: { value: low },
        uHigh: { value: high },
        uFloorHigh: { value: FLOOR_HIGH },
        uFloorMid: { value: FLOOR_MID },
        uFloorLow: { value: FLOOR_LOW },
        uWall: { value: WALL },
      },
    })
    const mesh = new Mesh(geometry, material)
    mesh.matrixAutoUpdate = false
    mesh.matrix.copy(this.#axes).multiply(place)
    mesh.matrixWorldNeedsUpdate = true
    geometry.computeBoundingSphere()
    geometry.computeBoundingBox()
    this.#scene.add(mesh)
    this.#meshes.set(info.id, { info, mesh, low, high })
    this.requestRender()
  }

  /** An area's water surfaces, drawn over its floors (after them, without hiding them). */
  addWater(id: string, data: MeshData) {
    if (this.#disposed || this.#water.has(id)) return
    const geometry = new BufferGeometry()
    geometry.setAttribute('position', new BufferAttribute(data.positions, 3, true))
    geometry.setIndex(new BufferAttribute(data.indices, 1))
    const span = data.max.map((v, i) => Math.max(v - data.min[i], 1e-6))
    const place = new Matrix4().makeTranslation(data.min[0], data.min[1], data.min[2]).multiply(new Matrix4().makeScale(span[0], span[1], span[2]))
    const material = new ShaderMaterial({
      vertexShader,
      fragmentShader: waterFragmentShader,
      uniforms: { uClip: { value: this.#clip } },
      transparent: true,
      depthWrite: false,
      side: DoubleSide,
    })
    const mesh = new Mesh(geometry, material)
    mesh.matrixAutoUpdate = false
    mesh.matrix.copy(this.#axes).multiply(place)
    mesh.matrixWorldNeedsUpdate = true
    mesh.renderOrder = 1
    geometry.computeBoundingSphere()
    this.#scene.add(mesh)
    this.#water.set(id, mesh)
    this.requestRender()
  }

  /** A translucent ground surface for orientation, from a heightmap (row 0 north). */
  setGround(heights: Float32Array | null, size: number, west: number, east: number, south: number, north: number) {
    if (this.#ground) {
      this.#scene.remove(this.#ground)
      this.#ground.geometry.dispose()
      ;(this.#ground.material as ShaderMaterial).dispose()
      this.#ground = null
    }
    if (heights) {
      const g = new PlaneGeometry(east - west, north - south, size - 1, size - 1)
      g.rotateX(-Math.PI / 2) // lies flat; row 0 of the plane is at -z, north
      const pos = g.getAttribute('position') as BufferAttribute
      for (let i = 0; i < pos.count; i++) pos.setY(i, heights[i])
      g.translate((west + east) / 2, 0, -(south + north) / 2)
      const m = new ShaderMaterial({ vertexShader, fragmentShader: groundFragmentShader, transparent: true, depthWrite: false, side: DoubleSide })
      this.#ground = new Mesh(g, m)
      this.#ground.renderOrder = 1
      this.#ground.visible = this.#groundVisible
      this.#scene.add(this.#ground)
    }
    this.requestRender()
  }

  setGroundVisible(visible: boolean) {
    this.#groundVisible = visible
    if (this.#ground && this.#ground.visible !== visible) {
      this.#ground.visible = visible
      this.requestRender()
    }
  }

  /** Hides everything above this world height (null: nothing hidden). */
  setClip(height: number | null) {
    const clip = height ?? Infinity
    if (clip === this.#clip) return
    this.#clip = clip
    for (const m of this.#meshes.values()) m.mesh.material.uniforms.uClip.value = clip
    for (const w of this.#water.values()) w.material.uniforms.uClip.value = clip
    for (const p of this.#pins.values()) p.sprite.visible = p.sprite.position.y <= clip + 1
    this.requestRender()
  }

  /** Moves the camera to look at a mesh from above at an angle. */
  frameMesh(id: string) {
    const m = this.#meshes.get(id)
    const info = m?.info
    if (!info) return
    const a = this.toScene({ x: info.min[0], y: info.min[1], z: info.min[2] })
    const b = this.toScene({ x: info.max[0], y: info.max[1], z: info.max[2] })
    const center = a.clone().add(b).multiplyScalar(0.5)
    const radius = a.distanceTo(b) / 2
    this.lookAt(center, Math.max(radius * 1.4, 40), FRAME_TILT)
  }

  /**
   * Points the camera at a spot from `distance` away, keeping the direction it looks
   * from; `tilt` (radians from straight down) resets how steeply.
   */
  lookAt(target: Vector3, distance?: number, tilt?: number) {
    const c = this.#controls
    const offset = this.#camera.position.clone().sub(c.target)
    const azimuth = Math.atan2(offset.x, offset.z)
    const polar = tilt ?? Math.acos(Math.min(Math.max(offset.y / (offset.length() || 1), -1), 1))
    const d = distance ?? offset.length()
    c.target.copy(target)
    this.#camera.position.copy(target).add(new Vector3(Math.sin(polar) * Math.sin(azimuth), Math.cos(polar), Math.sin(polar) * Math.cos(azimuth)).multiplyScalar(d))
    c.update()
    this.requestRender()
  }

  /** Turns the camera to look straight down (north up) or at an angle, around the same spot. */
  setTilt(view: 'top' | 'angled') {
    const c = this.#controls
    const d = this.distance
    if (view === 'top') {
      // Straight down would leave the turn undefined: a hair off it, north up.
      this.#camera.position.copy(c.target).add(new Vector3(0, d, 0.001 * d))
      c.update()
      this.requestRender()
    } else {
      this.lookAt(c.target.clone(), d, FRAME_TILT)
    }
  }

  /** Moves the camera toward its target (positive steps) or away, as the zoom keys do. */
  zoomBy(steps: number) {
    const c = this.#controls
    const offset = this.#camera.position.clone().sub(c.target)
    const d = Math.min(Math.max(offset.length() * ZOOM_STEP ** -steps, c.minDistance), c.maxDistance)
    this.#camera.position.copy(c.target).addScaledVector(offset.normalize(), d)
    c.update()
    this.requestRender()
  }

  /** Keeps the camera's offset while its target moves (following a player). */
  follow(target: Vector3) {
    const c = this.#controls
    const delta = target.clone().sub(c.target)
    if (delta.lengthSq() < 0.0004) return
    c.target.add(delta)
    this.#camera.position.add(delta)
    c.update()
    this.requestRender()
  }

  /** How far the camera is from what it looks at. */
  get distance(): number {
    return this.#camera.position.distanceTo(this.#controls.target)
  }

  get target(): Vector3 {
    return this.#controls.target.clone()
  }

  // --- markers ----------------------------------------------------------------------

  /** Sets the badge images pins refer to (by badge key). */
  setBadge(key: string, image: ImageData) {
    if (this.#textures.has(key)) return
    const canvas = document.createElement('canvas')
    canvas.width = image.width
    canvas.height = image.height
    canvas.getContext('2d')!.putImageData(image, 0, 0)
    const t = new CanvasTexture(canvas)
    t.colorSpace = SRGBColorSpace
    this.#textures.set(key, t)
  }

  setPins(pins: PinMarker[]) {
    const seen = new Set<string>()
    for (const p of pins) {
      const texture = this.#textures.get(p.badge)
      if (!texture) continue
      seen.add(p.id)
      const key = `${p.badge}|${p.found}|${p.selected}`
      let entry = this.#pins.get(p.id)
      if (!entry) {
        const sprite = new Sprite(new SpriteMaterial({ map: texture, sizeAttenuation: false, transparent: true, depthWrite: false }))
        sprite.userData = { kind: 'pin', id: p.id, name: p.name }
        sprite.center.set(0.5, 0.5)
        this.#scene.add(sprite)
        entry = { sprite, key: '', px: PIN_PX }
        this.#pins.set(p.id, entry)
      }
      const s = entry.sprite
      s.position.copy(this.toScene(p.position)).y += 0.8
      s.userData.name = p.name
      if (entry.key !== key) {
        s.material.map = texture
        s.material.opacity = p.found ? 0.45 : 1
        // Pins hidden by rock still show, faintly; the selected one always shows.
        s.material.depthTest = !p.selected
        s.renderOrder = p.selected ? 10 : 2
        entry.px = p.selected ? SELECTED_PX : PIN_PX
        entry.key = key
        this.#scaleSprite(s, entry.px)
      }
      s.visible = s.position.y <= this.#clip + 1
    }
    for (const [id, e] of this.#pins) {
      if (seen.has(id)) continue
      this.#scene.remove(e.sprite)
      e.sprite.material.dispose()
      this.#pins.delete(id)
    }
    this.requestRender()
  }

  setLocal(local: LocalMarker | null) {
    if (!local) {
      if (this.#local) {
        this.#scene.remove(this.#local.arrow)
        this.#local.arrow.material.dispose()
        this.#local = null
        this.requestRender()
      }
      return
    }
    if (!this.#local) {
      const material = new SpriteMaterial({ map: this.#texture('local', arrowCanvas), sizeAttenuation: false, depthTest: false, transparent: true })
      const arrow = new Sprite(material)
      arrow.renderOrder = 20
      arrow.userData = { kind: 'local' }
      this.#scaleSprite(arrow, PLAYER_PX)
      this.#scene.add(arrow)
      this.#local = { arrow, heading: null }
    }
    this.#local.arrow.position.copy(this.toScene(local.position)).y += 1
    this.#local.heading = local.heading
    this.requestRender()
  }

  /** Rings what's tracked, through rock too (a spot picked on the map has no pin). */
  setTarget(position: Vec3 | null) {
    if (!position) {
      if (this.#target) {
        this.#target.visible = false
        this.requestRender()
      }
      return
    }
    if (!this.#target) {
      const ring = new Sprite(new SpriteMaterial({ map: this.#texture('target', ringCanvas), sizeAttenuation: false, depthTest: false, transparent: true }))
      ring.renderOrder = 12
      this.#scaleSprite(ring, TARGET_PX)
      this.#scene.add(ring)
      this.#target = ring
    }
    const at = this.toScene(position)
    at.y += 0.8
    if (this.#target.visible && this.#target.position.distanceToSquared(at) < 0.01) return
    this.#target.position.copy(at)
    this.#target.visible = true
    this.requestRender()
  }

  setPlayers(players: PlayerMarker[]) {
    const seen = new Set<string>()
    for (const p of players) {
      seen.add(p.id)
      const key = `${p.name}|${p.color}`
      let e = this.#players.get(p.id)
      if (e && e.key !== key) {
        this.#removePlayer(p.id)
        e = undefined
      }
      if (!e) {
        const dot = new Sprite(new SpriteMaterial({ map: this.#texture(`dot:${p.color}`, () => dotCanvas(p.color)), sizeAttenuation: false, depthTest: false, transparent: true }))
        dot.renderOrder = 15
        dot.userData = { kind: 'player', id: p.id }
        this.#scaleSprite(dot, FRIEND_PX)
        const labelCanvas = textCanvas(p.name)
        const label = new Sprite(new SpriteMaterial({ map: new CanvasTexture(labelCanvas), sizeAttenuation: false, depthTest: false, transparent: true }))
        label.renderOrder = 15
        label.center.set(0.5, -0.4)
        label.userData = { kind: 'player', id: p.id, width: labelCanvas.width, height: labelCanvas.height }
        this.#scaleLabel(label)
        this.#scene.add(dot, label)
        e = { dot, label, key }
        this.#players.set(p.id, e)
      }
      const at = this.toScene(p.position)
      at.y += 1
      e.dot.position.copy(at)
      e.label.position.copy(at)
    }
    for (const id of [...this.#players.keys()]) if (!seen.has(id)) this.#removePlayer(id)
    this.requestRender()
  }

  #removePlayer(id: string) {
    const e = this.#players.get(id)
    if (!e) return
    this.#scene.remove(e.dot, e.label)
    e.dot.material.dispose()
    e.label.material.map?.dispose()
    e.label.material.dispose()
    this.#players.delete(id)
  }

  /**
   * The route as a tube through its points, colored by how each stretch is travelled
   * (null: none). Past its `walkable` legs (no route found the rest of the way) it's
   * drawn thinner and fainter.
   */
  setRoute(route: TrackPath | null) {
    const points = route?.points ?? []
    const walkable = route?.walkable ?? 0
    const key = `${walkable}|${route?.moves.join() ?? ''}|${points.map((p) => `${p.x.toFixed(1)},${p.y.toFixed(1)},${p.z.toFixed(1)}`).join('|')}`
    if (key === this.#routeKey) return
    this.#routeKey = key
    for (const tube of this.#route) {
      this.#scene.remove(tube)
      tube.geometry.dispose()
      ;(tube.material as MeshBasicMaterial).dispose()
    }
    this.#route = []
    const scene = points.map((p) => this.toScene(p).setY(this.toScene(p).y + ROUTE_LIFT))
    const tube = (from: number, to: number, radius: number, opacity: number, color = MOVE_COLORS.walk) => {
      const path = new CurvePath<Vector3>()
      for (let i = from + 1; i <= to; i++) if (scene[i].distanceToSquared(scene[i - 1]) > 1e-4) path.add(new LineCurve3(scene[i - 1], scene[i]))
      if (!path.curves.length) return
      const g = new TubeGeometry(path, Math.min(path.curves.length * 2, 2000), radius, 6, false)
      // Drawn over the rock, so a route through a wall or down a level still shows.
      const m = new MeshBasicMaterial({ color, transparent: true, opacity, depthTest: false })
      const mesh = new Mesh(g, m)
      mesh.renderOrder = 5
      this.#scene.add(mesh)
      this.#route.push(mesh)
    }
    const end = points.length - 1
    const split = Math.min(Math.max(walkable, 0), end)
    if (route) for (const run of moveRuns(route)) tube(run.from, run.to, ROUTE_RADIUS, 0.9, MOVE_COLORS[run.move])
    if (split < end) tube(split, end, ROUTE_RADIUS * 0.5, 0.45)
    this.requestRender()
  }

  #texture(key: string, draw: () => HTMLCanvasElement): Texture {
    let t = this.#textures.get(key)
    if (!t) {
      t = new CanvasTexture(draw())
      t.colorSpace = SRGBColorSpace
      this.#textures.set(key, t)
    }
    return t
  }

  /** Sprite scale for a size in CSS pixels (sprites without size attenuation). */
  #pxScale(px: number): number {
    const h = this.canvas.clientHeight || 1
    return (2 * px) / (this.#camera.projectionMatrix.elements[5] * h)
  }

  #scaleSprite(s: Sprite, px: number) {
    const k = this.#pxScale(px)
    s.scale.set(k, k, 1)
  }

  #scaleLabel(s: Sprite) {
    const { width, height } = s.userData as { width: number; height: number }
    const k = this.#pxScale(height / LABEL_RATIO)
    s.scale.set((k * width) / height, k, 1)
  }

  #rescaleMarkers() {
    for (const e of this.#pins.values()) this.#scaleSprite(e.sprite, e.px)
    for (const e of this.#players.values()) {
      this.#scaleSprite(e.dot, FRIEND_PX)
      this.#scaleLabel(e.label)
    }
    if (this.#local) this.#scaleSprite(this.#local.arrow, PLAYER_PX)
    if (this.#target) this.#scaleSprite(this.#target, TARGET_PX)
  }

  // --- picking -----------------------------------------------------------------------

  /** What's under a canvas point (CSS pixels): a marker, else the cave surface. */
  pick(x: number, y: number, markers = true, surface = true): Pick | null {
    const ndc = new Vector2((x / this.canvas.clientWidth) * 2 - 1, -(y / this.canvas.clientHeight) * 2 + 1)
    this.#raycaster.setFromCamera(ndc, this.#camera)
    if (markers) {
      const sprites: Object3D[] = []
      if (this.#local) sprites.push(this.#local.arrow)
      for (const e of this.#players.values()) sprites.push(e.dot)
      for (const e of this.#pins.values()) if (e.sprite.visible) sprites.push(e.sprite)
      const hit = this.#raycaster.intersectObjects(sprites, false)[0]
      if (hit) {
        const d = hit.object.userData as { kind: Pick['kind']; id?: string }
        return { kind: d.kind, id: d.id, position: this.toWorld(hit.object.position) }
      }
    }
    if (!surface) return null
    // The cave surface, skipping what's cut away above the clip height.
    const hits = this.#raycaster.intersectObjects([...this.#meshes.values()].map((m) => m.mesh), false)
    const hit = hits.find((h) => h.point.y <= this.#clip)
    return hit ? { kind: 'ground', position: this.toWorld(hit.point) } : null
  }

  /** Name of the pin under a canvas point, for the hover tip. */
  hoverName(x: number, y: number): string | null {
    const p = this.pick(x, y, true, false)
    if (p?.kind !== 'pin' || !p.id) return null
    return (this.#pins.get(p.id)?.sprite.userData.name as string | undefined) ?? null
  }

  // --- drawing -----------------------------------------------------------------------

  requestRender() {
    if (this.#frame || this.#disposed) return
    this.#frame = requestAnimationFrame(() => {
      this.#frame = 0
      this.#draw()
    })
  }

  #draw() {
    if (this.#local) {
      // The arrow points the player's way on screen: their heading less the camera's.
      const toCamera = this.#camera.position.clone().sub(this.#controls.target)
      const cameraHeading = Math.atan2(-toCamera.x, toCamera.z) // radians, 0 = looking north
      const h = this.#local.heading
      this.#local.arrow.material.rotation = h === null ? 0 : cameraHeading - (h * Math.PI) / 180
    }
    this.#renderer.render(this.#scene, this.#camera)
  }

  dispose() {
    this.#disposed = true
    cancelAnimationFrame(this.#frame)
    this.#controls.dispose()
    this.#scene.traverse((o) => {
      const m = o as Mesh
      m.geometry?.dispose()
      const mat = m.material as { dispose?: () => void } | undefined
      mat?.dispose?.()
    })
    for (const t of this.#textures.values()) t.dispose()
    this.#renderer.dispose()
    this.#renderer.forceContextLoss()
    this.canvas.remove()
  }
}

// --- marker images ---------------------------------------------------------------------

const RATIO = 2
const LABEL_RATIO = 2

function arrowCanvas(): HTMLCanvasElement {
  const px = 34 * RATIO
  const c = document.createElement('canvas')
  c.width = c.height = px
  const ctx = c.getContext('2d')!
  ctx.scale(RATIO, RATIO)
  ctx.fillStyle = 'rgba(56,189,248,0.22)'
  ctx.beginPath()
  ctx.arc(17, 17, 15, 0, Math.PI * 2)
  ctx.fill()
  ctx.fillStyle = '#38bdf8'
  ctx.strokeStyle = '#fff'
  ctx.lineWidth = 2
  ctx.lineJoin = 'round'
  ctx.beginPath()
  ctx.moveTo(17, 4)
  ctx.lineTo(26, 27)
  ctx.lineTo(17, 21.5)
  ctx.lineTo(8, 27)
  ctx.closePath()
  ctx.fill()
  ctx.stroke()
  return c
}

function ringCanvas(): HTMLCanvasElement {
  const px = 40 * RATIO
  const c = document.createElement('canvas')
  c.width = c.height = px
  const ctx = c.getContext('2d')!
  ctx.fillStyle = 'rgba(255,224,138,0.18)'
  ctx.strokeStyle = '#ffe08a'
  ctx.lineWidth = 2.5 * RATIO
  ctx.beginPath()
  ctx.arc(px / 2, px / 2, px / 2 - 2 * RATIO, 0, Math.PI * 2)
  ctx.fill()
  ctx.stroke()
  return c
}

function dotCanvas(color: string): HTMLCanvasElement {
  const px = 16 * RATIO
  const c = document.createElement('canvas')
  c.width = c.height = px
  const ctx = c.getContext('2d')!
  ctx.fillStyle = color
  ctx.strokeStyle = '#fff'
  ctx.lineWidth = 2 * RATIO
  ctx.beginPath()
  ctx.arc(px / 2, px / 2, px / 2 - 2 * RATIO, 0, Math.PI * 2)
  ctx.fill()
  ctx.stroke()
  return c
}

function textCanvas(text: string): HTMLCanvasElement {
  const font = `600 ${12 * LABEL_RATIO}px system-ui, sans-serif`
  const probe = document.createElement('canvas').getContext('2d')!
  probe.font = font
  const w = Math.ceil(probe.measureText(text).width) + 12 * LABEL_RATIO
  const h = 20 * LABEL_RATIO
  const c = document.createElement('canvas')
  c.width = w
  c.height = h
  const ctx = c.getContext('2d')!
  ctx.fillStyle = 'rgba(10,14,20,0.75)'
  ctx.beginPath()
  ctx.roundRect(0, 0, w, h, 6 * LABEL_RATIO)
  ctx.fill()
  ctx.font = font
  ctx.fillStyle = '#fff'
  ctx.textBaseline = 'middle'
  ctx.fillText(text, 6 * LABEL_RATIO, h / 2)
  return c
}
