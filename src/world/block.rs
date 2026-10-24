//! Block registry. Blocks are plain `u8` ids so a chunk is a flat byte array.
//!
//! Blocks that carry state (fluid level, redstone power, torch orientation, piston facing, ...)
//! encode that state in the id itself. Every such *family* has a canonical id below 100 — the one
//! that exists as an inventory item — and its state variants live above 103, because the ids
//! 100..=103 belong to the armor items. The numbering is defined once in the `fam` constants.

use bevy::math::IVec3;

/// Which side of a block a face belongs to; selects the atlas tile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    Top,
    Bottom,
    Side,
}

/// A direction in the voxel grid. `North` is -Z (where the player looks at yaw 0).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Facing {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Facing {
    pub const ALL: [Facing; 6] = [Facing::Down, Facing::Up, Facing::North, Facing::South, Facing::West, Facing::East];
    pub const HORIZONTAL: [Facing; 4] = [Facing::North, Facing::South, Facing::West, Facing::East];

    pub const fn index(self) -> u8 {
        self as u8
    }

    pub const fn from_index(i: u8) -> Facing {
        match i {
            0 => Facing::Down,
            1 => Facing::Up,
            2 => Facing::North,
            3 => Facing::South,
            4 => Facing::West,
            _ => Facing::East,
        }
    }

    pub const fn offset(self) -> IVec3 {
        match self {
            Facing::Down => IVec3::new(0, -1, 0),
            Facing::Up => IVec3::new(0, 1, 0),
            Facing::North => IVec3::new(0, 0, -1),
            Facing::South => IVec3::new(0, 0, 1),
            Facing::West => IVec3::new(-1, 0, 0),
            Facing::East => IVec3::new(1, 0, 0),
        }
    }

    pub const fn opposite(self) -> Facing {
        match self {
            Facing::Down => Facing::Up,
            Facing::Up => Facing::Down,
            Facing::North => Facing::South,
            Facing::South => Facing::North,
            Facing::West => Facing::East,
            Facing::East => Facing::West,
        }
    }

    pub const fn is_horizontal(self) -> bool {
        !matches!(self, Facing::Down | Facing::Up)
    }

    /// The facing whose offset is `n` (a unit axis vector).
    pub fn from_offset(n: IVec3) -> Option<Facing> {
        Facing::ALL.into_iter().find(|f| f.offset() == n)
    }

    /// Facing of the axis along which `v` is largest (ties prefer the vertical axis).
    pub fn dominant(v: bevy::math::Vec3) -> Facing {
        let a = v.abs();
        if a.y >= a.x && a.y >= a.z {
            if v.y >= 0.0 { Facing::Up } else { Facing::Down }
        } else if a.x >= a.z {
            if v.x >= 0.0 { Facing::East } else { Facing::West }
        } else if v.z >= 0.0 {
            Facing::South
        } else {
            Facing::North
        }
    }
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
    /// Item icons (armor); `HELMET + n` is the icon for armor slot `n`.
    pub const HELMET: u8 = 13;
    pub const CHESTPLATE: u8 = 14;
    pub const LEGGINGS: u8 = 15;
    pub const BOOTS: u8 = 16;
    pub const OBSIDIAN: u8 = 17;
    pub const LAVA: u8 = 18;
    pub const DUST_OFF: u8 = 19;
    pub const DUST_ON: u8 = 20;
    pub const TORCH_OFF: u8 = 21;
    pub const TORCH_ON: u8 = 22;
    pub const PISTON_TOP: u8 = 23;
    pub const PISTON_STICKY: u8 = 24;
    pub const PISTON_SIDE: u8 = 25;
    pub const PISTON_BOTTOM: u8 = 26;
    pub const LAMP_OFF: u8 = 27;
    pub const LAMP_ON: u8 = 28;
    pub const LEVER: u8 = 29;
    pub const COUNT: u8 = 30;

    /// Added to the tile index in the mesh's vertex colour: the shader draws it fully bright.
    pub const EMISSIVE: u8 = 64;
}

