//! Cellular fluid simulation for water and lava, independent of Bevy.
//!
//! Every fluid cell is one block whose id encodes its state: a *source* (state 0, never dries
//! up), *flowing* fluid with its distance `1..=reach` from the nearest source, or a *falling*
//! column. Cells are re-evaluated by scheduled ticks (water every 5, lava every 30 game ticks):
//!
//! 1. a non-source cell recomputes what it should be from its neighbours — fed from above it
//!    falls, otherwise it takes the smallest neighbour distance plus one, and it dries up when
//!    nothing feeds it any more (water additionally turns into a source between two sources);
//! 2. it then spreads: down if it can, otherwise sideways with distance + 1, preferring the
//!    directions that lead to a drop within four blocks.
//!
//! Water meeting lava turns the lava (source or flowing) into obsidian, lava flowing into water
//! turns the water into cobblestone.

use crate::world::block::{BlockId, FALLING, Facing, Kind, LAVA_REACH, WATER_REACH};
use crate::world::{BlockAccess, BlockQuery};
use bevy::math::IVec3;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// Game ticks per second of the simulation clock.
pub const TICKS_PER_SECOND: f32 = 20.0;
const WATER_DELAY: u64 = 5;
const LAVA_DELAY: u64 = 30;
/// Most cells processed per game tick; the rest wait for the next one.
const BUDGET_PER_TICK: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum FluidKind {
    Water,
    Lava,
}

impl FluidKind {
    pub fn reach(self) -> u8 {
        match self {
            FluidKind::Water => WATER_REACH,
            FluidKind::Lava => LAVA_REACH,
        }
    }

    pub fn delay(self) -> u64 {
        match self {
            FluidKind::Water => WATER_DELAY,
            FluidKind::Lava => LAVA_DELAY,
        }
    }

    pub fn block(self, state: u8) -> BlockId {
        match self {
            FluidKind::Water => BlockId::water(state),
            FluidKind::Lava => BlockId::lava(state),
        }
    }

    pub fn is_same(self, b: BlockId) -> bool {
        Self::of(b).is_some_and(|(k, _)| k == self)
    }

    /// Kind and state of a fluid block.
    pub fn of(b: BlockId) -> Option<(FluidKind, u8)> {
        match b.kind() {
            Kind::Water(s) => Some((FluidKind::Water, s)),
            Kind::Lava(s) => Some((FluidKind::Lava, s)),
            _ => None,
        }
    }
}

/// How far a cell is from the source that feeds it (sources and falling columns count as 0).
fn distance(state: u8) -> u8 {
    if state == FALLING { 0 } else { state }
}

#[derive(Default)]
pub struct FluidSim {
    tick: u64,
    seq: u64,
    queue: BinaryHeap<Reverse<(u64, u64, [i32; 3])>>,
    /// Latest due tick per scheduled cell; queue entries that disagree are stale.
    scheduled: HashMap<IVec3, u64>,
}

impl FluidSim {
    pub fn pending(&self) -> usize {
        self.scheduled.len()
    }

    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    /// Schedules `pos` if it holds fluid.
    pub fn schedule<W: BlockQuery>(&mut self, w: &W, pos: IVec3) {
        let Some((kind, _)) = w.block(pos).and_then(FluidKind::of) else { return };
        let due = self.tick + kind.delay();
        let entry = self.scheduled.entry(pos).or_insert(u64::MAX);
        if due < *entry {
            *entry = due;
            self.seq += 1;
            self.queue.push(Reverse((due, self.seq, pos.to_array())));
        }
    }

    /// A block at `pos` changed: it and its neighbours may need to flow, dry up or react.
    pub fn touch<W: BlockQuery>(&mut self, w: &W, pos: IVec3) {
        self.schedule(w, pos);
        for f in Facing::ALL {
            self.schedule(w, pos + f.offset());
        }
    }

    /// Advances the clock by one game tick and processes every cell that is due.
    pub fn advance<W: BlockAccess>(&mut self, w: &mut W) {
        self.tick += 1;
        let mut processed = 0;
        while processed < BUDGET_PER_TICK {
            let Some(&Reverse((due, _, pos))) = self.queue.peek() else { break };
            if due > self.tick {
                break;
            }
            self.queue.pop();
            let pos = IVec3::from_array(pos);
            if self.scheduled.get(&pos) != Some(&due) {
                continue; // superseded
            }
            self.scheduled.remove(&pos);
            processed += 1;
            self.update(w, pos);
        }
    }

