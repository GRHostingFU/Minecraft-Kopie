//! Procedural mob models: box-built bodies textured from a small generated atlas (no external
//! art), plus the materials that tint them for daylight, damage, fire and the creeper's flash.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// One model pixel in world units.
pub const PX: f32 = 1.0 / 16.0;
const TILE: usize = 16;
const COLS: usize = 4;
const ROWS: usize = 2;

/// Tiles of the mob atlas.
pub mod mob_tile {
    pub const CREEPER_SKIN: u8 = 0;
    pub const CREEPER_FACE: u8 = 1;
    pub const BONE: u8 = 2;
    pub const SKULL: u8 = 3;
    pub const WOOD: u8 = 4;
    pub const ARROW: u8 = 5;
    pub const FEATHER: u8 = 6;
    pub const STRING: u8 = 7;
}

fn hash(x: i32, y: i32, salt: u32) -> u32 {
    let mut h =
        (x as u32).wrapping_mul(0x85EB_CA6B) ^ (y as u32).wrapping_mul(0xC2B2_AE35) ^ salt.wrapping_mul(0x27D4_EB2F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h
}

fn rnd(x: i32, y: i32, salt: u32) -> f32 {
    (hash(x, y, salt) & 0xFFFF) as f32 / 65536.0
}

fn rgb(c: [f32; 3], k: f32) -> [u8; 4] {
    let q = |v: f32| (v * k * 255.0).clamp(0.0, 255.0) as u8;
    [q(c[0]), q(c[1]), q(c[2]), 255]
}

fn paint(tile: u8, x: i32, y: i32) -> [u8; 4] {
    let salt = tile as u32 * 17 + 3;
    match tile {
        mob_tile::CREEPER_SKIN | mob_tile::CREEPER_FACE => {
            // Camouflage: blotches of 2x2 pixels in several greens.
            let blotch = rnd(x / 2, y / 2, salt);
            let k = 0.65 + 0.7 * blotch;
            let mut px = rgb([0.32, 0.62, 0.24], k);
            if tile == mob_tile::CREEPER_FACE {
                let rect = |x0: i32, y0: i32, x1: i32, y1: i32| (x0..=x1).contains(&x) && (y0..=y1).contains(&y);
                let dark = rect(3, 4, 5, 6)
                    || rect(10, 4, 12, 6)
                    || rect(7, 7, 8, 10)
                    || rect(5, 9, 6, 13)
                    || rect(9, 9, 10, 13);
                if dark {
                    px = rgb([0.03, 0.05, 0.03], 1.0);
                }
            }
            px
        }
        mob_tile::BONE | mob_tile::SKULL => {
            let streak = rnd(x, y / 3, salt);
            let mut px = rgb([0.86, 0.85, 0.78], 0.8 + 0.3 * streak);
            if tile == mob_tile::SKULL {
                let rect = |x0: i32, y0: i32, x1: i32, y1: i32| (x0..=x1).contains(&x) && (y0..=y1).contains(&y);
                if rect(3, 5, 6, 8) || rect(9, 5, 12, 8) {
                    px = rgb([0.05, 0.05, 0.06], 1.0);
                } else if rect(7, 9, 8, 10) {
                    px = rgb([0.35, 0.33, 0.3], 1.0);
                } else if rect(4, 11, 11, 13) && (x % 2 == 0 || y == 11) {
                    px = rgb([0.12, 0.11, 0.1], 1.0);
                }
            } else if y % 5 == 0 {
                px = rgb([0.55, 0.54, 0.5], 1.0);
            }
            px
        }
        mob_tile::WOOD => rgb([0.45, 0.31, 0.16], 0.75 + 0.5 * rnd(x, y / 4, salt)),
        mob_tile::ARROW => rgb([0.50, 0.37, 0.2], 0.8 + 0.4 * rnd(x, y, salt)),
        mob_tile::FEATHER => rgb([0.92, 0.92, 0.9], 0.85 + 0.2 * rnd(x, y, salt)),
        mob_tile::STRING => rgb([0.85, 0.85, 0.8], 1.0),
        _ => [255, 0, 255, 255],
    }
}

pub fn build_atlas_image() -> Image {
    let (w, h) = (COLS * TILE, ROWS * TILE);
    let mut data = vec![0u8; w * h * 4];
    for t in 0..(COLS * ROWS) as u8 {
        let (col, row) = (t as usize % COLS, t as usize / COLS);
        for y in 0..TILE {
            for x in 0..TILE {
                let o = ((row * TILE + y) * w + col * TILE + x) * 4;
                data[o..o + 4].copy_from_slice(&paint(t, x as i32, y as i32));
            }
        }
    }
    let mut image = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        ..default()
    });
    image
}

