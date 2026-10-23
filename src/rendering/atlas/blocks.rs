//! Procedural block textures (32x32), painted per pixel from noise and simple shapes.

use super::{Rgb, fbm, hash, mix, px, rnd, scale, vnoise, worley};
use crate::world::block::tile;
use std::f32::consts::TAU;

const GRASS: Rgb = [0.33, 0.60, 0.19];
const DIRT: Rgb = [0.47, 0.33, 0.21];
const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];

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

/// Salt used for the plain stone texture; ores reuse it so they match the rock around them.
const STONE_SALT: u32 = tile::STONE as u32 * 31 + 5;

fn stone_color(x: i32, y: i32, u: f32, v: f32) -> Rgb {
    let salt = STONE_SALT;
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
    c
}

fn cobble_color(x: i32, y: i32, u: f32, v: f32, salt: u32) -> Rgb {
    let (d1, d2, id) = worley(u, v, 4, salt);
    let stone_shade = 0.72 + 0.5 * (id & 0xFF) as f32 / 255.0;
    let mut c = scale([0.47, 0.47, 0.49], stone_shade * (0.9 + 0.2 * fbm(u, v, salt)));
    let gap = d2 - d1;
    if gap < 0.028 {
        c = [0.18, 0.18, 0.19];
    } else if gap < 0.06 {
        c = scale(c, 0.78 + 4.0 * (gap - 0.028));
    }
    scale(c, 0.95 + 0.1 * rnd(x, y, salt + 3))
}

fn planks_color(x: i32, y: i32, u: f32, v: f32, salt: u32) -> Rgb {
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
    c
}

/// Stone with `spots` blobs of an ore: `(base colour, highlight colour)`.
fn ore(x: i32, y: i32, u: f32, v: f32, tile_id: u8, spots: i32, base: Rgb, hi: Rgb) -> Rgb {
    let mut c = stone_color(x, y, u, v);
    let salt = tile_id as u32 * 17 + 3;
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    for k in 0..spots {
        let cx = 3.5 + rnd(k, 0, salt) * 25.0;
        let cy = 3.5 + rnd(k, 1, salt) * 25.0;
        let r = 2.2 + rnd(k, 2, salt) * 1.8;
        // Wobble the outline so the blobs are not perfect discs.
        let wobble = (rnd(x, y, salt + 9) - 0.5) * 0.9;
        let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() + wobble;
        if d < r {
            let shade = 0.85 + 0.3 * rnd(x, y, salt + 21);
            let mut o = scale(mix(base, hi, rnd(x, y, salt + 33)), shade);
            if d > r - 0.9 {
                o = scale(o, 0.78);
            }
            c = o;
        }
    }
    c
}

/// Rows of bricks of `bh` px height and `bw` px width, alternate rows shifted by half a brick.
fn bricks(x: i32, y: i32, bw: i32, bh: i32, base: Rgb, mortar: Rgb, salt: u32) -> Rgb {
    let row = y / bh;
    let off = if row % 2 == 1 { bw / 2 } else { 0 };
    let bx = (x + off).rem_euclid(32);
    let col = bx / bw;
    let (lx, ly) = (bx % bw, y % bh);
    if lx == 0 || ly == 0 {
        return scale(mortar, 0.92 + 0.16 * rnd(x, y, salt + 2));
    }
    let tint = 0.82 + 0.36 * rnd(col, row, salt);
    let mut c = scale(base, tint);
    c = scale(c, 0.93 + 0.14 * rnd(x, y, salt + 1));
    // Light top edge, dark bottom edge.
    if ly == 1 {
        c = scale(c, 1.12);
    } else if ly == bh - 1 {
        c = scale(c, 0.85);
    }
    c
}

