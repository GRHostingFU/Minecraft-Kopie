//! Meshes for everything that is not a full cube: fluids with sloped surfaces, redstone dust,
//! torches, levers, buttons, pressure plates and piston heads.
//!
//! These are emitted per block (no greedy merging) as plain boxes / sloped quads in the same
//! vertex format as [`super::mesher`]: `color = (ambient occlusion, sky light, tile index +
//! [`tile::EMISSIVE`] if fully bright, face shade)`. Water goes into the translucent buffer,
//! everything else (lava included) into the opaque one.

use super::mesher::{MeshBuffers, PaddedVolume};
use crate::physics::fluids::fluid_height;
use crate::redstone::rules::{dust_arms, effective_arms};
use crate::world::block::{BlockId, Facing, Kind, framed_box, tile};
use crate::world::chunk::{CHUNK_H, CHUNK_W};
use bevy::math::{IVec3, Vec3};

const PX: f32 = 1.0 / 16.0;

/// Appends one quad. `p` is in any winding; it is flipped to face `normal`.
fn quad(
    out: &mut MeshBuffers,
    p: [[f32; 3]; 4],
    normal: Vec3,
    uv: [[f32; 2]; 4],
    tile_value: f32,
    shade: f32,
    sky: f32,
) {
    let v = |i: usize| Vec3::from(p[i]);
    let facing = (v(1) - v(0)).cross(v(2) - v(0));
    let order: [usize; 4] = if facing.dot(normal) >= 0.0 { [0, 1, 2, 3] } else { [3, 2, 1, 0] };
    let base = out.positions.len() as u32;
    for i in order {
        out.positions.push(p[i]);
        out.normals.push(normal.to_array());
        out.uvs.push(uv[i]);
        out.colors.push([1.0, sky, tile_value, shade]);
    }
    out.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn face_shade(f: Facing) -> f32 {
    match f {
        Facing::Up => 1.0,
        Facing::Down => 0.5,
        Facing::West | Facing::East => 0.65,
        Facing::North | Facing::South => 0.8,
    }
}

fn encode_tile(tile_id: u8, emissive: bool) -> f32 {
    (tile_id + if emissive { tile::EMISSIVE } else { 0 }) as f32
}

/// A box in chunk-local coordinates with one tile per face (indexed by `Facing::index`).
struct BoxSpec {
    min: Vec3,
    max: Vec3,
    tiles: [u8; 6],
    emissive: bool,
    /// Multiplies the face shade (dust brightens with its power).
    brightness: f32,
    sky: f32,
}

fn emit_box(out: &mut MeshBuffers, b: &BoxSpec) {
    let (lo, hi) = (b.min, b.max);
    for f in Facing::ALL {
        let tv = encode_tile(b.tiles[f.index() as usize], b.emissive);
        let shade = face_shade(f) * b.brightness;
        let n = f.offset().as_vec3();
        // Corner positions of the face and the texture coordinates (block-local, tile cropped).
        let (p, uv): ([[f32; 3]; 4], [[f32; 2]; 4]) = match f {
            Facing::Up | Facing::Down => {
                let y = if f == Facing::Up { hi.y } else { lo.y };
                (
                    [[lo.x, y, lo.z], [lo.x, y, hi.z], [hi.x, y, hi.z], [hi.x, y, lo.z]],
                    [[lo.x, lo.z], [lo.x, hi.z], [hi.x, hi.z], [hi.x, lo.z]]
                        .map(|[u, v]| [u.rem_euclid(1.0), v.rem_euclid(1.0)]),
                )
            }
            Facing::West | Facing::East => {
                let x = if f == Facing::East { hi.x } else { lo.x };
                (
                    [[x, lo.y, lo.z], [x, hi.y, lo.z], [x, hi.y, hi.z], [x, lo.y, hi.z]],
                    [[lo.z, lo.y], [lo.z, hi.y], [hi.z, hi.y], [hi.z, lo.y]].map(frac2),
                )
            }
            Facing::North | Facing::South => {
                let z = if f == Facing::South { hi.z } else { lo.z };
                (
                    [[lo.x, lo.y, z], [hi.x, lo.y, z], [hi.x, hi.y, z], [lo.x, hi.y, z]],
                    [[lo.x, lo.y], [hi.x, lo.y], [hi.x, hi.y], [lo.x, hi.y]].map(frac2),
                )
            }
        };
        quad(out, p, n, uv, tv, shade, b.sky);
    }
}

/// Block-local coordinates `0..=1` keep their value, others wrap into the cell. The tile repeats
/// with `fract` in the shader, so only the position inside the block matters.
fn frac2([u, v]: [f32; 2]) -> [f32; 2] {
    let w = |a: f32| if (0.0..=1.0).contains(&a) { a } else { a.rem_euclid(1.0) };
    [w(u), w(v)]
}

pub(super) fn mesh_special(vol: &PaddedVolume, opaque: &mut MeshBuffers, water: &mut MeshBuffers) {
    for y in 0..CHUNK_H as i32 {
        for z in 0..CHUNK_W as i32 {
            for x in 0..CHUNK_W as i32 {
                let b = vol.get(x, y, z);
                if b.is_fluid() {
                    mesh_fluid(vol, [x, y, z], b, if b.is_water() { &mut *water } else { &mut *opaque });
                } else if b.is_model() {
                    mesh_model(vol, [x, y, z], b, opaque);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Fluids
// ---------------------------------------------------------------------------------------------

fn fluid_state(b: BlockId) -> Option<(bool, u8)> {
    match b.kind() {
        Kind::Water(s) => Some((false, s)),
        Kind::Lava(s) => Some((true, s)),
        _ => None,
    }
}

fn same_fluid(a: BlockId, lava: bool) -> bool {
    if lava { a.is_lava() } else { a.is_water() }
}

/// Height of the surface at the corner `(cx, cz)` (lattice coordinates) of the cell at height
/// `y`: the weighted average of the four cells that meet there; sources count more so a pool
/// stays level, and a fluid cell with fluid above it is always full.
fn corner_height(vol: &PaddedVolume, cx: i32, y: i32, cz: i32, lava: bool) -> f32 {
    let (mut sum, mut weight) = (0.0, 0.0);
    for dz in -1..=0 {
        for dx in -1..=0 {
            let (x, z) = (cx + dx, cz + dz);
            if same_fluid(vol.get(x, y + 1, z), lava) {
                return 1.0;
            }
            let b = vol.get(x, y, z);
            if let Some((_, state)) = fluid_state(b).filter(|(l, _)| *l == lava) {
                let h = fluid_height(state);
                if state == 0 {
                    sum += h * 10.0;
                    weight += 10.0;
                } else {
                    sum += h;
                    weight += 1.0;
                }
            } else if !b.is_solid() {
                weight += 1.0;
            }
        }
    }
    if weight == 0.0 { 0.0 } else { sum / weight }
}

fn mesh_fluid(vol: &PaddedVolume, [x, y, z]: [i32; 3], b: BlockId, out: &mut MeshBuffers) {
    let Some((lava, _)) = fluid_state(b) else { return };
    let tile_id = b.tile_facing(Facing::Up);
    let tv = encode_tile(tile_id, lava);
    let sky_at = |p: [i32; 3]| vol.sky_light_f32(p);
    let (fx, fy, fz) = (x as f32, y as f32, z as f32);
    // Corner heights, indexed [dz][dx].
    let h = |dx: i32, dz: i32| corner_height(vol, x + dx, y, z + dz, lava);
    let (h00, h10, h01, h11) = (h(0, 0), h(1, 0), h(0, 1), h(1, 1));

    let above = vol.get(x, y + 1, z);
    if !same_fluid(above, lava) && !above.is_opaque() {
        let p =
            [[fx, fy + h00, fz], [fx, fy + h01, fz + 1.0], [fx + 1.0, fy + h11, fz + 1.0], [fx + 1.0, fy + h10, fz]];
        let uv = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
        let sky = sky_at([x, y + 1, z]);
        quad(out, p, Vec3::Y, uv, tv, 1.0, sky);
        if !lava {
            // Seen from below the surface closes the water volume.
            quad(out, p, Vec3::NEG_Y, uv, tv, 0.7, sky);
        }
    }
    let below = vol.get(x, y - 1, z);
    if !same_fluid(below, lava) && !below.is_opaque() && y > 0 {
        let p = [[fx, fy, fz], [fx, fy, fz + 1.0], [fx + 1.0, fy, fz + 1.0], [fx + 1.0, fy, fz]];
        quad(out, p, Vec3::NEG_Y, [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]], tv, 0.5, sky_at([x, y - 1, z]));
    }
    for f in Facing::HORIZONTAL {
        let off = f.offset();
        let n = vol.get(x + off.x, y, z + off.z);
        if n.is_opaque() || same_fluid(n, lava) {
            continue;
        }
        // The two top corners on that side, ordered along the face's horizontal axis.
        let (ha, hb, p0, p1) = match f {
            Facing::North => (h00, h10, [fx, fz], [fx + 1.0, fz]),
            Facing::South => (h01, h11, [fx, fz + 1.0], [fx + 1.0, fz + 1.0]),
            Facing::West => (h00, h01, [fx, fz], [fx, fz + 1.0]),
            _ => (h10, h11, [fx + 1.0, fz], [fx + 1.0, fz + 1.0]),
        };
        if ha <= 0.001 && hb <= 0.001 {
            continue;
        }
        let p = [[p0[0], fy, p0[1]], [p1[0], fy, p1[1]], [p1[0], fy + hb, p1[1]], [p0[0], fy + ha, p0[1]]];
        let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, hb], [0.0, ha]];
        quad(out, p, off.as_vec3(), uv, tv, face_shade(f), sky_at([x + off.x, y, z + off.z]));
    }
}

// ---------------------------------------------------------------------------------------------
// Boxes: redstone parts and piston heads
// ---------------------------------------------------------------------------------------------

fn mesh_model(vol: &PaddedVolume, [x, y, z]: [i32; 3], b: BlockId, out: &mut MeshBuffers) {
    let origin = Vec3::new(x as f32, y as f32, z as f32);
    let sky = vol.sky_light_f32([x, y, z]);
    let emissive = b.is_emissive();
    let mut push = |min: Vec3, max: Vec3, tiles: [u8; 6], emissive: bool, brightness: f32| {
        emit_box(out, &BoxSpec { min, max, tiles, emissive, brightness, sky });
    };
    let all = |t: u8| [t; 6];

    match b.kind() {
        Kind::Dust { power } => {
            let t = b.tile_facing(Facing::Up);
            let brightness = if power > 0 { 0.45 + 0.55 * power as f32 / 15.0 } else { 0.8 };
            let arms = dust_arms(|p: IVec3| vol.get(p.x, p.y, p.z), IVec3::new(x, y, z));
            let any = arms.iter().any(Option::is_some);
            let eff = effective_arms(&arms);
            let (c0, c1) = (6.0 * PX, 10.0 * PX);
            let flat = |lo: (f32, f32), hi: (f32, f32)| (Vec3::new(lo.0, 0.0, lo.1), Vec3::new(hi.0, PX * 0.5, hi.1));
            let mut boxes = vec![flat((c0, c0), (c1, c1))];
            for (i, (f, arm)) in Facing::HORIZONTAL.into_iter().zip(arms).enumerate() {
                // A lone dust is drawn as a small cross; connected arms (and the straight
                // continuation of a line end) reach the block edge.
                let (far, near) = match (arm, eff[i], any) {
                    (Some(_), _, _) | (None, true, _) => (1.0, 0.0),
                    (None, false, false) => (13.0 * PX, 3.0 * PX),
                    (None, false, true) => continue,
                };
                boxes.push(match f {
                    Facing::North => flat((c0, near), (c1, c0)),
                    Facing::South => flat((c0, c1), (c1, far)),
                    Facing::West => flat((near, c0), (c0, c1)),
                    _ => flat((c1, c0), (far, c1)),
                });
                if arm == Some(1) {
                    // The wire continues up the side of the block it climbs.
                    let thin = PX * 0.5;
                    boxes.push(match f {
                        Facing::North => (Vec3::new(c0, 0.0, 0.0), Vec3::new(c1, 1.0, thin)),
                        Facing::South => (Vec3::new(c0, 0.0, 1.0 - thin), Vec3::new(c1, 1.0, 1.0)),
                        Facing::West => (Vec3::new(0.0, 0.0, c0), Vec3::new(thin, 1.0, c1)),
                        _ => (Vec3::new(1.0 - thin, 0.0, c0), Vec3::new(1.0, 1.0, c1)),
                    });
                }
            }
            for (lo, hi) in boxes {
                push(origin + lo, origin + hi, all(t), emissive, brightness);
            }
        }
        Kind::Torch { attach, lit } => {
            let t = b.tile_facing(Facing::Up);
            if attach == Facing::Down {
                push(
                    origin + Vec3::new(7.0 * PX, 0.0, 7.0 * PX),
                    origin + Vec3::new(9.0 * PX, 10.0 * PX, 9.0 * PX),
                    all(t),
                    lit,
                    1.0,
                );
            } else {
                let (lo, hi) =
                    framed_box(origin, attach, (3.0 * PX, 13.0 * PX), (7.0 * PX, 9.0 * PX), (6.0 * PX, 8.0 * PX));
                push(lo, hi, all(t), lit, 1.0);
            }
        }
        Kind::Lever { attach, on } => {
            let base = framed_box(origin, attach, (4.0 * PX, 12.0 * PX), (5.0 * PX, 11.0 * PX), (0.0, 3.0 * PX));
            push(base.0, base.1, all(tile::COBBLE), false, 1.0);
            // The handle leans to one side when off and to the other when on.
            let (a0, a1) = if on { (9.0 * PX, 11.0 * PX) } else { (5.0 * PX, 7.0 * PX) };
            let stick = framed_box(origin, attach, (a0, a1), (7.0 * PX, 9.0 * PX), (3.0 * PX, 11.0 * PX));
            push(stick.0, stick.1, all(tile::LEVER), on, 1.0);
        }
        Kind::Button { attach, pressed } => {
            let depth = if pressed { PX } else { 2.0 * PX };
            let (lo, hi) = framed_box(origin, attach, (5.0 * PX, 11.0 * PX), (6.0 * PX, 10.0 * PX), (0.0, depth));
            push(lo, hi, all(tile::STONE), false, 1.0);
        }
        Kind::Plate { pressed } => {
            let top = if pressed { 0.5 * PX } else { PX };
            push(
                origin + Vec3::new(PX, 0.0, PX),
                origin + Vec3::new(15.0 * PX, top, 15.0 * PX),
                all(tile::STONE),
                false,
                1.0,
            );
        }
        Kind::PistonHead { facing, .. } => {
            // `support` points from the head towards the piston body behind it.
            let support = facing.opposite();
            let mut tiles = [tile::PLANKS; 6];
            for f in Facing::ALL {
                tiles[f.index() as usize] = b.tile_facing(f);
            }
            let plate = framed_box(origin, support, (0.0, 1.0), (0.0, 1.0), (12.0 * PX, 1.0));
            push(plate.0, plate.1, tiles, false, 1.0);
            let rod = framed_box(origin, support, (6.0 * PX, 10.0 * PX), (6.0 * PX, 10.0 * PX), (0.0, 12.0 * PX));
            push(rod.0, rod.1, all(tile::PLANKS), false, 1.0);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::block::FALLING;
    use crate::world::chunk::Chunk;

    fn volume(chunk: &Chunk) -> PaddedVolume {
        let empty = Chunk::empty();
        PaddedVolume::build(|dx, dz| if (dx, dz) == (0, 0) { Some(chunk) } else { Some(&empty) })
    }

    fn mesh(chunk: &Chunk) -> (MeshBuffers, MeshBuffers) {
        let (mut o, mut w) = (MeshBuffers::default(), MeshBuffers::default());
        mesh_special(&volume(chunk), &mut o, &mut w);
        (o, w)
    }

    #[test]
    fn heights_follow_the_flow_level() {
        assert!(fluid_height(0) < 1.0 && fluid_height(0) > fluid_height(1));
        assert!(fluid_height(1) > fluid_height(7));
        assert_eq!(fluid_height(FALLING), 1.0);
    }

    #[test]
    fn an_isolated_source_slopes_down_at_its_corners_but_a_pool_is_level() {
        let mut c = Chunk::empty();
        c.set(5, 50, 5, BlockId::WATER);
        let (o, w) = mesh(&c);
        assert!(o.is_empty());
        // Top (+ underside of the surface), bottom and four sides.
        assert_eq!(w.quad_count(), 7);

        let mut pool = Chunk::empty();
        for x in 4..=6 {
            for z in 4..=6 {
                pool.set(x, 50, z, BlockId::WATER);
            }
        }
        let (_, w) = mesh(&pool);
        // The corner in the middle of the pool is surrounded by sources: full source height.
        let centre = w.positions.iter().filter(|p| p[0] == 5.0 && p[2] == 5.0).map(|p| p[1]).fold(0.0f32, f32::max);
        assert!((centre - (50.0 + 8.0 / 9.0)).abs() < 1e-4, "pool surface height {centre}");
    }

    #[test]
    fn water_under_water_has_no_internal_faces() {
        let mut c = Chunk::empty();
        for y in 40..44 {
            c.set(5, y, 5, BlockId::WATER);
        }
        let (_, w) = mesh(&c);
        // 4 cells in a column: top + underside, bottom, 4 sides each.
        assert_eq!(w.quad_count(), 2 + 1 + 16);
    }

    #[test]
    fn lava_goes_to_the_opaque_buffer_and_is_emissive() {
        let mut c = Chunk::empty();
        c.set(5, 50, 5, BlockId::LAVA);
        let (o, w) = mesh(&c);
        assert!(w.is_empty());
        assert!(!o.is_empty());
        assert!(o.colors.iter().all(|col| col[2] >= tile::EMISSIVE as f32));
    }

    #[test]
    fn the_surface_drops_along_a_flow() {
        // A corridor: sources at x = 4, 5, then flowing water of growing distance.
        let mut c = Chunk::empty();
        for x in 3..=10 {
            c.set(x, 50, 4, BlockId::STONE);
            c.set(x, 50, 6, BlockId::STONE);
        }
        c.set(4, 50, 5, BlockId::WATER);
        c.set(5, 50, 5, BlockId::WATER);
        for (x, level) in [(6, 2), (7, 4), (8, 6)] {
            c.set(x, 50, 5, BlockId::water(level));
        }
        let (_, w) = mesh(&c);
        let height_at =
            |x: f32| w.positions.iter().filter(|p| p[0] == x && p[2] == 5.0).map(|p| p[1]).fold(0.0f32, f32::max);
        let (h6, h7, h8, h9) = (height_at(6.0), height_at(7.0), height_at(8.0), height_at(9.0));
        assert!(h6 > h7 && h7 > h8 && h8 > h9, "{h6} {h7} {h8} {h9}");
    }

    #[test]
    fn models_emit_boxes_and_lit_parts_are_emissive() {
        let mut c = Chunk::empty();
        c.set(2, 10, 2, BlockId::STONE);
        c.set(2, 11, 2, BlockId::torch(Facing::Down, true));
        c.set(4, 11, 4, BlockId::torch(Facing::Down, false));
        let (o, _) = mesh(&c);
        assert_eq!(o.quad_count(), 12);
        let lit = o.colors.iter().filter(|col| col[2] >= tile::EMISSIVE as f32).count();
        assert_eq!(lit, 24, "only the lit torch glows");
    }

    #[test]
    fn dust_draws_a_cross_alone_and_reaches_the_edge_when_connected() {
        let width_x = |c: &Chunk| {
            let (o, _) = mesh(c);
            let xs = o.positions.iter().map(|p| p[0]);
            xs.clone().fold(f32::MIN, f32::max) - xs.fold(f32::MAX, f32::min)
        };
        let mut alone = Chunk::empty();
        alone.set(8, 20, 8, BlockId::dust(0));
        let mut linked = alone.clone();
        linked.set(9, 20, 8, BlockId::dust(0));
        assert!((width_x(&alone) - 10.0 * PX).abs() < 1e-4);
        assert!(width_x(&linked) > 1.2, "two linked dust cells span both blocks");
    }

    #[test]
    fn framed_boxes_cover_the_expected_cells() {
        let (lo, hi) = framed_box(Vec3::ZERO, Facing::North, (0.0, 1.0), (0.0, 1.0), (0.0, 0.25));
        assert_eq!((lo, hi), (Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.25)));
        let (lo, hi) = framed_box(Vec3::ZERO, Facing::East, (0.0, 1.0), (0.0, 1.0), (0.0, 0.25));
        assert_eq!((lo, hi), (Vec3::new(0.75, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0)));
        let (lo, hi) = framed_box(Vec3::ZERO, Facing::Up, (0.0, 1.0), (0.0, 1.0), (0.0, 0.25));
        assert_eq!((lo, hi), (Vec3::new(0.0, 0.75, 0.0), Vec3::new(1.0, 1.0, 1.0)));
    }
}
