//! Saving orchestration. Everything that must survive a restart goes through here:
//! modified chunks (`c.<x>.<z>.bin`) and `level.json` with the player position, inventory,
//! health and hunger, experience, furnaces, time of day, seed and game mode. (Options are global
//! and live in `options.json`, see `settings`.)
//!
//! A save happens on a [`SaveRequest`] (pause menu, Esc), every 30 seconds, when the window is
//! closed and, as a last resort, on any `AppExit`.

use crate::inventory::Inventory;
use crate::player::controller::{Player, SpawnPoint};
use crate::rendering::sky::TimeOfDay;
use crate::session::{AppState, GameMode};
use crate::survival::{Experience, Vitals};
use crate::world::VoxelWorld;
use crate::world::storage::{self, LEVEL_VERSION, LevelData, OptionsData};
use crate::world::streaming::{WorldSettings, save_modified_chunks};
use bevy::app::OnAppExitSystems;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::{ExitSystems, WindowCloseRequested};
use std::path::Path;
use std::time::Duration;

/// Ask for an immediate, synchronous save of the whole world.
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct SaveRequest {
    /// Exit the game once the data is on disk.
    pub then_quit: bool,
    /// Return to the title screen once the data is on disk.
    pub then_title: bool,
}

/// Result of the most recent save, shown in the pause menu.
#[derive(Resource, Default, Debug, Clone)]
pub struct SaveStatus {
    pub message: String,
    pub ok: bool,
    /// A quit was already refused because saving failed; asking again quits anyway.
    quit_refused: bool,
}

#[derive(Resource)]
struct LevelAutosave(Timer);

/// Collects everything `level.json` needs from the ECS.
#[derive(SystemParam)]
pub struct LevelSnapshot<'w, 's> {
    settings: Res<'w, WorldSettings>,
    mode: Res<'w, GameMode>,
    tod: Res<'w, TimeOfDay>,
    inventory: Res<'w, Inventory>,
    vitals: Res<'w, Vitals>,
    xp: Res<'w, Experience>,
    spawn: Res<'w, SpawnPoint>,
    loaded: Res<'w, crate::player::LoadedLevel>,
    player: Query<'w, 's, (&'static Player, &'static Transform)>,
}

impl LevelSnapshot<'_, '_> {
    /// `None` while the player has not been placed in the world yet (nothing worth saving).
    pub fn capture(&self) -> Option<LevelData> {
        let (p, t) = self.player.single().ok()?;
        let name = self.loaded.0.as_ref().map_or_else(|| "World".to_string(), |l| l.name.clone());
        p.active.then(|| LevelData {
            version: LEVEL_VERSION,
            seed: self.settings.seed,
            spawned: true,
            player_pos: t.translation.to_array(),
            yaw: p.yaw,
            pitch: p.pitch,
            time_of_day: self.tod.t,
            selected_slot: self.inventory.selected,
            flying: p.flying,
            inventory: Some(self.inventory.to_save()),
            options: OptionsData::default(),
            name,
            game_mode: *self.mode,
            last_played: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            health: self.vitals.health,
            food: self.vitals.food,
            saturation: self.vitals.saturation,
            xp_total: self.xp.total,
            spawn_point: self.spawn.0.map(|v| v.to_array()),
            furnaces: self.inventory.furnace_saves(),
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct SaveReport {
    pub chunks_saved: usize,
    pub chunks_failed: usize,
    pub level_error: Option<String>,
}

impl SaveReport {
    pub fn ok(&self) -> bool {
        self.chunks_failed == 0 && self.level_error.is_none()
    }

    pub fn summary(&self) -> String {
        if self.ok() {
            format!("World saved ({} changed chunks)", self.chunks_saved)
        } else {
            format!(
                "Saving failed: {} chunks, level: {}",
                self.chunks_failed,
                self.level_error.as_deref().unwrap_or("ok")
            )
        }
    }
}

/// Writes every modified chunk and, if given, the level file.
pub fn save_world(world: &mut VoxelWorld, dir: &Path, level: Option<&LevelData>) -> SaveReport {
    let (chunks_saved, chunks_failed) = save_modified_chunks(world, dir);
    let level_error = level.and_then(|l| storage::save_level(dir, l).err()).map(|e| e.to_string());
    SaveReport { chunks_saved, chunks_failed, level_error }
}

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveRequest>()
            .add_message::<WindowCloseRequested>()
            .init_resource::<SaveStatus>()
            .insert_resource(LevelAutosave(Timer::new(Duration::from_secs(30), TimerMode::Repeating)))
            .add_systems(Update, (request_save_on_close, handle_save_requests, autosave_level).chain())
            .add_systems(Last, save_on_exit.in_set(OnAppExitSystems).after(ExitSystems));
    }
}

/// The window's close button saves first and only then quits (the default would just close it).
fn request_save_on_close(mut closes: MessageReader<WindowCloseRequested>, mut saves: MessageWriter<SaveRequest>) {
    if closes.read().next().is_some() {
        saves.write(SaveRequest { then_quit: true, then_title: false });
    }
}

fn handle_save_requests(
    mut requests: MessageReader<SaveRequest>,
    snapshot: LevelSnapshot,
    mut world: ResMut<VoxelWorld>,
    mut status: ResMut<SaveStatus>,
    mut exit: MessageWriter<AppExit>,
    mut next: ResMut<NextState<AppState>>,
) {
    let mut quit = false;
    let mut title = false;
    let mut any = false;
    for r in requests.read() {
        any = true;
        quit |= r.then_quit;
        title |= r.then_title;
    }
    if !any {
        return;
    }
    let report = save_world(&mut world, &snapshot.settings.save_dir, snapshot.capture().as_ref());
    if report.ok() {
        info!("{}", report.summary());
    } else {
        error!("{}", report.summary());
    }
    status.message = report.summary();
    status.ok = report.ok();
    if title && report.ok() {
        next.set(AppState::MainMenu);
        status.message.clear();
    }
    if quit {
        // A failed save keeps the game open once so the player can fix the disk and retry; asking
        // a second time quits regardless, so the window can never get stuck.
        if report.ok() || status.quit_refused {
            exit.write(AppExit::Success);
        } else {
            status.quit_refused = true;
            status.message.push_str(" - quit again to exit without saving");
        }
    }
}

fn autosave_level(time: Res<Time>, mut timer: ResMut<LevelAutosave>, snapshot: LevelSnapshot) {
    if timer.0.tick(time.delta()).just_finished()
        && let Some(level) = snapshot.capture()
        && let Err(e) = storage::save_level(&snapshot.settings.save_dir, &level)
    {
        error!("failed to save level: {e}");
    }
}

fn save_on_exit(mut exit: MessageReader<AppExit>, snapshot: LevelSnapshot, mut world: ResMut<VoxelWorld>) {
    if exit.read().next().is_some() {
        let report = save_world(&mut world, &snapshot.settings.save_dir, snapshot.capture().as_ref());
        if report.ok() {
            info!("{} on exit", report.summary());
        } else {
            error!("{}", report.summary());
        }
    }
}
