//! Redstone signal rules: which part gives how much power to which neighbour.
//!
//! The model follows the familiar one. Sources (levers, buttons, pressure plates, lit torches)
//! give a *weak* signal of 15 to the blocks around them and a *strong* signal to one specific
//! block (the one they are attached to / stand on / sit below), which then powers its own
//! neighbours. Dust gives its power to the blocks it points at and to the block below it.
//! Power in dust decays by one per block; see [`super::sim`] for the wire solver.

use super::rules::{dust_arms, effective_arms};
use crate::world::BlockQuery;
use crate::world::block::{BlockId, Facing, Kind};
use bevy::math::IVec3;

pub const MAX_POWER: u8 = 15;

fn at<W: BlockQuery>(w: &W, p: IVec3) -> BlockId {
    w.block(p).unwrap_or(BlockId::AIR)
}

/// Signal the part at `pos` gives to its neighbour in direction `toward`.
/// With `ignore_wire`, dust gives nothing (used when solving the wire itself).
pub fn emitted_signal<W: BlockQuery>(w: &W, pos: IVec3, toward: Facing, ignore_wire: bool) -> u8 {
    match at(w, pos).kind() {
        Kind::Lever { on: true, .. } | Kind::Button { pressed: true, .. } | Kind::Plate { pressed: true } => MAX_POWER,
        Kind::Torch { attach, lit: true } if toward != attach => MAX_POWER,
        Kind::Dust { power } if power > 0 && !ignore_wire => dust_emits(w, pos, toward, power),
        _ => 0,
    }
}

fn dust_emits<W: BlockQuery>(w: &W, pos: IVec3, toward: Facing, power: u8) -> u8 {
    match toward {
        Facing::Up => 0,
        Facing::Down => power,
        horizontal => {
            let arms = dust_arms(|p| at(w, p), pos);
            let any = arms.iter().any(Option::is_some);
            let eff = effective_arms(&arms);
            let pointing = Facing::HORIZONTAL.iter().position(|f| *f == horizontal).is_some_and(|i| eff[i]);
            if pointing || !any { power } else { 0 }
        }
    }
}

/// Signal the part at `pos` gives *strongly* to the neighbour in direction `toward`; a strongly
/// powered block passes the power on to everything around it.
pub fn emitted_strong<W: BlockQuery>(w: &W, pos: IVec3, toward: Facing, ignore_wire: bool) -> u8 {
    match at(w, pos).kind() {
        Kind::Lever { attach, on: true } | Kind::Button { attach, pressed: true } if toward == attach => MAX_POWER,
        Kind::Plate { pressed: true } if toward == Facing::Down => MAX_POWER,
        Kind::Torch { lit: true, .. } if toward == Facing::Up => MAX_POWER,
        Kind::Dust { power } if power > 0 && !ignore_wire => dust_emits(w, pos, toward, power),
        _ => 0,
    }
}

/// Power a conductor block at `pos` receives from the parts that power it strongly.
pub fn conductor_power<W: BlockQuery>(w: &W, pos: IVec3, ignore_wire: bool) -> u8 {
    Facing::ALL.into_iter().map(|f| emitted_strong(w, pos + f.offset(), f.opposite(), ignore_wire)).max().unwrap_or(0)
}

/// Signal arriving at `pos` from its neighbour in direction `dir`.
pub fn input_from<W: BlockQuery>(w: &W, pos: IVec3, dir: Facing, ignore_wire: bool) -> u8 {
    let n = pos + dir.offset();
    if at(w, n).is_conductor() {
        conductor_power(w, n, ignore_wire)
    } else {
        emitted_signal(w, n, dir.opposite(), ignore_wire)
    }
}

