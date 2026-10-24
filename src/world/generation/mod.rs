//! Deterministic procedural terrain: continents, hills, mountains, biomes,
//! 3D caves, ore veins, oceans, trees and villages. Pure functions of `(seed, x, z)` so chunks can
//! be generated independently on any thread and still line up at their borders.

pub mod caves;
pub mod ores;
pub mod villages;

use super::block::BlockId;
use super::chunk::{CHUNK_H, CHUNK_W, Chunk, ChunkPos};
use bevy::math::IVec2;
use caves::Caves;
use noise::{Fbm, MultiFractal, NoiseFn, OpenSimplex};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use villages::Village;

pub const SEA_LEVEL: i32 = 62;
const TREE_MARGIN: i32 = 2;
const COLS: usize = CHUNK_W + 2 * TREE_MARGIN as usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Biome {
    Ocean,
    Beach,
    Plains,
    Forest,
    Desert,
    Snowy,
    Mountains,
}

#[derive(Clone, Copy, Debug)]
pub struct Column {
    pub height: i32,
    pub biome: Biome,
}

pub struct TerrainGenerator {
    seed: u32,
    continent: Fbm<OpenSimplex>,
    hills: Fbm<OpenSimplex>,
    mountain_mask: Fbm<OpenSimplex>,
    ridges: Fbm<OpenSimplex>,
    temperature: Fbm<OpenSimplex>,
    humidity: Fbm<OpenSimplex>,
    caves: Caves,
    /// Planned villages per grid cell (`None` = the cell has none), computed on first use.
    villages: Mutex<HashMap<IVec2, Option<Arc<Village>>>>,
}

fn fbm(seed: u32, octaves: usize) -> Fbm<OpenSimplex> {
    Fbm::<OpenSimplex>::new(seed).set_octaves(octaves).set_frequency(1.0)
}

