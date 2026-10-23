//! Block registry. Blocks are plain `u8` ids so a chunk is a flat byte array; every property
//! (textures, hardness, preferred tool, light, shape, ...) lives in one static table.

use crate::items::ToolKind;

/// Which side of a block a face belongs to; selects the atlas tile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    Top,
    Bottom,
    Side,
}

/// Atlas tile indices. The procedural atlas in `rendering::atlas` paints these.
///
/// Layout of the 16x16 tile atlas: tiles `0..64` are textures a chunk mesh can reference (the
/// vertex format packs the index next to a shade id), `ICON_BLOCK + id` holds an isometric icon of
/// every block, `ICON_ITEM..` holds item sprites and `ICON_UI..` the HUD sprites.
pub mod tile {
    pub const GRASS_TOP: u8 = 0;
    pub const GRASS_SIDE: u8 = 1;
    pub const DIRT: u8 = 2;
    pub const STONE: u8 = 3;
    pub const SAND: u8 = 4;
    pub const WATER: u8 = 5;
    pub const LOG_SIDE: u8 = 6;
    pub const LOG_TOP: u8 = 7;
    pub const LEAVES: u8 = 8;
    pub const PLANKS: u8 = 9;
    pub const BEDROCK: u8 = 10;
    pub const SNOW: u8 = 11;
    pub const COBBLE: u8 = 12;
    /// Item icons (armor); `HELMET + n` is the icon for armor slot `n`.
    pub const HELMET: u8 = 13;
    pub const CHESTPLATE: u8 = 14;
    pub const LEGGINGS: u8 = 15;
    pub const BOOTS: u8 = 16;
    pub const COAL_ORE: u8 = 17;
    pub const IRON_ORE: u8 = 18;
    pub const GOLD_ORE: u8 = 19;
    pub const REDSTONE_ORE: u8 = 20;
    pub const DIAMOND_ORE: u8 = 21;
    pub const GRAVEL: u8 = 22;
    pub const SANDSTONE_SIDE: u8 = 23;
    pub const SANDSTONE_TOP: u8 = 24;
    pub const BRICKS: u8 = 25;
    pub const STONE_BRICKS: u8 = 26;
    pub const MOSSY_COBBLE: u8 = 27;
    pub const GRANITE: u8 = 28;
    pub const DIORITE: u8 = 29;
    pub const ANDESITE: u8 = 30;
    pub const GLASS: u8 = 31;
    pub const TORCH_SIDE: u8 = 32;
    pub const TORCH_TOP: u8 = 33;
    pub const CRAFTING_TOP: u8 = 34;
    pub const CRAFTING_SIDE: u8 = 35;
    pub const FURNACE_SIDE: u8 = 36;
    pub const FURNACE_TOP: u8 = 37;
    pub const NETHERRACK: u8 = 38;
    pub const NETHER_BRICKS: u8 = 39;
    pub const GLOWSTONE: u8 = 40;
    pub const OBSIDIAN: u8 = 41;
    pub const QUARTZ: u8 = 42;
    pub const COAL_BLOCK: u8 = 43;
    pub const IRON_BLOCK: u8 = 44;
    pub const GOLD_BLOCK: u8 = 45;
    pub const DIAMOND_BLOCK: u8 = 46;
    pub const WOOL_WHITE: u8 = 47;
    pub const WOOL_RED: u8 = 48;
    pub const BOOKSHELF: u8 = 49;
    pub const SMOOTH_STONE: u8 = 50;
    pub const NETHER_TILES: u8 = 51;
    /// Number of tiles a chunk mesh may reference.
    pub const BLOCK_TILES: u8 = 52;

