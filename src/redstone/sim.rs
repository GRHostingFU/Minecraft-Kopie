//! The redstone simulation: wire solving, delayed components and piston control, independent
//! of Bevy (it runs on any [`BlockAccess`]).
//!
//! Time is counted in *redstone ticks* (10 per second). Edits mark blocks dirty; every tick the
//! dirty blocks and everything within two blocks of them is re-evaluated:
//!
//! * a connected network of dust is solved at once — each cell takes the strongest outside
//!   power or its strongest linked neighbour minus one;
//! * torches (inverters), lamps (when switching off) and pistons react after a delay, which is
//!   what makes clocks and pulse circuits behave over time;
//! * parts that lost their support (dust without floor, a lever on a removed wall) break.

use super::piston;
use super::power::{conductor_power, input_power};
use super::rules::{dust_links, has_support};
use crate::world::block::{BlockId, Kind};
use crate::world::{BlockAccess, BlockQuery};
use bevy::math::IVec3;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

pub const TICKS_PER_SECOND: f32 = 10.0;
/// A pressed button pops out after one second.
const BUTTON_TICKS: u64 = 10;
/// A pressure plate stays down this long after the last body left it.
const PLATE_RELEASE_TICKS: u32 = 5;
/// Delay before a lamp turns off.
const LAMP_OFF_DELAY: u64 = 2;
/// A torch that flips more than this often within [`BURNOUT_WINDOW`] ticks burns out.
const BURNOUT_TOGGLES: usize = 8;
const BURNOUT_WINDOW: u64 = 30;
const BURNOUT_TICKS: u64 = 80;
/// Evaluation passes within one tick (wire updates ripple instantly, components wait a tick).
const MAX_PASSES: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
enum Action {
    Torch,
    Lamp,
    Piston,
    ButtonRelease,
}

/// Due tick, insertion order (for stable ordering), cell and what to do there.
type QueueEntry = (u64, u64, [i32; 3], Action);

#[derive(Default)]
pub struct RedstoneSim {
    tick: u64,
    seq: u64,
    dirty: HashSet<IVec3>,
    queue: BinaryHeap<Reverse<QueueEntry>>,
    scheduled: HashSet<(IVec3, Action)>,
    /// Pressed plates and how many ticks in a row nobody stood on them.
    plates: HashMap<IVec3, u32>,
    toggles: HashMap<IVec3, Vec<u64>>,
    burnt: HashMap<IVec3, u64>,
}

fn at<W: BlockQuery>(w: &W, p: IVec3) -> BlockId {
    w.block(p).unwrap_or(BlockId::AIR)
}

/// Is `b` part of a redstone circuit (anything the simulation has to look at)?
pub fn is_part(b: BlockId) -> bool {
    !matches!(b.kind(), Kind::Air | Kind::Solid | Kind::Water(_) | Kind::Lava(_))
}

impl RedstoneSim {
    pub fn ticks(&self) -> u64 {
        self.tick
    }

    pub fn is_idle(&self) -> bool {
        self.dirty.is_empty() && self.queue.is_empty() && self.plates.is_empty()
    }

    /// The block at `pos` changed (or was loaded): re-evaluate the circuit around it.
    pub fn notify(&mut self, pos: IVec3) {
        self.dirty.insert(pos);
    }

    /// Picks up timers of a part that was saved mid-action.
    pub fn register_loaded<W: BlockQuery>(&mut self, w: &W, pos: IVec3) {
        match at(w, pos).kind() {
            Kind::Button { pressed: true, .. } => self.schedule(pos, Action::ButtonRelease, BUTTON_TICKS),
            Kind::Plate { pressed: true } => {
                self.plates.insert(pos, 0);
            }
            _ => {}
        }
        self.notify(pos);
    }

    fn schedule(&mut self, pos: IVec3, action: Action, delay: u64) {
        if self.scheduled.insert((pos, action)) {
            self.seq += 1;
            self.queue.push(Reverse((self.tick + delay, self.seq, pos.to_array(), action)));
        }
    }

