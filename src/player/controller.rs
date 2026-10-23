//! First-person controller: mouse look, acceleration-based movement, gravity,
//! jumping, sprinting (double-tap W or Ctrl), sneaking (Shift, no falling off
//! edges), swimming and a creative fly mode. Collision is delegated to
//! `physics::move_body`.

use super::LoadedLevel;
use crate::physics::collision::{Body, in_water, move_body, standing_on};
use crate::settings::GameSettings;
use crate::world::block::BlockId;
use crate::world::generation::{Biome, SEA_LEVEL, TerrainGenerator};
use crate::world::streaming::{ChunkAnchor, Generator};
use crate::world::{BlockQuery, VoxelWorld};
use bevy::camera::Projection;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

pub const EYE_HEIGHT: f32 = 1.62;
pub const SNEAK_EYE_HEIGHT: f32 = 1.27;
pub const WALK_SPEED: f32 = 4.3;
pub const SPRINT_SPEED: f32 = 5.6;
pub const SNEAK_SPEED: f32 = 1.3;
const FLY_SPEED: f32 = 10.9;
const GROUND_ACCEL: f32 = 60.0;
const GROUND_FRICTION: f32 = 45.0;
const AIR_ACCEL: f32 = 14.0;
const GRAVITY: f32 = 32.0;
const TERMINAL_VELOCITY: f32 = 78.0;
const JUMP_SPEED: f32 = 9.2;
/// Radians per pixel of mouse movement at 100 % sensitivity.
const BASE_SENSITIVITY: f32 = 0.0022;
const PHYSICS_STEP: f32 = 1.0 / 120.0;
/// A sneaking player may step down at most this far (a slab), anything deeper counts as an edge.
const SNEAK_STEP_DOWN: f32 = 0.6;
/// Two W presses within this many seconds start a sprint.
const DOUBLE_TAP_SECS: f32 = 0.3;
/// Field-of-view multiplier while sprinting.
pub const SPRINT_FOV_SCALE: f32 = 1.12;

