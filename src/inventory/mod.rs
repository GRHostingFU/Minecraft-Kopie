//! The player inventory, including the Minecraft-style click rules (pick up, place, merge, split,
//! swap, shift-click, drag), the crafting grid and the slots of an opened furnace. Pure data and
//! logic — no Bevy dependency except the `Resource` derive — so it is fully unit-tested.

pub mod crafting;
pub mod creative;

pub use crate::items::{Item, ItemStack};
use bevy::math::IVec3;
use bevy::prelude::Resource;
use crafting::FurnaceState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const HOTBAR_SLOTS: usize = 9;
/// Hotbar (slots `0..9`) plus the 27 slots of the main inventory (`9..36`).
pub const INVENTORY_SLOTS: usize = 36;
pub const ARMOR_SLOTS: usize = 4;
/// Armor slots `36..40`: helmet, chestplate, leggings, boots.
pub const ARMOR_START: usize = INVENTORY_SLOTS;
pub const OFFHAND: usize = ARMOR_START + ARMOR_SLOTS;
/// Cells of the crafting grid (a 3x3 board; the inventory only uses its top-left 2x2).
pub const GRID_CELLS: usize = 9;
pub const CRAFT_START: usize = OFFHAND + 1;
pub const CRAFT_OUT: usize = CRAFT_START + GRID_CELLS;
const LAST_CRAFT_CELL: usize = CRAFT_OUT - 1;
/// Slots of the furnace that is currently open.
pub const FURNACE_IN: usize = CRAFT_OUT + 1;
pub const FURNACE_FUEL: usize = FURNACE_IN + 1;
pub const FURNACE_OUT: usize = FURNACE_FUEL + 1;
pub const TOTAL_SLOTS: usize = FURNACE_OUT + 1;
/// Slots that are written to the save file (everything up to and including the off hand).
pub const SAVED_SLOTS: usize = OFFHAND + 1;
/// Old save files stored 40 slots followed by the cursor stack.
const LEGACY_SAVE_LEN: usize = ARMOR_START + ARMOR_SLOTS + 1;
pub const MAX_STACK: u8 = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClickButton {
    Left,
    Right,
}

/// A furnace together with its position, as stored in `level.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FurnaceSave {
    pub pos: [i32; 3],
    pub state: FurnaceState,
}

/// Slot contents plus the stack currently attached to the mouse cursor.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Inventory {
    pub slots: [Option<ItemStack>; TOTAL_SLOTS],
    pub selected: usize,
    /// The stack "in the hand" while the inventory screen is open.
    pub cursor: Option<ItemStack>,
    /// Edge length of the crafting grid in use: 2 (inventory) or 3 (crafting table).
    pub craft_size: usize,
    /// All furnaces of the world. The stacks of the open one live in the `FURNACE_*` slots.
    pub furnaces: HashMap<IVec3, FurnaceState>,
    pub open_furnace: Option<IVec3>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self::empty()
    }
}

impl Inventory {
    pub fn empty() -> Self {
        Self {
            slots: [None; TOTAL_SLOTS],
            selected: 0,
            cursor: None,
            craft_size: 2,
            furnaces: HashMap::new(),
            open_furnace: None,
        }
    }

    /// What a fresh creative world starts with: building blocks in the hotbar and a spare armor set.
    pub fn starter_kit() -> Self {
        use crate::items::{Tier, ToolKind};
        use crate::world::block::BlockId;
        let mut inv = Self::empty();
        let hotbar = [
            Item::block(BlockId::STONE),
            Item::block(BlockId::PLANKS),
            Item::block(BlockId::LOG),
            Item::block(BlockId::COBBLE),
            Item::block(BlockId::GLASS),
            Item::block(BlockId::TORCH),
            Item::block(BlockId::DIRT),
            Item::block(BlockId::SAND),
        ];
        for (i, item) in hotbar.into_iter().enumerate() {
            inv.slots[i] = Some(ItemStack::new(item, MAX_STACK));
        }
        inv.slots[8] = Some(ItemStack::new(Item::tool(ToolKind::Pickaxe, Tier::Diamond), 1));
        inv.slots[HOTBAR_SLOTS] = Some(ItemStack::new(Item::IRON_HELMET, 1));
        inv.slots[HOTBAR_SLOTS + 1] = Some(ItemStack::new(Item::IRON_CHESTPLATE, 1));
        inv.slots[HOTBAR_SLOTS + 2] = Some(ItemStack::new(Item::IRON_LEGGINGS, 1));
        inv.slots[HOTBAR_SLOTS + 3] = Some(ItemStack::new(Item::IRON_BOOTS, 1));
        inv.slots[HOTBAR_SLOTS + 4] = Some(ItemStack::new(Item::block(BlockId::SNOW), 32));
        inv.slots[HOTBAR_SLOTS + 5] = Some(ItemStack::new(Item::block(BlockId::LEAVES), 32));
        inv
    }

    /// Whether `slot` may hold `item` (armor slots only take their own piece, the outputs nothing).
    pub fn accepts(&self, slot: usize, item: Item) -> bool {
        match slot {
            0..INVENTORY_SLOTS | OFFHAND | CRAFT_START..=LAST_CRAFT_CELL => true,
            ARMOR_START..OFFHAND => item.armor_slot() == Some(slot - ARMOR_START),
            FURNACE_IN => self.open_furnace.is_some() && crafting::smelt(item).is_some(),
            FURNACE_FUEL => self.open_furnace.is_some() && item.fuel_secs().is_some(),
            _ => false,
        }
    }

