//! A* path finding on the voxel grid for walking mobs.
//!
//! A *node* is the block cell a mob's feet occupy. A node is standable if the cell and the cells
//! above it up to the mob's height are free of solids and harmful fluids, and the cell below is
//! solid. From a node a mob can walk to the four neighbours (and diagonals if both side cells
//! are free), step up one block (a jump), or drop down up to [`MAX_DROP`] blocks.

use crate::world::block::BlockId;
use crate::world::{BlockQuery, Facing};
use bevy::math::{IVec3, Vec3};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

pub const MAX_DROP: i32 = 3;

#[derive(Clone, Copy, Debug)]
pub struct PathOptions {
    /// Cells of headroom the mob needs (2 for a creeper, 2 for a skeleton).
    pub height: i32,
    /// Search budget in expanded nodes.
    pub max_nodes: usize,
    /// Nodes farther than this (in blocks, horizontally) from the start are not explored.
    pub max_range: i32,
}

impl Default for PathOptions {
    fn default() -> Self {
        Self { height: 2, max_nodes: 2500, max_range: 48 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathResult {
    /// Cells from the first step to the end (the start cell is not included).
    pub cells: Vec<IVec3>,
    /// The last cell is the goal (or next to it); otherwise it is the closest cell found.
    pub reached: bool,
    pub expanded: usize,
}

fn passable_cell<W: BlockQuery>(w: &W, p: IVec3) -> bool {
    // Unloaded chunks are walls, lava and deep water are hazards.
    w.block(p).is_some_and(|b| !b.is_solid() && !b.is_lava() && !is_harmful(b))
}

fn is_harmful(b: BlockId) -> bool {
    // Mobs also avoid wading: water cells are not part of a ground route.
    b.is_water() || b.is_lava()
}

/// Can a mob of height `h` stand with its feet in `p`?
pub fn standable<W: BlockQuery>(w: &W, p: IVec3, h: i32) -> bool {
    if !(0..h).all(|dy| passable_cell(w, p + IVec3::new(0, dy, 0))) {
        return false;
    }
    w.block(p - IVec3::Y).is_some_and(|b| b.is_solid())
}

/// First standable cell at or below `p` (searching at most `depth` cells down), or above.
pub fn snap_to_ground<W: BlockQuery>(w: &W, p: IVec3, h: i32, depth: i32) -> Option<IVec3> {
    (0..=depth).map(|d| p - IVec3::new(0, d, 0)).find(|c| standable(w, *c, h)).or_else(|| {
        (1..=2).map(|d| p + IVec3::new(0, d, 0)).find(|c| standable(w, *c, h))
    })
}

struct Node {
    f: f32,
    g: f32,
    pos: IVec3,
}

impl PartialEq for Node {
    fn eq(&self, o: &Self) -> bool {
        self.f == o.f
    }
}
impl Eq for Node {}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        // Reversed so the BinaryHeap pops the smallest f; prefer larger g on ties (deeper nodes).
        o.f.partial_cmp(&self.f).unwrap_or(Ordering::Equal).then(self.g.partial_cmp(&o.g).unwrap_or(Ordering::Equal))
    }
}

fn heuristic(a: IVec3, b: IVec3) -> f32 {
    let (dx, dz) = ((a.x - b.x).abs() as f32, (a.z - b.z).abs() as f32);
    let (lo, hi) = if dx < dz { (dx, dz) } else { (dz, dx) };
    // Octile distance plus a little for climbing.
    hi + 0.414 * lo + 0.6 * (a.y - b.y).abs() as f32
}

/// Moves a mob can make from `p`: `(destination, cost)`.
fn neighbours<W: BlockQuery>(w: &W, p: IVec3, h: i32, out: &mut Vec<(IVec3, f32)>) {
    out.clear();
    for f in Facing::HORIZONTAL {
        let d = f.offset();
        let n = p + d;
        // Same level.
        if standable(w, n, h) {
            out.push((n, 1.0));
        } else {
            // Step up one block (needs headroom above the start for the jump).
            let up = n + IVec3::Y;
            if standable(w, up, h) && passable_cell(w, p + IVec3::new(0, h, 0)) && w.block(n).is_some_and(|b| b.is_solid()) {
                out.push((up, 1.6));
            } else if (0..h).all(|dy| passable_cell(w, n + IVec3::new(0, dy, 0))) {
                // Walk off an edge and fall up to MAX_DROP blocks.
                for k in 1..=MAX_DROP {
                    let below = n - IVec3::new(0, k, 0);
                    if !passable_cell(w, below) {
                        break;
                    }
                    if standable(w, below, h) {
                        out.push((below, 1.0 + 0.4 * k as f32));
                        break;
                    }
                }
            }
        }
    }
    // Diagonals on the same level, only if both orthogonal neighbours are free (no corner cutting).
    for (a, b) in [(Facing::North, Facing::East), (Facing::North, Facing::West), (Facing::South, Facing::East), (Facing::South, Facing::West)] {
        let (da, db) = (a.offset(), b.offset());
        let n = p + da + db;
        let free = |c: IVec3| (0..h).all(|dy| passable_cell(w, c + IVec3::new(0, dy, 0)));
        if standable(w, n, h) && free(p + da) && free(p + db) {
            out.push((n, 1.414));
        }
    }
}

/// Finds a route from `start` to `goal`. Both are snapped to the ground first. When the goal
/// cannot be reached within the budget the route to the explored cell closest to it is returned
/// with `reached == false`, so a chasing mob still makes progress.
pub fn find_path<W: BlockQuery>(w: &W, start: IVec3, goal: IVec3, opts: PathOptions) -> Option<PathResult> {
    let h = opts.height;
    let start = snap_to_ground(w, start, h, 2)?;
    let goal = snap_to_ground(w, goal, h, 6).unwrap_or(goal);
    if start == goal {
        return Some(PathResult { cells: Vec::new(), reached: true, expanded: 0 });
    }
    let mut open = BinaryHeap::new();
    let mut best: HashMap<IVec3, (f32, IVec3)> = HashMap::new(); // g, parent
    best.insert(start, (0.0, start));
    open.push(Node { f: heuristic(start, goal), g: 0.0, pos: start });
    let mut closest = (heuristic(start, goal), start);
    let mut expanded = 0;
    let mut buf = Vec::with_capacity(12);
    let mut found = None;
    while let Some(Node { g, pos, .. }) = open.pop() {
        if best.get(&pos).is_some_and(|(bg, _)| g > *bg + 1e-4) {
            continue; // stale entry
        }
        if pos == goal || (pos.x == goal.x && pos.z == goal.z && (pos.y - goal.y).abs() <= 1) {
            found = Some(pos);
            break;
        }
        expanded += 1;
        if expanded > opts.max_nodes {
            break;
        }
        let hh = heuristic(pos, goal);
        if hh < closest.0 {
            closest = (hh, pos);
        }
        neighbours(w, pos, h, &mut buf);
        for &(n, cost) in &buf {
            if (n.x - start.x).abs() > opts.max_range || (n.z - start.z).abs() > opts.max_range {
                continue;
            }
            let ng = g + cost;
            if best.get(&n).is_none_or(|(bg, _)| ng < *bg - 1e-4) {
                best.insert(n, (ng, pos));
                open.push(Node { f: ng + heuristic(n, goal), g: ng, pos: n });
            }
        }
    }
    let (end, reached) = match found {
        Some(p) => (p, true),
        None => (closest.1, false),
    };
    if end == start {
        return Some(PathResult { cells: Vec::new(), reached: false, expanded });
    }
    let mut cells = vec![end];
    let mut cur = end;
    while let Some((_, parent)) = best.get(&cur) {
        if *parent == cur || *parent == start {
            break;
        }
        cells.push(*parent);
        cur = *parent;
    }
    cells.reverse();
    Some(PathResult { cells: smooth(w, start, cells, h), reached, expanded })
}

/// Drops waypoints that lie on a straight, level, free line between their neighbours.
fn smooth<W: BlockQuery>(w: &W, start: IVec3, cells: Vec<IVec3>, h: i32) -> Vec<IVec3> {
    if cells.len() < 3 {
        return cells;
    }
    let mut out = Vec::with_capacity(cells.len());
    let mut anchor = start;
    let mut i = 0;
    while i < cells.len() {
        // Farthest later cell that can be walked to in a straight line from the anchor.
        let mut j = i;
        for k in (i + 1)..cells.len().min(i + 16) {
            if cells[k].y == anchor.y && cells[i..=k].iter().all(|c| c.y == anchor.y) && line_walkable(w, anchor, cells[k], h) {
                j = k;
            }
        }
        out.push(cells[j]);
        anchor = cells[j];
        i = j + 1;
    }
    out
}

/// Is the straight segment between the centres of two cells on the same level walkable?
fn line_walkable<W: BlockQuery>(w: &W, a: IVec3, b: IVec3, h: i32) -> bool {
    let (pa, pb) = (Vec3::new(a.x as f32 + 0.5, 0.0, a.z as f32 + 0.5), Vec3::new(b.x as f32 + 0.5, 0.0, b.z as f32 + 0.5));
    let len = pa.distance(pb);
    let steps = (len / 0.25).ceil().max(1.0) as i32;
    (0..=steps).all(|s| {
        let p = pa.lerp(pb, s as f32 / steps as f32);
        // The body (0.6 wide) must fit: test the four corners of its footprint.
        [(-0.3, -0.3), (0.3, -0.3), (-0.3, 0.3), (0.3, 0.3)]
            .iter()
            .all(|(dx, dz)| standable(w, IVec3::new((p.x + dx).floor() as i32, a.y, (p.z + dz).floor() as i32), h))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Flat stone floor at y = 0, extra blocks on request.
    struct Grid {
        blocks: HashMap<IVec3, BlockId>,
        floor: bool,
    }

    impl Grid {
        fn flat() -> Self {
            Grid { blocks: HashMap::new(), floor: true }
        }
        fn put(&mut self, p: IVec3, b: BlockId) -> &mut Self {
            self.blocks.insert(p, b);
            self
        }
        fn wall(&mut self, x: i32, z0: i32, z1: i32, height: i32) {
            for z in z0..=z1 {
                for y in 1..=height {
                    self.blocks.insert(IVec3::new(x, y, z), BlockId::STONE);
                }
            }
        }
    }

    impl BlockQuery for Grid {
        fn block(&self, p: IVec3) -> Option<BlockId> {
            if p.x.abs() > 100 || p.z.abs() > 100 {
                return None;
            }
            Some(self.blocks.get(&p).copied().unwrap_or(if self.floor && p.y <= 0 { BlockId::STONE } else { BlockId::AIR }))
        }
    }

    fn p(x: i32, y: i32, z: i32) -> IVec3 {
        IVec3::new(x, y, z)
    }

    /// Every step of a returned path must be a legal move.
    fn assert_walkable(w: &Grid, start: IVec3, r: &PathResult) {
        let mut prev = start;
        for &c in &r.cells {
            assert!(standable(w, c, 2), "{c} is not standable");
            let (dx, dz) = ((c.x - prev.x).abs(), (c.z - prev.z).abs());
            assert!(dx.max(dz) >= 1 || c.y != prev.y, "no movement from {prev} to {c}");
            prev = c;
        }
    }

    #[test]
    fn straight_line_on_open_ground_is_a_single_waypoint_after_smoothing() {
        let w = Grid::flat();
        let r = find_path(&w, p(0, 1, 0), p(10, 1, 0), PathOptions::default()).unwrap();
        assert!(r.reached);
        assert_eq!(r.cells.last(), Some(&p(10, 1, 0)));
        assert!(r.cells.len() <= 2, "{:?}", r.cells);
    }

    #[test]
    fn goes_around_a_wall() {
        let mut w = Grid::flat();
        w.wall(5, -6, 6, 3);
        let r = find_path(&w, p(0, 1, 0), p(10, 1, 0), PathOptions::default()).unwrap();
        assert!(r.reached);
        assert_walkable(&w, p(0, 1, 0), &r);
        assert!(r.cells.iter().any(|c| c.z.abs() >= 7), "must detour around the wall ends: {:?}", r.cells);
        assert!(r.cells.iter().all(|c| c.x != 5 || c.z.abs() > 6));
    }

    #[test]
    fn climbs_a_one_block_step_and_stairs() {
        let mut w = Grid::flat();
        for i in 0..4 {
            for z in -2..=2 {
                for y in 1..=(i + 1) {
                    w.put(p(3 + i, y, z), BlockId::COBBLE);
                }
            }
        }
        let r = find_path(&w, p(0, 1, 0), p(6, 5, 0), PathOptions::default()).unwrap();
        assert!(r.reached, "{:?}", r.cells);
        assert_eq!(r.cells.last().unwrap().y, 5);
        assert_walkable(&w, p(0, 1, 0), &r);
    }

    #[test]
    fn two_block_walls_cannot_be_climbed_so_the_path_is_partial() {
        let mut w = Grid::flat();
        // A closed ring of 2-high walls around the goal.
        for x in 8..=12 {
            for z in -2..=2 {
                if x == 8 || x == 12 || z == -2 || z == 2 {
                    w.put(p(x, 1, z), BlockId::STONE).put(p(x, 2, z), BlockId::STONE);
                }
            }
        }
        let r = find_path(&w, p(0, 1, 0), p(10, 1, 0), PathOptions::default()).unwrap();
        assert!(!r.reached);
        let end = *r.cells.last().unwrap();
        assert!(end.x >= 6, "gets as close as possible: {end}");
        assert_walkable(&w, p(0, 1, 0), &r);
    }

    #[test]
    fn drops_down_small_ledges_but_not_big_cliffs() {
        let mut w = Grid::flat();
        for x in -3..=3 {
            for z in -3..=3 {
                for y in 1..=3 {
                    w.put(p(x, y, z), BlockId::STONE);
                }
            }
        }
        // Standing on top (y = 4), goal on the ground 3 below: a legal drop.
        let r = find_path(&w, p(0, 4, 0), p(5, 1, 0), PathOptions::default()).unwrap();
        assert!(r.reached);
        // Make the platform 5 high: the 5-block drop is too deep, the path must find another way
        // (there is none: the platform is a solid block), so it is partial.
        for x in -3..=3 {
            for z in -3..=3 {
                w.put(p(x, 4, z), BlockId::STONE).put(p(x, 5, z), BlockId::STONE);
            }
        }
        let r = find_path(&w, p(0, 6, 0), p(5, 1, 0), PathOptions::default()).unwrap();
        assert!(!r.reached);
    }

    #[test]
    fn avoids_lava_and_pits() {
        let mut w = Grid::flat();
        for x in 3..=5 {
            for z in -8..=8 {
                w.put(p(x, 0, z), BlockId::LAVA);
            }
        }
        let r = find_path(&w, p(0, 1, 0), p(9, 1, 0), PathOptions::default()).unwrap();
        // The lava strip spans z = -8..8: the only way around is past its ends.
        assert!(r.reached);
        assert!(r.cells.iter().all(|c| !w.block(*c - IVec3::Y).unwrap().is_lava()));
        assert!(r.cells.iter().any(|c| c.z.abs() >= 9), "{:?}", r.cells);
    }

    #[test]
    fn respects_headroom() {
        let mut w = Grid::flat();
        // A one-block-high tunnel is not passable for a two-block-tall mob.
        for z in -6..=6 {
            w.put(p(5, 2, z), BlockId::STONE);
        }
        for z in -6..=6 {
            for x in [4, 6] {
                w.put(p(x, 1, z), BlockId::STONE).put(p(x, 2, z), BlockId::STONE);
            }
        }
        w.put(p(5, 1, 7), BlockId::STONE).put(p(5, 2, 7), BlockId::STONE);
        w.put(p(5, 1, -7), BlockId::STONE).put(p(5, 2, -7), BlockId::STONE);
        let r = find_path(&w, p(0, 1, 0), p(10, 1, 0), PathOptions::default()).unwrap();
        assert!(!r.cells.iter().any(|c| c.x == 5 && c.z.abs() <= 6 && c.y == 1), "must not squeeze under the ceiling");
    }

    #[test]
    fn no_corner_cutting_through_diagonal_gaps() {
        let mut w = Grid::flat();
        // Two pillars touching diagonally.
        w.put(p(1, 1, 0), BlockId::STONE).put(p(1, 2, 0), BlockId::STONE);
        w.put(p(0, 1, 1), BlockId::STONE).put(p(0, 2, 1), BlockId::STONE);
        let mut out = Vec::new();
        neighbours(&w, p(0, 1, 0), 2, &mut out);
        assert!(!out.iter().any(|(c, _)| *c == p(1, 1, 1)), "diagonal between two blocks is closed");
    }

    #[test]
    fn start_equals_goal_and_unsupported_start() {
        let w = Grid::flat();
        let r = find_path(&w, p(2, 1, 2), p(2, 1, 2), PathOptions::default()).unwrap();
        assert!(r.cells.is_empty() && r.reached);
        let sky = Grid { blocks: HashMap::new(), floor: false };
        assert!(find_path(&sky, p(0, 50, 0), p(5, 50, 0), PathOptions::default()).is_none());
    }

    #[test]
    fn respects_the_node_budget_and_stays_fast() {
        let mut w = Grid::flat();
        w.wall(5, -90, 90, 3);
        let t = std::time::Instant::now();
        let r = find_path(&w, p(0, 1, 0), p(10, 1, 0), PathOptions { max_nodes: 800, ..default_opts() }).unwrap();
        assert!(r.expanded <= 801);
        assert!(!r.reached, "the detour exceeds the budget");
        assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
        // A longer open path within budget is found quickly.
        let w = Grid::flat();
        let r = find_path(&w, p(0, 1, 0), p(40, 1, 33), default_opts()).unwrap();
        assert!(r.reached);
    }

    fn default_opts() -> PathOptions {
        PathOptions::default()
    }

    #[test]
    fn snapping_finds_the_ground_below_a_floating_goal() {
        let w = Grid::flat();
        assert_eq!(snap_to_ground(&w, p(3, 4, 3), 2, 6), Some(p(3, 1, 3)));
        assert_eq!(snap_to_ground(&w, p(3, 20, 3), 2, 6), None);
    }
}
