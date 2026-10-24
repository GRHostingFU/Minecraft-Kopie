//! Headless end-to-end tests of saving: chunks, player, inventory, time, seed and options must
//! all make it to disk and back unchanged.

use bevy::prelude::*;
use minecraft_kopie::inventory::{ARMOR_START, Inventory, Item, ItemStack};
use minecraft_kopie::player::PlayerPlugin;
use minecraft_kopie::player::controller::Player;
use minecraft_kopie::rendering::sky::TimeOfDay;
use minecraft_kopie::save::{SavePlugin, SaveRequest, SaveStatus};
use minecraft_kopie::settings::GameSettings;
use minecraft_kopie::world::storage;
use minecraft_kopie::world::streaming::WorldSettings;
use minecraft_kopie::world::{BlockId, BlockQuery, Chunk, VoxelWorld};
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mk_save_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn player(active: bool) -> Player {
    Player {
        vel: Vec3::ZERO,
        yaw: 1.25,
        pitch: -0.4,
        on_ground: true,
        in_water: false,
        in_lava: false,
        flying: true,
        sprinting: false,
        sneaking: false,
        eye_height: 1.62,
        fov_scale: 1.0,
        active,
        needs_spawn_search: false,
    }
}

/// An app with everything the save plugin reads, but no windows or rendering.
fn save_app(dir: &std::path::Path, active: bool) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(WorldSettings { seed: 77, render_distance: 9, save_dir: dir.to_path_buf() })
        .insert_resource(GameSettings { fov: 95.0, mouse_sensitivity: 0.5, fullscreen: true })
        .insert_resource(TimeOfDay { t: 0.77, speed: 1.0 })
        .init_resource::<VoxelWorld>()
        .add_plugins(SavePlugin);
    let mut inventory = Inventory::starter_kit();
    inventory.selected = 3;
    inventory.slots[ARMOR_START] = Some(ItemStack::new(Item::IRON_HELMET, 1));
    inventory.slots[9] = None;
    inventory.cursor = Some(ItemStack::new(Item::block(BlockId::LOG), 7));
    app.insert_resource(inventory);
    app.world_mut().spawn((player(active), Transform::from_xyz(12.5, 70.0, -3.25)));
    app
}