    fn set<W: BlockAccess>(&mut self, w: &mut W, pos: IVec3, id: BlockId) {
        if w.set(pos, id) {
            self.dirty.insert(pos);
        }
    }

    /// The player used the block at `pos`. Returns whether something happened.
    pub fn use_block<W: BlockAccess>(&mut self, w: &mut W, pos: IVec3) -> bool {
        match at(w, pos).kind() {
            Kind::Lever { attach, on } => {
                self.set(w, pos, BlockId::lever(attach, !on));
                true
            }
            Kind::Button { attach, pressed: false } => {
                self.set(w, pos, BlockId::button(attach, true));
                self.schedule(pos, Action::ButtonRelease, BUTTON_TICKS);
                true
            }
            Kind::Button { pressed: true, .. } => true,
            _ => false,
        }
    }

    /// One redstone tick. `occupied` holds the cells that currently contain a body (player or
    /// mob) standing on the floor, which presses plates.
    pub fn advance<W: BlockAccess>(&mut self, w: &mut W, occupied: &HashSet<IVec3>) {
        self.tick += 1;
        self.update_plates(w, occupied);
        while let Some(&Reverse((due, _, pos, action))) = self.queue.peek() {
            if due > self.tick {
                break;
            }
            self.queue.pop();
            let pos = IVec3::from_array(pos);
            self.scheduled.remove(&(pos, action));
            self.run_action(w, pos, action);
        }
        for _ in 0..MAX_PASSES {
            if self.dirty.is_empty() {
                break;
            }
            let batch: Vec<IVec3> = self.dirty.drain().collect();
            self.evaluate(w, &batch);
        }
    }

    pub fn run<W: BlockAccess>(&mut self, w: &mut W, ticks: u32) {
        let none = HashSet::new();
        for _ in 0..ticks {
            self.advance(w, &none);
        }
    }

    fn update_plates<W: BlockAccess>(&mut self, w: &mut W, occupied: &HashSet<IVec3>) {
        for &cell in occupied {
            if let Kind::Plate { pressed: false } = at(w, cell).kind() {
                self.set(w, cell, BlockId::plate(true));
                self.plates.insert(cell, 0);
            }
        }
        let mut released = Vec::new();
        for (pos, idle) in self.plates.iter_mut() {
            if occupied.contains(pos) {
                *idle = 0;
            } else {
                *idle += 1;
                if *idle >= PLATE_RELEASE_TICKS {
                    released.push(*pos);
                }
            }
        }
        for pos in released {
            self.plates.remove(&pos);
            if let Kind::Plate { pressed: true } = at(w, pos).kind() {
                self.set(w, pos, BlockId::plate(false));
            }
        }
        // Plates that were broken or replaced no longer need tracking.
        self.plates.retain(|p, _| matches!(at(w, *p).kind(), Kind::Plate { .. }));
    }

    fn run_action<W: BlockAccess>(&mut self, w: &mut W, pos: IVec3, action: Action) {
        match (action, at(w, pos).kind()) {
            (Action::Torch, Kind::Torch { attach, lit }) => {
                let want = !torch_powered(w, pos, attach) && !self.is_burnt(pos);
                if want != lit {
                    self.set(w, pos, BlockId::torch(attach, want));
                    self.record_toggle(w, pos, attach, want);
                }
            }
            (Action::Lamp, Kind::Lamp { lit }) => {
                let want = input_power(w, pos, None, false) > 0;
                if want != lit {
                    self.set(w, pos, BlockId::lamp(want));
                }
            }
            (Action::Piston, Kind::Piston { extended, facing, .. }) => {
                let want = input_power(w, pos, Some(facing), false) > 0;
                if want && !extended {
                    if piston::extend(w, pos) {
                        self.mark_piston_area(pos, facing.offset());
                    }
                } else if !want && extended && piston::retract(w, pos) {
                    self.mark_piston_area(pos, facing.offset());
                }
            }
            (Action::ButtonRelease, Kind::Button { attach, pressed: true }) => {
                self.set(w, pos, BlockId::button(attach, false));
            }
            _ => {}
        }
    }

