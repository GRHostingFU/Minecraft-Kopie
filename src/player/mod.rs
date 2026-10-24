pub mod controller;
pub mod controls;
pub mod interaction;
pub mod viewmodel;

use crate::world::storage::LevelData;
use bevy::prelude::*;

/// The `level.json` of the running world (`None` outside a session).
#[derive(Resource, Clone, Debug, Default)]
pub struct LoadedLevel(pub Option<LevelData>);

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadedLevel>()
            .init_resource::<crate::inventory::Inventory>()
            .init_resource::<crate::settings::GameSettings>()
            .add_plugins((
                controller::ControllerPlugin,
                controls::ControlsPlugin,
                interaction::InteractionPlugin,
                viewmodel::ViewModelPlugin,
            ));
    }
}
