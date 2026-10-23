//! First-person hand: a separately rendered view model (own camera and render layer, fixed FOV,
//! its own depth buffer so it never clips into walls) that holds the selected hotbar item as a
//! small 3D mesh and plays swing, walk-bob, sprint and equip animations.

use super::controller::{InputCapture, Player, PlayerCamera};
use super::interaction::HandSwing;
use crate::inventory::Inventory;
use crate::items::Item;
use crate::rendering::AtlasHandle;
use crate::rendering::atlas::tile_uv_rect;
use crate::rendering::sky::TimeOfDay;
use crate::world::block::{BlockId, Face, Shape};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use std::f32::consts::PI;

/// Render layer of everything that belongs to the hand.
pub const VIEW_MODEL_LAYER: usize = 1;
/// Fixed FOV of the view model camera; independent of the player's FOV setting and sprint widening.
const VIEW_MODEL_FOV_DEG: f32 = 70.0;
/// Seconds for one full arm swing.
pub const SWING_SECS: f32 = 0.3;
const EQUIP_SECS: f32 = 0.22;
/// Resting position of the hand in view space (x right, y up, -z forward).
const HAND_REST: Vec3 = Vec3::new(0.44, -0.37, -0.74);
const SKIN: Color = Color::srgb(0.80, 0.60, 0.46);

// ---------------------------------------------------------------------------------------------
// Animation maths (pure functions, unit-tested)
// ---------------------------------------------------------------------------------------------

/// Pose offset of the swing animation at `progress` in `0..=1`. At 0 and 1 the offset is the
/// identity, in between the hand sweeps forward-left, dips and comes back (Minecraft's curve).
pub fn swing_pose(progress: f32) -> (Vec3, Quat) {
    let p = progress.clamp(0.0, 1.0);
    let f = (p * p * PI).sin();
    let f1 = (p.sqrt() * PI).sin();
    let f2 = (p.sqrt() * 2.0 * PI).sin();
    let offset = Vec3::new(-0.4 * f1, 0.2 * f2, -0.2 * f);
    // Built from the three Euler pieces of the original animation, composed as quaternions.
    let rot = Quat::from_rotation_y((45.0 - 20.0 * f).to_radians())
        * Quat::from_rotation_z((-20.0 * f1).to_radians())
        * Quat::from_rotation_x((-80.0 * f1).to_radians())
        * Quat::from_rotation_y((-45.0f32).to_radians());
    (offset, rot)
}

/// Sway while walking. `phase` advances with the distance walked, `amp` is 0 when standing still
/// and 1 at walking speed, `sprint` blends in the sprint pose (tilted sideways, lowered, livelier).
pub fn bob_pose(phase: f32, amp: f32, sprint: f32) -> (Vec3, Quat) {
    let a = amp * (1.0 + 0.45 * sprint);
    let (s, c) = (phase.sin(), phase.cos().abs());
    let offset = Vec3::new(s * 0.045 * a - 0.06 * sprint, -c * 0.04 * a - 0.035 * sprint, 0.0);
    let rot = Quat::from_rotation_z((s * 2.5 * a - 14.0 * sprint).to_radians())
        * Quat::from_rotation_x((c * 3.0 * a).to_radians())
        * Quat::from_rotation_y((-10.0 * sprint).to_radians());
    (offset, rot)
}

/// Local rotation of the arm inside the pivot so that its world rotation is only
/// [`ARM_FOLLOW`] of the pivot's rotation.
pub fn arm_local_rotation(pivot_rotation: Quat) -> Quat {
    pivot_rotation.inverse() * Quat::IDENTITY.slerp(pivot_rotation, ARM_FOLLOW)
}

/// Offset while a newly selected item is raised into view; `t` goes from 0 (hidden) to 1 (in place).
pub fn equip_offset(t: f32) -> Vec3 {
    let t = t.clamp(0.0, 1.0);
    let ease = 1.0 - (1.0 - t) * (1.0 - t);
    Vec3::new(0.0, -0.6 * (1.0 - ease), 0.0)
}