/// Fluid cell state inside a family: 0 = source, `1..=7` = flowing with that distance from a
/// source, [`FALLING`] = a column of fluid that is falling.
pub const FALLING: u8 = 8;
/// How many blocks water / lava can spread horizontally from a source.
pub const WATER_REACH: u8 = 7;
pub const LAVA_REACH: u8 = 3;

/// A state family: `canonical` is index 0, indices `1..len` occupy `base..base + len - 1`.
#[derive(Clone, Copy)]
struct Family {
    canonical: u8,
    base: u8,
    len: u8,
}

impl Family {
    const fn id(self, idx: u8) -> BlockId {
        if idx == 0 { BlockId(self.canonical) } else { BlockId(self.base + idx - 1) }
    }

    const fn index(self, id: u8) -> Option<u8> {
        if id == self.canonical {
            Some(0)
        } else if id >= self.base && id < self.base + self.len - 1 {
            Some(id - self.base + 1)
        } else {
            None
        }
    }
}

mod fam {
    use super::Family;
    pub const WATER: Family = Family { canonical: 5, base: 104, len: 9 }; // 104..=111
    pub const LAVA: Family = Family { canonical: 13, base: 112, len: 9 }; // 112..=119
    pub const DUST: Family = Family { canonical: 14, base: 120, len: 16 }; // 120..=134
    pub const LAMP: Family = Family { canonical: 21, base: 135, len: 2 }; // 135
    pub const TORCH: Family = Family { canonical: 15, base: 136, len: 10 }; // 136..=144
    pub const LEVER: Family = Family { canonical: 16, base: 145, len: 10 }; // 145..=153
    pub const PLATE: Family = Family { canonical: 17, base: 154, len: 2 }; // 154
    pub const BUTTON: Family = Family { canonical: 18, base: 155, len: 10 }; // 155..=163
    pub const PISTON: Family = Family { canonical: 19, base: 164, len: 12 }; // 164..=174
    pub const STICKY: Family = Family { canonical: 20, base: 175, len: 12 }; // 175..=185
    pub const HEAD: Family = Family { canonical: 186, base: 187, len: 12 }; // 186..=197
}

/// Directions a torch / lever / button can be attached in: index 0 sits on the floor, `1..=4`
/// hang on a wall. The value is the direction from the block towards its support.
pub const ATTACH: [Facing; 5] = [Facing::Down, Facing::North, Facing::South, Facing::West, Facing::East];

pub fn attach_index(support: Facing) -> Option<u8> {
    ATTACH.iter().position(|f| *f == support).map(|i| i as u8)
}

/// What a block id means. Decoded from the id, see [`BlockId::kind`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Air,
    /// Ordinary full opaque cube (also obsidian, bedrock, ...).
    Solid,
    /// Fluid state `0..=FALLING`.
    Water(u8),
    Lava(u8),
    Dust {
        power: u8,
    },
    Torch {
        attach: Facing,
        lit: bool,
    },
    Lever {
        attach: Facing,
        on: bool,
    },
    Plate {
        pressed: bool,
    },
    Button {
        attach: Facing,
        pressed: bool,
    },
    Piston {
        sticky: bool,
        facing: Facing,
        extended: bool,
    },
    PistonHead {
        sticky: bool,
        facing: Facing,
    },
    Lamp {
        lit: bool,
    },
}

