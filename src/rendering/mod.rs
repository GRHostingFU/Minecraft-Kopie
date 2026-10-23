pub mod atlas;
pub mod chunk_mesh;
pub mod material;
pub mod mesher;
pub mod sky;

use bevy::prelude::*;
use chunk_mesh::{ChunkMeshPlugin, VoxelMaterials};
use material::{VoxelMaterial, VoxelMaterialPlugin, VoxelParams};

pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((VoxelMaterialPlugin, ChunkMeshPlugin, sky::SkyPlugin))
            .add_systems(PreStartup, setup_materials);
    }
}

/// Handle to the shared procedural atlas image (also used by the hotbar UI).
#[derive(Resource, Clone)]
pub struct AtlasHandle(pub Handle<Image>);

fn setup_materials(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
) {
    let atlas = images.add(atlas::build_atlas_image());
    let p = VoxelParams::default();
    let opaque = materials.add(VoxelMaterial {
        fog_color: p.fog_color,
        params: p.params,
        atlas: atlas.clone(),
        translucent: false,
    });
    let water = materials.add(VoxelMaterial {
        fog_color: p.fog_color,
        params: p.params,
        atlas: atlas.clone(),
        translucent: true,
    });
    commands.insert_resource(AtlasHandle(atlas));
    commands.insert_resource(VoxelMaterials { opaque, water });
}