    pub fn selected_stack(&self) -> Option<ItemStack> {
        self.slots[self.selected]
    }

    /// The item in the selected hotbar slot.
    pub fn held_item(&self) -> Option<Item> {
        self.selected_stack().map(|s| s.item)
    }

    pub fn offhand(&self) -> Option<ItemStack> {
        self.slots[OFFHAND]
    }

    pub fn select(&mut self, slot: usize) {
        self.selected = slot.min(HOTBAR_SLOTS - 1);
    }

    pub fn cycle(&mut self, steps: i32) {
        self.selected = (self.selected as i32 + steps).rem_euclid(HOTBAR_SLOTS as i32) as usize;
    }

    /// Swaps the selected hotbar stack with the off hand.
    pub fn swap_offhand(&mut self) {
        self.slots.swap(self.selected, OFFHAND);
    }

    /// Removes one item from the selected slot (placing a block). Returns what was taken.
    pub fn take_one_selected(&mut self) -> Option<Item> {
        self.take_one(self.selected)
    }

    /// Removes one item from `slot`. Returns what was taken.
    pub fn take_one(&mut self, slot: usize) -> Option<Item> {
        let slot = &mut self.slots[slot];
        let stack = slot.as_mut()?;
        let item = stack.item;
        stack.count -= 1;
        if stack.count == 0 {
            *slot = None;
        }
        Some(item)
    }

    /// Adds wear to the tool in the selected slot; a tool that wears out disappears.
    /// Returns `true` if it broke.
    pub fn wear_selected(&mut self, uses: u16) -> bool {
        let slot = &mut self.slots[self.selected];
        let Some(stack) = slot.as_mut() else { return false };
        if stack.wear_out(uses) {
            *slot = None;
            return true;
        }
        false
    }

    /// Picks an item up: tops up existing stacks first, then fills empty slots, hotbar first.
    /// Returns what did not fit.
    pub fn add(&mut self, stack: ItemStack) -> Option<ItemStack> {
        self.insert_into(0..INVENTORY_SLOTS, stack)
    }

    fn merge_into(&mut self, range: std::ops::Range<usize>, mut stack: ItemStack) -> Option<ItemStack> {
        for i in range {
            if let Some(s) = &mut self.slots[i]
                && s.item == stack.item
            {
                let moved = s.space_left().min(stack.count);
                s.count += moved;
                stack.count -= moved;
                if stack.count == 0 {
                    return None;
                }
            }
        }
        Some(stack)
    }

    fn fill_empty(&mut self, range: std::ops::Range<usize>, mut stack: ItemStack) -> Option<ItemStack> {
        for i in range {
            if self.slots[i].is_none() && self.accepts(i, stack.item) {
                let moved = stack.item.max_stack().min(stack.count);
                self.slots[i] = Some(ItemStack { count: moved, ..stack });
                stack.count -= moved;
                if stack.count == 0 {
                    return None;
                }
            }
        }
        Some(stack)
    }

    fn insert_into(&mut self, range: std::ops::Range<usize>, stack: ItemStack) -> Option<ItemStack> {
        let stack = self.merge_into(range.clone(), stack)?;
        self.fill_empty(range, stack)
    }

    /// Whether at least one more `item` would fit into the inventory slots.
    pub fn can_hold(&self, item: Item) -> bool {
        self.slots[..INVENTORY_SLOTS].iter().any(|s| match s {
            None => true,
            Some(s) => s.item == item && s.space_left() > 0,
        })
    }

    /// Total number of `item`s in the inventory slots (not the cursor).
    pub fn count_of(&self, item: Item) -> u32 {
        self.slots[..INVENTORY_SLOTS].iter().flatten().filter(|s| s.item == item).map(|s| s.count as u32).sum()
    }

    /// Mouse click on a slot, following the Minecraft rules.
    pub fn click(&mut self, slot: usize, button: ClickButton) {
        if slot >= TOTAL_SLOTS {
            return;
        }
        if slot == CRAFT_OUT || slot == FURNACE_OUT {
            self.take_output(slot);
            self.after_change();
            return;
        }
        match (self.cursor, self.slots[slot]) {
            (None, None) => {}
            (None, Some(s)) => {
                let take = match button {
                    ClickButton::Left => s.count,
                    ClickButton::Right => s.count.div_ceil(2),
                };
                self.cursor = Some(ItemStack { count: take, ..s });
                self.slots[slot] = (s.count > take).then(|| ItemStack { count: s.count - take, ..s });
            }
            (Some(c), None) => {
                if !self.accepts(slot, c.item) {
                    return;
                }
                let put = match button {
                    ClickButton::Left => c.count.min(c.item.max_stack()),
                    ClickButton::Right => 1,
                };
                self.slots[slot] = Some(ItemStack { count: put, ..c });
                self.cursor = (c.count > put).then(|| ItemStack { count: c.count - put, ..c });
            }
            (Some(c), Some(s)) if c.item == s.item => {
                let moved = match button {
                    ClickButton::Left => s.space_left().min(c.count),
                    ClickButton::Right => s.space_left().min(1),
                };
                self.slots[slot] = Some(ItemStack { count: s.count + moved, ..s });
                self.cursor = (c.count > moved).then(|| ItemStack { count: c.count - moved, ..c });
            }
            (Some(c), Some(s)) => {
                // Different items: swap, if the slot accepts the carried one.
                if self.accepts(slot, c.item) && c.count <= c.item.max_stack() {
                    self.slots[slot] = Some(c);
                    self.cursor = Some(s);
                }
            }
        }
        self.after_change();
    }