const fn decode(id: u8) -> Kind {
    if id == 0 {
        return Kind::Air;
    }
    if let Some(i) = fam::WATER.index(id) {
        return Kind::Water(i);
    }
    if let Some(i) = fam::LAVA.index(id) {
        return Kind::Lava(i);
    }
    if let Some(i) = fam::DUST.index(id) {
        return Kind::Dust { power: i };
    }
    if let Some(i) = fam::LAMP.index(id) {
        return Kind::Lamp { lit: i == 1 };
    }
    if let Some(i) = fam::TORCH.index(id) {
        return Kind::Torch { attach: ATTACH[(i / 2) as usize], lit: i % 2 == 0 };
    }
    if let Some(i) = fam::LEVER.index(id) {
        return Kind::Lever { attach: ATTACH[(i / 2) as usize], on: i % 2 == 1 };
    }
    if let Some(i) = fam::PLATE.index(id) {
        return Kind::Plate { pressed: i == 1 };
    }
    if let Some(i) = fam::BUTTON.index(id) {
        return Kind::Button { attach: ATTACH[(i / 2) as usize], pressed: i % 2 == 1 };
    }
    if let Some(i) = fam::PISTON.index(id) {
        return Kind::Piston { sticky: false, facing: Facing::from_index(i / 2), extended: i % 2 == 1 };
    }
    if let Some(i) = fam::STICKY.index(id) {
        return Kind::Piston { sticky: true, facing: Facing::from_index(i / 2), extended: i % 2 == 1 };
    }
    if let Some(i) = fam::HEAD.index(id) {
        return Kind::PistonHead { sticky: i % 2 == 1, facing: Facing::from_index(i / 2) };
    }
    Kind::Solid
}

const F_OPAQUE: u8 = 1;
const F_SOLID: u8 = 2;
const F_FLUID: u8 = 4;
const F_MODEL: u8 = 8;
const F_REPLACEABLE: u8 = 16;
const F_BREAKABLE: u8 = 32;
const F_KNOWN: u8 = 64;

const fn flags_of(id: u8) -> u8 {
    match decode(id) {
        Kind::Air => F_REPLACEABLE | F_KNOWN,
        Kind::Solid => {
            if id == 9 {
                F_OPAQUE | F_SOLID | F_KNOWN
            } else if (id >= 1 && id <= 12) || id == 0 {
                F_OPAQUE | F_SOLID | F_BREAKABLE | F_KNOWN
            } else {
                // An id nobody defined (corrupt or newer save): a plain breakable cube, but not `known`.
                F_OPAQUE | F_SOLID | F_BREAKABLE
            }
        }
        Kind::Water(_) | Kind::Lava(_) => F_FLUID | F_REPLACEABLE | F_KNOWN,
        Kind::Dust { .. } | Kind::Torch { .. } | Kind::Lever { .. } | Kind::Plate { .. } | Kind::Button { .. } => {
            F_MODEL | F_BREAKABLE | F_KNOWN
        }
        Kind::PistonHead { .. } => F_MODEL | F_SOLID | F_BREAKABLE | F_KNOWN,
        Kind::Piston { .. } | Kind::Lamp { .. } => F_OPAQUE | F_SOLID | F_BREAKABLE | F_KNOWN,
    }
}

const FLAGS: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = flags_of(i as u8);
        i += 1;
    }
    t
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(transparent)]
pub struct BlockId(pub u8);

impl BlockId {
    pub const AIR: BlockId = BlockId(0);
    pub const GRASS: BlockId = BlockId(1);
    pub const DIRT: BlockId = BlockId(2);
    pub const STONE: BlockId = BlockId(3);
    pub const SAND: BlockId = BlockId(4);
    /// Water source block. Flowing water has other ids, see [`BlockId::water`].
    pub const WATER: BlockId = BlockId(5);
    pub const LOG: BlockId = BlockId(6);
    pub const LEAVES: BlockId = BlockId(7);
    pub const PLANKS: BlockId = BlockId(8);
    pub const BEDROCK: BlockId = BlockId(9);
    pub const SNOW: BlockId = BlockId(10);
    pub const COBBLE: BlockId = BlockId(11);
    pub const OBSIDIAN: BlockId = BlockId(12);
    pub const LAVA: BlockId = BlockId(13);
    pub const REDSTONE_DUST: BlockId = BlockId(14);
    pub const REDSTONE_TORCH: BlockId = BlockId(15);
    pub const LEVER: BlockId = BlockId(16);
    pub const STONE_PLATE: BlockId = BlockId(17);
    pub const STONE_BUTTON: BlockId = BlockId(18);
    pub const PISTON: BlockId = BlockId(19);
    pub const STICKY_PISTON: BlockId = BlockId(20);
    pub const REDSTONE_LAMP: BlockId = BlockId(21);

