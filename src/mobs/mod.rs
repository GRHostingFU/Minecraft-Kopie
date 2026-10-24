//! Hostile mobs: the creeper and the skeleton.
//!
//! * [`pathfinding`] — A* routes over the voxel grid;
//! * [`ai`] — what each mob decides every frame;
//! * [`explosion`] — block destruction and damage of explosions;
//! * [`spawning`] — where mobs may appear and when the sun burns them;
//! * [`model`] — their procedural bodies and materials;
//! * this module — components, physics, damage, arrows, spawning and rendering systems.

pub mod ai;
pub mod combat;
pub mod explosion;
pub mod model;
pub mod pathfinding;
pub mod spawning;

use crate::audio::{PlaySound, Sound};
use crate::effects::{Burst, ExplosionFlash, ParticleAssets, spawn_debris};
use crate::physics::collision::{Body, move_body, standing_on};
use crate::physics::fluids::body_contact;
use crate::physics::raycast::{ray_box, raycast};
use crate::player::controller::Player;
use crate::player::health::{Damage, DamageKind};
use crate::redstone::PressesPlates;
use crate::rendering::sky::TimeOfDay;
use crate::world::{BlockQuery, VoxelWorld};
use bevy::prelude::*;
use model::{MobMaterials, PartKind};

pub use combat::MobTarget;

/// Most hostile mobs alive at once.
pub const MAX_MOBS: usize = 10;
/// Mobs farther than this from the player are removed.
pub const DESPAWN_DISTANCE: f32 = 80.0;
/// Mobs only think and move while they are this close (chunks beyond are not loaded anyway).
pub const ACTIVE_DISTANCE: f32 = 70.0;
const GRAVITY: f32 = 28.0;
const JUMP_SPEED: f32 = 8.4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MobKind {
    Creeper,
    Skeleton,
}

impl MobKind {
    /// Hitbox width and height in blocks.
    pub fn size(self) -> (f32, f32) {
        match self {
            MobKind::Creeper => (0.6, 1.7),
            MobKind::Skeleton => (0.6, 1.95),
        }
    }

    pub fn max_health(self) -> f32 {
        20.0
    }

    /// Walking speed in blocks per second.
    pub fn speed(self) -> f32 {
        match self {
            MobKind::Creeper => 3.1,
            MobKind::Skeleton => 3.4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            MobKind::Creeper => "Creeper",
            MobKind::Skeleton => "Skeleton",
        }
    }
}

#[derive(Component, Debug)]
pub struct Mob {
    pub kind: MobKind,
    pub health: f32,
    /// Seconds of red flash left after a hit.
    pub hurt: f32,
    /// Seconds left on fire.
    pub burning: f32,
    pub vel: Vec3,
    pub on_ground: bool,
    pub in_water: bool,
    /// Body direction: forward is `(-sin yaw, -cos yaw)` like the player's.
    pub yaw: f32,
    /// What the head follows, if anything.
    pub look_at: Option<Vec3>,
    pub walk_phase: f32,
    /// 0 standing .. 1 walking at full speed (smoothed).
    pub walk_amount: f32,
    pub age: f32,
    fire_timer: f32,
}

impl Mob {
    pub fn new(kind: MobKind, yaw: f32) -> Self {
        Self {
            kind,
            health: kind.max_health(),
            hurt: 0.0,
            burning: 0.0,
            vel: Vec3::ZERO,
            on_ground: false,
            in_water: false,
            yaw,
            look_at: None,
            walk_phase: 0.0,
            walk_amount: 0.0,
            age: 0.0,
            fire_timer: 0.0,
        }
    }

    pub fn body(&self, feet: Vec3) -> Body {
        let (w, h) = self.kind.size();
        Body { pos: feet, half_width: w * 0.5, height: h }
    }
}

#[derive(Component, Debug)]
pub struct Creeper {
    /// Seconds of fuse burnt so far.
    pub fuse: f32,
    pub igniting: bool,
}

pub const CREEPER_FUSE_SECS: f32 = 1.5;
/// Distance at which a creeper starts its fuse and the distance at which it gives up.
pub const CREEPER_IGNITE_DISTANCE: f32 = 3.0;
pub const CREEPER_ABORT_DISTANCE: f32 = 7.0;
pub const CREEPER_POWER: f32 = 3.0;

#[derive(Component, Debug, Default)]
pub struct Skeleton {
    pub cooldown: f32,
    pub strafe: f32,
    pub strafe_timer: f32,
    /// 0..1: how far the bow is drawn (animation).
    pub draw: f32,
}

