//! Turns loaded chunks into GPU meshes. Meshing runs on the async compute pool;
//! the main thread only snapshots `Arc`'d chunks and uploads finished buffers.

use super::material::VoxelMaterial;
use super::mesher::{ChunkMeshData, MeshBuffers, PaddedVolume, mesh_chunk};
use crate::world::chunk::{CHUNK_H, CHUNK_W, ChunkPos};
use crate::world::streaming::{ChunkAnchor, RemovedChunks, WorldSettings};
use crate::world::{Chunk, VoxelWorld};
use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use std::sync::Arc;

#[derive(Resource)]
pub struct VoxelMaterials {
    pub opaque: Handle<VoxelMaterial>,
    pub water: Handle<VoxelMaterial>,
}

#[derive(Component)]
pub struct ChunkMesh(pub ChunkPos);

#[derive(Default)]
struct RenderedChunk {
    entities: Vec<Entity>,
    quads: usize,
}

#[derive(Resource, Default)]
pub struct MeshState {
    pending: HashSet<ChunkPos>,
    tasks: HashMap<ChunkPos, Task<ChunkMeshData>>,
    rendered: HashMap<ChunkPos, RenderedChunk>,
    pub stats: MeshStats,
}

#[derive(Default, Clone, Copy)]
pub struct MeshStats {
    pub chunks: usize,
    pub quads: usize,
    pub pending: usize,
    pub in_flight: usize,
}

const MAX_MESH_TASKS: usize = 6;
const MAX_UPLOADS_PER_FRAME: usize = 6;

pub struct ChunkMeshPlugin;

impl Plugin for ChunkMeshPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MeshState>()
            .add_systems(Update, (drop_removed_chunks, schedule_meshing, upload_meshes).chain());
    }
}

fn drop_removed_chunks(mut commands: Commands, mut removed: ResMut<RemovedChunks>, mut state: ResMut<MeshState>) {
    for pos in removed.0.drain(..) {
        state.pending.remove(&pos);
        state.tasks.remove(&pos);
        if let Some(r) = state.rendered.remove(&pos) {
            for e in r.entities {
                commands.entity(e).despawn();
            }
        }
    }
}

fn neighbours_ready(world: &VoxelWorld, pos: ChunkPos) -> bool {
    (-1..=1).all(|dz| (-1..=1).all(|dx| world.is_loaded(pos + IVec2::new(dx, dz))))
}

fn schedule_meshing(
    anchor: Query<&Transform, With<ChunkAnchor>>,
    settings: Res<WorldSettings>,
    mut world: ResMut<VoxelWorld>,
    mut state: ResMut<MeshState>,
) {
    let state = &mut *state;
    state.pending.extend(world.take_needs_mesh());
    let center = anchor
        .single()
        .map(|t| IVec2::new((t.translation.x / 16.0).floor() as i32, (t.translation.z / 16.0).floor() as i32))
        .unwrap_or_default();
    let r = settings.render_distance;

    // Only chunks inside the render radius whose 8 neighbours exist can be meshed correctly.
    let mut ready: Vec<ChunkPos> = state
        .pending
        .iter()
        .copied()
        .filter(|p| (*p - center).length_squared() <= r * r && neighbours_ready(&world, *p))
        .collect();
    ready.sort_unstable_by_key(|p| (*p - center).length_squared());

    let pool = AsyncComputeTaskPool::get();
    for pos in ready {
        // A replaced task is dropped, which cancels the stale mesh job.
        let in_flight_new = !state.tasks.contains_key(&pos);
        if in_flight_new && state.tasks.len() >= MAX_MESH_TASKS {
            break;
        }
        let snapshot: Vec<Option<Arc<Chunk>>> = (-1..=1)
            .flat_map(|dz| (-1..=1).map(move |dx| (dx, dz)))
            .map(|(dx, dz)| world.chunk_arc(pos + IVec2::new(dx, dz)))
            .collect();
        state.pending.remove(&pos);
        state.tasks.insert(
            pos,
            pool.spawn(async move {
                let vol = PaddedVolume::build(|dx, dz| snapshot[((dz + 1) * 3 + dx + 1) as usize].as_deref());
                mesh_chunk(&vol)
            }),
        );
    }
    state.stats.pending = state.pending.iter().filter(|p| (**p - center).length_squared() <= r * r).count();
    state.stats.in_flight = state.tasks.len();
}

fn build_mesh(buf: MeshBuffers) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, buf.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, buf.normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, buf.uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, buf.colors);
    mesh.insert_indices(Indices::U32(buf.indices));
    mesh
}

fn upload_meshes(
    mut commands: Commands,
    mut state: ResMut<MeshState>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<VoxelMaterials>,
) {
    let state = &mut *state;
    let mut finished = Vec::new();
    for (pos, task) in state.tasks.iter_mut() {
        if finished.len() >= MAX_UPLOADS_PER_FRAME {
            break;
        }
        if let Some(data) = block_on(poll_once(task)) {
            finished.push((*pos, data));
        }
    }
    for (pos, data) in finished {
        state.tasks.remove(&pos);
        let old = state.rendered.remove(&pos);
        let mut entities = Vec::with_capacity(2);
        let mut quads = 0;
        let origin = Transform::from_xyz((pos.x * CHUNK_W as i32) as f32, 0.0, (pos.y * CHUNK_W as i32) as f32);
        let bounds = Aabb::from_min_max(Vec3::ZERO, Vec3::new(CHUNK_W as f32, CHUNK_H as f32, CHUNK_W as f32));
        for (buf, material) in [(data.opaque, &materials.opaque), (data.water, &materials.water)] {
            if buf.is_empty() {
                continue;
            }
            quads += buf.quad_count();
            entities.push(
                commands
                    .spawn((
                        Mesh3d(meshes.add(build_mesh(buf))),
                        MeshMaterial3d(material.clone()),
                        origin,
                        bounds,
                        ChunkMesh(pos),
                    ))
                    .id(),
            );
        }
        // Swap in the new mesh and retire the old one in the same frame (no flicker).
        if let Some(old) = old {
            for e in old.entities {
                commands.entity(e).despawn();
            }
        }
        state.rendered.insert(pos, RenderedChunk { entities, quads });
    }
    state.stats.chunks = state.rendered.len();
    state.stats.quads = state.rendered.values().map(|r| r.quads).sum();
}
