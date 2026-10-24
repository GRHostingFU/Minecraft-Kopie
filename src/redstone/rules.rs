//! Static rules about how redstone parts connect and what they need to stand on. Pure functions
//! of the surrounding blocks; used by the simulation and by the mesher (to draw dust arms).

use crate::world::block::{BlockId, Facing, Kind};
use bevy::math::IVec3;

/// Where dust connects to horizontally: `Some(dy)` with the height offset of the neighbouring
/// wire (`0` same level, `1` climbs a step, `-1` drops down), `None` if there is no connection.
pub type DustArms = [Option<i8>; 4];

/// Parts a wire visually and logically attaches to when they stand next to it.
fn is_wire_partner(b: BlockId) -> bool {
    matches!(
        b.kind(),
        Kind::Dust { .. } | Kind::Torch { .. } | Kind::Lever { .. } | Kind::Button { .. } | Kind::Plate { .. }
    )
}

/// Connections of the dust at `pos`, in the order of [`Facing::HORIZONTAL`].
pub fn dust_arms(get: impl Fn(IVec3) -> BlockId, pos: IVec3) -> DustArms {
    let above_is_open = !get(pos + IVec3::Y).is_opaque();
    let mut arms = [None; 4];
    for (i, f) in Facing::HORIZONTAL.into_iter().enumerate() {
        let n = pos + f.offset();
        let side = get(n);
        arms[i] = if is_wire_partner(side) {
            Some(0)
        } else if above_is_open && matches!(get(n + IVec3::Y).kind(), Kind::Dust { .. }) && side.is_opaque() {
            // A wire on top of the neighbouring block: climb up.
            Some(1)
        } else if !side.is_opaque() && matches!(get(n - IVec3::Y).kind(), Kind::Dust { .. }) {
            Some(-1)
        } else {
            None
        };
    }
    arms
}

/// The directions dust effectively points in: its connections, extended to a straight line when
/// it connects on only one side of an axis (so the end of a dust line powers the block it ends
/// at). Dust without any connection is a dot and is not covered here.
pub fn effective_arms(arms: &DustArms) -> [bool; 4] {
    // Order: North, South, West, East.
    let c = arms.map(|a| a.is_some());
    let (ns, ew) = (c[0] || c[1], c[2] || c[3]);
    let mut e = c;
    if ns && !ew {
        e[0] = true;
        e[1] = true;
    }
    if ew && !ns {
        e[2] = true;
        e[3] = true;
    }
    e
}

/// Only dust-to-dust links carry power along the wire (levers etc. feed it from outside).
pub fn dust_links(get: impl Fn(IVec3) -> BlockId, pos: IVec3) -> impl Iterator<Item = IVec3> {
    let arms = dust_arms(&get, pos);
    Facing::HORIZONTAL.into_iter().zip(arms).filter_map(move |(f, arm)| {
        let dy = arm?;
        let n = pos + f.offset() + IVec3::new(0, dy as i32, 0);
        matches!(get(n).kind(), Kind::Dust { .. }).then_some(n)
    })
}

/// Can `part` exist at `pos` — is there something to stand on / hang from?
pub fn has_support(get: impl Fn(IVec3) -> BlockId, pos: IVec3, part: BlockId) -> bool {
    let solid_at = |f: Facing| get(pos + f.offset()).is_opaque() || is_piston_support(get(pos + f.offset()));
    match part.kind() {
        Kind::Dust { .. } | Kind::Plate { .. } => solid_at(Facing::Down),
        Kind::Torch { attach, .. } | Kind::Lever { attach, .. } | Kind::Button { attach, .. } => solid_at(attach),
        Kind::PistonHead { facing, .. } => matches!(
            get(pos + facing.opposite().offset()).kind(),
            Kind::Piston { extended: true, facing: f, .. } if f == facing
        ),
        _ => true,
    }
}

