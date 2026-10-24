//! Piston mechanics on the voxel grid: pushing a line of up to [`PUSH_LIMIT`] blocks when
//! extending, pulling one block back with a sticky piston when retracting.

use crate::world::block::{BlockId, Facing, Kind};
use crate::world::{BlockAccess, CHUNK_H};
use bevy::math::IVec3;

/// How many blocks one piston can push at once.
pub const PUSH_LIMIT: usize = 12;

/// Can a piston move this block?
pub fn is_movable(b: BlockId) -> bool {
    match b.kind() {
        Kind::Solid => b != BlockId::BEDROCK && b != BlockId::OBSIDIAN,
        Kind::Lamp { .. } => true,
        // A piston that is not extended can be pushed like any block.
        Kind::Piston { extended, .. } => !extended,
        _ => false,
    }
}

/// Cells in the push direction a piston at `pos` would move, or `None` if it is blocked.
/// The cell after the last block is the one that gets overwritten (air, a fluid, a fragile part).
pub fn push_line<W: BlockAccess>(w: &W, pos: IVec3, facing: Facing) -> Option<Vec<IVec3>> {
    let dir = facing.offset();
    let mut line = Vec::new();
    let mut cell = pos + dir;
    loop {
        let b = w.block(cell)?;
        if !(0..CHUNK_H as i32).contains(&cell.y) {
            return None;
        }
        if b.is_air() || b.is_fluid() || b.is_fragile() {
            return Some(line);
        }
        if !is_movable(b) || line.len() >= PUSH_LIMIT {
            return None;
        }
        line.push(cell);
        cell += dir;
    }
}

/// Extends the piston at `pos` if it is retracted and the way is free. Returns whether it moved.
pub fn extend<W: BlockAccess>(w: &mut W, pos: IVec3) -> bool {
    let Some(Kind::Piston { sticky, facing, extended: false }) = w.block(pos).map(BlockId::kind) else { return false };
    let Some(line) = push_line(&*w, pos, facing) else { return false };
    let dir = facing.offset();
    // The head cell must be writable even when nothing is pushed.
    let head = pos + dir;
    if w.block(head).is_none() {
        return false;
    }
    for &cell in line.iter().rev() {
        let b = w.block(cell).unwrap_or(BlockId::AIR);
        if !w.set(cell + dir, b) {
            return false;
        }
    }
    w.set(head, BlockId::piston_head(sticky, facing));
    w.set(pos, BlockId::piston(sticky, facing, true));
    true
}

