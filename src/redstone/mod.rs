//! Redstone: dust, torches, levers, buttons, pressure plates, lamps and pistons.
//!
//! * [`rules`] — how parts connect and what they need to stand on;
//! * [`power`] — who powers whom (weak/strong signals);
//! * [`piston`] — pushing and pulling blocks;
//! * [`sim`] — the tick-based simulation tying it together (pure, testable);
//! * this module — the Bevy plugin that feeds world edits and bodies on plates into the sim.

pub mod piston;
pub mod power;
pub mod rules;
pub mod sim;

pub use sim::RedstoneSim;

use crate::world::chunk::{CHUNK_W, ChunkPos};
use crate::world::{BlockChanged, ChunkLoaded, VoxelWorld};
use bevy::prelude::*;
use std::collections::HashSet;

/// The running redstone simulation.
#[derive(Resource, Default)]
pub struct RedstoneSimulation(pub RedstoneSim);

/// Entities with this component press pressure plates they stand on (the player and mobs).
#[derive(Component)]
pub struct PressesPlates;

#[derive(Resource, Default)]
struct RedstoneClock(f32);

const MAX_TICKS_PER_FRAME: u32 = 4;

pub struct RedstonePlugin;

impl Plugin for RedstonePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RedstoneSimulation>()
            .init_resource::<RedstoneClock>()
            .add_systems(Update, (notify_from_edits, notify_from_chunks, run_redstone).chain());
    }
}

fn notify_from_edits(mut changes: MessageReader<BlockChanged>, mut sim: ResMut<RedstoneSimulation>) {
    for BlockChanged(c) in changes.read() {
        sim.0.notify(c.pos);
    }
}

/// Circuits saved in a chunk wake up again when it loads.
fn notify_from_chunks(
    mut loaded: MessageReader<ChunkLoaded>,
    world: Res<VoxelWorld>,
    mut sim: ResMut<RedstoneSimulation>,
) {
    for ChunkLoaded(pos) in loaded.read() {
        for p in circuit_parts(&world, *pos) {
            sim.0.register_loaded(&*world, p);
        }
    }
}

/// World positions of the redstone parts in a chunk.
pub fn circuit_parts(world: &VoxelWorld, pos: ChunkPos) -> Vec<IVec3> {
    let Some(chunk) = world.chunk(pos) else { return Vec::new() };
    let mut out = Vec::new();
    for (i, b) in chunk.raw().iter().enumerate() {
        // Cheap pre-filter: every circuit part has an id outside the plain-block range.
        if b.0 > 12 && sim::is_part(*b) {
            let (x, z, y) = (i % CHUNK_W, (i / CHUNK_W) % CHUNK_W, i / (CHUNK_W * CHUNK_W));
            out.push(IVec3::new(pos.x * CHUNK_W as i32 + x as i32, y as i32, pos.y * CHUNK_W as i32 + z as i32));
        }
    }
    out
}

fn run_redstone(
    time: Res<Time>,
    mut clock: ResMut<RedstoneClock>,
    mut sim: ResMut<RedstoneSimulation>,
    mut world: ResMut<VoxelWorld>,
    bodies: Query<(&Transform, &PressesPlates)>,
) {
    clock.0 += time.delta_secs();
    let step = 1.0 / sim::TICKS_PER_SECOND;
    if clock.0 < step {
        return;
    }
    // The cells that currently hold a body (feet cell), computed once per frame.
    let occupied: HashSet<IVec3> = bodies
        .iter()
        .map(|(t, _)| {
            IVec3::new(
                t.translation.x.floor() as i32,
                (t.translation.y + 0.01).floor() as i32,
                t.translation.z.floor() as i32,
            )
        })
        .collect();
    let mut ticks = 0;
    while clock.0 >= step && ticks < MAX_TICKS_PER_FRAME {
        clock.0 -= step;
        ticks += 1;
        sim.0.advance(&mut *world, &occupied);
    }
    clock.0 = clock.0.min(step * 2.0);
}
