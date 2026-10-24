//! Which block state a held item turns into when it is placed against a face.

use super::block::{BlockId, Facing, Kind};
use bevy::math::{IVec3, Vec3};

/// The state of `item` (a canonical block id) when placed against a face with outward normal
/// `normal` by a player looking along `look`. `None` if it cannot be placed that way (a torch
/// on a ceiling, for instance).
pub fn placement_state(item: BlockId, normal: IVec3, look: Vec3) -> Option<BlockId> {
    let face = Facing::from_offset(normal);
    // The direction from the new block towards the block it is attached to.
    let support = face.map(Facing::opposite);
    match item.kind() {
        Kind::Dust { .. } => Some(BlockId::dust(0)),
        Kind::Plate { .. } => Some(BlockId::plate(false)),
        Kind::Lamp { .. } => Some(BlockId::lamp(false)),
        Kind::Torch { .. } => attached(support).map(|a| BlockId::torch(a, true)),
        Kind::Lever { .. } => attached(support).map(|a| BlockId::lever(a, false)),
        Kind::Button { .. } => attached(support).map(|a| BlockId::button(a, false)),
        // Pistons face the player, like in the original.
        Kind::Piston { sticky, .. } => Some(BlockId::piston(sticky, Facing::dominant(look).opposite(), false)),
        Kind::Water(_) => Some(BlockId::WATER),
        Kind::Lava(_) => Some(BlockId::LAVA),
        Kind::PistonHead { .. } | Kind::Air => None,
        Kind::Solid => Some(item),
    }
}

/// Parts can hang on walls and stand on floors, but not on ceilings.
fn attached(support: Option<Facing>) -> Option<Facing> {
    support.filter(|f| *f != Facing::Up)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torches_and_levers_attach_to_the_face_they_are_placed_on() {
        let on_floor = placement_state(BlockId::REDSTONE_TORCH, IVec3::Y, Vec3::NEG_Z);
        assert_eq!(on_floor, Some(BlockId::torch(Facing::Down, true)));
        // Placed against the east face of a block: the block is to the west.
        let wall = placement_state(BlockId::LEVER, IVec3::X, Vec3::NEG_X);
        assert_eq!(wall, Some(BlockId::lever(Facing::West, false)));
        assert_eq!(placement_state(BlockId::STONE_BUTTON, IVec3::NEG_Z, Vec3::Z), Some(BlockId::button(Facing::South, false)));
        assert_eq!(placement_state(BlockId::REDSTONE_TORCH, IVec3::NEG_Y, Vec3::Y), None, "no ceiling torches");
    }

    #[test]
    fn pistons_face_the_player() {
        let looking_north = placement_state(BlockId::PISTON, IVec3::Y, Vec3::new(0.1, -0.2, -1.0));
        assert_eq!(looking_north, Some(BlockId::piston(false, Facing::South, false)));
        let looking_down = placement_state(BlockId::STICKY_PISTON, IVec3::Y, Vec3::new(0.1, -1.0, 0.1));
        assert_eq!(looking_down, Some(BlockId::piston(true, Facing::Up, false)));
    }

    #[test]
    fn plain_blocks_and_fluids() {
        assert_eq!(placement_state(BlockId::STONE, IVec3::Y, Vec3::X), Some(BlockId::STONE));
        assert_eq!(placement_state(BlockId::WATER, IVec3::Y, Vec3::X), Some(BlockId::WATER));
        assert_eq!(placement_state(BlockId::LAVA, IVec3::Y, Vec3::X), Some(BlockId::LAVA));
        assert_eq!(placement_state(BlockId::REDSTONE_DUST, IVec3::Y, Vec3::X), Some(BlockId::dust(0)));
        assert_eq!(placement_state(BlockId::REDSTONE_LAMP, IVec3::Y, Vec3::X), Some(BlockId::lamp(false)));
    }
}
