//! A clean-room voxel sandbox built on Bevy. All terrain, textures, fonts and UI are generated
//! procedurally; the repository contains no third-party game assets.

#![allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy systems take many parameters and complex queries.

pub mod inventory;
pub mod items;
pub mod mobs;
pub mod physics;
pub mod player;
pub mod rendering;
pub mod save;
pub mod session;
pub mod settings;
pub mod survival;
pub mod ui;
pub mod world;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;

/// Every gameplay plugin, in dependency order. Requires `WorldSettings` to be inserted first.
pub struct VoxelGamePlugins;

impl PluginGroup for VoxelGamePlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(world::streaming::ChunkStreamingPlugin)
            .add(rendering::RenderingPlugin)
            .add(session::SessionPlugin)
            .add(player::PlayerPlugin)
            .add(survival::SurvivalPlugin)
            .add(mobs::MobsPlugin)
            .add(save::SavePlugin)
            .add(ui::UiPlugin)
    }
}
