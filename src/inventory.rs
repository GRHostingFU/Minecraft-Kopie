//! Items, stacks and the player inventory, including the Minecraft-style click
//! rules (pick up, place, merge, split, swap, shift-click). Pure data and logic —
//! no Bevy dependency except the `Resource` derive — so it is fully unit-tested.

use crate::world::block::tile;
use crate::world::block::{BlockId, Face};
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

pub const HOTBAR_SLOTS: usize = 9;
/// Hotbar (slots `0..9`) plus the 27 slots of the main inventory (`9..36`).
pub const INVENTORY_SLOTS: usize = 36;
pub const ARMOR_SLOTS: usize = 4;
/// Inventory slots followed by the armor slots (`36..40`: helmet, chestplate, leggings, boots).
pub const TOTAL_SLOTS: usize = INVENTORY_SLOTS + ARMOR_SLOTS;
pub const ARMOR_START: usize = INVENTORY_SLOTS;
pub const MAX_STACK: u8 = 64;

/// What a stack contains. Block items reuse the block id; armor pieces start at 100.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Item(pub u8);

impl Item {
    pub const IRON_HELMET: Item = Item(100);
    pub const IRON_CHESTPLATE: Item = Item(101);
    pub const IRON_LEGGINGS: Item = Item(102);
    pub const IRON_BOOTS: Item = Item(103);

    pub const fn block(block: BlockId) -> Item {
        Item(block.0)
    }

    /// The block this item places, if it is a placeable block.
    pub fn as_block(self) -> Option<BlockId> {
        let b = BlockId(self.0);
        (self.0 != 0 && self.0 < 100 && b.is_targetable() && b.name() != "Unknown").then_some(b)
    }

    /// Which armor slot (0 = helmet .. 3 = boots) this item fits, if it is armor.
    pub fn armor_slot(self) -> Option<usize> {
        (100..104).contains(&self.0).then(|| (self.0 - 100) as usize)
    }

    /// Items loaded from disk are validated so a corrupt or newer save cannot inject unknown ids.
    pub fn is_valid(self) -> bool {
        self.as_block().is_some() || self.armor_slot().is_some()
    }

