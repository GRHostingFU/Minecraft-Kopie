//! Fully procedural texture atlas. No external art: every tile is painted from
//! hash noise and simple patterns, so the repository ships zero binary assets.

use crate::world::block::tile;
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

pub const TILE_PX: usize = 16;
pub const ATLAS_COLS: usize = 4;
pub const ATLAS_ROWS: usize = 4;

#[inline]
fn hash2(x: u32, y: u32, salt: u32) -> u32 {
    let mut h = x.wrapping_mul(0x85EB_CA6B) ^ y.wrapping_mul(0xC2B2_AE35) ^ salt.wrapping_mul(0x27D4_EB2F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// Uniform noise in `[-1, 1]`.
#[inline]
fn noise(x: u32, y: u32, salt: u32) -> f32 {
    (hash2(x, y, salt) & 0xFFFF) as f32 / 32767.5 - 1.0
}

fn shade(base: [f32; 3], amount: f32) -> [u8; 4] {
    let f = 1.0 + amount;
    let c = |v: f32| (v * f * 255.0).clamp(0.0, 255.0) as u8;
    [c(base[0]), c(base[1]), c(base[2]), 255]
}

fn paint(tile_id: u8, x: u32, y: u32) -> [u8; 4] {
    let n = noise(x, y, tile_id as u32 + 1);
    match tile_id {
        tile::GRASS_TOP => shade([0.33, 0.58, 0.20], n * 0.12),
        tile::GRASS_SIDE => {
            // Dirt with a ragged grass fringe along the top.
            let fringe = 3 + (hash2(x, 0, 9) % 3);
            if y < fringe {
                shade([0.33, 0.58, 0.20], n * 0.12)
            } else {
                shade([0.46, 0.32, 0.20], noise(x, y, 2) * 0.12)
            }
        }
        tile::DIRT => shade([0.46, 0.32, 0.20], n * 0.14),
        tile::STONE => {
            let crack = if hash2(x / 3, y / 2, 77).is_multiple_of(11) { -0.18 } else { 0.0 };
            shade([0.50, 0.50, 0.52], n * 0.10 + crack)
        }
        tile::SAND => shade([0.84, 0.78, 0.54], n * 0.06),
        tile::WATER => {
            let wave = ((x as f32 * 0.9).sin() + (y as f32 * 0.7).cos()) * 0.04;
            let mut c = shade([0.10, 0.28, 0.72], n * 0.05 + wave);
            c[3] = 200;
            c
        }
        tile::LOG_SIDE => {
            let stripe = noise(x, 0, 31) * 0.14;
            shade([0.40, 0.29, 0.16], stripe + n * 0.05)
        }
        tile::LOG_TOP => {
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
            let ring = ((dx * dx + dy * dy).sqrt() * 1.6).sin() * 0.10;
            shade([0.62, 0.48, 0.28], ring + n * 0.04)
        }
        tile::LEAVES => {
            let hole = if hash2(x, y, 5).is_multiple_of(9) { -0.35 } else { 0.0 };
            shade([0.20, 0.45, 0.16], n * 0.18 + hole)
        }
        tile::PLANKS => {
            let seam = if y % 4 == 3 { -0.22 } else { 0.0 };
            let grain = noise(x / 5, y / 4, 8) * 0.08;
            shade([0.68, 0.53, 0.30], seam + grain + n * 0.03)
        }
        tile::BEDROCK => shade([0.17, 0.17, 0.19], n * 0.35),
        tile::SNOW => shade([0.94, 0.96, 0.99], n * 0.03),
        tile::COBBLE => {
            let cell = hash2(x / 4, y / 4, 123) % 5;
            let edge = if x.is_multiple_of(4) || y.is_multiple_of(4) { -0.20 } else { 0.0 };
            shade([0.44, 0.44, 0.46], cell as f32 * 0.05 - 0.1 + edge + n * 0.04)
        }
        _ => [255, 0, 255, 255],
    }
}

/// Pixel rectangle of a tile inside the atlas image (used by the hotbar).
pub fn tile_rect(tile_id: u8) -> Rect {
    let (col, row) = (tile_id as usize % ATLAS_COLS, tile_id as usize / ATLAS_COLS);
    let min = Vec2::new((col * TILE_PX) as f32, (row * TILE_PX) as f32);
    Rect::from_corners(min, min + Vec2::splat(TILE_PX as f32))
}

pub fn build_atlas_pixels() -> Vec<u8> {
    let (w, h) = (ATLAS_COLS * TILE_PX, ATLAS_ROWS * TILE_PX);
    let mut data = vec![255u8; w * h * 4];
    for t in 0..tile::COUNT {
        let (col, row) = (t as usize % ATLAS_COLS, t as usize / ATLAS_COLS);
        for y in 0..TILE_PX {
            for x in 0..TILE_PX {
                let px = paint(t, x as u32, y as u32);
                let o = ((row * TILE_PX + y) * w + col * TILE_PX + x) * 4;
                data[o..o + 4].copy_from_slice(&px);
            }
        }
    }
    data
}

pub fn build_atlas_image() -> Image {
    let mut image = Image::new(
        Extent3d {
            width: (ATLAS_COLS * TILE_PX) as u32,
            height: (ATLAS_ROWS * TILE_PX) as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        build_atlas_pixels(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    // Crisp pixels, like the blocks they paint.
    image.sampler = ImageSampler::nearest();
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tile_is_painted_and_opaque_except_water() {
        let px = build_atlas_pixels();
        let w = ATLAS_COLS * TILE_PX;
        for t in 0..tile::COUNT {
            let (col, row) = (t as usize % ATLAS_COLS, t as usize / ATLAS_COLS);
            let o = ((row * TILE_PX) * w + col * TILE_PX) * 4;
            assert_ne!(&px[o..o + 3], &[255, 0, 255], "tile {t} fell through to the error colour");
            assert_eq!(px[o + 3] == 255, t != tile::WATER);
        }
    }

    #[test]
    fn tiles_are_deterministic_and_distinct() {
        assert_eq!(build_atlas_pixels(), build_atlas_pixels());
        assert_ne!(paint(tile::STONE, 3, 3), paint(tile::DIRT, 3, 3));
    }
}
