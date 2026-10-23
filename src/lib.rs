//! A clean-room voxel sandbox built on Bevy. All terrain and textures are
//! generated procedurally; the repository contains no third-party game assets.

pub mod inventory;
pub mod physics;
pub mod player;
pub mod rendering;
pub mod save;
pub mod settings;
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
            .add(player::PlayerPlugin)
            .add(save::SavePlugin)
            .add(ui::UiPlugin)
    }
}