    #[inline]
    pub fn kind(self) -> Kind {
        decode(self.0)
    }

    #[inline]
    fn flag(self, f: u8) -> bool {
        FLAGS[self.0 as usize] & f != 0
    }

    // ---- state constructors ----

    pub const fn water(state: u8) -> BlockId {
        fam::WATER.id(state)
    }

    pub const fn lava(state: u8) -> BlockId {
        fam::LAVA.id(state)
    }

    pub const fn dust(power: u8) -> BlockId {
        fam::DUST.id(power)
    }

    pub const fn lamp(lit: bool) -> BlockId {
        fam::LAMP.id(lit as u8)
    }

    pub const fn torch(attach: Facing, lit: bool) -> BlockId {
        fam::TORCH.id(attach_slot(attach) * 2 + !lit as u8)
    }

    pub const fn lever(attach: Facing, on: bool) -> BlockId {
        fam::LEVER.id(attach_slot(attach) * 2 + on as u8)
    }

    pub const fn plate(pressed: bool) -> BlockId {
        fam::PLATE.id(pressed as u8)
    }

    pub const fn button(attach: Facing, pressed: bool) -> BlockId {
        fam::BUTTON.id(attach_slot(attach) * 2 + pressed as u8)
    }

    pub const fn piston(sticky: bool, facing: Facing, extended: bool) -> BlockId {
        let i = facing.index() * 2 + extended as u8;
        if sticky { fam::STICKY.id(i) } else { fam::PISTON.id(i) }
    }

    pub const fn piston_head(sticky: bool, facing: Facing) -> BlockId {
        fam::HEAD.id(facing.index() * 2 + sticky as u8)
    }

    // ---- classification ----

    #[inline]
    pub fn is_air(self) -> bool {
        self == Self::AIR
    }

    #[inline]
    pub fn is_water(self) -> bool {
        matches!(self.kind(), Kind::Water(_))
    }

    #[inline]
    pub fn is_lava(self) -> bool {
        matches!(self.kind(), Kind::Lava(_))
    }

    #[inline]
    pub fn is_fluid(self) -> bool {
        self.flag(F_FLUID)
    }

    /// Hides the faces of its neighbours and is drawn in the opaque pass.
    #[inline]
    pub fn is_opaque(self) -> bool {
        self.flag(F_OPAQUE)
    }

    /// Drawn from a list of boxes instead of a full cube (dust, torches, levers, ...).
    #[inline]
    pub fn is_model(self) -> bool {
        self.flag(F_MODEL)
    }

    /// Blocks the movement of players and mobs.
    #[inline]
    pub fn is_solid(self) -> bool {
        self.flag(F_SOLID)
    }

    /// Can be selected by the crosshair ray (fluids are looked through).
    #[inline]
    pub fn is_targetable(self) -> bool {
        self.flag(F_OPAQUE | F_MODEL)
    }

    /// A placed block may overwrite this one.
    #[inline]
    pub fn is_replaceable(self) -> bool {
        self.flag(F_REPLACEABLE)
    }

    #[inline]
    pub fn is_breakable(self) -> bool {
        self.flag(F_BREAKABLE)
    }

    /// Ids that have a name, texture and behaviour. Items loaded from disk are checked with it.
    #[inline]
    pub fn is_known(self) -> bool {
        self.flag(F_KNOWN)
    }

    /// Redstone dust, torches, levers, buttons and plates are destroyed by flowing fluid and by
    /// pistons pushing into them.
    #[inline]
    pub fn is_fragile(self) -> bool {
        self.is_model() && !matches!(self.kind(), Kind::PistonHead { .. })
    }

    /// The block that can be lit by redstone / conducts a signal from a source next to it.
    #[inline]
    pub fn is_conductor(self) -> bool {
        self.is_opaque() && !matches!(self.kind(), Kind::Lamp { .. } | Kind::Piston { .. }) && self != Self::LEAVES
    }

