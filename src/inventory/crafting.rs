//! Crafting recipes (shaped and shapeless), smelting and the furnace simulation. Pure logic.

use super::GRID_CELLS;
use crate::items::{Item, ItemStack, Tier, ToolKind};
use crate::world::block::BlockId;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Clone, Debug)]
enum Pattern {
    /// Row-major `w * h` cells, already trimmed to their bounding box.
    Shaped { w: usize, h: usize, cells: Vec<Option<Item>> },
    Shapeless(Vec<Item>),
}

#[derive(Clone, Debug)]
pub struct Recipe {
    pattern: Pattern,
    pub result: ItemStack,
}

impl Recipe {
    /// Smallest crafting grid (2 or 3) the recipe fits into.
    pub fn grid_size(&self) -> usize {
        match &self.pattern {
            Pattern::Shaped { w, h, .. } => (*w).max(*h).max(2),
            Pattern::Shapeless(items) => {
                if items.len() > 4 {
                    3
                } else {
                    2
                }
            }
        }
    }

    /// Ingredient cells of a shaped recipe (`w`, `h`, cells) for the recipe-book style hints.
    pub fn ingredients(&self) -> Vec<Item> {
        match &self.pattern {
            Pattern::Shaped { cells, .. } => cells.iter().flatten().copied().collect(),
            Pattern::Shapeless(items) => items.clone(),
        }
    }
}

fn shaped(rows: &[&str], key: &[(char, Item)], result: ItemStack) -> Recipe {
    let h = rows.len();
    let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
    let mut cells = vec![None; w * h];
    for (y, row) in rows.iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            if c != '.' {
                let item = key.iter().find(|(k, _)| *k == c).unwrap_or_else(|| panic!("no key for {c}")).1;
                cells[y * w + x] = Some(item);
            }
        }
    }
    Recipe { pattern: Pattern::Shaped { w, h, cells }, result }
}

fn shapeless(items: &[Item], result: ItemStack) -> Recipe {
    Recipe { pattern: Pattern::Shapeless(items.to_vec()), result }
}

fn stack(item: Item, n: u8) -> ItemStack {
    ItemStack::new(item, n)
}

fn build_recipes() -> Vec<Recipe> {
    let b = Item::block;
    let planks = b(BlockId::PLANKS);
    let cobble = b(BlockId::COBBLE);
    let stick = Item::STICK;
    let mut r = vec![
        shapeless(&[b(BlockId::LOG)], stack(planks, 4)),
        shaped(&["P", "P"], &[('P', planks)], stack(stick, 4)),
        shaped(&["PP", "PP"], &[('P', planks)], stack(b(BlockId::CRAFTING_TABLE), 1)),
        shaped(&["CCC", "C.C", "CCC"], &[('C', cobble)], stack(b(BlockId::FURNACE), 1)),
        shaped(&["C", "S"], &[('C', Item::COAL), ('S', stick)], stack(b(BlockId::TORCH), 4)),
        shaped(&["PSP", "PSP"], &[('P', planks), ('S', stick)], stack(b(BlockId::FENCE), 3)),
        shaped(&["SS", "SS"], &[('S', b(BlockId::STONE))], stack(b(BlockId::STONE_BRICKS), 4)),
        shaped(&["SS", "SS"], &[('S', b(BlockId::SAND))], stack(b(BlockId::SANDSTONE), 1)),
        shaped(&["SS", "SS"], &[('S', cobble)], stack(b(BlockId::ANDESITE), 4)),
    ];
    // Compressed storage blocks and back.
    for (item, block) in [
        (Item::COAL, BlockId::COAL_BLOCK),
        (Item::IRON_INGOT, BlockId::IRON_BLOCK),
        (Item::GOLD_INGOT, BlockId::GOLD_BLOCK),
        (Item::DIAMOND, BlockId::DIAMOND_BLOCK),
    ] {
        r.push(shaped(&["III", "III", "III"], &[('I', item)], stack(b(block), 1)));
        r.push(shapeless(&[b(block)], stack(item, 9)));
    }
    // Tools of every material.
    let materials = [
        (Tier::Wood, planks),
        (Tier::Stone, cobble),
        (Tier::Iron, Item::IRON_INGOT),
        (Tier::Gold, Item::GOLD_INGOT),
        (Tier::Diamond, Item::DIAMOND),
    ];
    for (tier, m) in materials {
        let key = [('M', m), ('S', stick)];
        r.push(shaped(&["MMM", ".S.", ".S."], &key, stack(Item::tool(ToolKind::Pickaxe, tier), 1)));
        r.push(shaped(&["MM", "MS", ".S"], &key, stack(Item::tool(ToolKind::Axe, tier), 1)));
        r.push(shaped(&["M", "S", "S"], &key, stack(Item::tool(ToolKind::Shovel, tier), 1)));
        r.push(shaped(&["M", "M", "S"], &key, stack(Item::tool(ToolKind::Sword, tier), 1)));
    }
    // Iron armor.
    let i = [('I', Item::IRON_INGOT)];
    r.push(shaped(&["III", "I.I"], &i, stack(Item::IRON_HELMET, 1)));
    r.push(shaped(&["I.I", "III", "III"], &i, stack(Item::IRON_CHESTPLATE, 1)));
    r.push(shaped(&["III", "I.I", "I.I"], &i, stack(Item::IRON_LEGGINGS, 1)));
    r.push(shaped(&["I.I", "I.I"], &i, stack(Item::IRON_BOOTS, 1)));
    r
}

