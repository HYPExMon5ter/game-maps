//! Draws a Valheim world from the textures its own map is built from.
//!
//! `Minimap` keeps three square textures of the whole world, one pixel per
//! `m_pixelSize` meters, row 0 in the south: the biome colors, a mask (red: forest, blue:
//! Ashlands lava and sea) and the height. The game combines them in a
//! shader; this does the same job with a hillshade, so the land's shape can be read.

use crate::worldmap::Terrain;

/// An RGBA pixel, as `Texture2D.GetPixels32` returns it.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Color32 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// `ZoneSystem.m_waterLevel`.
const SEA_LEVEL: f32 = 30.0;
/// Where the world ends, in meters from its center.
const WORLD_RADIUS: f32 = 10500.0;
/// Width of the fade at the world's edge, in meters.
const EDGE_FADE: f32 = 250.0;

const SHALLOW_SEA: [f32; 3] = [0.27, 0.50, 0.60];
const DEEP_SEA: [f32; 3] = [0.05, 0.17, 0.29];
const BEYOND_WORLD: [f32; 3] = [0.03, 0.09, 0.16];
const ASHLANDS_SEA: [f32; 3] = [0.35, 0.14, 0.11];
const LAVA: [f32; 3] = [0.90, 0.33, 0.10];
/// Biome color of the Mistlands (`Minimap.m_mistlandsColor`). It's nearly black, which
/// hides everything on it, so it's drawn as `MISTLANDS`.
const MISTLANDS_BIOME: (u8, u8, u8) = (51, 51, 51);
const MISTLANDS: [f32; 3] = [0.30, 0.29, 0.35];
/// Depth at which the sea reaches its darkest, in meters.
const FULL_DEPTH: f32 = 45.0;
/// Swamps sit around sea level: this far under it they still read as swamp, not as sea.
const SWAMP_DEPTH: f32 = 3.0;
/// Biome color of the swamp (`Minimap.m_swampColor`).
const SWAMP: (u8, u8, u8) = (163, 114, 88);
/// How far swamp at or under the sea counts as above it.
const SWAMP_SHORE: f32 = 0.5;
/// Height over which the coast fades from sea to land, in meters.
const COAST_BLEND: f32 = 1.0;
/// How many output pixels each texture pixel becomes.
pub const UPSCALE: usize = 2;
/// How much forest darkens the land.
const FOREST_SHADE: f32 = 0.38;
/// Exaggerates slopes in the hillshade.
const RELIEF: f32 = 2.2;
/// How far the sun lights the land, from the north-west.
const SUN: [f32; 3] = [-0.5, 0.5, 0.7];

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale(c: [f32; 3], s: f32) -> [f32; 3] {
    [c[0] * s, c[1] * s, c[2] * s]
}

/// A 16-bit float (how `m_heightTexture` stores heights).
fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((h >> 10) & 0x1f);
    let fraction = f32::from(h & 0x3ff);
    sign * match exponent {
        0 => fraction * 2f32.powi(-24),
        0x1f => 0.0, // infinity or not a number: never a height
        e => (1.0 + fraction / 1024.0) * 2f32.powi(e - 15),
    }
}

/// The light on the land at `(x, y)`: 1 on flat ground.
fn hillshade(height: &[f32], size: usize, x: usize, y: usize, pixel_meters: f32) -> f32 {
    let at = |x: usize, y: usize| height[y * size + x];
    let (left, right) = (x.saturating_sub(1), (x + 1).min(size - 1));
    let (down, up) = (y.saturating_sub(1), (y + 1).min(size - 1));
    let dx = (at(right, y) - at(left, y)) / ((right - left).max(1) as f32 * pixel_meters);
    let dy = (at(x, up) - at(x, down)) / ((up - down).max(1) as f32 * pixel_meters);
    let (nx, ny) = (-dx * RELIEF, -dy * RELIEF);
    let length = (nx * nx + ny * ny + 1.0).sqrt();
    let light = (nx * SUN[0] + ny * SUN[1] + SUN[2]) / length / SUN[2];
    (0.55 + 0.5 * light).clamp(0.45, 1.3)
}

fn sea(height: f32, ash: f32) -> [f32; 3] {
    let depth = ((SEA_LEVEL - height) / FULL_DEPTH).clamp(0.0, 1.0);
    mix(mix(SHALLOW_SEA, DEEP_SEA, depth), ASHLANDS_SEA, 0.75 * ash)
}

fn land(biome: Color32, mask: Color32, height: f32, light: f32) -> [f32; 3] {
    let color = if (biome.r, biome.g, biome.b) == MISTLANDS_BIOME { MISTLANDS } else { [f32::from(biome.r) / 255.0, f32::from(biome.g) / 255.0, f32::from(biome.b) / 255.0] };
    let forest = f32::from(mask.r) / 255.0;
    let lava = f32::from(mask.b) / 255.0;
    let altitude = ((height - SEA_LEVEL) / 200.0).clamp(0.0, 1.0);
    let shaded = scale(color, (1.0 - FOREST_SHADE * forest) * light * (0.92 + 0.16 * altitude));
    mix(shaded, LAVA, 0.5 * lava)
}

