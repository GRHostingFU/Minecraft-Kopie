use super::block::BlockId;
use bevy::math::{IVec2, IVec3};

pub const CHUNK_W: usize = 16;
pub const CHUNK_H: usize = 256;
pub const CHUNK_VOLUME: usize = CHUNK_W * CHUNK_H * CHUNK_W;

pub type ChunkPos = IVec2;

/// A 16x256x16 column of blocks stored as a flat, boxed byte slice (64 KiB).
/// Index layout is `(y * 16 + z) * 16 + x`, so horizontal slices are contiguous.
#[derive(Clone)]
pub struct Chunk {
    blocks: Box<[BlockId]>,
    /// Edited since it was generated or loaded; such chunks are persisted.
    pub modified: bool,
}

impl Default for Chunk {
    fn default() -> Self {
        Self::empty()
    }
}

impl Chunk {
    pub fn empty() -> Self {
        Self { blocks: vec![BlockId::AIR; CHUNK_VOLUME].into_boxed_slice(), modified: false }
    }

    #[inline]
    pub const fn index(x: usize, y: usize, z: usize) -> usize {
        (y * CHUNK_W + z) * CHUNK_W + x
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> BlockId {
        self.blocks[Self::index(x, y, z)]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, id: BlockId) {
        self.blocks[Self::index(x, y, z)] = id;
    }

    pub fn raw(&self) -> &[BlockId] {
        &self.blocks
    }

    pub fn raw_mut(&mut self) -> &mut [BlockId] {
        &mut self.blocks
    }

    /// Highest non-air block in a column, if any.
    pub fn column_top(&self, x: usize, z: usize) -> Option<usize> {
        (0..CHUNK_H).rev().find(|&y| !self.get(x, y, z).is_air())
    }
}

/// Splits a world-space block position into its chunk and the local offset.
#[inline]
pub fn split_world_pos(p: IVec3) -> (ChunkPos, [usize; 3]) {
    let cx = p.x.div_euclid(CHUNK_W as i32);
    let cz = p.z.div_euclid(CHUNK_W as i32);
    (
        IVec2::new(cx, cz),
        [
            p.x.rem_euclid(CHUNK_W as i32) as usize,
            p.y.clamp(0, CHUNK_H as i32 - 1) as usize,
            p.z.rem_euclid(CHUNK_W as i32) as usize,
        ],
    )
}

#[inline]
pub fn chunk_of(p: IVec3) -> ChunkPos {
    split_world_pos(p).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_handles_negative_coordinates() {
        let (c, l) = split_world_pos(IVec3::new(-1, 70, -17));
        assert_eq!(c, IVec2::new(-1, -2));
        assert_eq!(l, [15, 70, 15]);
        let (c, l) = split_world_pos(IVec3::new(16, 0, 0));
        assert_eq!(c, IVec2::new(1, 0));
        assert_eq!(l, [0, 0, 0]);
    }

    #[test]
    fn set_get_roundtrip_and_column_top() {
        let mut c = Chunk::empty();
        assert_eq!(c.column_top(3, 4), None);
        c.set(3, 100, 4, BlockId::STONE);
        c.set(3, 40, 4, BlockId::DIRT);
        assert_eq!(c.get(3, 100, 4), BlockId::STONE);
        assert_eq!(c.column_top(3, 4), Some(100));
    }
}