    /// The inventory item that stands for this block (state variants map to their family's
    /// canonical id; stone drops cobblestone, grass drops dirt). `None` if nothing drops.
    pub fn drop_block(self) -> Option<BlockId> {
        if !self.is_breakable() {
            return None;
        }
        Some(match self.kind() {
            Kind::Solid => match self {
                Self::STONE => Self::COBBLE,
                Self::GRASS => Self::DIRT,
                other => other,
            },
            Kind::Dust { .. } => Self::REDSTONE_DUST,
            Kind::Lamp { .. } => Self::REDSTONE_LAMP,
            Kind::Torch { .. } => Self::REDSTONE_TORCH,
            Kind::Lever { .. } => Self::LEVER,
            Kind::Plate { .. } => Self::STONE_PLATE,
            Kind::Button { .. } => Self::STONE_BUTTON,
            Kind::Piston { sticky, .. } => {
                if sticky {
                    Self::STICKY_PISTON
                } else {
                    Self::PISTON
                }
            }
            Kind::PistonHead { .. } | Kind::Air | Kind::Water(_) | Kind::Lava(_) => return None,
        })
    }

    /// Fully lit regardless of daylight (lava, lit lamps and torches, powered dust).
    pub fn is_emissive(self) -> bool {
        match self.kind() {
            Kind::Lava(_) => true,
            Kind::Lamp { lit } | Kind::Torch { lit, .. } => lit,
            Kind::Dust { power } => power > 0,
            _ => false,
        }
    }

    /// Does this block keep mobs from spawning next to it (it gives off light)?
    pub fn gives_light(self) -> bool {
        match self.kind() {
            Kind::Lava(_) => true,
            Kind::Lamp { lit } | Kind::Torch { lit, .. } => lit,
            _ => false,
        }
    }

    pub fn tile(self, face: Face) -> u8 {
        self.tile_facing(match face {
            Face::Top => Facing::Up,
            Face::Bottom => Facing::Down,
            Face::Side => Facing::North,
        })
    }

    /// Atlas tile of the face pointing in direction `f`.
    pub fn tile_facing(self, f: Facing) -> u8 {
        let side = !matches!(f, Facing::Up | Facing::Down);
        match self.kind() {
            Kind::Water(_) => return tile::WATER,
            Kind::Lava(_) => return tile::LAVA,
            Kind::Dust { power } => return if power > 0 { tile::DUST_ON } else { tile::DUST_OFF },
            Kind::Torch { lit, .. } => return if lit { tile::TORCH_ON } else { tile::TORCH_OFF },
            Kind::Lever { .. } => return tile::LEVER,
            Kind::Plate { .. } | Kind::Button { .. } => return tile::STONE,
            Kind::Lamp { lit } => return if lit { tile::LAMP_ON } else { tile::LAMP_OFF },
            Kind::Piston { sticky, facing, .. } => {
                return if f == facing {
                    if sticky { tile::PISTON_STICKY } else { tile::PISTON_TOP }
                } else if f == facing.opposite() {
                    tile::PISTON_BOTTOM
                } else {
                    tile::PISTON_SIDE
                };
            }
            Kind::PistonHead { sticky, facing } => {
                return if f == facing {
                    if sticky { tile::PISTON_STICKY } else { tile::PISTON_TOP }
                } else if f == facing.opposite() {
                    tile::PISTON_BOTTOM
                } else {
                    tile::PLANKS
                };
            }
            _ => {}
        }
        match (self, f) {
            (Self::GRASS, Facing::Up) => tile::GRASS_TOP,
            (Self::GRASS, _) if side => tile::GRASS_SIDE,
            (Self::GRASS, _) | (Self::DIRT, _) => tile::DIRT,
            (Self::STONE, _) => tile::STONE,
            (Self::SAND, _) => tile::SAND,
            (Self::LOG, _) if side => tile::LOG_SIDE,
            (Self::LOG, _) => tile::LOG_TOP,
            (Self::LEAVES, _) => tile::LEAVES,
            (Self::PLANKS, _) => tile::PLANKS,
            (Self::BEDROCK, _) => tile::BEDROCK,
            (Self::SNOW, _) => tile::SNOW,
            (Self::COBBLE, _) => tile::COBBLE,
            (Self::OBSIDIAN, _) => tile::OBSIDIAN,
            _ => tile::STONE,
        }
    }