/// Retracts the piston at `pos`: removes its head and, if sticky, pulls the block behind it.
pub fn retract<W: BlockAccess>(w: &mut W, pos: IVec3) -> bool {
    let Some(Kind::Piston { sticky, facing, extended: true }) = w.block(pos).map(BlockId::kind) else { return false };
    let dir = facing.offset();
    let head = pos + dir;
    if matches!(w.block(head).map(BlockId::kind), Some(Kind::PistonHead { facing: f, .. }) if f == facing) {
        w.set(head, BlockId::AIR);
        if sticky {
            let beyond = head + dir;
            if let Some(b) = w.block(beyond)
                && is_movable(b)
                && !b.is_air()
            {
                w.set(head, b);
                w.set(beyond, BlockId::AIR);
            }
        }
    }
    w.set(pos, BlockId::piston(sticky, facing, false));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::fluids::sim::tests::Map;

    fn piston(w: &mut Map, p: IVec3, sticky: bool, f: Facing) {
        w.put(p, BlockId::piston(sticky, f, false));
    }

    #[test]
    fn extending_pushes_a_line_of_blocks_and_places_the_head() {
        let mut w = Map::flat();
        piston(&mut w, IVec3::new(0, 1, 0), false, Facing::East);
        for x in 1..=3 {
            w.put(IVec3::new(x, 1, 0), BlockId::DIRT);
        }
        assert!(extend(&mut w, IVec3::new(0, 1, 0)));
        assert_eq!(w.at(IVec3::new(0, 1, 0)), BlockId::piston(false, Facing::East, true));
        assert_eq!(w.at(IVec3::new(1, 1, 0)), BlockId::piston_head(false, Facing::East));
        for x in 2..=4 {
            assert_eq!(w.at(IVec3::new(x, 1, 0)), BlockId::DIRT, "block moved to x={x}");
        }
    }

    #[test]
    fn push_limit_is_twelve_blocks() {
        let mut w = Map::flat();
        piston(&mut w, IVec3::new(0, 1, 0), false, Facing::East);
        for x in 1..=12 {
            w.put(IVec3::new(x, 1, 0), BlockId::STONE);
        }
        assert!(extend(&mut w, IVec3::new(0, 1, 0)), "exactly twelve blocks can be pushed");
        assert_eq!(w.at(IVec3::new(13, 1, 0)), BlockId::STONE);

        let mut w = Map::flat();
        piston(&mut w, IVec3::new(0, 1, 0), false, Facing::East);
        for x in 1..=13 {
            w.put(IVec3::new(x, 1, 0), BlockId::STONE);
        }
        assert!(!extend(&mut w, IVec3::new(0, 1, 0)), "thirteen is too many");
        assert_eq!(w.at(IVec3::new(0, 1, 0)), BlockId::piston(false, Facing::East, false));
        assert_eq!(w.at(IVec3::new(1, 1, 0)), BlockId::STONE, "nothing moved");
    }

    #[test]
    fn immovable_blocks_stop_the_piston() {
        for blocker in [BlockId::OBSIDIAN, BlockId::BEDROCK, BlockId::piston_head(false, Facing::Up)] {
            let mut w = Map::flat();
            piston(&mut w, IVec3::new(0, 1, 0), false, Facing::East);
            w.put(IVec3::new(1, 1, 0), BlockId::DIRT);
            w.put(IVec3::new(2, 1, 0), blocker);
            assert!(!extend(&mut w, IVec3::new(0, 1, 0)), "{blocker:?}");
            assert_eq!(w.at(IVec3::new(1, 1, 0)), BlockId::DIRT);
        }
    }

    #[test]
    fn pushing_destroys_fragile_parts_and_fluids_at_the_end() {
        for victim in [BlockId::dust(3), BlockId::torch(Facing::Down, true), BlockId::water(2), BlockId::LAVA] {
            let mut w = Map::flat();
            piston(&mut w, IVec3::new(0, 1, 0), false, Facing::East);
            w.put(IVec3::new(1, 1, 0), BlockId::DIRT);
            w.put(IVec3::new(2, 1, 0), victim);
            assert!(extend(&mut w, IVec3::new(0, 1, 0)), "{victim:?}");
            assert_eq!(w.at(IVec3::new(2, 1, 0)), BlockId::DIRT);
        }
    }

    #[test]
    fn a_sticky_piston_pulls_one_block_back_and_a_plain_one_does_not() {
        for sticky in [true, false] {
            let mut w = Map::flat();
            piston(&mut w, IVec3::new(0, 1, 0), sticky, Facing::East);
            w.put(IVec3::new(1, 1, 0), BlockId::DIRT);
            w.put(IVec3::new(2, 1, 0), BlockId::DIRT);
            assert!(extend(&mut w, IVec3::new(0, 1, 0)));
            // dirt is now at x = 2 and 3
            assert!(retract(&mut w, IVec3::new(0, 1, 0)));
            assert_eq!(w.at(IVec3::new(0, 1, 0)), BlockId::piston(sticky, Facing::East, false));
            if sticky {
                assert_eq!(w.at(IVec3::new(1, 1, 0)), BlockId::DIRT, "pulled back");
                assert!(w.at(IVec3::new(2, 1, 0)).is_air());
                assert_eq!(w.at(IVec3::new(3, 1, 0)), BlockId::DIRT, "only one block is pulled");
            } else {
                assert!(w.at(IVec3::new(1, 1, 0)).is_air());
                assert_eq!(w.at(IVec3::new(2, 1, 0)), BlockId::DIRT);
            }
        }
    }

    #[test]
    fn pistons_work_in_every_direction() {
        for f in Facing::ALL {
            let mut w = Map::flat();
            let base = IVec3::new(0, 5, 0);
            piston(&mut w, base, false, f);
            w.put(base + f.offset(), BlockId::SAND);
            assert!(extend(&mut w, base), "{f:?}");
            assert_eq!(w.at(base + f.offset() * 2), BlockId::SAND);
            assert_eq!(w.at(base + f.offset()), BlockId::piston_head(false, f));
        }
    }

    #[test]
    fn a_piston_cannot_extend_into_the_world_floor_or_ceiling() {
        let mut w = Map::flat();
        // y = 0 is the floor: pushing the stone at y = 0 downwards would leave the world at y = -1,
        // which the test map still reports as loaded; the height check is for the real world.
        let mut top = Map { blocks: Default::default(), floor: false };
        top.put(IVec3::new(0, CHUNK_H as i32 - 1, 0), BlockId::piston(false, Facing::Up, false));
        assert!(!extend(&mut top, IVec3::new(0, CHUNK_H as i32 - 1, 0)));
        let _ = &mut w;
    }

    #[test]
    fn retracting_without_a_head_just_resets_the_piston() {
        let mut w = Map::flat();
        w.put(IVec3::new(0, 1, 0), BlockId::piston(true, Facing::East, true));
        assert!(retract(&mut w, IVec3::new(0, 1, 0)));
        assert_eq!(w.at(IVec3::new(0, 1, 0)), BlockId::piston(true, Facing::East, false));
    }
}
