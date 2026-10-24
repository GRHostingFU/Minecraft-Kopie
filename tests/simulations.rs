//! Headless end-to-end tests of the fluid and redstone simulations as the game wires them:
//! world edits flow through `BlockChanged` messages into the simulations, which edit the world
//! again. No window, no rendering.

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use minecraft_kopie::physics::fluids::FluidPlugin;
use minecraft_kopie::redstone::{PressesPlates, RedstonePlugin, RedstoneSimulation};
use minecraft_kopie::world::block::{BlockId, FALLING, Facing};
use minecraft_kopie::world::streaming::{ChunkStreamingPlugin, WorldSettings};
use minecraft_kopie::world::{BlockQuery, Chunk, VoxelWorld};
use std::time::Duration;

/// A flat stone world (floor at y = 0) of 3x3 chunks around the origin.
fn app() -> App {
    let dir = std::env::temp_dir().join(format!("mk_sim_{}", std::process::id()));
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(WorldSettings { seed: 1, render_distance: 2, save_dir: dir })
        // 50 ms per frame = one fluid game tick, half a redstone tick.
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(50)))
        .add_plugins((ChunkStreamingPlugin, FluidPlugin, RedstonePlugin));
    {
        let mut world = app.world_mut().resource_mut::<VoxelWorld>();
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut c = Chunk::empty();
                for x in 0..16 {
                    for z in 0..16 {
                        c.set(x, 0, z, BlockId::STONE);
                    }
                }
                world.insert_chunk(IVec2::new(cx, cz), c);
            }
        }
    }
    app
}

fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

fn set(app: &mut App, p: IVec3, b: BlockId) {
    assert!(app.world_mut().resource_mut::<VoxelWorld>().set_block(p, b));
}

fn at(app: &App, p: IVec3) -> BlockId {
    app.world().resource::<VoxelWorld>().block(p).unwrap()
}

fn p(x: i32, y: i32, z: i32) -> IVec3 {
    IVec3::new(x, y, z)
}

/// The player uses the lever / button at `pos` (exactly what a right click does).
fn use_block(app: &mut App, pos: IVec3) {
    // Take the real simulation out of the world so both resources can be borrowed.
    let mut sim = app.world_mut().remove_resource::<RedstoneSimulation>().unwrap();
    {
        let mut world = app.world_mut().resource_mut::<VoxelWorld>();
        assert!(sim.0.use_block(&mut *world, pos));
    }
    app.world_mut().insert_resource(sim);
}

#[test]
fn water_poured_by_the_player_flows_out_and_stops_after_seven_blocks() {
    let mut app = app();
    set(&mut app, p(8, 1, 8), BlockId::WATER);
    run(&mut app, 300);
    for d in 1..=7 {
        assert_eq!(at(&app, p(8 + d, 1, 8)), BlockId::water(d as u8), "level at distance {d}");
    }
    assert!(at(&app, p(16, 1, 8)).is_air());
    // Removing the source drains everything again.
    set(&mut app, p(8, 1, 8), BlockId::AIR);
    run(&mut app, 600);
    assert!(!at(&app, p(9, 1, 8)).is_water(), "flow dried up");
}

#[test]
fn water_and_lava_make_obsidian_and_cobblestone() {
    let mut app = app();
    set(&mut app, p(4, 1, 4), BlockId::LAVA);
    set(&mut app, p(5, 1, 4), BlockId::WATER);
    run(&mut app, 120);
    assert_eq!(at(&app, p(4, 1, 4)), BlockId::OBSIDIAN);

    // A waterfall onto a lava flow.
    set(&mut app, p(12, 1, 12), BlockId::LAVA);
    run(&mut app, 200);
    assert!(at(&app, p(13, 1, 12)).is_lava(), "lava flowed");
    set(&mut app, p(13, 4, 12), BlockId::WATER);
    run(&mut app, 120);
    assert_eq!(at(&app, p(13, 1, 12)), BlockId::OBSIDIAN);
    assert_eq!(at(&app, p(13, 3, 12)), BlockId::water(FALLING));
}

