pub mod controller;
pub mod interaction;

use crate::rendering::sky::TimeOfDay;
use crate::world::BlockId;
use crate::world::storage::{self, LevelData};
use crate::world::streaming::WorldSettings;
use bevy::app::OnAppExitSystems;
use bevy::prelude::*;
use bevy::window::ExitSystems;
use std::time::Duration;

pub const HOTBAR_SLOTS: usize = 5;

#[derive(Resource, Clone, Debug)]
pub struct Hotbar {
    pub slots: [BlockId; HOTBAR_SLOTS],
    pub selected: usize,
}

impl Default for Hotbar {
    fn default() -> Self {
        Self { slots: [BlockId::DIRT, BlockId::STONE, BlockId::PLANKS, BlockId::LOG, BlockId::COBBLE], selected: 0 }
    }
}

impl Hotbar {
    pub fn current(&self) -> BlockId {
        self.slots[self.selected]
    }

    pub fn cycle(&mut self, steps: i32) {
        self.selected = (self.selected as i32 + steps).rem_euclid(HOTBAR_SLOTS as i32) as usize;
    }
}

#[derive(Resource)]
struct LevelSaveTimer(Timer);

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hotbar>()
            .insert_resource(LevelSaveTimer(Timer::new(Duration::from_secs(30), TimerMode::Repeating)))
            .add_plugins((controller::ControllerPlugin, interaction::InteractionPlugin))
            .add_systems(Update, autosave_level)
            .add_systems(Last, save_level_on_exit.in_set(OnAppExitSystems).after(ExitSystems));
    }
}

fn snapshot(
    settings: &WorldSettings,
    player: &controller::Player,
    transform: &Transform,
    tod: &TimeOfDay,
    hotbar: &Hotbar,
) -> LevelData {
    LevelData {
        seed: settings.seed,
        player_pos: transform.translation.to_array(),
        yaw: player.yaw,
        pitch: player.pitch,
        time_of_day: tod.t,
        selected_slot: hotbar.selected,
        flying: player.flying,
    }
}

fn autosave_level(
    time: Res<Time>,
    mut timer: ResMut<LevelSaveTimer>,
    settings: Res<WorldSettings>,
    q: Query<(&controller::Player, &Transform)>,
    tod: Res<TimeOfDay>,
    hotbar: Res<Hotbar>,
) {
    if timer.0.tick(time.delta()).just_finished()
        && let Ok((p, t)) = q.single()
        && p.active
        && let Err(e) = storage::save_level(&settings.save_dir, &snapshot(&settings, p, t, &tod, &hotbar))
    {
        error!("failed to save level: {e}");
    }
}

fn save_level_on_exit(
    mut exit: MessageReader<AppExit>,
    settings: Res<WorldSettings>,
    q: Query<(&controller::Player, &Transform)>,
    tod: Res<TimeOfDay>,
    hotbar: Res<Hotbar>,
) {
    if exit.read().next().is_some()
        && let Ok((p, t)) = q.single()
        && p.active
        && let Err(e) = storage::save_level(&settings.save_dir, &snapshot(&settings, p, t, &tod, &hotbar))
    {
        error!("failed to save level: {e}");
    }
}