    /// Finishing a drag across several slots with a stack on the cursor. `one_each` (right-button
    /// drag) puts a single item into every slot, otherwise (left-button drag) the stack is split
    /// evenly. Slots that are occupied by another item, or do not accept the item, are skipped.
    pub fn distribute(&mut self, slots: &[usize], one_each: bool) {
        let Some(mut cursor) = self.cursor else { return };
        let mut targets: Vec<usize> = Vec::new();
        for &slot in slots {
            let usable = slot < TOTAL_SLOTS
                && !targets.contains(&slot)
                && self.accepts(slot, cursor.item)
                && self.slots[slot].is_none_or(|s| s.item == cursor.item && s.space_left() > 0);
            if usable {
                targets.push(slot);
            }
        }
        if targets.is_empty() {
            return;
        }
        let per = if one_each { 1 } else { (cursor.count as usize / targets.len()).max(1) as u8 };
        for slot in targets {
            if cursor.count == 0 {
                break;
            }
            let have = self.slots[slot].map_or(0, |s| s.count);
            let put = per.min(cursor.count).min(cursor.item.max_stack() - have);
            if put > 0 {
                self.slots[slot] = Some(ItemStack { count: have + put, ..cursor });
                cursor.count -= put;
            }
        }
        self.cursor = (cursor.count > 0).then_some(cursor);
        self.after_change();
    }

    /// Shift-click: hotbar <-> main inventory, armor/grid/furnace -> inventory, armor pieces equip
    /// themselves, smeltables and fuels go into an open furnace, outputs are collected.
    pub fn shift_click(&mut self, slot: usize) {
        if slot >= TOTAL_SLOTS {
            return;
        }
        if slot == CRAFT_OUT {
            self.craft_all();
            self.after_change();
            return;
        }
        let Some(stack) = self.slots[slot] else { return };
        self.slots[slot] = None;
        let leftover = if slot >= ARMOR_START {
            self.insert_into(0..INVENTORY_SLOTS, stack)
        } else if let Some(a) = stack.item.armor_slot()
            && self.slots[ARMOR_START + a].is_none()
        {
            self.fill_empty(ARMOR_START + a..ARMOR_START + a + 1, stack)
        } else if self.open_furnace.is_some() && self.accepts(FURNACE_IN, stack.item) {
            self.insert_into(FURNACE_IN..FURNACE_IN + 1, stack)
        } else if self.open_furnace.is_some() && self.accepts(FURNACE_FUEL, stack.item) {
            self.insert_into(FURNACE_FUEL..FURNACE_FUEL + 1, stack)
        } else if slot < HOTBAR_SLOTS {
            self.insert_into(HOTBAR_SLOTS..INVENTORY_SLOTS, stack)
        } else {
            self.insert_into(0..HOTBAR_SLOTS, stack)
        };
        if let Some(rest) = leftover {
            // No room at the destination: put the remainder back where it came from.
            self.slots[slot] = Some(rest);
        }
        self.after_change();
    }

    // -----------------------------------------------------------------------------------------
    // Crafting
    // -----------------------------------------------------------------------------------------

    fn grid(&self) -> [Option<Item>; GRID_CELLS] {
        let mut g = [None; GRID_CELLS];
        for (i, cell) in g.iter_mut().enumerate() {
            *cell = self.slots[CRAFT_START + i].map(|s| s.item);
        }
        g
    }

    /// Recomputes the preview in the crafting output slot.
    pub fn refresh_craft(&mut self) {
        let result = crafting::find_recipe(&self.grid(), self.craft_size).map(|r| r.result);
        if self.slots[CRAFT_OUT] != result {
            self.slots[CRAFT_OUT] = result;
        }
    }

    fn after_change(&mut self) {
        self.refresh_craft();
    }

    /// Uses up one of every ingredient in the grid.
    fn consume_ingredients(&mut self) {
        for i in 0..GRID_CELLS {
            let slot = &mut self.slots[CRAFT_START + i];
            if let Some(s) = slot {
                s.count -= 1;
                if s.count == 0 {
                    *slot = None;
                }
            }
        }
    }

    /// Switches between the 2x2 inventory grid and the 3x3 crafting table. Items lying in cells
    /// that do not exist in the smaller grid are returned; the second value is what did not fit.
    pub fn set_craft_size(&mut self, size: usize) -> Vec<ItemStack> {
        self.craft_size = size.clamp(2, 3);
        let mut overflow = Vec::new();
        if self.craft_size == 2 {
            for i in 0..GRID_CELLS {
                if i % 3 >= 2 || i / 3 >= 2 {
                    overflow.extend(self.move_to_inventory(CRAFT_START + i));
                }
            }
        }
        self.refresh_craft();
        overflow
    }

    fn move_to_inventory(&mut self, slot: usize) -> Option<ItemStack> {
        let stack = self.slots[slot].take()?;
        self.insert_into(0..INVENTORY_SLOTS, stack)
    }

    /// Empties the crafting grid into the inventory (screen closed). Returns what did not fit.
    pub fn clear_crafting(&mut self) -> Vec<ItemStack> {
        let mut overflow = Vec::new();
        for i in 0..GRID_CELLS {
            overflow.extend(self.move_to_inventory(CRAFT_START + i));
        }
        self.slots[CRAFT_OUT] = None;
        overflow
    }

