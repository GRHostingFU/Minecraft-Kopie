//! Day/night cycle: sun and moon on a sky dome rig, sky/fog colour and the
//! global brightness fed to the voxel shader.

use super::chunk_mesh::VoxelMaterials;
use super::material::VoxelMaterial;
use crate::player::controller::PlayerCamera;
use bevy::prelude::*;
use std::f32::consts::TAU;

/// Seconds for one full day.
pub const DAY_LENGTH_SECS: f32 = 600.0;

#[derive(Resource, Clone, Copy, Debug)]
pub struct TimeOfDay {
    /// 0.0 = midnight, 0.25 = sunrise, 0.5 = noon, 0.75 = sunset.
    pub t: f32,
    pub speed: f32,
}

impl Default for TimeOfDay {
    fn default() -> Self {
        Self { t: 0.32, speed: 1.0 }
    }
}

impl TimeOfDay {
    /// Sun height: +1 at noon, -1 at midnight.
    pub fn sun_elevation(&self) -> f32 {
        ((self.t - 0.25) * TAU).sin()
    }

    /// 0 at night, 1 in full daylight, with a wide, smooth dawn/dusk ramp.
    pub fn daylight(&self) -> f32 {
        smoothstep(-0.18, 0.32, self.sun_elevation())
    }

    /// 1 while the sun sits on the horizon, fading out smoothly towards noon and midnight.
    pub fn twilight(&self) -> f32 {
        let e = self.sun_elevation() / 0.16;
        (-e * e).exp()
    }

    /// Horizon/sky colour used for the clear colour and fog.
    pub fn sky_color(&self) -> Vec3 {
        let day = Vec3::new(0.47, 0.70, 0.96);
        let night = Vec3::new(0.012, 0.02, 0.06);
        let dusk = Vec3::new(0.98, 0.55, 0.30);
        let base = night.lerp(day, self.daylight());
        base.lerp(dusk, self.twilight() * 0.5)
    }
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Component)]
pub struct SkyRig;

#[derive(Component)]
struct SunDisc;

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimeOfDay>()
            .insert_resource(ClearColor(Color::srgb(0.5, 0.72, 0.96)))
            .add_systems(Startup, spawn_sky)
            .add_systems(Update, (advance_time, update_sky, update_shader_params).chain());
    }
}

const SKY_DISTANCE: f32 = 900.0;

fn spawn_sky(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let sun_mat = materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.95, 0.7), unlit: true, ..default() });
    let moon_mat =
        materials.add(StandardMaterial { base_color: Color::srgb(0.85, 0.88, 0.95), unlit: true, ..default() });
    let disc = meshes.add(Cuboid::new(110.0, 110.0, 8.0));
    commands.spawn((SkyRig, Transform::default(), Visibility::default())).with_children(|p| {
        p.spawn((
            SunDisc,
            Mesh3d(disc.clone()),
            MeshMaterial3d(sun_mat),
            Transform::from_xyz(0.0, SKY_DISTANCE, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
        ));
        p.spawn((
            Mesh3d(disc),
            MeshMaterial3d(moon_mat),
            Transform::from_xyz(0.0, -SKY_DISTANCE, 0.0).looking_at(Vec3::ZERO, Vec3::Z).with_scale(Vec3::splat(0.7)),
        ));
    });
}

fn advance_time(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, mut tod: ResMut<TimeOfDay>) {
    // T cycles through time-lapse speeds.
    if keys.just_pressed(KeyCode::KeyT) {
        tod.speed = match tod.speed {
            s if s < 2.0 => 30.0,
            s if s < 60.0 => 150.0,
            _ => 1.0,
        };
    }
    let dt = time.delta_secs() * tod.speed / DAY_LENGTH_SECS;
    tod.t = (tod.t + dt).rem_euclid(1.0);
}

fn update_sky(
    tod: Res<TimeOfDay>,
    camera: Query<&GlobalTransform, With<PlayerCamera>>,
    mut rig: Query<&mut Transform, With<SkyRig>>,
    mut clear: ResMut<ClearColor>,
) {
    let c = tod.sky_color();
    clear.0 = Color::srgb(c.x, c.y, c.z);
    if let (Ok(cam), Ok(mut t)) = (camera.single(), rig.single_mut()) {
        t.translation = cam.translation();
        // Sun rises in +X, passes overhead at noon, sets in -X.
        t.rotation = Quat::from_rotation_z((tod.t - 0.5) * TAU);
    }
}

fn update_shader_params(
    tod: Res<TimeOfDay>,
    handles: Option<Res<VoxelMaterials>>,
    settings: Res<crate::world::streaming::WorldSettings>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
) {
    let Some(handles) = handles else { return };
    let c = tod.sky_color();
    let fog_end = (settings.render_distance as f32 * 16.0 - 8.0).max(48.0);
    let fog = Vec4::new(c.x, c.y, c.z, fog_end * 0.3);
    for (handle, alpha) in [(&handles.opaque, 1.0), (&handles.water, 0.84)] {
        if let Some(mut m) = materials.get_mut(handle) {
            m.fog_color = fog;
            m.params = Vec4::new(tod.daylight(), fog_end, alpha, tod.twilight());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_is_bright_midnight_is_dark() {
        let noon = TimeOfDay { t: 0.5, speed: 1.0 };
        let midnight = TimeOfDay { t: 0.0, speed: 1.0 };
        assert!(noon.daylight() > 0.99);
        assert!(midnight.daylight() < 0.01);
        assert!(noon.sky_color().length() > midnight.sky_color().length() * 5.0);
    }

    #[test]
    fn twilight_peaks_at_the_horizon_only() {
        let sunrise = TimeOfDay { t: 0.25, speed: 1.0 };
        let noon = TimeOfDay { t: 0.5, speed: 1.0 };
        let midnight = TimeOfDay { t: 0.0, speed: 1.0 };
        assert!(sunrise.twilight() > 0.99);
        assert!(noon.twilight() < 0.01 && midnight.twilight() < 0.01);
        // The sky warms up around sunrise compared with the same daylight level at noon.
        assert!(sunrise.sky_color().x > sunrise.sky_color().z * 0.8);
    }

    #[test]
    fn daylight_is_continuous() {
        let mut prev = TimeOfDay { t: 0.0, speed: 1.0 }.daylight();
        for i in 1..=1000 {
            let d = TimeOfDay { t: i as f32 / 1000.0, speed: 1.0 }.daylight();
            assert!((d - prev).abs() < 0.03, "jump at {i}");
            prev = d;
        }
    }
}
