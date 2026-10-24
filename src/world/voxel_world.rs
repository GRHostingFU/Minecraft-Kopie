use super::block::BlockId;
use super::chunk::{Chunk, ChunkPos, split_world_pos};
use bevy::math::{IVec2, IVec3};
use bevy::prelude::{Message, MessageWriter, ResMut, Resource};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Read access to blocks, shared by physics, raycasting and tests.
/// `None` means the block lies in a chunk that is not loaded.
pub trait BlockQuery {
    fn block(&self, pos: IVec3) -> Option<BlockId>;
}

/// Read and write access to blocks for the simulations (fluids, redstone, explosions). The
/// game uses [`VoxelWorld`]; tests use a plain map.
pub trait BlockAccess: BlockQuery {
    /// Replaces the block at `pos`. Returns `false` if that position is not loaded.
    fn set(&mut self, pos: IVec3, id: BlockId) -> bool;
}

/// One edit of one block, recorded by [`VoxelWorld::set_block`] for the simulations
/// (fluids, redstone, mobs) that react to changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockChange {
    pub pos: IVec3,
    pub old: BlockId,
    pub new: BlockId,
}

/// Broadcast form of [`BlockChange`], delivered one frame after the edit.
#[derive(Message, Clone, Copy, Debug)]
pub struct BlockChanged(pub BlockChange);

/// A chunk was inserted into the world (generated or loaded from disk).
#[derive(Message, Clone, Copy, Debug)]
pub struct ChunkLoaded(pub ChunkPos);

/// Turns the world's change log into messages for the simulations.
pub fn dispatch_world_events(
    mut world: ResMut<VoxelWorld>,
    mut changes: MessageWriter<BlockChanged>,
    mut loaded: MessageWriter<ChunkLoaded>,
) {
    // Avoid marking the resource changed on idle frames.
    if world.changes.is_empty() && world.new_chunks.is_empty() {
        return;
    }
    for c in world.take_changes() {
        changes.write(BlockChanged(c));
    }
    for p in world.take_new_chunks() {
        loaded.write(ChunkLoaded(p));
    }
}

/// Upper bound for the change log if nobody drains it (headless tests).
const MAX_LOGGED_CHANGES: usize = 1 << 20;

/// All loaded chunks plus the set of chunks whose mesh is out of date.
#[derive(Resource, Default)]
pub struct VoxelWorld {
    /// Shared so mesher threads can snapshot neighbours without copying; edits are copy-on-write.
    chunks: HashMap<ChunkPos, Arc<Chunk>>,
    needs_mesh: HashSet<ChunkPos>,
    changes: Vec<BlockChange>,
    new_chunks: Vec<ChunkPos>,
}

impl VoxelWorld {
    pub fn chunk(&self, pos: ChunkPos) -> Option<&Chunk> {
        self.chunks.get(&pos).map(Arc::as_ref)
    }

    pub fn chunk_arc(&self, pos: ChunkPos) -> Option<Arc<Chunk>> {
        self.chunks.get(&pos).cloned()
    }

    pub fn is_loaded(&self, pos: ChunkPos) -> bool {
        self.chunks.contains_key(&pos)
    }

    pub fn loaded_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn loaded_positions(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        self.chunks.keys().copied()
    }

    /// Inserts a freshly generated/loaded chunk. The chunk and every loaded
    /// neighbour must be re-meshed because border faces and AO depend on it.
    pub fn insert_chunk(&mut self, pos: ChunkPos, chunk: Chunk) {
        self.chunks.insert(pos, Arc::new(chunk));
        self.new_chunks.push(pos);
        self.mark_with_neighbours(pos);
    }

    pub fn remove_chunk(&mut self, pos: ChunkPos) -> Option<Arc<Chunk>> {
        self.needs_mesh.remove(&pos);
        self.chunks.remove(&pos)
    }

    pub fn modified_chunks(&self) -> impl Iterator<Item = (ChunkPos, &Chunk)> {
        self.chunks.iter().filter(|(_, c)| c.modified).map(|(p, c)| (*p, c.as_ref()))
    }

    pub fn clear_modified(&mut self, pos: ChunkPos) {
        if let Some(c) = self.chunks.get_mut(&pos) {
            Arc::make_mut(c).modified = false;
        }
    }

