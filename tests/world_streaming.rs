//! Headless end-to-end test of async chunk streaming and persistence.

use bevy::prelude::*;
use minecraft_kopie::world::block::BlockId;
use minecraft_kopie::world::streaming::{ChunkAnchor, ChunkStreamingPlugin, WorldSettings, save_all_modified};
use minecraft_kopie::world::{BlockQuery, VoxelWorld};
use std::path::PathBuf;

fn app(settings: WorldSettings, anchor: Vec3) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).insert_resource(settings).add_plugins(ChunkStreamingPlugin);
    app.world_mut().spawn((ChunkAnchor, Transform::from_translation(anchor)));
    app
}

fn pump_until(app: &mut App, what: &str, done: impl Fn(&World) -> bool) {
    for _ in 0..2000 {
        app.update();
        if done(app.world()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("timed out waiting for {what}");
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mk_it_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn chunks_stream_in_around_the_anchor_and_far_ones_unload() {
    let settings = WorldSettings { seed: 3, render_distance: 2, save_dir: tmp("stream") };
    let mut app = app(settings.clone(), Vec3::new(8.0, 80.0, 8.0));
    pump_until(&mut app, "initial chunks", |w| {
        let world = w.resource::<VoxelWorld>();
        (-3..=3).all(|x| {
            (-3..=3).all(|z| {
                let d2 = x * x + z * z;
                d2 > 9 || world.is_loaded(IVec2::new(x, z))
            })
        })
    });
    let loaded_near_origin = app.world().resource::<VoxelWorld>().loaded_count();
    assert!(loaded_near_origin >= 25);

    // Teleport far away: the old area must be dropped and the new area appears.
    let mut q = app.world_mut().query_filtered::<&mut Transform, With<ChunkAnchor>>();
    q.single_mut(app.world_mut()).unwrap().translation = Vec3::new(16.0 * 100.0 + 8.0, 80.0, 8.0);
    pump_until(&mut app, "streaming at new location", |w| {
        let world = w.resource::<VoxelWorld>();
        world.is_loaded(IVec2::new(100, 0)) && !world.is_loaded(IVec2::ZERO)
    });
    let _ = std::fs::remove_dir_all(settings.save_dir);
}

#[test]
fn edits_survive_unload_and_restart() {
    let dir = tmp("persist");
    let settings = WorldSettings { seed: 8, render_distance: 1, save_dir: dir.clone() };
    let edit = IVec3::new(5, 90, 7);

    let mut first = app(settings.clone(), Vec3::new(8.0, 80.0, 8.0));
    pump_until(&mut first, "chunk 0,0", |w| w.resource::<VoxelWorld>().is_loaded(IVec2::ZERO));
    assert!(first.world_mut().resource_mut::<VoxelWorld>().set_block(edit, BlockId::PLANKS));
    let saved = save_all_modified(&mut first.world_mut().resource_mut::<VoxelWorld>(), &dir);
    assert_eq!(saved, 1);
    assert_eq!(first.world().resource::<VoxelWorld>().chunk(IVec2::ZERO).map(|c| c.modified), Some(false));
    drop(first);

    // A fresh app reads the edit back from disk instead of regenerating.
    let mut second = app(settings, Vec3::new(8.0, 80.0, 8.0));
    pump_until(&mut second, "reload", |w| w.resource::<VoxelWorld>().is_loaded(IVec2::ZERO));
    assert_eq!(second.world().resource::<VoxelWorld>().block(edit), Some(BlockId::PLANKS));
    let _ = std::fs::remove_dir_all(dir);
}
