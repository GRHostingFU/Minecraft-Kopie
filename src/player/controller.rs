//! First-person controller: mouse look, acceleration-based movement, gravity,
//! jumping, swimming and a creative fly mode. Collision is delegated to
//! `physics::move_body`.

use crate::physics::collision::{Body, in_water, move_body};
use crate::world::block::BlockId;
use crate::world::generation::{Biome, SEA_LEVEL, TerrainGenerator};
use crate::world::storage;
use crate::world::streaming::{ChunkAnchor, Generator, WorldSettings};
use crate::world::{BlockQuery, VoxelWorld};
use bevy::camera::Projection;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use std::f32::consts::FRAC_PI_2;

pub const EYE_HEIGHT: f32 = 1.62;
const WALK_SPEED: f32 = 4.3;
const SPRINT_SPEED: f32 = 5.6;
const FLY_SPEED: f32 = 10.9;
const GROUND_ACCEL: f32 = 60.0;
const GROUND_FRICTION: f32 = 45.0;
const AIR_ACCEL: f32 = 14.0;
const GRAVITY: f32 = 32.0;
const TERMINAL_VELOCITY: f32 = 78.0;
const JUMP_SPEED: f32 = 9.2;
const MOUSE_SENSITIVITY: f32 = 0.0022;
const PHYSICS_STEP: f32 = 1.0 / 120.0;

#[derive(Component)]
pub struct Player {
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub in_water: bool,
    pub flying: bool,
    /// Physics runs only once the terrain below the player exists.
    pub active: bool,
    /// Fresh world: still looking for a safe spot once the first chunk is ready.
    pub needs_spawn_search: bool,
}

#[derive(Component)]
pub struct PlayerCamera;

/// True while the mouse is captured by the game window.
#[derive(Resource, Default)]
pub struct InputCapture(pub bool);

pub struct ControllerPlugin;

impl Plugin for ControllerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InputCapture>().add_systems(Startup, spawn_player).add_systems(
            Update,
            (cursor_capture, resolve_spawn, mouse_look, toggle_fly, player_physics, sync_camera).chain(),
        );
    }
}

/// Finds a dry, flat-ish column near the origin to start in.
pub fn find_spawn_column(gen_: &TerrainGenerator) -> IVec2 {
    for ring in 0..64 {
        let r = ring * 12;
        for step in 0..(1 + ring * 4).min(32) {
            let a = step as f32 / (1 + ring * 4).min(32) as f32 * std::f32::consts::TAU;
            let (x, z) = ((a.cos() * r as f32) as i32, (a.sin() * r as f32) as i32);
            let c = gen_.column(x, z);
            if c.height > SEA_LEVEL + 2 && matches!(c.biome, Biome::Plains | Biome::Forest | Biome::Desert) {
                return IVec2::new(x, z);
            }
        }
    }
    IVec2::ZERO
}

fn spawn_player(mut commands: Commands, settings: Res<WorldSettings>, generator: Res<Generator>) {
    let level = storage::load_level(&settings.save_dir);
    let (pos, yaw, pitch, flying, fresh) = match &level {
        Some(l) => (Vec3::from(l.player_pos), l.yaw, l.pitch, l.flying, false),
        None => {
            let c = find_spawn_column(&generator.0);
            (Vec3::new(c.x as f32 + 0.5, 200.0, c.y as f32 + 0.5), 0.0, 0.0, false, true)
        }
    };
    commands
        .spawn((
            Player {
                vel: Vec3::ZERO,
                yaw,
                pitch,
                on_ground: false,
                in_water: false,
                flying,
                active: false,
                needs_spawn_search: fresh,
            },
            ChunkAnchor,
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
        ))
        .with_children(|p| {
            p.spawn((
                PlayerCamera,
                Camera3d::default(),
                Projection::Perspective(PerspectiveProjection {
                    fov: 80f32.to_radians(),
                    near: 0.05,
                    far: 2500.0,
                    ..default()
                }),
                bevy::core_pipeline::tonemapping::Tonemapping::None,
                bevy::core_pipeline::tonemapping::DebandDither::Disabled,
                Transform::from_xyz(0.0, EYE_HEIGHT, 0.0).with_rotation(Quat::from_rotation_x(pitch)),
            ));
        });
}