    /// First tile of the isometric block icons: `ICON_BLOCK + block id`.
    pub const ICON_BLOCK: u8 = 64;
    /// Item sprites (see [`crate::items::Item::icon_tile`]).
    pub const ICON_ITEM: u8 = 112;
    pub const ICON_STICK: u8 = ICON_ITEM;
    pub const ICON_COAL: u8 = ICON_ITEM + 1;
    pub const ICON_IRON_INGOT: u8 = ICON_ITEM + 2;
    pub const ICON_GOLD_INGOT: u8 = ICON_ITEM + 3;
    pub const ICON_DIAMOND: u8 = ICON_ITEM + 4;
    pub const ICON_REDSTONE: u8 = ICON_ITEM + 5;
    pub const ICON_APPLE: u8 = ICON_ITEM + 6;
    pub const ICON_BREAD: u8 = ICON_ITEM + 7;
    pub const ICON_RAW_PORK: u8 = ICON_ITEM + 8;
    pub const ICON_COOKED_PORK: u8 = ICON_ITEM + 9;
    pub const ICON_RAW_BEEF: u8 = ICON_ITEM + 10;
    pub const ICON_COOKED_BEEF: u8 = ICON_ITEM + 11;
    pub const ICON_ROTTEN_FLESH: u8 = ICON_ITEM + 12;
    /// `ICON_TOOL + tier * 4 + kind` (20 tiles).
    pub const ICON_TOOL: u8 = ICON_ITEM + 13;
    /// HUD sprites.
    pub const UI: u8 = 160;
    pub const UI_HEART_FULL: u8 = UI;
    pub const UI_HEART_HALF: u8 = UI + 1;
    pub const UI_HEART_EMPTY: u8 = UI + 2;
    pub const UI_FOOD_FULL: u8 = UI + 3;
    pub const UI_FOOD_HALF: u8 = UI + 4;
    pub const UI_FOOD_EMPTY: u8 = UI + 5;
    pub const UI_BUBBLE: u8 = UI + 6;
    pub const UI_ARMOR_FULL: u8 = UI + 7;
    pub const UI_ARMOR_HALF: u8 = UI + 8;
    pub const UI_ARMOR_EMPTY: u8 = UI + 9;
    pub const UI_ARROW: u8 = UI + 10;
    pub const UI_FLAME: u8 = UI + 11;
    pub const UI_FLAME_OFF: u8 = UI + 12;
    pub const UI_ARROW_FULL: u8 = UI + 13;
    pub const COUNT: u8 = 176;
}

/// How a block is rendered and collided with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// A full unit cube (greedy-meshed when opaque).
    Cube,
    /// A thin upright pole drawn as a small box (torch).
    Torch,
    /// A post with rails towards connecting neighbours.
    Fence,
}

/// Static description of one block type.
#[derive(Clone, Copy, Debug)]
pub struct BlockInfo {
    pub name: &'static str,
    pub top: u8,
    pub bottom: u8,
    pub side: u8,
    /// Seconds it takes to break with the ideal tool, before the Minecraft 1.5x / 5x factor.
    /// Negative means unbreakable.
    pub hardness: f32,
    /// The tool that mines this block fast.
    pub tool: Option<ToolKind>,
    /// Without a tool of at least `harvest_level` the block drops nothing (only checked when `needs_tool`).
    pub needs_tool: bool,
    pub harvest_level: u8,
    /// Hides the faces of its neighbours and is drawn in the opaque pass.
    pub opaque: bool,
    /// Blocks the player's movement.
    pub solid: bool,
    /// Can be selected by the crosshair ray.
    pub targetable: bool,
    /// A placed block may overwrite this one.
    pub replaceable: bool,
    /// Drawn in the blended pass (water, glass).
    pub translucent: bool,
    /// Block light emitted, `0..=15`.
    pub light: u8,
    pub shape: Shape,
    /// Representative sRGB colour for particles and the mobs' palette.
    pub color: [f32; 3],
}

impl BlockInfo {
    const fn new(name: &'static str, tile: u8, color: [f32; 3]) -> Self {
        Self {
            name,
            top: tile,
            bottom: tile,
            side: tile,
            hardness: 1.0,
            tool: None,
            needs_tool: false,
            harvest_level: 0,
            opaque: true,
            solid: true,
            targetable: true,
            replaceable: false,
            translucent: false,
            light: 0,
            shape: Shape::Cube,
            color,
        }
    }

    const fn tiles(mut self, top: u8, bottom: u8, side: u8) -> Self {
        self.top = top;
        self.bottom = bottom;
        self.side = side;
        self
    }

    const fn hard(mut self, hardness: f32) -> Self {
        self.hardness = hardness;
        self
    }

