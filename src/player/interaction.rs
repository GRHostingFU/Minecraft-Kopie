//! Block targeting, breaking (with debris particles), placing and hotbar input.

use super::controller::{InputCapture, Player, PlayerCamera};
use super::{HOTBAR_SLOTS, Hotbar};
use crate::physics::collision::Body;
use crate::physics::raycast::{RayHit, raycast};
use crate::world::VoxelWorld;
use crate::world::block::BlockId;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

pub const REACH: f32 = 6.0;
const REPEAT_SECS: f32 = 0.2;

/// The block under the crosshair, if any.
#[derive(Resource, Default)]
pub struct Target(pub Option<RayHit>);

#[derive(Resource, Default)]
struct ActionCooldown(f32);

#[derive(Component)]
struct Particle {
    vel: Vec3,
    life: f32,
}

#[derive(Resource, Default)]
struct ParticleAssets {
    mesh: Option<Handle<Mesh>>,
    materials: HashMap<u8, Handle<StandardMaterial>>,
}

const MAX_PARTICLES: usize = 400;

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Target>().init_resource::<ActionCooldown>().init_resource::<ParticleAssets>().add_systems(
            Update,
            (hotbar_input, update_target, block_actions, draw_highlight, update_particles).chain(),
        );
    }
}

fn hotbar_input(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    capture: Res<InputCapture>,
    mut hotbar: ResMut<Hotbar>,
) {
    if !capture.0 {
        return;
    }
    const DIGITS: [KeyCode; HOTBAR_SLOTS] =
        [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5];
    for (i, k) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*k) {
            hotbar.selected = i;
        }
    }
    if scroll.delta.y != 0.0 {
        hotbar.cycle(-scroll.delta.y.signum() as i32);
    }
}

fn update_target(world: Res<VoxelWorld>, cam: Query<&GlobalTransform, With<PlayerCamera>>, mut target: ResMut<Target>) {
    target.0 = cam.single().ok().and_then(|g| raycast(&*world, g.translation(), g.forward().as_vec3(), REACH));
}

#[allow(clippy::too_many_arguments)]
fn block_actions(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    capture: Res<InputCapture>,
    target: Res<Target>,
    hotbar: Res<Hotbar>,
    mut world: ResMut<VoxelWorld>,
    player: Query<(&Player, &Transform)>,
    mut cooldown: ResMut<ActionCooldown>,
    mut assets: ResMut<ParticleAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    particles: Query<(), With<Particle>>,
) {
    cooldown.0 = (cooldown.0 - time.delta_secs()).max(0.0);
    let Some(hit) = target.0 else { return };
    let Ok((p, pt)) = player.single() else { return };
    // The click that captures the mouse must not also break a block.
    if !capture.0 || !p.active {
        return;
    }
    let left = mouse.just_pressed(MouseButton::Left) || (mouse.pressed(MouseButton::Left) && cooldown.0 == 0.0);
    let right = mouse.just_pressed(MouseButton::Right) || (mouse.pressed(MouseButton::Right) && cooldown.0 == 0.0);

    if left {
        let id = world.block_at(hit.block);
        if id.is_breakable() && world.set_block(hit.block, BlockId::AIR) {
            cooldown.0 = REPEAT_SECS;
            if particles.iter().count() < MAX_PARTICLES {
                spawn_debris(&mut commands, &mut assets, &mut meshes, &mut materials, hit.block, id);
            }
        }
    } else if right && hit.normal != IVec3::ZERO {
        let at = hit.place_pos();
        let body = Body::player(pt.translation);
        let free = world.block_at(at).is_replaceable();
        if free
            && !body.intersects_block(at)
            && (0..crate::world::CHUNK_H as i32).contains(&at.y)
            && world.set_block(at, hotbar.current())
        {
            cooldown.0 = REPEAT_SECS;
        }
    }
}

fn draw_highlight(target: Res<Target>, mut gizmos: Gizmos) {
    if let Some(hit) = target.0 {
        let center = hit.block.as_vec3() + Vec3::splat(0.5);
        gizmos.cube(
            Transform::from_translation(center).with_scale(Vec3::splat(1.004)),
            Color::srgba(0.0, 0.0, 0.0, 0.85),
        );
    }
}

fn spawn_debris(
    commands: &mut Commands,
    assets: &mut ParticleAssets,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    block: IVec3,
    id: BlockId,
) {
    let mesh = assets.mesh.get_or_insert_with(|| meshes.add(Cuboid::new(0.12, 0.12, 0.12))).clone();
    let material = assets
        .materials
        .entry(id.0)
        .or_insert_with(|| {
            let [r, g, b] = id.base_color();
            materials.add(StandardMaterial { base_color: Color::srgb(r, g, b), unlit: true, ..default() })
        })
        .clone();
    // Cheap deterministic scatter based on the block position.
    let mut s = (block.x as u32).wrapping_mul(73856093)
        ^ (block.y as u32).wrapping_mul(19349663)
        ^ (block.z as u32).wrapping_mul(83492791);
    let mut rnd = move || {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (s >> 8) as f32 / (1u32 << 24) as f32
    };
    for _ in 0..10 {
        let offset = Vec3::new(rnd(), rnd(), rnd());
        let vel = Vec3::new(rnd() - 0.5, rnd() * 0.8 + 0.2, rnd() - 0.5) * 4.0;
        commands.spawn((
            Particle { vel, life: 0.6 + rnd() * 0.4 },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(block.as_vec3() + offset),
        ));
    }
}

fn update_particles(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Particle, &mut Transform)>) {
    let dt = time.delta_secs();
    for (e, mut p, mut t) in &mut q {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        p.vel.y -= 18.0 * dt;
        t.translation += p.vel * dt;
        t.scale = Vec3::splat((p.life * 2.0).min(1.0));
    }
}
