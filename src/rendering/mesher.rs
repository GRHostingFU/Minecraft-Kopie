//! CPU meshing: greedy quad merging with hidden-face culling, per-vertex
//! ambient occlusion, a cheap column-based sky light and flood-filled block light.
//!
//! The mesher is a pure function of a [`PaddedVolume`] (a chunk plus a border copied from its
//! neighbours), so it can run on any worker thread.
//!
//! Vertex attributes:
//! * `position`/`normal`/`uv` — uv is in *block units* (0..w, 0..h) so the shader
//!   can tile the atlas tile across merged quads with `fract`.
//! * `color` — `r` = ambient occlusion, `g` = sky light, `b` = atlas tile index plus
//!   `256 * shade id` (both constant over a quad), `a` = block light of the vertex.

use crate::world::block::{BlockId, Face, Shape};
use crate::world::chunk::{CHUNK_H, CHUNK_W, Chunk};

/// Width of the border around the chunk. Block light spreads 15 blocks, so every emitter that can
/// brighten a block of the chunk lies inside the padded volume.
pub const BORDER: usize = 14;
pub const PW: usize = CHUNK_W + 2 * BORDER;
const DIMS: [usize; 3] = [CHUNK_W, CHUNK_H, CHUNK_W];

/// Brightness for AO levels 0 (fully occluded) to 3 (open).
const AO_CURVE: [f32; 4] = [0.42, 0.62, 0.82, 1.0];
const MIN_SKY: i32 = 2;
const MAX_SKY: i32 = 15;
const MAX_LIGHT: u8 = 15;