/// Piston heads can hold a lever etc. like any other block.
fn is_piston_support(b: BlockId) -> bool {
    matches!(b.kind(), Kind::PistonHead { .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn world(blocks: &[(IVec3, BlockId)]) -> impl Fn(IVec3) -> BlockId + use<> {
        let m: HashMap<_, _> = blocks.iter().copied().collect();
        move |p| m.get(&p).copied().unwrap_or(BlockId::AIR)
    }

    #[test]
    fn dust_connects_to_neighbours_on_the_same_level() {
        let w = world(&[
            (IVec3::new(0, 0, 0), BlockId::dust(0)),
            (IVec3::new(1, 0, 0), BlockId::dust(0)),
            (IVec3::new(0, 0, 1), BlockId::lever(Facing::Down, false)),
        ]);
        let arms = dust_arms(&w, IVec3::ZERO);
        // North, South, West, East.
        assert_eq!(arms, [None, Some(0), None, Some(0)]);
        assert_eq!(dust_links(&w, IVec3::ZERO).count(), 1);
    }

    #[test]
    fn a_dust_line_end_points_straight_on() {
        let w = world(&[(IVec3::new(0, 0, 0), BlockId::dust(0)), (IVec3::new(-1, 0, 0), BlockId::dust(0))]);
        let arms = dust_arms(&w, IVec3::ZERO);
        assert_eq!(arms, [None, None, Some(0), None]);
        assert_eq!(effective_arms(&arms), [false, false, true, true]);
        // A corner keeps its two real arms only.
        let corner = world(&[
            (IVec3::new(0, 0, 0), BlockId::dust(0)),
            (IVec3::new(-1, 0, 0), BlockId::dust(0)),
            (IVec3::new(0, 0, 1), BlockId::dust(0)),
        ]);
        assert_eq!(effective_arms(&dust_arms(&corner, IVec3::ZERO)), [false, true, true, false]);
    }

    #[test]
    fn dust_climbs_steps_unless_the_ceiling_is_solid() {
        let step = [
            (IVec3::new(0, 0, 0), BlockId::dust(0)),
            (IVec3::new(1, 0, 0), BlockId::STONE),
            (IVec3::new(1, 1, 0), BlockId::dust(0)),
        ];
        assert_eq!(dust_arms(world(&step), IVec3::ZERO)[3], Some(1));
        let mut blocked = step.to_vec();
        blocked.push((IVec3::new(0, 1, 0), BlockId::STONE));
        assert_eq!(dust_arms(world(&blocked), IVec3::ZERO)[3], None);
    }

    #[test]
    fn dust_steps_down_over_an_open_edge() {
        let w = world(&[(IVec3::new(0, 0, 0), BlockId::dust(0)), (IVec3::new(1, -1, 0), BlockId::dust(0))]);
        assert_eq!(dust_arms(&w, IVec3::ZERO)[3], Some(-1));
        let w = world(&[
            (IVec3::new(0, 0, 0), BlockId::dust(0)),
            (IVec3::new(1, 0, 0), BlockId::STONE),
            (IVec3::new(1, -1, 0), BlockId::dust(0)),
        ]);
        assert_eq!(dust_arms(&w, IVec3::ZERO)[3], None, "a solid block in between hides it");
    }

    #[test]
    fn support_rules() {
        let floor = world(&[(IVec3::new(0, -1, 0), BlockId::STONE)]);
        assert!(has_support(&floor, IVec3::ZERO, BlockId::dust(3)));
        assert!(has_support(&floor, IVec3::ZERO, BlockId::torch(Facing::Down, true)));
        assert!(!has_support(&floor, IVec3::ZERO, BlockId::torch(Facing::North, true)));
        assert!(has_support(&floor, IVec3::ZERO, BlockId::PISTON));
        let none = world(&[]);
        assert!(!has_support(&none, IVec3::ZERO, BlockId::STONE_PLATE));
        let wall = world(&[(IVec3::new(0, 0, -1), BlockId::STONE)]);
        assert!(has_support(&wall, IVec3::ZERO, BlockId::lever(Facing::North, true)));
    }
}
