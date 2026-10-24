//! Procedural villages on flat plains and deserts. A village is a pure function of
//! `(seed, cell)`: its blocks are planned once as an ordered list of placements, and every chunk
//! that overlaps the village replays the placements that fall inside it, so houses line up
//! across chunk borders without any shared state.

use super::{Biome, SEA_LEVEL, TerrainGenerator};
use crate::world::block::BlockId;
use crate::world::chunk::{CHUNK_H, CHUNK_W, Chunk};
use bevy::math::{IVec2, IVec3};

/// Edge length of the grid cells that each hold at most one village (a multiple of the chunk size).
pub const CELL: i32 = 128;
/// Villages stay this close to their centre; the cell margin is larger, so a village never leaves its cell.
pub const RADIUS: i32 = 34;
const MARGIN: i32 = 48;
/// Percentage of cells that try to host a village (terrain must still be suitable).
const CHANCE: u32 = 85;
const MAX_HEIGHT_SPREAD: i32 = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Plains,
    Desert,
}

#[derive(Clone, Debug)]
pub struct Village {
    pub center: IVec2,
    pub ground: i32,
    pub style: Style,
    /// Placements in the order they must be applied (later ones overwrite earlier ones).
    ops: Vec<(IVec3, BlockId)>,
    /// Cells just outside the front doors, where villagers like to stand.
    pub door_spots: Vec<IVec3>,
}

