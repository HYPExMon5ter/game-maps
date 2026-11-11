//! Draws a Stranded Deep world from its zones' terrains. Every zone has a square terrain
//! a few hundred meters across, its own heightmap applied whether or not it's been
//! visited. Between them is open ocean with no floor at all, drawn as deep water.

use crate::worldmap::Terrain;

/// `game.json` projects the map onto this square, centered on the world's origin.
pub const MAP_MIN: f32 = -2400.0;
pub const MAP_SIDE: f32 = 4800.0;
/// Pixels per side of the picture: the app cuts it into 16 tiles of 256.
pub const IMAGE_SIDE: usize = 4096;
/// Cells per side of `biomes.bin`. Finer would make looking for the nearest one slow.
const BIOME_SIDE: usize = 1024;

/// The sea's surface (Ceto's ocean sits at 0).
pub const SEA_LEVEL: f32 = 0.0;
/// Where there's no terrain, the sea is at least this deep.
const NO_FLOOR: f32 = -150.0;
/// Biome ids, as `game.json` names them.
pub const LAND: u8 = 1;
pub const SHALLOWS: u8 = 2;
pub const OPEN_SEA: u8 = 3;
pub const DEEP_SEA: u8 = 4;
/// Water up to this deep is wadeable shallows and reef.
const SHALLOWS_DEPTH: f32 = 6.0;
/// Below this the sea is deep, whatever's under it.
const OPEN_SEA_DEPTH: f32 = 60.0;

/// One zone's terrain, read from the game.
pub struct ZoneTerrain {
    /// World position of the heightmap's first sample (the terrain's corner).
    pub origin_x: f32,
    pub origin_z: f32,
    /// World size of the terrain.
    pub size_x: f32,
    pub size_z: f32,
    /// World height of a sample of 0, and how much higher a sample of 1 is.
    pub base_y: f32,
    pub scale_y: f32,
    /// Samples per side.
    pub resolution: usize,
    /// `resolution * resolution` samples from 0 to 1, row-major from the terrain's corner,
    /// rows along z (as `TerrainData.GetHeights` returns them).
    pub heights: Vec<f32>,
    /// A rocky island: bare stone where others have grass.
    pub rocky: bool,
}

impl ZoneTerrain {
    /// World height at `(x, z)`, if it's on this terrain.
    pub fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        let n = self.resolution;
        if n < 2 || self.heights.len() != n * n {
            return None;
        }
        let u = (x - self.origin_x) / self.size_x * (n - 1) as f32;
        let v = (z - self.origin_z) / self.size_z * (n - 1) as f32;
        let last = (n - 1) as f32;
        if !(0.0..=last).contains(&u) || !(0.0..=last).contains(&v) {
            return None;
        }
        let (c0, r0) = ((u as usize).min(n - 2), (v as usize).min(n - 2));
        let (fu, fv) = (u - c0 as f32, v - r0 as f32);
        let at = |r: usize, c: usize| self.heights[r * n + c];
        let top = at(r0, c0) * (1.0 - fu) + at(r0, c0 + 1) * fu;
        let bottom = at(r0 + 1, c0) * (1.0 - fu) + at(r0 + 1, c0 + 1) * fu;
        Some(self.base_y + (top * (1.0 - fv) + bottom * fv) * self.scale_y)
    }

    fn contains(&self, x: f32, z: f32) -> bool {
        x >= self.origin_x && z >= self.origin_z && x <= self.origin_x + self.size_x && z <= self.origin_z + self.size_z
    }
}

/// The ground or sea floor everywhere on the map, and which zone each point is in.
struct Field<'a> {
    zones: &'a [ZoneTerrain],
}

impl Field<'_> {
    fn sample(&self, x: f32, z: f32) -> (f32, Option<&ZoneTerrain>) {
        for zone in self.zones {
            if zone.contains(x, z) {
                if let Some(h) = zone.height_at(x, z) {
                    return (h, Some(zone));
                }
            }
        }
        (NO_FLOOR, None)
    }
}