fn cursor_capture(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut capture: ResMut<InputCapture>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(mut cursor) = cursor.single_mut() else { return };
    if keys.just_pressed(KeyCode::Escape) {
        capture.0 = false;
    } else if !capture.0 && mouse.just_pressed(MouseButton::Left) {
        capture.0 = true;
    }
    let want = if capture.0 { CursorGrabMode::Locked } else { CursorGrabMode::None };
    if cursor.grab_mode != want {
        cursor.grab_mode = want;
        cursor.visible = !capture.0;
    }
}

/// Places the player on solid ground once the chunk below exists.
fn resolve_spawn(
    world: Res<VoxelWorld>,
    mut q: Query<(&mut Player, &mut Transform)>,
    mut capture: ResMut<InputCapture>,
) {
    let Ok((mut player, mut t)) = q.single_mut() else { return };
    if player.active {
        return;
    }
    let (px, pz) = (t.translation.x.floor() as i32, t.translation.z.floor() as i32);
    let chunk = IVec2::new(px.div_euclid(16), pz.div_euclid(16));
    // Wait for the 3x3 neighbourhood so the player never starts on half-built terrain.
    let ready = (-1..=1).all(|dz| (-1..=1).all(|dx| world.is_loaded(chunk + IVec2::new(dx, dz))));
    if !ready {
        return;
    }
    if player.needs_spawn_search {
        // Skip columns that are covered by trees or lakes.
        'search: for r in 0..8i32 {
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dz.abs()) != r {
                        continue;
                    }
                    let (x, z) = (px + dx, pz + dz);
                    let Some(h) = world.surface_height(x, z) else { continue };
                    let top = world.block(IVec3::new(x, h, z)).unwrap_or(BlockId::AIR);
                    let clear_above =
                        (1..=3).all(|k| world.block(IVec3::new(x, h + k, z)).is_some_and(|b| !b.is_solid()));
                    if clear_above && h > SEA_LEVEL && !matches!(top, BlockId::LOG | BlockId::LEAVES) {
                        t.translation = Vec3::new(x as f32 + 0.5, h as f32 + 1.0 + 1e-3, z as f32 + 0.5);
                        break 'search;
                    }
                }
            }
        }
        player.needs_spawn_search = false;
    }
    player.vel = Vec3::ZERO;
    player.active = true;
    capture.0 = true;
}

fn mouse_look(
    motion: Res<AccumulatedMouseMotion>,
    capture: Res<InputCapture>,
    mut q: Query<(&mut Player, &mut Transform), Without<PlayerCamera>>,
) {
    let Ok((mut p, mut t)) = q.single_mut() else { return };
    if capture.0 {
        p.yaw -= motion.delta.x * MOUSE_SENSITIVITY;
        p.pitch = (p.pitch - motion.delta.y * MOUSE_SENSITIVITY).clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
    }
    t.rotation = Quat::from_rotation_y(p.yaw);
}

fn sync_camera(q: Query<&Player>, mut cam: Query<&mut Transform, With<PlayerCamera>>) {
    if let (Ok(p), Ok(mut t)) = (q.single(), cam.single_mut()) {
        t.rotation = Quat::from_rotation_x(p.pitch);
    }
}

fn toggle_fly(keys: Res<ButtonInput<KeyCode>>, mut q: Query<&mut Player>) {
    if keys.just_pressed(KeyCode::KeyF)
        && let Ok(mut p) = q.single_mut()
    {
        p.flying = !p.flying;
        p.vel = Vec3::ZERO;
    }
}

fn approach(current: Vec2, target: Vec2, max_delta: f32) -> Vec2 {
    let d = target - current;
    let len = d.length();
    if len <= max_delta { target } else { current + d / len * max_delta }
}

