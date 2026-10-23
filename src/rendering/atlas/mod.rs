//! Fully procedural texture atlas. No external art: every tile is painted from
//! tileable value noise, cellular patterns and simple shapes, so the repository
//! ships zero binary assets. Tiles are 32x32 and the atlas carries a hand-built
//! mip chain so distant blocks are filtered instead of shimmering.
//!
//! * `blocks` paints the block textures,
//! * `icons` paints the isometric block icons, item sprites and HUD sprites.

mod blocks;
mod icons;

use crate::world::block::tile;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

pub const TILE_PX: usize = 32;
pub const ATLAS_COLS: usize = 16;
pub const ATLAS_ROWS: usize = 16;
/// Mip levels kept in the atlas. Tiles are 32 px, so level 3 still has 4x4 texels per tile and
/// the box filter never mixes neighbouring tiles.
pub const ATLAS_MIP_LEVELS: u32 = 4;

pub(crate) type Rgb = [f32; 3];

pub(crate) fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

pub(crate) fn scale(c: Rgb, f: f32) -> Rgb {
    [c[0] * f, c[1] * f, c[2] * f]
}

pub(crate) fn px(c: Rgb) -> [u8; 4] {
    let q = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    [q(c[0]), q(c[1]), q(c[2]), 255]
}

