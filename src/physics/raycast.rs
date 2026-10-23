//! Voxel ray traversal (Amanatides & Woo DDA).

use crate::world::BlockQuery;
use bevy::math::{IVec3, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    /// The targeted block.
    pub block: IVec3,
    /// Outward normal of the face that was hit; `ZERO` if the ray started inside the block.
    pub normal: IVec3,
    pub distance: f32,
}

impl RayHit {
    /// Where a new block would be placed.
    pub fn place_pos(&self) -> IVec3 {
        self.block + self.normal
    }
}

/// Casts a ray and returns the first targetable block within `max_dist`.
/// Unloaded chunks end the ray.
pub fn raycast<W: BlockQuery>(world: &W, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<RayHit> {
    let dir = dir.try_normalize()?;
    let mut cell = origin.floor().as_ivec3();
    let step = IVec3::new(dir.x.signum() as i32, dir.y.signum() as i32, dir.z.signum() as i32);
    let mut t_max = Vec3::ZERO;
    let mut t_delta = Vec3::splat(f32::INFINITY);
    for a in 0..3 {
        if dir[a] != 0.0 {
            t_delta[a] = (1.0 / dir[a]).abs();
            let next_boundary = if step[a] > 0 { cell[a] as f32 + 1.0 } else { cell[a] as f32 };
            t_max[a] = (next_boundary - origin[a]) / dir[a];
        } else {
            t_max[a] = f32::INFINITY;
        }
    }

    let mut normal = IVec3::ZERO;
    let mut dist = 0.0;
    loop {
        match world.block(cell) {
            None => return None,
            Some(b) if b.is_targetable() => return Some(RayHit { block: cell, normal, distance: dist }),
            _ => {}
        }
        // Advance along the axis whose boundary is nearest.
        let axis = if t_max.x < t_max.y && t_max.x < t_max.z {
            0
        } else if t_max.y < t_max.z {
            1
        } else {
            2
        };
        dist = t_max[axis];
        if dist > max_dist {
            return None;
        }
        cell[axis] += step[axis];
        t_max[axis] += t_delta[axis];
        normal = IVec3::ZERO;
        normal[axis] = -step[axis];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::BlockId;
    use std::collections::HashMap;

    struct Blocks(HashMap<IVec3, BlockId>);
    impl BlockQuery for Blocks {
        fn block(&self, p: IVec3) -> Option<BlockId> {
            Some(self.0.get(&p).copied().unwrap_or(BlockId::AIR))
        }
    }

    fn world(blocks: &[IVec3]) -> Blocks {
        Blocks(blocks.iter().map(|&p| (p, BlockId::STONE)).collect())
    }

    #[test]
    fn hits_face_with_correct_normal_and_distance() {
        let w = world(&[IVec3::new(5, 0, 0)]);
        let hit = raycast(&w, Vec3::new(0.5, 0.5, 0.5), Vec3::X, 10.0).unwrap();
        assert_eq!(hit.block, IVec3::new(5, 0, 0));
        assert_eq!(hit.normal, IVec3::NEG_X);
        assert!((hit.distance - 4.5).abs() < 1e-5);
        assert_eq!(hit.place_pos(), IVec3::new(4, 0, 0));
    }

    #[test]
    fn negative_directions_and_vertical() {
        let w = world(&[IVec3::new(0, -3, 0), IVec3::new(-4, 0, -4)]);
        let hit = raycast(&w, Vec3::new(0.5, 0.5, 0.5), Vec3::NEG_Y, 10.0).unwrap();
        assert_eq!((hit.block, hit.normal), (IVec3::new(0, -3, 0), IVec3::Y));
        let hit = raycast(&w, Vec3::new(0.5, 0.5, 0.5), Vec3::new(-1.0, 0.0, -1.0), 10.0).unwrap();
        assert_eq!(hit.block, IVec3::new(-4, 0, -4));
    }

    #[test]
    fn respects_max_distance_and_misses() {
        let w = world(&[IVec3::new(20, 0, 0)]);
        assert!(raycast(&w, Vec3::new(0.5, 0.5, 0.5), Vec3::X, 6.0).is_none());
        assert!(raycast(&w, Vec3::new(0.5, 0.5, 0.5), Vec3::Y, 50.0).is_none());
        assert!(raycast(&w, Vec3::ZERO, Vec3::ZERO, 6.0).is_none());
    }

    #[test]
    fn water_is_not_targetable() {
        let mut w = world(&[IVec3::new(3, 0, 0)]);
        w.0.insert(IVec3::new(1, 0, 0), BlockId::WATER);
        let hit = raycast(&w, Vec3::new(0.5, 0.5, 0.5), Vec3::X, 10.0).unwrap();
        assert_eq!(hit.block, IVec3::new(3, 0, 0));
    }

    #[test]
    fn diagonal_ray_agrees_with_dense_sampling() {
        // Compare the DDA against brute-force marching for many directions.
        let mut cells = Vec::new();
        for x in -6..=6i32 {
            for z in -6..=6 {
                if (x * 7 + z * 13).rem_euclid(5) == 0 && (x, z) != (0, 0) {
                    cells.push(IVec3::new(x, 0, z));
                }
            }
        }
        let w = world(&cells);
        let origin = Vec3::new(0.37, 0.5, 0.61);
        for i in 0..360 {
            let a = (i as f32).to_radians();
            let dir = Vec3::new(a.cos(), 0.0, a.sin());
            let dda = raycast(&w, origin, dir, 8.0).map(|h| h.block);
            let mut brute = None;
            let mut t = 0.0;
            while t < 8.0 {
                let c = (origin + dir * t).floor().as_ivec3();
                if cells.contains(&c) {
                    brute = Some(c);
                    break;
                }
                t += 0.001;
            }
            assert_eq!(dda, brute, "angle {i}");
        }
    }
}