    pub fn name(self) -> &'static str {
        match self.kind() {
            Kind::Air => "Air",
            Kind::Water(_) => "Water",
            Kind::Lava(_) => "Lava",
            Kind::Dust { .. } => "Redstone Dust",
            Kind::Torch { .. } => "Redstone Torch",
            Kind::Lever { .. } => "Lever",
            Kind::Plate { .. } => "Pressure Plate",
            Kind::Button { .. } => "Button",
            Kind::Lamp { .. } => "Redstone Lamp",
            Kind::Piston { sticky: false, .. } => "Piston",
            Kind::Piston { sticky: true, .. } => "Sticky Piston",
            Kind::PistonHead { .. } => "Piston Head",
            Kind::Solid => match self {
                Self::GRASS => "Grass",
                Self::DIRT => "Dirt",
                Self::STONE => "Stone",
                Self::SAND => "Sand",
                Self::LOG => "Log",
                Self::LEAVES => "Leaves",
                Self::PLANKS => "Planks",
                Self::BEDROCK => "Bedrock",
                Self::SNOW => "Snow",
                Self::COBBLE => "Cobble",
                Self::OBSIDIAN => "Obsidian",
                _ => "Unknown",
            },
        }
    }

    /// Representative sRGB colour for particles and the hotbar.
    pub fn base_color(self) -> [f32; 3] {
        match self.kind() {
            Kind::Water(_) => return [0.22, 0.40, 0.80],
            Kind::Lava(_) => return [1.0, 0.45, 0.08],
            Kind::Dust { .. } => return [0.8, 0.1, 0.1],
            Kind::Torch { .. } => return [0.9, 0.3, 0.1],
            Kind::Lever { .. } => return [0.5, 0.4, 0.25],
            Kind::Plate { .. } | Kind::Button { .. } => return [0.5, 0.5, 0.52],
            Kind::Lamp { lit } => return if lit { [1.0, 0.85, 0.5] } else { [0.45, 0.32, 0.2] },
            Kind::Piston { .. } | Kind::PistonHead { .. } => return [0.62, 0.5, 0.32],
            _ => {}
        }
        match self {
            Self::GRASS => [0.36, 0.62, 0.22],
            Self::DIRT => [0.47, 0.33, 0.20],
            Self::STONE => [0.50, 0.50, 0.52],
            Self::SAND => [0.85, 0.80, 0.55],
            Self::LOG => [0.40, 0.29, 0.16],
            Self::LEAVES => [0.20, 0.45, 0.16],
            Self::PLANKS => [0.68, 0.53, 0.30],
            Self::BEDROCK => [0.18, 0.18, 0.20],
            Self::SNOW => [0.94, 0.96, 0.98],
            Self::COBBLE => [0.42, 0.42, 0.44],
            Self::OBSIDIAN => [0.10, 0.06, 0.18],
            _ => [1.0, 0.0, 1.0],
        }
    }

    /// Blast resistance used by explosions (stone 6, dirt 0.5, ... like the real game's scale).
    pub fn blast_resistance(self) -> f32 {
        match self.kind() {
            Kind::Air => 0.0,
            Kind::Water(_) | Kind::Lava(_) => 100.0,
            Kind::Dust { .. } | Kind::Torch { .. } | Kind::Lever { .. } | Kind::Button { .. } => 0.0,
            Kind::Plate { .. } => 0.5,
            Kind::Piston { .. } | Kind::PistonHead { .. } => 0.5,
            Kind::Lamp { .. } => 0.3,
            Kind::Solid => match self {
                Self::DIRT | Self::GRASS | Self::SAND => 0.5,
                Self::SNOW => 0.1,
                Self::LEAVES => 0.2,
                Self::LOG | Self::PLANKS => 2.0,
                Self::STONE | Self::COBBLE => 6.0,
                Self::OBSIDIAN => 1200.0,
                Self::BEDROCK => 3_600_000.0,
                _ => 1.0,
            },
        }
    }
}