    /// Takes the result of the crafting grid or the furnace onto the cursor.
    fn take_output(&mut self, slot: usize) {
        let Some(out) = self.slots[slot] else { return };
        let fits = match self.cursor {
            None => true,
            Some(c) => c.item == out.item && c.space_left() >= out.count,
        };
        if !fits {
            return;
        }
        self.cursor = Some(match self.cursor {
            Some(c) => ItemStack { count: c.count + out.count, ..c },
            None => out,
        });
        if slot == CRAFT_OUT {
            self.consume_ingredients();
        } else {
            self.slots[slot] = None;
        }
    }

    /// Shift-click on the crafting output: craft as often as the ingredients and the free space allow.
    fn craft_all(&mut self) {
        for _ in 0..(MAX_STACK as usize) {
            self.refresh_craft();
            let Some(out) = self.slots[CRAFT_OUT] else { break };
            if self.would_fit(out) {
                self.consume_ingredients();
                let _ = self.insert_into(0..INVENTORY_SLOTS, out);
            } else {
                break;
            }
        }
    }

    fn would_fit(&self, stack: ItemStack) -> bool {
        let mut free = 0u32;
        for s in &self.slots[..INVENTORY_SLOTS] {
            match s {
                None => free += stack.item.max_stack() as u32,
                Some(s) if s.item == stack.item => free += s.space_left() as u32,
                _ => {}
            }
        }
        free >= stack.count as u32
    }

    // -----------------------------------------------------------------------------------------
    // Furnaces
    // -----------------------------------------------------------------------------------------

    /// Opens the furnace at `pos`: its stacks move into the `FURNACE_*` slots.
    pub fn open_furnace(&mut self, pos: IVec3) {
        self.close_furnace();
        let state = self.furnaces.entry(pos).or_default();
        let (input, fuel, output) = (state.input.take(), state.fuel.take(), state.output.take());
        self.slots[FURNACE_IN] = input;
        self.slots[FURNACE_FUEL] = fuel;
        self.slots[FURNACE_OUT] = output;
        self.open_furnace = Some(pos);
    }

    /// Writes the open furnace's stacks back. Any stack on the cursor is not touched.
    pub fn close_furnace(&mut self) {
        let Some(pos) = self.open_furnace.take() else { return };
        let state = self.furnaces.entry(pos).or_default();
        state.input = self.slots[FURNACE_IN].take();
        state.fuel = self.slots[FURNACE_FUEL].take();
        state.output = self.slots[FURNACE_OUT].take();
    }

    /// State of the furnace at `pos`, including the stacks of the open one.
    pub fn furnace_state(&self, pos: IVec3) -> Option<FurnaceState> {
        let mut state = self.furnaces.get(&pos)?.clone();
        if self.open_furnace == Some(pos) {
            state.input = self.slots[FURNACE_IN];
            state.fuel = self.slots[FURNACE_FUEL];
            state.output = self.slots[FURNACE_OUT];
        }
        Some(state)
    }

    /// Advances every furnace by `dt` seconds. Returns `true` if a furnace changed visibly.
    pub fn tick_furnaces(&mut self, dt: f32) -> bool {
        let mut changed = false;
        let open = self.open_furnace;
        let positions: Vec<IVec3> = self.furnaces.keys().copied().collect();
        for pos in positions {
            let mut state = self.furnaces.remove(&pos).unwrap_or_default();
            if open == Some(pos) {
                state.input = self.slots[FURNACE_IN];
                state.fuel = self.slots[FURNACE_FUEL];
                state.output = self.slots[FURNACE_OUT];
            }
            if state.is_active() {
                state.tick(dt);
                changed = true;
            }
            if open == Some(pos) {
                self.slots[FURNACE_IN] = state.input.take();
                self.slots[FURNACE_FUEL] = state.fuel.take();
                self.slots[FURNACE_OUT] = state.output.take();
            }
            self.furnaces.insert(pos, state);
        }
        changed
    }

    /// Removes the furnace at `pos` (block broken) and returns what was inside.
    pub fn remove_furnace(&mut self, pos: IVec3) -> Vec<ItemStack> {
        if self.open_furnace == Some(pos) {
            self.close_furnace();
        }
        let Some(s) = self.furnaces.remove(&pos) else { return Vec::new() };
        [s.input, s.fuel, s.output].into_iter().flatten().collect()
    }

    pub fn furnace_saves(&self) -> Vec<FurnaceSave> {
        self.furnaces
            .keys()
            .filter_map(|p| Some(FurnaceSave { pos: p.to_array(), state: self.furnace_state(*p)? }))
            .filter(|s| !s.state.is_empty() || s.state.burn_left > 0.0)
            .collect()
    }

    pub fn load_furnaces(&mut self, saved: &[FurnaceSave]) {
        self.furnaces = saved.iter().map(|s| (IVec3::from_array(s.pos), s.state.clone())).collect();
        self.open_furnace = None;
    }

    // -----------------------------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------------------------

    /// Hands the cursor stack back to the inventory (screen closed). Whatever does not fit
    /// stays on the cursor so nothing is ever deleted.
    pub fn return_cursor(&mut self) {
        if let Some(c) = self.cursor.take() {
            self.cursor = self.add(c);
        }
    }

    /// Slot contents in save-file order: inventory, armor, off hand, then the cursor stack (so a
    /// quit mid-drag loses nothing). The crafting grid is not saved; its items are returned to
    /// the inventory whenever a screen closes.
    pub fn to_save(&self) -> Vec<Option<ItemStack>> {
        let mut v = self.slots[..SAVED_SLOTS].to_vec();
        v.push(self.cursor);
        v
    }

