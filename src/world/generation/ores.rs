//! Ore veins and rock variants. Veins are random walks that start at a hashed position inside a
//! chunk; every chunk replays the veins of its eight neighbours too, so a vein that crosses a
//! chunk border is identical on both sides without any shared state.

use crate::world::block::BlockId;
use crate::world::chunk::{CHUNK_W, Chunk};

pub struct OreSpec {
    pub block: BlockId,
    pub y_min: i32,
    pub y_max: i32,
    /// Veins started per chunk (the fraction is a chance for one more).
    pub veins: f32,
    /// Blocks per vein.
    pub size: u32,
}

/// Rock variants first, then ores (so an ore can sit inside granite only if it was placed first).
pub const SPECS: [OreSpec; 9] = [
    OreSpec { block: BlockId::GRAVEL, y_min: 5, y_max: 70, veins: 3.0, size: 20 },
    OreSpec { block: BlockId::GRANITE, y_min: 5, y_max: 90, veins: 2.5, size: 24 },
    OreSpec { block: BlockId::DIORITE, y_min: 5, y_max: 90, veins: 2.5, size: 24 },
    OreSpec { block: BlockId::ANDESITE, y_min: 5, y_max: 90, veins: 2.5, size: 24 },
    OreSpec { block: BlockId::COAL_ORE, y_min: 5, y_max: 128, veins: 13.0, size: 11 },
    OreSpec { block: BlockId::IRON_ORE, y_min: 3, y_max: 64, veins: 8.0, size: 8 },
    OreSpec { block: BlockId::GOLD_ORE, y_min: 3, y_max: 32, veins: 2.5, size: 7 },
    OreSpec { block: BlockId::REDSTONE_ORE, y_min: 3, y_max: 16, veins: 5.0, size: 7 },
    OreSpec { block: BlockId::DIAMOND_ORE, y_min: 3, y_max: 16, veins: 1.6, size: 5 },
];

