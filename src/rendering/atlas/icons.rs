//! Armor icons, isometric block icons, item sprites and HUD sprites. Everything is computed from
//! geometry or from the block textures, no pixel art is stored.

use super::{Rgb, mix, px, rnd, scale};
use crate::world::block::{BlockId, Face, Shape, tile};

const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];
const OUTLINE: Rgb = [0.10, 0.09, 0.12];

pub(super) fn paint(tile_id: u8, x: i32, y: i32) -> [u8; 4] {
    match tile_id {
        tile::HELMET..=tile::BOOTS => armor_icon(tile_id - tile::HELMET, x, y),
        t if (tile::ICON_BLOCK..tile::ICON_BLOCK + BlockId::COUNT).contains(&t) => {
            block_icon(BlockId(t - tile::ICON_BLOCK), x, y)
        }
        t if (tile::ICON_TOOL..tile::ICON_TOOL + 20).contains(&t) => tool_icon(t - tile::ICON_TOOL, x, y),
        t if (tile::ICON_ITEM..tile::ICON_TOOL).contains(&t) => item_icon(t, x, y),
        t if (tile::UI..=tile::UI_ARROW_FULL).contains(&t) => ui_sprite(t, x, y),
        _ => TRANSPARENT,
    }
}

// -------------------------------------------------------------------------------------------------
// Armor
// -------------------------------------------------------------------------------------------------

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
        return TRANSPARENT;
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

// -------------------------------------------------------------------------------------------------
// Isometric block icons
// -------------------------------------------------------------------------------------------------

/// A parallelogram `origin + u * eu + v * ev` in icon pixels; returns `(u, v)` if `p` is inside.
fn in_quad(p: (f32, f32), origin: (f32, f32), eu: (f32, f32), ev: (f32, f32)) -> Option<(f32, f32)> {
    let det = eu.0 * ev.1 - eu.1 * ev.0;
    let (dx, dy) = (p.0 - origin.0, p.1 - origin.1);
    let u = (dx * ev.1 - dy * ev.0) / det;
    let v = (eu.0 * dy - eu.1 * dx) / det;
    ((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v)).then_some((u, v))
}

fn block_icon(block: BlockId, x: i32, y: i32) -> [u8; 4] {
    match block.shape() {
        Shape::Torch => return torch_icon(x, y),
        Shape::Fence => return fence_icon(x, y),
        Shape::Cube => {}
    }
    let p = (x as f32 + 0.5, y as f32 + 0.5);
    // Cube: top rhombus, left and right parallelograms (2:1 dimetric projection).
    let faces = [
        (Face::Top, (3.0, 8.0), (13.0, -7.0), (13.0, 7.0), 1.0),
        (Face::Side, (3.0, 8.0), (13.0, 7.0), (0.0, 16.0), 0.78),
        (Face::Side, (16.0, 15.0), (13.0, -7.0), (0.0, 16.0), 0.58),
    ];
    for (face, origin, eu, ev, shade) in faces {
        if let Some((u, v)) = in_quad(p, origin, eu, ev) {
            let t = block.tile(face).min(tile::BLOCK_TILES - 1);
            let c = super::paint(t, ((u * 32.0) as i32).clamp(0, 31), ((v * 32.0) as i32).clamp(0, 31));
            let rgb = scale([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0], shade);
            let mut out = px(rgb);
            out[3] = if block.is_translucent() { (c[3] as f32 * 0.9).max(70.0) as u8 } else { 255 };
            return out;
        }
    }
    TRANSPARENT
}

fn torch_icon(x: i32, y: i32) -> [u8; 4] {
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    // Pole.
    if (14.0..18.0).contains(&fx) && (13.0..29.0).contains(&fy) {
        let edge = fx < 15.0 || fx > 17.0;
        let c = if edge { [0.30, 0.20, 0.09] } else { [0.55, 0.38, 0.17] };
        return px(scale(c, 0.9 + 0.2 * rnd(x, y, 5)));
    }
    // Flame.
    let d = ((fx - 16.0).powi(2) / 9.0 + (fy - 9.5).powi(2) / 20.0).sqrt();
    if d < 1.0 {
        let core = mix([1.0, 0.97, 0.7], [1.0, 0.55, 0.1], d.powf(1.4));
        return px(core);
    }
    if d < 1.5 {
        let mut c = px([1.0, 0.75, 0.25]);
        c[3] = (255.0 * (1.5 - d) * 1.4).min(160.0) as u8;
        return c;
    }
    TRANSPARENT
}

