// Projects a world position onto the game's screen using the game camera, for
// waypoints drawn over the game.

import type { CameraView, Vec3 } from './protocol'

export interface ScreenPoint {
  /** CSS pixels from the screen's top-left. */
  x: number
  y: number
  /** Inside the screen (minus `margin`) and in front of the camera. */
  onScreen: boolean
  /** Direction from the screen center towards the target, degrees clockwise from up. */
  angle: number
}

const dot = (a: Vec3, b: Vec3) => a.x * b.x + a.y * b.y + a.z * b.z
const sub = (a: Vec3, b: Vec3): Vec3 => ({ x: a.x - b.x, y: a.y - b.y, z: a.z - b.z })
const cross = (a: Vec3, b: Vec3): Vec3 => ({ x: a.y * b.z - a.z * b.y, y: a.z * b.x - a.x * b.z, z: a.x * b.y - a.y * b.x })
const norm = (a: Vec3): Vec3 => {
  const l = Math.hypot(a.x, a.y, a.z) || 1
  return { x: a.x / l, y: a.y / l, z: a.z / l }
}

/**
 * Screen position (CSS pixels) of `p`, wherever it falls, or null when it's behind
 * the camera. For drawing shapes in the world, which may reach past the screen edges.
 */
export function projectPoint(camera: CameraView, p: Vec3, width: number, height: number): { x: number; y: number } | null {
  const forward = norm(camera.forward)
  const up = norm(camera.up)
  const right = norm(cross(up, forward))
  const d = sub(p, camera.position)
  const cz = dot(d, forward)
  if (cz <= 0.05) return null
  const tanY = Math.tan((camera.fov_y * Math.PI) / 360)
  const tanX = tanY * (camera.aspect || width / height)
  return { x: width / 2 + (dot(d, right) / (cz * tanX)) * (width / 2), y: height / 2 - (dot(d, up) / (cz * tanY)) * (height / 2) }
}

/**
 * Screen position of `target` for a `width` x `height` screen (Unity cameras: +Y up,
 * +Z forward). Targets off screen or behind the camera are pinned to the screen edge,
 * `margin` pixels in, in their direction.
 */
export function projectToScreen(camera: CameraView, target: Vec3, width: number, height: number, margin = 48): ScreenPoint {
  const forward = norm(camera.forward)
  const up = norm(camera.up)
  const right = norm(cross(up, forward))
  const d = sub(target, camera.position)
  const cx = dot(d, right)
  const cy = dot(d, up)
  const cz = dot(d, forward)

  const tanY = Math.tan((camera.fov_y * Math.PI) / 360)
  const tanX = tanY * (camera.aspect || width / height)
  const halfW = width / 2
  const halfH = height / 2
  const angle = (Math.atan2(cx, cy) * 180) / Math.PI

  if (cz > 0.01) {
    const x = halfW + (cx / (cz * tanX)) * halfW
    const y = halfH - (cy / (cz * tanY)) * halfH
    if (x >= margin && x <= width - margin && y >= margin && y <= height - margin) {
      return { x, y, onScreen: true, angle }
    }
  }
  // Off screen or behind: on the edge, along the direction from the screen center.
  const dx = Math.sin((angle * Math.PI) / 180)
  const dy = -Math.cos((angle * Math.PI) / 180)
  const scale = Math.min((halfW - margin) / Math.max(Math.abs(dx), 1e-6), (halfH - margin) / Math.max(Math.abs(dy), 1e-6))
  return { x: halfW + dx * scale, y: halfH + dy * scale, onScreen: false, angle }
}