/// Movement intent and route of a mob.
#[derive(Component, Debug, Default)]
pub struct Brain {
    pub path: Vec<IVec3>,
    pub path_index: usize,
    /// The cell the current path leads to.
    pub goal: Option<IVec3>,
    pub repath: f32,
    pub wander_wait: f32,
    pub wander_goal: Option<IVec3>,
    pub stuck: f32,
    pub last_pos: Vec3,
    /// Desired horizontal velocity in blocks per second.
    pub wish: Vec2,
    /// Face this direction (horizontal) instead of the walking direction.
    pub face: Option<Vec2>,
}

/// An arrow in flight or stuck in a block.
#[derive(Component, Debug)]
pub struct Arrow {
    pub vel: Vec3,
    pub age: f32,
    pub stuck: Option<f32>,
}

#[derive(Message, Clone, Copy, Debug)]
pub struct MobDamage {
    pub target: Entity,
    pub amount: f32,
    pub knockback: Vec3,
}

#[derive(Message, Clone, Copy, Debug)]
pub struct Explode {
    pub center: Vec3,
    pub power: f32,
}

/// Search budget: path searches allowed per frame.
#[derive(Resource)]
pub struct PathBudget(pub u32);

#[derive(Resource)]
struct SpawnState {
    timer: f32,
    rng: u32,
}

#[derive(Resource)]
pub struct MobAssets {
    materials: MobMaterials,
    arrow_mesh: Handle<Mesh>,
    creeper: Vec<(model::PartSpec, Handle<Mesh>)>,
    skeleton: Vec<(model::PartSpec, Handle<Mesh>)>,
}

/// A visible piece of a mob; animated from its owner's state.
#[derive(Component)]
struct MobPart {
    owner: Entity,
    kind: PartKind,
    pivot: Vec3,
}

pub struct MobPlugin;

impl Plugin for MobPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MobTarget>()
            .insert_resource(PathBudget(3))
            .insert_resource(SpawnState { timer: 4.0, rng: 0x5EED_1234 })
            .add_message::<MobDamage>()
            .add_message::<Explode>()
            .add_systems(PreStartup, setup_assets)
            .add_systems(
                Update,
                (
                    reset_budget,
                    ai::creeper_brain,
                    ai::skeleton_brain,
                    mob_physics,
                    mob_environment,
                    apply_mob_damage,
                    arrows,
                    handle_explosions,
                    spawn_mobs,
                    despawn_far_mobs,
                    debug_spawn_keys,
                    animate_mobs,
                )
                    .chain(),
            );
    }
}

fn setup_assets(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let mats = MobMaterials::new(&mut images, &mut materials);
    let mut build = |parts: Vec<model::PartSpec>| -> Vec<(model::PartSpec, Handle<Mesh>)> {
        parts.into_iter().map(|p| {
            let mesh = meshes.add(model::boxes_mesh(&p.boxes));
            (p, mesh)
        }).collect()
    };
    let creeper = build(model::creeper_parts());
    let skeleton = build(model::skeleton_parts());
    commands.insert_resource(MobAssets { materials: mats, arrow_mesh: meshes.add(model::arrow_mesh()), creeper, skeleton });
}

fn reset_budget(mut budget: ResMut<PathBudget>) {
    budget.0 = 3;
}

/// Spawns a mob with its parts. The returned entity is the root.
pub fn spawn_mob(commands: &mut Commands, assets: &MobAssets, kind: MobKind, feet: Vec3, yaw: f32) -> Entity {
    let parts = match kind {
        MobKind::Creeper => &assets.creeper,
        MobKind::Skeleton => &assets.skeleton,
    };
    let mut root = commands.spawn((
        Mob::new(kind, yaw),
        Brain { last_pos: feet, ..default() },
        PressesPlates,
        Transform::from_translation(feet).with_rotation(Quat::from_rotation_y(yaw)),
        Visibility::default(),
    ));
    match kind {
        MobKind::Creeper => {
            root.insert(Creeper { fuse: 0.0, igniting: false });
        }
        MobKind::Skeleton => {
            root.insert(Skeleton { cooldown: 1.5, strafe: 1.0, ..default() });
        }
    }
    let owner = root.id();
    root.with_children(|c| {
        for (spec, mesh) in parts {
            c.spawn((
                MobPart { owner, kind: spec.kind, pivot: spec.pivot },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(assets.materials.normal.clone()),
                Transform::from_translation(spec.pivot),
            ));
        }
    });
    owner
}

