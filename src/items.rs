//! Everything that can sit in an inventory slot: block items, raw materials, food, tools and armor,
//! together with the rules that tie them to the world (mining speed, loot, durability, fuel).

use crate::world::block::{BlockId, Face, tile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ToolKind {
    Pickaxe,
    Axe,
    Shovel,
    Sword,
}

impl ToolKind {
    pub const ALL: [ToolKind; 4] = [ToolKind::Pickaxe, ToolKind::Axe, ToolKind::Shovel, ToolKind::Sword];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Pickaxe => "Pickaxe",
            ToolKind::Axe => "Axe",
            ToolKind::Shovel => "Shovel",
            ToolKind::Sword => "Sword",
        }
    }

    fn index(self) -> u16 {
        self as u16
    }

    /// Damage dealt to a mob before the material bonus.
    fn base_damage(self) -> f32 {
        match self {
            ToolKind::Sword => 4.0,
            ToolKind::Axe => 3.0,
            ToolKind::Pickaxe => 2.0,
            ToolKind::Shovel => 1.5,
        }
    }
}

/// Tool material.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tier {
    Wood,
    Stone,
    Iron,
    Gold,
    Diamond,
}

impl Tier {
    pub const ALL: [Tier; 5] = [Tier::Wood, Tier::Stone, Tier::Iron, Tier::Gold, Tier::Diamond];

    pub fn label(self) -> &'static str {
        match self {
            Tier::Wood => "Wooden",
            Tier::Stone => "Stone",
            Tier::Iron => "Iron",
            Tier::Gold => "Golden",
            Tier::Diamond => "Diamond",
        }
    }

    fn index(self) -> u16 {
        self as u16
    }

    /// Mining speed multiplier on blocks the tool is made for.
    pub fn speed(self) -> f32 {
        [2.0, 4.0, 6.0, 12.0, 8.0][self as usize]
    }

    /// Highest block `harvest_level` the tool can still collect drops from.
    pub fn harvest_level(self) -> u8 {
        [0, 1, 2, 0, 3][self as usize]
    }

    pub fn durability(self) -> u16 {
        [59, 131, 250, 32, 1561][self as usize]
    }

    fn damage_bonus(self) -> f32 {
        [0.0, 1.0, 2.0, 0.0, 3.0][self as usize]
    }
}

/// What a stack contains. Block items reuse the block id (`1..BlockId::COUNT`), armor pieces start
/// at 100, everything else at 256.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Item(pub u16);

/// How an item behaves when used.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ItemKind {
    Block(BlockId),
    Material,
    Food { hunger: u8, saturation: f32 },
    Tool(ToolKind, Tier),
    Armor { slot: usize, defense: u8 },
}

/// Inventory sections of the creative catalogue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Building,
    Natural,
    Decoration,
    Tools,
    Materials,
}

impl Item {
    pub const IRON_HELMET: Item = Item(100);
    pub const IRON_CHESTPLATE: Item = Item(101);
    pub const IRON_LEGGINGS: Item = Item(102);
    pub const IRON_BOOTS: Item = Item(103);

    pub const STICK: Item = Item(256);
    pub const COAL: Item = Item(257);
    pub const IRON_INGOT: Item = Item(258);
    pub const GOLD_INGOT: Item = Item(259);
    pub const DIAMOND: Item = Item(260);
    pub const REDSTONE: Item = Item(261);
    pub const APPLE: Item = Item(262);
    pub const BREAD: Item = Item(263);
    pub const RAW_PORK: Item = Item(264);
    pub const COOKED_PORK: Item = Item(265);
    pub const RAW_BEEF: Item = Item(266);
    pub const COOKED_BEEF: Item = Item(267);
    pub const ROTTEN_FLESH: Item = Item(268);
    const LAST_MATERIAL: u16 = 268;
    const TOOL_BASE: u16 = 300;

    pub const fn block(block: BlockId) -> Item {
        Item(block.0 as u16)
    }

    pub const fn tool(kind: ToolKind, tier: Tier) -> Item {
        Item(Self::TOOL_BASE + tier as u16 * 4 + kind as u16)
    }

    /// The block this item places, if it is a placeable block.
    pub fn as_block(self) -> Option<BlockId> {
        (self.0 != 0 && self.0 < BlockId::COUNT as u16).then_some(BlockId(self.0 as u8))
    }