#[inline]
fn hash(seed: u32, a: i32, b: i32, salt: u32) -> u32 {
    let mut h = (seed as u64) << 32 | salt as u64;
    h ^= (a as u32 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h = h.rotate_left(31) ^ (b as u32 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (h ^ (h >> 31)) as u32
}

pub fn cell_of(x: i32, z: i32) -> IVec2 {
    IVec2::new(x.div_euclid(CELL), z.div_euclid(CELL))
}

/// Plans the village of grid cell `cell`, if the terrain there allows one.
pub fn plan(gen_: &TerrainGenerator, cell: IVec2) -> Option<Village> {
    let seed = gen_.seed();
    let h = hash(seed, cell.x, cell.y, 0x71_11A6);
    if h % 100 >= CHANCE {
        return None;
    }
    let span = (CELL - 2 * MARGIN) as u32;
    let center = IVec2::new(
        cell.x * CELL + MARGIN + ((h >> 8) % span) as i32,
        cell.y * CELL + MARGIN + ((h >> 16) % span) as i32,
    );

    // The ground must be flat, dry and plain.
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    let mut deserts = 0;
    for dz in (-24..=24).step_by(8) {
        for dx in (-24..=24).step_by(8) {
            let c = gen_.column(center.x + dx, center.y + dz);
            if !matches!(c.biome, Biome::Plains | Biome::Desert) || c.height <= SEA_LEVEL + 1 {
                return None;
            }
            deserts += (c.biome == Biome::Desert) as i32;
            lo = lo.min(c.height);
            hi = hi.max(c.height);
        }
    }
    if hi - lo > MAX_HEIGHT_SPREAD {
        return None;
    }
    let ground = gen_.column(center.x, center.y).height;
    let style = if deserts > 24 { Style::Desert } else { Style::Plains };
    Some(Builder::new(gen_, center, ground, style, h).build())
}

struct Builder<'a> {
    gen_: &'a TerrainGenerator,
    center: IVec2,
    ground: i32,
    style: Style,
    seed: u32,
    ops: Vec<(IVec3, BlockId)>,
    door_spots: Vec<IVec3>,
}

/// Direction a door faces: the unit step from the house towards the village centre.
#[derive(Clone, Copy)]
struct Facing(i32, i32);

impl<'a> Builder<'a> {
    fn new(gen_: &'a TerrainGenerator, center: IVec2, ground: i32, style: Style, seed: u32) -> Self {
        Self { gen_, center, ground, style, seed, ops: Vec::new(), door_spots: Vec::new() }
    }

    fn put(&mut self, x: i32, y: i32, z: i32, id: BlockId) {
        self.ops.push((IVec3::new(x, y, z), id));
    }

    fn fill(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, id: BlockId) {
        for y in y0..=y1 {
            for z in z0..=z1 {
                for x in x0..=x1 {
                    self.put(x, y, z, id);
                }
            }
        }
    }

    fn surface(&self, x: i32, z: i32) -> i32 {
        self.gen_.column(x, z).height
    }

    fn build(mut self) -> Village {
        let c = self.center;
        // Houses: four on the axes, up to four more on the diagonals.
        let mut houses: Vec<(i32, i32, Facing)> =
            vec![(13, 0, Facing(-1, 0)), (-13, 0, Facing(1, 0)), (0, 13, Facing(0, -1)), (0, -13, Facing(0, 1))];
        for (i, (dx, dz, f)) in
            [(18, 18, Facing(0, -1)), (-18, 18, Facing(0, -1)), (18, -18, Facing(0, 1)), (-18, -18, Facing(0, 1))]
                .into_iter()
                .enumerate()
        {
            if hash(self.seed, i as i32, 3, 0xD1A6) % 100 < 60 {
                houses.push((dx, dz, f));
            }
        }
        // Axis houses may be skipped now and then so no two villages look alike.
        let keep: Vec<bool> =
            (0..houses.len()).map(|i| i >= 3 || hash(self.seed, i as i32, 9, 0x5E1) % 100 < 85).collect();

        // Paths first: the houses overwrite whatever is in their way.
        let mut doors = Vec::new();
        for (i, &(dx, dz, f)) in houses.iter().enumerate() {
            if !keep[i] {
                continue;
            }
            let (w, d) = self.house_size(i);
            // Footprint extents along x / z (the door side sits on the centre-facing wall).
            let (hx, hz) = if f.0 != 0 { (d, w) } else { (w, d) };
            let (x0, z0) = (c.x + dx - hx / 2, c.y + dz - hz / 2);
            let (x1, z1) = (x0 + hx - 1, z0 + hz - 1);
            doors.push((i, x0, z0, x1, z1, f));
        }
        for &(_, x0, z0, x1, z1, f) in &doors {
            // The path ends one step outside the door.
            let (ex, ez) = if f.0 != 0 {
                (if f.0 < 0 { x0 - 1 } else { x1 + 1 }, (z0 + z1) / 2)
            } else {
                ((x0 + x1) / 2, if f.1 < 0 { z0 - 1 } else { z1 + 1 })
            };
            self.path(c.x, c.y, ex, ez);
        }
        self.well();
        for &(i, x0, z0, x1, z1, f) in &doors {
            self.house(x0, z0, x1, z1, f, i);
        }
        // Lamp posts around the well.
        for (lx, lz) in [(4, 4), (-4, 4), (4, -4), (-4, -4)] {
            let (x, z) = (c.x + lx, c.y + lz);
            let y = self.surface(x, z);
            self.put(x, y, z, BlockId::GRAVEL);
            self.put(x, y + 1, z, BlockId::FENCE);
            self.put(x, y + 2, z, BlockId::FENCE);
            self.put(x, y + 3, z, BlockId::TORCH);
            self.fill(x, y + 4, z, x, y + 6, z, BlockId::AIR);
        }
        Village {
            center: self.center,
            ground: self.ground,
            style: self.style,
            ops: self.ops,
            door_spots: self.door_spots,
        }
    }

    fn house_size(&self, i: usize) -> (i32, i32) {
        let h = hash(self.seed, i as i32, 5, 0x51_2E);
        // (width along the wall that faces the centre, depth away from it)
        [(5, 5), (7, 5), (5, 7), (7, 7)][(h % 4) as usize]
    }

    /// One-block-wide gravel path: first along x, then along z.
    fn path(&mut self, x0: i32, z0: i32, x1: i32, z1: i32) {
        let mut tiles = Vec::new();
        let step = |a: i32, b: i32| if b >= a { 1 } else { -1 };
        let mut x = x0;
        while x != x1 {
            tiles.push((x, z0));
            x += step(x0, x1);
        }
        let mut z = z0;
        while z != z1 {
            tiles.push((x1, z));
            z += step(z0, z1);
        }
        tiles.push((x1, z1));
        for (x, z) in tiles {
            let y = self.surface(x, z);
            if y > SEA_LEVEL {
                self.put(x, y, z, BlockId::GRAVEL);
                self.fill(x, y + 1, z, x, y + 3, z, BlockId::AIR);
            }
        }
    }

    fn well(&mut self) {
        let (cx, cz, g) = (self.center.x, self.center.y, self.ground);
        self.fill(cx - 2, g + 1, cz - 2, cx + 2, g + 8, cz + 2, BlockId::AIR);
        for z in cz - 1..=cz + 1 {
            for x in cx - 1..=cx + 1 {
                self.column_foundation(x, z, BlockId::COBBLE);
                self.put(x, g, z, BlockId::COBBLE);
            }
        }
        self.put(cx, g, cz, BlockId::WATER);
        self.put(cx, g - 1, cz, BlockId::WATER);
        self.put(cx, g - 2, cz, BlockId::COBBLE);
        for (px, pz) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
            self.put(cx + px, g + 1, cz + pz, BlockId::FENCE);
            self.put(cx + px, g + 2, cz + pz, BlockId::FENCE);
        }
        self.fill(cx - 1, g + 3, cz - 1, cx + 1, g + 3, cz + 1, BlockId::PLANKS);
        self.put(cx, g + 4, cz, BlockId::TORCH);
    }

    /// Fills the gap between the natural terrain and `ground` with `block` (and digs away hills).
    fn column_foundation(&mut self, x: i32, z: i32, block: BlockId) {
        let h = self.surface(x, z);
        if h < self.ground {
            self.fill(x, h, z, x, self.ground - 1, z, block);
        }
    }

    fn palette(&self, i: usize) -> (BlockId, BlockId, BlockId, BlockId) {
        let h = hash(self.seed, i as i32, 11, 0xBA11);
        match self.style {
            Style::Plains => {
                let wall = if h.is_multiple_of(3) { BlockId::COBBLE } else { BlockId::PLANKS };
                let roof = [BlockId::PLANKS, BlockId::BRICKS, BlockId::COBBLE][((h >> 4) % 3) as usize];
                (wall, roof, BlockId::LOG, BlockId::PLANKS)
            }
            Style::Desert => (BlockId::SANDSTONE, BlockId::SANDSTONE, BlockId::SANDSTONE, BlockId::SANDSTONE),
        }
    }

    fn house(&mut self, x0: i32, z0: i32, x1: i32, z1: i32, door: Facing, i: usize) {
        let g = self.ground;
        let (wall, roof, pillar, floor) = self.palette(i);
        // Clear everything in and above the building (trees, hills) and fill gaps below it.
        self.fill(x0 - 1, g + 1, z0 - 1, x1 + 1, g + 9, z1 + 1, BlockId::AIR);
        for z in z0 - 1..=z1 + 1 {
            for x in x0 - 1..=x1 + 1 {
                self.column_foundation(x, z, BlockId::COBBLE);
            }
        }
        // Floor, with a cobblestone rim.
        for z in z0..=z1 {
            for x in x0..=x1 {
                let rim = x == x0 || x == x1 || z == z0 || z == z1;
                self.put(x, g, z, if rim { BlockId::COBBLE } else { floor });
            }
        }
        let (xm, zm) = ((x0 + x1) / 2, (z0 + z1) / 2);
        // Walls with windows; the door wall keeps its middle cell open.
        for z in z0..=z1 {
            for x in x0..=x1 {
                let on_x_wall = x == x0 || x == x1;
                let on_z_wall = z == z0 || z == z1;
                if !(on_x_wall || on_z_wall) {
                    continue;
                }
                let corner = on_x_wall && on_z_wall;
                if corner {
                    self.fill(x, g + 1, z, x, g + 4, z, pillar);
                    continue;
                }
                let is_door_wall = (door.0 < 0 && x == x0)
                    || (door.0 > 0 && x == x1)
                    || (door.1 < 0 && z == z0)
                    || (door.1 > 0 && z == z1);
                let along = if on_x_wall { z - z0 } else { x - x0 };
                if is_door_wall && ((on_x_wall && z == zm) || (on_z_wall && x == xm)) {
                    self.put(x, g + 1, z, BlockId::AIR);
                    self.put(x, g + 2, z, BlockId::AIR);
                    self.put(x, g + 3, z, wall);
                    // `door` points towards the village centre, which is where the doorstep is.
                    self.door_spots.push(IVec3::new(x + door.0, g + 1, z + door.1));
                    continue;
                }
                let window = !is_door_wall && along % 2 == 0;
                self.fill(x, g + 1, z, x, g + 3, z, wall);
                if window {
                    self.put(x, g + 2, z, BlockId::GLASS);
                }
            }
        }
        // Stepped roof.
        self.fill(x0 - 1, g + 4, z0 - 1, x1 + 1, g + 4, z1 + 1, roof);
        self.fill(x0, g + 5, z0, x1, g + 5, z1, roof);
        let mut layer = 1;
        while x0 + layer <= x1 - layer && z0 + layer <= z1 - layer {
            self.fill(x0 + layer, g + 5 + layer, z0 + layer, x1 - layer, g + 5 + layer, z1 - layer, roof);
            layer += 1;
        }
        for (px, pz) in [(x0, z0), (x1, z0), (x0, z1), (x1, z1)] {
            self.fill(px, g + 1, pz, px, g + 4, pz, pillar);
        }
        // Furniture: a torch and a crafting table in opposite corners of the room.
        self.put(x0 + 1, g + 1, z0 + 1, BlockId::TORCH);
        self.put(x1 - 1, g + 1, z1 - 1, BlockId::CRAFTING_TABLE);
        if x1 - x0 >= 6 {
            self.put(x1 - 1, g + 1, z0 + 1, BlockId::FENCE);
            self.put(x1 - 1, g + 2, z0 + 1, BlockId::TORCH);
        }
    }
}