// ---------------------------------------------------------------------------------------------
// Physics
// ---------------------------------------------------------------------------------------------

fn approach(current: Vec2, target: Vec2, max_delta: f32) -> Vec2 {
    let d = target - current;
    let len = d.length();
    if len <= max_delta { target } else { current + d / len * max_delta }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MobStep {
    pub on_ground: bool,
    pub in_water: bool,
    pub in_lava: bool,
    pub hit_horizontal: bool,
}

/// One physics step of a walking mob: gravity, acceleration towards `wish` (blocks/s), water
/// buoyancy and currents, auto-jump over one-block steps. Pure with respect to Bevy.
pub fn step_mob<W: BlockQuery>(world: &W, pos: &mut Vec3, vel: &mut Vec3, size: (f32, f32), wish: Vec2, dt: f32) -> MobStep {
    let body = Body { pos: *pos, half_width: size.0 * 0.5, height: size.1 };
    let contact = body_contact(world, *pos, body.half_width, body.height);
    let swimming = contact.any();
    let grounded = standing_on(world, &body, 0.02);

    let mut target = wish * if swimming { 0.6 } else { 1.0 };
    if swimming {
        let push = contact.flow * if contact.lava { 0.5 } else { 1.5 };
        target += Vec2::new(push.x, push.z);
    }
    let accel = if grounded { 36.0 } else if swimming { 12.0 } else { 10.0 };
    let h = approach(Vec2::new(vel.x, vel.z), target, accel * dt);
    vel.x = h.x;
    vel.z = h.y;

    if swimming {
        // Float up slowly (lava: sink), falling water drags down.
        let want = if contact.lava { -0.8 } else { 1.4 } + contact.flow.y * 3.0;
        vel.y += (want - vel.y).clamp(-18.0 * dt, 18.0 * dt);
    } else {
        vel.y = (vel.y - GRAVITY * dt).max(-60.0);
    }

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
    let hit_horizontal = res.hit_x || res.hit_z;
    if hit_horizontal && wish != Vec2::ZERO {
        if grounded {
            vel.y = JUMP_SPEED;
        } else if swimming && !contact.lava {
            vel.y = vel.y.max(3.5);
        }
    }
    MobStep { on_ground: res.on_ground || (grounded && vel.y <= 0.0), in_water: contact.water, in_lava: contact.lava, hit_horizontal }
}

fn turn_towards(current: f32, target: f32, max_step: f32) -> f32 {
    let diff = (target - current + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    current + diff.clamp(-max_step, max_step)
}

/// Yaw that makes a body look along the horizontal direction `d` (x, z).
pub fn yaw_of(d: Vec2) -> f32 {
    (-d.x).atan2(-d.y)
}

fn mob_physics(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    player: Query<&Transform, (With<Player>, Without<Mob>)>,
    mut mobs: Query<(&mut Mob, &Brain, &mut Transform), Without<Player>>,
) {
    let dt = time.delta_secs().min(0.05);
    if dt == 0.0 {
        return;
    }
    let ppos = player.single().map(|t| t.translation).ok();
    for (mut mob, brain, mut t) in &mut mobs {
        if ppos.is_some_and(|p| p.distance(t.translation) > ACTIVE_DISTANCE) {
            continue;
        }
        mob.age += dt;
        mob.hurt = (mob.hurt - dt).max(0.0);
        let size = mob.kind.size();
        let (mut pos, mut vel) = (t.translation, mob.vel);
        // Substeps keep fast knockback from tunnelling.
        let steps = (dt / (1.0 / 90.0)).ceil().max(1.0) as u32;
        let mut last = MobStep::default();
        for _ in 0..steps {
            last = step_mob(&*world, &mut pos, &mut vel, size, brain.wish, dt / steps as f32);
        }
        // Never fall out of the world.
        if pos.y < -40.0 {
            mob.health = 0.0;
        }
        let moved = Vec2::new(pos.x - t.translation.x, pos.z - t.translation.z);
        t.translation = pos;
        mob.vel = vel;
        mob.on_ground = last.on_ground;
        mob.in_water = last.in_water;
        // Face the walking direction (or the requested one) and swing the legs.
        let face = brain.face.or_else(|| (brain.wish.length_squared() > 0.01).then_some(brain.wish));
        if let Some(f) = face {
            mob.yaw = turn_towards(mob.yaw, yaw_of(f), 9.0 * dt);
        }
        let speed = moved.length() / dt;
        let target = (speed / mob.kind.speed()).min(1.0);
        mob.walk_amount += (target - mob.walk_amount) * (1.0 - (-dt * 10.0).exp());
        mob.walk_phase += moved.length() * 5.5;
        t.rotation = Quat::from_rotation_y(mob.yaw);
    }
}

/// Fire, lava and the sun.
fn mob_environment(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    tod: Res<TimeOfDay>,
    mut mobs: Query<(Entity, &mut Mob, &Transform)>,
    mut damage: MessageWriter<MobDamage>,
    mut bursts: MessageWriter<Burst>,
    mut sounds: MessageWriter<PlaySound>,
) {
    let dt = time.delta_secs();
    if dt == 0.0 {
        return;
    }
    let daylight = tod.daylight();
    for (e, mut mob, t) in &mut mobs {
        let (_, h) = mob.kind.size();
        if mob.kind == MobKind::Skeleton && spawning::burns_in_sun(&*world, t.translation, h, daylight) {
            mob.burning = mob.burning.max(4.0);
        }
        let contact = body_contact(&*world, t.translation, mob.kind.size().0 * 0.5, h);
        if contact.lava {
            mob.burning = mob.burning.max(6.0);
            if (mob.age * 2.0).fract() < dt * 2.0 {
                damage.write(MobDamage { target: e, amount: 4.0, knockback: Vec3::ZERO });
            }
        }
        if contact.water {
            mob.burning = 0.0;
        }
        if mob.burning > 0.0 {
            mob.burning -= dt;
            mob.fire_timer -= dt;
            if mob.fire_timer <= 0.0 {
                mob.fire_timer = 0.5;
                damage.write(MobDamage { target: e, amount: 1.0, knockback: Vec3::ZERO });
                bursts.write(Burst {
                    center: t.translation + Vec3::Y * h * 0.6,
                    color: [1.0, 0.55, 0.1],
                    count: 3,
                    speed: 1.5,
                    life: 0.5,
                });
                sounds.write(PlaySound::at(Sound::Fire, t.translation).with_volume(0.4));
            }
        } else {
            mob.fire_timer = 0.0;
        }
    }
}

fn apply_mob_damage(
    mut commands: Commands,
    mut hits: MessageReader<MobDamage>,
    mut mobs: Query<(&mut Mob, &Transform)>,
    mut bursts: MessageWriter<Burst>,
    mut sounds: MessageWriter<PlaySound>,
) {
    for hit in hits.read() {
        let Ok((mut mob, t)) = mobs.get_mut(hit.target) else { continue };
        if mob.health <= 0.0 {
            continue;
        }
        mob.health -= hit.amount;
        mob.vel += hit.knockback;
        let (_, h) = mob.kind.size();
        let centre = t.translation + Vec3::Y * h * 0.5;
        if hit.amount >= 1.5 {
            mob.hurt = 0.4;
            sounds.write(PlaySound::at(Sound::MobHurt, centre));
        }
        if mob.health <= 0.0 {
            sounds.write(PlaySound::at(Sound::Death, centre));
            let color = if mob.kind == MobKind::Creeper { [0.32, 0.6, 0.25] } else { [0.85, 0.85, 0.78] };
            bursts.write(Burst { center: centre, color, count: 18, speed: 3.5, life: 0.8 });
            bursts.write(Burst { center: centre, color: [0.4, 0.4, 0.42], count: 8, speed: 1.5, life: 1.0 });
            commands.entity(hit.target).despawn();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Arrows
// ---------------------------------------------------------------------------------------------

pub const ARROW_SPEED: f32 = 24.0;
const ARROW_GRAVITY: f32 = 16.0;
const ARROW_LIFETIME: f32 = 10.0;
const ARROW_STICK_SECS: f32 = 4.0;

pub fn spawn_arrow(commands: &mut Commands, assets: &MobAssets, origin: Vec3, vel: Vec3) {
    commands.spawn((
        Arrow { vel, age: 0.0, stuck: None },
        Mesh3d(assets.arrow_mesh.clone()),
        MeshMaterial3d(assets.materials.arrow.clone()),
        Transform::from_translation(origin).looking_to(vel, Vec3::Y),
    ));
}

#[allow(clippy::too_many_arguments)]
fn arrows(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<VoxelWorld>,
    player: Query<(&Player, &Transform), Without<Arrow>>,
    mut arrows: Query<(Entity, &mut Arrow, &mut Transform)>,
    mut damage: MessageWriter<Damage>,
    mut sounds: MessageWriter<PlaySound>,
    mut bursts: MessageWriter<Burst>,
) {
    let dt = time.delta_secs();
    if dt == 0.0 {
        return;
    }
    let target = player.single().ok().filter(|(p, _)| p.active);
    for (e, mut arrow, mut t) in &mut arrows {
        arrow.age += dt;
        if let Some(stuck) = arrow.stuck.as_mut() {
            *stuck -= dt;
            if *stuck <= 0.0 {
                commands.entity(e).despawn();
            }
            continue;
        }
        if arrow.age > ARROW_LIFETIME {
            commands.entity(e).despawn();
            continue;
        }
        arrow.vel.y -= ARROW_GRAVITY * dt;
        let from = t.translation;
        let delta = arrow.vel * dt;
        let len = delta.length();
        let dir = delta / len.max(1e-6);
        // Closest of: player hitbox, block.
        let block_hit = raycast(&*world, from, dir, len);
        let player_hit = target.and_then(|(_, pt)| {
            let (lo, hi) = (
                pt.translation - Vec3::new(Body::PLAYER_HALF_WIDTH, 0.0, Body::PLAYER_HALF_WIDTH) - Vec3::splat(0.1),
                pt.translation + Vec3::new(Body::PLAYER_HALF_WIDTH, Body::PLAYER_HEIGHT, Body::PLAYER_HALF_WIDTH) + Vec3::splat(0.1),
            );
            ray_box(from, dir, lo, hi).filter(|(d, _)| *d <= len).map(|(d, _)| d)
        });
        match (player_hit, block_hit) {
            (Some(d), b) if b.is_none_or(|b| d <= b.distance) => {
                let amount = (2.0 + arrow.vel.length() / ARROW_SPEED * 2.0).round();
                damage.write(Damage { amount, kind: DamageKind::Arrow, impulse: dir * 2.0 });
                bursts.write(Burst { center: from + dir * d, color: [0.6, 0.1, 0.1], count: 5, speed: 2.0, life: 0.5 });
                commands.entity(e).despawn();
            }
            (_, Some(hit)) => {
                t.translation = from + dir * (hit.distance - 0.1).max(0.0);
                arrow.stuck = Some(ARROW_STICK_SECS);
                sounds.write(PlaySound::at(Sound::ArrowHit, t.translation));
            }
            _ => {
                t.translation = from + delta;
                t.look_to(arrow.vel, Vec3::Y);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Explosions
// ---------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_explosions(
    mut commands: Commands,
    mut blasts: MessageReader<Explode>,
    mut world: ResMut<VoxelWorld>,
    player: Query<(&Player, &Transform)>,
    mobs: Query<(Entity, &Mob, &Transform)>,
    mut particle_assets: ResMut<ParticleAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut player_damage: MessageWriter<Damage>,
    mut mob_damage: MessageWriter<MobDamage>,
    mut sounds: MessageWriter<PlaySound>,
    mut bursts: MessageWriter<Burst>,
    mut flashes: MessageWriter<ExplosionFlash>,
    mut serial: Local<u32>,
) {
    for blast in blasts.read() {
        *serial = serial.wrapping_add(1);
        let seed = (blast.center.x as i32 as u32).wrapping_mul(73856093) ^ (blast.center.z as i32 as u32).wrapping_mul(83492791) ^ serial.wrapping_mul(2654435761);
        // Damage is computed against the intact world, so walls shelter bodies before they break.
        if let Ok((p, pt)) = player.single()
            && p.active
            && let Some(fx) = explosion::blast_effect(&*world, blast.center, blast.power, pt.translation, 0.6, 1.8)
        {
            player_damage.write(Damage { amount: fx.damage, kind: DamageKind::Explosion, impulse: fx.knockback });
        }
        for (e, mob, t) in &mobs {
            let (w, h) = mob.kind.size();
            if let Some(fx) = explosion::blast_effect(&*world, blast.center, blast.power, t.translation, w, h) {
                mob_damage.write(MobDamage { target: e, amount: fx.damage, knockback: fx.knockback });
            }
        }
        let removed = explosion::detonate(&mut *world, blast.center, blast.power, seed);
        for (i, cell) in removed.iter().enumerate() {
            if i % 4 == 0 && i / 4 < 24 {
                spawn_debris(&mut commands, &mut particle_assets, &mut meshes, &mut materials, *cell, crate::world::BlockId::DIRT);
            }
        }
        sounds.write(PlaySound::at(Sound::Explosion, blast.center).with_volume(1.2));
        flashes.write(ExplosionFlash { center: blast.center, radius: blast.power * 1.3 });
        bursts.write(Burst { center: blast.center, color: [0.25, 0.25, 0.27], count: 40, speed: 5.0, life: 1.4 });
        bursts.write(Burst { center: blast.center, color: [1.0, 0.6, 0.15], count: 30, speed: 7.0, life: 0.7 });
    }
}

// ---------------------------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------------------------

fn next_rand(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 / (1u32 << 24) as f32
}

/// Finds a place for a new hostile mob around `player`: out of sight distance but near enough to
/// matter, standing on solid ground in the dark.
pub fn find_spawn_cell<W: BlockQuery>(w: &W, player: Vec3, daylight: f32, rng: &mut u32) -> Option<IVec3> {
    for _ in 0..8 {
        let angle = next_rand(rng) * std::f32::consts::TAU;
        let dist = 22.0 + next_rand(rng) * 18.0;
        let x = (player.x + angle.cos() * dist).floor() as i32;
        let z = (player.z + angle.sin() * dist).floor() as i32;
        // Scan the column near the player's height for a place to stand.
        let top = player.y as i32 + 14;
        let bottom = (player.y as i32 - 24).max(1);
        for y in (bottom..=top).rev() {
            let cell = IVec3::new(x, y, z);
            if pathfinding::standable(w, cell, 2) && spawning::is_dark(w, cell, daylight) {
                return Some(cell);
            }
        }
    }
    None
}

fn spawn_mobs(
    mut commands: Commands,
    time: Res<Time>,
    tod: Res<TimeOfDay>,
    world: Res<VoxelWorld>,
    assets: Option<Res<MobAssets>>,
    player: Query<(&Player, &Transform), Without<Mob>>,
    mobs: Query<(), With<Mob>>,
    mut state: ResMut<SpawnState>,
) {
    let Some(assets) = assets else { return };
    let Ok((p, pt)) = player.single() else { return };
    if !p.active {
        return;
    }
    state.timer -= time.delta_secs();
    if state.timer > 0.0 {
        return;
    }
    state.timer = 2.0 + next_rand(&mut state.rng) * 2.5;
    if mobs.iter().count() >= MAX_MOBS {
        return;
    }
    let Some(cell) = find_spawn_cell(&*world, pt.translation, tod.daylight(), &mut state.rng) else { return };
    let kind = if next_rand(&mut state.rng) < 0.45 { MobKind::Creeper } else { MobKind::Skeleton };
    let yaw = next_rand(&mut state.rng) * std::f32::consts::TAU;
    spawn_mob(&mut commands, &assets, kind, Vec3::new(cell.x as f32 + 0.5, cell.y as f32, cell.z as f32 + 0.5), yaw);
}

fn despawn_far_mobs(
    mut commands: Commands,
    player: Query<&Transform, (With<Player>, Without<Mob>)>,
    mobs: Query<(Entity, &Transform), With<Mob>>,
) {
    let Ok(pt) = player.single() else { return };
    for (e, t) in &mobs {
        if t.translation.distance(pt.translation) > DESPAWN_DISTANCE {
            commands.entity(e).despawn();
        }
    }
}

/// F6 / F7 place a creeper / skeleton a few blocks in front of the player (for testing).
fn debug_spawn_keys(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    assets: Option<Res<MobAssets>>,
    player: Query<(&Player, &Transform), Without<Mob>>,
) {
    let Some(assets) = assets else { return };
    let Ok((p, t)) = player.single() else { return };
    let kind = if keys.just_pressed(KeyCode::F6) {
        MobKind::Creeper
    } else if keys.just_pressed(KeyCode::F7) {
        MobKind::Skeleton
    } else {
        return;
    };
    let forward = Vec3::new(-p.yaw.sin(), 0.0, -p.yaw.cos());
    let at = t.translation + forward * 7.0 + Vec3::Y * 1.0;
    spawn_mob(&mut commands, &assets, kind, at, p.yaw + std::f32::consts::PI);
}

// ---------------------------------------------------------------------------------------------
// Visuals
// ---------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn animate_mobs(
    tod: Res<TimeOfDay>,
    assets: Option<Res<MobAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut mobs: Query<(&Mob, Option<&Creeper>, Option<&Skeleton>, &mut Transform), Without<MobPart>>,
    mut parts: Query<(&MobPart, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(assets) = assets else { return };
    // Mobs are dim at night.
    let b = model::brightness(tod.daylight());
    for (handle, tint) in [
        (&assets.materials.normal, Color::srgb(b, b, b)),
        (&assets.materials.hurt, Color::srgb(b, b * 0.35, b * 0.35)),
        (&assets.materials.burning, Color::srgb(b, b * 0.65, b * 0.3)),
        (&assets.materials.arrow, Color::srgb(b, b, b)),
    ] {
        if let Some(mut m) = materials.get_mut(handle)
            && m.base_color != tint
        {
            m.base_color = tint;
        }
    }
    // A creeper swells and wobbles while its fuse burns.
    for (_, creeper, _, mut root_t) in &mut mobs {
        if let Some(c) = creeper {
            let p = (c.fuse / CREEPER_FUSE_SECS).clamp(0.0, 1.0);
            let wobble = (c.fuse * 30.0).sin() * 0.03 * p;
            let s = 1.0 + p * 0.3 + wobble;
            root_t.scale = Vec3::new(s, 1.0 + p * 0.12, s);
        }
    }
    for (part, mut t, mut material) in &mut parts {
        let Ok((mob, creeper, skeleton, root_t)) = mobs.get(part.owner) else { continue };
        let swing = mob.walk_phase.sin() * 0.85 * mob.walk_amount;
        let mut rotation = Quat::IDENTITY;
        match (part.kind, mob.kind) {
            (PartKind::Leg(i), MobKind::Creeper) => {
                // Diagonal pairs move together.
                let sign = if i == 0 || i == 3 { 1.0 } else { -1.0 };
                rotation = Quat::from_rotation_x(swing * sign);
            }
            (PartKind::Leg(i), MobKind::Skeleton) => {
                rotation = Quat::from_rotation_x(if i == 0 { swing } else { -swing });
            }
            (PartKind::ArmLeft, MobKind::Skeleton) => {
                let aim = skeleton.map_or(0.0, |s| s.draw);
                rotation = Quat::from_rotation_x(-swing * (1.0 - aim) - 1.45 * aim);
            }
            (PartKind::ArmRight, MobKind::Skeleton) => {
                let aim = skeleton.map_or(0.0, |s| s.draw);
                rotation = Quat::from_rotation_x(swing * (1.0 - aim) - 1.55 * aim);
            }
            (PartKind::Head, _) => {
                if let Some(target) = mob.look_at {
                    let to = target - (root_t.translation + part.pivot);
                    let want_yaw = yaw_of(Vec2::new(to.x, to.z));
                    let rel = (want_yaw - mob.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                    let pitch = (to.y.atan2(Vec2::new(to.x, to.z).length())).clamp(-0.6, 0.6);
                    rotation = Quat::from_rotation_y(rel.clamp(-1.1, 1.1)) * Quat::from_rotation_x(pitch);
                }
            }
            _ => {}
        }
        t.rotation = rotation;
        t.translation = part.pivot;

        let flash = creeper.is_some_and(|c| c.igniting && ((c.fuse * 7.0) as i32) % 2 == 0);
        let want = if flash {
            &assets.materials.flash
        } else if mob.hurt > 0.0 {
            &assets.materials.hurt
        } else if mob.burning > 0.0 {
            &assets.materials.burning
        } else {
            &assets.materials.normal
        };
        if material.0 != *want {
            material.0 = want.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::fluids::sim::tests::Map;
    use crate::world::block::BlockId;

    fn run(w: &Map, pos: &mut Vec3, vel: &mut Vec3, wish: Vec2, secs: f32) -> MobStep {
        let mut last = MobStep::default();
        for _ in 0..(secs * 90.0) as usize {
            last = step_mob(w, pos, vel, MobKind::Skeleton.size(), wish, 1.0 / 90.0);
        }
        last
    }

    #[test]
    fn mobs_fall_land_and_stand_still() {
        let w = Map::flat();
        let (mut pos, mut vel) = (Vec3::new(0.5, 6.0, 0.5), Vec3::ZERO);
        let s = run(&w, &mut pos, &mut vel, Vec2::ZERO, 2.0);
        assert!(s.on_ground);
        assert!((pos.y - 1.0).abs() < 0.01, "y = {}", pos.y);
    }

    #[test]
    fn mobs_walk_at_the_commanded_speed_and_stop_at_walls() {
        let mut w = Map::flat();
        for y in 1..=3 {
            w.put(IVec3::new(6, y, 0), BlockId::STONE);
        }
        let (mut pos, mut vel) = (Vec3::new(0.5, 1.0, 0.5), Vec3::ZERO);
        run(&w, &mut pos, &mut vel, Vec2::new(3.0, 0.0), 0.5);
        assert!(pos.x > 1.5 && pos.x < 2.6, "x = {}", pos.x);
        run(&w, &mut pos, &mut vel, Vec2::new(3.0, 0.0), 4.0);
        assert!(pos.x < 6.0 - 0.3 + 1e-3, "stopped by the 3-high wall, x = {}", pos.x);
    }

    #[test]
    fn mobs_jump_up_single_steps_but_not_two_block_walls() {
        let mut w = Map::flat();
        w.put(IVec3::new(4, 1, 0), BlockId::COBBLE);
        let (mut pos, mut vel) = (Vec3::new(0.5, 1.0, 0.5), Vec3::ZERO);
        run(&w, &mut pos, &mut vel, Vec2::new(3.0, 0.0), 3.0);
        assert!(pos.x > 5.0 && (pos.y - 2.0).abs() < 0.02, "climbed onto the step: {pos}");

        let mut w = Map::flat();
        for y in 1..=2 {
            w.put(IVec3::new(4, y, 0), BlockId::COBBLE);
        }
        let (mut pos, mut vel) = (Vec3::new(0.5, 1.0, 0.5), Vec3::ZERO);
        run(&w, &mut pos, &mut vel, Vec2::new(3.0, 0.0), 3.0);
        assert!(pos.x < 4.0, "a two-high wall stops them: {pos}");
    }

    #[test]
    fn mobs_float_in_water_and_sink_in_lava() {
        let mut w = Map::flat();
        for x in -3..=3 {
            for z in -3..=3 {
                for y in 1..=4 {
                    w.put(IVec3::new(x, y, z), BlockId::WATER);
                }
            }
        }
        let (mut pos, mut vel) = (Vec3::new(0.5, 1.0, 0.5), Vec3::ZERO);
        run(&w, &mut pos, &mut vel, Vec2::ZERO, 3.0);
        assert!(pos.y > 3.0, "rises towards the surface: {pos}");
        let mut lava = Map::flat();
        for y in 1..=4 {
            lava.put(IVec3::new(0, y, 0), BlockId::LAVA);
        }
        let (mut pos, mut vel) = (Vec3::new(0.5, 4.0, 0.5), Vec3::ZERO);
        run(&lava, &mut pos, &mut vel, Vec2::ZERO, 2.0);
        assert!(pos.y < 2.0, "lava pulls down: {pos}");
    }

    #[test]
    fn yaw_matches_the_walking_direction() {
        // Forward at yaw 0 is -Z, like the player.
        assert!((yaw_of(Vec2::new(0.0, -1.0))).abs() < 1e-6);
        let yaw = yaw_of(Vec2::new(1.0, 0.0));
        let forward = Vec2::new(-yaw.sin(), -yaw.cos());
        assert!((forward - Vec2::new(1.0, 0.0)).length() < 1e-5);
        assert!((turn_towards(0.0, 3.0, 0.5) - 0.5).abs() < 1e-6);
        assert!((turn_towards(3.0, -3.0, 0.5) - 3.5).abs() < 1e-5, "turns the short way round");
    }

    #[test]
    fn spawn_cells_are_dark_far_from_the_player_and_on_solid_ground() {
        let w = Map::flat();
        let mut rng = 12345;
        let player = Vec3::new(0.5, 1.0, 0.5);
        assert!(find_spawn_cell(&w, player, 1.0, &mut rng).is_none(), "no spawning on open ground in daylight");
        let mut found = 0;
        for _ in 0..20 {
            let c = find_spawn_cell(&w, player, 0.0, &mut rng).expect("night spawn");
            let d = Vec2::new(c.x as f32 + 0.5 - player.x, c.z as f32 + 0.5 - player.z).length();
            assert!((21.0..41.0).contains(&d), "distance {d}");
            assert!(pathfinding::standable(&w, c, 2));
            found += 1;
        }
        assert_eq!(found, 20);
    }
}