/// Biome ids in `biomes.bin`, matching `games/valheim/game.json`.
const OCEAN: u8 = 0;
const MEADOWS: u8 = 1;
const BLACK_FOREST: u8 = 2;
const SWAMP_ID: u8 = 3;
const MOUNTAIN: u8 = 4;
const PLAINS: u8 = 5;
const MISTLANDS_ID: u8 = 6;
const ASHLANDS: u8 = 7;
const DEEP_NORTH: u8 = 8;

/// `WorldGenerator.IsDeepnorth`.
fn in_deep_north(x: f32, z: f32) -> bool {
    let wobble = (f64::from(x).atan2(f64::from(z)) * 20.0).sin() * 100.0;
    f64::from(x).hypot(f64::from(z) + 4000.0) > 12000.0 + wobble
}

/// Which biome a pixel of the game's map is, from its biome color. Ocean, mountains and the
/// Deep North are all white in it, so height and position tell those apart.
fn biome_of(color: Color32, height: f32, x: f32, z: f32) -> u8 {
    match (color.r, color.g, color.b) {
        (146, 167, 92) => MEADOWS,
        (107, 116, 63) => BLACK_FOREST,
        (163, 114, 88) => SWAMP_ID,
        (231, 171, 120) => PLAINS,
        (51, 51, 51) => MISTLANDS_ID,
        (123, 32, 32) => ASHLANDS,
        _ if height < SEA_LEVEL => OCEAN,
        _ if in_deep_north(x, z) => DEEP_NORTH,
        _ => MOUNTAIN,
    }
}

/// The biome of every pixel, row 0 in the south.
fn biomes(size: usize, pixel_meters: f32, biome: &[Color32], height: &[f32]) -> Vec<u8> {
    let middle = size as f32 / 2.0;
    let at = |i: usize| (i as f32 + 0.5 - middle) * pixel_meters;
    (0..size * size).map(|i| biome_of(biome[i], height[i], at(i % size), at(i / size))).collect()
}

/// One pixel of the source textures, drawn as both land and sea: at the coast, each side
/// blends only with its own kind, so the white of the ocean biome never shows.
struct Layers {
    land: Vec<[u8; 3]>,
    sea: Vec<[u8; 3]>,
    /// Above 0 is land: meters above the sea, with shallow swamp counted as land.
    shore: Vec<f32>,
}

fn to_bytes(c: [f32; 3]) -> [u8; 3] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8)
}

fn layers(size: usize, pixel_meters: f32, biome: &[Color32], mask: &[Color32], height: &[f32]) -> Layers {
    let mut out = Layers { land: Vec::with_capacity(height.len()), sea: Vec::with_capacity(height.len()), shore: Vec::with_capacity(height.len()) };
    for (i, &h) in height.iter().enumerate() {
        let swamp = (biome[i].r, biome[i].g, biome[i].b) == SWAMP && h > SEA_LEVEL - SWAMP_DEPTH;
        let light = hillshade(height, size, i % size, i / size, pixel_meters);
        out.land.push(to_bytes(land(biome[i], mask[i], h, light)));
        out.sea.push(to_bytes(sea(h, f32::from(mask[i].b) / 255.0)));
        out.shore.push(if swamp { (h - SEA_LEVEL).max(SWAMP_SHORE) } else { h - SEA_LEVEL });
    }
    out
}

