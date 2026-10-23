//! Block registry. Blocks are plain `u8` ids so a chunk is a flat byte array.

/// Which side of a block a face belongs to; selects the atlas tile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    Top,
    Bottom,
    Side,
}

/// Atlas tile indices. The procedural atlas in `rendering::atlas` paints these.
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
    pub const COUNT: u8 = 13;
}

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
        !self.is_air() && !self.is_water()
    }

    /// Blocks the player's movement.
    #[inline]
    pub fn is_solid(self) -> bool {
        self.is_opaque()
    }

    /// Can be selected by the crosshair ray.
    #[inline]
    pub fn is_targetable(self) -> bool {
        self.is_opaque()
    }

    /// A placed block may overwrite this one.
    #[inline]
    pub fn is_replaceable(self) -> bool {
        self.is_air() || self.is_water()
    }

    #[inline]
    pub fn is_breakable(self) -> bool {
        self.is_opaque() && self != Self::BEDROCK
    }

    pub fn tile(self, face: Face) -> u8 {
        match (self, face) {
            (Self::GRASS, Face::Top) => tile::GRASS_TOP,
            (Self::GRASS, Face::Side) => tile::GRASS_SIDE,
            (Self::GRASS, Face::Bottom) | (Self::DIRT, _) => tile::DIRT,
            (Self::STONE, _) => tile::STONE,
            (Self::SAND, _) => tile::SAND,
            (Self::WATER, _) => tile::WATER,
            (Self::LOG, Face::Side) => tile::LOG_SIDE,
            (Self::LOG, _) => tile::LOG_TOP,
            (Self::LEAVES, _) => tile::LEAVES,
            (Self::PLANKS, _) => tile::PLANKS,
            (Self::BEDROCK, _) => tile::BEDROCK,
            (Self::SNOW, _) => tile::SNOW,
            (Self::COBBLE, _) => tile::COBBLE,
            _ => tile::STONE,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::AIR => "Air",
            Self::GRASS => "Grass",
            Self::DIRT => "Dirt",
            Self::STONE => "Stone",
            Self::SAND => "Sand",
            Self::WATER => "Water",
            Self::LOG => "Log",
            Self::LEAVES => "Leaves",
            Self::PLANKS => "Planks",
            Self::BEDROCK => "Bedrock",
            Self::SNOW => "Snow",
            Self::COBBLE => "Cobble",
            _ => "Unknown",
        }
    }

    /// Representative sRGB colour for particles and the hotbar.
    pub fn base_color(self) -> [f32; 3] {
        match self {
            Self::GRASS => [0.36, 0.62, 0.22],
            Self::DIRT => [0.47, 0.33, 0.20],
            Self::STONE => [0.50, 0.50, 0.52],
            Self::SAND => [0.85, 0.80, 0.55],
            Self::WATER => [0.22, 0.40, 0.80],
            Self::LOG => [0.40, 0.29, 0.16],
            Self::LEAVES => [0.20, 0.45, 0.16],
            Self::PLANKS => [0.68, 0.53, 0.30],
            Self::BEDROCK => [0.18, 0.18, 0.20],
            Self::SNOW => [0.94, 0.96, 0.98],
            Self::COBBLE => [0.42, 0.42, 0.44],
            _ => [1.0, 0.0, 1.0],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_maps_to_a_valid_tile() {
        for id in 1..=11u8 {
            for face in [Face::Top, Face::Bottom, Face::Side] {
                assert!(BlockId(id).tile(face) < tile::COUNT, "block {id} {face:?}");
            }
        }
    }

    #[test]
    fn classification() {
        assert!(!BlockId::AIR.is_solid());
        assert!(!BlockId::WATER.is_solid());
        assert!(BlockId::WATER.is_replaceable());
        assert!(BlockId::STONE.is_solid());
        assert!(!BlockId::BEDROCK.is_breakable());
    }
}