/// How fast the walk phase advances: proportional to speed, and a bit quicker while sprinting.
pub fn bob_phase_rate(horizontal_speed: f32, sprint: f32) -> f32 {
    horizontal_speed * 1.15 * PI * (1.0 + 0.25 * sprint)
}

// ---------------------------------------------------------------------------------------------
// Meshes
// ---------------------------------------------------------------------------------------------

struct Face3 {
    normal: Vec3,
    right: Vec3,
    up: Vec3,
    shade: f32,
}

const CUBE_FACES: [Face3; 6] = [
    Face3 { normal: Vec3::Z, right: Vec3::X, up: Vec3::Y, shade: 0.80 },
    Face3 { normal: Vec3::NEG_Z, right: Vec3::NEG_X, up: Vec3::Y, shade: 0.80 },
    Face3 { normal: Vec3::X, right: Vec3::NEG_Z, up: Vec3::Y, shade: 0.62 },
    Face3 { normal: Vec3::NEG_X, right: Vec3::Z, up: Vec3::Y, shade: 0.62 },
    Face3 { normal: Vec3::Y, right: Vec3::X, up: Vec3::NEG_Z, shade: 1.0 },
    Face3 { normal: Vec3::NEG_Y, right: Vec3::X, up: Vec3::Z, shade: 0.50 },
];