/// Draws the world as 8-bit RGB, `size * UPSCALE` pixels per side, row 0 in the north. All
/// slices hold `size * size` pixels, row 0 in the south. Land and sea are told apart between
/// the texture's pixels, so coasts come out smooth when the map is zoomed in.
pub fn draw(size: usize, pixel_meters: f32, biome: &[Color32], mask: &[Color32], height: &[u16]) -> Terrain {
    let height: Vec<f32> = height.iter().copied().map(half_to_f32).collect();
    let layers = layers(size, pixel_meters, biome, mask, &height);
    let biomes = biomes(size, pixel_meters, biome, &height);
    let side = size * UPSCALE;
    let middle = side as f32 / 2.0;
    let meters = pixel_meters / UPSCALE as f32;
    let last = size as f32 - 1.0;
    let mut out = vec![0u8; side * side * 3];
    for y in 0..side {
        let sy = ((y as f32 + 0.5) / UPSCALE as f32 - 0.5).clamp(0.0, last);
        let (y0, fy) = (sy as usize, sy.fract());
        let y1 = (y0 + 1).min(size - 1);
        let row = (side - 1 - y) * side * 3;
        for x in 0..side {
            let sx = ((x as f32 + 0.5) / UPSCALE as f32 - 0.5).clamp(0.0, last);
            let (x0, fx) = (sx as usize, sx.fract());
            let x1 = (x0 + 1).min(size - 1);
            let corners = [(y0 * size + x0, (1.0 - fx) * (1.0 - fy)), (y0 * size + x1, fx * (1.0 - fy)), (y1 * size + x0, (1.0 - fx) * fy), (y1 * size + x1, fx * fy)];

            let (mut shore, mut land_weight, mut sea_weight) = (0.0, 0.0, 0.0);
            let (mut land_color, mut sea_color) = ([0.0f32; 3], [0.0f32; 3]);
            for (i, w) in corners {
                let h = layers.shore[i];
                shore += h * w;
                let (color, sum, total) = if h > 0.0 { (layers.land[i], &mut land_color, &mut land_weight) } else { (layers.sea[i], &mut sea_color, &mut sea_weight) };
                *total += w;
                for c in 0..3 {
                    sum[c] += f32::from(color[c]) * w;
                }
            }
            // A pixel on the wrong side of the coast takes its color from the side it's on,
            // or from its nearest pixel's other layer if all four corners are across it.
            let fallback = corners.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map_or(0, |c| c.0);
            let land_color = if land_weight > 0.0 { scale(land_color, 1.0 / (255.0 * land_weight)) } else { layers.land[fallback].map(|v| f32::from(v) / 255.0) };
            let sea_color = if sea_weight > 0.0 { scale(sea_color, 1.0 / (255.0 * sea_weight)) } else { layers.sea[fallback].map(|v| f32::from(v) / 255.0) };
            let color = mix(sea_color, land_color, (shore / COAST_BLEND + 0.5).clamp(0.0, 1.0));

            let distance = (x as f32 + 0.5 - middle).hypot(y as f32 + 0.5 - middle) * meters;
            let beyond = ((distance - WORLD_RADIUS) / EDGE_FADE).clamp(0.0, 1.0);
            let at = row + x * 3;
            out[at..at + 3].copy_from_slice(&to_bytes(mix(color, BEYOND_WORLD, beyond)));
        }
    }
    Terrain { image: out, biomes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_half_floats() {
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert_eq!(half_to_f32(0xc000), -2.0);
        assert_eq!(half_to_f32(0x4f80), 30.0);
        assert_eq!(half_to_f32(0x7c00), 0.0);
    }

    /// 0x4f80 is sea level, 0x5640 is 100 m.
    fn world(size: usize, height: u16) -> (Vec<Color32>, Vec<Color32>, Vec<u16>) {
        (vec![Color32 { r: 146, g: 167, b: 92, a: 255 }; size * size], vec![Color32::default(); size * size], vec![height; size * size])
    }

    #[test]
    fn flips_rows_so_north_comes_first() {
        let size = 4;
        let (biome, mask, mut height) = world(size, 0x5640);
        // The south row is under water.
        height[..size].fill(0);
        let image = draw(size, 12.0, &biome, &mask, &height).image;
        let side = size * UPSCALE;
        let pixel = |x: usize, y: usize| &image[(y * side + x) * 3..][..3];
        assert!(pixel(0, side - 1)[2] > pixel(0, side - 1)[0], "the south is blue");
        assert!(pixel(0, 0)[1] > pixel(0, 0)[2], "the north is green");
    }

    #[test]
    fn tells_the_white_biomes_apart() {
        let white = Color32 { r: 255, g: 255, b: 255, a: 255 };
        assert_eq!(biome_of(white, 10.0, 0.0, 0.0), OCEAN);
        assert_eq!(biome_of(white, 200.0, 0.0, 0.0), MOUNTAIN);
        assert_eq!(biome_of(white, 60.0, 0.0, 9500.0), DEEP_NORTH);
        assert_eq!(biome_of(Color32 { r: 146, g: 167, b: 92, a: 255 }, 40.0, 0.0, 0.0), MEADOWS);
    }

    #[test]
    fn the_ocean_biome_never_shows_at_the_coast() {
        let size = 4;
        // White ocean biome on the left under water, meadows on the right above it.
        let mut biome = vec![Color32 { r: 255, g: 255, b: 255, a: 255 }; size * size];
        let mut height = vec![0x4c00u16; size * size];
        for y in 0..size {
            for x in 2..size {
                biome[y * size + x] = Color32 { r: 146, g: 167, b: 92, a: 255 };
                height[y * size + x] = 0x5640;
            }
        }
        let image = draw(size, 12.0, &biome, &vec![Color32::default(); size * size], &height).image;
        assert!(image.iter().step_by(3).all(|&red| red < 235), "no white pixels");
    }

    #[test]
    fn past_the_world_edge_is_empty_sea() {
        let size = 2048;
        let (biome, mask, height) = world(size, 0x5640);
        let image = draw(size, 12.0, &biome, &mask, &height).image;
        let expected: Vec<u8> = BEYOND_WORLD.iter().map(|c| (c * 255.0) as u8).collect();
        assert_eq!(&image[..3], expected.as_slice());
    }

    #[test]
    fn swamp_just_under_the_sea_stays_swamp() {
        let size = 2;
        let (mut biome, mask, _) = world(size, 0);
        biome.fill(Color32 { r: SWAMP.0, g: SWAMP.1, b: SWAMP.2, a: 255 });
        // 29 m: one meter under the sea.
        let image = draw(size, 12.0, &biome, &mask, &vec![0x4f40; size * size]).image;
        assert!(image[0] > image[2], "red over blue: {:?}", &image[..3]);
    }
}