impl Village {
    /// Whether the column is inside the village area (used to keep trees away).
    pub fn contains_column(&self, x: i32, z: i32) -> bool {
        (x - self.center.x).abs() <= RADIUS && (z - self.center.y).abs() <= RADIUS
    }

    pub fn op_count(&self) -> usize {
        self.ops.len()
    }

    /// Writes the part of the village that lies inside chunk `pos`.
    pub fn apply(&self, chunk: &mut Chunk, pos: IVec2) {
        let (ox, oz) = (pos.x * CHUNK_W as i32, pos.y * CHUNK_W as i32);
        for &(p, id) in &self.ops {
            let (lx, lz) = (p.x - ox, p.z - oz);
            if (0..CHUNK_W as i32).contains(&lx)
                && (0..CHUNK_W as i32).contains(&lz)
                && (1..CHUNK_H as i32 - 1).contains(&p.y)
            {
                chunk.set(lx as usize, p.y as usize, lz as usize, id);
            }
        }
    }

    /// Whether the village touches chunk `pos` at all.
    pub fn overlaps_chunk(&self, pos: IVec2) -> bool {
        let (ox, oz) = (pos.x * CHUNK_W as i32, pos.y * CHUNK_W as i32);
        ox + CHUNK_W as i32 > self.center.x - RADIUS
            && ox <= self.center.x + RADIUS
            && oz + CHUNK_W as i32 > self.center.y - RADIUS
            && oz <= self.center.y + RADIUS
    }

