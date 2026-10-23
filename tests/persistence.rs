//! Headless end-to-end tests of saving: chunks, player, inventory, vitals, time, seed and game
//! mode must all make it to disk and back unchanged.

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use minecraft_kopie::inventory::{ARMOR_START, FURNACE_FUEL, FURNACE_IN, Inventory, OFFHAND};
use minecraft_kopie::items::{Item, ItemStack, Tier, ToolKind};
use minecraft_kopie::player::LoadedLevel;
use minecraft_kopie::player::controller::{Player, SpawnPoint};
use minecraft_kopie::rendering::sky::TimeOfDay;
use minecraft_kopie::save::{SavePlugin, SaveRequest, SaveStatus};
use minecraft_kopie::session::{AppState, GameMode};
use minecraft_kopie::survival::{Experience, Vitals};
use minecraft_kopie::world::storage::{self, LevelData};
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
fn save_app(dir: &PathBuf, active: bool) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .init_state::<AppState>()
        .insert_resource(WorldSettings { seed: 77, render_distance: 9, save_dir: dir.clone() })
        .insert_resource(GameMode::Survival)
        .insert_resource(TimeOfDay { t: 0.77, speed: 1.0 })
        .insert_resource(Vitals::from_save(13.0, 11.0, 2.5))
        .insert_resource(Experience { total: 123 })
        .insert_resource(SpawnPoint(Some(Vec3::new(4.0, 66.0, -9.0))))
        .insert_resource(LoadedLevel(Some(LevelData::new(77, "Test World", GameMode::Survival))))
        .init_resource::<VoxelWorld>()
        .add_plugins(SavePlugin);
    let mut inventory = Inventory::starter_kit();
    inventory.selected = 3;
    inventory.slots[ARMOR_START] = Some(ItemStack::new(Item::IRON_HELMET, 1));
    inventory.slots[OFFHAND] = Some(ItemStack::new(Item::block(BlockId::TORCH), 9));
    inventory.slots[9] = None;
    inventory.slots[10] = Some(ItemStack { damage: 17, ..ItemStack::new(Item::tool(ToolKind::Axe, Tier::Iron), 1) });
    inventory.cursor = Some(ItemStack::new(Item::block(BlockId::LOG), 7));
    // A furnace in the middle of smelting.
    inventory.open_furnace(IVec3::new(2, 64, 2));
    inventory.slots[FURNACE_IN] = Some(ItemStack::new(Item::block(BlockId::IRON_ORE), 5));
    inventory.slots[FURNACE_FUEL] = Some(ItemStack::new(Item::COAL, 3));
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
    app.world_mut().write_message(SaveRequest::default());
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
    assert_eq!((level.seed, level.name.as_str(), level.game_mode), (77, "Test World", GameMode::Survival));
    assert!(level.spawned);
    assert!(level.last_played > 0);
    assert_eq!(level.player_pos, [12.5, 70.0, -3.25]);
    assert_eq!((level.yaw, level.pitch, level.flying), (1.25, -0.4, true));
    assert_eq!(level.time_of_day, 0.77);
    assert_eq!(level.selected_slot, 3);
    assert_eq!((level.health, level.food, level.saturation, level.xp_total), (13.0, 11.0, 2.5, 123));
    assert_eq!(level.spawn_point, Some([4.0, 66.0, -9.0]));

    // The inventory, including armor, off hand, tool wear and the stack on the cursor, comes back whole.
    let original = app.world().resource::<Inventory>().clone();
    let restored = Inventory::from_save(level.inventory.as_ref().unwrap(), level.selected_slot);
    assert_eq!(restored.selected, 3);
    assert_eq!(restored.slots[ARMOR_START], original.slots[ARMOR_START]);
    assert_eq!(restored.slots[OFFHAND], original.slots[OFFHAND]);
    assert_eq!(restored.slots[10].map(|s| s.damage), Some(17));
    // The cursor stack lands in the first free slot (9); every other slot is untouched.
    for i in (0..ARMOR_START + 5).filter(|i| *i != 9) {
        assert_eq!(restored.slots[i], original.slots[i], "slot {i}");
    }
    assert_eq!(restored.slots[9], Some(ItemStack::new(Item::block(BlockId::LOG), 7)));
    assert_eq!(restored.cursor, None, "the cursor stack was merged back instead of being lost");

    // Furnaces, including the stacks of the one that was open, are stored.
    assert_eq!(level.furnaces.len(), 1);
    let mut reloaded = Inventory::empty();
    reloaded.load_furnaces(&level.furnaces);
    let furnace = reloaded.furnace_state(IVec3::new(2, 64, 2)).unwrap();
    assert_eq!(furnace.input, Some(ItemStack::new(Item::block(BlockId::IRON_ORE), 5)));
    assert_eq!(furnace.fuel, Some(ItemStack::new(Item::COAL, 3)));
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
    app.world_mut().write_message(SaveRequest::default());
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
    app.world_mut().write_message(SaveRequest { then_quit: true, ..default() });
    app.update();
    assert!(storage::load_level(&dir).is_some(), "level must be on disk before the app exits");
    assert!(app.should_exit().is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn quitting_to_the_title_saves_and_switches_the_state() {
    let dir = tmp("title");
    let mut app = save_app(&dir, true);
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::InGame);
    app.update();
    assert_eq!(*app.world().resource::<State<AppState>>().get(), AppState::InGame);
    app.world_mut().write_message(SaveRequest { then_title: true, ..default() });
    app.update();
    app.update();
    assert!(storage::load_level(&dir).is_some(), "saved before leaving");
    assert_eq!(*app.world().resource::<State<AppState>>().get(), AppState::MainMenu);
    assert!(app.should_exit().is_none(), "returning to the title screen does not quit");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_player_that_is_not_in_the_world_yet_does_not_overwrite_the_level() {
    let dir = tmp("inactive");
    storage::save_level(&dir, &LevelData::fresh(77)).unwrap();
    let mut app = save_app(&dir, false);
    app.world_mut().write_message(SaveRequest::default());
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
    app.world_mut().write_message(SaveRequest { then_quit: true, ..default() });
    app.update();
    assert!(!app.world().resource::<SaveStatus>().ok);
    assert!(app.should_exit().is_none(), "first quit attempt is refused so the data is not lost silently");
    app.world_mut().write_message(SaveRequest { then_quit: true, ..default() });
    app.update();
    assert!(app.should_exit().is_some(), "asking again must quit");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn old_world_files_open_as_creative_worlds() {
    let dir = tmp("legacy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("level.json"),
        r#"{"seed":5,"player_pos":[1.0,70.0,2.0],"yaw":0.0,"pitch":0.0,"time_of_day":0.5,"selected_slot":2,"flying":true,
            "options":{"render_distance":12,"fov":90.0}}"#,
    )
    .unwrap();
    let level = storage::load_level(&dir).unwrap();
    assert_eq!(level.game_mode, GameMode::Creative, "worlds from before the game modes were sandboxes");
    assert_eq!((level.health, level.food), (20.0, 20.0));
    assert!(level.furnaces.is_empty() && level.spawn_point.is_none());
    let _ = std::fs::remove_dir_all(dir);
}