    /// Edits one block. Returns `false` if the chunk is not loaded.
    pub fn set_block(&mut self, pos: IVec3, id: BlockId) -> bool {
        if !(0..super::chunk::CHUNK_H as i32).contains(&pos.y) {
            return false;
        }
        let (cp, [x, y, z]) = split_world_pos(pos);
        let Some(chunk) = self.chunks.get_mut(&cp) else {
            return false;
        };
        let old = chunk.get(x, y, z);
        if old == id {
            return true;
        }
        if self.changes.len() < MAX_LOGGED_CHANGES {
            self.changes.push(BlockChange { pos, old, new: id });
        }
        let chunk = self.chunks.get_mut(&cp).expect("chunk checked above");
        let chunk = Arc::make_mut(chunk);
        chunk.set(x, y, z, id);
        chunk.modified = true;
        self.needs_mesh.insert(cp);
        // Edits on a chunk border change the neighbour's faces and AO too
        // (including the diagonal neighbours at corners).
        let (dx_lo, dx_hi) = (x == 0, x == 15);
        let (dz_lo, dz_hi) = (z == 0, z == 15);
        for dx in [-1, 0, 1] {
            for dz in [-1, 0, 1] {
                let touches_x = dx == 0 || (dx == -1 && dx_lo) || (dx == 1 && dx_hi);
                let touches_z = dz == 0 || (dz == -1 && dz_lo) || (dz == 1 && dz_hi);
                if touches_x && touches_z && (dx != 0 || dz != 0) {
                    let n = cp + IVec2::new(dx, dz);
                    if self.chunks.contains_key(&n) {
                        self.needs_mesh.insert(n);
                    }
                }
            }
        }
        true
    }

    fn mark_with_neighbours(&mut self, pos: ChunkPos) {
        for dx in -1..=1 {
            for dz in -1..=1 {
                let n = pos + IVec2::new(dx, dz);
                if self.chunks.contains_key(&n) {
                    self.needs_mesh.insert(n);
                }
            }
        }
    }

    /// Block at `pos`, treating unloaded space as air.
    pub fn block_at(&self, pos: IVec3) -> BlockId {
        self.block(pos).unwrap_or(BlockId::AIR)
    }

    /// Edits since the last call, oldest first.
    pub fn take_changes(&mut self) -> Vec<BlockChange> {
        std::mem::take(&mut self.changes)
    }

    /// Chunks inserted since the last call (simulations scan them for work to schedule).
    pub fn take_new_chunks(&mut self) -> Vec<ChunkPos> {
        std::mem::take(&mut self.new_chunks)
    }

    pub fn take_needs_mesh(&mut self) -> Vec<ChunkPos> {
        self.needs_mesh.drain().collect()
    }

    pub fn needs_mesh_count(&self) -> usize {
        self.needs_mesh.len()
    }

    pub fn requeue_mesh(&mut self, pos: ChunkPos) {
        if self.chunks.contains_key(&pos) {
            self.needs_mesh.insert(pos);
        }
    }

    /// Highest solid block in the column containing `(x, z)`.
    pub fn surface_height(&self, x: i32, z: i32) -> Option<i32> {
        let (cp, [lx, _, lz]) = split_world_pos(IVec3::new(x, 0, z));
        let chunk = self.chunks.get(&cp)?;
        (0..super::chunk::CHUNK_H).rev().find(|&y| chunk.get(lx, y, lz).is_solid()).map(|y| y as i32)
    }
}

impl BlockAccess for VoxelWorld {
    fn set(&mut self, pos: IVec3, id: BlockId) -> bool {
        self.set_block(pos, id)
    }
}

impl BlockQuery for VoxelWorld {
    fn block(&self, pos: IVec3) -> Option<BlockId> {
        if pos.y < 0 {
            return Some(BlockId::BEDROCK);
        }
        if pos.y >= super::chunk::CHUNK_H as i32 {
            return Some(BlockId::AIR);
        }
        let (cp, [x, y, z]) = split_world_pos(pos);
        self.chunks.get(&cp).map(|c| c.get(x, y, z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_block_marks_neighbours_on_borders() {
        let mut w = VoxelWorld::default();
        for dx in -1..=1 {
            for dz in -1..=1 {
                w.insert_chunk(IVec2::new(dx, dz), Chunk::empty());
            }
        }
        w.take_needs_mesh();
        assert!(w.set_block(IVec3::new(5, 10, 5), BlockId::STONE));
        assert_eq!(w.take_needs_mesh(), vec![IVec2::ZERO]);

        assert!(w.set_block(IVec3::new(0, 10, 0), BlockId::STONE));
        let mut dirty = w.take_needs_mesh();
        dirty.sort_by_key(|p| (p.x, p.y));
        assert_eq!(dirty, vec![IVec2::new(-1, -1), IVec2::new(-1, 0), IVec2::new(0, -1), IVec2::ZERO]);
        assert!(w.chunk(IVec2::ZERO).unwrap().modified);
    }

    #[test]
    fn unloaded_chunks_report_none() {
        let w = VoxelWorld::default();
        assert_eq!(w.block(IVec3::new(0, 10, 0)), None);
        assert_eq!(w.block(IVec3::new(0, -1, 0)), Some(BlockId::BEDROCK));
        assert_eq!(w.block(IVec3::new(0, 300, 0)), Some(BlockId::AIR));
    }

    #[test]
    fn surface_height_finds_top_solid() {
        let mut w = VoxelWorld::default();
        w.insert_chunk(IVec2::ZERO, Chunk::empty());
        w.set_block(IVec3::new(2, 30, 2), BlockId::STONE);
        w.set_block(IVec3::new(2, 31, 2), BlockId::WATER);
        assert_eq!(w.surface_height(2, 2), Some(30));
        assert_eq!(w.surface_height(3, 3), None);
    }
}