    /// Rebuilds an inventory from save data, dropping invalid items and clamping counts.
    pub fn from_save(saved: &[Option<ItemStack>], selected: usize) -> Self {
        let mut inv = Self::empty();
        let clean = |s: &Option<ItemStack>| {
            s.filter(|s| s.item.is_valid() && s.count > 0).map(|s| ItemStack {
                count: s.count.min(s.item.max_stack()),
                damage: s.damage.min(s.item.durability().unwrap_or(0)),
                ..s
            })
        };
        // Worlds saved before the off hand existed have 40 slots and the cursor right behind them.
        let (slot_count, cursor_at) =
            if saved.len() == LEGACY_SAVE_LEN { (ARMOR_START + ARMOR_SLOTS, ARMOR_START + ARMOR_SLOTS) } else { (SAVED_SLOTS, SAVED_SLOTS) };
        for (i, s) in saved.iter().take(slot_count).enumerate() {
            let s = clean(s);
            inv.slots[i] = s.filter(|s| inv.accepts(i, s.item));
        }
        inv.cursor = saved.get(cursor_at).and_then(clean);
        inv.return_cursor();
        inv.select(selected);
        inv
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::{Tier, ToolKind};
    use crate::world::block::BlockId;

    fn stack(b: BlockId, n: u8) -> Option<ItemStack> {
        Some(ItemStack::new(Item::block(b), n))
    }

    #[test]
    fn slot_layout_is_consistent() {
        assert_eq!(ARMOR_START, 36);
        assert_eq!(OFFHAND, 40);
        assert_eq!(CRAFT_START, 41);
        assert_eq!(CRAFT_OUT, 50);
        assert_eq!(TOTAL_SLOTS, 54);
        assert_eq!(Inventory::starter_kit().to_save().len(), SAVED_SLOTS + 1);
    }

    #[test]
    fn left_click_picks_up_places_and_swaps() {
        let mut inv = Inventory::empty();
        inv.slots[0] = stack(BlockId::DIRT, 10);
        inv.click(0, ClickButton::Left);
        assert_eq!((inv.cursor, inv.slots[0]), (stack(BlockId::DIRT, 10), None));
        inv.click(12, ClickButton::Left);
        assert_eq!((inv.cursor, inv.slots[12]), (None, stack(BlockId::DIRT, 10)));
        inv.slots[13] = stack(BlockId::STONE, 3);
        inv.click(12, ClickButton::Left);
        inv.click(13, ClickButton::Left);
        assert_eq!((inv.cursor, inv.slots[13]), (stack(BlockId::STONE, 3), stack(BlockId::DIRT, 10)));
    }

    #[test]
    fn same_items_merge_up_to_the_stack_limit() {
        let mut inv = Inventory::empty();
        inv.slots[0] = stack(BlockId::DIRT, 60);
        inv.slots[1] = stack(BlockId::DIRT, 10);
        inv.click(1, ClickButton::Left);
        inv.click(0, ClickButton::Left);
        assert_eq!(inv.slots[0], stack(BlockId::DIRT, 64));
        assert_eq!(inv.cursor, stack(BlockId::DIRT, 6));
        assert_eq!(inv.count_of(Item::block(BlockId::DIRT)), 64);
    }

    #[test]
    fn right_click_splits_in_half_and_places_one() {
        let mut inv = Inventory::empty();
        inv.slots[0] = stack(BlockId::STONE, 7);
        inv.click(0, ClickButton::Right);
        assert_eq!((inv.cursor, inv.slots[0]), (stack(BlockId::STONE, 4), stack(BlockId::STONE, 3)));
        inv.click(5, ClickButton::Right);
        assert_eq!((inv.cursor, inv.slots[5]), (stack(BlockId::STONE, 3), stack(BlockId::STONE, 1)));
        inv.click(5, ClickButton::Right);
        assert_eq!((inv.cursor, inv.slots[5]), (stack(BlockId::STONE, 2), stack(BlockId::STONE, 2)));
        // Splitting a single item takes it whole.
        inv.slots[9] = stack(BlockId::LOG, 1);
        inv.cursor = None;
        inv.click(9, ClickButton::Right);
        assert_eq!((inv.cursor, inv.slots[9]), (stack(BlockId::LOG, 1), None));
    }

    #[test]
    fn items_are_conserved_by_any_click_sequence() {
        let mut inv = Inventory::starter_kit();
        // Ingredients in the grid must not be lost by clicking either.
        inv.slots[CRAFT_START] = stack(BlockId::LOG, 5);
        let total = |i: &Inventory| {
            i.slots[..CRAFT_OUT].iter().chain([&i.cursor]).flatten().map(|s| s.count as u32).sum::<u32>()
        };
        let before = total(&inv);
        let mut seed = 12345u32;
        for _ in 0..4000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let slot = (seed >> 8) as usize % CRAFT_OUT;
            let button = if seed & 1 == 0 { ClickButton::Left } else { ClickButton::Right };
            if seed & 0x400 != 0 {
                inv.shift_click(slot);
            } else {
                inv.click(slot, button);
            }
            assert_eq!(total(&inv), before);
            assert!(inv.slots.iter().flatten().all(|s| s.count >= 1 && s.count <= s.item.max_stack()));
        }
    }