#[inline]
pub(crate) fn hash(x: i32, y: i32, salt: u32) -> u32 {
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
pub(crate) fn rnd(x: i32, y: i32, salt: u32) -> f32 {
    (hash(x, y, salt) & 0xFFFF) as f32 / 65536.0
}

/// Tileable value noise in `[0, 1]`: `(u, v)` are tile coordinates in `[0, 1)` and the lattice
/// has `px` x `py` cells, wrapping at the tile border so neighbouring tiles join seamlessly.
pub(crate) fn vnoise(u: f32, v: f32, px: i32, py: i32, salt: u32) -> f32 {
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
pub(crate) fn fbm(u: f32, v: f32, salt: u32) -> f32 {
    0.5 * vnoise(u, v, 4, 4, salt) + 0.3 * vnoise(u, v, 8, 8, salt + 1) + 0.2 * vnoise(u, v, 16, 16, salt + 2)
}

/// Tileable Worley noise: returns `(distance to nearest feature, distance to second nearest, cell id)`.
pub(crate) fn worley(u: f32, v: f32, cells: i32, salt: u32) -> (f32, f32, u32) {
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

/// RGBA of pixel `(x, y)` of tile `tile_id`.
pub(crate) fn paint(tile_id: u8, x: i32, y: i32) -> [u8; 4] {
    if tile_id < tile::BLOCK_TILES && !(tile::HELMET..=tile::BOOTS).contains(&tile_id) {
        blocks::paint(tile_id, x, y)
    } else {
        icons::paint(tile_id, x, y)
    }
}

/// Pixel rectangle of a tile inside the atlas image (used by UI icons).
pub fn tile_rect(tile_id: u8) -> Rect {
    let (col, row) = (tile_id as usize % ATLAS_COLS, tile_id as usize / ATLAS_COLS);
    let min = Vec2::new((col * TILE_PX) as f32, (row * TILE_PX) as f32);
    Rect::from_corners(min, min + Vec2::splat(TILE_PX as f32))
}

/// Pixel rectangle of the 9x9 HUD sprite drawn (3x enlarged, 27 px) inside its tile.
pub fn ui_sprite_rect(tile_id: u8) -> Rect {
    let r = tile_rect(tile_id);
    Rect::from_corners(r.min + Vec2::splat(2.0), r.min + Vec2::splat(29.0))
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
    use crate::world::block::{BlockId, Face};

    fn tile_origin(t: u8) -> (usize, usize) {
        ((t as usize % ATLAS_COLS) * TILE_PX, (t as usize / ATLAS_COLS) * TILE_PX)
    }

    /// Tiles that are allowed to be (partly) transparent.
    fn is_see_through(t: u8) -> bool {
        t == tile::WATER
            || t == tile::GLASS
            || t == tile::TORCH_SIDE
            || t == tile::TORCH_TOP
            || (tile::HELMET..=tile::BOOTS).contains(&t)
            || t >= tile::ICON_BLOCK
    }

    #[test]
    fn every_tile_is_painted_and_block_tiles_are_opaque() {
        let px = build_atlas_pixels();
        let w = ATLAS_COLS * TILE_PX;
        for t in 0..tile::COUNT {
            // Unused tiles in the gaps of the layout stay empty; everything else must be painted.
            let used = t < tile::BLOCK_TILES
                || (tile::ICON_BLOCK..tile::ICON_BLOCK + BlockId::COUNT).contains(&t)
                || (tile::ICON_ITEM..tile::ICON_TOOL + 20).contains(&t)
                || (tile::UI..=tile::UI_ARROW_FULL).contains(&t);
            if !used {
                continue;
            }
            let (ox, oy) = tile_origin(t);
            let (mut opaque, mut visible, mut magenta) = (0, 0, 0);
            for y in 0..TILE_PX {
                for x in 0..TILE_PX {
                    let o = ((oy + y) * w + ox + x) * 4;
                    opaque += (px[o + 3] == 255) as usize;
                    visible += (px[o + 3] > 0) as usize;
                    magenta += (px[o..o + 4] == [255, 0, 255, 255]) as usize;
                }
            }
            assert_eq!(magenta, 0, "tile {t} fell through to the error colour");
            assert!(visible > 12, "tile {t} is blank");
            let full = TILE_PX * TILE_PX;
            if !is_see_through(t) {
                assert_eq!(opaque, full, "tile {t} must be fully opaque");
            }
        }
    }

    #[test]
    fn water_is_translucent_and_glass_keeps_a_frame() {
        let p = |t: u8, x: i32, y: i32| paint(t, x, y)[3];
        assert!(p(tile::WATER, 5, 5) < 255);
        assert_eq!(p(tile::GLASS, 0, 10), 255, "glass has an opaque rim");
        assert!(p(tile::GLASS, 16, 16) < 160, "and a see-through middle");
        // The torch only paints its pole.
        assert!(p(tile::TORCH_SIDE, 15, 25) == 255 && p(tile::TORCH_SIDE, 2, 25) == 0);
    }

    #[test]
    fn tiles_are_deterministic_distinct_and_detailed() {
        assert_eq!(build_atlas_pixels(), build_atlas_pixels());
        assert_ne!(paint(tile::STONE, 3, 3), paint(tile::DIRT, 3, 3));
        // "HD" check: every block texture has plenty of tonal variation, none is a flat colour.
        let px = build_atlas_pixels();
        let w = ATLAS_COLS * TILE_PX;
        for t in (tile::GRASS_TOP..tile::BLOCK_TILES).filter(|t| !is_see_through(*t)) {
            let (ox, oy) = tile_origin(t);
            let mut seen = std::collections::HashSet::new();
            for y in 0..TILE_PX {
                for x in 0..TILE_PX {
                    let o = ((oy + y) * w + ox + x) * 4;
                    seen.insert([px[o], px[o + 1], px[o + 2]]);
                }
            }
            assert!(seen.len() > 20, "tile {t} has only {} distinct colours", seen.len());
        }
    }

    #[test]
    fn ores_are_stone_with_coloured_spots() {
        let px = build_atlas_pixels();
        let w = ATLAS_COLS * TILE_PX;
        // Mean colour of the tile: diamond is cyan-ish, redstone red-ish, iron warm.
        let mean = |t: u8| -> [f32; 3] {
            let (ox, oy) = tile_origin(t);
            let mut m = [0.0f32; 3];
            for y in 0..TILE_PX {
                for x in 0..TILE_PX {
                    for c in 0..3 {
                        m[c] += px[((oy + y) * w + ox + x) * 4 + c] as f32;
                    }
                }
            }
            m.map(|v| v / (TILE_PX * TILE_PX) as f32)
        };
        let stone = mean(tile::STONE);
        let diamond = mean(tile::DIAMOND_ORE);
        let redstone = mean(tile::REDSTONE_ORE);
        assert!(diamond[2] > stone[2] + 3.0 && diamond[2] > diamond[0], "{diamond:?} vs {stone:?}");
        assert!(redstone[0] > redstone[2] + 8.0, "{redstone:?}");
        for t in [tile::COAL_ORE, tile::IRON_ORE, tile::GOLD_ORE] {
            let m = mean(t);
            let diff: f32 = (0..3).map(|c| (m[c] - stone[c]).abs()).sum();
            assert!(diff > 4.0, "tile {t} looks like plain stone");
        }
    }

    /// `cargo test dump_atlas -- --ignored` writes `target/atlas.ppm` (4x upscaled) for eyeballing.
    #[test]
    #[ignore]
    fn dump_atlas_preview() {
        let px = build_atlas_pixels();
        let (w, h) = (ATLAS_COLS * TILE_PX, ATLAS_ROWS * TILE_PX);
        let k = 2;
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
        for t in [
            tile::GRASS_TOP,
            tile::DIRT,
            tile::STONE,
            tile::SAND,
            tile::COBBLE,
            tile::SNOW,
            tile::LEAVES,
            tile::GRAVEL,
            tile::NETHERRACK,
            tile::GRANITE,
            tile::COAL_ORE,
            tile::WOOL_WHITE,
        ] {
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
            assert!(seam < typical * 2.0 + 14.0, "tile {t}: seam {seam} vs typical {typical}");
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

    #[test]
    fn isometric_icons_show_the_block_textures() {
        // The top corner of a grass icon is green, its left face brown-green side, none is blank.
        let top = paint(tile::ICON_BLOCK + BlockId::GRASS.0, 16, 6);
        assert!(top[3] == 255 && top[1] > top[0], "{top:?}");
        let corner = paint(tile::ICON_BLOCK + BlockId::STONE.0, 0, 0);
        assert_eq!(corner[3], 0, "icon corners are transparent");
        for id in BlockId::all() {
            let t = tile::ICON_BLOCK + id.0;
            let visible = (0..TILE_PX as i32)
                .flat_map(|y| (0..TILE_PX as i32).map(move |x| (x, y)))
                .filter(|&(x, y)| paint(t, x, y)[3] > 0)
                .count();
            assert!(visible > 100, "icon of {} is nearly empty", id.name());
        }
        let _ = Face::Top;
    }
}