#[derive(Default)]
struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    /// Adds one quad: `centre`, half extents along `right`/`up`, atlas rect `uv`.
    fn quad(&mut self, centre: Vec3, right: Vec3, up: Vec3, normal: Vec3, uv: Rect, shade: f32) {
        let base = self.positions.len() as u32;
        for (c, uv) in [
            (centre - right - up, Vec2::new(uv.min.x, uv.max.y)),
            (centre + right - up, Vec2::new(uv.max.x, uv.max.y)),
            (centre + right + up, Vec2::new(uv.max.x, uv.min.y)),
            (centre - right + up, Vec2::new(uv.min.x, uv.min.y)),
        ] {
            self.positions.push(c.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv.to_array());
            self.colors.push([shade, shade, shade, 1.0]);
        }
        self.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn build(self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

/// A unit cube (centred on the origin) textured like the block, with Minecraft-style face shading.
pub fn block_cube_mesh(block: BlockId) -> Mesh {
    let mut b = MeshBuilder::default();
    for f in &CUBE_FACES {
        let face = if f.normal == Vec3::Y {
            Face::Top
        } else if f.normal == Vec3::NEG_Y {
            Face::Bottom
        } else {
            Face::Side
        };
        b.quad(f.normal * 0.5, f.right * 0.5, f.up * 0.5, f.normal, tile_uv_rect(block.tile(face)), f.shade);
    }
    b.build()
}

/// A flat, unit-sized icon quad facing +Z (for items that are not blocks).
pub fn icon_quad_mesh(tile: u8) -> Mesh {
    let mut b = MeshBuilder::default();
    b.quad(Vec3::ZERO, Vec3::X * 0.5, Vec3::Y * 0.5, Vec3::Z, tile_uv_rect(tile), 1.0);
    b.build()
}

/// Blocks that are shown as a small textured cube; everything else (tools, torches, glass, ...)
/// as a flat icon.
pub fn is_cube_item(item: Item) -> bool {
    item.as_block().is_some_and(|b| b.shape() == Shape::Cube && !b.is_translucent())
}

pub fn item_mesh(item: Item) -> Mesh {
    match item.as_block() {
        Some(block) if is_cube_item(item) => block_cube_mesh(block),
        _ => icon_quad_mesh(item.icon_tile()),
    }
}

// ---------------------------------------------------------------------------------------------
// ECS
// ---------------------------------------------------------------------------------------------

#[derive(Component)]
pub struct ViewModelCamera;

/// Parent of arm and item; this is the entity the animations move.
#[derive(Component)]
struct HandPivot;

/// Holds the arm meshes. It counter-rotates part of the swing so the arm lags behind the item and
/// never sweeps across the whole screen.
#[derive(Component)]
struct ArmRig;

#[derive(Component)]
struct HeldItemModel;

/// Share of the swing/bob rotation the arm follows (the held item follows all of it).
const ARM_FOLLOW: f32 = 0.4;

#[derive(Resource)]
struct HandAssets {
    block_material: Handle<StandardMaterial>,
    icon_material: Handle<StandardMaterial>,
    meshes: HashMap<Item, Handle<Mesh>>,
}

/// Animation state of the hand.
#[derive(Resource, Debug)]
pub struct HandAnimation {
    /// `Some(progress)` while the arm is swinging.
    pub swing: Option<f32>,
    pub bob_phase: f32,
    pub bob_amp: f32,
    /// 0 = walking, 1 = fully in the sprint pose.
    pub sprint_blend: f32,
    pub sneak_blend: f32,
    /// 0 = item just selected (lowered), 1 = in place.
    pub equip: f32,
    shown: Option<Item>,
    shown_valid: bool,
}

impl Default for HandAnimation {
    fn default() -> Self {
        Self {
            swing: None,
            bob_phase: 0.0,
            bob_amp: 0.0,
            sprint_blend: 0.0,
            sneak_blend: 0.0,
            equip: 1.0,
            shown: None,
            shown_valid: false,
        }
    }
}

impl HandAnimation {
    /// Starts a swing unless one is already running.
    pub fn start_swing(&mut self) {
        if self.swing.is_none() {
            self.swing = Some(0.0);
        }
    }

    /// Advances all animations by `dt`. `speed` is the horizontal speed; `moving` is true while
    /// the player walks on the ground.
    pub fn tick(&mut self, dt: f32, speed: f32, moving: bool, sprinting: bool, sneaking: bool) {
        if let Some(t) = &mut self.swing {
            *t += dt / SWING_SECS;
            if *t >= 1.0 {
                self.swing = None;
            }
        }
        let ease = |current: &mut f32, target: f32, rate: f32| {
            *current += (target - *current) * (1.0 - (-rate * dt).exp());
        };
        ease(&mut self.sprint_blend, sprinting as i32 as f32, 9.0);
        ease(&mut self.sneak_blend, sneaking as i32 as f32, 9.0);
        let target_amp = if moving { (speed / 4.3).clamp(0.0, 1.4) } else { 0.0 };
        ease(&mut self.bob_amp, target_amp, 10.0);
        if moving {
            self.bob_phase = (self.bob_phase + bob_phase_rate(speed, self.sprint_blend) * dt) % (2.0 * PI * 64.0);
        }
        self.equip = (self.equip + dt / EQUIP_SECS).min(1.0);
    }

    /// Full transform of the hand pivot in view space.
    pub fn pose(&self, idle_time: f32) -> Transform {
        let (bob_pos, bob_rot) = bob_pose(self.bob_phase, self.bob_amp, self.sprint_blend);
        let (swing_pos, swing_rot) = self.swing.map_or((Vec3::ZERO, Quat::IDENTITY), swing_pose);
        // Barely visible breathing so the hand is never perfectly still.
        let idle = Vec3::new(0.0, (idle_time * 1.4).sin() * 0.003, 0.0);
        let sneak = Vec3::new(0.0, -0.025, 0.02) * self.sneak_blend;
        Transform {
            translation: HAND_REST + swing_pos + bob_pos + equip_offset(self.equip) + idle + sneak,
            rotation: bob_rot * swing_rot,
            scale: Vec3::ONE,
        }
    }
}

pub struct ViewModelPlugin;

impl Plugin for ViewModelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HandAnimation>().add_systems(
            Update,
            (animate_hand, swap_held_model, shade_hand).chain().run_if(in_state(crate::session::AppState::InGame)),
        );
    }
}