    #[test]
    fn dragging_distributes_the_cursor_stack() {
        let mut inv = Inventory::empty();
        inv.cursor = stack(BlockId::DIRT, 10);
        inv.slots[3] = stack(BlockId::STONE, 5); // occupied by something else: skipped
        inv.slots[4] = stack(BlockId::DIRT, 63);
        inv.distribute(&[0, 1, 2, 3, 4, 1], false);
        // 10 items over the four usable slots (0, 1, 2, 4): 2 each, the rest stays on the cursor.
        assert_eq!(inv.slots[0], stack(BlockId::DIRT, 2));
        assert_eq!(inv.slots[2], stack(BlockId::DIRT, 2));
        assert_eq!(inv.slots[3], stack(BlockId::STONE, 5));
        assert_eq!(inv.slots[4], stack(BlockId::DIRT, 64));
        assert_eq!(inv.cursor, stack(BlockId::DIRT, 3));

        let mut inv = Inventory::empty();
        inv.cursor = stack(BlockId::LOG, 3);
        inv.distribute(&[10, 11, 12, 13, 14], true);
        assert_eq!(inv.count_of(Item::block(BlockId::LOG)), 3);
        assert_eq!(inv.slots[12], stack(BlockId::LOG, 1));
        assert_eq!((inv.slots[13], inv.cursor), (None, None));
        // Armor slots take their piece only.
        let mut inv = Inventory::empty();
        inv.cursor = stack(BlockId::LOG, 2);
        inv.distribute(&[ARMOR_START, 5], false);
        assert_eq!((inv.slots[ARMOR_START], inv.slots[5], inv.cursor), (None, stack(BlockId::LOG, 2), None));
    }

    #[test]
    fn armor_slots_only_accept_matching_pieces() {
        let mut inv = Inventory::empty();
        inv.slots[0] = stack(BlockId::DIRT, 1);
        inv.slots[1] = Some(ItemStack::new(Item::IRON_BOOTS, 1));
        inv.click(0, ClickButton::Left);
        inv.click(ARMOR_START, ClickButton::Left);
        assert_eq!(inv.slots[ARMOR_START], None, "dirt is not a helmet");
        assert_eq!(inv.cursor, stack(BlockId::DIRT, 1));
        inv.click(0, ClickButton::Left);
        inv.click(1, ClickButton::Left);
        inv.click(ARMOR_START, ClickButton::Left);
        assert_eq!(inv.slots[ARMOR_START], None, "boots do not fit the helmet slot");
        inv.click(ARMOR_START + 3, ClickButton::Left);
        assert_eq!(inv.slots[ARMOR_START + 3], Some(ItemStack::new(Item::IRON_BOOTS, 1)));
    }

    #[test]
    fn the_off_hand_takes_anything_and_swaps_with_the_hotbar() {
        let mut inv = Inventory::empty();
        inv.slots[0] = stack(BlockId::TORCH, 12);
        inv.click(0, ClickButton::Left);
        inv.click(OFFHAND, ClickButton::Left);
        assert_eq!(inv.offhand(), stack(BlockId::TORCH, 12));
        inv.slots[0] = stack(BlockId::STONE, 1);
        inv.swap_offhand();
        assert_eq!((inv.slots[0], inv.offhand()), (stack(BlockId::TORCH, 12), stack(BlockId::STONE, 1)));
    }

    #[test]
    fn add_tops_up_then_fills_empty_slots_and_reports_overflow() {
        let mut inv = Inventory::empty();
        inv.slots[3] = stack(BlockId::SAND, 60);
        assert_eq!(inv.add(ItemStack::new(Item::block(BlockId::SAND), 10)), None);
        assert_eq!(inv.slots[3], stack(BlockId::SAND, 64));
        assert_eq!(inv.slots[0], stack(BlockId::SAND, 6));
        let mut full = Inventory::empty();
        for s in 0..INVENTORY_SLOTS {
            full.slots[s] = stack(BlockId::STONE, 64);
        }
        let rest = full.add(ItemStack::new(Item::block(BlockId::DIRT), 5));
        assert_eq!(rest, Some(ItemStack::new(Item::block(BlockId::DIRT), 5)));
    }

    #[test]
    fn shift_click_moves_between_hotbar_and_main_and_equips_armor() {
        let mut inv = Inventory::empty();
        inv.slots[2] = stack(BlockId::PLANKS, 20);
        inv.shift_click(2);
        assert_eq!((inv.slots[2], inv.slots[HOTBAR_SLOTS]), (None, stack(BlockId::PLANKS, 20)));
        inv.shift_click(HOTBAR_SLOTS);
        assert_eq!((inv.slots[HOTBAR_SLOTS], inv.slots[0]), (None, stack(BlockId::PLANKS, 20)));
        inv.slots[20] = Some(ItemStack::new(Item::IRON_CHESTPLATE, 1));
        inv.shift_click(20);
        assert_eq!(inv.slots[ARMOR_START + 1], Some(ItemStack::new(Item::IRON_CHESTPLATE, 1)));
        inv.shift_click(ARMOR_START + 1);
        assert_eq!(inv.slots[ARMOR_START + 1], None);
        assert_eq!(inv.count_of(Item::IRON_CHESTPLATE), 1);
    }

    #[test]
    fn placing_consumes_one_and_clears_empty_slots() {
        let mut inv = Inventory::empty();
        inv.slots[0] = stack(BlockId::DIRT, 2);
        assert_eq!(inv.take_one_selected(), Some(Item::block(BlockId::DIRT)));
        assert_eq!(inv.take_one_selected(), Some(Item::block(BlockId::DIRT)));
        assert_eq!(inv.slots[0], None);
        assert_eq!(inv.take_one_selected(), None);
    }

