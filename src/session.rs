//! The two top-level states of the game and everything that happens when switching between them:
//! the main menu (with a drifting panorama of generated terrain behind it) and a running world.
//! Also the world list on disk: creating, listing and deleting saves.

use crate::inventory::Inventory;
use crate::player::LoadedLevel;
use crate::player::controller::find_spawn_column;
use crate::rendering::sky::{SkyCenter, TimeOfDay};
use crate::settings::SAVES_DIR;
use crate::survival::{Experience, Vitals};
use crate::ui::screens::Screen;
use crate::world::generation::TerrainGenerator;
use crate::world::storage::{self, LevelData};
use crate::world::streaming::{ChunkAnchor, Generator, WorldReset, WorldSettings};
use crate::world::VoxelWorld;
use bevy::camera::Projection;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(States, Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AppState {
    #[default]
    MainMenu,
    InGame,
}

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum GameMode {
    Creative,
    #[default]
    Survival,
}

impl GameMode {
    pub fn label(self) -> &'static str {
        match self {
            GameMode::Creative => "Creative",
            GameMode::Survival => "Survival",
        }
    }

    pub fn flip(self) -> Self {
        match self {
            GameMode::Creative => GameMode::Survival,
            GameMode::Survival => GameMode::Creative,
        }
    }
}

/// Everything that exists only while a world is running; despawned when the session ends.
#[derive(Component)]
pub struct GameEntity;

/// The world the menu asked to enter.
#[derive(Resource, Clone, Debug)]
pub struct PendingGame {
    pub dir: PathBuf,
    pub seed: u32,
    pub mode: GameMode,
    pub name: String,
}

/// The slowly turning camera behind the main menu.
#[derive(Component)]
pub struct MenuCamera;

// -------------------------------------------------------------------------------------------------
// Worlds on disk
// -------------------------------------------------------------------------------------------------

/// A seed from the text of the seed field: empty = random, a number is used as is, anything else
/// is hashed (so "hello" always gives the same world).
pub fn seed_from_text(text: &str) -> u32 {
    let t = text.trim();
    if t.is_empty() {
        return std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32);
    }
    if let Ok(n) = t.parse::<i64>() {
        return n as u32;
    }
    // FNV-1a.
    t.bytes().fold(0x811C_9DC5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Loads the world in `pending.dir`, or creates it (writing `level.json` immediately so chunks
/// can never be paired with a different seed). An existing world keeps its own seed.
pub fn prepare_level(pending: &PendingGame) -> LevelData {
    if let Some(level) = storage::load_level(&pending.dir) {
        return level;
    }
    let fresh = LevelData { last_played: now_secs(), ..LevelData::new(pending.seed, &pending.name, pending.mode) };
    if let Err(e) = storage::save_level(&pending.dir, &fresh) {
        error!("cannot create {}: {e}", pending.dir.display());
    }
    fresh
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorldEntry {
    pub dir: PathBuf,
    pub name: String,
    pub seed: u32,
    pub mode: GameMode,
    pub last_played: u64,
}

/// Every world below `root`, most recently played first.
pub fn list_worlds(root: &Path) -> Vec<WorldEntry> {
    let Ok(read) = std::fs::read_dir(root) else { return Vec::new() };
    let mut worlds: Vec<WorldEntry> = read
        .flatten()
        .filter(|e| e.path().is_dir() && !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let level = storage::load_level(&e.path())?;
            Some(WorldEntry {
                name: if level.name.trim().is_empty() { e.file_name().to_string_lossy().into_owned() } else { level.name.clone() },
                dir: e.path(),
                seed: level.seed,
                mode: level.game_mode,
                last_played: level.last_played,
            })
        })
        .collect();
    worlds.sort_by(|a, b| b.last_played.cmp(&a.last_played).then_with(|| a.name.cmp(&b.name)));
    worlds
}

/// A folder name for a new world: the sanitised name, with a number appended if it is taken.
pub fn unique_world_dir(root: &Path, name: &str) -> PathBuf {
    let base: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let base = if base.is_empty() { "World".to_string() } else { base };
    let mut candidate = root.join(&base);
    let mut n = 2;
    while candidate.exists() {
        candidate = root.join(format!("{base}_{n}"));
        n += 1;
    }
    candidate
}

/// Deletes a world folder. Refuses anything that is not a world directly below `root`.
pub fn delete_world(root: &Path, dir: &Path) -> std::io::Result<()> {
    let ok = dir.parent() == Some(root) && dir.join("level.json").is_file();
    if !ok {
        return Err(std::io::Error::other("not a world folder"));
    }
    std::fs::remove_dir_all(dir)
}

// -------------------------------------------------------------------------------------------------
// State transitions
// -------------------------------------------------------------------------------------------------

pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .add_systems(OnEnter(AppState::MainMenu), setup_panorama)
            .add_systems(OnExit(AppState::MainMenu), teardown_panorama)
            .add_systems(
                OnEnter(AppState::InGame),
                (begin_session, crate::player::controller::spawn_player, crate::player::viewmodel::spawn_view_model).chain(),
            )
            .add_systems(OnExit(AppState::InGame), end_session)
            .add_systems(Update, drift_panorama.run_if(in_state(AppState::MainMenu)));
    }
}