/// Strongest signal arriving at `pos` from any side except `skip`.
pub fn input_power<W: BlockQuery>(w: &W, pos: IVec3, skip: Option<Facing>, ignore_wire: bool) -> u8 {
    Facing::ALL.into_iter().filter(|f| Some(*f) != skip).map(|f| input_from(w, pos, f, ignore_wire)).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct W(HashMap<IVec3, BlockId>);
    impl BlockQuery for W {
        fn block(&self, p: IVec3) -> Option<BlockId> {
            Some(self.0.get(&p).copied().unwrap_or(BlockId::AIR))
        }
    }
    fn world(blocks: &[(IVec3, BlockId)]) -> W {
        W(blocks.iter().copied().collect())
    }

    #[test]
    fn a_lever_powers_adjacent_parts_and_strongly_its_support() {
        // Lever on the east face of a stone block at the origin.
        let w = world(&[
            (IVec3::new(0, 0, 0), BlockId::STONE),
            (IVec3::new(1, 0, 0), BlockId::lever(Facing::West, true)),
            (IVec3::new(0, 0, 1), BlockId::PISTON),
            (IVec3::new(1, 0, 1), BlockId::lamp(false)),
        ]);
        assert_eq!(conductor_power(&w, IVec3::ZERO, false), 15, "the support block is strongly powered");
        // The lamp next to the lever is powered directly.
        assert_eq!(input_power(&w, IVec3::new(1, 0, 1), None, false), 15);
        // The piston beside the stone block gets power through the block.
        assert_eq!(input_power(&w, IVec3::new(0, 0, 1), None, false), 15);
    }

    #[test]
    fn a_torch_powers_the_block_above_but_not_the_one_it_stands_on() {
        let w = world(&[
            (IVec3::new(0, 0, 0), BlockId::STONE),
            (IVec3::new(0, 1, 0), BlockId::torch(Facing::Down, true)),
            (IVec3::new(0, 2, 0), BlockId::STONE),
        ]);
        assert_eq!(conductor_power(&w, IVec3::new(0, 2, 0), false), 15);
        assert_eq!(conductor_power(&w, IVec3::ZERO, false), 0);
        let off =
            world(&[(IVec3::new(0, 1, 0), BlockId::torch(Facing::Down, false)), (IVec3::new(0, 2, 0), BlockId::STONE)]);
        assert_eq!(conductor_power(&off, IVec3::new(0, 2, 0), false), 0);
    }

    #[test]
    fn dust_powers_what_it_points_at_and_the_block_below() {
        let w = world(&[
            (IVec3::new(0, 0, 0), BlockId::STONE),
            (IVec3::new(0, 1, 0), BlockId::dust(9)),
            (IVec3::new(1, 1, 0), BlockId::dust(8)),
            (IVec3::new(0, 1, 1), BlockId::PISTON),
        ]);
        // Dust at (0,1,0) points east only (its single connection), so it powers (1,1,0)'s side
        // and the block below it, but not the piston to the south.
        assert_eq!(emitted_signal(&w, IVec3::new(0, 1, 0), Facing::East, false), 9);
        assert_eq!(emitted_signal(&w, IVec3::new(0, 1, 0), Facing::South, false), 0);
        assert_eq!(conductor_power(&w, IVec3::ZERO, false), 9);
        assert_eq!(conductor_power(&w, IVec3::ZERO, true), 0, "ignored while solving the wire");
        // A lone dust powers all four sides.
        let lone = world(&[(IVec3::new(0, 1, 0), BlockId::dust(5))]);
        for f in Facing::HORIZONTAL {
            assert_eq!(emitted_signal(&lone, IVec3::new(0, 1, 0), f, false), 5);
        }
    }

    #[test]
    fn unpowered_parts_give_nothing() {
        let w = world(&[
            (IVec3::new(0, 0, 0), BlockId::lever(Facing::Down, false)),
            (IVec3::new(2, 0, 0), BlockId::STONE_BUTTON),
            (IVec3::new(4, 0, 0), BlockId::plate(false)),
        ]);
        for x in [0, 2, 4] {
            for f in Facing::ALL {
                assert_eq!(emitted_signal(&w, IVec3::new(x, 0, 0), f, false), 0);
            }
        }
    }
}
