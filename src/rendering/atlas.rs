//! Fully procedural texture atlas. No external art: every tile is painted from
//! tileable value noise, cellular patterns and simple shapes, so the repository
//! ships zero binary assets. Tiles are 32x32 and the atlas carries a hand-built
//! mip chain so distant blocks are filtered instead of shimmering.

use crate::world::block::tile;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::f32::consts::TAU;

pub const TILE_PX: usize = 32;
pub const ATLAS_COLS: usize = 8;
pub const ATLAS_ROWS: usize = 4;
/// Mip levels kept in the atlas. Tiles are 32 px, so level 3 still has 4x4 texels per tile and
/// the box filter never mixes neighbouring tiles.
pub const ATLAS_MIP_LEVELS: u32 = 4;

type Rgb = [f32; 3];

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale(c: Rgb, f: f32) -> Rgb {
    [c[0] * f, c[1] * f, c[2] * f]
}

fn px(c: Rgb) -> [u8; 4] {
    let q = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    [q(c[0]), q(c[1]), q(c[2]), 255]
}

#[inline]
fn hash(x: i32, y: i32, salt: u32) -> u32 {
    let mut h =
        (x as u32).wrapping_mul(0x85EB_CA6B) ^ (y as u32).wrapping_mul(0xC2B2_AE35) ^ salt.wrapping_mul(0x27D4_EB2F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// Uniform noise in `[0, 1)`.
#[inline]
fn rnd(x: i32, y: i32, salt: u32) -> f32 {
    (hash(x, y, salt) & 0xFFFF) as f32 / 65536.0
}

/// Tileable value noise in `[0, 1]`: `(u, v)` are tile coordinates in `[0, 1)` and the lattice
/// has `px` x `py` cells, wrapping at the tile border so neighbouring tiles join seamlessly.
fn vnoise(u: f32, v: f32, px: i32, py: i32, salt: u32) -> f32 {
    let (x, y) = (u * px as f32, v * py as f32);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let at = |i: i32, j: i32| rnd((x0 as i32 + i).rem_euclid(px), (y0 as i32 + j).rem_euclid(py), salt);
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
    top + (bottom - top) * sy
}

/// Three octaves of tileable noise.
fn fbm(u: f32, v: f32, salt: u32) -> f32 {
    0.5 * vnoise(u, v, 4, 4, salt) + 0.3 * vnoise(u, v, 8, 8, salt + 1) + 0.2 * vnoise(u, v, 16, 16, salt + 2)
}

/// Tileable Worley noise: returns `(distance to nearest feature, distance to second nearest, cell id)`.
fn worley(u: f32, v: f32, cells: i32, salt: u32) -> (f32, f32, u32) {
    let (x, y) = (u * cells as f32, v * cells as f32);
    let (cx, cy) = (x.floor() as i32, y.floor() as i32);
    let (mut d1, mut d2, mut id) = (f32::MAX, f32::MAX, 0);
    for j in -1..=1 {
        for i in -1..=1 {
            let (gx, gy) = ((cx + i).rem_euclid(cells), (cy + j).rem_euclid(cells));
            let fx = (cx + i) as f32 + 0.2 + 0.6 * rnd(gx, gy, salt);
            let fy = (cy + j) as f32 + 0.2 + 0.6 * rnd(gx, gy, salt + 7);
            let d = ((fx - x).powi(2) + (fy - y).powi(2)).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                id = hash(gx, gy, salt + 13);
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2, id)
}

const GRASS: Rgb = [0.33, 0.60, 0.19];
const DIRT: Rgb = [0.47, 0.33, 0.21];

fn dirt_color(x: i32, y: i32, u: f32, v: f32, salt: u32) -> Rgb {
    let mut c = scale(DIRT, 0.82 + 0.36 * fbm(u, v, salt));
    // Embedded pebbles: 3x3 patches that are lighter and greyer.
    let (cx, cy) = (x.div_euclid(3), y.div_euclid(3));
    if rnd(cx, cy, salt + 40) < 0.13 {
        c = mix(c, [0.58, 0.52, 0.45], 0.55);
    }
    let speck = rnd(x, y, salt + 50);
    if speck < 0.05 {
        c = scale(c, 0.7);
    } else if speck > 0.97 {
        c = scale(c, 1.2);
    }
    c
}

fn grass_color(x: i32, y: i32, u: f32, v: f32, salt: u32) -> Rgb {
    let lush = fbm(u, v, salt);
    let mut c = mix(GRASS, [0.46, 0.67, 0.20], lush * 0.8);
    c = scale(c, 0.88 + 0.24 * rnd(x, y, salt + 3));
    // Short blades: vertical streaks a few pixels long.
    let blade = rnd(x, y.div_euclid(4), salt + 9);
    if blade < 0.16 {
        c = scale(c, 0.78);
    } else if blade > 0.92 {
        c = scale(c, 1.15);
    }
    c
}

fn paint(tile_id: u8, x: i32, y: i32) -> [u8; 4] {
    let (u, v) = ((x as f32 + 0.5) / TILE_PX as f32, (y as f32 + 0.5) / TILE_PX as f32);
    let salt = tile_id as u32 * 31 + 5;
    match tile_id {
        tile::GRASS_TOP => px(grass_color(x, y, u, v, salt)),
        tile::GRASS_SIDE => {
            // Dirt with a ragged grass overlay; some strands hang further down.
            let mut depth = 7 + (hash(x, 0, 9) % 4) as i32;
            if hash(x, 0, 21).is_multiple_of(7) {
                depth += 3;
            }
            let dirt = dirt_color(x, y, u, v, 2 * 31 + 5);
            if y < depth {
                let mut g = grass_color(x, y, u, v, salt);
                // Darken towards the lower rim so the overlay reads as a separate layer.
                if y >= depth - 2 {
                    g = mix(g, scale(GRASS, 0.62), 0.55);
                }
                px(g)
            } else if y == depth {
                px(mix(dirt, scale(GRASS, 0.5), 0.35))
            } else {
                px(dirt)
            }
        }
        tile::DIRT => px(dirt_color(x, y, u, v, salt)),
        tile::STONE => {
            let blotch = vnoise(u, v, 3, 3, salt + 3) - 0.5;
            let mut c = scale([0.50, 0.50, 0.52], 0.86 + 0.28 * fbm(u, v, salt) + blotch * 0.10);
            // Thin dark veins where a low-frequency noise field crosses a level.
            let vein = (vnoise(u, v, 5, 5, salt + 11) - 0.5).abs();
            if vein < 0.018 {
                c = scale(c, 0.72);
            }
            let speck = rnd(x, y, salt + 17);
            if speck < 0.07 {
                c = scale(c, 0.82);
            } else if speck > 0.95 {
                c = scale(c, 1.14);
            }
            px(c)
        }
        tile::SAND => {
            let mut c = scale([0.85, 0.79, 0.55], 0.93 + 0.14 * rnd(x, y, salt) + 0.06 * (fbm(u, v, salt) - 0.5));
            if rnd(x, y, salt + 4) < 0.04 {
                c = scale(c, 0.85);
            }
            px(c)
        }
        tile::WATER => {
            let ripple = ((u * 2.0 + 0.15 * (v * TAU * 2.0).sin()) * TAU).sin() * 0.5
                + ((v * 3.0 - 0.1 * (u * TAU * 3.0).cos()) * TAU).sin() * 0.5;
            let c = scale([0.10, 0.30, 0.74], 0.9 + 0.1 * ripple + 0.08 * (fbm(u, v, salt) - 0.5));
            let mut p = px(c);
            p[3] = 205;
            p
        }
        tile::LOG_SIDE => {
            // Bark: tall vertical ridges and grooves.
            let ridge = vnoise(u, v, 12, 3, salt);
            let mut c = scale([0.38, 0.27, 0.15], 0.72 + 0.5 * ridge);
            if vnoise(u, v, 12, 3, salt + 20) < 0.22 {
                c = scale(c, 0.65);
            }
            c = scale(c, 0.94 + 0.12 * rnd(x, y, salt + 2));
            px(c)
        }
        tile::LOG_TOP => {
            let edge = x.min(y).min(31 - x).min(31 - y);
            if edge < 3 {
                let c = scale([0.34, 0.24, 0.13], 0.8 + 0.4 * rnd(x, y, salt));
                px(if edge == 0 { scale(c, 0.8) } else { c })
            } else {
                let (dx, dy) = (u - 0.5, v - 0.5);
                let wobble = (vnoise(u, v, 4, 4, salt) - 0.5) * 0.12;
                let d = (dx * dx + dy * dy).sqrt() + wobble;
                let ring = (d * TAU * 4.2).sin() * 0.5 + 0.5;
                let c = mix([0.55, 0.40, 0.23], [0.70, 0.54, 0.32], ring);
                px(scale(c, 0.95 + 0.1 * rnd(x, y, salt + 3)))
            }
        }
        tile::LEAVES => {
            let clump = vnoise(u, v, 8, 8, salt);
            let mut c = mix([0.12, 0.34, 0.10], [0.27, 0.55, 0.18], clump);
            c = scale(c, 0.85 + 0.3 * rnd(x, y, salt + 2));
            // Gaps between leaf clusters show dark depth, hinting at transparency.
            if vnoise(u, v, 16, 16, salt + 6) > 0.80 {
                c = scale(c, 0.35);
            } else if vnoise(u, v, 8, 8, salt + 9) > 0.78 {
                c = scale(c, 1.25);
            }
            px(c)
        }
        tile::PLANKS => {
            let board = y / 8;
            let local = y % 8;
            let tint = 0.9 + 0.2 * rnd(board, 0, salt);
            let mut c = scale([0.68, 0.53, 0.30], tint);
            // Grain: thin horizontal streaks, different for every board.
            let grain = vnoise(u + rnd(board, 1, salt), v, 3, 32, salt + board as u32);
            c = scale(c, 0.9 + 0.2 * grain);
            if local == 0 {
                c = scale(c, 0.62);
            } else if local == 7 {
                c = scale(c, 1.08);
            }
            // One vertical joint per board, staggered like real flooring.
            let joint = (rnd(board, 2, salt) * 28.0) as i32 + 2;
            if x == joint {
                c = scale(c, 0.7);
            }
            // Nails next to the joint.
            if (x == (joint + 2).rem_euclid(32) || x == (joint - 2).rem_euclid(32)) && (local == 2 || local == 5) {
                c = scale(c, 0.62);
            }
            px(c)
        }
        tile::BEDROCK => {
            let n = fbm(u, v, salt);
            let mut c = scale([0.20, 0.20, 0.22], 0.35 + 1.5 * n);
            if rnd(x, y, salt + 1) < 0.10 {
                c = scale(c, 0.55);
            }
            px(c)
        }
        tile::SNOW => {
            let shade = fbm(u, v, salt);
            let mut c = mix([0.97, 0.98, 1.0], [0.78, 0.85, 0.96], shade * 0.8);
            c = scale(c, 0.96 + 0.06 * rnd(x, y, salt + 5));
            if rnd(x, y, salt + 2) > 0.985 {
                c = [1.0, 1.0, 1.0];
            }
            px(c)
        }
        tile::COBBLE => {
            let (d1, d2, id) = worley(u, v, 4, salt);
            let stone_shade = 0.72 + 0.5 * (id & 0xFF) as f32 / 255.0;
            let mut c = scale([0.47, 0.47, 0.49], stone_shade * (0.9 + 0.2 * fbm(u, v, salt)));
            let gap = d2 - d1;
            if gap < 0.028 {
                c = [0.18, 0.18, 0.19];
            } else if gap < 0.06 {
                c = scale(c, 0.78 + 4.0 * (gap - 0.028));
            }
            c = scale(c, 0.95 + 0.1 * rnd(x, y, salt + 3));
            px(c)
        }
        tile::OBSIDIAN => {
            // Glassy dark volcanic rock: deep violet-black with faint lighter veins and sparks.
            let n = fbm(u, v, salt);
            let (d1, d2, _) = worley(u, v, 3, salt + 6);
            let mut c = scale([0.075, 0.045, 0.13], 0.7 + 0.9 * n);
            if d2 - d1 < 0.05 {
                c = mix(c, [0.22, 0.12, 0.38], 0.55);
            }
            let spark = rnd(x, y, salt + 9);
            if spark > 0.985 {
                c = mix(c, [0.55, 0.38, 0.85], 0.7);
            }
            px(c)
        }
        tile::LAVA => {
            // Glowing cells separated by a darker crust.
            let (d1, d2, id) = worley(u, v, 4, salt);
            let heat = (1.0 - d1 * 2.2).clamp(0.0, 1.0);
            let crust = (1.0 - ((d2 - d1) / 0.16).clamp(0.0, 1.0)).clamp(0.0, 1.0);
            let mut c = mix([0.62, 0.12, 0.02], [1.0, 0.72, 0.14], heat * (0.7 + 0.3 * (id & 0xFF) as f32 / 255.0));
            c = mix(c, [0.32, 0.05, 0.01], crust * 0.8);
            c = scale(c, 0.92 + 0.16 * rnd(x, y, salt + 3));
            px(c)
        }
        tile::DUST_OFF | tile::DUST_ON => {
            let on = tile_id == tile::DUST_ON;
            let base = if on { [0.93, 0.10, 0.06] } else { [0.36, 0.03, 0.03] };
            let mut c = scale(base, 0.85 + 0.3 * rnd(x, y, salt));
            if rnd(x / 2, y / 2, salt + 1) > 0.8 {
                c = scale(c, if on { 1.2 } else { 0.7 });
            }
            px(c)
        }
        tile::TORCH_OFF | tile::TORCH_ON => {
            let on = tile_id == tile::TORCH_ON;
            // The flame sits in the upper third of the stick (the box crops the middle columns).
            let tip = y < 15;
            let c = match (on, tip) {
                (true, true) => mix([1.0, 0.95, 0.55], [1.0, 0.35, 0.05], y as f32 / 15.0),
                (true, false) => scale([0.55, 0.14, 0.08], 0.8 + 0.4 * rnd(x, y, salt)),
                (false, true) => scale([0.30, 0.08, 0.06], 0.8 + 0.4 * rnd(x, y, salt)),
                (false, false) => scale([0.30, 0.20, 0.12], 0.8 + 0.4 * rnd(x, y, salt)),
            };
            px(c)
        }
        tile::PISTON_TOP | tile::PISTON_STICKY => {
            let edge = x.min(y).min(31 - x).min(31 - y);
            let sticky = tile_id == tile::PISTON_STICKY;
            let base = if sticky { [0.42, 0.68, 0.30] } else { [0.70, 0.55, 0.32] };
            let mut c = scale(base, 0.88 + 0.24 * fbm(u, v, salt));
            if edge < 3 {
                c = scale(c, 0.62);
            }
            if edge == 3 {
                c = scale(c, 1.15);
            }
            if sticky && rnd(x / 3, y / 3, salt + 5) > 0.9 {
                c = scale(c, 1.2);
            }
            px(c)
        }
        tile::PISTON_SIDE | tile::PISTON_BOTTOM => {
            let edge = x.min(y).min(31 - x).min(31 - y);
            let mut c = scale([0.52, 0.52, 0.54], 0.84 + 0.3 * fbm(u, v, salt));
            if tile_id == tile::PISTON_SIDE && y < 7 {
                // The wooden plate band of the piston's business end.
                c = scale([0.66, 0.51, 0.30], 0.9 + 0.2 * rnd(x, y, salt));
            }
            if edge < 2 {
                c = scale(c, 0.65);
            }
            px(c)
        }
        tile::LAMP_OFF | tile::LAMP_ON => {
            let on = tile_id == tile::LAMP_ON;
            // A lattice of frame strips around glass panes.
            let frame = x % 16 < 2 || y % 16 < 2;
            let c = if frame {
                scale(if on { [0.80, 0.62, 0.30] } else { [0.35, 0.24, 0.14] }, 0.85 + 0.3 * rnd(x, y, salt))
            } else if on {
                let glow = 1.0 - ((u - 0.5).abs() + (v - 0.5).abs()) * 0.3;
                scale([1.0, 0.88, 0.52], glow * (0.92 + 0.12 * rnd(x, y, salt)))
            } else {
                scale([0.55, 0.38, 0.22], 0.8 + 0.3 * fbm(u, v, salt))
            };
            px(c)
        }
        tile::LEVER => {
            let c = scale([0.56, 0.40, 0.21], 0.8 + 0.4 * vnoise(u, v, 2, 8, salt));
            px(c)
        }
        tile::HELMET..=tile::BOOTS => armor_icon(tile_id - tile::HELMET, x, y),
        _ => [255, 0, 255, 255],
    }
}

/// Whether pixel `(x, y)` belongs to the silhouette of armor piece `piece` (0..4) in a 32x32 icon.
fn armor_shape(piece: u8, x: i32, y: i32) -> bool {
    let rect = |x0: i32, y0: i32, x1: i32, y1: i32| (x0..=x1).contains(&x) && (y0..=y1).contains(&y);
    match piece {
        0 => {
            // Dome with a face opening.
            let (dx, dy) = ((x as f32 - 15.5) / 11.0, (y as f32 - 17.0) / 11.0);
            let dome = dx * dx + dy * dy <= 1.0 && y <= 24;
            let visor = rect(10, 18, 21, 25);
            let rim = rect(4, 23, 27, 26) && !rect(11, 23, 20, 26);
            (dome && !visor) || rim
        }
        1 => {
            let torso = rect(8, 6, 23, 28);
            let shoulders = rect(3, 6, 9, 14) || rect(22, 6, 28, 14);
            let neck = rect(13, 6, 18, 10);
            (torso || shoulders) && !neck
        }
        2 => {
            let waist = rect(8, 5, 23, 12);
            let legs = rect(8, 12, 14, 28) || rect(17, 12, 23, 28);
            waist || legs
        }
        _ => {
            let left = rect(6, 8, 13, 21) || rect(3, 21, 14, 27);
            let right = rect(18, 8, 25, 21) || rect(17, 21, 28, 27);
            left || right
        }
    }
}

fn armor_icon(piece: u8, x: i32, y: i32) -> [u8; 4] {
    if !armor_shape(piece, x, y) {
        return [0, 0, 0, 0];
    }
    let outline = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| !armor_shape(piece, x + dx, y + dy));
    if outline {
        return px([0.17, 0.19, 0.23]);
    }
    // Light from the top-left plus a little brushed-metal noise.
    let light = 1.05 - (x + y) as f32 / 90.0;
    let streak = 0.94 + 0.12 * rnd(x, y / 3, 700 + piece as u32);
    let mut c = scale([0.74, 0.77, 0.82], light * streak);
    // Rivet-like highlights.
    if (x + 2 * y) % 11 == 0 && rnd(x, y, 650) > 0.5 {
        c = scale(c, 1.2);
    }
    px(c)
}

/// Pixel rectangle of a tile inside the atlas image (used by UI icons).
pub fn tile_rect(tile_id: u8) -> Rect {
    let (col, row) = (tile_id as usize % ATLAS_COLS, tile_id as usize / ATLAS_COLS);
    let min = Vec2::new((col * TILE_PX) as f32, (row * TILE_PX) as f32);
    Rect::from_corners(min, min + Vec2::splat(TILE_PX as f32))
}

/// Same rectangle in normalised UV space, inset by half a texel so bilinear sampling of a
/// single tile (the held item) never bleeds in a neighbour.
pub fn tile_uv_rect(tile_id: u8) -> Rect {
    let r = tile_rect(tile_id);
    let (w, h) = ((ATLAS_COLS * TILE_PX) as f32, (ATLAS_ROWS * TILE_PX) as f32);
    let inset = Vec2::splat(0.5);
    Rect::from_corners((r.min + inset) / Vec2::new(w, h), (r.max - inset) / Vec2::new(w, h))
}

/// Mip level 0 of the atlas as RGBA8.
pub fn build_atlas_pixels() -> Vec<u8> {
    let (w, h) = (ATLAS_COLS * TILE_PX, ATLAS_ROWS * TILE_PX);
    let mut data = vec![0u8; w * h * 4];
    for t in 0..tile::COUNT {
        let (col, row) = (t as usize % ATLAS_COLS, t as usize / ATLAS_COLS);
        for y in 0..TILE_PX {
            for x in 0..TILE_PX {
                let px = paint(t, x as i32, y as i32);
                let o = ((row * TILE_PX + y) * w + col * TILE_PX + x) * 4;
                data[o..o + 4].copy_from_slice(&px);
            }
        }
    }
    data
}

fn srgb_to_linear(v: u8) -> f32 {
    (v as f32 / 255.0).powf(2.2)
}

fn linear_to_srgb(v: f32) -> u8 {
    (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8
}

/// Halves an RGBA8 image with a 2x2 box filter. Colour is averaged in linear light and weighted
/// by alpha, so transparent pixels (armor icons, water) neither darken nor fringe their edges.
pub fn downsample(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let (nw, nh) = (w / 2, h / 2);
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        for x in 0..nw {
            let (mut rgb, mut alpha) = ([0.0f32; 3], 0.0f32);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let i = ((2 * y + dy) * w + 2 * x + dx) * 4;
                let a = src[i + 3] as f32 / 255.0;
                for c in 0..3 {
                    rgb[c] += srgb_to_linear(src[i + c]) * a;
                }
                alpha += a;
            }
            let o = (y * nw + x) * 4;
            if alpha > 0.0 {
                for c in 0..3 {
                    out[o + c] = linear_to_srgb(rgb[c] / alpha);
                }
            }
            out[o + 3] = (alpha / 4.0 * 255.0).round() as u8;
        }
    }
    out
}

/// All mip levels back to back (level 0 first), the layout wgpu expects for a single-layer texture.
pub fn build_atlas_mips() -> Vec<u8> {
    let (mut w, mut h) = (ATLAS_COLS * TILE_PX, ATLAS_ROWS * TILE_PX);
    let mut level = build_atlas_pixels();
    let mut all = level.clone();
    for _ in 1..ATLAS_MIP_LEVELS {
        level = downsample(&level, w, h);
        (w, h) = (w / 2, h / 2);
        all.extend_from_slice(&level);
    }
    all
}

pub fn build_atlas_image() -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width: (ATLAS_COLS * TILE_PX) as u32,
            height: (ATLAS_ROWS * TILE_PX) as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.data = Some(build_atlas_mips());
    image.texture_descriptor.mip_level_count = ATLAS_MIP_LEVELS;
    // Crisp texels up close (like the blocks they paint), trilinear filtering with mipmaps in the
    // distance so far-away blocks do not flicker.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        lod_min_clamp: 0.0,
        lod_max_clamp: (ATLAS_MIP_LEVELS - 1) as f32,
        ..default()
    });
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile_origin(t: u8) -> (usize, usize) {
        ((t as usize % ATLAS_COLS) * TILE_PX, (t as usize / ATLAS_COLS) * TILE_PX)
    }

    #[test]
    fn every_tile_is_painted_and_opaque_except_water_and_icons() {
        let px = build_atlas_pixels();
        let w = ATLAS_COLS * TILE_PX;
        for t in 0..tile::COUNT {
            let (ox, oy) = tile_origin(t);
            let (mut opaque, mut magenta) = (0, 0);
            for y in 0..TILE_PX {
                for x in 0..TILE_PX {
                    let o = ((oy + y) * w + ox + x) * 4;
                    opaque += (px[o + 3] == 255) as usize;
                    magenta += (px[o..o + 4] == [255, 0, 255, 255]) as usize;
                }
            }
            assert_eq!(magenta, 0, "tile {t} fell through to the error colour");
            let full = TILE_PX * TILE_PX;
            match t {
                tile::WATER => assert_eq!(opaque, 0),
                tile::HELMET..=tile::BOOTS => assert!(opaque > 80 && opaque < full - 80, "icon {t}: {opaque}"),
                _ => assert_eq!(opaque, full, "tile {t} must be fully opaque"),
            }
        }
    }

    #[test]
    fn tiles_are_deterministic_distinct_and_detailed() {
        assert_eq!(build_atlas_pixels(), build_atlas_pixels());
        assert_ne!(paint(tile::STONE, 3, 3), paint(tile::DIRT, 3, 3));
        // "HD" check: every block texture has plenty of tonal variation, none is a flat colour.
        let px = build_atlas_pixels();
        let w = ATLAS_COLS * TILE_PX;
        for t in (tile::GRASS_TOP..=tile::COBBLE).filter(|t| *t != tile::WATER) {
            let (ox, oy) = tile_origin(t);
            let mut seen = std::collections::HashSet::new();
            for y in 0..TILE_PX {
                for x in 0..TILE_PX {
                    let o = ((oy + y) * w + ox + x) * 4;
                    seen.insert([px[o], px[o + 1], px[o + 2]]);
                }
            }
            assert!(seen.len() > 40, "tile {t} has only {} distinct colours", seen.len());
        }
    }

    /// `cargo test dump_atlas -- --ignored` writes `target/atlas.ppm` (8x upscaled) for eyeballing.
    #[test]
    #[ignore]
    fn dump_atlas_preview() {
        let px = build_atlas_pixels();
        let (w, h) = (ATLAS_COLS * TILE_PX, ATLAS_ROWS * TILE_PX);
        let k = 4;
        let mut out = format!("P6\n{} {}\n255\n", w * k, h * k).into_bytes();
        for y in 0..h * k {
            for x in 0..w * k {
                let i = ((y / k) * w + x / k) * 4;
                let a = px[i + 3] as f32 / 255.0;
                // Checkerboard behind transparent pixels.
                let bg = if (x / 16 + y / 16) % 2 == 0 { 90.0 } else { 140.0 };
                for c in 0..3 {
                    out.push((px[i + c] as f32 * a + bg * (1.0 - a)) as u8);
                }
            }
        }
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/target/atlas.ppm"), out).unwrap();
    }

    #[test]
    fn repeating_tiles_have_no_hard_seam() {
        // Mean colour difference across the wrap-around edge must not exceed that between ordinary
        // neighbouring columns by much, otherwise merged quads would show grid lines.
        let px = build_atlas_pixels();
        let w = ATLAS_COLS * TILE_PX;
        for t in [tile::GRASS_TOP, tile::DIRT, tile::STONE, tile::SAND, tile::COBBLE, tile::SNOW, tile::LEAVES] {
            let (ox, oy) = tile_origin(t);
            let diff = |xa: usize, xb: usize| -> f32 {
                (0..TILE_PX)
                    .map(|y| {
                        let (a, b) = (((oy + y) * w + ox + xa) * 4, ((oy + y) * w + ox + xb) * 4);
                        (0..3).map(|c| (px[a + c] as f32 - px[b + c] as f32).abs()).sum::<f32>()
                    })
                    .sum::<f32>()
                    / TILE_PX as f32
            };
            let seam = diff(TILE_PX - 1, 0);
            let typical: f32 = (0..TILE_PX - 1).map(|x| diff(x, x + 1)).sum::<f32>() / (TILE_PX - 1) as f32;
            assert!(seam < typical * 2.0 + 12.0, "tile {t}: seam {seam} vs typical {typical}");
        }
    }

    #[test]
    fn mip_chain_has_the_expected_size_and_keeps_average_colour() {
        let all = build_atlas_mips();
        let (w, h) = (ATLAS_COLS * TILE_PX, ATLAS_ROWS * TILE_PX);
        let expected: usize = (0..ATLAS_MIP_LEVELS).map(|l| (w >> l) * (h >> l) * 4).sum();
        assert_eq!(all.len(), expected);
        let level0 = &all[..w * h * 4];
        let level1 = &all[w * h * 4..w * h * 4 + (w / 2) * (h / 2) * 4];
        let (o, _) = (tile_origin(tile::STONE), ());
        let mean = |data: &[u8], width: usize, size: usize, ox: usize, oy: usize| -> f32 {
            let mut s = 0.0;
            for y in 0..size {
                for x in 0..size {
                    s += data[((oy + y) * width + ox + x) * 4] as f32;
                }
            }
            s / (size * size) as f32
        };
        let (m0, m1) = (mean(level0, w, TILE_PX, o.0, o.1), mean(level1, w / 2, TILE_PX / 2, o.0 / 2, o.1 / 2));
        assert!((m0 - m1).abs() < 6.0, "stone mean {m0} vs mip1 {m1}");
    }

    #[test]
    fn downsampling_ignores_transparent_pixels() {
        // One opaque red pixel among three transparent ones stays pure red, at quarter alpha.
        let src = [200, 10, 10, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let out = downsample(&src, 2, 2);
        assert!(out[0] >= 198 && out[1] <= 12 && out[2] <= 12, "{out:?}");
        assert_eq!(out[3], 64);
    }

    #[test]
    fn uv_rects_stay_inside_their_tile() {
        let r = tile_uv_rect(tile::STONE);
        let full = tile_rect(tile::STONE);
        let (w, h) = ((ATLAS_COLS * TILE_PX) as f32, (ATLAS_ROWS * TILE_PX) as f32);
        assert!(r.min.x > full.min.x / w && r.max.x < full.max.x / w);
        assert!(r.min.y > full.min.y / h && r.max.y < full.max.y / h);
    }
}
