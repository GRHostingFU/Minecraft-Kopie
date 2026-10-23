//! Chunk streaming: generates/loads chunks around the player on background
//! threads, unloads far chunks and persists edited ones.

use super::chunk::{Chunk, ChunkPos};
use super::generation::TerrainGenerator;
use super::storage;
use super::voxel_world::VoxelWorld;
use bevy::app::OnAppExitSystems;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy::window::ExitSystems;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Marks the entity whose position drives chunk loading.
#[derive(Component)]
pub struct ChunkAnchor;

#[derive(Resource, Clone)]
pub struct WorldSettings {
    pub seed: u32,
    /// Radius (in chunks) that is meshed and drawn; terrain is generated one ring further.
    pub render_distance: i32,
    pub save_dir: PathBuf,
}

impl WorldSettings {
    pub fn load_radius(&self) -> i32 {
        self.render_distance + 1
    }
}

#[derive(Resource)]
pub struct Generator(pub Arc<TerrainGenerator>);

#[derive(Resource, Default)]
struct GenTasks(HashMap<ChunkPos, Task<Chunk>>);

/// Throws away every loaded chunk and every running generation task (a different world is about
/// to be shown). Unsaved edits are lost, so save first.
#[derive(Message, Clone, Copy, Debug)]
pub struct WorldReset;

/// Positions unloaded this frame; the renderer drains it to despawn meshes.
#[derive(Resource, Default)]
pub struct RemovedChunks(pub Vec<ChunkPos>);

#[derive(Resource)]
struct AutosaveTimer(Timer);

const MAX_TASKS_IN_FLIGHT: usize = 6;
const MAX_INSERTS_PER_FRAME: usize = 4;
const AUTOSAVE_SECS: u64 = 30;

pub struct ChunkStreamingPlugin;

impl Plugin for ChunkStreamingPlugin {
    fn build(&self, app: &mut App) {
        let settings = app.world().get_resource::<WorldSettings>().cloned().expect("insert WorldSettings first");
        app.insert_resource(Generator(Arc::new(TerrainGenerator::new(settings.seed))))
            .init_resource::<VoxelWorld>()
            .init_resource::<GenTasks>()
            .init_resource::<RemovedChunks>()
            .insert_resource(AutosaveTimer(Timer::new(Duration::from_secs(AUTOSAVE_SECS), TimerMode::Repeating)))
            .add_message::<WorldReset>()
            .add_systems(Update, (reset_world, request_chunks, collect_chunks, unload_chunks, autosave_chunks).chain())
            .add_systems(Last, save_on_exit.in_set(OnAppExitSystems).after(ExitSystems));
    }
}

fn reset_world(
    mut resets: MessageReader<WorldReset>,
    mut tasks: ResMut<GenTasks>,
    mut world: ResMut<VoxelWorld>,
    mut removed: ResMut<RemovedChunks>,
) {
    if resets.read().next().is_none() {
        return;
    }
    tasks.0.clear();
    let all: Vec<ChunkPos> = world.loaded_positions().collect();
    for pos in all {
        world.remove_chunk(pos);
        removed.0.push(pos);
    }
}

fn anchor_chunk(anchor: &Query<&Transform, With<ChunkAnchor>>) -> Option<ChunkPos> {
    let t = anchor.single().ok()?;
    Some(IVec2::new((t.translation.x / 16.0).floor() as i32, (t.translation.z / 16.0).floor() as i32))
}