    pub fn kind(self) -> Option<ItemKind> {
        if let Some(b) = self.as_block() {
            return Some(ItemKind::Block(b));
        }
        Some(match self.0 {
            100..=103 => {
                let slot = (self.0 - 100) as usize;
                ItemKind::Armor { slot, defense: [2, 6, 5, 2][slot] }
            }
            256..=Self::LAST_MATERIAL => match self {
                Self::APPLE => ItemKind::Food { hunger: 4, saturation: 2.4 },
                Self::BREAD => ItemKind::Food { hunger: 5, saturation: 6.0 },
                Self::RAW_PORK | Self::RAW_BEEF => ItemKind::Food { hunger: 3, saturation: 1.8 },
                Self::COOKED_PORK | Self::COOKED_BEEF => ItemKind::Food { hunger: 8, saturation: 12.8 },
                Self::ROTTEN_FLESH => ItemKind::Food { hunger: 4, saturation: 0.8 },
                _ => ItemKind::Material,
            },
            300..=319 => {
                let i = self.0 - Self::TOOL_BASE;
                ItemKind::Tool(ToolKind::ALL[(i % 4) as usize], Tier::ALL[(i / 4) as usize])
            }
            _ => return None,
        })
    }

    /// Armor slot (0 = helmet .. 3 = boots) this item fits, if it is armor.
    pub fn armor_slot(self) -> Option<usize> {
        match self.kind() {
            Some(ItemKind::Armor { slot, .. }) => Some(slot),
            _ => None,
        }
    }

    pub fn tool_info(self) -> Option<(ToolKind, Tier)> {
        match self.kind() {
            Some(ItemKind::Tool(k, t)) => Some((k, t)),
            _ => None,
        }
    }

    pub fn food(self) -> Option<(u8, f32)> {
        match self.kind() {
            Some(ItemKind::Food { hunger, saturation }) => Some((hunger, saturation)),
            _ => None,
        }
    }

    /// Items loaded from disk are validated so a corrupt or newer save cannot inject unknown ids.
    pub fn is_valid(self) -> bool {
        self.kind().is_some()
    }

    pub fn max_stack(self) -> u8 {
        match self.kind() {
            Some(ItemKind::Armor { .. } | ItemKind::Tool(..)) => 1,
            _ => 64,
        }
    }

    /// Uses of a tool before it breaks (armor is not damaged in this game).
    pub fn durability(self) -> Option<u16> {
        self.tool_info().map(|(_, t)| t.durability())
    }

    pub fn name(self) -> String {
        match self.kind() {
            Some(ItemKind::Block(b)) => b.name().to_string(),
            Some(ItemKind::Tool(k, t)) => format!("{} {}", t.label(), k.label()),
            Some(ItemKind::Armor { slot, .. }) => ["Iron Helmet", "Iron Chestplate", "Iron Leggings", "Iron Boots"][slot].into(),
            Some(_) => match self {
                Self::STICK => "Stick",
                Self::COAL => "Coal",
                Self::IRON_INGOT => "Iron Ingot",
                Self::GOLD_INGOT => "Gold Ingot",
                Self::DIAMOND => "Diamond",
                Self::REDSTONE => "Redstone Dust",
                Self::APPLE => "Apple",
                Self::BREAD => "Bread",
                Self::RAW_PORK => "Raw Porkchop",
                Self::COOKED_PORK => "Cooked Porkchop",
                Self::RAW_BEEF => "Raw Beef",
                Self::COOKED_BEEF => "Steak",
                Self::ROTTEN_FLESH => "Rotten Flesh",
                _ => "Unknown",
            }
            .into(),
            None => "Unknown".into(),
        }
    }