    pub fn max_stack(self) -> u8 {
        if self.armor_slot().is_some() { 1 } else { MAX_STACK }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::IRON_HELMET => "Iron Helmet",
            Self::IRON_CHESTPLATE => "Iron Chestplate",
            Self::IRON_LEGGINGS => "Iron Leggings",
            Self::IRON_BOOTS => "Iron Boots",
            other => other.as_block().map_or("Unknown", BlockId::name),
        }
    }

    /// Atlas tile used for slot icons.
    pub fn icon_tile(self) -> u8 {
        match self.armor_slot() {
            Some(slot) => tile::HELMET + slot as u8,
            None => self.as_block().map_or(tile::STONE, |b| b.tile(Face::Side)),
        }
    }

    /// What breaking `block` yields (Minecraft rules: stone drops cobblestone, grass drops dirt).
    pub fn drop_for(block: BlockId) -> Option<Item> {
        if !block.is_breakable() {
            return None;
        }
        Some(match block {
            BlockId::STONE => Item::block(BlockId::COBBLE),
            BlockId::GRASS => Item::block(BlockId::DIRT),
            other => Item::block(other),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ItemStack {
    pub item: Item,
    pub count: u8,
}

impl ItemStack {
    pub fn new(item: Item, count: u8) -> Self {
        Self { item, count }
    }

    pub fn space_left(&self) -> u8 {
        self.item.max_stack().saturating_sub(self.count)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClickButton {
    Left,
    Right,
}

/// Slot contents plus the stack currently attached to the mouse cursor.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Inventory {
    pub slots: [Option<ItemStack>; TOTAL_SLOTS],
    pub selected: usize,
    /// The stack "in the hand" while the inventory screen is open.
    pub cursor: Option<ItemStack>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self::starter_kit()
    }
}

impl Inventory {
    pub fn empty() -> Self {
        Self { slots: [None; TOTAL_SLOTS], selected: 0, cursor: None }
    }

    /// What a fresh world starts with: building blocks in the hotbar and a spare armor set.
    pub fn starter_kit() -> Self {
        let mut inv = Self::empty();
        let blocks = [BlockId::DIRT, BlockId::STONE, BlockId::PLANKS, BlockId::LOG, BlockId::COBBLE, BlockId::SAND];
        for (i, b) in blocks.into_iter().enumerate() {
            inv.slots[i] = Some(ItemStack::new(Item::block(b), MAX_STACK));
        }
        inv.slots[HOTBAR_SLOTS] = Some(ItemStack::new(Item::IRON_HELMET, 1));
        inv.slots[HOTBAR_SLOTS + 1] = Some(ItemStack::new(Item::IRON_CHESTPLATE, 1));
        inv.slots[HOTBAR_SLOTS + 2] = Some(ItemStack::new(Item::IRON_LEGGINGS, 1));
        inv.slots[HOTBAR_SLOTS + 3] = Some(ItemStack::new(Item::IRON_BOOTS, 1));
        inv.slots[HOTBAR_SLOTS + 4] = Some(ItemStack::new(Item::block(BlockId::SNOW), 32));
        inv.slots[HOTBAR_SLOTS + 5] = Some(ItemStack::new(Item::block(BlockId::LEAVES), 32));
        inv
    }

    /// Whether `slot` may hold `item` (armor slots only take their own piece).
    pub fn accepts(slot: usize, item: Item) -> bool {
        match slot {
            0..INVENTORY_SLOTS => true,
            ARMOR_START..TOTAL_SLOTS => item.armor_slot() == Some(slot - ARMOR_START),
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

    pub fn select(&mut self, slot: usize) {
        self.selected = slot.min(HOTBAR_SLOTS - 1);
    }

    pub fn cycle(&mut self, steps: i32) {
        self.selected = (self.selected as i32 + steps).rem_euclid(HOTBAR_SLOTS as i32) as usize;
    }

    /// Removes one item from the selected slot (placing a block). Returns what was taken.
    pub fn take_one_selected(&mut self) -> Option<Item> {
        let slot = &mut self.slots[self.selected];
        let stack = slot.as_mut()?;
        let item = stack.item;
        stack.count -= 1;
        if stack.count == 0 {
            *slot = None;
        }
        Some(item)
    }

    /// Picks an item up: tops up existing stacks first, then fills empty slots, hotbar first.
    /// Returns what did not fit.
    pub fn add(&mut self, stack: ItemStack) -> Option<ItemStack> {
        let stack = self.merge_into(0..INVENTORY_SLOTS, stack)?;
        self.fill_empty(0..INVENTORY_SLOTS, stack)
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
            if self.slots[i].is_none() && Self::accepts(i, stack.item) {
                let moved = stack.item.max_stack().min(stack.count);
                self.slots[i] = Some(ItemStack::new(stack.item, moved));
                stack.count -= moved;
                if stack.count == 0 {
                    return None;
                }
            }
        }
        Some(stack)
    }

    /// Total number of `item`s in the inventory slots (not the cursor).
    pub fn count_of(&self, item: Item) -> u32 {
        self.slots.iter().flatten().filter(|s| s.item == item).map(|s| s.count as u32).sum()
    }

    /// Mouse click on a slot, following the Minecraft rules.
    pub fn click(&mut self, slot: usize, button: ClickButton) {
        if slot >= TOTAL_SLOTS {
            return;
        }
        match (self.cursor, self.slots[slot]) {
            (None, None) => {}
            (None, Some(s)) => {
                let take = match button {
                    ClickButton::Left => s.count,
                    ClickButton::Right => s.count.div_ceil(2),
                };
                self.cursor = Some(ItemStack::new(s.item, take));
                self.slots[slot] = (s.count > take).then(|| ItemStack::new(s.item, s.count - take));
            }
            (Some(c), None) => {
                if !Self::accepts(slot, c.item) {
                    return;
                }
                let put = match button {
                    ClickButton::Left => c.count.min(c.item.max_stack()),
                    ClickButton::Right => 1,
                };
                self.slots[slot] = Some(ItemStack::new(c.item, put));
                self.cursor = (c.count > put).then(|| ItemStack::new(c.item, c.count - put));
            }
            (Some(c), Some(s)) if c.item == s.item => {
                let moved = match button {
                    ClickButton::Left => s.space_left().min(c.count),
                    ClickButton::Right => s.space_left().min(1),
                };
                self.slots[slot] = Some(ItemStack::new(s.item, s.count + moved));
                self.cursor = (c.count > moved).then(|| ItemStack::new(c.item, c.count - moved));
            }
            (Some(c), Some(s)) => {
                // Different items: swap, if the slot accepts the carried one.
                if Self::accepts(slot, c.item) && c.count <= c.item.max_stack() {
                    self.slots[slot] = Some(c);
                    self.cursor = Some(s);
                }
            }
        }
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
                && Self::accepts(slot, cursor.item)
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
                self.slots[slot] = Some(ItemStack::new(cursor.item, have + put));
                cursor.count -= put;
            }
        }
        self.cursor = (cursor.count > 0).then_some(cursor);
    }

    /// Shift-click: hotbar <-> main inventory, armor <-> inventory, armor pieces equip themselves.
    pub fn shift_click(&mut self, slot: usize) {
        if slot >= TOTAL_SLOTS {
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
        } else if slot < HOTBAR_SLOTS {
            self.insert_into(HOTBAR_SLOTS..INVENTORY_SLOTS, stack)
        } else {
            self.insert_into(0..HOTBAR_SLOTS, stack)
        };
        if let Some(rest) = leftover {
            // No room at the destination: put the remainder back where it came from.
            self.slots[slot] = Some(rest);
        }
    }

    fn insert_into(&mut self, range: std::ops::Range<usize>, stack: ItemStack) -> Option<ItemStack> {
        let stack = self.merge_into(range.clone(), stack)?;
        self.fill_empty(range, stack)
    }

    /// Hands the cursor stack back to the inventory (screen closed). Whatever does not fit
    /// stays on the cursor so nothing is ever deleted.
    pub fn return_cursor(&mut self) {
        if let Some(c) = self.cursor.take() {
            self.cursor = self.add(c);
        }
    }

    /// Slot contents in save-file order (cursor stack included so a quit mid-drag loses nothing).
    pub fn to_save(&self) -> Vec<Option<ItemStack>> {
        let mut v = self.slots.to_vec();
        v.push(self.cursor);
        v
    }

    /// Rebuilds an inventory from save data, dropping invalid items and clamping counts.
    pub fn from_save(saved: &[Option<ItemStack>], selected: usize) -> Self {
        let mut inv = Self::empty();
        let clean = |s: &Option<ItemStack>| {
            s.filter(|s| s.item.is_valid() && s.count > 0)
                .map(|s| ItemStack::new(s.item, s.count.min(s.item.max_stack())))
        };
        for (i, s) in saved.iter().take(TOTAL_SLOTS).enumerate() {
            let s = clean(s);
            inv.slots[i] = s.filter(|s| Self::accepts(i, s.item));
        }
        inv.cursor = saved.get(TOTAL_SLOTS).and_then(clean);
        inv.return_cursor();
        inv.select(selected);
        inv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(b: BlockId, n: u8) -> Option<ItemStack> {
        Some(ItemStack::new(Item::block(b), n))
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
        let total = |i: &Inventory| i.slots.iter().chain([&i.cursor]).flatten().map(|s| s.count as u32).sum::<u32>();
        let before = total(&inv);
        let mut seed = 12345u32;
        for _ in 0..2000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let slot = (seed >> 8) as usize % TOTAL_SLOTS;
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
    fn drops_follow_minecraft_rules() {
        assert_eq!(Item::drop_for(BlockId::STONE), Some(Item::block(BlockId::COBBLE)));
        assert_eq!(Item::drop_for(BlockId::GRASS), Some(Item::block(BlockId::DIRT)));
        assert_eq!(Item::drop_for(BlockId::LOG), Some(Item::block(BlockId::LOG)));
        assert_eq!(Item::drop_for(BlockId::BEDROCK), None);
        assert_eq!(Item::drop_for(BlockId::WATER), None);
        for id in 1..=11u8 {
            if let Some(item) = Item::drop_for(BlockId(id)) {
                assert!(item.as_block().is_some(), "drop of block {id} must be placeable");
            }
        }
    }

    #[test]
    fn save_roundtrip_keeps_everything_and_sanitises_garbage() {
        let mut inv = Inventory::starter_kit();
        inv.selected = 4;
        inv.cursor = stack(BlockId::LOG, 12);
        inv.slots[ARMOR_START] = Some(ItemStack::new(Item::IRON_HELMET, 1));
        let json = serde_json::to_string(&inv.to_save()).unwrap();
        let back: Vec<Option<ItemStack>> = serde_json::from_str(&json).unwrap();
        let restored = Inventory::from_save(&back, 4);
        // The cursor stack goes back into the inventory instead of being lost.
        assert_eq!(restored.cursor, None);
        assert_eq!(restored.selected, 4);
        assert_eq!(restored.count_of(Item::block(BlockId::LOG)), inv.count_of(Item::block(BlockId::LOG)) + 12);
        assert_eq!(restored.slots[ARMOR_START], inv.slots[ARMOR_START]);

        let bad = vec![Some(ItemStack::new(Item(200), 5)), Some(ItemStack::new(Item::block(BlockId::DIRT), 250))];
        let cleaned = Inventory::from_save(&bad, 99);
        assert_eq!(cleaned.slots[0], None);
        assert_eq!(cleaned.slots[1], stack(BlockId::DIRT, 64));
        assert_eq!(cleaned.selected, HOTBAR_SLOTS - 1);
    }
}