fn begin_session(
    mut commands: Commands,
    pending: Res<PendingGame>,
    mut settings: ResMut<WorldSettings>,
    mut resets: MessageWriter<WorldReset>,
    mut screen: ResMut<Screen>,
) {
    let level = prepare_level(&pending);
    settings.seed = level.seed;
    settings.save_dir = pending.dir.clone();
    commands.insert_resource(Generator(Arc::new(TerrainGenerator::new(level.seed))));

    let mut inventory = match &level.inventory {
        Some(saved) => Inventory::from_save(saved, level.selected_slot),
        None if level.game_mode == GameMode::Creative => Inventory::starter_kit(),
        None => Inventory::empty(),
    };
    inventory.load_furnaces(&level.furnaces);
    commands.insert_resource(inventory);
    commands.insert_resource(level.game_mode);
    commands.insert_resource(Vitals::from_save(level.health, level.food, level.saturation));
    commands.insert_resource(Experience { total: level.xp_total });
    let t = if level.time_of_day.is_finite() { level.time_of_day.rem_euclid(1.0) } else { 0.32 };
    commands.insert_resource(TimeOfDay { t, ..default() });
    commands.insert_resource(LoadedLevel(Some(level)));
    resets.write(WorldReset);
    *screen = Screen::Playing;
    info!("entering world {} (seed {})", pending.dir.display(), pending.seed);
}

fn end_session(mut commands: Commands, entities: Query<Entity, With<GameEntity>>, mut screen: ResMut<Screen>) {
    for e in &entities {
        commands.entity(e).despawn();
    }
    commands.insert_resource(LoadedLevel(None));
    *screen = Screen::Title;
}

/// Terrain behind the title screen: a throw-away world that is never saved.
fn setup_panorama(
    mut commands: Commands,
    mut settings: ResMut<WorldSettings>,
    mut resets: MessageWriter<WorldReset>,
    mut tod: ResMut<TimeOfDay>,
) {
    let seed = seed_from_text("");
    settings.seed = seed;
    settings.save_dir = PathBuf::from(SAVES_DIR).join(".panorama");
    let generator = Arc::new(TerrainGenerator::new(seed));
    let spawn = find_spawn_column(&generator);
    commands.insert_resource(Generator(generator));
    resets.write(WorldReset);
    *tod = TimeOfDay { t: 0.40, speed: 1.0 };
    commands.spawn((
        MenuCamera,
        ChunkAnchor,
        SkyCenter,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: 75f32.to_radians(), near: 0.1, far: 2500.0, ..default() }),
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        Transform::from_xyz(spawn.x as f32, 110.0, spawn.y as f32)
            .with_rotation(Quat::from_euler(EulerRot::YXZ, 0.0, -0.18, 0.0)),
    ));
}

fn teardown_panorama(mut commands: Commands, cameras: Query<Entity, With<MenuCamera>>) {
    for e in &cameras {
        commands.entity(e).despawn();
    }
}