/// Chunk blocks with a [`BORDER`]-block horizontal border from the 8 neighbours.
pub struct PaddedVolume {
    blocks: Vec<BlockId>,
    /// Highest light-blocking y per padded column (-1 if none).
    top: Vec<i16>,
    /// Block light per cell; empty when the volume contains no emitter.
    light: Vec<u8>,
    has_models: bool,
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
                let raw = chunk.raw();
                for y in 0..CHUNK_H {
                    for pz in zs.0..zs.1 {
                        let lz = Self::local(pz, dz);
                        let src = Chunk::index(0, y, lz);
                        let dst = (y * PW + pz) * PW;
                        let lx0 = Self::local(xs.0, dx);
                        let n = xs.1 - xs.0;
                        blocks[dst + xs.0..dst + xs.1].copy_from_slice(&raw[src + lx0..src + lx0 + n]);
                    }
                }
            }
        }
        let mut top = vec![-1i16; PW * PW];
        let (mut has_models, mut emitters) = (false, Vec::new());
        for pz in 0..PW {
            for px in 0..PW {
                let mut found = false;
                for y in (0..CHUNK_H).rev() {
                    let b = blocks[(y * PW + pz) * PW + px];
                    if !found && b.is_opaque() {
                        top[pz * PW + px] = y as i16;
                        found = true;
                    }
                    if b.light() > 0 {
                        emitters.push(((y * PW + pz) * PW + px, b.light()));
                    }
                    if b.shape() != Shape::Cube
                        && (BORDER..BORDER + CHUNK_W).contains(&px)
                        && (BORDER..BORDER + CHUNK_W).contains(&pz)
                    {
                        has_models = true;
                    }
                }
            }
        }
        let light = if emitters.is_empty() { Vec::new() } else { Self::flood_light(&blocks, &emitters) };
        Self { blocks, top, light, has_models }
    }

    /// Breadth-first block light: every step away from an emitter loses one level, opaque blocks stop it.
    fn flood_light(blocks: &[BlockId], emitters: &[(usize, u8)]) -> Vec<u8> {
        let mut light = vec![0u8; blocks.len()];
        let mut queue: Vec<usize> = Vec::with_capacity(emitters.len() * 64);
        for &(i, level) in emitters {
            light[i] = light[i].max(level);
            queue.push(i);
        }
        let mut head = 0;
        while head < queue.len() {
            let i = queue[head];
            head += 1;
            let level = light[i];
            if level <= 1 {
                continue;
            }
            let (x, z, y) = (i % PW, (i / PW) % PW, i / (PW * PW));
            let mut visit = |j: usize| {
                if !blocks[j].is_opaque() && light[j] < level - 1 {
                    light[j] = level - 1;
                    queue.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < PW {
                visit(i + 1);
            }
            if z > 0 {
                visit(i - PW);
            }
            if z + 1 < PW {
                visit(i + PW);
            }
            if y > 0 {
                visit(i - PW * PW);
            }
            if y + 1 < CHUNK_H {
                visit(i + PW * PW);
            }
        }
        light
    }

    /// Padded index range `[from, to)` covered by the neighbour at offset `d`.
    fn span(d: i32) -> (usize, usize) {
        match d {
            -1 => (0, BORDER),
            0 => (BORDER, BORDER + CHUNK_W),
            _ => (BORDER + CHUNK_W, PW),
        }
    }

    /// Chunk-local coordinate (inside the neighbour chunk) of padded index `p`.
    fn local(p: usize, d: i32) -> usize {
        (p as i32 - BORDER as i32 - CHUNK_W as i32 * d) as usize
    }

    #[inline]
    fn index(x: i32, y: i32, z: i32) -> usize {
        (y as usize * PW + (z + BORDER as i32) as usize) * PW + (x + BORDER as i32) as usize
    }

    /// Block at chunk-local coordinates; x/z may reach `-BORDER..16 + BORDER`.
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> BlockId {
        if y < 0 {
            return BlockId::STONE;
        }
        if y >= CHUNK_H as i32 {
            return BlockId::AIR;
        }
        self.blocks[Self::index(x, y, z)]
    }

    #[inline]
    fn opaque(&self, p: [i32; 3]) -> bool {
        self.get(p[0], p[1], p[2]).is_opaque()
    }

    fn sky_light(&self, p: [i32; 3]) -> u8 {
        let top = self.top[(p[2] + BORDER as i32) as usize * PW + (p[0] + BORDER as i32) as usize] as i32;
        if p[1] > top { MAX_SKY as u8 } else { (MAX_SKY - 2 * (top - p[1])).max(MIN_SKY) as u8 }
    }

    /// Block light at a cell (0 for cells outside the volume).
    pub fn block_light(&self, p: [i32; 3]) -> u8 {
        if self.light.is_empty() || p[1] < 0 || p[1] >= CHUNK_H as i32 {
            return 0;
        }
        self.light[Self::index(p[0], p[1], p[2])]
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
    /// Smoothed block light at the four corners.
    light: [u8; 4],
}

impl FaceKey {
    /// Cells can only be merged along an axis if the AO and the light do not change along it;
    /// otherwise the gradient of one cell would be stretched over several blocks.
    /// Corner order is (u0,v0), (u1,v0), (u1,v1), (u0,v1).
    fn merge_along_u(&self) -> bool {
        self.ao[0] == self.ao[1]
            && self.ao[3] == self.ao[2]
            && self.light[0] == self.light[1]
            && self.light[3] == self.light[2]
    }

    fn merge_along_v(&self) -> bool {
        self.ao[0] == self.ao[3]
            && self.ao[1] == self.ao[2]
            && self.light[0] == self.light[3]
            && self.light[1] == self.light[2]
    }
}

pub fn mesh_chunk(vol: &PaddedVolume) -> ChunkMeshData {
    let mut out = ChunkMeshData::default();
    greedy(vol, Layer::Opaque, &mut out.opaque);
    greedy(vol, Layer::Water, &mut out.water);
    if vol.has_models {
        emit_models(vol, &mut out.opaque);
    }
    out
}

fn face_visible(layer: Layer, block: BlockId, neighbour: BlockId) -> bool {
    match layer {
        Layer::Opaque => block.is_opaque() && !neighbour.is_opaque(),
        // Water and glass: a face towards anything that does not hide it and is not the same fluid/glass.
        Layer::Water => block.is_translucent() && neighbour != block && !neighbour.is_opaque(),
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
                                light: corner_light(vol, n, ua, va),
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

/// Smooth block light: the average of the four cells around each corner of the face (cells that
/// are opaque do not count), rounded to a whole level. Same corner order as [`corner_ao`].
fn corner_light(vol: &PaddedVolume, n: [i32; 3], ua: usize, va: usize) -> [u8; 4] {
    if vol.light.is_empty() {
        return [0; 4];
    }
    let sample = |du: i32, dv: i32| {
        let mut p = n;
        p[ua] += du;
        p[va] += dv;
        (!vol.opaque(p)).then(|| vol.block_light(p) as u32)
    };
    let centre = vol.block_light(n) as u32;
    let corner = |cu: i32, cv: i32| {
        let su = if cu == 0 { -1 } else { 1 };
        let sv = if cv == 0 { -1 } else { 1 };
        let (mut sum, mut count) = (centre, 1);
        for l in [sample(su, 0), sample(0, sv), sample(su, sv)].into_iter().flatten() {
            sum += l;
            count += 1;
        }
        ((sum as f32 / count as f32).round() as u8).min(MAX_LIGHT)
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

/// Shade id decoded by the shader: 0 top, 1 bottom, 2 east/west, 3 north/south.
fn shade_id(axis: usize, positive: bool) -> f32 {
    match (axis, positive) {
        (1, true) => 0.0,
        (1, false) => 1.0,
        (0, _) => 2.0,
        _ => 3.0,
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

    // Tile index and shade id are constant over the quad, so they can share one vertex channel.
    let tile = key.block.tile(face_for(axis, positive)) as f32 + 256.0 * shade_id(axis, positive);
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
        out.colors.push([AO_CURVE[ao as usize], sky, tile, key.light[*ci] as f32 / MAX_LIGHT as f32]);
    }
    // Pick the diagonal that keeps the AO gradient isotropic.
    if ao_order[0] + ao_order[2] >= ao_order[1] + ao_order[3] {
        out.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    } else {
        out.indices.extend_from_slice(&[base + 1, base + 2, base + 3, base + 1, base + 3, base]);
    }
}

// -------------------------------------------------------------------------------------------------
// Non-cube blocks: torches and fences are drawn as small boxes.
// -------------------------------------------------------------------------------------------------

/// An axis-aligned box inside a block cell, in block units (`0..=1`).
type BoxF = ([f32; 3], [f32; 3]);

fn torch_boxes() -> Vec<BoxF> {
    vec![([7.0 / 16.0, 0.0, 7.0 / 16.0], [9.0 / 16.0, 10.0 / 16.0, 9.0 / 16.0])]
}

fn fence_boxes(vol: &PaddedVolume, p: [i32; 3]) -> Vec<BoxF> {
    let (lo, hi) = (6.0 / 16.0, 10.0 / 16.0);
    let mut boxes = vec![([lo, 0.0, lo], [hi, 1.0, hi])];
    let connects = |dx: i32, dz: i32| {
        let b = vol.get(p[0] + dx, p[1], p[2] + dz);
        b.shape() == Shape::Fence || b.is_opaque()
    };
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        if !connects(dx, dz) {
            continue;
        }
        for (y0, y1) in [(6.0 / 16.0, 9.0 / 16.0), (12.0 / 16.0, 15.0 / 16.0)] {
            let (a, b) = (7.0 / 16.0, 9.0 / 16.0);
            let bx = match dx {
                1 => (hi, 1.0),
                -1 => (0.0, lo),
                _ => (a, b),
            };
            let bz = match dz {
                1 => (hi, 1.0),
                -1 => (0.0, lo),
                _ => (a, b),
            };
            boxes.push(([bx.0, y0, bz.0], [bx.1, y1, bz.1]));
        }
    }
    boxes
}

fn emit_models(vol: &PaddedVolume, out: &mut MeshBuffers) {
    for y in 0..CHUNK_H as i32 {
        for z in 0..CHUNK_W as i32 {
            for x in 0..CHUNK_W as i32 {
                let block = vol.get(x, y, z);
                let boxes = match block.shape() {
                    Shape::Cube => continue,
                    Shape::Torch => torch_boxes(),
                    Shape::Fence => fence_boxes(vol, [x, y, z]),
                };
                let sky = vol.sky_light([x, y, z]) as f32 / MAX_SKY as f32;
                let light = vol.block_light([x, y, z]).max(block.light()) as f32 / MAX_LIGHT as f32;
                for (min, max) in boxes {
                    emit_box(out, [x as f32, y as f32, z as f32], min, max, block, sky, light);
                }
            }
        }
    }
}

/// Six faces of a box. UVs are the position inside the block cell, so the shader's `fract` shows
/// exactly that part of the tile.
fn emit_box(
    out: &mut MeshBuffers,
    origin: [f32; 3],
    min: [f32; 3],
    max: [f32; 3],
    block: BlockId,
    sky: f32,
    light: f32,
) {
    let (x0, y0, z0) = (min[0], min[1], min[2]);
    let (x1, y1, z1) = (max[0], max[1], max[2]);
    // (normal, shade id, face, corners bottom-left, bottom-right, top-right, top-left)
    let faces: [([f32; 3], f32, Face, [[f32; 3]; 4]); 6] = [
        ([1.0, 0.0, 0.0], 2.0, Face::Side, [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]]),
        ([-1.0, 0.0, 0.0], 2.0, Face::Side, [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]]),
        ([0.0, 0.0, 1.0], 3.0, Face::Side, [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]),
        ([0.0, 0.0, -1.0], 3.0, Face::Side, [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]]),
        ([0.0, 1.0, 0.0], 0.0, Face::Top, [[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]]),
        ([0.0, -1.0, 0.0], 1.0, Face::Bottom, [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]]),
    ];
    for (normal, shade, face, corners) in faces {
        let tile = block.tile(face) as f32 + 256.0 * shade;
        let base = out.positions.len() as u32;
        for c in corners {
            out.positions.push([origin[0] + c[0], origin[1] + c[1], origin[2] + c[2]]);
            out.normals.push(normal);
            // Horizontal texture axis: z for east/west faces, x otherwise; vertical: y (z for tops).
            let uv = if normal[0] != 0.0 {
                [c[2], c[1]]
            } else if normal[2] != 0.0 {
                [c[0], c[1]]
            } else {
                [c[0], c[2]]
            };
            out.uvs.push(uv);
            out.colors.push([1.0, sky, tile, light]);
        }
        out.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
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
        assert!(quads > 0.0 && naive / quads > 1.1, "avg quad area {}", naive / quads);
        // Indices must always reference valid vertices.
        assert!(m.opaque.indices.iter().all(|&i| (i as usize) < m.opaque.positions.len()));
    }

    #[test]
    fn torch_light_spreads_and_fades_with_distance() {
        let mut c = Chunk::empty();
        c.set(8, 11, 8, BlockId::TORCH);
        let vol = volume_of(&c, true);
        assert_eq!(vol.block_light([8, 11, 8]), 14);
        assert_eq!(vol.block_light([8, 11, 9]), 13);
        assert_eq!(vol.block_light([8, 11, 8 + 13]), 1, "light crosses the chunk border");
        assert_eq!(vol.block_light([8, 11, 8 + 14]), 0);
        // Without an emitter the volume carries no light at all.
        assert_eq!(volume_of(&Chunk::empty(), true).block_light([8, 11, 8]), 0);
    }

    #[test]
    fn opaque_walls_block_light() {
        let mut c = Chunk::empty();
        c.set(8, 11, 8, BlockId::TORCH);
        for y in 0..40 {
            for z in 0..16 {
                c.set(10, y, z, BlockId::STONE);
            }
        }
        let vol = volume_of(&c, true);
        assert_eq!(vol.block_light([9, 11, 8]), 13);
        assert_eq!(vol.block_light([11, 11, 8]), 0, "behind the wall it is dark");
    }

    #[test]
    fn lit_faces_get_block_light_in_the_vertex_colour() {
        let mut c = Chunk::empty();
        for z in 0..16 {
            for x in 0..16 {
                c.set(x, 10, z, BlockId::STONE);
            }
        }
        let dark = mesh_chunk(&volume_of(&c, true)).opaque;
        assert!(dark.colors.iter().all(|c| c[3] == 0.0));
        c.set(8, 11, 8, BlockId::GLOWSTONE);
        let lit = mesh_chunk(&volume_of(&c, true)).opaque;
        let brightest = lit.colors.iter().map(|c| c[3]).fold(0.0, f32::max);
        assert!(brightest > 0.9, "faces next to glowstone are fully lit, got {brightest}");
        // Tile index and shade id share one channel and decode back.
        for col in &lit.colors {
            let packed = col[2] as u32;
            assert!((packed % 256) < crate::world::block::tile::BLOCK_TILES as u32 && packed / 256 < 4);
        }
    }

    #[test]
    fn glass_is_drawn_in_the_blended_layer_and_does_not_hide_neighbours() {
        let mut c = Chunk::empty();
        c.set(5, 100, 5, BlockId::STONE);
        c.set(6, 100, 5, BlockId::GLASS);
        let m = mesh_chunk(&volume_of(&c, true));
        assert_eq!(m.opaque.quad_count(), 6, "stone keeps all six faces, glass hides nothing");
        assert_eq!(m.water.quad_count(), 5, "the glass face touching the stone is hidden by the stone");
        // Two glass blocks next to each other share no inner faces.
        c.set(7, 100, 5, BlockId::GLASS);
        let m = mesh_chunk(&volume_of(&c, true));
        assert_eq!(m.water.quad_count(), 5, "a merged strip: top, bottom, two long sides and the far end");
    }

    #[test]
    fn torches_and_fences_are_small_boxes_with_correct_winding() {
        let mut c = Chunk::empty();
        c.set(3, 20, 3, BlockId::TORCH);
        let m = mesh_chunk(&volume_of(&c, true));
        assert_eq!(m.opaque.quad_count(), 6);
        let (lo, hi) = m.opaque.positions.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
            (std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i])))
        });
        assert!((hi[0] - lo[0] - 0.125).abs() < 1e-4 && (hi[1] - lo[1] - 0.625).abs() < 1e-4, "{lo:?} {hi:?}");

        let mut c = Chunk::empty();
        c.set(5, 20, 5, BlockId::FENCE);
        assert_eq!(mesh_chunk(&volume_of(&c, true)).opaque.quad_count(), 6, "a lone post");
        c.set(6, 20, 5, BlockId::FENCE);
        c.set(5, 20, 4, BlockId::STONE);
        let m = mesh_chunk(&volume_of(&c, true)).opaque;
        for t in m.indices.chunks(3) {
            let p = |i: u32| bevy::math::Vec3::from(m.positions[i as usize]);
            let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).normalize();
            let expect = bevy::math::Vec3::from(m.normals[t[0] as usize]);
            assert!((n - expect).length() < 1e-4, "box triangle {t:?} faces {n}, normal says {expect}");
        }
        // Post + rails towards the fence at +x and the stone at -z, for both posts (the second
        // post connects back), plus the stone's own faces (5 visible, one touches nothing).
        assert!(m.quad_count() > 6 * 2 + 6 * 2 * 2, "rails were emitted: {}", m.quad_count());
    }

    #[test]
    fn border_copy_reads_the_right_neighbour_cells() {
        let mut centre = Chunk::empty();
        centre.set(0, 50, 0, BlockId::GOLD_BLOCK);
        let mut west = Chunk::empty();
        west.set(15, 50, 7, BlockId::DIAMOND_BLOCK);
        west.set(2, 50, 7, BlockId::COAL_BLOCK); // exactly BORDER cells away from the chunk
        let mut east = Chunk::empty();
        east.set(0, 60, 0, BlockId::IRON_BLOCK);
        east.set(13, 60, 0, BlockId::BRICKS);
        let vol = PaddedVolume::build(|dx, dz| match (dx, dz) {
            (0, 0) => Some(&centre),
            (-1, 0) => Some(&west),
            (1, 0) => Some(&east),
            _ => None,
        });
        assert_eq!(vol.get(0, 50, 0), BlockId::GOLD_BLOCK);
        assert_eq!(vol.get(-1, 50, 7), BlockId::DIAMOND_BLOCK);
        assert_eq!(vol.get(-(BORDER as i32), 50, 7), BlockId::COAL_BLOCK);
        assert_eq!(vol.get(16, 60, 0), BlockId::IRON_BLOCK);
        assert_eq!(vol.get(16 + 13, 60, 0), BlockId::BRICKS);
        // A missing neighbour is solid stone.
        assert_eq!(vol.get(3, 10, -1), BlockId::STONE);
    }
}