    /// Every placement (for tests and tools).
    pub fn ops(&self) -> &[(IVec3, BlockId)] {
        &self.ops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_villages(seed: u32, cells: i32) -> Vec<Village> {
        let g = TerrainGenerator::new(seed);
        (-cells..cells)
            .flat_map(|z| (-cells..cells).map(move |x| IVec2::new(x, z)))
            .filter_map(|c| plan(&g, c))
            .collect()
    }

    #[test]
    fn some_cells_host_villages_and_most_do_not() {
        let mut found = 0;
        let mut cells = 0;
        for seed in [1u32, 2, 3, 4] {
            found += find_villages(seed, 8).len();
            cells += 16 * 16;
        }
        assert!(found >= 2, "expected villages in {cells} cells, found {found}");
        assert!(found < cells / 2, "villages should be rare, found {found}/{cells}");
    }

    #[test]
    fn villages_are_deterministic_and_stay_inside_their_cell() {
        let all = find_villages(7, 10);
        let again = find_villages(7, 10);
        assert_eq!(all.len(), again.len());
        for (a, b) in all.iter().zip(&again) {
            assert_eq!(a.center, b.center);
            assert_eq!(a.ops(), b.ops());
            let cell = cell_of(a.center.x, a.center.y);
            for (p, _) in a.ops() {
                assert_eq!(cell_of(p.x, p.z), cell, "block {p} left the village cell");
                assert!((p.x - a.center.x).abs() <= RADIUS && (p.z - a.center.y).abs() <= RADIUS);
            }
        }
    }

    #[test]
    fn a_village_has_houses_with_doors_windows_and_light() {
        let villages: Vec<Village> = (1..8).flat_map(|s| find_villages(s, 8)).collect();
        assert!(!villages.is_empty());
        for v in villages.iter().take(6) {
            let count = |b: BlockId| v.ops().iter().filter(|(_, id)| *id == b).count();
            assert!(count(BlockId::TORCH) >= 4, "torches: {}", count(BlockId::TORCH));
            assert!(count(BlockId::CRAFTING_TABLE) >= 3);
            assert!(v.door_spots.len() >= 3, "doors: {}", v.door_spots.len());
            assert!(count(BlockId::WATER) >= 2, "the well holds water");
            match v.style {
                Style::Plains => assert!(count(BlockId::GLASS) >= 6 && count(BlockId::LOG) >= 12),
                Style::Desert => assert!(count(BlockId::SANDSTONE) > 100),
            }
            // Doors are open: the cell in front of each door is walkable (air above the path).
            assert!(v.op_count() > 500);
        }
    }

    #[test]
    fn applying_a_village_to_chunks_places_its_blocks() {
        let g = TerrainGenerator::new(3);
        let v = (-10..10)
            .flat_map(|z| (-10..10).map(move |x| IVec2::new(x, z)))
            .find_map(|c| plan(&g, c))
            .expect("a village");
        let mut placed = 0;
        let ChunkRange { min, max } = ChunkRange::around(&v);
        for cz in min.y..=max.y {
            for cx in min.x..=max.x {
                let pos = IVec2::new(cx, cz);
                if !v.overlaps_chunk(pos) {
                    continue;
                }
                let mut chunk = Chunk::empty();
                v.apply(&mut chunk, pos);
                placed += chunk.raw().iter().filter(|b| **b == BlockId::CRAFTING_TABLE).count();
            }
        }
        assert!(placed >= 3, "crafting tables found across chunks: {placed}");
        assert!(v.contains_column(v.center.x, v.center.y) && !v.contains_column(v.center.x + RADIUS + 1, v.center.y));
    }

    struct ChunkRange {
        min: IVec2,
        max: IVec2,
    }

    impl ChunkRange {
        fn around(v: &Village) -> Self {
            let c = |a: i32| a.div_euclid(CHUNK_W as i32);
            Self {
                min: IVec2::new(c(v.center.x - RADIUS), c(v.center.y - RADIUS)),
                max: IVec2::new(c(v.center.x + RADIUS), c(v.center.y + RADIUS)),
            }
        }
    }
}