#[derive(Component)]
pub struct Player {
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub in_water: bool,
    pub flying: bool,
    pub sprinting: bool,
    pub sneaking: bool,
    /// Current camera height above the feet; eases between standing and sneaking.
    pub eye_height: f32,
    /// Current FOV multiplier; eases towards [`SPRINT_FOV_SCALE`] while sprinting.
    pub fov_scale: f32,
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

/// Tracks the sprint "latch": once started by a double-tap or Ctrl it lasts until the player
/// stops moving forward, sneaks, or runs into something.
#[derive(Resource, Default, Debug)]
pub struct SprintState {
    last_forward_tap: Option<f32>,
    pub latched: bool,
    /// The previous physics step ran into a wall while sprinting.
    blocked: bool,
}

impl SprintState {
    /// `now` is in seconds. Returns whether the player is sprinting this frame.
    pub fn update(
        &mut self,
        now: f32,
        forward_pressed: bool,
        forward_held: bool,
        ctrl_held: bool,
        sneaking: bool,
        blocked: bool,
    ) -> bool {
        if forward_pressed {
            if self.last_forward_tap.is_some_and(|t| now - t <= DOUBLE_TAP_SECS) {
                self.latched = true;
            }
            self.last_forward_tap = Some(now);
        }
        if forward_held && ctrl_held {
            self.latched = true;
        }
        if !forward_held || sneaking || blocked {
            self.latched = false;
        }
        self.latched
    }
}

pub struct ControllerPlugin;

impl Plugin for ControllerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InputCapture>()
            .init_resource::<SprintState>()
            .add_systems(Startup, spawn_player)
            .add_systems(Update, (resolve_spawn, mouse_look, toggle_fly, player_physics, sync_camera).chain());
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

fn spawn_player(
    mut commands: Commands,
    loaded: Res<LoadedLevel>,
    settings: Res<GameSettings>,
    generator: Res<Generator>,
) {
    let (pos, yaw, pitch, flying, fresh) = match loaded.0.as_ref().filter(|l| l.spawned) {
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
                sprinting: false,
                sneaking: false,
                eye_height: EYE_HEIGHT,
                fov_scale: 1.0,
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
                    fov: settings.fov.to_radians(),
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

/// Places the player on solid ground once the chunk below exists.
fn resolve_spawn(world: Res<VoxelWorld>, mut q: Query<(&mut Player, &mut Transform)>) {
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
}

fn mouse_look(
    motion: Res<AccumulatedMouseMotion>,
    capture: Res<InputCapture>,
    settings: Res<GameSettings>,
    mut q: Query<(&mut Player, &mut Transform), Without<PlayerCamera>>,
) {
    let Ok((mut p, mut t)) = q.single_mut() else { return };
    if capture.0 {
        let k = BASE_SENSITIVITY * settings.mouse_sensitivity;
        p.yaw -= motion.delta.x * k;
        p.pitch = (p.pitch - motion.delta.y * k).clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
    }
    t.rotation = Quat::from_rotation_y(p.yaw);
}

/// Eases the eye height (sneaking) and field of view (sprinting), then applies pitch.
fn sync_camera(
    time: Res<Time>,
    settings: Res<GameSettings>,
    mut q: Query<&mut Player>,
    mut cam: Query<(&mut Transform, &mut Projection), With<PlayerCamera>>,
) {
    let (Ok(mut p), Ok((mut t, mut projection))) = (q.single_mut(), cam.single_mut()) else { return };
    let k = 1.0 - (-time.delta_secs() * 12.0).exp();
    let eye_target = if p.sneaking { SNEAK_EYE_HEIGHT } else { EYE_HEIGHT };
    p.eye_height += (eye_target - p.eye_height) * k;
    let fov_target = if p.sprinting { SPRINT_FOV_SCALE } else { 1.0 };
    p.fov_scale += (fov_target - p.fov_scale) * k;
    t.rotation = Quat::from_rotation_x(p.pitch);
    t.translation.y = p.eye_height;
    if let Projection::Perspective(persp) = &mut *projection {
        let fov = (settings.fov * p.fov_scale).to_radians().min(2.6);
        if (persp.fov - fov).abs() > 1e-5 {
            persp.fov = fov;
        }
    }
}

fn toggle_fly(keys: Res<ButtonInput<KeyCode>>, capture: Res<InputCapture>, mut q: Query<&mut Player>) {
    if capture.0
        && keys.just_pressed(KeyCode::KeyF)
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
    /// Descend while flying.
    pub descend: bool,
    pub sprint: bool,
    /// Walking slowly without falling off ledges (ignored while flying or swimming).
    pub sneak: bool,
}

/// Outcome of one physics step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StepResult {
    pub on_ground: bool,
    pub swimming: bool,
    /// Ran into a wall on a horizontal axis.
    pub hit_horizontal: bool,
    /// The sneak edge guard held the player back this step.
    pub edge_guarded: bool,
}

/// Cancels horizontal motion that would carry a sneaking player's feet off every support, so
/// they stop at the very edge of a block instead of falling (Minecraft's "safe walk").
fn sneak_clamp<W: BlockQuery>(world: &W, body: &Body, delta: Vec3) -> Vec3 {
    let mut out = delta;
    let mut pos = body.pos;
    for axis in [0usize, 2] {
        let mut step = out[axis];
        while step != 0.0 {
            let mut probe = pos;
            probe[axis] += step;
            if standing_on(world, &Body { pos: probe, ..*body }, SNEAK_STEP_DOWN) {
                break;
            }
            step = if step.abs() < 0.05 { 0.0 } else { step - step.signum() * 0.05 };
        }
        out[axis] = step;
        pos[axis] += step;
    }
    out
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
) -> StepResult {
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    let wish = (right * input.wish.x + forward * input.wish.y).normalize_or_zero();
    let wish2 = Vec2::new(wish.x, wish.z);

    let body = Body::player(*pos);
    let swimming = !flying && in_water(world, &body);
    let on_ground_probe = standing_on(world, &body, 0.02);
    let sneaking = input.sneak && !flying && !swimming;

    let mut h = Vec2::new(vel.x, vel.z);
    if flying {
        let speed = if input.sprint { FLY_SPEED * 1.8 } else { FLY_SPEED };
        h = approach(h, wish2 * speed, 60.0 * dt);
        let vertical = (input.jump as i32 - input.descend as i32) as f32 * speed * 0.75;
        vel.y += (vertical - vel.y).clamp(-50.0 * dt, 50.0 * dt);
    } else {
        let base = if sneaking {
            SNEAK_SPEED
        } else if input.sprint && input.wish.y > 0.0 {
            SPRINT_SPEED
        } else {
            WALK_SPEED
        };
        let speed = base * if swimming { 0.55 } else { 1.0 };
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

    let mut delta = *vel * dt;
    let mut edge_guarded = false;
    if sneaking && on_ground_probe {
        let safe = sneak_clamp(world, &body, delta);
        edge_guarded = safe != delta;
        if safe.x != delta.x {
            vel.x = 0.0;
        }
        if safe.z != delta.z {
            vel.z = 0.0;
        }
        delta = safe;
    }

    let res = move_body(world, &body, delta);
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
    StepResult { on_ground: res.on_ground, swimming, hit_horizontal: res.hit_x || res.hit_z, edge_guarded }
}

fn player_physics(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<InputCapture>,
    world: Res<VoxelWorld>,
    mut sprint: ResMut<SprintState>,
    mut q: Query<(&mut Player, &mut Transform)>,
) {
    let Ok((mut p, mut t)) = q.single_mut() else { return };
    if !p.active {
        return;
    }
    let mut input = MoveInput::default();
    let mut sprinting = false;
    if capture.0 {
        let axis = |pos: KeyCode, neg: KeyCode| keys.pressed(pos) as i32 as f32 - keys.pressed(neg) as i32 as f32;
        input.wish = Vec2::new(axis(KeyCode::KeyD, KeyCode::KeyA), axis(KeyCode::KeyW, KeyCode::KeyS));
        input.jump = keys.pressed(KeyCode::Space);
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        input.descend = shift;
        input.sneak = shift && !p.flying;
        if p.flying {
            input.sprint = ctrl;
            sprint.latched = false;
        } else {
            // Running into a wall ends the sprint (the previous step reported the collision).
            let blocked = std::mem::take(&mut sprint.blocked);
            sprinting = sprint.update(
                time.elapsed_secs(),
                keys.just_pressed(KeyCode::KeyW),
                keys.pressed(KeyCode::KeyW),
                ctrl,
                input.sneak,
                blocked,
            );
            input.sprint = sprinting;
        }
    } else {
        sprint.latched = false;
    }
    let dt = time.delta_secs().min(0.1);
    let steps = (dt / PHYSICS_STEP).ceil().max(1.0) as u32;
    let step_dt = dt / steps as f32;
    let (mut pos, mut vel) = (t.translation, p.vel);
    let mut last = StepResult::default();
    for _ in 0..steps {
        last = step_player(&*world, &mut pos, &mut vel, p.yaw, p.flying, input, step_dt);
    }
    // Safety net: never let the player fall out of the world.
    if pos.y < -64.0 {
        pos.y = 200.0;
        vel = Vec3::ZERO;
    }
    t.translation = pos;
    p.vel = vel;
    sprint.blocked = sprinting && last.hit_horizontal;
    p.on_ground = last.on_ground;
    p.in_water = last.swimming;
    p.sprinting = (sprinting || (p.flying && input.sprint)) && Vec2::new(vel.x, vel.z).length() > 0.5;
    p.sneaking = input.sneak && !last.swimming;
    if p.flying && last.on_ground {
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
    fn sprinting_is_faster_and_sneaking_slower_than_walking() {
        let speed_with = |input: MoveInput| {
            let (mut pos, mut vel) = (Vec3::new(0.5, 1.0, 0.5), Vec3::ZERO);
            run(&mut pos, &mut vel, input, 1.0);
            Vec2::new(vel.x, vel.z).length()
        };
        let fwd = MoveInput { wish: Vec2::Y, ..default() };
        assert!((speed_with(MoveInput { sprint: true, ..fwd }) - SPRINT_SPEED).abs() < 0.05);
        assert!((speed_with(MoveInput { sneak: true, ..fwd }) - SNEAK_SPEED).abs() < 0.05);
        // Sprinting only helps when moving forward.
        let back = MoveInput { wish: -Vec2::Y, sprint: true, ..default() };
        assert!((speed_with(back) - WALK_SPEED).abs() < 0.05);
        // Sneaking beats sprinting if both are requested.
        assert!((speed_with(MoveInput { sprint: true, sneak: true, ..fwd }) - SNEAK_SPEED).abs() < 0.05);
    }

    /// Solid ground for x < 0, a bottomless void for x >= 0.
    struct Ledge;
    impl BlockQuery for Ledge {
        fn block(&self, p: IVec3) -> Option<BlockId> {
            Some(if p.y <= 0 && p.x < 0 { BlockId::STONE } else { BlockId::AIR })
        }
    }

    #[test]
    fn sneaking_stops_at_the_edge_but_walking_falls_off() {
        let start = Vec3::new(-0.5, 1.0, 0.5);
        let east = MoveInput { wish: Vec2::X, ..default() };

        let (mut pos, mut vel) = (start, Vec3::ZERO);
        for _ in 0..(4.0 / PHYSICS_STEP) as usize {
            step_player(&Ledge, &mut pos, &mut vel, 0.0, false, MoveInput { sneak: true, ..east }, PHYSICS_STEP);
        }
        assert!((pos.y - 1.0).abs() < 0.01, "sneaking player fell: y = {}", pos.y);
        assert!(pos.x > 0.15, "should have crept right up to the edge, x = {}", pos.x);
        assert!(pos.x <= 0.3 + 1e-3, "feet must keep some support, x = {}", pos.x);

        let (mut pos, mut vel) = (start, Vec3::ZERO);
        for _ in 0..(3.0 / PHYSICS_STEP) as usize {
            step_player(&Ledge, &mut pos, &mut vel, 0.0, false, east, PHYSICS_STEP);
        }
        assert!(pos.y < -5.0, "walking player should have fallen, y = {}", pos.y);
    }

    #[test]
    fn sneak_guard_does_not_trap_the_player_on_a_ledge() {
        // Walking back from the edge must work immediately.
        let (mut pos, mut vel) = (Vec3::new(0.25, 1.0, 0.5), Vec3::ZERO);
        let west = MoveInput { wish: -Vec2::X, sneak: true, ..default() };
        for _ in 0..120 {
            step_player(&Ledge, &mut pos, &mut vel, 0.0, false, west, PHYSICS_STEP);
        }
        assert!(pos.x < 0.0, "x = {}", pos.x);
    }

    #[test]
    fn sprint_starts_on_double_tap_or_ctrl_and_stops_when_released_or_blocked() {
        let mut s = SprintState::default();
        // A single tap is just walking.
        assert!(!s.update(0.0, true, true, false, false, false));
        assert!(!s.update(0.1, false, false, false, false, false));
        // Second tap within the window sprints; holding W keeps it going.
        assert!(s.update(0.2, true, true, false, false, false));
        assert!(s.update(0.5, false, true, false, false, false));
        // Releasing W ends it.
        assert!(!s.update(0.6, false, false, false, false, false));
        // Too slow for a double tap.
        assert!(!s.update(2.0, true, true, false, false, false));
        assert!(!s.update(2.5, true, true, false, false, false));
        // Ctrl + W sprints and keeps going after Ctrl is released.
        let mut s = SprintState::default();
        assert!(s.update(0.0, true, true, true, false, false));
        assert!(s.update(0.2, false, true, false, false, false));
        // Hitting a wall or sneaking ends it.
        assert!(!s.update(0.3, false, true, false, false, true));
        assert!(s.update(0.4, false, true, true, false, false));
        assert!(!s.update(0.5, false, true, true, true, false));
    }

    #[test]
    fn spawn_column_is_dry_land() {
        let g = TerrainGenerator::new(31337);
        let c = find_spawn_column(&g);
        assert!(g.column(c.x, c.y).height > SEA_LEVEL);
    }
}