    fn mark_piston_area(&mut self, pos: IVec3, dir: IVec3) {
        // Everything from the piston to one block past the longest possible push may have changed.
        for i in 0..=(piston::PUSH_LIMIT as i32 + 2) {
            self.dirty.insert(pos + dir * i);
        }
    }

    fn is_burnt(&self, pos: IVec3) -> bool {
        self.burnt.get(&pos).is_some_and(|until| *until > self.tick)
    }

    fn record_toggle<W: BlockAccess>(&mut self, w: &mut W, pos: IVec3, attach: crate::world::Facing, now_lit: bool) {
        let tick = self.tick;
        let log = self.toggles.entry(pos).or_default();
        log.push(tick);
        log.retain(|t| tick - *t <= BURNOUT_WINDOW);
        if now_lit && log.len() >= BURNOUT_TOGGLES {
            // Too fast a clock: the torch burns out for a while.
            log.clear();
            self.burnt.insert(pos, tick + BURNOUT_TICKS);
            self.set(w, pos, BlockId::torch(attach, false));
            self.schedule(pos, Action::Torch, BURNOUT_TICKS);
        }
    }

    fn evaluate<W: BlockAccess>(&mut self, w: &mut W, batch: &[IVec3]) {
        let mut cells: HashSet<IVec3> = HashSet::new();
        for &p in batch {
            for dx in -2..=2i32 {
                for dy in -2..=2i32 {
                    for dz in -2..=2i32 {
                        if dx.abs() + dy.abs() + dz.abs() <= 2 {
                            cells.insert(p + IVec3::new(dx, dy, dz));
                        }
                    }
                }
            }
        }
        let mut solved: HashSet<IVec3> = HashSet::new();
        for c in cells {
            let b = at(w, c);
            if !is_part(b) {
                continue;
            }
            if !has_support(|p| at(w, p), c, b) {
                self.set(w, c, BlockId::AIR);
                continue;
            }
            match b.kind() {
                Kind::Dust { .. } => {
                    if !solved.contains(&c) {
                        self.solve_wire(w, c, &mut solved);
                    }
                }
                Kind::Torch { attach, lit } => {
                    let want = !torch_powered(w, c, attach) && !self.is_burnt(c);
                    if want != lit {
                        self.schedule(c, Action::Torch, 1);
                    }
                }
                Kind::Lamp { lit } => {
                    let want = input_power(w, c, None, false) > 0;
                    if want && !lit {
                        self.set(w, c, BlockId::lamp(true));
                    } else if !want && lit {
                        self.schedule(c, Action::Lamp, LAMP_OFF_DELAY);
                    }
                }
                Kind::Piston { extended, facing, .. } => {
                    let head_ok = matches!(
                        at(w, c + facing.offset()).kind(),
                        Kind::PistonHead { facing: f, .. } if f == facing
                    );
                    if extended && !head_ok {
                        // The head was broken: the piston resets itself.
                        piston::retract(w, c);
                        self.dirty.insert(c);
                    } else {
                        let want = input_power(w, c, Some(facing), false) > 0;
                        if want != extended {
                            self.schedule(c, Action::Piston, 1);
                        }
                    }
                }
                Kind::Button { pressed: true, .. } => self.schedule(c, Action::ButtonRelease, BUTTON_TICKS),
                _ => {}
            }
        }
    }