/// All crafting recipes.
pub fn recipes() -> &'static [Recipe] {
    static RECIPES: OnceLock<Vec<Recipe>> = OnceLock::new();
    RECIPES.get_or_init(build_recipes)
}

/// The crafting grid: `GRID_CELLS` row-major cells of a 3x3 board. When `size` is 2 only the
/// top-left 2x2 cells take part.
pub fn find_recipe(grid: &[Option<Item>; GRID_CELLS], size: usize) -> Option<&'static Recipe> {
    let size = size.clamp(2, 3);
    let mut cells: [Option<Item>; GRID_CELLS] = [None; GRID_CELLS];
    for y in 0..size {
        for x in 0..size {
            cells[y * 3 + x] = grid[y * 3 + x];
        }
    }
    let used: Vec<usize> = (0..GRID_CELLS).filter(|i| cells[*i].is_some()).collect();
    if used.is_empty() {
        return None;
    }
    let (x0, x1) = (used.iter().map(|i| i % 3).min()?, used.iter().map(|i| i % 3).max()?);
    let (y0, y1) = (used.iter().map(|i| i / 3).min()?, used.iter().map(|i| i / 3).max()?);
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let trimmed = |mirror: bool| -> Vec<Option<Item>> {
        let mut v = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let sx = if mirror { w - 1 - x } else { x };
                v.push(cells[(y0 + y) * 3 + x0 + sx]);
            }
        }
        v
    };
    let (plain, mirrored) = (trimmed(false), trimmed(true));
    let mut items: Vec<Item> = used.iter().filter_map(|i| cells[*i]).collect();
    items.sort_by_key(|i| i.0);
    recipes().iter().find(|r| match &r.pattern {
        Pattern::Shaped { w: rw, h: rh, cells: rc } => *rw == w && *rh == h && (*rc == plain || *rc == mirrored),
        Pattern::Shapeless(want) => {
            let mut want = want.clone();
            want.sort_by_key(|i| i.0);
            want == items
        }
    })
}

/// What a furnace turns `item` into.
pub fn smelt(item: Item) -> Option<ItemStack> {
    let out = match item.as_block() {
        Some(BlockId::IRON_ORE) => Item::IRON_INGOT,
        Some(BlockId::GOLD_ORE) => Item::GOLD_INGOT,
        Some(BlockId::SAND) => Item::block(BlockId::GLASS),
        Some(BlockId::COBBLE) => Item::block(BlockId::STONE),
        Some(BlockId::STONE) => Item::block(BlockId::SMOOTH_STONE),
        Some(BlockId::NETHERRACK) => Item::block(BlockId::NETHER_BRICKS),
        _ => match item {
            Item::RAW_PORK => Item::COOKED_PORK,
            Item::RAW_BEEF => Item::COOKED_BEEF,
            _ => return None,
        },
    };
    Some(ItemStack::new(out, 1))
}

/// Seconds a furnace needs per item.
pub const SMELT_SECS: f32 = 10.0;

/// One furnace: three stacks and the burn/cook counters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FurnaceState {
    pub input: Option<ItemStack>,
    pub fuel: Option<ItemStack>,
    pub output: Option<ItemStack>,
    /// Seconds of fuel left in the current piece.
    #[serde(default)]
    pub burn_left: f32,
    /// Length of the current fuel piece (for the flame animation).
    #[serde(default)]
    pub burn_max: f32,
    /// Progress of the current item, `0..SMELT_SECS`.
    #[serde(default)]
    pub cook: f32,
}

impl FurnaceState {
    pub fn is_burning(&self) -> bool {
        self.burn_left > 0.0
    }

    pub fn is_empty(&self) -> bool {
        self.input.is_none() && self.fuel.is_none() && self.output.is_none()
    }

    /// Whether the furnace needs simulating at all.
    pub fn is_active(&self) -> bool {
        self.is_burning() || self.cook > 0.0 || (self.input.is_some() && self.fuel.is_some())
    }