fn fence_icon(x: i32, y: i32) -> [u8; 4] {
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    let post = |cx: f32| (cx - 2.5..cx + 2.5).contains(&fx) && (3.0..30.0).contains(&fy);
    let rail = |cy: f32| (3.0..29.0).contains(&fx) && (cy - 2.0..cy + 2.0).contains(&fy);
    if post(7.5) || post(24.5) || rail(11.0) || rail(21.0) {
        let wood = super::paint(tile::PLANKS, x.rem_euclid(32), y.rem_euclid(32));
        let mut c = [wood[0] as f32 / 255.0, wood[1] as f32 / 255.0, wood[2] as f32 / 255.0];
        let edge = !(post(7.5) || post(24.5)) && (fy < 9.5 || fy > 22.5) && false;
        if edge || (post(7.5) && fx < 5.5) || (post(24.5) && fx < 22.5) {
            c = scale(c, 1.1);
        }
        if (post(7.5) && fx > 9.0) || (post(24.5) && fx > 26.0) {
            c = scale(c, 0.78);
        }
        return px(c);
    }
    TRANSPARENT
}

// -------------------------------------------------------------------------------------------------
// Item sprites
// -------------------------------------------------------------------------------------------------

/// Sprites are modelled on a 16x16 grid and drawn at 2x: `sx`/`sy` are the grid coordinates of the
/// pixel centre, and a pixel counts as part of a shape if the shape function says so.
struct Canvas {
    fill: Vec<Option<Rgb>>,
}

impl Canvas {
    const N: i32 = 16;

    fn new() -> Self {
        Self { fill: vec![None; (Self::N * Self::N) as usize] }
    }

    fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if (0..Self::N).contains(&x) && (0..Self::N).contains(&y) {
            self.fill[(y * Self::N + x) as usize] = Some(c);
        }
    }

    fn get(&self, x: i32, y: i32) -> Option<Rgb> {
        if (0..Self::N).contains(&x) && (0..Self::N).contains(&y) { self.fill[(y * Self::N + x) as usize] } else { None }
    }

    /// Draws every grid cell for which `inside` is true, colouring it with `color`.
    fn shape(&mut self, inside: impl Fn(f32, f32) -> bool, color: impl Fn(i32, i32) -> Rgb) {
        for y in 0..Self::N {
            for x in 0..Self::N {
                if inside(x as f32 + 0.5, y as f32 + 0.5) {
                    self.set(x, y, color(x, y));
                }
            }
        }
    }

    /// Final 32x32 pixel with a one-cell dark outline.
    fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        let (gx, gy) = (x.div_euclid(2), y.div_euclid(2));
        if let Some(c) = self.get(gx, gy) {
            return px(c);
        }
        let touches = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| self.get(gx + dx, gy + dy).is_some());
        if touches { px(OUTLINE) } else { TRANSPARENT }
    }
}

fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (ab, ap) = ((b.0 - a.0, b.1 - a.1), (p.0 - a.0, p.1 - a.1));
    let t = ((ap.0 * ab.0 + ap.1 * ab.1) / (ab.0 * ab.0 + ab.1 * ab.1)).clamp(0.0, 1.0);
    let q = (a.0 + ab.0 * t, a.1 + ab.1 * t);
    (((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt(), t)
}

/// Colours of a tool material: `(dark, base, light)`.
fn tier_palette(tier: usize) -> [Rgb; 3] {
    match tier {
        0 => [[0.40, 0.27, 0.12], [0.62, 0.46, 0.24], [0.78, 0.62, 0.36]],
        1 => [[0.36, 0.36, 0.38], [0.55, 0.55, 0.57], [0.72, 0.72, 0.74]],
        2 => [[0.58, 0.60, 0.64], [0.80, 0.82, 0.85], [0.96, 0.97, 0.98]],
        3 => [[0.75, 0.55, 0.08], [0.98, 0.80, 0.20], [1.0, 0.95, 0.55]],
        _ => [[0.10, 0.60, 0.62], [0.30, 0.88, 0.85], [0.75, 1.0, 0.98]],
    }
}

fn tool_icon(index: u8, x: i32, y: i32) -> [u8; 4] {
    let (tier, kind) = ((index / 4) as usize, index % 4);
    let [dark, base, light] = tier_palette(tier);
    let handle = [[0.30, 0.20, 0.09], [0.52, 0.36, 0.16]];
    let mut c = Canvas::new();
    // Handle: from the lower left to the upper right (the sword is held by its grip only).
    let (h0, h1) = match kind {
        3 => ((1.5, 14.5), (5.0, 11.0)),
        _ => ((1.5, 14.5), (11.5, 4.5)),
    };
    c.shape(
        |px_, py| dist_to_segment((px_, py), h0, h1).0 < 0.95,
        |gx, gy| if (gx + gy) % 2 == 0 { handle[1] } else { handle[0] },
    );
    let head = |gx: i32, gy: i32, hi: bool| -> Rgb {
        // Light from the top-left.
        let shade = ((gx + gy) as f32 / 30.0).clamp(0.0, 1.0);
        let base_c = mix(light, dark, shade);
        if hi { light } else { mix(base_c, base, 0.35) }
    };
    match kind {
        0 => {
            // Pickaxe: an arc across the handle tip, curving back towards the handle.
            let (cx, cy) = (11.0f32, 5.0f32);
            c.shape(
                |px_, py| {
                    let (rx, ry) = (px_ - cx, py - cy);
                    let along = (rx - ry) / std::f32::consts::SQRT_2;
                    let across = (rx + ry) / std::f32::consts::SQRT_2;
                    let arc = 1.3 - across * across * 0.10;
                    (along - arc).abs() < 1.15 && across.abs() < 6.2
                },
                |gx, gy| head(gx, gy, (gx + gy) % 5 == 0),
            );
        }
        1 => {
            // Axe: a wedge on one side of the handle tip.
            let (cx, cy) = (11.0f32, 5.0f32);
            c.shape(
                |px_, py| {
                    let (rx, ry) = (px_ - cx, py - cy);
                    let along = (rx - ry) / std::f32::consts::SQRT_2;
                    let across = (rx + ry) / std::f32::consts::SQRT_2;
                    (-1.0..=5.6).contains(&across) && along.abs() <= 3.4 - 0.45 * across.max(0.0) + 0.6
                },
                |gx, gy| head(gx, gy, (gx * 2 + gy) % 6 == 0),
            );
        }
        2 => {
            // Shovel: a rounded blade at the tip.
            let (cx, cy) = (12.2f32, 3.8f32);
            c.shape(
                |px_, py| {
                    let (rx, ry) = (px_ - cx, py - cy);
                    let along = (rx - ry) / std::f32::consts::SQRT_2;
                    let across = (rx + ry) / std::f32::consts::SQRT_2;
                    (across / 2.4).powi(2) + (along / 3.3).powi(2) <= 1.0
                },
                |gx, gy| head(gx, gy, (gx + gy) % 4 == 0),
            );
        }
        _ => {
            // Sword: blade along the diagonal, guard and pommel.
            let tip = (14.3f32, 1.7f32);
            let hilt = (5.2f32, 10.8f32);
            c.shape(
                |px_, py| {
                    let (d, t) = dist_to_segment((px_, py), hilt, tip);
                    d < 1.35 * (1.0 - 0.45 * t * t)
                },
                |gx, gy| {
                    let (_, t) = dist_to_segment((gx as f32 + 0.5, gy as f32 + 0.5), hilt, tip);
                    // A bright edge down the middle of the blade.
                    let edge = (gx - gy + 4).abs() <= 1 && t > 0.1;
                    if edge { light } else { mix(base, dark, (t * 0.4).min(0.4)) }
                },
            );
            // Cross guard.
            c.shape(
                |px_, py| dist_to_segment((px_, py), (3.2, 8.4), (7.8, 13.0)).0 < 0.9,
                |gx, gy| if (gx + gy) % 2 == 0 { [0.46, 0.30, 0.14] } else { [0.62, 0.42, 0.20] },
            );
            // Grip pommel.
            c.set(1, 14, [0.62, 0.42, 0.20]);
        }
    }
    c.pixel(x, y)
}

fn item_icon(tile_id: u8, x: i32, y: i32) -> [u8; 4] {
    let mut c = Canvas::new();
    let h = |gx: i32, gy: i32, salt: u32| rnd(gx, gy, salt);
    match tile_id {
        tile::ICON_STICK => {
            c.shape(
                |px_, py| dist_to_segment((px_, py), (2.5, 13.5), (13.0, 3.0)).0 < 1.05,
                |gx, gy| if (gx + gy) % 3 == 0 { [0.34, 0.22, 0.09] } else { [0.60, 0.43, 0.20] },
            );
        }
        tile::ICON_COAL => {
            for (cx, cy, r) in [(7.0f32, 9.0f32, 4.6f32), (10.5, 7.0, 3.4), (5.5, 6.0, 2.8)] {
                c.shape(
                    |px_, py| (px_ - cx).powi(2) + (py - cy).powi(2) < r * r,
                    |gx, gy| {
                        let shade = 0.55 + 0.7 * h(gx, gy, 3);
                        let hi = (gx + gy) < 13 && h(gx, gy, 4) > 0.55;
                        if hi { [0.38, 0.38, 0.42] } else { scale([0.14, 0.14, 0.16], shade) }
                    },
                );
            }
        }
        tile::ICON_IRON_INGOT | tile::ICON_GOLD_INGOT => {
            let (base, dark, light) = if tile_id == tile::ICON_IRON_INGOT {
                ([0.82, 0.83, 0.86], [0.55, 0.56, 0.60], [0.97, 0.98, 1.0])
            } else {
                ([0.98, 0.80, 0.20], [0.72, 0.52, 0.08], [1.0, 0.95, 0.55])
            };
            // Sloped top face and a darker front face.
            c.shape(
                |px_, py| {
                    let top = py > 5.0 && py < 9.5 && px_ > 4.0 + (9.5 - py) * 0.6 && px_ < 13.5 - (py - 5.0) * 0.3;
                    let front = py >= 9.5 && py < 12.5 && px_ > 2.5 && px_ < 11.5;
                    top || front
                },
                |gx, gy| {
                    if gy >= 9 {
                        mix(dark, base, 0.2 + 0.3 * h(gx, gy, 1))
                    } else if gy == 6 || gx == 6 {
                        light
                    } else {
                        mix(base, light, 0.3 * h(gx, gy, 2))
                    }
                },
            );
        }
        tile::ICON_DIAMOND => {
            c.shape(
                |px_, py| (px_ - 8.0).abs() / 6.0 + (py - 8.5).abs() / 6.5 <= 1.0,
                |gx, gy| {
                    let facet = (gx as f32 - 8.0).abs() < 1.0 || (gy as f32 - 8.0).abs() < 0.8;
                    let top = gy < 7;
                    if (gx + gy) < 9 && h(gx, gy, 2) > 0.4 {
                        [0.88, 1.0, 1.0]
                    } else if facet {
                        [0.45, 0.92, 0.92]
                    } else if top {
                        [0.30, 0.82, 0.84]
                    } else {
                        [0.12, 0.56, 0.62]
                    }
                },
            );
        }
        tile::ICON_REDSTONE => {
            for (cx, cy, r) in [(6.5f32, 9.5f32, 3.4f32), (10.0, 8.0, 3.0), (8.0, 11.5, 2.8)] {
                c.shape(
                    |px_, py| (px_ - cx).powi(2) + (py - cy).powi(2) < r * r,
                    |gx, gy| mix([0.45, 0.02, 0.02], [0.95, 0.12, 0.10], h(gx, gy, 6)),
                );
            }
        }
        tile::ICON_APPLE => {
            c.shape(
                |px_, py| {
                    let d = (px_ - 8.0).powi(2) / 30.0 + (py - 9.5).powi(2) / 27.0;
                    d <= 1.0 && !((px_ - 8.0).abs() < 0.8 && py < 5.2)
                },
                |gx, gy| {
                    let hi = (gx - 5).abs() <= 1 && (6..=8).contains(&gy);
                    if hi { [1.0, 0.55, 0.50] } else { scale([0.82, 0.10, 0.08], 0.85 + 0.3 * h(gx, gy, 8) - gy as f32 * 0.01) }
                },
            );
            c.shape(|px_, py| (px_ - 8.4).abs() < 0.7 && (2.5..5.5).contains(&py), |_, _| [0.35, 0.22, 0.08]);
            c.shape(
                |px_, py| ((px_ - 10.5) / 2.2).powi(2) + ((py - 3.4) / 1.2).powi(2) <= 1.0,
                |_, _| [0.25, 0.65, 0.18],
            );
        }
        tile::ICON_BREAD => {
            c.shape(
                |px_, py| ((px_ - 8.0) / 6.8).powi(2) + ((py - 9.0) / 3.8).powi(2) <= 1.0,
                |gx, gy| {
                    let slash = (gx + gy * 2) % 6 == 0 && gy < 9;
                    let crust = gy >= 10;
                    if slash {
                        [0.88, 0.70, 0.40]
                    } else if crust {
                        [0.58, 0.33, 0.12]
                    } else {
                        scale([0.78, 0.52, 0.22], 0.9 + 0.2 * h(gx, gy, 9))
                    }
                },
            );
        }
        tile::ICON_RAW_PORK | tile::ICON_COOKED_PORK | tile::ICON_RAW_BEEF | tile::ICON_COOKED_BEEF => {
            let (meat, fat) = match tile_id {
                tile::ICON_RAW_PORK => ([0.92, 0.55, 0.55], [0.98, 0.82, 0.78]),
                tile::ICON_COOKED_PORK => ([0.68, 0.38, 0.20], [0.85, 0.62, 0.40]),
                tile::ICON_RAW_BEEF => ([0.74, 0.17, 0.16], [0.90, 0.62, 0.58]),
                _ => ([0.50, 0.25, 0.12], [0.70, 0.48, 0.30]),
            };
            let cooked = matches!(tile_id, tile::ICON_COOKED_PORK | tile::ICON_COOKED_BEEF);
            c.shape(
                |px_, py| ((px_ - 8.5) / 5.6).powi(2) + ((py - 8.0) / 4.6).powi(2) <= 1.0 + 0.15 * (px_ * 1.7).sin(),
                |gx, gy| {
                    let rim = ((gx as f32 + 0.5 - 8.5) / 5.6).powi(2) + ((gy as f32 + 0.5 - 8.0) / 4.6).powi(2) > 0.55;
                    if rim {
                        mix(fat, meat, 0.3 * h(gx, gy, 3))
                    } else if cooked && (gx + gy) % 4 == 0 {
                        scale(meat, 0.7)
                    } else {
                        scale(meat, 0.88 + 0.24 * h(gx, gy, 4))
                    }
                },
            );
            // A small bone at the lower left.
            c.shape(|px_, py| dist_to_segment((px_, py), (2.4, 13.4), (5.6, 10.4)).0 < 0.9, |_, _| [0.95, 0.93, 0.85]);
            c.shape(|px_, py| (px_ - 1.9).powi(2) + (py - 13.9).powi(2) < 2.0, |_, _| [0.95, 0.93, 0.85]);
        }
        _ => {
            // Rotten flesh: a ragged green-brown slab.
            c.shape(
                |px_, py| {
                    let wob = 0.9 * (px_ * 1.9).sin() + 0.7 * (py * 2.3).cos();
                    ((px_ - 8.0) / 6.2).powi(2) + ((py - 8.5) / 4.8).powi(2) <= 1.0 + 0.14 * wob
                },
                |gx, gy| {
                    let n = h(gx, gy, 12);
                    if n > 0.75 {
                        [0.34, 0.50, 0.20]
                    } else if n < 0.25 {
                        [0.40, 0.18, 0.12]
                    } else {
                        [0.52, 0.34, 0.20]
                    }
                },
            );
        }
    }
    c.pixel(x, y)
}

// -------------------------------------------------------------------------------------------------
// HUD sprites (9x9 grids drawn at 3x inside a 32 px tile, offset 2)
// -------------------------------------------------------------------------------------------------

const HEART: [&str; 9] =
    [".........", "..##.##..", ".#######.", "#########", "#########", ".#######.", "..#####..", "...###...", "....#...."];
const DRUMSTICK: [&str; 9] =
    ["......##.", ".....####", "....#####", "...######", "..#####..", ".#####...", "bb.##....", "bbb#.....", ".bb......"];
const CHESTPLATE: [&str; 9] =
    [".........", "###...###", "####.####", "#########", "#########", ".#######.", ".#######.", ".#######.", ".#######."];
const BUBBLE: [&str; 9] =
    ["..#####..", ".#.....#.", "#..##...#", "#.#.....#", "#.......#", "#.......#", "#.......#", ".#.....#.", "..#####.."];

/// One 9x9 sprite pixel: `' '` = nothing, otherwise `(fill?, outline)`.
fn mask_at(mask: &[&str; 9], gx: i32, gy: i32) -> char {
    if (0..9).contains(&gx) && (0..9).contains(&gy) { mask[gy as usize].as_bytes()[gx as usize] as char } else { '.' }
}

fn sprite_mask(mask: &[&str; 9], x: i32, y: i32, fill: impl Fn(i32, i32, char) -> Option<Rgb>) -> [u8; 4] {
    let (gx, gy) = ((x - 2).div_euclid(3), (y - 2).div_euclid(3));
    if !(0..27).contains(&(x - 2)) || !(0..27).contains(&(y - 2)) {
        return TRANSPARENT;
    }
    let ch = mask_at(mask, gx, gy);
    if ch != '.' {
        return fill(gx, gy, ch).map_or(TRANSPARENT, px);
    }
    let touches = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| mask_at(mask, gx + dx, gy + dy) != '.');
    if touches { px([0.0, 0.0, 0.0]) } else { TRANSPARENT }
}

