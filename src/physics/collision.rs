//! Axis-separated swept AABB collision against voxels.
//!
//! The body is moved one axis at a time (Y, X, Z). For each axis the movement is
//! clipped against every solid block inside the swept volume, so arbitrarily
//! large per-frame deltas can never tunnel through a wall.

use crate::world::{BlockId, BlockQuery};
use bevy::math::{IVec3, Vec3};

/// Gap kept between the body and a surface it rests against.
const SKIN: f32 = 1e-4;

#[derive(Clone, Copy, Debug)]
pub struct Body {
    /// Centre of the feet.
    pub pos: Vec3,
    pub half_width: f32,
    pub height: f32,
}

impl Body {
    pub const PLAYER_HALF_WIDTH: f32 = 0.3;
    pub const PLAYER_HEIGHT: f32 = 1.8;

    pub fn player(pos: Vec3) -> Self {
        Self { pos, half_width: Self::PLAYER_HALF_WIDTH, height: Self::PLAYER_HEIGHT }
    }

    pub fn min(&self) -> Vec3 {
        self.pos - Vec3::new(self.half_width, 0.0, self.half_width)
    }

    pub fn max(&self) -> Vec3 {
        self.pos + Vec3::new(self.half_width, self.height, self.half_width)
    }

    pub fn intersects_block(&self, b: IVec3) -> bool {
        let (lo, hi) = (self.min(), self.max());
        let bf = b.as_vec3();
        lo.x < bf.x + 1.0 && hi.x > bf.x && lo.y < bf.y + 1.0 && hi.y > bf.y && lo.z < bf.z + 1.0 && hi.z > bf.z
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoveResult {
    pub pos: Vec3,
    pub hit_x: bool,
    pub hit_y: bool,
    pub hit_z: bool,
    /// Moving down and stopped by a floor.
    pub on_ground: bool,
}

/// Unloaded chunks count as solid so the player can never fall into the void.
fn solid<W: BlockQuery>(world: &W, p: IVec3) -> bool {
    world.block(p).is_none_or(BlockId::is_solid)
}

/// Moves `body` by `delta`, sliding along obstacles.
pub fn move_body<W: BlockQuery>(world: &W, body: &Body, delta: Vec3) -> MoveResult {
    let mut pos = body.pos;
    let mut res = MoveResult::default();
    for axis in [1usize, 0, 2] {
        let want = delta[axis];
        if want == 0.0 {
            continue;
        }
        let got = clip_axis(world, &Body { pos, ..*body }, axis, want);
        pos[axis] += got;
        if (got - want).abs() > 1e-6 {
            match axis {
                0 => res.hit_x = true,
                1 => res.hit_y = true,
                _ => res.hit_z = true,
            }
            if axis == 1 && want < 0.0 {
                res.on_ground = true;
            }
        }
    }
    res.pos = pos;
    res
}

/// How far the body can travel along `axis` (signed), at most `want`.
fn clip_axis<W: BlockQuery>(world: &W, body: &Body, axis: usize, want: f32) -> f32 {
    let (lo, hi) = (body.min(), body.max());
    // Region swept by the body, in block coordinates.
    let (mut sweep_lo, mut sweep_hi) = (lo, hi);
    if want > 0.0 {
        sweep_hi[axis] += want;
    } else {
        sweep_lo[axis] += want;
    }
    let range = |a: usize| ((sweep_lo[a] + SKIN).floor() as i32, (sweep_hi[a] - SKIN).floor() as i32);
    // On the other two axes only blocks that really overlap matter (strictly, so
    // touching neighbours never block).
    let overlap = |a: usize| ((lo[a] + SKIN).floor() as i32, (hi[a] - SKIN).floor() as i32);

    let mut allowed = want;
    let (a1, a2) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let (r0, r1) = range(axis);
    let (o1a, o1b) = overlap(a1);
    let (o2a, o2b) = overlap(a2);
    for i in r0..=r1 {
        for j in o1a..=o1b {
            for k in o2a..=o2b {
                let mut p = IVec3::ZERO;
                p[axis] = i;
                p[a1] = j;
                p[a2] = k;
                if !solid(world, p) {
                    continue;
                }
                if want > 0.0 {
                    let gap = i as f32 - hi[axis];
                    if gap >= -SKIN {
                        allowed = allowed.min((gap - SKIN).max(0.0));
                    }
                } else {
                    let gap = (i + 1) as f32 - lo[axis];
                    if gap <= SKIN {
                        allowed = allowed.max((gap + SKIN).min(0.0));
                    }
                }
            }
        }
    }
    allowed
}

/// True if there is solid ground within `dist` below the feet.
pub fn standing_on<W: BlockQuery>(world: &W, body: &Body, dist: f32) -> bool {
    clip_axis(world, body, 1, -dist) > -dist + 1e-6
}

/// Does any part of the body overlap water?
pub fn in_water<W: BlockQuery>(world: &W, body: &Body) -> bool {
    super::fluids::body_contact(world, body.pos, body.half_width, body.height).water
}

/// Does any part of the body overlap solid geometry?
pub fn is_stuck<W: BlockQuery>(world: &W, body: &Body) -> bool {
    let (lo, hi) = (body.min(), body.max());
    for y in (lo.y.floor() as i32)..=((hi.y - SKIN).floor() as i32) {
        for z in (lo.z.floor() as i32)..=((hi.z - SKIN).floor() as i32) {
            for x in (lo.x.floor() as i32)..=((hi.x - SKIN).floor() as i32) {
                if world.block(IVec3::new(x, y, z)).is_some_and(BlockId::is_solid) {
                    return true;
                }
            }
        }
    }
    false
}

/// A position near `pos` (straight up, at most `max_up` blocks) where the body fits, if the body
/// is currently embedded in a block (a piston or obsidian formed around it).
pub fn free_spot_above<W: BlockQuery>(world: &W, body: &Body, max_up: f32) -> Option<Vec3> {
    let mut dy = 0.0;
    while dy <= max_up {
        let b = Body { pos: body.pos + Vec3::new(0.0, dy, 0.0), ..*body };
        if !is_stuck(world, &b) {
            return Some(b.pos);
        }
        dy += 0.25;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Solid ground up to and including y = 0, optional extra blocks.
    struct Flat {
        extra: Vec<IVec3>,
    }
    impl BlockQuery for Flat {
        fn block(&self, p: IVec3) -> Option<BlockId> {
            if p.y <= 0 || self.extra.contains(&p) { Some(BlockId::STONE) } else { Some(BlockId::AIR) }
        }
    }

    fn at(x: f32, y: f32, z: f32) -> Body {
        Body::player(Vec3::new(x, y, z))
    }

    #[test]
    fn falls_and_lands_on_floor() {
        let w = Flat { extra: vec![] };
        let r = move_body(&w, &at(0.5, 5.0, 0.5), Vec3::new(0.0, -10.0, 0.0));
        assert!(r.on_ground && r.hit_y);
        assert!((r.pos.y - 1.0).abs() < 1e-3, "rests on top of block y=0: {}", r.pos.y);
        assert!(standing_on(&w, &at(0.5, r.pos.y, 0.5), 0.01));
    }

    #[test]
    fn huge_step_cannot_tunnel_through_a_wall() {
        let w = Flat { extra: (1..=3).map(|y| IVec3::new(3, y, 0)).collect() };
        let r = move_body(&w, &at(0.5, 1.0, 0.5), Vec3::new(50.0, 0.0, 0.0));
        assert!(r.hit_x);
        assert!(r.pos.x <= 3.0 - 0.3 + 1e-3 && r.pos.x > 2.6, "x = {}", r.pos.x);
    }

    #[test]
    fn slides_along_wall() {
        let w = Flat { extra: (1..=3).map(|y| IVec3::new(3, y, 0)).collect() };
        // Moving diagonally into the wall: X is blocked, Z continues.
        let r = move_body(&w, &at(2.6, 1.0, 0.5), Vec3::new(1.0, 0.0, 0.4));
        assert!(r.hit_x && !r.hit_z);
        assert!((r.pos.z - 0.9).abs() < 1e-4);
    }

    #[test]
    fn head_bumps_ceiling() {
        let w = Flat { extra: vec![IVec3::new(0, 3, 0)] };
        let r = move_body(&w, &at(0.5, 1.0, 0.5), Vec3::new(0.0, 5.0, 0.0));
        assert!(r.hit_y && !r.on_ground);
        assert!((r.pos.y + 1.8 - 3.0).abs() < 1e-3);
    }

    #[test]
    fn resting_contact_does_not_block_sideways_motion() {
        let w = Flat { extra: vec![] };
        let b = at(0.5, 1.0, 0.5);
        let r = move_body(&w, &b, Vec3::new(2.0, -0.01, 0.0));
        assert!(!r.hit_x, "sliding on the floor must not register a wall");
        assert!((r.pos.x - 2.5).abs() < 1e-4);
    }

    #[test]
    fn unloaded_world_is_solid() {
        struct Void;
        impl BlockQuery for Void {
            fn block(&self, _: IVec3) -> Option<BlockId> {
                None
            }
        }
        let r = move_body(&Void, &at(0.5, 10.0, 0.5), Vec3::new(0.0, -1.0, 0.0));
        assert!(r.hit_y);
    }

    #[test]
    fn many_random_steps_never_end_inside_geometry() {
        let w =
            Flat { extra: vec![IVec3::new(2, 1, 2), IVec3::new(3, 1, 2), IVec3::new(3, 2, 2), IVec3::new(-2, 1, 0)] };
        let mut b = at(0.5, 3.0, 0.5);
        let mut seed = 12345u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        };
        for _ in 0..5000 {
            let d = Vec3::new(rnd() * 1.5, rnd() * 1.5, rnd() * 1.5);
            b.pos = move_body(&w, &b, d).pos;
            for x in -4..6 {
                for y in 0..5 {
                    for z in -4..6 {
                        let p = IVec3::new(x, y, z);
                        if w.block(p).unwrap().is_solid() {
                            assert!(!b.intersects_block(p), "inside {p} at {:?}", b.pos);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod embed_tests {
    use super::*;

    struct Slab;
    impl BlockQuery for Slab {
        fn block(&self, p: IVec3) -> Option<BlockId> {
            Some(if p.y <= 0 || (p.y <= 2 && p.x == 0 && p.z == 0) { BlockId::STONE } else { BlockId::AIR })
        }
    }

    #[test]
    fn an_embedded_body_is_lifted_out() {
        let body = Body::player(Vec3::new(0.5, 1.0, 0.5));
        assert!(is_stuck(&Slab, &body));
        let out = free_spot_above(&Slab, &body, 4.0).unwrap();
        assert!(out.y >= 3.0 && out.y < 3.3, "{out}");
        assert!(!is_stuck(&Slab, &Body::player(out)));
        assert!(free_spot_above(&Slab, &body, 1.0).is_none());
    }
}