#[inline]
fn hash(seed: u32, a: i32, b: i32, salt: u32) -> u32 {
    let mut h = (seed as u64) << 32 | salt as u64;
    h ^= (a as u32 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h = h.rotate_left(29) ^ (b as u32 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (h ^ (h >> 31)) as u32
}

/// Places all veins that touch chunk `(cx, cz)`.
pub fn place(seed: u32, chunk: &mut Chunk, cx: i32, cz: i32) {
    for (kind, spec) in SPECS.iter().enumerate() {
        for nz in cz - 1..=cz + 1 {
            for nx in cx - 1..=cx + 1 {
                let base = hash(seed, nx, nz, 0x0DE5 + kind as u32 * 977);
                let whole = spec.veins.floor() as u32;
                let extra = (spec.veins.fract() * 1000.0) as u32;
                let count = whole + (hash(seed, nx, nz, 0xFACE + kind as u32) % 1000 < extra) as u32;
                for i in 0..count {
                    place_vein(chunk, cx, cz, spec, hash(base, i as i32, 17, 0xBEEF), nx, nz);
                }
            }
        }
    }
}

/// One vein whose origin chunk is `(nx, nz)`; only blocks inside chunk `(cx, cz)` are written.
fn place_vein(chunk: &mut Chunk, cx: i32, cz: i32, spec: &OreSpec, h: u32, nx: i32, nz: i32) {
    let span = (spec.y_max - spec.y_min + 1) as u32;
    let mut pos = [
        nx * CHUNK_W as i32 + (h & 15) as i32,
        spec.y_min + ((h >> 8) % span) as i32,
        nz * CHUNK_W as i32 + ((h >> 4) & 15) as i32,
    ];
    let mut state = h | 1;
    for _ in 0..spec.size {
        let (lx, ly, lz) = (pos[0] - cx * CHUNK_W as i32, pos[1], pos[2] - cz * CHUNK_W as i32);
        if (0..CHUNK_W as i32).contains(&lx)
            && (0..CHUNK_W as i32).contains(&lz)
            && ly > 0
            && chunk.get(lx as usize, ly as usize, lz as usize) == BlockId::STONE
        {
            chunk.set(lx as usize, ly as usize, lz as usize, spec.block);
        }
        // xorshift step: pick an axis and a direction, with a slight preference to stay flat.
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let axis = match state % 7 {
            0..=2 => 0,
            3 => 1,
            4 => 1,
            _ => 2,
        };
        pos[axis] += if (state >> 8) & 1 == 0 { 1 } else { -1 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::CHUNK_H;

    fn stone_chunk() -> Chunk {
        let mut c = Chunk::empty();
        for y in 1..120 {
            for z in 0..16 {
                for x in 0..16 {
                    c.set(x, y, z, BlockId::STONE);
                }
            }
        }
        c
    }

    fn count(c: &Chunk, b: BlockId) -> usize {
        c.raw().iter().filter(|x| **x == b).count()
    }

    #[test]
    fn ores_appear_in_their_height_bands() {
        let mut totals = std::collections::HashMap::new();
        for cx in 0..6 {
            let mut c = stone_chunk();
            place(7, &mut c, cx, 2);
            for spec in &SPECS {
                for y in 0..CHUNK_H {
                    for z in 0..16 {
                        for x in 0..16 {
                            if c.get(x, y, z) == spec.block {
                                assert!((spec.y_min - spec.size as i32..=spec.y_max + spec.size as i32).contains(&(y as i32)), "{} at y={y}", spec.block.name());
                                *totals.entry(spec.block).or_insert(0usize) += 1;
                            }
                        }
                    }
                }
            }
        }
        for spec in &SPECS {
            assert!(totals.get(&spec.block).copied().unwrap_or(0) > 0, "{} never generated", spec.block.name());
        }
        // Coal is far more common than diamonds.
        assert!(totals[&BlockId::COAL_ORE] > totals[&BlockId::DIAMOND_ORE] * 4);
        assert!(totals[&BlockId::IRON_ORE] > totals[&BlockId::GOLD_ORE]);
    }

    #[test]
    fn diamonds_and_redstone_stay_deep() {
        let mut c = stone_chunk();
        for cx in 0..10 {
            place(3, &mut c, cx, 0);
        }
        for y in 40..CHUNK_H {
            for z in 0..16 {
                for x in 0..16 {
                    let b = c.get(x, y, z);
                    assert!(b != BlockId::DIAMOND_ORE && b != BlockId::REDSTONE_ORE && b != BlockId::GOLD_ORE, "found {} at y={y}", b.name());
                }
            }
        }
    }

    #[test]
    fn placement_is_deterministic_and_only_replaces_stone() {
        let mut a = stone_chunk();
        let mut b = stone_chunk();
        place(11, &mut a, -3, 4);
        place(11, &mut b, -3, 4);
        assert!(a.raw() == b.raw());
        let mut other = stone_chunk();
        place(12, &mut other, -3, 4);
        assert!(a.raw() != other.raw());
        // Air (caves) and water are never overwritten.
        let mut hollow = Chunk::empty();
        place(11, &mut hollow, -3, 4);
        assert_eq!(count(&hollow, BlockId::COAL_ORE) + count(&hollow, BlockId::GRAVEL), 0);
    }

    #[test]
    fn veins_continue_across_chunk_borders() {
        // Merge two neighbouring chunks and count ore blocks directly at the shared border: with
        // veins of 7-11 blocks, some must straddle it.
        let mut straddling = 0;
        for cz in 0..40 {
            let (mut west, mut east) = (stone_chunk(), stone_chunk());
            place(5, &mut west, 0, cz);
            place(5, &mut east, 1, cz);
            for y in 1..100 {
                for z in 0..16 {
                    let (w, e) = (west.get(15, y, z), east.get(0, y, z));
                    if w == BlockId::COAL_ORE && e == BlockId::COAL_ORE {
                        straddling += 1;
                    }
                }
            }
        }
        assert!(straddling > 0, "no vein crossed the border");
    }
}