    /// Atlas tile used for slot icons and dropped items.
    pub fn icon_tile(self) -> u8 {
        match self.kind() {
            Some(ItemKind::Block(b)) => tile::ICON_BLOCK + b.0,
            Some(ItemKind::Armor { slot, .. }) => tile::HELMET + slot as u8,
            Some(ItemKind::Tool(k, t)) => tile::ICON_TOOL + (t.index() * 4 + k.index()) as u8,
            Some(_) => match self {
                Self::STICK => tile::ICON_STICK,
                Self::COAL => tile::ICON_COAL,
                Self::IRON_INGOT => tile::ICON_IRON_INGOT,
                Self::GOLD_INGOT => tile::ICON_GOLD_INGOT,
                Self::DIAMOND => tile::ICON_DIAMOND,
                Self::REDSTONE => tile::ICON_REDSTONE,
                Self::APPLE => tile::ICON_APPLE,
                Self::BREAD => tile::ICON_BREAD,
                Self::RAW_PORK => tile::ICON_RAW_PORK,
                Self::COOKED_PORK => tile::ICON_COOKED_PORK,
                Self::RAW_BEEF => tile::ICON_RAW_BEEF,
                Self::COOKED_BEEF => tile::ICON_COOKED_BEEF,
                _ => tile::ICON_ROTTEN_FLESH,
            },
            None => tile::STONE,
        }
    }

    /// Seconds one unit burns in a furnace, if it is a fuel.
    pub fn fuel_secs(self) -> Option<f32> {
        match self {
            Self::COAL => Some(80.0),
            Self::STICK => Some(5.0),
            i if i == Self::block(BlockId::COAL_BLOCK) => Some(800.0),
            i if matches!(
                i.as_block(),
                Some(BlockId::LOG | BlockId::PLANKS | BlockId::FENCE | BlockId::CRAFTING_TABLE | BlockId::BOOKSHELF)
            ) =>
            {
                Some(15.0)
            }
            _ => None,
        }
    }

    /// Where the item belongs in the creative catalogue.
    pub fn category(self) -> Category {
        match self.kind() {
            Some(ItemKind::Block(b)) => match b {
                BlockId::GRASS
                | BlockId::DIRT
                | BlockId::SAND
                | BlockId::LOG
                | BlockId::LEAVES
                | BlockId::SNOW
                | BlockId::GRAVEL
                | BlockId::STONE
                | BlockId::COAL_ORE
                | BlockId::IRON_ORE
                | BlockId::GOLD_ORE
                | BlockId::REDSTONE_ORE
                | BlockId::DIAMOND_ORE
                | BlockId::GRANITE
                | BlockId::DIORITE
                | BlockId::ANDESITE
                | BlockId::NETHERRACK
                | BlockId::OBSIDIAN
                | BlockId::BEDROCK => Category::Natural,
                BlockId::GLASS
                | BlockId::FENCE
                | BlockId::TORCH
                | BlockId::WOOL_WHITE
                | BlockId::WOOL_RED
                | BlockId::BOOKSHELF
                | BlockId::CRAFTING_TABLE
                | BlockId::FURNACE
                | BlockId::GLOWSTONE => Category::Decoration,
                _ => Category::Building,
            },
            Some(ItemKind::Tool(..) | ItemKind::Armor { .. }) => Category::Tools,
            _ => Category::Materials,
        }
    }

    /// Every item in registry order (blocks, armor, materials, tools).
    pub fn all() -> impl Iterator<Item = Item> {
        let blocks = (1..BlockId::COUNT as u16).map(Item);
        let armor = (100..=103).map(Item);
        let materials = (256..=Self::LAST_MATERIAL).map(Item);
        let tools = (Self::TOOL_BASE..Self::TOOL_BASE + 20).map(Item);
        blocks.chain(armor).chain(materials).chain(tools)
    }

    /// How hard hitting a mob with this item in hand is.
    pub fn attack_damage(self) -> f32 {
        match self.tool_info() {
            Some((k, t)) => k.base_damage() + t.damage_bonus(),
            None => 1.0,
        }
    }

    /// Seconds to break `block` while holding `self` (`None` = bare hand). `f32::INFINITY` for
    /// unbreakable blocks. Second value: whether the block drops its loot.
    pub fn mining(held: Option<Item>, block: BlockId) -> (f32, bool) {
        let info = block.info();
        if info.hardness < 0.0 {
            return (f32::INFINITY, false);
        }
        let tool = held.and_then(Item::tool_info);
        let effective = matches!((tool, info.tool), (Some((k, _)), Some(want)) if k == want);
        let harvest = !info.needs_tool || matches!(tool, Some((_, t)) if effective && t.harvest_level() >= info.harvest_level);
        let speed = if effective { tool.map_or(1.0, |(_, t)| t.speed()) } else { 1.0 };
        // Swords cut leaves and other soft blocks a bit faster, like cobwebs would be.
        let factor = if harvest { 1.5 } else { 5.0 };
        (info.hardness * factor / speed, harvest)
    }