/// UV rectangle of a tile, inset by half a texel against bleeding.
pub fn tile_uv(tile: u8) -> Rect {
    let (col, row) = (tile as usize % COLS, tile as usize / COLS);
    let (w, h) = ((COLS * TILE) as f32, (ROWS * TILE) as f32);
    let min = Vec2::new((col * TILE) as f32 + 0.5, (row * TILE) as f32 + 0.5);
    Rect::from_corners(min / Vec2::new(w, h), (min + Vec2::splat(TILE as f32 - 1.0)) / Vec2::new(w, h))
}

/// Face order of [`BoxSpec::tiles`]: +X, -X, +Y, -Y, +Z, -Z.
#[derive(Clone, Copy, Debug)]
pub struct BoxSpec {
    /// Corners in pixels, relative to the part's pivot.
    pub min: Vec3,
    pub max: Vec3,
    pub tiles: [u8; 6],
}

impl BoxSpec {
    pub fn all(min: Vec3, max: Vec3, tile: u8) -> Self {
        Self { min, max, tiles: [tile; 6] }
    }

    /// Like [`BoxSpec::all`] but with another tile on the front (-Z) face.
    pub fn with_front(mut self, tile: u8) -> Self {
        self.tiles[5] = tile;
        self
    }
}

/// Merges boxes (positions in pixels) into one mesh (positions in blocks).
pub fn boxes_mesh(boxes: &[BoxSpec]) -> Mesh {
    let (mut pos, mut nor, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    const FACES: [(Vec3, Vec3, Vec3); 6] = [
        // normal, u axis, v axis (right-handed so that u x v = normal)
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
    ];
    for b in boxes {
        let centre = (b.min + b.max) * 0.5 * PX;
        let half = (b.max - b.min) * 0.5 * PX;
        for (i, (n, u, v)) in FACES.iter().enumerate() {
            let rect = tile_uv(b.tiles[i]);
            let c = centre + *n * (half * n.abs()).length();
            let (hu, hv) = ((half * u.abs()).length(), (half * v.abs()).length());
            let base = pos.len() as u32;
            for (su, sv, tu, tv) in
                [(-1.0, -1.0, 0.0, 1.0), (1.0, -1.0, 1.0, 1.0), (1.0, 1.0, 1.0, 0.0), (-1.0, 1.0, 0.0, 0.0)]
            {
                let p = c + *u * hu * su + *v * hv * sv;
                pos.push(p.to_array());
                nor.push(n.to_array());
                uv.push([rect.min.x + (rect.max.x - rect.min.x) * tu, rect.min.y + (rect.max.y - rect.min.y) * tv]);
            }
            idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartKind {
    Head,
    /// The body; never animated.
    Torso,
    /// Index 0..4: front-left, front-right, back-left, back-right (creeper) or left, right (skeleton).
    Leg(u8),
    ArmLeft,
    ArmRight,
}

pub struct PartSpec {
    pub kind: PartKind,
    /// Pivot in blocks, relative to the feet.
    pub pivot: Vec3,
    pub boxes: Vec<BoxSpec>,
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// The creeper: a tall body on four stubby legs with a big head. 26 px (1.625 blocks) tall.
pub fn creeper_parts() -> Vec<PartSpec> {
    let skin = mob_tile::CREEPER_SKIN;
    let mut parts = vec![PartSpec {
        kind: PartKind::Head,
        pivot: v(0.0, 18.0, 0.0) * PX,
        boxes: vec![BoxSpec::all(v(-4.0, 0.0, -4.0), v(4.0, 8.0, 4.0), skin).with_front(mob_tile::CREEPER_FACE)],
    }];
    parts.push(PartSpec {
        kind: PartKind::Torso,
        pivot: v(0.0, 6.0, 0.0) * PX,
        boxes: vec![BoxSpec::all(v(-4.0, 0.0, -2.0), v(4.0, 12.0, 2.0), skin)],
    });
    for (i, (x, z)) in [(-2.0, -4.0), (2.0, -4.0), (-2.0, 4.0), (2.0, 4.0)].into_iter().enumerate() {
        parts.push(PartSpec {
            kind: PartKind::Leg(i as u8),
            pivot: v(x, 6.0, z) * PX,
            boxes: vec![BoxSpec::all(v(-2.0, -6.0, -2.0), v(2.0, 0.0, 2.0), skin)],
        });
    }
    parts
}

/// The skeleton: thin bony limbs, a skull and a bow in the right hand. 32 px (2 blocks) tall.
pub fn skeleton_parts() -> Vec<PartSpec> {
    let bone = mob_tile::BONE;
    let mut parts = vec![
        PartSpec {
            kind: PartKind::Head,
            pivot: v(0.0, 24.0, 0.0) * PX,
            boxes: vec![BoxSpec::all(v(-4.0, 0.0, -4.0), v(4.0, 8.0, 4.0), mob_tile::BONE).with_front(mob_tile::SKULL)],
        },
        PartSpec {
            kind: PartKind::ArmLeft,
            pivot: v(-5.0, 22.0, 0.0) * PX,
            boxes: vec![BoxSpec::all(v(-1.0, -10.0, -1.0), v(1.0, 2.0, 1.0), bone)],
        },
        PartSpec {
            kind: PartKind::ArmRight,
            pivot: v(5.0, 22.0, 0.0) * PX,
            boxes: vec![
                BoxSpec::all(v(-1.0, -10.0, -1.0), v(1.0, 2.0, 1.0), bone),
                // The bow: a stave and a string, held at the hand.
                BoxSpec::all(v(-0.5, -16.0, -3.5), v(0.5, -4.0, -2.5), mob_tile::WOOD),
                BoxSpec::all(v(-0.25, -15.0, -2.5), v(0.25, -5.0, -2.0), mob_tile::STRING),
            ],
        },
    ];
    for (i, x) in [-2.0, 2.0].into_iter().enumerate() {
        parts.push(PartSpec {
            kind: PartKind::Leg(i as u8),
            pivot: v(x, 12.0, 0.0) * PX,
            boxes: vec![BoxSpec::all(v(-1.0, -12.0, -1.0), v(1.0, 0.0, 1.0), bone)],
        });
    }
    parts.push(PartSpec {
        kind: PartKind::Torso,
        pivot: v(0.0, 12.0, 0.0) * PX,
        boxes: vec![BoxSpec::all(v(-4.0, 0.0, -2.0), v(4.0, 12.0, 2.0), bone)],
    });
    parts
}

/// An arrow, pointing along -Z, about 0.6 blocks long.
pub fn arrow_mesh() -> Mesh {
    boxes_mesh(&[
        BoxSpec::all(v(-0.5, -0.5, -8.0), v(0.5, 0.5, 4.0), mob_tile::ARROW),
        BoxSpec::all(v(-1.0, -1.0, -10.0), v(1.0, 1.0, -8.0), mob_tile::STRING),
        BoxSpec::all(v(-0.25, -1.5, 2.0), v(0.25, 1.5, 5.0), mob_tile::FEATHER),
        BoxSpec::all(v(-1.5, -0.25, 2.0), v(1.5, 0.25, 5.0), mob_tile::FEATHER),
    ])
}

/// Tinted materials shared by every mob; their colours follow the time of day.
#[derive(Resource)]
pub struct MobMaterials {
    pub normal: Handle<StandardMaterial>,
    pub hurt: Handle<StandardMaterial>,
    pub burning: Handle<StandardMaterial>,
    pub flash: Handle<StandardMaterial>,
    pub arrow: Handle<StandardMaterial>,
}

impl MobMaterials {
    pub fn new(images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>) -> Self {
        let atlas = images.add(build_atlas_image());
        let mut textured = |color: Color| {
            materials.add(StandardMaterial {
                base_color: color,
                base_color_texture: Some(atlas.clone()),
                unlit: true,
                ..default()
            })
        };
        let normal = textured(Color::WHITE);
        let hurt = textured(Color::srgb(1.0, 0.35, 0.35));
        let burning = textured(Color::srgb(1.0, 0.65, 0.3));
        let arrow = textured(Color::WHITE);
        let flash = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() });
        Self { normal, hurt, burning, flash, arrow }
    }
}

/// Brightness of mobs: dim at night, full in daylight.
pub fn brightness(daylight: f32) -> f32 {
    0.42 + 0.58 * daylight.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_rects_stay_inside_the_atlas_and_do_not_overlap() {
        let mut rects = Vec::new();
        for t in 0..(COLS * ROWS) as u8 {
            let r = tile_uv(t);
            assert!(r.min.x >= 0.0 && r.max.x <= 1.0 && r.min.y >= 0.0 && r.max.y <= 1.0);
            rects.push(r);
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(a.intersect(*b).is_empty(), "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn every_tile_is_painted_opaque_and_distinct() {
        for t in 0..8u8 {
            let mut colors = std::collections::HashSet::new();
            for y in 0..16 {
                for x in 0..16 {
                    let p = paint(t, x, y);
                    assert_eq!(p[3], 255, "tile {t} is opaque");
                    assert_ne!(p, [255, 0, 255, 255], "tile {t} is painted");
                    colors.insert(p);
                }
            }
            assert!(t == mob_tile::STRING || colors.len() > 3, "tile {t} has detail");
        }
        assert_ne!(paint(0, 4, 5), paint(1, 4, 5), "the creeper face has eyes");
    }

    #[test]
    fn a_box_has_six_faces_with_outward_normals() {
        let mesh = boxes_mesh(&[BoxSpec::all(v(-4.0, 0.0, -4.0), v(4.0, 8.0, 4.0), 0)]);
        let pos = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap().to_vec();
        let nor = mesh.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap().as_float3().unwrap().to_vec();
        assert_eq!(pos.len(), 24);
        let Some(Indices::U32(idx)) = mesh.indices() else { panic!("indices") };
        assert_eq!(idx.len(), 36);
        for t in idx.chunks(3) {
            let p = |i: u32| Vec3::from(pos[i as usize]);
            let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).normalize();
            assert!((n - Vec3::from(nor[t[0] as usize])).length() < 1e-4, "winding disagrees with normal");
        }
        // Size: 8 px wide, centred on x = 0, standing on y = 0.
        let (lo, hi) =
            pos.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p))));
        assert!((hi - lo - v(8.0, 8.0, 8.0) * PX).length() < 1e-5);
        assert!((lo.x + hi.x).abs() < 1e-6 && lo.y.abs() < 1e-6);
    }

    #[test]
    fn model_heights_match_the_hitboxes() {
        let top = |parts: &[PartSpec]| {
            parts
                .iter()
                .map(|p| p.pivot.y + p.boxes.iter().map(|b| b.max.y * PX).fold(f32::MIN, f32::max))
                .fold(f32::MIN, f32::max)
        };
        assert!((top(&creeper_parts()) - 26.0 * PX).abs() < 1e-5);
        assert!((top(&skeleton_parts()) - 32.0 * PX).abs() < 1e-5);
        assert_eq!(creeper_parts().iter().filter(|p| matches!(p.kind, PartKind::Leg(_))).count(), 4);
    }

    #[test]
    fn brightness_follows_daylight() {
        assert!(brightness(0.0) < brightness(1.0));
        assert!(brightness(1.0) <= 1.0 && brightness(0.0) > 0.3);
    }
}
