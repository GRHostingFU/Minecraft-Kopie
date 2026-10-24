pub mod controller;
pub mod health;
pub mod interaction;
pub mod viewmodel;

use crate::inventory::Inventory;
use crate::rendering::sky::TimeOfDay;
use crate::settings::{self, GameSettings};
use crate::world::storage::{self, LevelData};
use crate::world::streaming::WorldSettings;
use bevy::prelude::*;

/// The `level.json` found at startup (or the freshly created one), consumed by the spawn code.
#[derive(Resource, Clone, Debug)]
pub struct LoadedLevel(pub Option<LevelData>);

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        let settings = app.world().get_resource::<WorldSettings>().cloned().expect("insert WorldSettings first");
        let mut level = storage::load_level(&settings.save_dir);
        if level.is_none() {
            // Persist the seed right away: chunks saved before the first autosave must never be
            // paired with a different seed on the next start.
            let fresh = LevelData::fresh(settings.seed);
            match storage::save_level(&settings.save_dir, &fresh) {
                Ok(()) => level = Some(fresh),
                Err(e) => error!("cannot create {}: {e}", settings.save_dir.display()),
            }
        }

        let inventory = level
            .as_ref()
            .and_then(|l| l.inventory.as_ref().map(|inv| Inventory::from_save(inv, l.selected_slot)))
            .unwrap_or_default();
        let options = level.as_ref().map(|l| l.options).unwrap_or_default();
        let defaults = GameSettings::default();
        let game_settings = GameSettings {
            fov: options.fov.unwrap_or(defaults.fov),
            mouse_sensitivity: options.mouse_sensitivity.unwrap_or(defaults.mouse_sensitivity),
            fullscreen: options.fullscreen.unwrap_or(defaults.fullscreen),
        }
        .sanitized();
        if let Some(l) = level.as_ref().filter(|l| l.spawned) {
            let t = if l.time_of_day.is_finite() { l.time_of_day.rem_euclid(1.0) } else { 0.32 };
            app.insert_resource(TimeOfDay { t, ..default() });
        }
        debug_assert!(settings::clamp_render_distance(settings.render_distance) == settings.render_distance);

        app.insert_resource(inventory)
            .insert_resource(game_settings)
            .insert_resource(LoadedLevel(level))
            .add_plugins((
                controller::ControllerPlugin,
                health::HealthPlugin,
                interaction::InteractionPlugin,
                viewmodel::ViewModelPlugin,
            ));
    }
}
