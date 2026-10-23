//! CPU meshing: greedy quad merging with hidden-face culling, per-vertex
//! ambient occlusion and a cheap column-based sky light.
//!
//! The mesher is a pure function of a [`PaddedVolume`] (a chunk plus a one-block
//! border copied from its neighbours), so it can run on any worker thread.
//!
//! Vertex attributes:
//! * `position`/`normal`/`uv` — uv is in *block units* (0..w, 0..h) so the shader
//!   can tile the atlas tile across merged quads with `fract`.
//! * `color` — `r` = ambient occlusion, `g` = sky light, `b` = atlas tile index,
//!   `a` = directional face shading.

use crate::world::block::{BlockId, Face};
use crate::world::chunk::{CHUNK_H, CHUNK_W, Chunk};

pub const PW: usize = CHUNK_W + 2;
const DIMS: [usize; 3] = [CHUNK_W, CHUNK_H, CHUNK_W];

/// Brightness for AO levels 0 (fully occluded) to 3 (open).
const AO_CURVE: [f32; 4] = [0.42, 0.62, 0.82, 1.0];
const MIN_SKY: i32 = 2;
const MAX_SKY: i32 = 15;

/// Chunk blocks with a one-block horizontal border from the 8 neighbours.
pub struct PaddedVolume {
    blocks: Vec<BlockId>,
    /// Highest light-blocking y per padded column (-1 if none).
    top: Vec<i16>,
}

impl PaddedVolume {
    /// `neighbour(dx, dz)` returns the chunk at the given offset (`-1..=1`),
    /// `(0, 0)` being the chunk to mesh. Missing neighbours act as solid stone so
    /// no faces are generated towards unloaded terrain.
    pub fn build<'a>(neighbour: impl Fn(i32, i32) -> Option<&'a Chunk>) -> Self {
        let mut blocks = vec![BlockId::STONE; PW * CHUNK_H * PW];
        for dz in -1..=1 {
            for dx in -1..=1 {
                let Some(chunk) = neighbour(dx, dz) else { continue };
                let xs = Self::span(dx);
                let zs = Self::span(dz);
                for y in 0..CHUNK_H {
                    for pz in zs.0..=zs.1 {
                        let lz = Self::local(pz, dz);
                        for px in xs.0..=xs.1 {
                            let lx = Self::local(px, dx);
                            blocks[(y * PW + pz) * PW + px] = chunk.get(lx, y, lz);
                        }
                    }
                }
            }
        }
        let mut top = vec![-1i16; PW * PW];
        for pz in 0..PW {
            for px in 0..PW {
                for y in (0..CHUNK_H).rev() {
                    if blocks[(y * PW + pz) * PW + px].is_opaque() {
                        top[pz * PW + px] = y as i16;
                        break;
                    }
                }
            }
        }
        Self { blocks, top }
    }

    /// Padded index range covered by the neighbour at offset `d`.
    fn span(d: i32) -> (usize, usize) {
        match d {
            -1 => (0, 0),
            0 => (1, CHUNK_W),
            _ => (CHUNK_W + 1, CHUNK_W + 1),
        }
    }

    fn local(p: usize, d: i32) -> usize {
        match d {
            -1 => CHUNK_W - 1,
            0 => p - 1,
            _ => 0,
        }
    }

    /// Block at chunk-local coordinates; x/z may be -1 or 16.
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> BlockId {
        if y < 0 {
            return BlockId::STONE;
        }
        if y >= CHUNK_H as i32 {
            return BlockId::AIR;
        }
        self.blocks[(y as usize * PW + (z + 1) as usize) * PW + (x + 1) as usize]
    }

    #[inline]
    fn opaque(&self, p: [i32; 3]) -> bool {
        self.get(p[0], p[1], p[2]).is_opaque()
    }

    fn sky_light(&self, p: [i32; 3]) -> u8 {
        let top = self.top[(p[2] + 1) as usize * PW + (p[0] + 1) as usize] as i32;
        if p[1] > top { MAX_SKY as u8 } else { (MAX_SKY - 2 * (top - p[1])).max(MIN_SKY) as u8 }
    }
}