fn speckled(x: i32, y: i32, u: f32, v: f32, base: Rgb, light: Rgb, dark: Rgb, salt: u32) -> Rgb {
    let mut c = scale(base, 0.88 + 0.24 * fbm(u, v, salt));
    let blob = vnoise(u, v, 8, 8, salt + 5);
    if blob > 0.66 {
        c = mix(c, light, 0.7);
    } else if blob < 0.26 {
        c = mix(c, dark, 0.5);
    }
    let speck = rnd(x, y, salt + 12);
    if speck < 0.06 {
        c = mix(c, dark, 0.6);
    } else if speck > 0.95 {
        c = mix(c, light, 0.6);
    }
    c
}

/// Beveled square block of metal/gem with a lighter top-left and darker bottom-right rim.
fn bevel(x: i32, y: i32, base: Rgb, hi: Rgb, salt: u32) -> Rgb {
    let edge = x.min(y).min(31 - x).min(31 - y);
    let mut c = scale(mix(base, hi, rnd(x / 2, y / 2, salt)), 0.94 + 0.12 * rnd(x, y, salt + 1));
    if edge < 2 {
        let lit = (x + y) < 31;
        c = scale(c, if lit { 1.18 } else { 0.72 });
    } else if edge == 2 {
        c = scale(c, 0.9);
    }
    // Diagonal shine.
    if ((x + y) % 16 == 5 || (x + y) % 16 == 6) && edge > 3 && rnd(x, y, salt + 3) > 0.3 {
        c = mix(c, [1.0, 1.0, 1.0], 0.25);
    }
    c
}

