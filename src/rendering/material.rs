use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

/// Parameters shared with the voxel shader; updated every frame by the sky system.
#[derive(Clone, Copy, Debug)]
pub struct VoxelParams {
    /// rgb = fog / horizon colour, a = fog start distance.
    pub fog_color: Vec4,
    /// x = daylight (0..1), y = fog end distance, z = water alpha, w = twilight (0..1).
    pub params: Vec4,
}

impl Default for VoxelParams {
    fn default() -> Self {
        Self { fog_color: Vec4::new(0.6, 0.75, 0.95, 80.0), params: Vec4::new(1.0, 200.0, 1.0, 0.0) }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct VoxelMaterial {
    #[uniform(0)]
    pub fog_color: Vec4,
    #[uniform(1)]
    pub params: Vec4,
    #[texture(2)]
    #[sampler(3)]
    pub atlas: Handle<Image>,
    pub translucent: bool,
}

impl Material for VoxelMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://minecraft_kopie/rendering/voxel.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        if self.translucent { AlphaMode::Blend } else { AlphaMode::Opaque }
    }
}

pub struct VoxelMaterialPlugin;

impl Plugin for VoxelMaterialPlugin {
    fn build(&self, app: &mut App) {
        // The shader is compiled into the binary so the game runs from any working directory.
        bevy::asset::embedded_asset!(app, "voxel.wesl");
        app.add_plugins(MaterialPlugin::<VoxelMaterial>::default());
    }
}