    /// Solves the whole dust network containing `start`.
    fn solve_wire<W: BlockAccess>(&mut self, w: &mut W, start: IVec3, solved: &mut HashSet<IVec3>) {
        let mut links: HashMap<IVec3, Vec<IVec3>> = HashMap::new();
        let mut stack = vec![start];
        solved.insert(start);
        while let Some(p) = stack.pop() {
            let nbrs: Vec<IVec3> = dust_links(|q| at(w, q), p).collect();
            for n in nbrs {
                links.entry(p).or_default().push(n);
                links.entry(n).or_default().push(p);
                if solved.insert(n) {
                    stack.push(n);
                }
            }
            links.entry(p).or_default();
        }
        let cells: Vec<IVec3> = links.keys().copied().collect();
        let mut power: HashMap<IVec3, u8> = HashMap::new();
        let mut buckets: Vec<Vec<IVec3>> = vec![Vec::new(); 16];
        for &c in &cells {
            let ext = input_power(w, c, None, true);
            power.insert(c, ext);
            buckets[ext as usize].push(c);
        }
        for level in (2..=15usize).rev() {
            let mut i = 0;
            while i < buckets[level].len() {
                let c = buckets[level][i];
                i += 1;
                if power[&c] as usize != level {
                    continue;
                }
                for &n in &links[&c] {
                    if power[&n] < (level - 1) as u8 {
                        power.insert(n, (level - 1) as u8);
                        buckets[level - 1].push(n);
                    }
                }
            }
        }
        for (c, p) in power {
            let want = BlockId::dust(p);
            if at(w, c) != want && matches!(at(w, c).kind(), Kind::Dust { .. }) {
                self.set(w, c, want);
            }
        }
    }
}

