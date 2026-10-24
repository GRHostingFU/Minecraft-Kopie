//! How fluids act on bodies: whether a body is submerged and which way the water pushes it.

use super::sim::FluidKind;
use crate::world::BlockQuery;
use crate::world::block::{FALLING, Facing};
use bevy::math::{IVec3, Vec3};

/// Surface height of a fluid cell as a fraction of a block (sources are slightly lower than
/// full so a lake has a visible rim; falling fluid fills the whole block).
pub fn fluid_height(state: u8) -> f32 {
    match state {
        0 => 8.0 / 9.0,
        FALLING => 1.0,
        l => (8 - l.min(7)) as f32 / 9.0,
    }
}

/// Fluid "head" in eighths: sources and falling columns are 8, flowing fluid loses one per step.
fn units(state: u8) -> i32 {
    if state == 0 || state == FALLING { 8 } else { 8 - state as i32 }
}

/// Height of the fluid surface inside the cell at `cell` (1.0 if more of the same fluid is above).
pub fn surface_in_cell<W: BlockQuery>(w: &W, cell: IVec3) -> Option<(FluidKind, f32)> {
    let (kind, state) = w.block(cell).and_then(FluidKind::of)?;
    let full = w.block(cell + IVec3::Y).is_some_and(|b| kind.is_same(b));
    Some((kind, if full { 1.0 } else { fluid_height(state) }))
}

/// Direction (horizontal, plus down for falling columns) in which the fluid in `cell` flows;
/// longer when the surface drops more steeply.
pub fn flow_at<W: BlockQuery>(w: &W, cell: IVec3) -> Vec3 {
    let Some((kind, state)) = w.block(cell).and_then(FluidKind::of) else { return Vec3::ZERO };
    let own = units(state);
    let mut flow = Vec3::ZERO;
    for f in Facing::HORIZONTAL {
        let n = cell + f.offset();
        let Some(nb) = w.block(n) else { continue };
        let other = match FluidKind::of(nb) {
            Some((k, s)) if k == kind => units(s),
            _ if nb.is_solid() => continue,
            // An open neighbour: if the fluid continues below it, it drains there.
            _ => match w.block(n - IVec3::Y).and_then(FluidKind::of) {
                Some((k, s)) if k == kind => (units(s) - 1).max(0),
                _ => 0,
            },
        };
        flow += f.offset().as_vec3() * (own - other) as f32;
    }
    if state == FALLING {
        flow.y -= 8.0;
    }
    flow
}

/// What a body of a given size feels at a position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FluidContact {
    pub water: bool,
    pub lava: bool,
    /// Unit vector the fluids push towards (zero in still fluid).
    pub flow: Vec3,
}

impl FluidContact {
    pub fn any(&self) -> bool {
        self.water || self.lava
    }
}

/// Fluids touching the lower 60 % of a body whose feet are at `pos`.
pub fn body_contact<W: BlockQuery>(w: &W, pos: Vec3, half_width: f32, height: f32) -> FluidContact {
    const SKIN: f32 = 1e-4;
    let mut out = FluidContact::default();
    let mut flow = Vec3::ZERO;
    let top = pos.y + height * 0.6;
    for y in (pos.y.floor() as i32)..=((top - SKIN).floor() as i32) {
        for z in ((pos.z - half_width).floor() as i32)..=((pos.z + half_width - SKIN).floor() as i32) {
            for x in ((pos.x - half_width).floor() as i32)..=((pos.x + half_width - SKIN).floor() as i32) {
                let cell = IVec3::new(x, y, z);
                let Some((kind, surface)) = surface_in_cell(w, cell) else { continue };
                // Only the part of the cell below the fluid surface touches the body.
                if y as f32 + surface <= pos.y {
                    continue;
                }
                match kind {
                    FluidKind::Water => out.water = true,
                    FluidKind::Lava => out.lava = true,
                }
                flow += flow_at(w, cell);
            }
        }
    }
    out.flow = flow.normalize_or_zero();
    out
}

#[cfg(test)]
mod tests {
    use super::super::sim::tests::Map;
    use super::*;
    use crate::world::block::BlockId;

    #[test]
    fn flow_points_downhill_across_levels() {
        let mut w = Map::flat();
        w.put(IVec3::new(0, 1, 0), BlockId::WATER);
        w.put(IVec3::new(1, 1, 0), BlockId::water(1));
        w.put(IVec3::new(2, 1, 0), BlockId::water(2));
        let f = flow_at(&w, IVec3::new(1, 1, 0));
        // Between a higher west neighbour and a lower east neighbour the fluid flows east.
        assert!(f.x > 0.0 && f.z == 0.0, "{f}");
    }

    #[test]
    fn falling_water_pushes_down() {
        let mut w = Map::flat();
        w.put(IVec3::new(0, 2, 0), BlockId::water(FALLING));
        assert!(flow_at(&w, IVec3::new(0, 2, 0)).y < 0.0);
    }

    #[test]
    fn contact_respects_the_surface_height() {
        let mut w = Map::flat();
        w.put(IVec3::new(0, 1, 0), BlockId::water(7)); // surface at 1 + 1/9
        let in_it = body_contact(&w, Vec3::new(0.5, 1.0, 0.5), 0.3, 1.8);
        assert!(in_it.water && !in_it.lava);
        let above = body_contact(&w, Vec3::new(0.5, 1.2, 0.5), 0.3, 1.8);
        assert!(!above.any(), "feet above the surface are dry");
    }

    #[test]
    fn contact_reports_lava_and_flow_direction() {
        let mut w = Map::flat();
        w.put(IVec3::new(0, 1, 0), BlockId::lava(0));
        w.put(IVec3::new(1, 1, 0), BlockId::lava(2));
        w.put(IVec3::new(-1, 1, 0), BlockId::STONE);
        let c = body_contact(&w, Vec3::new(0.5, 1.0, 0.5), 0.3, 1.8);
        assert!(c.lava && !c.water);
        assert!(c.flow.x > 0.5, "{:?}", c.flow);
    }
}