/// Input for one physics step.
#[derive(Clone, Copy, Default)]
pub struct MoveInput {
    pub wish: Vec2,
    pub jump: bool,
    pub descend: bool,
    pub sprint: bool,
}

/// One fixed physics step of the player. Pure with respect to Bevy so it can be tested.
pub fn step_player<W: BlockQuery>(
    world: &W,
    pos: &mut Vec3,
    vel: &mut Vec3,
    yaw: f32,
    flying: bool,
    input: MoveInput,
    dt: f32,
) -> (bool, bool) {
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    let wish = (right * input.wish.x + forward * input.wish.y).normalize_or_zero();
    let wish2 = Vec2::new(wish.x, wish.z);

    let body = Body::player(*pos);
    let swimming = !flying && in_water(world, &body);
    let on_ground_probe = crate::physics::collision::standing_on(world, &body, 0.02);

    let mut h = Vec2::new(vel.x, vel.z);
    if flying {
        let speed = if input.sprint { FLY_SPEED * 1.8 } else { FLY_SPEED };
        h = approach(h, wish2 * speed, 60.0 * dt);
        let vertical = (input.jump as i32 - input.descend as i32) as f32 * speed * 0.75;
        vel.y += (vertical - vel.y).clamp(-50.0 * dt, 50.0 * dt);
    } else {
        let speed = if input.sprint { SPRINT_SPEED } else { WALK_SPEED } * if swimming { 0.55 } else { 1.0 };
        let target = wish2 * speed;
        if on_ground_probe {
            let accel = if wish2 == Vec2::ZERO { GROUND_FRICTION } else { GROUND_ACCEL };
            h = approach(h, target, accel * dt);
        } else {
            h = approach(h, target, AIR_ACCEL * dt);
        }
        if swimming {
            vel.y -= 6.0 * dt;
            vel.y = vel.y.clamp(-3.0, 3.0);
            if input.jump {
                vel.y = (vel.y + 18.0 * dt).min(3.2);
            }
        } else {
            vel.y = (vel.y - GRAVITY * dt).max(-TERMINAL_VELOCITY);
            if input.jump && on_ground_probe {
                vel.y = JUMP_SPEED;
            }
        }
    }
    vel.x = h.x;
    vel.z = h.y;

    let res = move_body(world, &body, *vel * dt);
    *pos = res.pos;
    if res.hit_x {
        vel.x = 0.0;
    }
    if res.hit_z {
        vel.z = 0.0;
    }
    if res.hit_y {
        vel.y = 0.0;
    }
    (res.on_ground, swimming)
}