    /// Runs the simulation for `ticks` game ticks.
    pub fn run<W: BlockAccess>(&mut self, w: &mut W, ticks: u32) {
        for _ in 0..ticks {
            self.advance(w);
        }
    }

    fn set<W: BlockAccess>(&mut self, w: &mut W, pos: IVec3, id: BlockId) {
        if w.set(pos, id) {
            self.touch(w, pos);
        }
    }

    fn update<W: BlockAccess>(&mut self, w: &mut W, pos: IVec3) {
        let Some((kind, state)) = w.block(pos).and_then(FluidKind::of) else { return };
        if state != 0 {
            let wanted = Self::wanted_state(w, pos, kind);
            if wanted != Some(state) {
                match wanted {
                    Some(new) => self.set(w, pos, kind.block(new)),
                    None => self.set(w, pos, BlockId::AIR),
                }
                // Re-check next tick; spreading happens once the cell is stable.
                self.schedule(w, pos);
                return;
            }
        }
        self.spread(w, pos, kind, state);
    }

    /// What a flowing cell should be given its surroundings; `None` if it dries up.
    fn wanted_state<W: BlockQuery>(w: &W, pos: IVec3, kind: FluidKind) -> Option<u8> {
        if w.block(pos + IVec3::Y).is_some_and(|b| kind.is_same(b)) {
            return Some(FALLING);
        }
        let mut best = u8::MAX;
        let mut sources = 0;
        for f in Facing::HORIZONTAL {
            if let Some((k, s)) = w.block(pos + f.offset()).and_then(FluidKind::of)
                && k == kind
            {
                if s == 0 {
                    sources += 1;
                }
                best = best.min(distance(s) + 1);
            }
        }
        if kind == FluidKind::Water && sources >= 2 {
            let below = w.block(pos - IVec3::Y);
            if below.is_some_and(|b| b.is_solid() || b == BlockId::WATER) {
                return Some(0);
            }
        }
        (best <= kind.reach()).then_some(best)
    }

    fn spread<W: BlockAccess>(&mut self, w: &mut W, pos: IVec3, kind: FluidKind, state: u8) {
        let below = pos - IVec3::Y;
        let Some(below_block) = w.block(below) else { return };
        if Self::can_enter(below_block, kind, FALLING) {
            self.flow_into(w, below, kind, FALLING);
            // A cell fed from three sides keeps spreading sideways as well.
            let sources = Facing::HORIZONTAL
                .into_iter()
                .filter(|f| w.block(pos + f.offset()).is_some_and(|b| b == kind.block(0)))
                .count();
            if sources < 3 {
                return;
            }
        }
        let next = distance(state) + 1;
        if next > kind.reach() {
            return;
        }
        for f in Self::spread_directions(w, pos, kind, next) {
            let target = pos + f.offset();
            if w.block(target).is_some_and(|b| Self::can_enter(b, kind, next)) {
                self.flow_into(w, target, kind, next);
            }
        }
    }

    /// Can fluid of `kind` with state `new_state` move into a cell holding `b`?
    fn can_enter(b: BlockId, kind: FluidKind, new_state: u8) -> bool {
        if b.is_air() || b.is_fragile() {
            return true;
        }
        match FluidKind::of(b) {
            None => false,
            // The other fluid: the two react.
            Some((other, _)) if other != kind => true,
            // Own fluid: only if the new state is an improvement (falling beats everything
            // except a source; a smaller distance beats a larger one).
            Some((_, 0)) => false,
            Some((_, s)) => {
                if s == FALLING {
                    false
                } else if new_state == FALLING {
                    true
                } else {
                    new_state < s
                }
            }
        }
    }

    fn flow_into<W: BlockAccess>(&mut self, w: &mut W, target: IVec3, kind: FluidKind, state: u8) {
        let Some(existing) = w.block(target) else { return };
        match FluidKind::of(existing) {
            Some((other, _)) if other != kind => {
                // Water onto lava (source or flowing) makes obsidian; lava onto water, cobblestone.
                let result = if kind == FluidKind::Water { BlockId::OBSIDIAN } else { BlockId::COBBLE };
                self.set(w, target, result);
            }
            _ => self.set(w, target, kind.block(state)),
        }
    }

