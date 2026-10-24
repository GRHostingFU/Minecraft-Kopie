//! Voxel ray traversal (Amanatides & Woo DDA).

use crate::world::BlockQuery;
use crate::world::block::select_box;
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
            Some(b) if b.is_targetable() && !b.is_model() => {
                return Some(RayHit { block: cell, normal, distance: dist });
            }
            // Thin parts (levers, dust, ...) are only hit where their box really is.
            Some(b) if b.is_model() => {
                let (lo, hi) = select_box(b);
                if let Some((t, n)) = ray_box(origin, dir, cell.as_vec3() + lo, cell.as_vec3() + hi)
                    && t <= max_dist
                {
                    return Some(RayHit { block: cell, normal: n, distance: t });
                }
            }
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

/// Ray against an axis-aligned box: entry distance and the outward normal of the face hit
/// (`ZERO` if the ray starts inside the box).
pub fn ray_box(origin: Vec3, dir: Vec3, lo: Vec3, hi: Vec3) -> Option<(f32, IVec3)> {
    let (mut t_near, mut t_far) = (f32::NEG_INFINITY, f32::INFINITY);
    let mut normal = IVec3::ZERO;
    for a in 0..3 {
        if dir[a].abs() < 1e-9 {
            if origin[a] < lo[a] || origin[a] > hi[a] {
                return None;
            }
            continue;
        }
        let (mut t0, mut t1) = ((lo[a] - origin[a]) / dir[a], (hi[a] - origin[a]) / dir[a]);
        let mut face = -1;
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
            face = 1;
        }
        if t0 > t_near {
            t_near = t0;
            normal = IVec3::ZERO;
            normal[a] = face;
        }
        t_far = t_far.min(t1);
        if t_near > t_far {
            return None;
        }
    }
    if t_far < 0.0 {
        return None;
    }
    if t_near < 0.0 { Some((0.0, IVec3::ZERO)) } else { Some((t_near, normal)) }
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

    #[test]
    fn thin_parts_are_only_hit_where_their_box_is() {
        use crate::world::block::Facing;
        let mut w = world(&[]);
        w.0.insert(IVec3::new(0, 0, 3), BlockId::STONE_PLATE);
        // Straight down onto the plate's footprint hits it from above.
        let hit = raycast(&w, Vec3::new(0.5, 3.0, 3.5), Vec3::NEG_Y, 6.0).unwrap();
        assert_eq!((hit.block, hit.normal), (IVec3::new(0, 0, 3), IVec3::Y));
        // A shallow ray passing over the 1/16-high plate misses it.
        assert!(raycast(&w, Vec3::new(0.5, 0.5, 0.5), Vec3::new(0.0, -0.02, 1.0), 6.0).is_none());
        // A lever on a wall can be clicked.
        w.0.insert(IVec3::new(5, 0, 5), BlockId::lever(Facing::West, false));
        let hit = raycast(&w, Vec3::new(8.0, 0.5, 5.5), Vec3::NEG_X, 6.0).unwrap();
        assert_eq!(hit.block, IVec3::new(5, 0, 5));
        assert_eq!(hit.normal, IVec3::X);
    }

    #[test]
    fn ray_box_reports_entry_face() {
        let (t, n) = ray_box(Vec3::new(-2.0, 0.5, 0.5), Vec3::X, Vec3::ZERO, Vec3::ONE).unwrap();
        assert!((t - 2.0).abs() < 1e-5 && n == IVec3::NEG_X);
        let (t, n) = ray_box(Vec3::new(0.5, 0.5, 0.5), Vec3::X, Vec3::ZERO, Vec3::ONE).unwrap();
        assert_eq!((t, n), (0.0, IVec3::ZERO));
        assert!(ray_box(Vec3::new(-2.0, 5.0, 0.5), Vec3::X, Vec3::ZERO, Vec3::ONE).is_none());
        assert!(ray_box(Vec3::new(2.0, 0.5, 0.5), Vec3::X, Vec3::ZERO, Vec3::ONE).is_none());
    }
}