#[derive(Default)]
pub struct MeshBuffers {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl MeshBuffers {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn quad_count(&self) -> usize {
        self.indices.len() / 6
    }
}

#[derive(Default)]
pub struct ChunkMeshData {
    pub opaque: MeshBuffers,
    pub water: MeshBuffers,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    Opaque,
    Water,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FaceKey {
    block: BlockId,
    ao: [u8; 4],
    sky: u8,
}

impl FaceKey {
    /// Cells can only be merged along an axis if the AO does not change along it;
    /// otherwise the gradient of one cell would be stretched over several blocks.
    /// Corner order is (u0,v0), (u1,v0), (u1,v1), (u0,v1).
    fn merge_along_u(&self) -> bool {
        self.ao[0] == self.ao[1] && self.ao[3] == self.ao[2]
    }

    fn merge_along_v(&self) -> bool {
        self.ao[0] == self.ao[3] && self.ao[1] == self.ao[2]
    }
}

pub fn mesh_chunk(vol: &PaddedVolume) -> ChunkMeshData {
    let mut out = ChunkMeshData::default();
    greedy(vol, Layer::Opaque, &mut out.opaque);
    greedy(vol, Layer::Water, &mut out.water);
    out
}

fn face_visible(layer: Layer, block: BlockId, neighbour: BlockId) -> bool {
    match layer {
        Layer::Opaque => block.is_opaque() && !neighbour.is_opaque(),
        Layer::Water => block.is_water() && neighbour.is_air(),
    }
}

fn greedy(vol: &PaddedVolume, layer: Layer, out: &mut MeshBuffers) {
    let mut mask: Vec<Option<FaceKey>> = Vec::new();
    for axis in 0..3 {
        let (ua, va) = ((axis + 1) % 3, (axis + 2) % 3);
        let (du, dv) = (DIMS[ua], DIMS[va]);
        mask.resize(du * dv, None);
        for positive in [true, false] {
            let dir: i32 = if positive { 1 } else { -1 };
            for d in 0..DIMS[axis] as i32 {
                // Build the visibility mask for this slice.
                let mut any = false;
                for v in 0..dv {
                    for u in 0..du {
                        let mut p = [0i32; 3];
                        p[axis] = d;
                        p[ua] = u as i32;
                        p[va] = v as i32;
                        let mut n = p;
                        n[axis] += dir;
                        let (b, nb) = (vol.get(p[0], p[1], p[2]), vol.get(n[0], n[1], n[2]));
                        mask[v * du + u] = if face_visible(layer, b, nb) {
                            any = true;
                            Some(FaceKey {
                                block: b,
                                ao: if layer == Layer::Water { [3; 4] } else { corner_ao(vol, n, ua, va) },
                                sky: vol.sky_light(n),
                            })
                        } else {
                            None
                        };
                    }
                }
                if !any {
                    continue;
                }
                // Greedy rectangle merge.
                for v in 0..dv {
                    let mut u = 0;
                    while u < du {
                        let Some(key) = mask[v * du + u] else {
                            u += 1;
                            continue;
                        };
                        let (mut w, mut h) = (1, 1);
                        if key.merge_along_u() {
                            while u + w < du && mask[v * du + u + w] == Some(key) {
                                w += 1;
                            }
                        }
                        if key.merge_along_v() {
                            'grow: while v + h < dv {
                                for k in 0..w {
                                    if mask[(v + h) * du + u + k] != Some(key) {
                                        break 'grow;
                                    }
                                }
                                h += 1;
                            }
                        }
                        for hv in 0..h {
                            for wu in 0..w {
                                mask[(v + hv) * du + u + wu] = None;
                            }
                        }
                        emit_quad(out, axis, positive, d, (u, v), (w, h), &key);
                        u += w;
                    }
                }
            }
        }
    }
}

/// Classic 0fps vertex AO. `n` is the voxel in front of the face; corner order is
/// (u0,v0), (u1,v0), (u1,v1), (u0,v1).
fn corner_ao(vol: &PaddedVolume, n: [i32; 3], ua: usize, va: usize) -> [u8; 4] {
    let sample = |du: i32, dv: i32| {
        let mut p = n;
        p[ua] += du;
        p[va] += dv;
        vol.opaque(p) as u8
    };
    let corner = |cu: i32, cv: i32| {
        let su = if cu == 0 { -1 } else { 1 };
        let sv = if cv == 0 { -1 } else { 1 };
        let (s1, s2, c) = (sample(su, 0), sample(0, sv), sample(su, sv));
        if s1 == 1 && s2 == 1 { 0 } else { 3 - (s1 + s2 + c) }
    };
    [corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)]
}

fn face_for(axis: usize, positive: bool) -> Face {
    match (axis, positive) {
        (1, true) => Face::Top,
        (1, false) => Face::Bottom,
        _ => Face::Side,
    }
}