fn player_physics(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<InputCapture>,
    world: Res<VoxelWorld>,
    mut q: Query<(&mut Player, &mut Transform)>,
) {
    let Ok((mut p, mut t)) = q.single_mut() else { return };
    if !p.active {
        return;
    }
    let mut input = MoveInput::default();
    if capture.0 {
        let axis = |pos: KeyCode, neg: KeyCode| keys.pressed(pos) as i32 as f32 - keys.pressed(neg) as i32 as f32;
        input.wish = Vec2::new(axis(KeyCode::KeyD, KeyCode::KeyA), axis(KeyCode::KeyW, KeyCode::KeyS));
        input.jump = keys.pressed(KeyCode::Space);
        input.descend = keys.pressed(KeyCode::ShiftLeft);
        input.sprint = keys.pressed(KeyCode::ControlLeft);
    }
    let dt = time.delta_secs().min(0.1);
    let steps = (dt / PHYSICS_STEP).ceil().max(1.0) as u32;
    let step_dt = dt / steps as f32;
    let (mut pos, mut vel) = (t.translation, p.vel);
    let (mut grounded, mut wet) = (false, false);
    for _ in 0..steps {
        (grounded, wet) = step_player(&*world, &mut pos, &mut vel, p.yaw, p.flying, input, step_dt);
    }
    // Safety net: never let the player fall out of the world.
    if pos.y < -64.0 {
        pos.y = 200.0;
        vel = Vec3::ZERO;
    }
    t.translation = pos;
    p.vel = vel;
    p.on_ground = grounded;
    p.in_water = wet;
    if p.flying && grounded {
        p.flying = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::IVec3;

    struct Flat;
    impl BlockQuery for Flat {
        fn block(&self, p: IVec3) -> Option<BlockId> {
            Some(if p.y <= 0 { BlockId::STONE } else { BlockId::AIR })
        }
    }

    fn run(pos: &mut Vec3, vel: &mut Vec3, input: MoveInput, secs: f32) {
        for _ in 0..(secs / PHYSICS_STEP) as usize {
            step_player(&Flat, pos, vel, 0.0, false, input, PHYSICS_STEP);
        }
    }

    #[test]
    fn falls_then_stands_still() {
        let (mut pos, mut vel) = (Vec3::new(0.5, 10.0, 0.5), Vec3::ZERO);
        run(&mut pos, &mut vel, MoveInput::default(), 3.0);
        assert!((pos.y - 1.0).abs() < 0.01, "y = {}", pos.y);
        assert!(vel.length() < 0.01);
    }

    #[test]
    fn walking_accelerates_to_walk_speed_and_friction_stops() {
        let (mut pos, mut vel) = (Vec3::new(0.5, 1.0, 0.5), Vec3::ZERO);
        run(&mut pos, &mut vel, MoveInput { wish: Vec2::Y, ..default() }, 1.0);
        let speed = Vec2::new(vel.x, vel.z).length();
        assert!((speed - WALK_SPEED).abs() < 0.05, "speed {speed}");
        assert!(pos.z < -3.0, "forward is -Z, got {}", pos.z);
        run(&mut pos, &mut vel, MoveInput::default(), 0.5);
        assert!(Vec2::new(vel.x, vel.z).length() < 0.01);
    }

    #[test]
    fn jump_reaches_expected_height_and_lands() {
        let (mut pos, mut vel) = (Vec3::new(0.5, 1.0, 0.5), Vec3::ZERO);
        let mut apex = pos.y;
        step_player(&Flat, &mut pos, &mut vel, 0.0, false, MoveInput { jump: true, ..default() }, PHYSICS_STEP);
        for _ in 0..240 {
            step_player(&Flat, &mut pos, &mut vel, 0.0, false, MoveInput::default(), PHYSICS_STEP);
            apex = apex.max(pos.y);
        }
        let h = apex - 1.0;
        assert!((1.1..1.6).contains(&h), "jump height {h}");
        assert!((pos.y - 1.0).abs() < 0.01);
    }

    #[test]
    fn no_double_jump_in_air() {
        let (mut pos, mut vel) = (Vec3::new(0.5, 1.0, 0.5), Vec3::ZERO);
        step_player(&Flat, &mut pos, &mut vel, 0.0, false, MoveInput { jump: true, ..default() }, PHYSICS_STEP);
        for _ in 0..20 {
            step_player(&Flat, &mut pos, &mut vel, 0.0, false, MoveInput::default(), PHYSICS_STEP);
        }
        let vy = vel.y;
        step_player(&Flat, &mut pos, &mut vel, 0.0, false, MoveInput { jump: true, ..default() }, PHYSICS_STEP);
        assert!(vel.y < vy, "holding jump in mid-air must not boost");
    }

    #[test]
    fn flying_ignores_gravity() {
        let (mut pos, mut vel) = (Vec3::new(0.5, 30.0, 0.5), Vec3::ZERO);
        for _ in 0..240 {
            step_player(&Flat, &mut pos, &mut vel, 0.0, true, MoveInput::default(), PHYSICS_STEP);
        }
        assert!((pos.y - 30.0).abs() < 0.01);
    }

    #[test]
    fn spawn_column_is_dry_land() {
        let g = TerrainGenerator::new(31337);
        let c = find_spawn_column(&g);
        assert!(g.column(c.x, c.y).height > SEA_LEVEL);
    }
}