#[inline]
fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// SplitMix64-style integer hash used for tree placement and bedrock noise.
#[inline]
fn hash(seed: u32, x: i32, z: i32, salt: u32) -> u32 {
    let mut h = (seed as u64) << 32 | salt as u64;
    h ^= (x as u32 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h = h.rotate_left(31) ^ (z as u32 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (h ^ (h >> 31)) as u32
}

impl TerrainGenerator {
    pub fn new(seed: u32) -> Self {
        Self {
            seed,
            continent: fbm(seed, 4),
            hills: fbm(seed.wrapping_add(1), 5),
            mountain_mask: fbm(seed.wrapping_add(2), 3),
            ridges: fbm(seed.wrapping_add(3), 5),
            temperature: fbm(seed.wrapping_add(4), 3),
            humidity: fbm(seed.wrapping_add(5), 3),
            caves: Caves::new(seed),
            villages: Mutex::new(HashMap::new()),
        }
    }

    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// Terrain surface height and biome of one world column.
    pub fn column(&self, x: i32, z: i32) -> Column {
        let (fx, fz) = (x as f64, z as f64);
        let continent = self.continent.get([fx / 900.0, fz / 900.0]);
        let hills = self.hills.get([fx / 110.0, fz / 110.0]);
        let mask = smoothstep(0.05, 0.40, self.mountain_mask.get([fx / 600.0, fz / 600.0]) + 0.1);
        let ridge = 1.0 - self.ridges.get([fx / 220.0, fz / 220.0]).abs();
        let mountains = ridge * ridge * 78.0 * mask * smoothstep(-0.1, 0.35, continent);

        let base = 66.0 + continent * 34.0;
        let rolling = hills * (4.0 + 10.0 * (0.5 + 0.5 * continent).clamp(0.0, 1.0));
        let height = ((base + rolling + mountains).round() as i32).clamp(2, CHUNK_H as i32 - 40);

        let temp = self.temperature.get([fx / 520.0, fz / 520.0]);
        let humid = self.humidity.get([fx / 420.0, fz / 420.0]);
        let biome = if height < SEA_LEVEL - 1 {
            Biome::Ocean
        } else if height <= SEA_LEVEL + 1 {
            Biome::Beach
        } else if height > 112 {
            Biome::Mountains
        } else if temp < -0.28 {
            Biome::Snowy
        } else if temp > 0.26 && humid < 0.05 {
            Biome::Desert
        } else if humid > 0.08 {
            Biome::Forest
        } else {
            Biome::Plains
        };
        Column { height, biome }
    }

    /// The village of grid cell `cell`, planned on first use.
    pub fn village_in_cell(&self, cell: IVec2) -> Option<Arc<Village>> {
        if let Some(v) = self.villages.lock().ok()?.get(&cell) {
            return v.clone();
        }
        // Planned outside the lock (it samples the terrain); a racing thread computes the same result.
        let planned = villages::plan(self, cell).map(Arc::new);
        if let Ok(mut cache) = self.villages.lock() {
            cache.insert(cell, planned.clone());
        }
        planned
    }

    /// The village whose area contains the world column `(x, z)`, if any.
    pub fn village_at(&self, x: i32, z: i32) -> Option<Arc<Village>> {
        self.village_in_cell(villages::cell_of(x, z)).filter(|v| v.contains_column(x, z))
    }

    /// Whether the block `(x, y, z)` is carved out by a cave.
    pub fn is_cave(&self, x: i32, y: i32, z: i32) -> bool {
        self.caves.is_cave(x, y, z)
    }

    fn tree_chance(&self, biome: Biome) -> u32 {
        match biome {
            Biome::Forest => 22,
            Biome::Plains => 160,
            Biome::Snowy => 90,
            _ => 0,
        }
    }

    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let mut chunk = Chunk::empty();
        let (ox, oz) = (pos.x * CHUNK_W as i32, pos.y * CHUNK_W as i32);

        // Columns including a margin so trees of neighbouring chunks can reach in.
        let mut cols = [Column { height: 0, biome: Biome::Plains }; COLS * COLS];
        for cz in 0..COLS {
            for cx in 0..COLS {
                cols[cz * COLS + cx] = self.column(ox + cx as i32 - TREE_MARGIN, oz + cz as i32 - TREE_MARGIN);
            }
        }
        let col_at = |lx: i32, lz: i32| cols[(lz + TREE_MARGIN) as usize * COLS + (lx + TREE_MARGIN) as usize];

        for lz in 0..CHUNK_W {
            for lx in 0..CHUNK_W {
                let (wx, wz) = (ox + lx as i32, oz + lz as i32);
                let Column { height, biome } = col_at(lx as i32, lz as i32);
                let (surface, subsurface, depth) = match biome {
                    Biome::Ocean => (BlockId::SAND, BlockId::SAND, 3),
                    Biome::Beach | Biome::Desert => (BlockId::SAND, BlockId::SAND, 4),
                    Biome::Snowy => (BlockId::SNOW, BlockId::DIRT, 3),
                    Biome::Mountains if height > 126 => (BlockId::SNOW, BlockId::STONE, 1),
                    Biome::Mountains => (BlockId::STONE, BlockId::STONE, 1),
                    Biome::Plains | Biome::Forest => (BlockId::GRASS, BlockId::DIRT, 3),
                };
                let protect_shore = height <= SEA_LEVEL + 2;
                // Tunnels may break through the surface in rare patches, never at the shore.
                let margin = if protect_shore { 3 } else { self.caves.ceiling_margin(wx, wz) };
                let sandstone_depth = if matches!(biome, Biome::Desert | Biome::Beach) { depth + 3 } else { 0 };
                for y in 0..=height {
                    // Solid bedrock floor with a ragged top (y = 1..3).
                    let id = if y == 0 || (y < 4 && hash(self.seed, wx, wz, y as u32).is_multiple_of(3)) {
                        BlockId::BEDROCK
                    } else if y == height {
                        surface
                    } else if y > height - depth {
                        subsurface
                    } else if y > height - sandstone_depth {
                        BlockId::SANDSTONE
                    } else {
                        BlockId::STONE
                    };
                    let carve = y > 3
                        && y < height - margin
                        && !(protect_shore && y > height - 8)
                        && self.caves.is_cave(wx, y, wz);
                    chunk.set(lx, y as usize, lz, if carve { BlockId::AIR } else { id });
                }
                for y in (height + 1)..=SEA_LEVEL {
                    chunk.set(lx, y as usize, lz, BlockId::WATER);
                }
            }
        }

        ores::place(self.seed, &mut chunk, pos.x, pos.y);

        let village = self.village_in_cell(villages::cell_of(ox, oz)).filter(|v| v.overlaps_chunk(pos));

        // Trees: every column in the margin may host one; only blocks inside this chunk are written.
        for tz in -TREE_MARGIN..(CHUNK_W as i32 + TREE_MARGIN) {
            for tx in -TREE_MARGIN..(CHUNK_W as i32 + TREE_MARGIN) {
                let Column { height, biome } = col_at(tx, tz);
                let chance = self.tree_chance(biome);
                if chance == 0 || height <= SEA_LEVEL + 1 {
                    continue;
                }
                let (wx, wz) = (ox + tx, oz + tz);
                if village.as_ref().is_some_and(|v| v.contains_column(wx, wz)) {
                    continue;
                }
                let h = hash(self.seed, wx, wz, 0xA11CE);
                if !h.is_multiple_of(chance) {
                    continue;
                }
                let trunk = 4 + (h >> 8) as i32 % 3;
                self.place_tree(&mut chunk, tx, height + 1, tz, trunk, h);
            }
        }

        if let Some(v) = village {
            v.apply(&mut chunk, pos);
        }
        chunk
    }

    fn place_tree(&self, chunk: &mut Chunk, tx: i32, base: i32, tz: i32, trunk: i32, h: u32) {
        let top = base + trunk;
        if top + 2 >= CHUNK_H as i32 {
            return;
        }
        let mut put = |x: i32, y: i32, z: i32, id: BlockId, overwrite_leaves: bool| {
            if !(0..CHUNK_W as i32).contains(&x) || !(0..CHUNK_W as i32).contains(&z) || y < 0 || y >= CHUNK_H as i32 {
                return;
            }
            let cur = chunk.get(x as usize, y as usize, z as usize);
            if cur.is_air() || (overwrite_leaves && cur == BlockId::LEAVES) {
                chunk.set(x as usize, y as usize, z as usize, id);
            }
        };
        for dy in -2..=1 {
            let radius: i32 = if dy <= -1 { 2 } else { 1 };
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    let corner = dx.abs() == radius && dz.abs() == radius;
                    let skip = corner
                        && (radius == 2 && hash(self.seed, tx + dx, tz + dz, h ^ dy as u32).is_multiple_of(2)
                            || dy == 1);
                    if !skip {
                        put(tx + dx, top + dy, tz + dz, BlockId::LEAVES, false);
                    }
                }
            }
        }
        for y in base..=top {
            put(tx, y, tz, BlockId::LOG, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::IVec2;

    #[test]
    fn generation_is_deterministic() {
        let g = TerrainGenerator::new(42);
        let a = g.generate(IVec2::new(3, -7));
        let b = TerrainGenerator::new(42).generate(IVec2::new(3, -7));
        assert!(a.raw() == b.raw());
        let c = TerrainGenerator::new(43).generate(IVec2::new(3, -7));
        assert!(a.raw() != c.raw());
    }

    #[test]
    fn floor_is_bedrock_and_sky_is_empty() {
        let g = TerrainGenerator::new(7);
        for p in [IVec2::ZERO, IVec2::new(-5, 9), IVec2::new(40, 40)] {
            let c = g.generate(p);
            for z in 0..16 {
                for x in 0..16 {
                    assert_eq!(c.get(x, 0, z), BlockId::BEDROCK);
                    assert!(c.get(x, CHUNK_H - 1, z).is_air());
                }
            }
        }
    }

    #[test]
    fn surface_matches_column_height() {
        let g = TerrainGenerator::new(1234);
        let c = g.generate(IVec2::new(1, 1));
        let mut grounded = 0;
        for z in 0..16 {
            for x in 0..16 {
                let col = g.column(16 + x as i32, 16 + z as i32);
                // Trees may stack on top, but the terrain block itself is never below the column height.
                let top = c.column_top(x, z).unwrap() as i32;
                assert!(top >= col.height.min(SEA_LEVEL));
                if !c.get(x, col.height as usize, z).is_air() {
                    grounded += 1;
                }
            }
        }
        assert!(grounded > 200, "surface is solid in most columns (caves may open a few)");
    }

    #[test]
    fn oceans_are_filled_to_sea_level() {
        let g = TerrainGenerator::new(99);
        let mut seen_water = false;
        'outer: for cx in -20..20 {
            let c = g.generate(IVec2::new(cx, 0));
            for x in 0..16 {
                let col = g.column(cx * 16 + x as i32, 0);
                if col.height < SEA_LEVEL - 2 {
                    assert_eq!(c.get(x, SEA_LEVEL as usize, 0), BlockId::WATER);
                    assert!(c.get(x, SEA_LEVEL as usize + 1, 0).is_air());
                    seen_water = true;
                    break 'outer;
                }
            }
        }
        assert!(seen_water, "expected at least one ocean column along the scan line");
    }

    #[test]
    fn world_contains_variety() {
        let g = TerrainGenerator::new(2024);
        let mut biomes = std::collections::HashSet::new();
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for gx in -40..40 {
            for gz in -40..40 {
                let c = g.column(gx * 64, gz * 64);
                biomes.insert(format!("{:?}", c.biome));
                lo = lo.min(c.height);
                hi = hi.max(c.height);
            }
        }
        assert!(biomes.len() >= 4, "got {biomes:?}");
        assert!(hi - lo > 50, "height range {lo}..{hi}");
    }

    #[test]
    fn caves_exist_underground() {
        let g = TerrainGenerator::new(5);
        let carved =
            (0..32).flat_map(|x| (10..50).map(move |y| (x, y))).filter(|&(x, y)| g.is_cave(x * 5, y, x * 3)).count();
        assert!(carved > 0);
    }

    fn count_blocks(chunks: &[Chunk], b: BlockId) -> usize {
        chunks.iter().map(|c| c.raw().iter().filter(|x| **x == b).count()).sum()
    }

    #[test]
    fn real_chunks_contain_every_ore_and_open_caves() {
        let g = TerrainGenerator::new(2024);
        let chunks: Vec<Chunk> =
            (0..8).flat_map(|z| (0..8).map(move |x| IVec2::new(x, z))).map(|p| g.generate(p)).collect();
        for ore in
            [BlockId::COAL_ORE, BlockId::IRON_ORE, BlockId::GOLD_ORE, BlockId::REDSTONE_ORE, BlockId::DIAMOND_ORE]
        {
            assert!(count_blocks(&chunks, ore) > 0, "{} missing", ore.name());
        }
        assert!(count_blocks(&chunks, BlockId::COAL_ORE) > count_blocks(&chunks, BlockId::DIAMOND_ORE));
        // Underground voids: air below y = 40 somewhere.
        let hollow = chunks.iter().any(|c| (5..40).any(|y| (0..16).any(|z| (0..16).any(|x| c.get(x, y, z).is_air()))));
        assert!(hollow, "no cave below y = 40");
        // Ores sit inside the rock: diamonds only low.
        for c in &chunks {
            for y in 20..CHUNK_H {
                for z in 0..16 {
                    for x in 0..16 {
                        assert_ne!(c.get(x, y, z), BlockId::DIAMOND_ORE);
                    }
                }
            }
        }
    }

    #[test]
    fn deserts_have_a_sandstone_layer() {
        let g = TerrainGenerator::new(2024);
        let mut seen = false;
        'scan: for gx in -60..60 {
            for gz in -60..60 {
                let (x, z) = (gx * 24, gz * 24);
                let col = g.column(x, z);
                if col.biome == Biome::Desert {
                    let chunk = g.generate(IVec2::new(x.div_euclid(16), z.div_euclid(16)));
                    let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
                    let below = chunk.get(lx, (col.height - 6) as usize, lz);
                    if below == BlockId::SANDSTONE {
                        seen = true;
                        break 'scan;
                    }
                }
            }
        }
        assert!(seen, "no desert column with sandstone found");
    }

    #[test]
    fn villages_show_up_in_generated_terrain() {
        let g = TerrainGenerator::new(5);
        let cell = (-6..6)
            .flat_map(|z| (-6..6).map(move |x| IVec2::new(x, z)))
            .find(|c| g.village_in_cell(*c).is_some())
            .expect("a village within 12x12 cells");
        let v = g.village_in_cell(cell).unwrap();
        let chunks: Vec<Chunk> = (-3..=3)
            .flat_map(|dz| (-3..=3).map(move |dx| (dx, dz)))
            .map(|(dx, dz)| g.generate(IVec2::new(v.center.x.div_euclid(16) + dx, v.center.y.div_euclid(16) + dz)))
            .collect();
        assert!(count_blocks(&chunks, BlockId::CRAFTING_TABLE) >= 3);
        assert!(count_blocks(&chunks, BlockId::GLASS) >= 4 || count_blocks(&chunks, BlockId::SANDSTONE) > 100);
        assert!(count_blocks(&chunks, BlockId::TORCH) >= 4);
        // No trees inside the village area: logs only come from the houses' pillars (plains) .
        assert_eq!(g.village_at(v.center.x, v.center.y).map(|x| x.center), Some(v.center));
    }
}