/// Slow turn plus a gentle drift; the camera hovers above whatever terrain is below it.
fn drift_panorama(time: Res<Time>, world: Res<VoxelWorld>, mut camera: Query<&mut Transform, With<MenuCamera>>) {
    let Ok(mut t) = camera.single_mut() else { return };
    let dt = time.delta_secs();
    let yaw = t.rotation.to_euler(EulerRot::YXZ).0 + dt * 0.045;
    t.rotation = Quat::from_euler(EulerRot::YXZ, yaw, -0.2, 0.0);
    t.translation.x += dt * 1.6;
    t.translation.z += dt * 0.7;
    let ground = world.surface_height(t.translation.x.floor() as i32, t.translation.z.floor() as i32);
    let want = ground.map_or(t.translation.y, |g| (g as f32 + 26.0).max(88.0));
    t.translation.y += (want - t.translation.y) * (1.0 - (-dt * 0.8).exp());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mk_session_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn seeds_from_text() {
        assert_eq!(seed_from_text("12345"), 12345);
        assert_eq!(seed_from_text("  42 "), 42);
        assert_eq!(seed_from_text("hello"), seed_from_text("hello"));
        assert_ne!(seed_from_text("hello"), seed_from_text("hellp"));
        assert_eq!(seed_from_text("-1"), u32::MAX);
        // An empty field gives a fresh random seed each time it is asked (clock based).
        let a = seed_from_text("");
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert_ne!(a, seed_from_text(""));
    }

    #[test]
    fn a_new_world_writes_its_seed_immediately_and_an_old_one_keeps_it() {
        let root = tmp("prepare");
        let pending = |seed| PendingGame { dir: root.join("w"), seed, mode: GameMode::Creative, name: "My World".into() };
        let first = prepare_level(&pending(1234));
        assert_eq!((first.seed, first.spawned, first.game_mode), (1234, false, GameMode::Creative));
        assert_eq!(storage::load_level(&root.join("w")).unwrap().seed, 1234, "written to disk right away");
        // A later start with another requested seed does not change the existing world.
        let again = prepare_level(&pending(999));
        assert_eq!((again.seed, again.name.as_str()), (1234, "My World"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn world_list_is_sorted_and_skips_junk() {
        let root = tmp("list");
        for (dir, name, played) in [("a", "Alpha", 10u64), ("b", "Beta", 30), ("c", "Gamma", 20)] {
            let level = LevelData { last_played: played, ..LevelData::new(1, name, GameMode::Survival) };
            storage::save_level(&root.join(dir), &level).unwrap();
        }
        std::fs::create_dir_all(root.join("empty_dir")).unwrap();
        std::fs::create_dir_all(root.join(".panorama")).unwrap();
        std::fs::write(root.join("options.json"), "{}").unwrap();
        let names: Vec<String> = list_worlds(&root).into_iter().map(|w| w.name).collect();
        assert_eq!(names, ["Beta", "Gamma", "Alpha"]);
        assert!(list_worlds(&root.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unique_dirs_never_collide_and_deleting_is_guarded() {
        let root = tmp("dirs");
        std::fs::create_dir_all(&root).unwrap();
        let a = unique_world_dir(&root, "New World!");
        assert_eq!(a.file_name().unwrap(), "New_World_");
        std::fs::create_dir_all(&a).unwrap();
        let b = unique_world_dir(&root, "New World!");
        assert_eq!(b.file_name().unwrap(), "New_World__2");
        assert_eq!(unique_world_dir(&root, "   ").file_name().unwrap(), "World");

        storage::save_level(&a, &LevelData::fresh(3)).unwrap();
        // Not a world (no level.json) or not below the root: refused.
        std::fs::create_dir_all(root.join("plain")).unwrap();
        assert!(delete_world(&root, &root.join("plain")).is_err());
        assert!(delete_world(&root, &root).is_err());
        assert!(delete_world(&root, Path::new("/tmp")).is_err());
        delete_world(&root, &a).unwrap();
        assert!(!a.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
