//! Breaking blocks takes time in survival mode: the progress is tracked per target block and
//! shown as a crack overlay with ten stages.

use crate::session::{AppState, GameEntity};
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

pub const CRACK_STAGES: usize = 10;
const CRACK_SIZE: usize = 16;

/// What the player is currently digging.
#[derive(Resource, Default, Debug)]
pub struct MiningState {
    pub target: Option<IVec3>,
    /// Seconds spent on the target so far.
    pub progress: f32,
    /// Seconds the target needs with the current tool.
    pub required: f32,
}

impl MiningState {
    pub fn reset(&mut self) {
        self.target = None;
        self.progress = 0.0;
        self.required = 0.0;
    }

    /// Crack stage `0..CRACK_STAGES` (`None` while nothing is being dug).
    pub fn stage(&self) -> Option<usize> {
        self.target?;
        if self.required <= 0.0 {
            return None;
        }
        Some(((self.progress / self.required * CRACK_STAGES as f32) as usize).min(CRACK_STAGES - 1))
    }
}

#[inline]
fn h(a: u32, b: u32) -> u32 {
    let mut x = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^ (x >> 13)
}

/// RGBA pixels of crack stage `stage`: eight branches grow outwards from the middle, each stage
/// extends all of them, so a later stage always contains every pixel of an earlier one.
pub fn crack_pixels(stage: usize) -> Vec<u8> {
    let mut px = vec![0u8; CRACK_SIZE * CRACK_SIZE * 4];
    let steps = 1 + stage * 2;
    for branch in 0..9u32 {
        let angle = (h(branch, 3) % 1000) as f32 / 1000.0 * std::f32::consts::TAU;
        let (mut x, mut y) = (7.5f32, 7.5f32);
        let mut dir = angle;
        // Later branches start a little away from the centre so they do not all overlap.
        if branch > 4 {
            x += angle.cos() * 3.0;
            y += angle.sin() * 3.0;
        }
        for step in 0..steps as u32 {
            let (ix, iy) = (x.round() as i32, y.round() as i32);
            if (0..CRACK_SIZE as i32).contains(&ix) && (0..CRACK_SIZE as i32).contains(&iy) {
                let i = (iy as usize * CRACK_SIZE + ix as usize) * 4;
                px[i..i + 4].copy_from_slice(&[18, 14, 12, 215]);
            }
            // Wander a little so the cracks look jagged.
            dir += ((h(branch * 31 + step, 7) % 100) as f32 / 100.0 - 0.5) * 1.1;
            x += dir.cos();
            y += dir.sin();
        }
    }
    px
}

#[derive(Resource)]
struct CrackAssets {
    materials: Vec<Handle<StandardMaterial>>,
    mesh: Handle<Mesh>,
}

#[derive(Component)]
struct CrackOverlay;

pub struct MiningPlugin;

impl Plugin for MiningPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MiningState>()
            .add_systems(Startup, setup_cracks)
            .add_systems(OnExit(AppState::InGame), |mut m: ResMut<MiningState>| m.reset())
            .add_systems(Update, update_overlay.run_if(in_state(AppState::InGame)));
    }
}

fn setup_cracks(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let handles = (0..CRACK_STAGES)
        .map(|stage| {
            let mut image = Image::new(
                Extent3d { width: CRACK_SIZE as u32, height: CRACK_SIZE as u32, depth_or_array_layers: 1 },
                TextureDimension::D2,
                crack_pixels(stage),
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            image.sampler = ImageSampler::nearest();
            materials.add(StandardMaterial {
                base_color_texture: Some(images.add(image)),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            })
        })
        .collect();
    commands.insert_resource(CrackAssets { materials: handles, mesh: meshes.add(Cuboid::new(1.0, 1.0, 1.0)) });
}

fn update_overlay(
    mut commands: Commands,
    state: Res<MiningState>,
    assets: Option<Res<CrackAssets>>,
    mut overlay: Query<(Entity, &mut Transform, &mut MeshMaterial3d<StandardMaterial>), With<CrackOverlay>>,
) {
    let Some(assets) = assets else { return };
    let (Some(target), Some(stage)) = (state.target, state.stage()) else {
        for (e, ..) in &overlay {
            commands.entity(e).despawn();
        }
        return;
    };
    let centre = target.as_vec3() + Vec3::splat(0.5);
    if let Ok((_, mut t, mut m)) = overlay.single_mut() {
        t.translation = centre;
        if m.0 != assets.materials[stage] {
            m.0 = assets.materials[stage].clone();
        }
    } else {
        commands.spawn((
            CrackOverlay,
            GameEntity,
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.materials[stage].clone()),
            Transform::from_translation(centre).with_scale(Vec3::splat(1.003)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covered(stage: usize) -> usize {
        crack_pixels(stage).chunks(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn cracks_grow_with_every_stage_and_never_shrink() {
        let mut prev = crack_pixels(0);
        assert!(covered(0) >= 3, "even the first stage is visible");
        for stage in 1..CRACK_STAGES {
            let now = crack_pixels(stage);
            for (a, b) in prev.chunks(4).zip(now.chunks(4)) {
                assert!(a[3] == 0 || b[3] > 0, "stage {stage} lost a pixel");
            }
            assert!(covered(stage) >= covered(stage - 1));
            prev = now;
        }
        assert!(covered(CRACK_STAGES - 1) > covered(0) * 3);
        assert!(covered(CRACK_STAGES - 1) < CRACK_SIZE * CRACK_SIZE / 2, "the block stays recognisable");
    }

    #[test]
    fn stage_follows_progress() {
        let mut m = MiningState::default();
        assert_eq!(m.stage(), None);
        m.target = Some(IVec3::ZERO);
        m.required = 2.0;
        assert_eq!(m.stage(), Some(0));
        m.progress = 1.0;
        assert_eq!(m.stage(), Some(5));
        m.progress = 5.0;
        assert_eq!(m.stage(), Some(CRACK_STAGES - 1), "never past the last stage");
        m.reset();
        assert_eq!(m.stage(), None);
    }
}