#[test]
fn a_lever_lights_a_lamp_through_dust_and_extends_a_piston() {
    let mut app = app();
    set(&mut app, p(2, 1, 2), BlockId::lever(Facing::Down, false));
    for x in 3..=6 {
        set(&mut app, p(x, 1, 2), BlockId::dust(0));
    }
    set(&mut app, p(7, 1, 2), BlockId::lamp(false));
    // A piston next to the lever pushing a block north.
    set(&mut app, p(2, 1, 3), BlockId::piston(false, Facing::South, false));
    set(&mut app, p(2, 1, 4), BlockId::COBBLE);
    run(&mut app, 10);
    assert_eq!(at(&app, p(7, 1, 2)), BlockId::lamp(false));

    use_block(&mut app, p(2, 1, 2));
    run(&mut app, 12);
    assert_eq!(at(&app, p(3, 1, 2)), BlockId::dust(15));
    assert_eq!(at(&app, p(6, 1, 2)), BlockId::dust(12));
    assert_eq!(at(&app, p(7, 1, 2)), BlockId::lamp(true));
    assert_eq!(at(&app, p(2, 1, 3)), BlockId::piston(false, Facing::South, true));
    assert_eq!(at(&app, p(2, 1, 4)), BlockId::piston_head(false, Facing::South));
    assert_eq!(at(&app, p(2, 1, 5)), BlockId::COBBLE, "pushed one block");

    use_block(&mut app, p(2, 1, 2));
    run(&mut app, 16);
    assert_eq!(at(&app, p(7, 1, 2)), BlockId::lamp(false));
    assert_eq!(at(&app, p(2, 1, 3)), BlockId::piston(false, Facing::South, false));
    assert!(at(&app, p(2, 1, 4)).is_air());
}

#[test]
fn bodies_press_plates_and_release_them() {
    let mut app = app();
    set(&mut app, p(10, 1, 10), BlockId::plate(false));
    set(&mut app, p(11, 1, 10), BlockId::lamp(false));
    run(&mut app, 4);
    let visitor = app.world_mut().spawn((PressesPlates, Transform::from_xyz(10.5, 1.0, 10.5))).id();
    run(&mut app, 8);
    assert_eq!(at(&app, p(10, 1, 10)), BlockId::plate(true));
    assert_eq!(at(&app, p(11, 1, 10)), BlockId::lamp(true));
    app.world_mut().despawn(visitor);
    run(&mut app, 30);
    assert_eq!(at(&app, p(10, 1, 10)), BlockId::plate(false));
    assert_eq!(at(&app, p(11, 1, 10)), BlockId::lamp(false));
}

#[test]
fn redstone_parts_without_support_fall_off_when_the_block_below_is_removed() {
    let mut app = app();
    set(&mut app, p(6, 1, 6), BlockId::STONE);
    set(&mut app, p(6, 2, 6), BlockId::dust(0));
    set(&mut app, p(7, 2, 6), BlockId::torch(Facing::West, true));
    run(&mut app, 6);
    set(&mut app, p(6, 1, 6), BlockId::AIR);
    run(&mut app, 8);
    assert!(at(&app, p(6, 2, 6)).is_air(), "dust lost its floor");
    assert!(at(&app, p(7, 2, 6)).is_air(), "torch lost its wall");
}

#[test]
fn saved_flowing_water_resumes_when_a_chunk_is_loaded() {
    let mut app = app();
    // Insert a chunk that contains a half-finished flow, as if it was saved mid-flow.
    let mut c = Chunk::empty();
    for x in 0..16 {
        for z in 0..16 {
            c.set(x, 0, z, BlockId::STONE);
        }
    }
    c.set(3, 1, 3, BlockId::WATER);
    c.set(4, 1, 3, BlockId::water(1));
    app.world_mut().resource_mut::<VoxelWorld>().insert_chunk(IVec2::new(5, 5), c);
    run(&mut app, 300);
    assert_eq!(at(&app, p(5 * 16 + 5, 1, 5 * 16 + 3)), BlockId::water(2), "the flow was completed");
}