#[test]
fn everything_survives_a_save_and_reload() {
    let dir = tmp("roundtrip");
    let mut app = save_app(&dir, true);
    let edit = IVec3::new(5, 90, 7);
    {
        let mut world = app.world_mut().resource_mut::<VoxelWorld>();
        world.insert_chunk(IVec2::ZERO, Chunk::empty());
        assert!(world.set_block(edit, BlockId::PLANKS));
    }
    app.world_mut().write_message(SaveRequest { then_quit: false });
    app.update();

    let status = app.world().resource::<SaveStatus>();
    assert!(status.ok, "{}", status.message);
    assert!(app.should_exit().is_none());

    // Chunk file.
    let chunk = storage::load_chunk(&dir, IVec2::ZERO).unwrap().expect("chunk file written");
    assert_eq!(chunk.get(5, 90, 7), BlockId::PLANKS);
    assert_eq!(app.world().resource::<VoxelWorld>().chunk(IVec2::ZERO).map(|c| c.modified), Some(false));

    // level.json.
    let level = storage::load_level(&dir).expect("level.json written");
    assert_eq!(level.seed, 77);
    assert!(level.spawned);
    assert_eq!(level.player_pos, [12.5, 70.0, -3.25]);
    assert_eq!((level.yaw, level.pitch, level.flying), (1.25, -0.4, true));
    assert_eq!(level.time_of_day, 0.77);
    assert_eq!(level.selected_slot, 3);
    assert_eq!(level.options.render_distance, Some(9));
    assert_eq!(level.options.fov, Some(95.0));
    assert_eq!(level.options.mouse_sensitivity, Some(0.5));
    assert_eq!(level.options.fullscreen, Some(true));

    // The inventory, including armor and the stack on the cursor, comes back whole.
    let original = app.world().resource::<Inventory>().clone();
    let restored = Inventory::from_save(level.inventory.as_ref().unwrap(), level.selected_slot);
    assert_eq!(restored.selected, 3);
    assert_eq!(restored.slots[ARMOR_START], original.slots[ARMOR_START]);
    // The cursor stack lands in the first free slot (6); every other slot is untouched.
    for i in (0..ARMOR_START).filter(|i| *i != 6) {
        assert_eq!(restored.slots[i], original.slots[i], "slot {i}");
    }
    assert_eq!(restored.slots[6], Some(ItemStack::new(Item::block(BlockId::LOG), 7)));
    // The cursor stack was merged back into the inventory instead of being lost.
    assert_eq!(restored.cursor, None);
    assert_eq!(restored.count_of(Item::block(BlockId::LOG)), original.count_of(Item::block(BlockId::LOG)) + 7);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saved_edits_are_read_back_by_a_fresh_world() {
    let dir = tmp("fresh");
    let mut app = save_app(&dir, true);
    {
        let mut world = app.world_mut().resource_mut::<VoxelWorld>();
        world.insert_chunk(IVec2::new(-3, 4), Chunk::empty());
        world.set_block(IVec3::new(-40, 12, 70), BlockId::COBBLE);
    }
    app.world_mut().write_message(SaveRequest { then_quit: false });
    app.update();
    drop(app);

    let chunk = storage::load_chunk(&dir, IVec2::new(-3, 4)).unwrap().unwrap();
    let mut world = VoxelWorld::default();
    world.insert_chunk(IVec2::new(-3, 4), chunk);
    assert_eq!(world.block(IVec3::new(-40, 12, 70)), Some(BlockId::COBBLE));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn quit_requests_save_first_and_then_exit() {
    let dir = tmp("quit");
    let mut app = save_app(&dir, true);
    app.world_mut().write_message(SaveRequest { then_quit: true });
    app.update();
    assert!(storage::load_level(&dir).is_some(), "level must be on disk before the app exits");
    assert!(app.should_exit().is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_player_that_is_not_in_the_world_yet_does_not_overwrite_the_level() {
    let dir = tmp("inactive");
    storage::save_level(&dir, &storage::LevelData::fresh(77)).unwrap();
    let mut app = save_app(&dir, false);
    app.world_mut().write_message(SaveRequest { then_quit: false });
    app.update();
    let level = storage::load_level(&dir).unwrap();
    assert!(!level.spawned, "the placeholder position of an unplaced player must not be saved");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_save_does_not_trap_the_player() {
    // The "directory" lives below a regular file, so nothing can ever be written there.
    let base = tmp("blocked");
    std::fs::create_dir_all(&base).unwrap();
    let file = base.join("not_a_dir");
    std::fs::write(&file, b"x").unwrap();
    let mut app = save_app(&file.join("world"), true);
    app.world_mut().write_message(SaveRequest { then_quit: true });
    app.update();
    assert!(!app.world().resource::<SaveStatus>().ok);
    assert!(app.should_exit().is_none(), "first quit attempt is refused so the data is not lost silently");
    app.world_mut().write_message(SaveRequest { then_quit: true });
    app.update();
    assert!(app.should_exit().is_some(), "asking again must quit");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_new_world_writes_its_seed_immediately_and_an_old_one_keeps_it() {
    let dir = tmp("seed");
    let build = |seed: u32| {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(WorldSettings { seed, render_distance: 6, save_dir: dir.clone() })
            .init_resource::<TimeOfDay>()
            .add_plugins(PlayerPlugin);
        app
    };
    let first = build(1234);
    let level = storage::load_level(&dir).expect("level.json exists right after startup");
    assert_eq!((level.seed, level.spawned), (1234, false));
    drop(first);
    // A later start with another requested seed does not overwrite the existing world's file.
    let _second = build(999);
    assert_eq!(storage::load_level(&dir).unwrap().seed, 1234);
    let _ = std::fs::remove_dir_all(dir);
}