    const fn tool(mut self, tool: ToolKind) -> Self {
        self.tool = Some(tool);
        self
    }

    /// Needs a pickaxe (or other tool) of at least `level` to drop anything.
    const fn needs(mut self, level: u8) -> Self {
        self.needs_tool = true;
        self.harvest_level = level;
        self
    }

    const fn light(mut self, light: u8) -> Self {
        self.light = light;
        self
    }

    const fn see_through(mut self) -> Self {
        self.opaque = false;
        self
    }

    const fn shape(mut self, shape: Shape) -> Self {
        self.shape = shape;
        self
    }
}

use ToolKind::{Axe, Pickaxe, Shovel};

/// Index = block id.
static BLOCKS: [BlockInfo; BlockId::COUNT as usize] = [
    // 0
    {
        let mut b = BlockInfo::new("Air", tile::STONE, [1.0, 0.0, 1.0]).see_through();
        b.solid = false;
        b.targetable = false;
        b.replaceable = true;
        b.hardness = -1.0;
        b
    },
    BlockInfo::new("Grass Block", tile::GRASS_SIDE, [0.36, 0.62, 0.22])
        .tiles(tile::GRASS_TOP, tile::DIRT, tile::GRASS_SIDE)
        .hard(0.6)
        .tool(Shovel),
    BlockInfo::new("Dirt", tile::DIRT, [0.47, 0.33, 0.20]).hard(0.5).tool(Shovel),
    BlockInfo::new("Stone", tile::STONE, [0.50, 0.50, 0.52]).hard(1.5).tool(Pickaxe).needs(0),
    BlockInfo::new("Sand", tile::SAND, [0.85, 0.80, 0.55]).hard(0.5).tool(Shovel),
    // 5: water
    {
        let mut b = BlockInfo::new("Water", tile::WATER, [0.22, 0.40, 0.80]).see_through();
        b.solid = false;
        b.targetable = false;
        b.replaceable = true;
        b.translucent = true;
        b.hardness = -1.0;
        b
    },
    BlockInfo::new("Log", tile::LOG_SIDE, [0.40, 0.29, 0.16])
        .tiles(tile::LOG_TOP, tile::LOG_TOP, tile::LOG_SIDE)
        .hard(2.0)
        .tool(Axe),
    BlockInfo::new("Leaves", tile::LEAVES, [0.20, 0.45, 0.16]).hard(0.2).tool(ToolKind::Sword),
    BlockInfo::new("Planks", tile::PLANKS, [0.68, 0.53, 0.30]).hard(2.0).tool(Axe),
    {
        let mut b = BlockInfo::new("Bedrock", tile::BEDROCK, [0.18, 0.18, 0.20]);
        b.hardness = -1.0;
        b
    },
    BlockInfo::new("Snow", tile::SNOW, [0.94, 0.96, 0.98]).hard(0.2).tool(Shovel),
    BlockInfo::new("Cobblestone", tile::COBBLE, [0.42, 0.42, 0.44]).hard(2.0).tool(Pickaxe).needs(0),
    // 12
    BlockInfo::new("Coal Ore", tile::COAL_ORE, [0.20, 0.20, 0.22]).hard(3.0).tool(Pickaxe).needs(0),
    BlockInfo::new("Iron Ore", tile::IRON_ORE, [0.72, 0.58, 0.48]).hard(3.0).tool(Pickaxe).needs(1),
    BlockInfo::new("Gold Ore", tile::GOLD_ORE, [0.85, 0.72, 0.25]).hard(3.0).tool(Pickaxe).needs(2),
    BlockInfo::new("Redstone Ore", tile::REDSTONE_ORE, [0.75, 0.12, 0.10]).hard(3.0).tool(Pickaxe).needs(2),
    BlockInfo::new("Diamond Ore", tile::DIAMOND_ORE, [0.35, 0.82, 0.82]).hard(3.0).tool(Pickaxe).needs(2),
    // 17
    BlockInfo::new("Gravel", tile::GRAVEL, [0.52, 0.50, 0.49]).hard(0.6).tool(Shovel),
    BlockInfo::new("Sandstone", tile::SANDSTONE_SIDE, [0.80, 0.74, 0.50])
        .tiles(tile::SANDSTONE_TOP, tile::SANDSTONE_TOP, tile::SANDSTONE_SIDE)
        .hard(0.8)
        .tool(Pickaxe)
        .needs(0),
    BlockInfo::new("Bricks", tile::BRICKS, [0.62, 0.30, 0.25]).hard(2.0).tool(Pickaxe).needs(0),
    BlockInfo::new("Stone Bricks", tile::STONE_BRICKS, [0.52, 0.52, 0.54]).hard(1.5).tool(Pickaxe).needs(0),
    BlockInfo::new("Mossy Cobblestone", tile::MOSSY_COBBLE, [0.38, 0.48, 0.36]).hard(2.0).tool(Pickaxe).needs(0),
    BlockInfo::new("Granite", tile::GRANITE, [0.60, 0.40, 0.34]).hard(1.5).tool(Pickaxe).needs(0),
    BlockInfo::new("Diorite", tile::DIORITE, [0.78, 0.78, 0.78]).hard(1.5).tool(Pickaxe).needs(0),
    BlockInfo::new("Andesite", tile::ANDESITE, [0.55, 0.55, 0.56]).hard(1.5).tool(Pickaxe).needs(0),
    // 25: glass
    {
        let mut b = BlockInfo::new("Glass", tile::GLASS, [0.75, 0.88, 0.92]).hard(0.3).see_through();
        b.translucent = true;
        b
    },
    BlockInfo::new("Oak Fence", tile::PLANKS, [0.68, 0.53, 0.30]).hard(2.0).tool(Axe).see_through().shape(Shape::Fence),
    // 27: torch
    {
        let mut b = BlockInfo::new("Torch", tile::TORCH_SIDE, [0.95, 0.75, 0.30])
            .tiles(tile::TORCH_TOP, tile::TORCH_SIDE, tile::TORCH_SIDE)
            .hard(0.0)
            .see_through()
            .light(14)
            .shape(Shape::Torch);
        b.solid = false;
        b
    },
    BlockInfo::new("Crafting Table", tile::CRAFTING_SIDE, [0.55, 0.40, 0.22])
        .tiles(tile::CRAFTING_TOP, tile::PLANKS, tile::CRAFTING_SIDE)
        .hard(2.5)
        .tool(Axe),
    BlockInfo::new("Furnace", tile::FURNACE_SIDE, [0.45, 0.45, 0.47])
        .tiles(tile::FURNACE_TOP, tile::FURNACE_TOP, tile::FURNACE_SIDE)
        .hard(3.5)
        .tool(Pickaxe)
        .needs(0),
    // 30
    BlockInfo::new("Netherrack", tile::NETHERRACK, [0.45, 0.20, 0.20]).hard(0.4).tool(Pickaxe),
    BlockInfo::new("Nether Bricks", tile::NETHER_BRICKS, [0.25, 0.10, 0.12]).hard(2.0).tool(Pickaxe).needs(0),
    BlockInfo::new("Glowstone", tile::GLOWSTONE, [0.95, 0.80, 0.45]).hard(0.3).light(15),
    BlockInfo::new("Obsidian", tile::OBSIDIAN, [0.10, 0.07, 0.17]).hard(50.0).tool(Pickaxe).needs(3),
    BlockInfo::new("Block of Quartz", tile::QUARTZ, [0.92, 0.90, 0.86]).hard(0.8).tool(Pickaxe).needs(0),
    BlockInfo::new("Block of Coal", tile::COAL_BLOCK, [0.10, 0.10, 0.11]).hard(5.0).tool(Pickaxe).needs(0),
    BlockInfo::new("Block of Iron", tile::IRON_BLOCK, [0.80, 0.80, 0.82]).hard(5.0).tool(Pickaxe).needs(1),
    BlockInfo::new("Block of Gold", tile::GOLD_BLOCK, [0.96, 0.82, 0.25]).hard(3.0).tool(Pickaxe).needs(2),
    BlockInfo::new("Block of Diamond", tile::DIAMOND_BLOCK, [0.40, 0.88, 0.85]).hard(5.0).tool(Pickaxe).needs(2),
    BlockInfo::new("White Wool", tile::WOOL_WHITE, [0.93, 0.93, 0.93]).hard(0.8),
    BlockInfo::new("Red Wool", tile::WOOL_RED, [0.65, 0.15, 0.14]).hard(0.8),
    BlockInfo::new("Bookshelf", tile::BOOKSHELF, [0.55, 0.40, 0.25])
        .tiles(tile::PLANKS, tile::PLANKS, tile::BOOKSHELF)
        .hard(1.5)
        .tool(Axe),
    BlockInfo::new("Smooth Stone", tile::SMOOTH_STONE, [0.62, 0.62, 0.64]).hard(2.0).tool(Pickaxe).needs(0),
    BlockInfo::new("Nether Tiles", tile::NETHER_TILES, [0.16, 0.10, 0.18]).hard(2.0).tool(Pickaxe).needs(0),
];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(transparent)]
pub struct BlockId(pub u8);