const fn attach_slot(f: Facing) -> u8 {
    match f {
        Facing::North => 1,
        Facing::South => 2,
        Facing::West => 3,
        Facing::East => 4,
        _ => 0,
    }
}

const PX: f32 = 1.0 / 16.0;

/// Maps a box given in the "support frame" of an attached part to cell coordinates. `w` runs
/// away from the support, `a` is the lean axis (x on the floor, y on walls), `b` the other one.
pub fn from_support_frame(support: Facing, a: f32, b: f32, w: f32) -> bevy::math::Vec3 {
    match support {
        Facing::Down => bevy::math::Vec3::new(a, w, b),
        Facing::Up => bevy::math::Vec3::new(a, 1.0 - w, b),
        Facing::North => bevy::math::Vec3::new(b, a, w),
        Facing::South => bevy::math::Vec3::new(b, a, 1.0 - w),
        Facing::West => bevy::math::Vec3::new(w, a, b),
        Facing::East => bevy::math::Vec3::new(1.0 - w, a, b),
    }
}

/// Box `a0..a1, b0..b1, w0..w1` in the support frame, translated to the cell at `origin`.
pub fn framed_box(
    origin: bevy::math::Vec3,
    support: Facing,
    a: (f32, f32),
    b: (f32, f32),
    w: (f32, f32),
) -> (bevy::math::Vec3, bevy::math::Vec3) {
    let p = from_support_frame(support, a.0, b.0, w.0);
    let q = from_support_frame(support, a.1, b.1, w.1);
    (origin + p.min(q), origin + p.max(q))
}