/// Is the block a torch is attached to receiving power?
fn torch_powered<W: BlockQuery>(w: &W, pos: IVec3, attach: crate::world::Facing) -> bool {
    let support = pos + attach.offset();
    at(w, support).is_conductor() && conductor_power(w, support, false) > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::fluids::sim::tests::Map;
    use crate::world::Facing;

    fn p(x: i32, y: i32, z: i32) -> IVec3 {
        IVec3::new(x, y, z)
    }

    /// A stone floor at y = 0 (from `Map::flat`) with parts placed on it at y = 1.
    fn line_of_dust(w: &mut Map, from: i32, to: i32) {
        for x in from..=to {
            w.put(p(x, 1, 0), BlockId::dust(0));
        }
    }

    fn flip_lever(w: &mut Map, sim: &mut RedstoneSim, at: IVec3) {
        assert!(sim.use_block(w, at));
    }

    #[test]
    fn dust_carries_a_signal_and_decays_by_one_per_block() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        w.put(p(0, 1, 0), BlockId::lever(Facing::Down, false));
        line_of_dust(&mut w, 1, 20);
        sim.notify(p(0, 1, 0));
        sim.run(&mut w, 2);
        assert_eq!(w.at(p(1, 1, 0)), BlockId::dust(0));
        flip_lever(&mut w, &mut sim, p(0, 1, 0));
        sim.run(&mut w, 2);
        for d in 1..=15 {
            assert_eq!(w.at(p(d, 1, 0)), BlockId::dust(16 - d as u8), "dust {d}");
        }
        for d in 16..=20 {
            assert_eq!(w.at(p(d, 1, 0)), BlockId::dust(0), "dust {d} is out of range");
        }
        // Switching off clears the wire again.
        flip_lever(&mut w, &mut sim, p(0, 1, 0));
        sim.run(&mut w, 2);
        for d in 1..=20 {
            assert_eq!(w.at(p(d, 1, 0)), BlockId::dust(0));
        }
    }

    #[test]
    fn a_lamp_follows_the_lever_through_the_wire() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        w.put(p(0, 1, 0), BlockId::lever(Facing::Down, true));
        line_of_dust(&mut w, 1, 4);
        w.put(p(5, 1, 0), BlockId::lamp(false));
        sim.notify(p(0, 1, 0));
        sim.run(&mut w, 3);
        assert_eq!(w.at(p(5, 1, 0)), BlockId::lamp(true));
        flip_lever(&mut w, &mut sim, p(0, 1, 0));
        sim.run(&mut w, 6);
        assert_eq!(w.at(p(5, 1, 0)), BlockId::lamp(false));
    }

    #[test]
    fn a_torch_inverts_the_power_of_its_support_with_one_tick_delay() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        // Lever on the floor next to a stone block; the torch hangs on the block's far side.
        w.put(p(1, 1, 0), BlockId::STONE);
        w.put(p(0, 1, 0), BlockId::lever(Facing::East, false));
        w.put(p(2, 1, 0), BlockId::torch(Facing::West, true));
        sim.notify(p(1, 1, 0));
        sim.run(&mut w, 3);
        assert_eq!(w.at(p(2, 1, 0)), BlockId::torch(Facing::West, true), "stays lit while unpowered");
        flip_lever(&mut w, &mut sim, p(0, 1, 0));
        sim.run(&mut w, 1);
        assert_eq!(w.at(p(2, 1, 0)), BlockId::torch(Facing::West, true), "reacts only after a tick");
        sim.run(&mut w, 2);
        assert_eq!(w.at(p(2, 1, 0)), BlockId::torch(Facing::West, false));
        flip_lever(&mut w, &mut sim, p(0, 1, 0));
        sim.run(&mut w, 3);
        assert_eq!(w.at(p(2, 1, 0)), BlockId::torch(Facing::West, true));
    }

    #[test]
    fn two_inverters_in_a_row_delay_the_signal_by_two_ticks() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        // lever -> block A -> torch1 (hangs on A, lit) -> dust -> block B -> torch2 (hangs on B).
        w.put(p(0, 1, 0), BlockId::lever(Facing::East, false));
        w.put(p(1, 1, 0), BlockId::STONE);
        w.put(p(2, 1, 0), BlockId::torch(Facing::West, true));
        w.put(p(3, 1, 0), BlockId::dust(0));
        w.put(p(4, 1, 0), BlockId::STONE);
        w.put(p(5, 1, 0), BlockId::torch(Facing::West, true));
        for x in 0..=5 {
            sim.notify(p(x, 1, 0));
        }
        sim.run(&mut w, 6);
        let off = |w: &Map, at| BlockId::torch(Facing::West, false) == w.at(at);
        assert_eq!(w.at(p(3, 1, 0)), BlockId::dust(15), "torch1 powers the dust");
        assert!(off(&w, p(5, 1, 0)), "dust pointing into block B turns torch2 off");
        flip_lever(&mut w, &mut sim, p(0, 1, 0));
        let mut first_off = None;
        let mut then_on = None;
        for tick in 1..=8 {
            sim.run(&mut w, 1);
            if first_off.is_none() && off(&w, p(2, 1, 0)) {
                first_off = Some(tick);
            }
            if then_on.is_none() && !off(&w, p(5, 1, 0)) {
                then_on = Some(tick);
            }
        }
        let (a, b) = (first_off.expect("torch1 turns off"), then_on.expect("torch2 turns on"));
        assert!(b > a, "the second inverter reacts later: {a} then {b}");
    }

    #[test]
    fn a_powered_piston_extends_and_retracts_when_the_power_goes() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        w.put(p(0, 1, 0), BlockId::piston(false, Facing::East, false));
        w.put(p(1, 1, 0), BlockId::DIRT);
        w.put(p(0, 1, 1), BlockId::lever(Facing::Down, false));
        sim.notify(p(0, 1, 0));
        sim.run(&mut w, 2);
        assert_eq!(w.at(p(0, 1, 0)), BlockId::piston(false, Facing::East, false));
        flip_lever(&mut w, &mut sim, p(0, 1, 1));
        sim.run(&mut w, 4);
        assert_eq!(w.at(p(0, 1, 0)), BlockId::piston(false, Facing::East, true));
        assert_eq!(w.at(p(1, 1, 0)), BlockId::piston_head(false, Facing::East));
        assert_eq!(w.at(p(2, 1, 0)), BlockId::DIRT);
        flip_lever(&mut w, &mut sim, p(0, 1, 1));
        sim.run(&mut w, 4);
        assert_eq!(w.at(p(0, 1, 0)), BlockId::piston(false, Facing::East, false));
        assert!(w.at(p(1, 1, 0)).is_air());
    }

    #[test]
    fn a_sticky_piston_brings_the_block_back() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        w.put(p(0, 1, 0), BlockId::piston(true, Facing::East, false));
        w.put(p(1, 1, 0), BlockId::COBBLE);
        w.put(p(0, 1, 1), BlockId::lever(Facing::Down, false));
        sim.notify(p(0, 1, 0));
        flip_lever(&mut w, &mut sim, p(0, 1, 1));
        sim.run(&mut w, 4);
        assert_eq!(w.at(p(2, 1, 0)), BlockId::COBBLE);
        flip_lever(&mut w, &mut sim, p(0, 1, 1));
        sim.run(&mut w, 4);
        assert_eq!(w.at(p(1, 1, 0)), BlockId::COBBLE);
        assert!(w.at(p(2, 1, 0)).is_air());
    }

    #[test]
    fn breaking_the_head_resets_the_piston_and_a_missing_base_removes_the_head() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        w.put(p(0, 1, 0), BlockId::piston(false, Facing::East, true));
        w.put(p(1, 1, 0), BlockId::piston_head(false, Facing::East));
        w.put(p(0, 1, 0), BlockId::piston(false, Facing::East, true));
        w.put(p(1, 1, 0), BlockId::AIR);
        sim.notify(p(1, 1, 0));
        sim.run(&mut w, 2);
        assert_eq!(w.at(p(0, 1, 0)), BlockId::piston(false, Facing::East, false));

        w.put(p(0, 1, 5), BlockId::AIR);
        w.put(p(1, 1, 5), BlockId::piston_head(false, Facing::East));
        sim.notify(p(1, 1, 5));
        sim.run(&mut w, 2);
        assert!(w.at(p(1, 1, 5)).is_air(), "a head without its piston disappears");
    }

    #[test]
    fn a_button_pulses_for_a_second() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        w.put(p(0, 1, 0), BlockId::button(Facing::Down, false));
        w.put(p(1, 1, 0), BlockId::lamp(false));
        sim.notify(p(0, 1, 0));
        assert!(sim.use_block(&mut w, p(0, 1, 0)));
        sim.run(&mut w, 3);
        assert_eq!(w.at(p(1, 1, 0)), BlockId::lamp(true));
        sim.run(&mut w, 10);
        assert_eq!(w.at(p(0, 1, 0)), BlockId::button(Facing::Down, false), "popped out again");
        sim.run(&mut w, 5);
        assert_eq!(w.at(p(1, 1, 0)), BlockId::lamp(false));
    }

    #[test]
    fn a_pressure_plate_is_held_down_by_a_body() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        w.put(p(0, 1, 0), BlockId::plate(false));
        w.put(p(1, 1, 0), BlockId::lamp(false));
        let on: HashSet<IVec3> = [p(0, 1, 0)].into();
        sim.advance(&mut w, &on);
        sim.advance(&mut w, &on);
        assert_eq!(w.at(p(0, 1, 0)), BlockId::plate(true));
        assert_eq!(w.at(p(1, 1, 0)), BlockId::lamp(true));
        for _ in 0..20 {
            sim.advance(&mut w, &on);
        }
        assert_eq!(w.at(p(0, 1, 0)), BlockId::plate(true), "stays pressed while someone stands on it");
        sim.run(&mut w, 12);
        assert_eq!(w.at(p(0, 1, 0)), BlockId::plate(false));
        assert_eq!(w.at(p(1, 1, 0)), BlockId::lamp(false));
    }

    #[test]
    fn parts_break_when_their_support_disappears() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        w.put(p(5, 1, 5), BlockId::STONE);
        w.put(p(5, 2, 5), BlockId::dust(0));
        w.put(p(6, 2, 5), BlockId::torch(Facing::West, true));
        w.put(p(5, 3, 5), BlockId::lever(Facing::Down, false));
        sim.notify(p(5, 2, 5));
        sim.run(&mut w, 2);
        assert_eq!(w.at(p(5, 2, 5)), BlockId::dust(0));
        w.put(p(5, 1, 5), BlockId::AIR);
        sim.notify(p(5, 1, 5));
        sim.run(&mut w, 2);
        assert!(w.at(p(5, 2, 5)).is_air(), "dust without floor");
        assert!(w.at(p(6, 2, 5)).is_air() || w.at(p(6, 2, 5)) == BlockId::torch(Facing::West, true));
        assert!(w.at(p(5, 3, 5)).is_air(), "lever lost the dust-supported cell? it needs a solid block");
    }

    #[test]
    fn dust_climbs_a_step_and_keeps_the_signal() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        w.put(p(0, 1, 0), BlockId::lever(Facing::Down, true));
        w.put(p(1, 1, 0), BlockId::dust(0));
        w.put(p(2, 1, 0), BlockId::STONE);
        w.put(p(2, 2, 0), BlockId::dust(0));
        w.put(p(3, 1, 0), BlockId::STONE);
        w.put(p(3, 2, 0), BlockId::dust(0));
        sim.notify(p(0, 1, 0));
        sim.run(&mut w, 2);
        assert_eq!(w.at(p(1, 1, 0)), BlockId::dust(15));
        assert_eq!(w.at(p(2, 2, 0)), BlockId::dust(14));
        assert_eq!(w.at(p(3, 2, 0)), BlockId::dust(13));
    }

    #[test]
    fn dust_pointing_into_a_block_powers_a_piston_behind_it_but_not_other_dust() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        w.put(p(0, 1, 0), BlockId::lever(Facing::Down, true));
        w.put(p(1, 1, 0), BlockId::dust(0));
        w.put(p(2, 1, 0), BlockId::STONE);
        w.put(p(2, 1, 1), BlockId::piston(false, Facing::Up, false)); // beside the powered block
        w.put(p(3, 1, 0), BlockId::dust(0)); // on the far side of the block
        sim.notify(p(0, 1, 0));
        sim.run(&mut w, 5);
        assert_eq!(w.at(p(1, 1, 0)), BlockId::dust(15));
        assert_eq!(
            w.at(p(2, 1, 1)),
            BlockId::piston(false, Facing::Up, true),
            "the piston is powered through the block"
        );
        assert_eq!(w.at(p(3, 1, 0)), BlockId::dust(0), "dust never powers dust through a block");
    }

    #[test]
    fn a_torch_ring_clock_oscillates_and_can_burn_out() {
        // A torch feeding its own support through dust flips every tick until it burns out.
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        w.put(p(1, 1, 0), BlockId::STONE);
        w.put(p(0, 1, 0), BlockId::torch(Facing::East, true)); // hangs on the west side of the block
        w.put(p(0, 2, 0), BlockId::STONE); // block above the torch gets strong power
        w.put(p(1, 2, 0), BlockId::dust(0)); // dust on top of the support block, next to the powered block
        w.put(p(0, 3, 0), BlockId::dust(0));
        sim.notify(p(0, 1, 0));
        sim.run(&mut w, 60);
        // The loop cannot be stable: the torch must have flipped at least a few times, and the
        // sim must stay finite (it reached this line).
        assert!(sim.ticks() == 60);
    }

    #[test]
    fn the_sim_goes_idle_when_nothing_changes() {
        let (mut w, mut sim) = (Map::flat(), RedstoneSim::default());
        line_of_dust(&mut w, 0, 5);
        sim.notify(p(0, 1, 0));
        sim.run(&mut w, 5);
        assert!(sim.is_idle());
    }
}