impl BlockId {
    pub const AIR: BlockId = BlockId(0);
    pub const GRASS: BlockId = BlockId(1);
    pub const DIRT: BlockId = BlockId(2);
    pub const STONE: BlockId = BlockId(3);
    pub const SAND: BlockId = BlockId(4);
    pub const WATER: BlockId = BlockId(5);
    pub const LOG: BlockId = BlockId(6);
    pub const LEAVES: BlockId = BlockId(7);
    pub const PLANKS: BlockId = BlockId(8);
    pub const BEDROCK: BlockId = BlockId(9);
    pub const SNOW: BlockId = BlockId(10);
    pub const COBBLE: BlockId = BlockId(11);
    pub const COAL_ORE: BlockId = BlockId(12);
    pub const IRON_ORE: BlockId = BlockId(13);
    pub const GOLD_ORE: BlockId = BlockId(14);
    pub const REDSTONE_ORE: BlockId = BlockId(15);
    pub const DIAMOND_ORE: BlockId = BlockId(16);
    pub const GRAVEL: BlockId = BlockId(17);
    pub const SANDSTONE: BlockId = BlockId(18);
    pub const BRICKS: BlockId = BlockId(19);
    pub const STONE_BRICKS: BlockId = BlockId(20);
    pub const MOSSY_COBBLE: BlockId = BlockId(21);
    pub const GRANITE: BlockId = BlockId(22);
    pub const DIORITE: BlockId = BlockId(23);
    pub const ANDESITE: BlockId = BlockId(24);
    pub const GLASS: BlockId = BlockId(25);
    pub const FENCE: BlockId = BlockId(26);
    pub const TORCH: BlockId = BlockId(27);
    pub const CRAFTING_TABLE: BlockId = BlockId(28);
    pub const FURNACE: BlockId = BlockId(29);
    pub const NETHERRACK: BlockId = BlockId(30);
    pub const NETHER_BRICKS: BlockId = BlockId(31);
    pub const GLOWSTONE: BlockId = BlockId(32);
    pub const OBSIDIAN: BlockId = BlockId(33);
    pub const QUARTZ_BLOCK: BlockId = BlockId(34);
    pub const COAL_BLOCK: BlockId = BlockId(35);
    pub const IRON_BLOCK: BlockId = BlockId(36);
    pub const GOLD_BLOCK: BlockId = BlockId(37);
    pub const DIAMOND_BLOCK: BlockId = BlockId(38);
    pub const WOOL_WHITE: BlockId = BlockId(39);
    pub const WOOL_RED: BlockId = BlockId(40);
    pub const BOOKSHELF: BlockId = BlockId(41);
    pub const SMOOTH_STONE: BlockId = BlockId(42);
    pub const NETHER_TILES: BlockId = BlockId(43);
    /// Number of block ids (valid ids are `0..COUNT`).
    pub const COUNT: u8 = 44;