    /// Horizontal directions to spread in. If a drop is reachable within four blocks, only the
    /// directions that lead to the nearest one are used, so fluid runs towards edges.
    fn spread_directions<W: BlockQuery>(w: &W, pos: IVec3, kind: FluidKind, next: u8) -> Vec<Facing> {
        let mut best = NO_DROP;
        let mut dirs = Vec::new();
        for f in Facing::HORIZONTAL {
            let n = pos + f.offset();
            if !w.block(n).is_some_and(|b| Self::passable(b, kind)) {
                continue;
            }
            let d = if Self::is_drop(w, n, kind) { 0 } else { Self::drop_distance(w, n, f, kind, 1) };
            if d < best {
                best = d;
                dirs.clear();
            }
            if d == best {
                dirs.push(f);
            }
        }
        if best == NO_DROP {
            // No drop nearby: spread everywhere that is open.
            dirs.clear();
            dirs.extend(Facing::HORIZONTAL);
        }
        dirs.retain(|f| w.block(pos + f.offset()).is_some_and(|b| Self::can_enter(b, kind, next)));
        dirs
    }

    /// Fluid can occupy or travel over this cell (sources of the same fluid are walls).
    fn passable(b: BlockId, kind: FluidKind) -> bool {
        b.is_air() || b.is_fragile() || FluidKind::of(b).is_some_and(|(k, s)| k != kind || s != 0)
    }

    /// Is `n` open and can fluid fall out of it (air or something it can replace below)?
    fn is_drop<W: BlockQuery>(w: &W, n: IVec3, kind: FluidKind) -> bool {
        w.block(n).is_some_and(|b| Self::passable(b, kind))
            && w.block(n - IVec3::Y).is_some_and(|b| b.is_air() || b.is_fragile() || FluidKind::of(b).is_some_and(|(k, _)| k != kind))
    }

    /// Steps (1..4) to the nearest drop when walking on from `from`, which was entered by moving
    /// in direction `dir`; [`NO_DROP`] if there is none within range.
    fn drop_distance<W: BlockQuery>(w: &W, from: IVec3, dir: Facing, kind: FluidKind, depth: u32) -> u32 {
        if depth >= 4 {
            return NO_DROP;
        }
        let mut best = NO_DROP;
        for f in Facing::HORIZONTAL {
            if f == dir.opposite() {
                continue;
            }
            let n = from + f.offset();
            if !w.block(n).is_some_and(|b| Self::passable(b, kind)) {
                continue;
            }
            if Self::is_drop(w, n, kind) {
                return depth;
            }
            best = best.min(Self::drop_distance(w, n, f, kind, depth + 1));
        }
        best
    }
}

