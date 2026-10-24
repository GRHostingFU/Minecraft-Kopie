//! Where and when hostile mobs may appear: dark places only. A spot is dark at night anywhere
//! under the open sky and always in caves or under overhangs, unless a light source (lava, a
//! torch, a lit lamp) is near.

use crate::world::BlockQuery;
use crate::world::block::BlockId;
use bevy::math::IVec3;

/// Daylight (0..1) below which the open sky counts as dark.
pub const DARK_DAYLIGHT: f32 = 0.3;
/// Daylight above which exposed skeletons burn.
pub const BURN_DAYLIGHT: f32 = 0.55;
/// Radius around a light source in which nothing spawns.
const LIGHT_RADIUS: i32 = 6;
/// How far up the sky is searched for a roof.
const SKY_SEARCH: i32 = 96;

/// Is there open sky above `cell` (nothing opaque in the column above it)?
pub fn sky_exposed<W: BlockQuery>(w: &W, cell: IVec3) -> bool {
    (1..=SKY_SEARCH).all(|dy| {
        // Unloaded space above counts as open: the column is only a few chunks high. Sunlight
        // shines through leaves.
        w.block(cell + IVec3::new(0, dy, 0)).is_none_or(|b| !b.is_opaque() || b == BlockId::LEAVES)
    })
}

/// Is a light source within [`LIGHT_RADIUS`] blocks (Manhattan distance) of `cell`?
pub fn near_light<W: BlockQuery>(w: &W, cell: IVec3) -> bool {
    for dx in -LIGHT_RADIUS..=LIGHT_RADIUS {
        for dz in -LIGHT_RADIUS..=LIGHT_RADIUS {
            let rest = LIGHT_RADIUS - dx.abs() - dz.abs();
            if rest < 0 {
                continue;
            }
            for dy in -rest.min(3)..=rest.min(3) {
                if w.block(cell + IVec3::new(dx, dy, dz)).is_some_and(BlockId::gives_light) {
                    return true;
                }
            }
        }
    }
    false
}

/// May a hostile mob spawn with its feet in `cell`?
pub fn is_dark<W: BlockQuery>(w: &W, cell: IVec3, daylight: f32) -> bool {
    if near_light(w, cell) {
        return false;
    }
    !sky_exposed(w, cell + IVec3::Y) || daylight < DARK_DAYLIGHT
}

/// Does the sun burn a mob standing at `feet` (head `height` above)?
pub fn burns_in_sun<W: BlockQuery>(w: &W, feet: bevy::math::Vec3, height: f32, daylight: f32) -> bool {
    if daylight < BURN_DAYLIGHT {
        return false;
    }
    let head = IVec3::new(feet.x.floor() as i32, (feet.y + height).floor() as i32, feet.z.floor() as i32);
    let in_water =
        w.block(feet.floor().as_ivec3()).is_some_and(BlockId::is_water) || w.block(head).is_some_and(BlockId::is_water);
    !in_water && sky_exposed(w, head)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::fluids::sim::tests::Map;
    use bevy::math::Vec3;

    #[test]
    fn open_ground_is_dark_only_at_night() {
        let w = Map::flat();
        let cell = IVec3::new(0, 1, 0);
        assert!(!is_dark(&w, cell, 1.0));
        assert!(!is_dark(&w, cell, 0.5));
        assert!(is_dark(&w, cell, 0.1));
        assert!(is_dark(&w, cell, 0.0));
    }

    #[test]
    fn tree_canopies_do_not_make_the_ground_dark() {
        let mut w = Map::flat();
        w.put(IVec3::new(0, 6, 0), BlockId::LEAVES);
        assert!(sky_exposed(&w, IVec3::new(0, 2, 0)));
        assert!(!is_dark(&w, IVec3::new(0, 1, 0), 1.0));
        w.put(IVec3::new(0, 7, 0), BlockId::PLANKS);
        assert!(!sky_exposed(&w, IVec3::new(0, 2, 0)), "a real roof does");
    }

    #[test]
    fn caves_are_dark_even_at_noon() {
        let mut w = Map::flat();
        for x in -3..=3 {
            for z in -3..=3 {
                w.put(IVec3::new(x, 4, z), BlockId::STONE);
            }
        }
        assert!(!sky_exposed(&w, IVec3::new(0, 2, 0)));
        assert!(is_dark(&w, IVec3::new(0, 1, 0), 1.0));
        // Step outside the roof: sunlit again.
        assert!(!is_dark(&w, IVec3::new(8, 1, 0), 1.0));
    }

    #[test]
    fn light_sources_keep_mobs_away() {
        let mut w = Map::flat();
        for x in -3..=3 {
            for z in -3..=3 {
                w.put(IVec3::new(x, 4, z), BlockId::STONE);
            }
        }
        let cell = IVec3::new(0, 1, 0);
        assert!(is_dark(&w, cell, 0.0));
        w.put(IVec3::new(2, 1, 0), BlockId::torch(crate::world::Facing::Down, true));
        assert!(!is_dark(&w, cell, 0.0), "a lit torch two blocks away");
        w.put(IVec3::new(2, 1, 0), BlockId::torch(crate::world::Facing::Down, false));
        assert!(is_dark(&w, cell, 0.0), "an unlit torch gives no light");
        w.put(IVec3::new(0, 1, 5), BlockId::LAVA);
        assert!(!is_dark(&w, cell, 0.0), "lava glows");
        w.put(IVec3::new(0, 1, 5), BlockId::AIR);
        w.put(IVec3::new(-1, 1, 2), BlockId::lamp(true));
        assert!(!is_dark(&w, cell, 0.0));
        // Far away lights do not matter.
        w.put(IVec3::new(-1, 1, 2), BlockId::AIR);
        w.put(IVec3::new(0, 1, 20), BlockId::lamp(true));
        assert!(is_dark(&w, cell, 0.0));
    }

    #[test]
    fn skeletons_burn_only_in_daylight_under_open_sky_and_not_in_water() {
        let mut w = Map::flat();
        let feet = Vec3::new(0.5, 1.0, 0.5);
        assert!(burns_in_sun(&w, feet, 1.95, 1.0));
        assert!(!burns_in_sun(&w, feet, 1.95, 0.2), "no sun at night");
        w.put(IVec3::new(0, 5, 0), BlockId::PLANKS);
        assert!(!burns_in_sun(&w, feet, 1.95, 1.0), "shade protects");
        let mut w = Map::flat();
        w.put(IVec3::new(0, 1, 0), BlockId::WATER);
        assert!(!burns_in_sun(&w, feet, 1.95, 1.0), "water puts the fire out");
    }
}