    #[test]
    fn tools_wear_out_and_vanish() {
        let mut inv = Inventory::empty();
        inv.slots[0] = Some(ItemStack::new(Item::tool(ToolKind::Shovel, Tier::Gold), 1));
        assert!(!inv.wear_selected(31));
        assert_eq!(inv.slots[0].unwrap().damage, 31);
        assert!(inv.wear_selected(1));
        assert_eq!(inv.slots[0], None);
        // Damaged tools never merge with anything.
        let mut inv = Inventory::empty();
        inv.slots[0] = Some(ItemStack { damage: 7, ..ItemStack::new(Item::tool(ToolKind::Axe, Tier::Wood), 1) });
        assert!(inv.add(ItemStack::new(Item::tool(ToolKind::Axe, Tier::Wood), 1)).is_none());
        assert_eq!(inv.slots[1].unwrap().damage, 0);
    }

    #[test]
    fn crafting_grid_produces_and_consumes() {
        let mut inv = Inventory::empty();
        inv.slots[CRAFT_START] = stack(BlockId::LOG, 2);
        inv.refresh_craft();
        assert_eq!(inv.slots[CRAFT_OUT], stack(BlockId::PLANKS, 4));
        // Take one craft onto the cursor.
        inv.click(CRAFT_OUT, ClickButton::Left);
        assert_eq!(inv.cursor, stack(BlockId::PLANKS, 4));
        assert_eq!(inv.slots[CRAFT_START], stack(BlockId::LOG, 1));
        assert_eq!(inv.slots[CRAFT_OUT], stack(BlockId::PLANKS, 4), "the next craft is ready");
        // A different item on the cursor blocks taking.
        inv.cursor = stack(BlockId::DIRT, 1);
        inv.click(CRAFT_OUT, ClickButton::Left);
        assert_eq!(inv.cursor, stack(BlockId::DIRT, 1));
        // Same item on the cursor adds up.
        inv.cursor = stack(BlockId::PLANKS, 10);
        inv.click(CRAFT_OUT, ClickButton::Left);
        assert_eq!(inv.cursor, stack(BlockId::PLANKS, 14));
        assert_eq!((inv.slots[CRAFT_START], inv.slots[CRAFT_OUT]), (None, None));
        // The output slot never accepts items.
        inv.click(CRAFT_OUT, ClickButton::Left);
        assert_eq!(inv.slots[CRAFT_OUT], None);
    }

    #[test]
    fn shift_clicking_the_output_crafts_everything() {
        let mut inv = Inventory::empty();
        inv.slots[CRAFT_START + 1] = stack(BlockId::LOG, 5);
        inv.refresh_craft();
        inv.shift_click(CRAFT_OUT);
        assert_eq!(inv.count_of(Item::block(BlockId::PLANKS)), 20);
        assert_eq!(inv.slots[CRAFT_START + 1], None);
        assert_eq!(inv.slots[CRAFT_OUT], None);
        // With a full inventory crafting stops instead of losing items.
        let mut full = Inventory::empty();
        for s in 0..INVENTORY_SLOTS {
            full.slots[s] = stack(BlockId::STONE, 64);
        }
        full.slots[CRAFT_START] = stack(BlockId::LOG, 3);
        full.refresh_craft();
        full.shift_click(CRAFT_OUT);
        assert_eq!(full.slots[CRAFT_START], stack(BlockId::LOG, 3));
    }

    #[test]
    fn the_small_grid_ignores_outer_cells_and_gives_them_back() {
        let mut inv = Inventory::empty();
        inv.set_craft_size(3);
        inv.slots[CRAFT_START + 8] = stack(BlockId::LOG, 1);
        inv.refresh_craft();
        assert_eq!(inv.slots[CRAFT_OUT], stack(BlockId::PLANKS, 4));
        let overflow = inv.set_craft_size(2);
        assert!(overflow.is_empty());
        assert_eq!(inv.slots[CRAFT_START + 8], None);
        assert_eq!(inv.slots[CRAFT_OUT], None);
        assert_eq!(inv.count_of(Item::block(BlockId::LOG)), 1);
        // Closing a screen sends the remaining ingredients home.
        inv.slots[CRAFT_START] = stack(BlockId::COBBLE, 3);
        assert!(inv.clear_crafting().is_empty());
        assert_eq!(inv.count_of(Item::block(BlockId::COBBLE)), 3);
    }

