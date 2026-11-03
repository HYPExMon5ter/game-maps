import { describe, expect, it } from 'vitest'
import type { CameraView } from './protocol'
import { projectPoint, projectToScreen } from './waypoint'

// Looking along +Z from the origin, 90° vertical field of view, 2:1 screen.
const camera: CameraView = { position: { x: 0, y: 0, z: 0 }, forward: { x: 0, y: 0, z: 1 }, up: { x: 0, y: 1, z: 0 }, fov_y: 90, aspect: 2 }
const W = 2000
const H = 1000

describe('projectToScreen', () => {
  it('puts a target straight ahead at the center', () => {
    const p = projectToScreen(camera, { x: 0, y: 0, z: 10 }, W, H)
    expect(p.onScreen).toBe(true)
    expect(p.x).toBeCloseTo(1000)
    expect(p.y).toBeCloseTo(500)
  })

  it('maps right and up to screen right and up', () => {
    // Half the horizontal half-angle to the right (tanX = 2), a quarter up.
    const p = projectToScreen(camera, { x: 10, y: 2.5, z: 10 }, W, H)
    expect(p.x).toBeCloseTo(1500)
    expect(p.y).toBeCloseTo(375)
  })

  it('pins targets behind the camera to the screen edge, pointing their way', () => {
    const p = projectToScreen(camera, { x: 5, y: 0, z: -10 }, W, H)
    expect(p.onScreen).toBe(false)
    expect(p.x).toBeCloseTo(W - 48)
    expect(p.angle).toBeGreaterThan(0)
  })
})

describe('projectPoint', () => {
  it('projects past the screen edges, but not behind the camera', () => {
    expect(projectPoint(camera, { x: 30, y: 0, z: 10 }, W, H)).toEqual({ x: 2500, y: 500 })
    expect(projectPoint(camera, { x: 0, y: 0, z: -1 }, W, H)).toBeNull()
  })
})