/// Builds the hand under the player camera (runs right after the player was spawned).
pub fn spawn_view_model(
    mut commands: Commands,
    atlas: Res<AtlasHandle>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    player_camera: Query<Entity, With<PlayerCamera>>,
) {
    let Ok(parent) = player_camera.single() else { return };
    // A new session starts with a fresh hand (nothing shown yet).
    commands.insert_resource(HandAnimation::default());
    let layer = RenderLayers::layer(VIEW_MODEL_LAYER);
    let textured = |alpha_mode| StandardMaterial {
        base_color_texture: Some(atlas.0.clone()),
        unlit: true,
        alpha_mode,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    commands.insert_resource(HandAssets {
        block_material: materials.add(textured(AlphaMode::Opaque)),
        icon_material: materials.add(textured(AlphaMode::Mask(0.5))),
        meshes: HashMap::default(),
    });
    let skin = materials.add(StandardMaterial { base_color: SKIN, unlit: true, ..default() });
    let sleeve =
        materials.add(StandardMaterial { base_color: Color::srgb(0.20, 0.42, 0.62), unlit: true, ..default() });

    // The arm runs from the hand back towards the lower-right corner of the screen.
    let dir = Vec3::new(0.36, -0.42, 0.83).normalize();
    let arm_rot = Quat::from_rotation_arc(Vec3::Z, dir);
    commands.entity(parent).with_children(|cam| {
        cam.spawn((
            ViewModelCamera,
            Camera3d::default(),
            Camera { order: 1, ..default() },
            Projection::Perspective(PerspectiveProjection {
                fov: VIEW_MODEL_FOV_DEG.to_radians(),
                near: 0.01,
                far: 10.0,
                ..default()
            }),
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            bevy::core_pipeline::tonemapping::DebandDither::Disabled,
            layer.clone(),
        ))
        .with_children(|vm| {
            vm.spawn((HandPivot, Transform::from_translation(HAND_REST), Visibility::default())).with_children(
                |hand| {
                    hand.spawn((ArmRig, Transform::IDENTITY, Visibility::default())).with_children(|arm| {
                        arm.spawn((
                            Mesh3d(meshes.add(Cuboid::new(0.17, 0.17, 0.36))),
                            MeshMaterial3d(skin),
                            Transform::from_translation(dir * 0.16).with_rotation(arm_rot),
                            layer.clone(),
                        ));
                        arm.spawn((
                            Mesh3d(meshes.add(Cuboid::new(0.20, 0.20, 0.55))),
                            MeshMaterial3d(sleeve),
                            Transform::from_translation(dir * 0.60).with_rotation(arm_rot),
                            layer.clone(),
                        ));
                    });
                },
            );
        });
    });
}

fn animate_hand(
    time: Res<Time>,
    capture: Res<InputCapture>,
    mut anim: ResMut<HandAnimation>,
    mut swings: MessageReader<HandSwing>,
    player: Query<&Player>,
    mut pivot: Query<(&mut Transform, &mut Visibility), With<HandPivot>>,
    mut arm: Query<&mut Transform, (With<ArmRig>, Without<HandPivot>)>,
) {
    let (Ok(p), Ok((mut tf, mut vis))) = (player.single(), pivot.single_mut()) else { return };
    for _ in swings.read() {
        anim.start_swing();
    }
    let speed = Vec2::new(p.vel.x, p.vel.z).length();
    let moving = p.active && p.on_ground && !p.flying && speed > 0.3;
    anim.tick(time.delta_secs(), speed, moving, p.sprinting, p.sneaking);
    *tf = anim.pose(time.elapsed_secs());
    if let Ok(mut arm) = arm.single_mut() {
        arm.rotation = arm_local_rotation(tf.rotation);
    }
    // The hand only shows while playing, not behind the inventory or pause screen.
    let want = if capture.0 { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
}

/// Replaces the held mesh whenever the selected hotbar item changes.
fn swap_held_model(
    mut commands: Commands,
    inventory: Res<Inventory>,
    assets: Option<ResMut<HandAssets>>,
    mut anim: ResMut<HandAnimation>,
    mut meshes: ResMut<Assets<Mesh>>,
    pivot: Query<Entity, With<HandPivot>>,
    old: Query<Entity, With<HeldItemModel>>,
) {
    let (Some(mut assets), Ok(pivot)) = (assets, pivot.single()) else { return };
    let held = inventory.held_item();
    if anim.shown_valid && anim.shown == held {
        return;
    }
    // Raise the new item into view; the very first model appears without the animation.
    if anim.shown_valid {
        anim.equip = 0.0;
    }
    anim.shown = held;
    anim.shown_valid = true;
    for e in &old {
        commands.entity(e).despawn();
    }
    let Some(item) = held else { return };
    let mesh = assets.meshes.entry(item).or_insert_with(|| meshes.add(item_mesh(item))).clone();
    let is_block = is_cube_item(item);
    let material = if is_block { assets.block_material.clone() } else { assets.icon_material.clone() };
    // Blocks sit in the palm, slightly turned so three faces show; icons are held upright.
    let transform = if is_block {
        Transform::from_translation(Vec3::new(-0.02, 0.09, -0.04))
            .with_rotation(Quat::from_rotation_y(-0.62) * Quat::from_rotation_x(0.30))
            .with_scale(Vec3::splat(0.23))
    } else {
        Transform::from_translation(Vec3::new(-0.02, 0.12, -0.05))
            .with_rotation(Quat::from_rotation_y(-0.35) * Quat::from_rotation_z(-0.15))
            .with_scale(Vec3::splat(0.42))
    };
    commands.entity(pivot).with_children(|p| {
        p.spawn((
            HeldItemModel,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            RenderLayers::layer(VIEW_MODEL_LAYER),
        ));
    });
}

/// Dims the hand at night so it does not glow against a dark world.
fn shade_hand(tod: Res<TimeOfDay>, assets: Option<Res<HandAssets>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let Some(assets) = assets else { return };
    let b = 0.42 + 0.58 * tod.daylight();
    for h in [&assets.block_material, &assets.icon_material] {
        if let Some(mut m) = materials.get_mut(h) {
            m.base_color = Color::srgb(b, b, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_starts_and_ends_at_rest_and_moves_in_between() {
        let rest = swing_pose(0.0);
        let end = swing_pose(1.0);
        assert!(rest.0.length() < 1e-5 && end.0.length() < 1e-5);
        // At rest and at the end the rotation is the neutral Y(45)*Y(-45) = identity.
        assert!(rest.1.angle_between(Quat::IDENTITY) < 1e-4);
        assert!(end.1.angle_between(Quat::IDENTITY) < 0.4, "end pose should be close to rest");
        let mid = swing_pose(0.5);
        assert!(mid.0.length() > 0.2, "hand must travel: {:?}", mid.0);
        assert!(mid.1.angle_between(Quat::IDENTITY) > 0.8, "hand must rotate");
        // Forward motion, i.e. away from the camera (-Z), peaks mid-swing.
        let forward = |p: f32| -swing_pose(p).0.z;
        assert!(forward(0.7) > forward(0.1) && forward(0.7) > forward(1.0));
    }

    #[test]
    fn swing_is_smooth() {
        let mut prev = swing_pose(0.0);
        for i in 1..=200 {
            let cur = swing_pose(i as f32 / 200.0);
            // The curve is steepest right at the start (sqrt of progress), as in the original.
            assert!((cur.0 - prev.0).length() < 0.2, "position jump at {i}");
            assert!(cur.1.angle_between(prev.1) < 0.5, "rotation jump at {i}");
            if i > 20 {
                assert!((cur.0 - prev.0).length() < 0.05, "position jump at {i}");
                assert!(cur.1.angle_between(prev.1) < 0.12, "rotation jump at {i}");
            }
            prev = cur;
        }
    }

    #[test]
    fn bob_is_periodic_and_zero_when_standing() {
        let still = bob_pose(1.23, 0.0, 0.0);
        assert!(still.0.length() < 1e-6 && still.1.angle_between(Quat::IDENTITY) < 1e-5);
        let a = bob_pose(0.7, 1.0, 0.0);
        let b = bob_pose(0.7 + 2.0 * PI, 1.0, 0.0);
        assert!((a.0 - b.0).length() < 1e-4);
        // The hand dips twice per full phase cycle (|cos|), never rises above its rest height.
        for i in 0..100 {
            assert!(bob_pose(i as f32 * 0.1, 1.0, 0.0).0.y <= 1e-6);
        }
    }

    #[test]
    fn sprinting_tilts_the_hand_to_the_side_and_speeds_up_the_bob() {
        let walk = bob_pose(0.0, 1.0, 0.0);
        let sprint = bob_pose(0.0, 1.0, 1.0);
        assert!(sprint.1.angle_between(walk.1) > 0.2, "sprint pose must be visibly tilted");
        assert!(sprint.0.x < walk.0.x, "hand shifts sideways");
        assert!(sprint.0.y < walk.0.y, "hand is lowered");
        assert!(bob_phase_rate(5.6, 1.0) > bob_phase_rate(5.6, 0.0) * 1.2);
        assert!(bob_phase_rate(5.6, 0.0) > bob_phase_rate(4.3, 0.0));
    }

    #[test]
    fn arm_lags_behind_the_swinging_item() {
        for i in 0..=20 {
            let (_, rot) = swing_pose(i as f32 / 20.0);
            let world_arm = rot * arm_local_rotation(rot);
            let full = rot.angle_between(Quat::IDENTITY);
            let arm = world_arm.angle_between(Quat::IDENTITY);
            assert!(arm <= full * ARM_FOLLOW + 1e-3, "arm turns {arm} while the item turns {full}");
        }
        assert!(arm_local_rotation(Quat::IDENTITY).angle_between(Quat::IDENTITY) < 1e-6);
    }

    #[test]
    fn equip_raises_the_item_from_below() {
        assert!(equip_offset(0.0).y < -0.5);
        assert!(equip_offset(1.0).length() < 1e-6);
        assert!(equip_offset(0.5).y > equip_offset(0.0).y);
    }

    #[test]
    fn animation_state_machine() {
        let mut a = HandAnimation::default();
        a.start_swing();
        let first = a.swing;
        a.tick(SWING_SECS * 0.5, 0.0, false, false, false);
        a.start_swing(); // ignored while swinging
        assert!(a.swing.unwrap() > first.unwrap());
        a.tick(SWING_SECS, 0.0, false, false, false);
        assert_eq!(a.swing, None);

        // Walking builds up bobbing, sprinting changes the pose.
        for _ in 0..60 {
            a.tick(1.0 / 60.0, 4.3, true, false, false);
        }
        assert!(a.bob_amp > 0.9 && a.bob_phase > 1.0);
        let walking = a.pose(0.0);
        for _ in 0..60 {
            a.tick(1.0 / 60.0, 5.6, true, true, false);
        }
        assert!(a.sprint_blend > 0.95);
        assert!(a.pose(0.0).rotation.angle_between(walking.rotation) > 0.2);
        // Stopping lets the bob fade out again.
        for _ in 0..120 {
            a.tick(1.0 / 60.0, 0.0, false, false, false);
        }
        assert!(a.bob_amp < 0.01 && a.sprint_blend < 0.01);
    }

    #[test]
    fn held_meshes_have_valid_geometry() {
        let cube = block_cube_mesh(BlockId::GRASS);
        assert_eq!(cube.count_vertices(), 24);
        assert_eq!(cube.indices().map(|i| i.len()), Some(36));
        let icon = icon_quad_mesh(crate::world::block::tile::HELMET);
        assert_eq!(icon.count_vertices(), 4);
        // Every UV stays inside the atlas.
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) = cube.attribute(Mesh::ATTRIBUTE_UV_0) else {
            panic!("missing uvs")
        };
        assert!(uvs.iter().all(|[u, v]| (0.0..=1.0).contains(u) && (0.0..=1.0).contains(v)));
    }
}