    /// What breaking `block` yields. `roll` is a random number used for chance drops.
    pub fn loot(block: BlockId, harvested: bool, roll: u32) -> Option<ItemStack> {
        if !block.is_breakable() {
            return None;
        }
        if block.info().needs_tool && !harvested {
            return None;
        }
        let one = |i: Item| Some(ItemStack::new(i, 1));
        match block {
            BlockId::STONE => one(Item::block(BlockId::COBBLE)),
            BlockId::GRASS => one(Item::block(BlockId::DIRT)),
            BlockId::COAL_ORE => one(Item::COAL),
            BlockId::DIAMOND_ORE => one(Item::DIAMOND),
            BlockId::REDSTONE_ORE => Some(ItemStack::new(Item::REDSTONE, 4 + (roll % 2) as u8)),
            BlockId::GLASS => None,
            BlockId::LEAVES => (roll % 20 == 0).then(|| ItemStack::new(Item::APPLE, 1)),
            other => one(Item::block(other)),
        }
    }

    /// Experience for breaking `block`.
    pub fn block_xp(block: BlockId) -> u32 {
        match block {
            BlockId::COAL_ORE => 1,
            BlockId::REDSTONE_ORE => 3,
            BlockId::DIAMOND_ORE => 4,
            _ => 0,
        }
    }