    /// Every id that is a real block (everything but air), in registry order.
    pub fn all() -> impl Iterator<Item = BlockId> {
        (1..Self::COUNT).map(BlockId)
    }

    #[inline]
    pub fn is_valid(self) -> bool {
        self.0 < Self::COUNT
    }

    #[inline]
    pub fn info(self) -> &'static BlockInfo {
        &BLOCKS[(self.0 as usize).min(BLOCKS.len() - 1)]
    }

    #[inline]
    pub fn is_air(self) -> bool {
        self == Self::AIR
    }

    #[inline]
    pub fn is_water(self) -> bool {
        self == Self::WATER
    }

    /// Hides the faces of its neighbours and is drawn in the opaque pass.
    #[inline]
    pub fn is_opaque(self) -> bool {
        self.info().opaque
    }

    /// Drawn in the blended pass (water, glass).
    #[inline]
    pub fn is_translucent(self) -> bool {
        self.info().translucent
    }

    /// Blocks the player's movement.
    #[inline]
    pub fn is_solid(self) -> bool {
        self.info().solid
    }

    /// Can be selected by the crosshair ray.
    #[inline]
    pub fn is_targetable(self) -> bool {
        self.info().targetable
    }

    /// A placed block may overwrite this one.
    #[inline]
    pub fn is_replaceable(self) -> bool {
        self.info().replaceable
    }

    #[inline]
    pub fn is_breakable(self) -> bool {
        self.is_valid() && self.info().targetable && self.info().hardness >= 0.0
    }

    /// Block light emitted, `0..=15`.
    #[inline]
    pub fn light(self) -> u8 {
        self.info().light
    }

    #[inline]
    pub fn shape(self) -> Shape {
        self.info().shape
    }

    pub fn tile(self, face: Face) -> u8 {
        let i = self.info();
        match face {
            Face::Top => i.top,
            Face::Bottom => i.bottom,
            Face::Side => i.side,
        }
    }

    pub fn name(self) -> &'static str {
        if self.is_valid() { self.info().name } else { "Unknown" }
    }

    /// Representative sRGB colour for particles.
    pub fn base_color(self) -> [f32; 3] {
        self.info().color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_the_id_constants() {
        assert_eq!(BLOCKS.len(), BlockId::COUNT as usize);
        assert_eq!(BlockId::GRASS.name(), "Grass Block");
        assert_eq!(BlockId::TORCH.name(), "Torch");
        assert_eq!(BlockId::NETHER_TILES.name(), "Nether Tiles");
        assert_eq!(BlockId::DIAMOND_ORE.info().harvest_level, 2);
        assert_eq!(BlockId(200).name(), "Unknown");
    }

    #[test]
    fn every_face_maps_to_a_valid_chunk_tile() {
        for id in BlockId::all() {
            for face in [Face::Top, Face::Bottom, Face::Side] {
                assert!(id.tile(face) < tile::BLOCK_TILES, "block {} {face:?}", id.name());
            }
        }
    }

    #[test]
    fn classification() {
        assert!(!BlockId::AIR.is_solid());
        assert!(!BlockId::WATER.is_solid());
        assert!(BlockId::WATER.is_replaceable());
        assert!(BlockId::STONE.is_solid() && BlockId::STONE.is_opaque());
        assert!(!BlockId::BEDROCK.is_breakable());
        // Glass and fences collide but do not hide their neighbours; torches do neither.
        assert!(BlockId::GLASS.is_solid() && !BlockId::GLASS.is_opaque() && BlockId::GLASS.is_translucent());
        assert!(BlockId::FENCE.is_solid() && !BlockId::FENCE.is_opaque());
        assert!(!BlockId::TORCH.is_solid() && BlockId::TORCH.is_targetable() && BlockId::TORCH.is_breakable());
        assert_eq!(BlockId::TORCH.light(), 14);
        assert_eq!(BlockId::GLOWSTONE.light(), 15);
        assert!(!BlockId::WATER.is_breakable() && !BlockId::AIR.is_breakable());
    }

    #[test]
    fn ores_need_ever_better_tools() {
        let level = |b: BlockId| b.info().needs_tool.then_some(b.info().harvest_level);
        assert_eq!(level(BlockId::COAL_ORE), Some(0));
        assert_eq!(level(BlockId::IRON_ORE), Some(1));
        assert_eq!(level(BlockId::DIAMOND_ORE), Some(2));
        assert_eq!(level(BlockId::OBSIDIAN), Some(3));
        assert_eq!(level(BlockId::DIRT), None);
    }
}