fn ui_sprite(tile_id: u8, x: i32, y: i32) -> [u8; 4] {
    match tile_id {
        tile::UI_HEART_FULL | tile::UI_HEART_HALF | tile::UI_HEART_EMPTY => {
            let state = tile_id - tile::UI_HEART_FULL; // 0 full, 1 half, 2 empty
            sprite_mask(&HEART, x, y, |gx, gy, _| {
                let filled = state == 0 || (state == 1 && gx < 5);
                Some(if filled {
                    if gx <= 2 && gy <= 3 && gy >= 1 && gx >= 1 {
                        [1.0, 0.65, 0.65]
                    } else if gy >= 6 {
                        [0.62, 0.03, 0.05]
                    } else {
                        [0.92, 0.10, 0.12]
                    }
                } else {
                    [0.22, 0.04, 0.05]
                })
            })
        }
        tile::UI_FOOD_FULL | tile::UI_FOOD_HALF | tile::UI_FOOD_EMPTY => {
            let state = tile_id - tile::UI_FOOD_FULL;
            sprite_mask(&DRUMSTICK, x, y, |gx, gy, ch| {
                if ch == 'b' {
                    return Some(if state == 2 { [0.30, 0.28, 0.25] } else { [0.95, 0.92, 0.82] });
                }
                // The right/top half of a half-full drumstick is empty.
                let filled = state == 0 || (state == 1 && gx + (8 - gy) <= 9);
                Some(if filled {
                    if gy <= 3 && gx >= 5 { [0.85, 0.50, 0.20] } else { [0.62, 0.32, 0.12] }
                } else {
                    [0.20, 0.13, 0.08]
                })
            })
        }
        tile::UI_BUBBLE => sprite_mask(&BUBBLE, x, y, |gx, gy, _| {
            Some(if gx <= 3 && gy <= 3 { [0.95, 1.0, 1.0] } else { [0.45, 0.70, 1.0] })
        }),
        tile::UI_ARMOR_FULL | tile::UI_ARMOR_HALF | tile::UI_ARMOR_EMPTY => {
            let state = tile_id - tile::UI_ARMOR_FULL;
            sprite_mask(&CHESTPLATE, x, y, |gx, _, _| {
                let filled = state == 0 || (state == 1 && gx < 5);
                Some(if filled { [0.82, 0.85, 0.90] } else { [0.20, 0.21, 0.24] })
            })
        }
        tile::UI_ARROW | tile::UI_ARROW_FULL => {
            // Right-pointing arrow, 22 x 15 px, centred. `UI_ARROW + 3` is the "filled" version.
            let filled = tile_id != tile::UI_ARROW;
            let (ax, ay) = (x - 5, y - 8);
            let shaft = (0..14).contains(&ax) && (5..10).contains(&ay);
            let head = (14..22).contains(&ax) && (ay - 7).abs() <= (22 - ax) / 2 + 0;
            if shaft || head {
                let c = if filled { [0.95, 0.95, 0.95] } else { [0.45, 0.45, 0.47] };
                return px(if ax < 1 || ay == 5 { scale(c, 1.1) } else { c });
            }
            TRANSPARENT
        }
        tile::UI_FLAME | tile::UI_FLAME_OFF => {
            let lit = tile_id == tile::UI_FLAME;
            let (fx, fy) = (x as f32 - 16.0, y as f32 - 16.0);
            let width = 8.0 * (1.0 - ((fy + 4.0) / 14.0).powi(2)).max(0.0).sqrt() * (0.55 + 0.45 * ((fy + 14.0) / 28.0));
            if fy > -14.0 && fy < 12.0 && fx.abs() < width {
                let t = ((fy + 14.0) / 26.0).clamp(0.0, 1.0);
                let c = if lit { mix([1.0, 0.95, 0.4], [0.95, 0.35, 0.05], t) } else { [0.22, 0.20, 0.20] };
                return px(c);
            }
            TRANSPARENT
        }
        _ => TRANSPARENT,
    }
}
