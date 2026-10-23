//! The creative block directory: every item of the game, sorted into tabs.

use crate::items::{Category, Item, Tier, ToolKind};
use crate::world::block::BlockId;
use std::sync::OnceLock;

pub struct Tab {
    pub name: &'static str,
    /// Item whose icon is drawn on the tab.
    pub icon: Item,
    pub items: Vec<Item>,
}

fn build() -> Vec<Tab> {
    let tab = |name, icon, cat: Category| Tab { name, icon, items: Item::all().filter(|i| i.category() == cat).collect() };
    vec![
        tab("Building Blocks", Item::block(BlockId::BRICKS), Category::Building),
        tab("Natural Blocks", Item::block(BlockId::GRASS), Category::Natural),
        tab("Decoration", Item::block(BlockId::TORCH), Category::Decoration),
        tab("Tools & Armor", Item::tool(ToolKind::Pickaxe, Tier::Iron), Category::Tools),
        tab("Materials & Food", Item::DIAMOND, Category::Materials),
    ]
}

/// The creative tabs, built once.
pub fn tabs() -> &'static [Tab] {
    static TABS: OnceLock<Vec<Tab>> = OnceLock::new();
    TABS.get_or_init(build)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_appears_in_exactly_one_tab() {
        let mut seen = std::collections::HashSet::new();
        for tab in tabs() {
            assert!(!tab.items.is_empty(), "{} is empty", tab.name);
            for item in &tab.items {
                assert!(seen.insert(*item), "{item:?} listed twice");
            }
        }
        assert_eq!(seen.len(), Item::all().count());
    }

    #[test]
    fn the_directory_contains_the_requested_blocks() {
        let all: Vec<Item> = tabs().iter().flat_map(|t| t.items.iter().copied()).collect();
        for b in [
            BlockId::COAL_ORE,
            BlockId::IRON_ORE,
            BlockId::GOLD_ORE,
            BlockId::DIAMOND_ORE,
            BlockId::BRICKS,
            BlockId::GLASS,
            BlockId::FENCE,
            BlockId::TORCH,
            BlockId::NETHER_BRICKS,
            BlockId::GRANITE,
        ] {
            assert!(all.contains(&Item::block(b)), "{} missing", b.name());
        }
    }
}
