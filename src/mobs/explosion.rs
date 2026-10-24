//! Explosions: ray-cast block destruction with per-block blast resistance, and the damage /
//! knockback an explosion deals to bodies standing around it.

use crate::physics::raycast::raycast;
use crate::world::block::BlockId;
use crate::world::{BlockAccess, BlockQuery};
use bevy::math::{IVec3, Vec3};

/// Rays are cast through the surface of a `GRID`-cube around the centre.
const GRID: i32 = 16;
const STEP: f32 = 0.3;

/// The cells an explosion of `power` at `center` destroys. Each of ~1350 rays loses strength
/// as it passes through blocks (more for higher blast resistance) and breaks every block it still
/// has strength for, so obsidian, bedrock and water shield what lies behind them.
pub fn blast_cells<W: BlockQuery>(w: &W, center: Vec3, power: f32, seed: u32) -> Vec<IVec3> {
    // Mix the seed: xorshift needs a well-spread state or its first outputs are all tiny.
    let mut rng = seed.wrapping_mul(0x9E37_79B9) | 1;
    for _ in 0..4 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
    }
    let mut rand = move || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        (rng >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut destroyed: std::collections::HashSet<IVec3> = std::collections::HashSet::new();
    for x in 0..GRID {
        for y in 0..GRID {
            for z in 0..GRID {
                // Only the surface of the grid cube.
                if !(x == 0 || x == GRID - 1 || y == 0 || y == GRID - 1 || z == 0 || z == GRID - 1) {
                    continue;
                }
                let dir = Vec3::new(
                    x as f32 / (GRID - 1) as f32 * 2.0 - 1.0,
                    y as f32 / (GRID - 1) as f32 * 2.0 - 1.0,
                    z as f32 / (GRID - 1) as f32 * 2.0 - 1.0,
                )
                .normalize();
                let mut strength = power * (0.7 + rand() * 0.6);
                let mut p = center;
                while strength > 0.0 {
                    let cell = p.floor().as_ivec3();
                    let Some(b) = w.block(cell) else { break };
                    if !b.is_air() {
                        strength -= (b.blast_resistance() + 0.3) * STEP;
                        if strength > 0.0 && b.blast_resistance() < 1_000_000.0 {
                            destroyed.insert(cell);
                        }
                    }
                    strength -= STEP * 0.75;
                    p += dir * STEP;
                }
            }
        }
    }
    destroyed.into_iter().collect()
}

/// Destroys the blocks of an explosion. Returns the cells that were removed.
pub fn detonate<W: BlockAccess>(w: &mut W, center: Vec3, power: f32, seed: u32) -> Vec<IVec3> {
    let cells = blast_cells(&*w, center, power, seed);
    let mut removed = Vec::with_capacity(cells.len());
    for c in cells {
        if w.block(c).is_some_and(|b| !b.is_air()) && w.set(c, BlockId::AIR) {
            removed.push(c);
        }
    }
    removed
}

/// Damage and shove an explosion deals to a body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlastEffect {
    pub damage: f32,
    pub knockback: Vec3,
}

/// What an explosion of `power` at `center` does to a box-shaped body at `feet` (width `w`,
/// height `h`). Falls off linearly to nothing at twice the power, scaled by how much of the body
/// the blast can "see" (walls shelter).
pub fn blast_effect<W: BlockQuery>(
    w: &W,
    center: Vec3,
    power: f32,
    feet: Vec3,
    width: f32,
    height: f32,
) -> Option<BlastEffect> {
    let mid = feet + Vec3::new(0.0, height * 0.5, 0.0);
    let dist = mid.distance(center);
    let reach = power * 2.0;
    if dist >= reach {
        return None;
    }
    let exposure = exposure(w, center, feet, width, height);
    let impact = (1.0 - dist / reach) * exposure;
    if impact <= 0.0 {
        return None;
    }
    let damage = ((impact * impact + impact) / 2.0 * 7.0 * reach + 1.0).floor();
    let dir = (mid - center).try_normalize().unwrap_or(Vec3::Y);
    Some(BlastEffect { damage, knockback: dir * impact * 11.0 + Vec3::Y * impact * 3.0 })
}