    #[test]
    fn furnace_slots_follow_the_open_furnace() {
        let pos = IVec3::new(4, 70, 4);
        let mut inv = Inventory::empty();
        inv.slots[0] = Some(ItemStack::new(Item::block(BlockId::IRON_ORE), 2));
        inv.slots[1] = Some(ItemStack::new(Item::COAL, 1));
        // Nothing can go in while no furnace is open.
        inv.shift_click(0);
        assert_eq!(inv.slots[FURNACE_IN], None);
        inv.slots[0] = Some(ItemStack::new(Item::block(BlockId::IRON_ORE), 2));
        inv.open_furnace(pos);
        inv.shift_click(0);
        inv.shift_click(1);
        assert_eq!(inv.slots[FURNACE_IN].map(|s| s.count), Some(2));
        assert_eq!(inv.slots[FURNACE_FUEL].map(|s| s.item), Some(Item::COAL));
        // Stone is neither smeltable nor fuel.
        inv.slots[2] = stack(BlockId::GRASS, 1);
        inv.click(2, ClickButton::Left);
        inv.click(FURNACE_IN, ClickButton::Left);
        assert_eq!(inv.slots[FURNACE_IN].map(|s| s.count), Some(2));
        inv.return_cursor();
        for _ in 0..120 {
            inv.tick_furnaces(0.1);
        }
        assert_eq!(inv.slots[FURNACE_OUT], Some(ItemStack::new(Item::IRON_INGOT, 1)));
        // Collect the ingot, close the screen: the rest stays inside the furnace and keeps cooking.
        inv.click(FURNACE_OUT, ClickButton::Left);
        assert_eq!(inv.cursor, Some(ItemStack::new(Item::IRON_INGOT, 1)));
        inv.close_furnace();
        assert_eq!(inv.slots[FURNACE_IN], None);
        for _ in 0..200 {
            inv.tick_furnaces(0.1);
        }
        let state = inv.furnace_state(pos).unwrap();
        assert_eq!(state.output.map(|s| s.count), Some(1));
        // Breaking it returns everything.
        let drops = inv.remove_furnace(pos);
        assert_eq!(drops, vec![ItemStack::new(Item::IRON_INGOT, 1)]);
        assert!(inv.furnace_state(pos).is_none());
    }

    #[test]
    fn furnaces_survive_a_save_roundtrip() {
        let pos = IVec3::new(-3, 64, 9);
        let mut inv = Inventory::empty();
        inv.open_furnace(pos);
        inv.slots[FURNACE_IN] = Some(ItemStack::new(Item::RAW_PORK, 4));
        inv.slots[FURNACE_FUEL] = Some(ItemStack::new(Item::COAL, 2));
        let saves = inv.furnace_saves();
        assert_eq!(saves.len(), 1, "the open furnace is saved with its current slots");
        let json = serde_json::to_string(&saves).unwrap();
        let back: Vec<FurnaceSave> = serde_json::from_str(&json).unwrap();
        let mut restored = Inventory::empty();
        restored.load_furnaces(&back);
        assert_eq!(restored.furnace_state(pos).unwrap().input, Some(ItemStack::new(Item::RAW_PORK, 4)));
    }

    #[test]
    fn drops_follow_minecraft_rules_for_every_block() {
        for id in crate::world::block::BlockId::all() {
            if let Some(stack) = Item::loot(id, true, 0) {
                assert!(stack.item.as_block().is_some() || stack.item.is_valid(), "{}", id.name());
            }
        }
    }

    #[test]
    fn save_roundtrip_keeps_everything_and_sanitises_garbage() {
        let mut inv = Inventory::starter_kit();
        inv.selected = 4;
        inv.cursor = stack(BlockId::LOG, 12);
        inv.slots[ARMOR_START] = Some(ItemStack::new(Item::IRON_HELMET, 1));
        inv.slots[OFFHAND] = stack(BlockId::TORCH, 9);
        inv.slots[HOTBAR_SLOTS + 8] = Some(ItemStack { damage: 12, ..ItemStack::new(Item::tool(ToolKind::Axe, Tier::Iron), 1) });
        let json = serde_json::to_string(&inv.to_save()).unwrap();
        let back: Vec<Option<ItemStack>> = serde_json::from_str(&json).unwrap();
        let restored = Inventory::from_save(&back, 4);
        // The cursor stack goes back into the inventory instead of being lost.
        assert_eq!(restored.cursor, None);
        assert_eq!(restored.selected, 4);
        assert_eq!(restored.count_of(Item::block(BlockId::LOG)), inv.count_of(Item::block(BlockId::LOG)) + 12);
        assert_eq!(restored.slots[ARMOR_START], inv.slots[ARMOR_START]);
        assert_eq!(restored.slots[OFFHAND], inv.slots[OFFHAND]);
        assert_eq!(restored.slots[HOTBAR_SLOTS + 8], inv.slots[HOTBAR_SLOTS + 8], "tool wear survives");

        let bad = vec![Some(ItemStack::new(Item(2000), 5)), Some(ItemStack::new(Item::block(BlockId::DIRT), 250))];
        let cleaned = Inventory::from_save(&bad, 99);
        assert_eq!(cleaned.slots[0], None);
        assert_eq!(cleaned.slots[1], stack(BlockId::DIRT, 64));
        assert_eq!(cleaned.selected, HOTBAR_SLOTS - 1);
    }

    #[test]
    fn worlds_saved_before_the_off_hand_still_load() {
        // 40 slots + cursor, as written by earlier versions.
        let mut legacy = vec![None; 41];
        legacy[0] = stack(BlockId::DIRT, 5);
        legacy[ARMOR_START] = Some(ItemStack::new(Item::IRON_HELMET, 1));
        legacy[40] = stack(BlockId::LOG, 3); // the cursor
        let inv = Inventory::from_save(&legacy, 0);
        assert_eq!(inv.slots[0], stack(BlockId::DIRT, 5));
        assert_eq!(inv.slots[ARMOR_START], Some(ItemStack::new(Item::IRON_HELMET, 1)));
        assert_eq!(inv.slots[OFFHAND], None, "the old cursor must not land in the off hand");
        assert_eq!(inv.count_of(Item::block(BlockId::LOG)), 3);
        // Item ids of the old format deserialize unchanged.
        let json = r#"[{"item":6,"count":4},null]"#;
        let parsed: Vec<Option<ItemStack>> = serde_json::from_str(json).unwrap();
        assert_eq!(parsed[0], stack(BlockId::LOG, 4));
    }
}