    fn recipe(&self) -> Option<ItemStack> {
        let result = smelt(self.input?.item)?;
        match self.output {
            None => Some(result),
            Some(o) if o.item == result.item && o.space_left() >= result.count => Some(result),
            Some(_) => None,
        }
    }

    /// Advances the simulation by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        let recipe = self.recipe();
        if self.burn_left <= 0.0
            && recipe.is_some()
            && let Some(fuel) = self.fuel
            && let Some(secs) = fuel.item.fuel_secs()
        {
            self.burn_left = secs;
            self.burn_max = secs;
            self.fuel = (fuel.count > 1).then(|| ItemStack::new(fuel.item, fuel.count - 1));
        }
        if self.burn_left > 0.0 {
            self.burn_left = (self.burn_left - dt).max(0.0);
            match recipe {
                Some(result) => {
                    self.cook += dt;
                    if self.cook >= SMELT_SECS {
                        self.cook -= SMELT_SECS;
                        if let Some(input) = &mut self.input {
                            input.count -= 1;
                            if input.count == 0 {
                                self.input = None;
                            }
                        }
                        self.output = Some(match self.output {
                            Some(o) => ItemStack::new(o.item, o.count + result.count),
                            None => result,
                        });
                    }
                }
                None => self.cook = (self.cook - 2.0 * dt).max(0.0),
            }
        } else {
            self.cook = (self.cook - 2.0 * dt).max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(cells: &[(usize, Item)]) -> [Option<Item>; GRID_CELLS] {
        let mut g = [None; GRID_CELLS];
        for (i, item) in cells {
            g[*i] = Some(*item);
        }
        g
    }

    const PLANKS: Item = Item::block(BlockId::PLANKS);
    const LOG: Item = Item::block(BlockId::LOG);

    #[test]
    fn logs_become_planks_anywhere_in_the_grid() {
        for cell in [0, 1, 3, 4] {
            let r = find_recipe(&grid(&[(cell, LOG)]), 2).expect("planks");
            assert_eq!(r.result, ItemStack::new(PLANKS, 4));
        }
        assert!(find_recipe(&grid(&[]), 3).is_none());
        // A log outside the 2x2 area does not count in the inventory grid.
        assert!(find_recipe(&grid(&[(8, LOG)]), 2).is_none());
        assert!(find_recipe(&grid(&[(8, LOG)]), 3).is_some());
    }

    #[test]
    fn sticks_need_two_planks_stacked_vertically() {
        let r = find_recipe(&grid(&[(0, PLANKS), (3, PLANKS)]), 2).unwrap();
        assert_eq!(r.result, ItemStack::new(Item::STICK, 4));
        assert!(find_recipe(&grid(&[(0, PLANKS), (1, PLANKS)]), 2).is_none(), "side by side is no recipe");
        // The same recipe works in the right column and in a 3x3 grid.
        assert!(find_recipe(&grid(&[(1, PLANKS), (4, PLANKS)]), 2).is_some());
        assert!(find_recipe(&grid(&[(7, PLANKS), (4, PLANKS)]), 3).is_some());
    }

    #[test]
    fn crafting_table_is_a_2x2_recipe_and_the_furnace_is_3x3() {
        let table = grid(&[(0, PLANKS), (1, PLANKS), (3, PLANKS), (4, PLANKS)]);
        assert_eq!(find_recipe(&table, 2).unwrap().result.item, Item::block(BlockId::CRAFTING_TABLE));
        let cobble = Item::block(BlockId::COBBLE);
        let ring: Vec<(usize, Item)> = (0..9).filter(|i| *i != 4).map(|i| (i, cobble)).collect();
        assert_eq!(find_recipe(&grid(&ring), 3).unwrap().result.item, Item::block(BlockId::FURNACE));
        assert!(find_recipe(&grid(&ring), 2).is_none());
        assert_eq!(find_recipe(&grid(&ring), 3).unwrap().grid_size(), 3);
    }

    #[test]
    fn tools_and_mirrored_axes() {
        let s = Item::STICK;
        let pick = grid(&[(0, PLANKS), (1, PLANKS), (2, PLANKS), (4, s), (7, s)]);
        assert_eq!(find_recipe(&pick, 3).unwrap().result.item, Item::tool(ToolKind::Pickaxe, Tier::Wood));
        let axe = grid(&[(0, PLANKS), (1, PLANKS), (3, PLANKS), (4, s), (7, s)]);
        let mirrored = grid(&[(1, PLANKS), (2, PLANKS), (4, PLANKS), (5, s), (8, s)]);
        let expect = Item::tool(ToolKind::Axe, Tier::Wood);
        assert_eq!(find_recipe(&axe, 3).unwrap().result.item, expect);
        assert_eq!(find_recipe(&mirrored, 3).unwrap().result.item, expect, "axes may be mirrored");
        let sword = grid(&[(1, Item::DIAMOND), (4, Item::DIAMOND), (7, s)]);
        assert_eq!(find_recipe(&sword, 3).unwrap().result.item, Item::tool(ToolKind::Sword, Tier::Diamond));
        // Wrong material mix: no result.
        let bad = grid(&[(1, Item::DIAMOND), (4, PLANKS), (7, s)]);
        assert!(find_recipe(&bad, 3).is_none());
    }

    #[test]
    fn torches_and_storage_blocks() {
        let t = find_recipe(&grid(&[(0, Item::COAL), (3, Item::STICK)]), 2).unwrap();
        assert_eq!(t.result, ItemStack::new(Item::block(BlockId::TORCH), 4));
        let nine: Vec<(usize, Item)> = (0..9).map(|i| (i, Item::IRON_INGOT)).collect();
        assert_eq!(find_recipe(&grid(&nine), 3).unwrap().result.item, Item::block(BlockId::IRON_BLOCK));
        let back = find_recipe(&grid(&[(4, Item::block(BlockId::DIAMOND_BLOCK))]), 3).unwrap();
        assert_eq!(back.result, ItemStack::new(Item::DIAMOND, 9));
    }

    #[test]
    fn every_recipe_is_findable_through_its_own_pattern() {
        for r in recipes() {
            assert!(r.result.count >= 1 && r.result.item.is_valid());
            let g = match &r.pattern {
                Pattern::Shaped { w, cells, .. } => {
                    let mut g = [None; GRID_CELLS];
                    for (i, c) in cells.iter().enumerate() {
                        g[(i / w) * 3 + i % w] = *c;
                    }
                    g
                }
                Pattern::Shapeless(items) => {
                    let mut g = [None; GRID_CELLS];
                    for (i, it) in items.iter().enumerate() {
                        g[i] = Some(*it);
                    }
                    g
                }
            };
            let found = find_recipe(&g, 3).unwrap_or_else(|| panic!("recipe for {:?} not found", r.result.item));
            assert_eq!(found.result, r.result, "ambiguous recipe for {}", r.result.item.name());
            let small = find_recipe(&g, 2).map(|x| x.result);
            if r.grid_size() == 2 {
                assert_eq!(small, Some(r.result), "{} fits the inventory grid", r.result.item.name());
            } else {
                assert_ne!(small, Some(r.result), "{} needs a crafting table", r.result.item.name());
            }
        }
    }

    #[test]
    fn smelting_table() {
        assert_eq!(smelt(Item::block(BlockId::IRON_ORE)).unwrap().item, Item::IRON_INGOT);
        assert_eq!(smelt(Item::block(BlockId::SAND)).unwrap().item, Item::block(BlockId::GLASS));
        assert_eq!(smelt(Item::RAW_BEEF).unwrap().item, Item::COOKED_BEEF);
        assert!(smelt(Item::STICK).is_none());
    }

    #[test]
    fn furnace_burns_fuel_and_produces_output() {
        let mut f = FurnaceState {
            input: Some(ItemStack::new(Item::block(BlockId::IRON_ORE), 3)),
            fuel: Some(ItemStack::new(Item::COAL, 1)),
            ..default_furnace()
        };
        assert!(f.is_active());
        for _ in 0..105 {
            f.tick(0.1);
        }
        assert_eq!(f.output, Some(ItemStack::new(Item::IRON_INGOT, 1)));
        assert_eq!(f.input.map(|s| s.count), Some(2));
        assert!(f.fuel.is_none(), "the coal was lit");
        // A coal burns 80 s: enough for all three ores (30 s) and then it goes out.
        for _ in 0..1000 {
            f.tick(0.1);
        }
        assert_eq!(f.output.map(|s| s.count), Some(3));
        assert!(f.input.is_none() && !f.is_burning());
    }

    fn default_furnace() -> FurnaceState {
        FurnaceState::default()
    }

    #[test]
    fn furnace_without_fuel_or_with_full_output_does_nothing() {
        let mut f = FurnaceState { input: Some(ItemStack::new(Item::block(BlockId::SAND), 1)), ..default_furnace() };
        for _ in 0..200 {
            f.tick(0.1);
        }
        assert!(f.output.is_none() && f.cook == 0.0);
        let mut full = FurnaceState {
            input: Some(ItemStack::new(Item::block(BlockId::SAND), 1)),
            fuel: Some(ItemStack::new(Item::COAL, 1)),
            output: Some(ItemStack::new(Item::block(BlockId::GLASS), 64)),
            ..default_furnace()
        };
        for _ in 0..200 {
            full.tick(0.1);
        }
        assert_eq!(full.fuel.map(|s| s.count), Some(1), "fuel is not wasted while the output is full");
    }
}
