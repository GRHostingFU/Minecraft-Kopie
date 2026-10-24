//! Fluid physics: the cellular water/lava simulation ([`sim`]), the forces they exert on bodies
//! ([`current`]) and the Bevy systems that drive the simulation from world edits.

pub mod current;
pub mod sim;

pub use current::{FluidContact, body_contact, flow_at, fluid_height, surface_in_cell};
pub use sim::{FluidKind, FluidSim};

use crate::world::block::BlockId;
use crate::world::chunk::{CHUNK_W, ChunkPos};
use crate::world::{BlockChanged, ChunkLoaded, VoxelWorld};
use bevy::prelude::*;

/// The running simulation (a resource so tests and tools can inspect the queue).
#[derive(Resource, Default)]
pub struct FluidSimulation(pub FluidSim);

#[derive(Resource, Default)]
struct FluidClock(f32);

/// At most this many game ticks are simulated per frame, so a long frame cannot snowball.
const MAX_TICKS_PER_FRAME: u32 = 6;

pub struct FluidPlugin;

impl Plugin for FluidPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FluidSimulation>()
            .init_resource::<FluidClock>()
            .add_systems(Update, (schedule_from_edits, schedule_from_chunks, run_fluids).chain());
    }
}

fn schedule_from_edits(
    mut changes: MessageReader<BlockChanged>,
    world: Res<VoxelWorld>,
    mut sim: ResMut<FluidSimulation>,
) {
    for BlockChanged(c) in changes.read() {
        sim.0.touch(&*world, c.pos);
    }
}

/// Fluid that was flowing when a chunk was saved resumes when the chunk is loaded again.
fn schedule_from_chunks(
    mut loaded: MessageReader<ChunkLoaded>,
    world: Res<VoxelWorld>,
    mut sim: ResMut<FluidSimulation>,
) {
    for ChunkLoaded(pos) in loaded.read() {
        for p in unsettled_fluid(&world, *pos) {
            sim.0.schedule(&*world, p);
        }
    }
}

/// Positions in a chunk holding flowing or falling fluid (never plain sources).
pub fn unsettled_fluid(world: &VoxelWorld, pos: ChunkPos) -> Vec<IVec3> {
    let Some(chunk) = world.chunk(pos) else { return Vec::new() };
    let mut out = Vec::new();
    for (i, b) in chunk.raw().iter().enumerate() {
        if b.is_fluid() && *b != BlockId::WATER && *b != BlockId::LAVA {
            let (x, z, y) = (i % CHUNK_W, (i / CHUNK_W) % CHUNK_W, i / (CHUNK_W * CHUNK_W));
            out.push(IVec3::new(pos.x * CHUNK_W as i32 + x as i32, y as i32, pos.y * CHUNK_W as i32 + z as i32));
        }
    }
    out
}

fn run_fluids(
    time: Res<Time>,
    mut clock: ResMut<FluidClock>,
    mut sim: ResMut<FluidSimulation>,
    mut world: ResMut<VoxelWorld>,
) {
    clock.0 += time.delta_secs();
    let step = 1.0 / sim::TICKS_PER_SECOND;
    let mut ticks = 0;
    while clock.0 >= step && ticks < MAX_TICKS_PER_FRAME {
        clock.0 -= step;
        ticks += 1;
        sim.0.advance(&mut *world);
    }
    // Drop the backlog instead of spiralling when the machine cannot keep up.
    clock.0 = clock.0.min(step * 2.0);
}