pub(super) fn paint(tile_id: u8, x: i32, y: i32) -> [u8; 4] {
    let (u, v) = ((x as f32 + 0.5) / 32.0, (y as f32 + 0.5) / 32.0);
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
        tile::STONE => px(stone_color(x, y, u, v)),
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
        tile::PLANKS => px(planks_color(x, y, u, v, salt)),
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
        tile::COBBLE => px(cobble_color(x, y, u, v, salt)),
        tile::COAL_ORE => px(ore(x, y, u, v, tile_id, 8, [0.09, 0.09, 0.10], [0.24, 0.24, 0.26])),
        tile::IRON_ORE => px(ore(x, y, u, v, tile_id, 7, [0.80, 0.60, 0.46], [0.92, 0.76, 0.62])),
        tile::GOLD_ORE => px(ore(x, y, u, v, tile_id, 6, [0.95, 0.76, 0.16], [1.0, 0.92, 0.45])),
        tile::REDSTONE_ORE => px(ore(x, y, u, v, tile_id, 9, [0.70, 0.05, 0.05], [1.0, 0.25, 0.20])),
        tile::DIAMOND_ORE => px(ore(x, y, u, v, tile_id, 6, [0.25, 0.80, 0.82], [0.75, 1.0, 1.0])),
        tile::GRAVEL => {
            let (d1, d2, id) = worley(u, v, 6, salt);
            let shade = 0.6 + 0.6 * (id & 0xFF) as f32 / 255.0;
            let warm = (id >> 8 & 0xFF) as f32 / 255.0;
            let mut c = scale(mix([0.52, 0.52, 0.54], [0.58, 0.50, 0.44], warm), shade);
            if d2 - d1 < 0.05 {
                c = scale(c, 0.55);
            }
            px(scale(c, 0.93 + 0.14 * rnd(x, y, salt + 1)))
        }
        tile::SANDSTONE_SIDE => {
            let mut c = scale([0.82, 0.75, 0.52], 0.93 + 0.1 * fbm(u, v, salt) + 0.06 * rnd(x, y, salt + 1));
            // Carved band and sediment layers.
            if (10..13).contains(&y) {
                c = scale(c, 0.9);
            }
            if y == 3 || y == 22 || y == 27 {
                c = scale(c, 0.82);
            }
            px(c)
        }
        tile::SANDSTONE_TOP => {
            let edge = x.min(y).min(31 - x).min(31 - y);
            let mut c = scale([0.86, 0.79, 0.55], 0.95 + 0.08 * rnd(x, y, salt));
            if edge == 0 {
                c = scale(c, 0.85);
            }
            px(c)
        }
        tile::BRICKS => px(bricks(x, y, 16, 8, [0.62, 0.27, 0.21], [0.66, 0.64, 0.60], salt)),
        tile::STONE_BRICKS => {
            let base = scale(stone_color(x, y, u, v), 1.02);
            let row = y / 16;
            let bx = (x + if row == 1 { 8 } else { 0 }).rem_euclid(32);
            if bx % 16 == 0 || y % 16 == 0 {
                px(scale(base, 0.58))
            } else if bx % 16 == 1 || y % 16 == 1 {
                px(scale(base, 1.12))
            } else {
                px(scale(base, 0.88 + 0.2 * rnd(bx / 16, row, salt)))
            }
        }
        tile::MOSSY_COBBLE => {
            let mut c = cobble_color(x, y, u, v, tile::COBBLE as u32 * 31 + 5);
            let moss = vnoise(u, v, 5, 5, salt) * 0.7 + 0.3 * rnd(x, y, salt + 1);
            if moss > 0.62 {
                c = mix(c, scale([0.24, 0.46, 0.18], 0.8 + 0.4 * rnd(x, y, salt + 2)), 0.8);
            }
            px(c)
        }
        tile::GRANITE => px(speckled(x, y, u, v, [0.60, 0.40, 0.34], [0.78, 0.55, 0.48], [0.38, 0.24, 0.20], salt)),
        tile::DIORITE => px(speckled(x, y, u, v, [0.74, 0.74, 0.75], [0.90, 0.90, 0.90], [0.42, 0.42, 0.44], salt)),
        tile::ANDESITE => px(speckled(x, y, u, v, [0.54, 0.54, 0.55], [0.64, 0.64, 0.64], [0.38, 0.38, 0.40], salt)),
        tile::GLASS => {
            let edge = x.min(y).min(31 - x).min(31 - y);
            if edge == 0 {
                return [215, 235, 240, 255];
            }
            if edge == 1 {
                return [170, 205, 215, 235];
            }
            // Shine streaks running diagonally.
            let d = (x - y).abs();
            if (6..9).contains(&d) || (x + y == 20 && x > 3) || (x + y == 22 && x > 3) {
                return [235, 248, 252, 170];
            }
            [190, 225, 235, 38]
        }
        tile::TORCH_SIDE => {
            // Only the pole (x 14..18, from y 12 down) is ever sampled.
            if !(14..18).contains(&x) || y < 12 {
                return TRANSPARENT;
            }
            if y < 18 {
                // Flame head: glowing yellow core fading to orange.
                let core = (x == 15 || x == 16) && y < 16;
                let c = if core { [1.0, 0.95, 0.55] } else { [1.0, 0.62, 0.12] };
                return px(scale(c, 0.92 + 0.16 * rnd(x, y, salt)));
            }
            let c = if x == 14 || x == 17 { [0.30, 0.20, 0.09] } else { [0.50, 0.35, 0.16] };
            px(scale(c, 0.9 + 0.2 * rnd(x, y, salt + 1)))
        }
        tile::TORCH_TOP => {
            if (14..18).contains(&x) && (14..18).contains(&y) {
                let core = (15..17).contains(&x) && (15..17).contains(&y);
                px(if core { [1.0, 0.97, 0.7] } else { [1.0, 0.7, 0.2] })
            } else {
                TRANSPARENT
            }
        }
        tile::CRAFTING_TOP => {
            let mut c = planks_color(x, y, u, v, tile::PLANKS as u32 * 31 + 5);
            let edge = x.min(y).min(31 - x).min(31 - y);
            if edge < 2 {
                c = scale(c, 0.55);
            } else if (x - 16).abs() < 9 && (y - 16).abs() < 9 && ((x - 7) % 6 == 0 || (y - 7) % 6 == 0) {
                // 3x3 crafting grid lines.
                c = scale(c, 0.5);
            }
            px(c)
        }
        tile::CRAFTING_SIDE => {
            let mut c = scale(planks_color(x, y, u, v, tile::PLANKS as u32 * 31 + 5), 0.85);
            let edge = x.min(31 - x);
            if edge < 2 || y < 3 {
                c = scale(c, 0.6);
            }
            // A saw (left) and a hammer (right) hang on the front.
            if (5..14).contains(&x) && (10..13).contains(&y) {
                c = [0.75, 0.77, 0.80];
                if (x + y) % 3 == 0 {
                    c = scale(c, 0.7);
                }
            }
            if (5..8).contains(&x) && (13..18).contains(&y) {
                c = [0.40, 0.26, 0.12];
            }
            if (21..24).contains(&x) && (12..24).contains(&y) {
                c = [0.40, 0.26, 0.12];
            }
            if (18..28).contains(&x) && (9..13).contains(&y) {
                c = [0.45, 0.46, 0.50];
            }
            px(c)
        }
        tile::FURNACE_SIDE => {
            let mut c = scale(cobble_color(x, y, u, v, tile::COBBLE as u32 * 31 + 5), 0.95);
            // Firebox opening with embers.
            if (8..24).contains(&x) && (14..27).contains(&y) {
                let edge = x == 8 || x == 23 || y == 14;
                c = if edge { [0.12, 0.12, 0.13] } else { [0.05, 0.04, 0.04] };
                if y >= 23 && !edge {
                    let ember = rnd(x, y, salt);
                    c = mix([0.30, 0.08, 0.02], [1.0, 0.55, 0.10], ember);
                }
            }
            // Vent slits above.
            if (9..23).contains(&x) && (y == 6 || y == 9) {
                c = [0.10, 0.10, 0.11];
            }
            px(c)
        }
        tile::FURNACE_TOP => {
            let mut c = scale(stone_color(x, y, u, v), 0.9);
            let edge = x.min(y).min(31 - x).min(31 - y);
            if edge < 2 {
                c = scale(c, 0.7);
            }
            if (9..23).contains(&x) && (9..23).contains(&y) {
                c = scale(c, 0.55);
            }
            px(c)
        }
        tile::NETHERRACK => {
            let n = fbm(u, v, salt);
            let mut c = mix([0.34, 0.11, 0.11], [0.55, 0.22, 0.20], n);
            if vnoise(u, v, 6, 6, salt + 4) > 0.7 {
                c = scale(c, 0.65);
            }
            c = scale(c, 0.88 + 0.24 * rnd(x, y, salt + 1));
            if rnd(x, y, salt + 7) > 0.97 {
                c = mix(c, [0.8, 0.4, 0.3], 0.5);
            }
            px(c)
        }
        tile::NETHER_BRICKS => px(bricks(x, y, 16, 8, [0.26, 0.09, 0.11], [0.12, 0.05, 0.07], salt)),
        tile::GLOWSTONE => {
            let (d1, d2, id) = worley(u, v, 5, salt);
            let bright = 0.75 + 0.5 * (id & 0xFF) as f32 / 255.0;
            let mut c = scale([1.0, 0.82, 0.42], bright.min(1.15));
            if d2 - d1 < 0.07 {
                c = scale([0.85, 0.55, 0.2], 0.9);
            }
            px(scale(c, 0.93 + 0.14 * rnd(x, y, salt + 1)))
        }
        tile::OBSIDIAN => {
            let n = fbm(u, v, salt);
            let mut c = mix([0.05, 0.03, 0.09], [0.14, 0.08, 0.24], n);
            if vnoise(u, v, 10, 3, salt + 9) > 0.72 {
                c = mix(c, [0.34, 0.20, 0.52], 0.5);
            }
            px(scale(c, 0.9 + 0.2 * rnd(x, y, salt + 1)))
        }
        tile::QUARTZ => {
            let mut c = scale([0.93, 0.91, 0.87], 0.96 + 0.06 * fbm(u, v, salt));
            if y % 16 == 0 {
                c = scale(c, 0.88);
            }
            if x % 16 == 0 {
                c = scale(c, 0.93);
            }
            px(scale(c, 0.97 + 0.05 * rnd(x, y, salt + 1)))
        }
        tile::COAL_BLOCK => {
            let (d1, d2, id) = worley(u, v, 5, salt);
            let shade = 0.5 + 0.9 * (id & 0xFF) as f32 / 255.0;
            let mut c = scale([0.10, 0.10, 0.12], shade);
            if d2 - d1 < 0.06 {
                c = scale(c, 0.5);
            }
            px(scale(c, 0.9 + 0.2 * rnd(x, y, salt + 1)))
        }
        tile::IRON_BLOCK => px(bevel(x, y, [0.80, 0.80, 0.82], [0.92, 0.92, 0.94], salt)),
        tile::GOLD_BLOCK => px(bevel(x, y, [0.96, 0.78, 0.22], [1.0, 0.93, 0.45], salt)),
        tile::DIAMOND_BLOCK => px(bevel(x, y, [0.30, 0.82, 0.80], [0.70, 1.0, 0.98], salt)),
        tile::WOOL_WHITE | tile::WOOL_RED => {
            let base = if tile_id == tile::WOOL_WHITE { [0.92, 0.92, 0.90] } else { [0.66, 0.14, 0.13] };
            // Fuzzy 2x2 tufts.
            let tuft = rnd(x / 2, y / 2, salt);
            let c = scale(base, 0.86 + 0.2 * tuft + 0.06 * rnd(x, y, salt + 1));
            px(c)
        }
        tile::BOOKSHELF => {
            let planks = planks_color(x, y, u, v, tile::PLANKS as u32 * 31 + 5);
            // Planks frame at the top, bottom and between the two shelves.
            if y < 3 || y >= 29 || (14..18).contains(&y) {
                return px(planks);
            }
            let shelf = y / 15;
            // Books of varying width and colour, side by side.
            let (mut bx, mut i) = (0, 0);
            loop {
                let bw = 2 + (hash(i, shelf, salt) % 3) as i32;
                if x < bx + bw {
                    break;
                }
                bx += bw;
                i += 1;
            }
            let book = hash(i, shelf + 4, salt + 3);
            let palette: [Rgb; 6] = [
                [0.55, 0.12, 0.10],
                [0.15, 0.30, 0.55],
                [0.20, 0.48, 0.20],
                [0.55, 0.40, 0.15],
                [0.38, 0.20, 0.45],
                [0.50, 0.50, 0.46],
            ];
            let mut c = palette[(book % 6) as usize];
            // Books stand at different heights.
            let gap = 4 + ((book >> 4) % 3) as i32;
            if y - shelf * 15 < gap {
                c = scale(planks, 0.45);
            }
            if x == bx {
                c = scale(c, 0.6);
            }
            px(scale(c, 0.9 + 0.2 * rnd(x, y, salt + 5)))
        }
        tile::SMOOTH_STONE => {
            let edge = x.min(y).min(31 - x).min(31 - y);
            let mut c = scale([0.62, 0.62, 0.64], 0.96 + 0.06 * fbm(u, v, salt) + 0.03 * rnd(x, y, salt + 1));
            if edge == 0 {
                c = scale(c, 0.82);
            }
            px(c)
        }
        tile::NETHER_TILES => {
            let (lx, ly) = (x % 16, y % 16);
            let mut c = scale([0.17, 0.11, 0.20], 0.85 + 0.3 * rnd(x / 16, y / 16, salt));
            if lx == 0 || ly == 0 {
                c = [0.10, 0.06, 0.12];
            } else if lx == 1 || ly == 1 {
                // Golden inlay line.
                c = [0.62, 0.45, 0.16];
            } else if lx == 15 || ly == 15 {
                c = scale(c, 0.7);
            }
            px(scale(c, 0.94 + 0.12 * rnd(x, y, salt + 1)))
        }
        _ => [255, 0, 255, 255],
    }
}