/// The biome at a height.
pub fn biome_of(height: f32, has_floor: bool) -> u8 {
    let depth = SEA_LEVEL - height;
    if depth < 0.0 {
        LAND
    } else if depth <= SHALLOWS_DEPTH {
        SHALLOWS
    } else if has_floor && depth <= OPEN_SEA_DEPTH {
        OPEN_SEA
    } else {
        DEEP_SEA
    }
}

type Rgb = [f32; 3];

const SHALLOW_WATER: Rgb = [0.42, 0.83, 0.82];
const MID_WATER: Rgb = [0.13, 0.52, 0.68];
const DEEP_WATER: Rgb = [0.043, 0.184, 0.29];
const SURF: Rgb = [0.72, 0.93, 0.9];
const SAND: Rgb = [0.9, 0.84, 0.64];
const GRASS: Rgb = [0.42, 0.6, 0.25];
const FOREST: Rgb = [0.25, 0.42, 0.17];
const ROCK: Rgb = [0.55, 0.53, 0.49];
/// Land this close to the sea is beach.
const BEACH_HEIGHT: f32 = 1.2;
/// Grass turns to dense forest by this height.
const FOREST_HEIGHT: f32 = 18.0;

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn water(depth: f32) -> Rgb {
    let t = (depth / OPEN_SEA_DEPTH).clamp(0.0, 1.0);
    let color = if t < 0.3 { mix(SHALLOW_WATER, MID_WATER, t / 0.3) } else { mix(MID_WATER, DEEP_WATER, (t - 0.3) / 0.7) };
    // A band of surf where the sea meets the beach.
    mix(SURF, color, depth / 0.8)
}

fn land(height: f32, slope: f32, rocky: bool) -> Rgb {
    let green = if rocky { ROCK } else { mix(GRASS, FOREST, (height - BEACH_HEIGHT) / FOREST_HEIGHT) };
    let inland = mix(SAND, green, (height - BEACH_HEIGHT) / 1.5);
    // Cliffs are bare whatever the island.
    mix(inland, ROCK, (slope - 0.9) / 0.6)
}