/// Fraction (0..=1) of sample points on the body that have a free line to the blast centre.
pub fn exposure<W: BlockQuery>(w: &W, center: Vec3, feet: Vec3, width: f32, height: f32) -> f32 {
    let hw = width * 0.5;
    let mut seen = 0;
    let mut total = 0;
    for ix in [-1.0, 0.0, 1.0] {
        for iy in [0.1, 0.5, 0.9] {
            for iz in [-1.0, 0.0, 1.0] {
                total += 1;
                let p = feet + Vec3::new(ix * hw * 0.9, iy * height, iz * hw * 0.9);
                let to = p - center;
                let d = to.length();
                let blocked = raycast(w, center, to, d).is_some_and(|hit| hit.distance < d - 0.05);
                if !blocked {
                    seen += 1;
                }
            }
        }
    }
    seen as f32 / total as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::fluids::sim::tests::Map;

    fn solid_cube(half: i32, block: BlockId) -> Map {
        let mut w = Map { blocks: Default::default(), floor: false };
        for x in -half..=half {
            for y in -half..=half {
                for z in -half..=half {
                    w.put(IVec3::new(x, 20 + y, z), block);
                }
            }
        }
        w
    }

    #[test]
    fn a_creeper_blast_carves_a_rough_sphere_in_dirt() {
        let mut w = solid_cube(8, BlockId::DIRT);
        let removed = detonate(&mut w, Vec3::new(0.5, 20.5, 0.5), 3.0, 7);
        assert!(removed.len() > 25 && removed.len() < 200, "{} blocks", removed.len());
        for c in &removed {
            let d = (c.as_vec3() + Vec3::splat(0.5) - Vec3::new(0.5, 20.5, 0.5)).length();
            assert!(d < 4.2, "{c} is {d} away");
        }
        assert!(w.at(IVec3::new(0, 20, 0)).is_air(), "the centre is gone");
        assert_eq!(w.at(IVec3::new(7, 20, 0)), BlockId::DIRT, "far blocks survive");
    }

    #[test]
    fn stone_resists_more_than_dirt_and_obsidian_and_bedrock_survive() {
        let count = |b| detonate(&mut solid_cube(8, b), Vec3::new(0.5, 20.5, 0.5), 3.0, 1).len();
        assert!(count(BlockId::DIRT) > count(BlockId::STONE));
        assert!(count(BlockId::STONE) > 0);
        assert!(count(BlockId::OBSIDIAN) <= 2);
        assert_eq!(count(BlockId::BEDROCK), 0);
    }

    #[test]
    fn water_shelters_what_is_behind_it() {
        let mut w = solid_cube(8, BlockId::DIRT);
        // A shell of water around the centre.
        for x in -2..=2i32 {
            for y in -2..=2i32 {
                for z in -2..=2i32 {
                    if x.abs() == 2 || y.abs() == 2 || z.abs() == 2 {
                        w.put(IVec3::new(x, 20 + y, z), BlockId::WATER);
                    }
                }
            }
        }
        let removed = detonate(&mut w, Vec3::new(0.5, 20.5, 0.5), 3.0, 3);
        assert!(
            removed.iter().all(|c| c.x.abs() <= 2 && (c.y - 20).abs() <= 2 && c.z.abs() <= 2),
            "contained by the water shell"
        );
    }

    #[test]
    fn explosions_are_deterministic_per_seed() {
        let run = |seed| {
            let mut v = detonate(&mut solid_cube(8, BlockId::DIRT), Vec3::new(0.5, 20.5, 0.5), 3.0, seed);
            v.sort_by_key(|c| (c.x, c.y, c.z));
            v
        };
        assert_eq!(run(5), run(5));
        // Thousands of rays saturate the crater, so other seeds carve (nearly) the same shape.
        let (a, b) = (run(5).len() as f32, run(6).len() as f32);
        assert!((a - b).abs() / a < 0.15, "{a} vs {b}");
    }

    #[test]
    fn damage_falls_off_with_distance_and_is_sheltered_by_walls() {
        let open = Map { blocks: Default::default(), floor: false };
        let center = Vec3::new(0.5, 20.5, 0.5);
        let at = |d: f32| blast_effect(&open, center, 3.0, Vec3::new(0.5 + d, 20.0, 0.5), 0.6, 1.8);
        let near = at(0.5).unwrap();
        let mid = at(3.0).unwrap();
        assert!(near.damage > mid.damage && mid.damage > 1.0, "{near:?} {mid:?}");
        assert!(near.damage >= 20.0, "point blank kills an unarmored player: {}", near.damage);
        assert!(at(6.5).is_none(), "outside twice the power nothing happens");
        assert!(near.knockback.x > 0.0 && near.knockback.y > 0.0, "pushed away and up");

        let mut walled = Map { blocks: Default::default(), floor: false };
        for y in 18..24 {
            for z in -3..=3 {
                walled.put(IVec3::new(2, y, z), BlockId::STONE);
            }
        }
        let sheltered = blast_effect(&walled, center, 3.0, Vec3::new(3.5, 20.0, 0.5), 0.6, 1.8);
        let exposed = blast_effect(&open, center, 3.0, Vec3::new(3.5, 20.0, 0.5), 0.6, 1.8).unwrap();
        assert!(sheltered.is_none() || sheltered.unwrap().damage < exposed.damage / 2.0);
    }
}