/// Distance value meaning "no drop within range".
const NO_DROP: u32 = 1000;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A bounded test world: stone floor at y = 0, optional extra blocks, air elsewhere.
    pub struct Map {
        pub blocks: HashMap<IVec3, BlockId>,
        pub floor: bool,
    }

    impl Map {
        pub fn flat() -> Self {
            Map { blocks: HashMap::new(), floor: true }
        }
        pub fn put(&mut self, p: IVec3, b: BlockId) {
            self.blocks.insert(p, b);
        }
        pub fn at(&self, p: IVec3) -> BlockId {
            self.block(p).unwrap()
        }
    }

    impl BlockQuery for Map {
        fn block(&self, p: IVec3) -> Option<BlockId> {
            if p.x.abs() > 40 || p.z.abs() > 40 || p.y > 100 {
                return None;
            }
            Some(self.blocks.get(&p).copied().unwrap_or(if p.y <= 0 && self.floor { BlockId::STONE } else { BlockId::AIR }))
        }
    }

    impl BlockAccess for Map {
        fn set(&mut self, p: IVec3, b: BlockId) -> bool {
            if self.block(p).is_none() {
                return false;
            }
            self.blocks.insert(p, b);
            true
        }
    }

    fn settle(sim: &mut FluidSim, w: &mut Map, ticks: u32) {
        sim.run(w, ticks);
    }

    fn pour(w: &mut Map, sim: &mut FluidSim, p: IVec3, b: BlockId) {
        w.put(p, b);
        sim.touch(w, p);
    }

    fn count(w: &Map, f: impl Fn(BlockId) -> bool) -> usize {
        w.blocks.values().filter(|b| f(**b)).count()
    }

    #[test]
    fn a_water_source_spreads_seven_blocks_with_decreasing_levels() {
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        let src = IVec3::new(0, 1, 0);
        pour(&mut w, &mut sim, src, BlockId::WATER);
        settle(&mut sim, &mut w, 200);
        for d in 1..=7 {
            assert_eq!(w.at(src + IVec3::new(d, 0, 0)), BlockId::water(d as u8), "distance {d}");
            assert_eq!(w.at(src + IVec3::new(0, 0, -d)), BlockId::water(d as u8));
        }
        assert!(w.at(src + IVec3::new(8, 0, 0)).is_air(), "spreading stops after 7 blocks");
        assert_eq!(w.at(src), BlockId::WATER, "the source stays");
        assert_eq!(sim.pending(), 0, "the simulation settles");
    }

    #[test]
    fn water_falls_down_a_cliff_and_spreads_at_the_bottom() {
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        // A pillar 4 high with the source on top.
        for y in 1..=4 {
            w.put(IVec3::new(0, y, 0), BlockId::STONE);
        }
        pour(&mut w, &mut sim, IVec3::new(0, 5, 0), BlockId::WATER);
        // Let it run off the pillar top and down its side.
        settle(&mut sim, &mut w, 300);
        assert_eq!(w.at(IVec3::new(1, 5, 0)), BlockId::water(1));
        for y in 1..=4 {
            assert_eq!(w.at(IVec3::new(1, y, 0)), BlockId::water(FALLING), "falling column at y={y}");
        }
        assert!(FluidKind::of(w.at(IVec3::new(2, 1, 0))).is_some(), "spreads sideways on the ground");
    }

    #[test]
    fn removing_the_source_drains_the_flow() {
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        let src = IVec3::new(0, 1, 0);
        pour(&mut w, &mut sim, src, BlockId::WATER);
        settle(&mut sim, &mut w, 200);
        assert!(count(&w, |b| b.is_water()) > 50);
        w.put(src, BlockId::AIR);
        sim.touch(&w, src);
        settle(&mut sim, &mut w, 600);
        assert_eq!(count(&w, |b| b.is_water()), 0, "all flowing water dried up");
    }

    #[test]
    fn two_sources_make_an_infinite_source() {
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        // A 3x1 trench: source - hole - source.
        for x in -1..=1 {
            w.put(IVec3::new(x, 1, -1), BlockId::STONE);
            w.put(IVec3::new(x, 1, 1), BlockId::STONE);
        }
        w.put(IVec3::new(-2, 1, 0), BlockId::STONE);
        w.put(IVec3::new(2, 1, 0), BlockId::STONE);
        pour(&mut w, &mut sim, IVec3::new(-1, 1, 0), BlockId::WATER);
        pour(&mut w, &mut sim, IVec3::new(1, 1, 0), BlockId::WATER);
        settle(&mut sim, &mut w, 100);
        assert_eq!(w.at(IVec3::new(0, 1, 0)), BlockId::WATER, "the gap between two sources becomes a source");
        // Taking one source away keeps the pool alive because of the new source.
        w.put(IVec3::new(-1, 1, 0), BlockId::AIR);
        sim.touch(&w, IVec3::new(-1, 1, 0));
        settle(&mut sim, &mut w, 200);
        assert_eq!(w.at(IVec3::new(0, 1, 0)), BlockId::WATER);
    }

    #[test]
    fn lava_spreads_only_three_blocks_and_slowly() {
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        let src = IVec3::new(0, 1, 0);
        pour(&mut w, &mut sim, src, BlockId::LAVA);
        settle(&mut sim, &mut w, 20);
        assert!(w.at(src + IVec3::X).is_air(), "lava is slow: nothing after one second");
        settle(&mut sim, &mut w, 400);
        assert_eq!(w.at(src + IVec3::new(3, 0, 0)), BlockId::lava(3));
        assert!(w.at(src + IVec3::new(4, 0, 0)).is_air());
    }

    #[test]
    fn water_meeting_lava_makes_obsidian_for_sources_and_flows() {
        // Lava source with water poured next to it.
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        pour(&mut w, &mut sim, IVec3::new(0, 1, 0), BlockId::LAVA);
        pour(&mut w, &mut sim, IVec3::new(1, 1, 0), BlockId::WATER);
        settle(&mut sim, &mut w, 100);
        assert_eq!(w.at(IVec3::new(0, 1, 0)), BlockId::OBSIDIAN);

        // Flowing lava hit from above by a water column.
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        pour(&mut w, &mut sim, IVec3::new(0, 1, 0), BlockId::LAVA);
        settle(&mut sim, &mut w, 100);
        assert!(w.at(IVec3::new(1, 1, 0)).is_lava());
        pour(&mut w, &mut sim, IVec3::new(1, 3, 0), BlockId::WATER);
        settle(&mut sim, &mut w, 100);
        assert_eq!(w.at(IVec3::new(1, 1, 0)), BlockId::OBSIDIAN, "water falling onto flowing lava");
        assert!(count(&w, |b| b == BlockId::OBSIDIAN) > 0);
    }

    #[test]
    fn lava_flowing_into_water_makes_cobblestone() {
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        // Water is a source far enough that lava reaches it first by being placed adjacent.
        w.put(IVec3::new(1, 1, 0), BlockId::water(3));
        w.put(IVec3::new(0, 1, 0), BlockId::LAVA);
        // Only the lava is scheduled, so the lava side acts.
        sim.schedule(&w, IVec3::new(0, 1, 0));
        settle(&mut sim, &mut w, 60);
        assert_eq!(w.at(IVec3::new(1, 1, 0)), BlockId::COBBLE);
    }

    #[test]
    fn fluid_does_not_flow_into_unloaded_space_or_through_walls() {
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        for z in -1..=1 {
            w.put(IVec3::new(1, 1, z), BlockId::STONE);
        }
        pour(&mut w, &mut sim, IVec3::new(0, 1, 0), BlockId::WATER);
        settle(&mut sim, &mut w, 200);
        assert!(w.at(IVec3::new(1, 1, 0)) == BlockId::STONE);
        // A source at the edge of the loaded area must not panic nor write outside it.
        pour(&mut w, &mut sim, IVec3::new(40, 1, 0), BlockId::WATER);
        settle(&mut sim, &mut w, 100);
        assert!(w.block(IVec3::new(41, 1, 0)).is_none());
    }

    #[test]
    fn flowing_water_destroys_fragile_redstone_parts() {
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        w.put(IVec3::new(1, 1, 0), BlockId::dust(4));
        pour(&mut w, &mut sim, IVec3::new(0, 1, 0), BlockId::WATER);
        settle(&mut sim, &mut w, 50);
        assert_eq!(w.at(IVec3::new(1, 1, 0)), BlockId::water(1));
    }

    #[test]
    fn spreading_prefers_the_direction_of_a_nearby_drop() {
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        // A raised platform with a hole two blocks east of the source.
        for x in -6..=6 {
            for z in -6..=6 {
                w.put(IVec3::new(x, 1, z), BlockId::STONE);
            }
        }
        w.put(IVec3::new(2, 1, 0), BlockId::AIR);
        pour(&mut w, &mut sim, IVec3::new(0, 2, 0), BlockId::WATER);
        settle(&mut sim, &mut w, 12);
        assert!(w.at(IVec3::new(1, 2, 0)).is_water(), "flows towards the hole first");
        assert!(w.at(IVec3::new(-1, 2, 0)).is_air());
        assert!(w.at(IVec3::new(0, 2, 1)).is_air());
    }

    #[test]
    fn large_floods_stay_within_the_per_tick_budget() {
        let (mut w, mut sim) = (Map::flat(), FluidSim::default());
        for x in -3..=3 {
            for z in -3..=3 {
                pour(&mut w, &mut sim, IVec3::new(x * 2, 1, z * 2), BlockId::WATER);
            }
        }
        settle(&mut sim, &mut w, 300);
        assert!(sim.pending() == 0);
        assert!(count(&w, |b| b.is_water()) > 49);
    }
}