    /// Armor icon tiles use the flat sprite, everything else the block-or-item tile; cube blocks
    /// are shown as isometric icons so the side texture is not needed any more.
    pub fn flat_tile(self) -> u8 {
        match self.as_block() {
            Some(b) => b.tile(Face::Side),
            None => self.icon_tile(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ItemStack {
    pub item: Item,
    pub count: u8,
    /// Wear of a tool: uses so far.
    #[serde(default)]
    pub damage: u16,
}

impl ItemStack {
    pub fn new(item: Item, count: u8) -> Self {
        Self { item, count, damage: 0 }
    }

    pub fn space_left(&self) -> u8 {
        self.item.max_stack().saturating_sub(self.count)
    }

    /// Wear as a fraction `0..=1` (`None` for items that do not wear out).
    pub fn wear(&self) -> Option<f32> {
        let max = self.item.durability()?;
        Some((self.damage as f32 / max as f32).clamp(0.0, 1.0))
    }

    /// Adds `uses` of wear. Returns `true` when the tool broke.
    pub fn wear_out(&mut self, uses: u16) -> bool {
        match self.item.durability() {
            Some(max) => {
                self.damage = self.damage.saturating_add(uses);
                self.damage >= max
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_ids_round_trip_through_their_kind() {
        for item in Item::all() {
            assert!(item.is_valid(), "{item:?}");
            assert_ne!(item.name(), "Unknown", "{item:?}");
            assert!(item.icon_tile() < tile::COUNT);
        }
        let pick = Item::tool(ToolKind::Pickaxe, Tier::Iron);
        assert_eq!(pick.tool_info(), Some((ToolKind::Pickaxe, Tier::Iron)));
        assert_eq!(pick.name(), "Iron Pickaxe");
        assert_eq!(Item::tool(ToolKind::Sword, Tier::Diamond).name(), "Diamond Sword");
        assert!(!Item(5000).is_valid() && !Item(0).is_valid() && !Item(200).is_valid());
        assert_eq!(Item::IRON_BOOTS.armor_slot(), Some(3));
        assert_eq!(Item::all().count(), (BlockId::COUNT as usize - 1) + 4 + 13 + 20);
    }

    #[test]
    fn tools_do_not_stack_and_wear_out() {
        let mut s = ItemStack::new(Item::tool(ToolKind::Pickaxe, Tier::Wood), 1);
        assert_eq!(s.item.max_stack(), 1);
        assert_eq!(s.wear(), Some(0.0));
        assert!(!s.wear_out(58));
        assert!(s.wear_out(1), "59th use breaks a wooden tool");
        assert_eq!(ItemStack::new(Item::STICK, 3).wear(), None);
        assert_eq!(Item::STICK.max_stack(), 64);
    }

    #[test]
    fn mining_follows_the_minecraft_formula() {
        let wood_pick = Some(Item::tool(ToolKind::Pickaxe, Tier::Wood));
        let (t, ok) = Item::mining(None, BlockId::LOG);
        assert!((t - 3.0).abs() < 1e-4 && ok, "logs by hand take 3 s");
        let (t, ok) = Item::mining(None, BlockId::STONE);
        assert!((t - 7.5).abs() < 1e-4 && !ok, "stone by hand is slow and drops nothing");
        let (t, ok) = Item::mining(wood_pick, BlockId::STONE);
        assert!((t - 1.125).abs() < 1e-4 && ok);
        // Diamond ore needs an iron pickaxe.
        assert!(!Item::mining(wood_pick, BlockId::DIAMOND_ORE).1);
        assert!(!Item::mining(Some(Item::tool(ToolKind::Pickaxe, Tier::Stone)), BlockId::DIAMOND_ORE).1);
        assert!(Item::mining(Some(Item::tool(ToolKind::Pickaxe, Tier::Iron)), BlockId::DIAMOND_ORE).1);
        // An axe does not speed up stone.
        let axe = Some(Item::tool(ToolKind::Axe, Tier::Diamond));
        assert!((Item::mining(axe, BlockId::STONE).0 - 7.5).abs() < 1e-4);
        assert_eq!(Item::mining(None, BlockId::BEDROCK).0, f32::INFINITY);
        // Gold digs fast but harvests only like wood.
        let gold = Some(Item::tool(ToolKind::Pickaxe, Tier::Gold));
        assert!(Item::mining(gold, BlockId::STONE).0 < Item::mining(wood_pick, BlockId::STONE).0);
        assert!(!Item::mining(gold, BlockId::IRON_ORE).1);
    }

    #[test]
    fn loot_follows_minecraft_rules() {
        let one = |i: Item| Some(ItemStack::new(i, 1));
        assert_eq!(Item::loot(BlockId::STONE, true, 0), one(Item::block(BlockId::COBBLE)));
        assert_eq!(Item::loot(BlockId::STONE, false, 0), None, "stone mined by hand drops nothing");
        assert_eq!(Item::loot(BlockId::GRASS, true, 0), one(Item::block(BlockId::DIRT)));
        assert_eq!(Item::loot(BlockId::LOG, true, 0), one(Item::block(BlockId::LOG)));
        assert_eq!(Item::loot(BlockId::COAL_ORE, true, 0), one(Item::COAL));
        assert_eq!(Item::loot(BlockId::DIAMOND_ORE, true, 0), one(Item::DIAMOND));
        assert_eq!(Item::loot(BlockId::DIAMOND_ORE, false, 0), None);
        assert_eq!(Item::loot(BlockId::REDSTONE_ORE, true, 1).unwrap().count, 5);
        assert_eq!(Item::loot(BlockId::GLASS, true, 0), None);
        assert_eq!(Item::loot(BlockId::BEDROCK, true, 0), None);
        assert_eq!(Item::loot(BlockId::WATER, true, 0), None);
        assert_eq!(Item::loot(BlockId::LEAVES, true, 20), one(Item::APPLE));
        assert_eq!(Item::loot(BlockId::LEAVES, true, 21), None);
    }

    #[test]
    fn food_and_fuel() {
        assert_eq!(Item::COOKED_BEEF.food(), Some((8, 12.8)));
        assert_eq!(Item::STICK.food(), None);
        assert_eq!(Item::COAL.fuel_secs(), Some(80.0));
        assert_eq!(Item::block(BlockId::PLANKS).fuel_secs(), Some(15.0));
        assert_eq!(Item::block(BlockId::STONE).fuel_secs(), None);
    }

    #[test]
    fn attack_damage_scales_with_material() {
        assert_eq!(Item::STICK.attack_damage(), 1.0);
        let wood = Item::tool(ToolKind::Sword, Tier::Wood).attack_damage();
        let dia = Item::tool(ToolKind::Sword, Tier::Diamond).attack_damage();
        assert!(dia > wood && wood > 1.0);
    }
}