/// Draws the world, `IMAGE_SIDE` pixels per side with row 0 in the north, and its biomes.
pub fn draw(zones: &[ZoneTerrain]) -> Terrain {
    let field = Field { zones };
    let meters = MAP_SIDE / IMAGE_SIDE as f32;
    let at = |col: usize, row_from_south: usize| -> (f32, f32) { (MAP_MIN + (col as f32 + 0.5) * meters, MAP_MIN + (row_from_south as f32 + 0.5) * meters) };

    // Heights first, row 0 in the south: shading needs each pixel's neighbors.
    let mut heights = vec![NO_FLOOR; IMAGE_SIDE * IMAGE_SIDE];
    let mut zone_of: Vec<u8> = vec![u8::MAX; IMAGE_SIDE * IMAGE_SIDE];
    for (i, zone) in zones.iter().enumerate() {
        let col = |x: f32| (((x - MAP_MIN) / meters).floor().max(0.0) as usize).min(IMAGE_SIDE - 1);
        let (c0, c1) = (col(zone.origin_x), col(zone.origin_x + zone.size_x));
        let (r0, r1) = (col(zone.origin_z), col(zone.origin_z + zone.size_z));
        for row in r0..=r1 {
            for c in c0..=c1 {
                let (x, z) = at(c, row);
                if let Some(h) = zone.height_at(x, z) {
                    heights[row * IMAGE_SIDE + c] = h;
                    zone_of[row * IMAGE_SIDE + c] = i.min(u8::MAX as usize - 1) as u8;
                }
            }
        }
    }

    let mut image = vec![0u8; IMAGE_SIDE * IMAGE_SIDE * 3];
    let height = |c: usize, r: usize| heights[r.min(IMAGE_SIDE - 1) * IMAGE_SIDE + c.min(IMAGE_SIDE - 1)];
    for row in 0..IMAGE_SIDE {
        for c in 0..IMAGE_SIDE {
            let h = height(c, row);
            let zone = zone_of[row * IMAGE_SIDE + c];
            // Gradient in meters per meter, for light from the north-west.
            let dx = (height(c + 1, row) - height(c.saturating_sub(1), row)) / (2.0 * meters);
            let dz = (height(c, row + 1) - height(c, row.saturating_sub(1))) / (2.0 * meters);
            let slope = dx.hypot(dz);
            let light = (1.0 + (-dx + dz) * 0.35).clamp(0.7, 1.25);
            let color = if h > SEA_LEVEL {
                let rocky = zones.get(zone as usize).is_some_and(|z| z.rocky);
                land(h - SEA_LEVEL, slope, rocky).map(|v| v * light)
            } else {
                // The sea floor shows through shallow water only.
                let depth = SEA_LEVEL - h;
                let relief = 1.0 + (light - 1.0) * (1.0 - depth / 25.0).clamp(0.0, 1.0);
                water(depth).map(|v| v * relief)
            };
            let out = ((IMAGE_SIDE - 1 - row) * IMAGE_SIDE + c) * 3;
            for (o, v) in image[out..out + 3].iter_mut().zip(color) {
                *o = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }

    let cell = MAP_SIDE / BIOME_SIDE as f32;
    let mut biomes = vec![DEEP_SEA; BIOME_SIDE * BIOME_SIDE];
    for row in 0..BIOME_SIDE {
        for c in 0..BIOME_SIDE {
            let (x, z) = (MAP_MIN + (c as f32 + 0.5) * cell, MAP_MIN + (row as f32 + 0.5) * cell);
            let (h, zone) = field.sample(x, z);
            biomes[row * BIOME_SIDE + c] = biome_of(h, zone.is_some());
        }
    }
    Terrain { image, biomes }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 100 m square terrain at `(x, z)`, all at `height` meters.
    fn flat(x: f32, z: f32, height: f32) -> ZoneTerrain {
        ZoneTerrain { origin_x: x, origin_z: z, size_x: 100.0, size_z: 100.0, base_y: -100.0, scale_y: 150.0, resolution: 3, heights: vec![(height + 100.0) / 150.0; 9], rocky: false }
    }

    #[test]
    fn heights_come_from_the_samples() {
        let mut zone = flat(0.0, 0.0, 0.0);
        // One corner raised: the far corner (row 2, column 2).
        zone.heights[8] = 1.0;
        assert_eq!(zone.height_at(0.0, 0.0), Some(0.0));
        assert_eq!(zone.height_at(100.0, 100.0), Some(50.0));
        assert_eq!(zone.height_at(75.0, 75.0), Some(12.5));
        assert_eq!(zone.height_at(-1.0, 0.0), None);
    }

    #[test]
    fn biomes_go_by_depth() {
        assert_eq!(biome_of(3.0, true), LAND);
        assert_eq!(biome_of(-2.0, true), SHALLOWS);
        assert_eq!(biome_of(-30.0, true), OPEN_SEA);
        assert_eq!(biome_of(-30.0, false), DEEP_SEA);
        assert_eq!(biome_of(-90.0, true), DEEP_SEA);
    }

    #[test]
    fn draws_islands_north_up() {
        // An island in the north-east corner of the map, the rest open sea.
        let island = flat(MAP_MIN + MAP_SIDE - 100.0, MAP_MIN + MAP_SIDE - 100.0, 10.0);
        let terrain = draw(&[island]);
        let pixel = |x: usize, y: usize| &terrain.image[(y * IMAGE_SIDE + x) * 3..][..3];
        let ne = pixel(IMAGE_SIDE - 5, 5);
        assert!(ne[1] > ne[2], "the north-east is land: {ne:?}");
        let sw = pixel(5, IMAGE_SIDE - 5);
        assert!(sw[2] > sw[0], "the south-west is sea: {sw:?}");
        // Biomes run from the south.
        assert_eq!(terrain.biomes[0], DEEP_SEA);
        assert_eq!(terrain.biomes[terrain.biomes.len() - 1], LAND);
    }
}