/// Bounding box (cell-local, `min`, `max`) used for crosshair selection of a model block.
pub fn select_box(id: BlockId) -> (bevy::math::Vec3, bevy::math::Vec3) {
    use bevy::math::Vec3;
    let o = Vec3::ZERO;
    match id.kind() {
        Kind::Dust { .. } => (o, Vec3::new(1.0, PX, 1.0)),
        Kind::Plate { pressed } => {
            (Vec3::new(PX, 0.0, PX), Vec3::new(15.0 * PX, if pressed { 0.5 * PX } else { PX }, 15.0 * PX))
        }
        Kind::Torch { attach, .. } => {
            if attach == Facing::Down {
                (Vec3::new(6.0 * PX, 0.0, 6.0 * PX), Vec3::new(10.0 * PX, 10.0 * PX, 10.0 * PX))
            } else {
                framed_box(o, attach, (3.0 * PX, 13.0 * PX), (6.0 * PX, 10.0 * PX), (5.0 * PX, 8.0 * PX))
            }
        }
        Kind::Lever { attach, .. } => {
            framed_box(o, attach, (3.0 * PX, 13.0 * PX), (5.0 * PX, 11.0 * PX), (0.0, 11.0 * PX))
        }
        Kind::Button { attach, pressed } => framed_box(
            o,
            attach,
            (5.0 * PX, 11.0 * PX),
            (6.0 * PX, 10.0 * PX),
            (0.0, if pressed { PX } else { 2.0 * PX }),
        ),
        _ => (o, Vec3::ONE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_ids() -> impl Iterator<Item = BlockId> {
        (0..=255u8).map(BlockId)
    }

    #[test]
    fn every_known_face_maps_to_a_valid_tile() {
        for id in all_ids().filter(|b| b.is_known()) {
            for f in Facing::ALL {
                assert!(id.tile_facing(f) < tile::COUNT, "block {} {f:?}", id.0);
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
        assert!(BlockId::OBSIDIAN.is_solid() && BlockId::OBSIDIAN.is_opaque());
        assert!(BlockId::lava(0).is_fluid() && !BlockId::lava(0).is_targetable());
        assert!(BlockId::LEVER.is_targetable() && !BlockId::LEVER.is_solid());
        assert!(BlockId::dust(7).is_model() && BlockId::dust(7).is_fragile());
        assert!(BlockId::piston(false, Facing::Up, false).is_solid());
        assert!(BlockId::piston_head(false, Facing::Up).is_solid());
        assert!(!BlockId::piston_head(false, Facing::Up).is_fragile());
    }

    #[test]
    fn families_round_trip_and_never_collide() {
        let mut seen = std::collections::HashMap::new();
        let mut put = |id: BlockId, what: String| {
            let prev = seen.insert(id, what.clone());
            assert!(prev.is_none(), "{what} collides with {prev:?}");
            assert!(id.0 < 100 || id.0 > 103, "{what} uses an armor item id");
            assert!(id.is_known(), "{what}");
        };
        for s in 0..=FALLING {
            put(BlockId::water(s), format!("water {s}"));
            put(BlockId::lava(s), format!("lava {s}"));
            assert_eq!(BlockId::water(s).kind(), Kind::Water(s));
            assert_eq!(BlockId::lava(s).kind(), Kind::Lava(s));
        }
        for p in 0..=15 {
            put(BlockId::dust(p), format!("dust {p}"));
            assert_eq!(BlockId::dust(p).kind(), Kind::Dust { power: p });
        }
        for lit in [false, true] {
            put(BlockId::lamp(lit), format!("lamp {lit}"));
            assert_eq!(BlockId::lamp(lit).kind(), Kind::Lamp { lit });
            for a in ATTACH {
                put(BlockId::torch(a, lit), format!("torch {a:?} {lit}"));
                assert_eq!(BlockId::torch(a, lit).kind(), Kind::Torch { attach: a, lit });
                put(BlockId::lever(a, lit), format!("lever {a:?} {lit}"));
                assert_eq!(BlockId::lever(a, lit).kind(), Kind::Lever { attach: a, on: lit });
                put(BlockId::button(a, lit), format!("button {a:?} {lit}"));
                assert_eq!(BlockId::button(a, lit).kind(), Kind::Button { attach: a, pressed: lit });
            }
            put(BlockId::plate(lit), format!("plate {lit}"));
            assert_eq!(BlockId::plate(lit).kind(), Kind::Plate { pressed: lit });
            for f in Facing::ALL {
                for ext in [false, true] {
                    for sticky in [false, true] {
                        let id = BlockId::piston(sticky, f, ext);
                        if !lit {
                            put(id, format!("piston {sticky} {f:?} {ext}"));
                        }
                        assert_eq!(id.kind(), Kind::Piston { sticky, facing: f, extended: ext });
                    }
                }
                put(BlockId::piston_head(lit, f), format!("head {lit} {f:?}"));
                assert_eq!(BlockId::piston_head(lit, f).kind(), Kind::PistonHead { sticky: lit, facing: f });
            }
        }
    }

    #[test]
    fn canonical_ids_are_items_and_variants_drop_them() {
        for canonical in 1..=21u8 {
            assert!(BlockId(canonical).is_known(), "{canonical}");
        }
        assert_eq!(BlockId::dust(9).drop_block(), Some(BlockId::REDSTONE_DUST));
        assert_eq!(BlockId::torch(Facing::East, false).drop_block(), Some(BlockId::REDSTONE_TORCH));
        assert_eq!(BlockId::piston(true, Facing::North, true).drop_block(), Some(BlockId::STICKY_PISTON));
        assert_eq!(BlockId::STONE.drop_block(), Some(BlockId::COBBLE));
        assert_eq!(BlockId::lava(0).drop_block(), None);
        assert_eq!(BlockId::piston_head(false, Facing::Up).drop_block(), None);
        assert_eq!(BlockId::BEDROCK.drop_block(), None);
    }

    #[test]
    fn facing_helpers() {
        for f in Facing::ALL {
            assert_eq!(f.offset() + f.opposite().offset(), IVec3::ZERO);
            assert_eq!(Facing::from_offset(f.offset()), Some(f));
            assert_eq!(Facing::from_index(f.index()), f);
        }
        assert_eq!(Facing::dominant(bevy::math::Vec3::new(0.2, 0.1, -1.0)), Facing::North);
        assert_eq!(Facing::dominant(bevy::math::Vec3::new(0.0, -2.0, 1.0)), Facing::Down);
    }
}