fn request_chunks(
    anchor: Query<&Transform, With<ChunkAnchor>>,
    settings: Res<WorldSettings>,
    generator: Res<Generator>,
    world: Res<VoxelWorld>,
    mut tasks: ResMut<GenTasks>,
) {
    let Some(center) = anchor_chunk(&anchor) else { return };
    let r = settings.load_radius();
    // Cancel work that has fallen out of range (dropping a Task cancels it).
    tasks.0.retain(|p, _| (*p - center).length_squared() <= (r + 1) * (r + 1));
    if tasks.0.len() >= MAX_TASKS_IN_FLIGHT {
        return;
    }
    let mut wanted: Vec<(i32, ChunkPos)> = Vec::new();
    for dz in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dz * dz;
            let p = center + IVec2::new(dx, dz);
            if d2 <= r * r && !world.is_loaded(p) && !tasks.0.contains_key(&p) {
                wanted.push((d2, p));
            }
        }
    }
    wanted.sort_unstable_by_key(|(d, _)| *d);
    let pool = AsyncComputeTaskPool::get();
    for (_, pos) in wanted.into_iter().take(MAX_TASKS_IN_FLIGHT - tasks.0.len()) {
        let generator = generator.0.clone();
        let dir = settings.save_dir.clone();
        tasks.0.insert(
            pos,
            pool.spawn(async move {
                match storage::load_chunk(&dir, pos) {
                    Ok(Some(chunk)) => chunk,
                    Ok(None) => generator.generate(pos),
                    Err(e) => {
                        warn!("chunk {pos} unreadable ({e}); regenerating");
                        generator.generate(pos)
                    }
                }
            }),
        );
    }
}

fn collect_chunks(mut tasks: ResMut<GenTasks>, mut world: ResMut<VoxelWorld>) {
    let mut inserted = 0;
    let mut done = Vec::new();
    for (pos, task) in tasks.0.iter_mut() {
        if inserted >= MAX_INSERTS_PER_FRAME {
            break;
        }
        if let Some(chunk) = block_on(poll_once(task)) {
            done.push((*pos, chunk));
            inserted += 1;
        }
    }
    for (pos, chunk) in done {
        tasks.0.remove(&pos);
        world.insert_chunk(pos, chunk);
    }
}

fn unload_chunks(
    anchor: Query<&Transform, With<ChunkAnchor>>,
    settings: Res<WorldSettings>,
    mut world: ResMut<VoxelWorld>,
    mut removed: ResMut<RemovedChunks>,
) {
    let Some(center) = anchor_chunk(&anchor) else { return };
    let keep = settings.load_radius() + 2;
    let far: Vec<ChunkPos> = world.loaded_positions().filter(|p| (*p - center).abs().max_element() > keep).collect();
    for pos in far {
        // A chunk whose edits cannot be written stays loaded: dropping it would lose the edits.
        if let Some(chunk) = world.chunk(pos)
            && chunk.modified
            && let Err(e) = storage::save_chunk(&settings.save_dir, pos, chunk)
        {
            error!("failed to save chunk {pos}, keeping it loaded: {e}");
            continue;
        }
        world.remove_chunk(pos);
        removed.0.push(pos);
    }
}

/// Writes every modified chunk to disk. Returns `(saved, failed)`; failed chunks stay marked as
/// modified so the next save retries them.
pub fn save_modified_chunks(world: &mut VoxelWorld, dir: &std::path::Path) -> (usize, usize) {
    let dirty: Vec<(ChunkPos, Chunk)> = world.modified_chunks().map(|(p, c)| (p, c.clone())).collect();
    let (mut saved, mut failed) = (0, 0);
    for (pos, chunk) in dirty {
        match storage::save_chunk(dir, pos, &chunk) {
            Ok(()) => {
                world.clear_modified(pos);
                saved += 1;
            }
            Err(e) => {
                failed += 1;
                error!("failed to save chunk {pos}: {e}");
            }
        }
    }
    (saved, failed)
}

pub fn save_all_modified(world: &mut VoxelWorld, dir: &std::path::Path) -> usize {
    save_modified_chunks(world, dir).0
}

fn autosave_chunks(
    time: Res<Time>,
    mut timer: ResMut<AutosaveTimer>,
    settings: Res<WorldSettings>,
    mut world: ResMut<VoxelWorld>,
) {
    if timer.0.tick(time.delta()).just_finished() {
        let n = save_all_modified(&mut world, &settings.save_dir);
        if n > 0 {
            info!("autosaved {n} chunks");
        }
    }
}

fn save_on_exit(mut exit: MessageReader<AppExit>, settings: Res<WorldSettings>, mut world: ResMut<VoxelWorld>) {
    if exit.read().next().is_some() {
        let n = save_all_modified(&mut world, &settings.save_dir);
        info!("saved {n} modified chunks on exit");
    }
}