fn face_shade(axis: usize, positive: bool) -> f32 {
    match (axis, positive) {
        (1, true) => 1.0,
        (1, false) => 0.5,
        (0, _) => 0.65,
        _ => 0.8,
    }
}

fn emit_quad(
    out: &mut MeshBuffers,
    axis: usize,
    positive: bool,
    d: i32,
    (u, v): (usize, usize),
    (w, h): (usize, usize),
    key: &FaceKey,
) {
    let (ua, va) = ((axis + 1) % 3, (axis + 2) % 3);
    let plane = (d + positive as i32) as f32;
    let corner = |cu: usize, cv: usize| {
        let mut p = [0f32; 3];
        p[axis] = plane;
        p[ua] = (u + cu * w) as f32;
        p[va] = (v + cv * h) as f32;
        p
    };
    // (u,v,axis) is a right-handed frame, so u0v0 -> u1v0 -> u1v1 -> u0v1 is CCW
    // when seen from +axis. The negative face needs the opposite winding.
    let mut verts = [(corner(0, 0), 0usize), (corner(1, 0), 1), (corner(1, 1), 2), (corner(0, 1), 3)];
    if !positive {
        verts.reverse();
    }

    // Texture coordinates: horizontal tex axis / vertical (world Y) tex axis.
    let tex = |p: [f32; 3]| -> [f32; 2] {
        let local = |a: usize| {
            p[a] - if a == ua {
                u as f32
            } else if a == va {
                v as f32
            } else {
                0.0
            }
        };
        match axis {
            // X and Y faces: v is horizontal, u is vertical (or arbitrary for Y).
            0 | 1 => [local(va), local(ua)],
            _ => [local(ua), local(va)],
        }
    };

    let tile = key.block.tile(face_for(axis, positive)) as f32;
    let shade = face_shade(axis, positive);
    let sky = key.sky as f32 / MAX_SKY as f32;
    let mut normal = [0f32; 3];
    normal[axis] = if positive { 1.0 } else { -1.0 };

    let base = out.positions.len() as u32;
    let mut ao_order = [0u8; 4];
    for (i, (p, ci)) in verts.iter().enumerate() {
        out.positions.push(*p);
        out.normals.push(normal);
        out.uvs.push(tex(*p));
        let ao = key.ao[*ci];
        ao_order[i] = ao;
        out.colors.push([AO_CURVE[ao as usize], sky, tile, shade]);
    }
    // Pick the diagonal that keeps the AO gradient isotropic.
    if ao_order[0] + ao_order[2] >= ao_order[1] + ao_order[3] {
        out.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    } else {
        out.indices.extend_from_slice(&[base + 1, base + 2, base + 3, base + 1, base + 3, base]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generation::TerrainGenerator;
    use bevy::math::IVec2;

    fn volume_of(chunk: &Chunk, fill_neighbours: bool) -> PaddedVolume {
        let empty = Chunk::empty();
        PaddedVolume::build(|dx, dz| {
            if (dx, dz) == (0, 0) {
                Some(chunk)
            } else if fill_neighbours {
                Some(&empty)
            } else {
                None
            }
        })
    }

    fn area(m: &MeshBuffers) -> f32 {
        m.indices
            .chunks(3)
            .map(|t| {
                let p = |i: u32| bevy::math::Vec3::from(m.positions[i as usize]);
                (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() / 2.0
            })
            .sum()
    }

    #[test]
    fn single_block_has_six_quads() {
        let mut c = Chunk::empty();
        c.set(5, 100, 5, BlockId::STONE);
        let m = mesh_chunk(&volume_of(&c, true));
        assert_eq!(m.opaque.quad_count(), 6);
        assert_eq!(m.opaque.positions.len(), 24);
        assert!(m.water.is_empty());
    }

    #[test]
    fn flat_slab_merges_into_minimal_quads() {
        let mut c = Chunk::empty();
        for y in 1..64 {
            for z in 0..16 {
                for x in 0..16 {
                    c.set(x, y, z, BlockId::STONE);
                }
            }
        }
        // With air neighbours: top + bottom + 4 sides, one quad each.
        let m = mesh_chunk(&volume_of(&c, true));
        assert_eq!(m.opaque.quad_count(), 6);
        assert_eq!(m.opaque.positions.len(), 24);
        // Without neighbours every border face is culled: only top and bottom faces remain.
        let culled = mesh_chunk(&volume_of(&c, false)).opaque;
        assert!(
            culled.normals.iter().all(|n| n[0] == 0.0 && n[2] == 0.0),
            "no horizontal faces towards missing chunks"
        );
    }

    #[test]
    fn winding_matches_normals() {
        let mut c = Chunk::empty();
        c.set(3, 3, 3, BlockId::STONE);
        c.set(4, 3, 3, BlockId::DIRT);
        c.set(3, 4, 3, BlockId::GRASS);
        let m = mesh_chunk(&volume_of(&c, true)).opaque;
        for t in m.indices.chunks(3) {
            let p = |i: u32| bevy::math::Vec3::from(m.positions[i as usize]);
            let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).normalize();
            let expect = bevy::math::Vec3::from(m.normals[t[0] as usize]);
            assert!((n - expect).length() < 1e-4, "triangle {t:?} faces {n}, normal says {expect}");
        }
    }

    #[test]
    fn mesh_area_equals_number_of_visible_faces() {
        // Random solid/air soup, no neighbours: compare against brute force counting.
        let mut c = Chunk::empty();
        let mut s = 99u32;
        for y in 60..70 {
            for z in 0..16 {
                for x in 0..16 {
                    s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                    let id = match (s >> 24) % 4 {
                        0 => BlockId::STONE,
                        1 => BlockId::DIRT,
                        2 => BlockId::WATER,
                        _ => BlockId::AIR,
                    };
                    c.set(x, y, z, id);
                }
            }
        }
        let vol = volume_of(&c, true);
        let m = mesh_chunk(&vol);
        let (mut opaque_faces, mut water_faces) = (0, 0);
        for y in 0..256i32 {
            for z in 0..16 {
                for x in 0..16 {
                    let b = vol.get(x, y, z);
                    for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
                        let n = vol.get(x + dx, y + dy, z + dz);
                        opaque_faces += face_visible(Layer::Opaque, b, n) as u32;
                        water_faces += face_visible(Layer::Water, b, n) as u32;
                    }
                }
            }
        }
        assert!((area(&m.opaque) - opaque_faces as f32).abs() < 0.01);
        assert!((area(&m.water) - water_faces as f32).abs() < 0.01);
    }

    #[test]
    fn ambient_occlusion_darkens_inner_corner() {
        let mut c = Chunk::empty();
        // A floor with a wall: the floor vertex touching the wall is darker.
        for z in 0..16 {
            for x in 0..16 {
                c.set(x, 10, z, BlockId::STONE);
            }
        }
        c.set(8, 11, 8, BlockId::STONE);
        let m = mesh_chunk(&volume_of(&c, true)).opaque;
        let min_ao = m.colors.iter().map(|c| c[0]).fold(1.0, f32::min);
        assert!(min_ao < 1.0, "expected darkened vertices next to the pillar");
        // Open floor far from the pillar stays fully lit.
        assert!(m.colors.iter().any(|c| c[0] == 1.0));
    }

    #[test]
    fn caves_get_less_sky_light_than_the_surface() {
        let mut c = Chunk::empty();
        for y in 0..=40 {
            for z in 0..16 {
                for x in 0..16 {
                    c.set(x, y, z, BlockId::STONE);
                }
            }
        }
        for x in 4..8 {
            for y in 20..24 {
                for z in 4..8 {
                    c.set(x, y, z, BlockId::AIR);
                }
            }
        }
        let m = mesh_chunk(&volume_of(&c, true)).opaque;
        let skies: Vec<f32> = m.colors.iter().map(|c| c[1]).collect();
        assert!(skies.iter().any(|&s| s > 0.99));
        assert!(skies.iter().any(|&s| s < 0.5));
    }

    #[test]
    fn greedy_meshing_beats_one_quad_per_face_on_real_terrain() {
        let g = TerrainGenerator::new(77);
        let chunks: Vec<Chunk> =
            (-1..=1).flat_map(|z| (-1..=1).map(move |x| IVec2::new(x, z))).map(|p| g.generate(p)).collect();
        let vol = PaddedVolume::build(|dx, dz| Some(&chunks[((dz + 1) * 3 + dx + 1) as usize]));
        let m = mesh_chunk(&vol);
        let naive: f32 = area(&m.opaque) + area(&m.water);
        let quads = (m.opaque.quad_count() + m.water.quad_count()) as f32;
        assert!(quads > 0.0 && naive / quads > 1.25, "avg quad area {}", naive / quads);
        // Indices must always reference valid vertices.
        assert!(m.opaque.indices.iter().all(|&i| (i as usize) < m.opaque.positions.len()));
    }
}
