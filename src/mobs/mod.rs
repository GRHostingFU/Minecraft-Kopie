//! Creatures: pigs, cows and sheep wander about, villagers stay near their village and zombies
//! hunt the player at night and in the dark. Everything is built from coloured boxes (see
//! `models`) and steered by a small state machine per mob.

pub mod models;

use crate::items::{Item, ItemStack};
use crate::physics::collision::{Body, in_water, move_body};
use crate::player::controller::Player;
use crate::rendering::sky::TimeOfDay;
use crate::session::{AppState, GameEntity, GameMode};
use crate::survival::drops::DropSpawner;
use crate::survival::{DamagePlayer, Experience};
use crate::world::block::BlockId;
use crate::world::streaming::Generator;
use crate::world::{BlockQuery, VoxelWorld};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use models::{LimbSpec, ModelSpec};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MobKind {
    Pig,
    Cow,
    Sheep,
    Zombie,
    Villager,
}

impl MobKind {
    pub const ALL: [MobKind; 5] = [MobKind::Pig, MobKind::Cow, MobKind::Sheep, MobKind::Zombie, MobKind::Villager];

    /// `(half width, height)` of the hit box.
    pub fn size(self) -> (f32, f32) {
        match self {
            MobKind::Pig => (0.45, 0.9),
            MobKind::Cow => (0.45, 1.4),
            MobKind::Sheep => (0.45, 1.3),
            MobKind::Zombie => (0.3, 1.95),
            MobKind::Villager => (0.3, 1.85),
        }
    }

    pub fn max_health(self) -> f32 {
        match self {
            MobKind::Pig | MobKind::Cow => 10.0,
            MobKind::Sheep => 8.0,
            MobKind::Zombie | MobKind::Villager => 20.0,
        }
    }

    pub fn hostile(self) -> bool {
        self == MobKind::Zombie
    }

    pub fn walk_speed(self) -> f32 {
        match self {
            MobKind::Pig => 2.0,
            MobKind::Cow => 1.8,
            MobKind::Sheep => 1.9,
            MobKind::Zombie => 2.4,
            MobKind::Villager => 1.7,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            MobKind::Pig => "Pig",
            MobKind::Cow => "Cow",
            MobKind::Sheep => "Sheep",
            MobKind::Zombie => "Zombie",
            MobKind::Villager => "Villager",
        }
    }

    pub fn xp(self) -> u32 {
        match self {
            MobKind::Zombie => 5,
            MobKind::Villager => 0,
            _ => 2,
        }
    }

    /// What the mob drops when killed (`roll` is a random number).
    pub fn loot(self, roll: u32) -> Option<ItemStack> {
        let n = |max: u32| 1 + (roll % max) as u8;
        match self {
            MobKind::Pig => Some(ItemStack::new(Item::RAW_PORK, n(3))),
            MobKind::Cow => Some(ItemStack::new(Item::RAW_BEEF, n(3))),
            MobKind::Sheep => Some(ItemStack::new(Item::block(BlockId::WOOL_WHITE), 1)),
            MobKind::Zombie => (roll % 3 != 0).then(|| ItemStack::new(Item::ROTTEN_FLESH, n(2))),
            MobKind::Villager => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ai {
    Idle(f32),
    Wander { dir: Vec2, left: f32 },
    Flee { dir: Vec2, left: f32 },
    Chase,
}

#[derive(Component, Debug)]
pub struct Mob {
    pub kind: MobKind,
    pub health: f32,
    pub vel: Vec3,
    pub yaw: f32,
    pub on_ground: bool,
    pub ai: Ai,
    /// Seconds of the red hurt tint left.
    pub hurt: f32,
    pub cooldown: f32,
    /// Villagers do not wander far from here.
    pub home: Vec3,
    pub phase: f32,
    pub speed: f32,
    pub age: f32,
    burn: f32,
    hurt_shown: bool,
    /// The last damage came from the player (kills by anything else drop no loot).
    by_player: bool,
}

impl Mob {
    fn new(kind: MobKind, home: Vec3, yaw: f32) -> Self {
        Self {
            kind,
            health: kind.max_health(),
            vel: Vec3::ZERO,
            yaw,
            on_ground: false,
            ai: Ai::Idle(1.0),
            hurt: 0.0,
            cooldown: 0.0,
            home,
            phase: 0.0,
            speed: 0.0,
            age: 0.0,
            burn: 0.0,
            hurt_shown: false,
            by_player: false,
        }
    }

    pub fn body(&self, pos: Vec3) -> Body {
        let (half_width, height) = self.kind.size();
        Body { pos, half_width, height }
    }
}

/// A swinging leg or arm.
#[derive(Component)]
struct Limb {
    phase: f32,
    raised: bool,
}

/// The normal material of a model part, restored after the hurt tint.
#[derive(Component)]
struct PartMaterial(Handle<StandardMaterial>);

/// A hit on a mob: damage plus a push (blocks per second).
#[derive(Message, Clone, Copy, Debug)]
pub struct DamageMob {
    pub entity: Entity,
    pub amount: f32,
    pub knockback: Vec3,
}

// -------------------------------------------------------------------------------------------------
// Pure helpers
// -------------------------------------------------------------------------------------------------

/// Distance along the ray to the box, if the ray hits it (slab test).
pub fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, f32::INFINITY);
    for a in 0..3 {
        if dir[a].abs() < 1e-8 {
            if origin[a] < min[a] || origin[a] > max[a] {
                return None;
            }
        } else {
            let (mut a0, mut a1) = ((min[a] - origin[a]) / dir[a], (max[a] - origin[a]) / dir[a]);
            if a0 > a1 {
                std::mem::swap(&mut a0, &mut a1);
            }
            t0 = t0.max(a0);
            t1 = t1.min(a1);
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0)
}

/// Small xorshift generator for the mob behaviour (not worth a dependency).
#[derive(Resource)]
pub struct MobRng(u64);

impl Default for MobRng {
    fn default() -> Self {
        Self(0x2545_F491_4F6C_DD1D)
    }
}

impl MobRng {
    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }

    pub fn unit(&mut self) -> f32 {
        (self.next_u32() & 0xFFFFFF) as f32 / 0x100_0000 as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.unit() * (hi - lo)
    }

    fn direction(&mut self) -> Vec2 {
        let a = self.unit() * std::f32::consts::TAU;
        Vec2::new(a.cos(), a.sin())
    }
}

/// Next state of a calm mob once its current one runs out.
fn next_calm(rng: &mut MobRng, home_pull: Option<Vec2>) -> Ai {
    if let Some(dir) = home_pull {
        return Ai::Wander { dir, left: rng.range(2.0, 4.0) };
    }
    if rng.unit() < 0.55 {
        Ai::Wander { dir: rng.direction(), left: rng.range(1.5, 4.5) }
    } else {
        Ai::Idle(rng.range(1.5, 5.0))
    }
}

fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    let mut d = (to - from) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    from + d * t.clamp(0.0, 1.0)
}

// -------------------------------------------------------------------------------------------------
// Assets and spawning
// -------------------------------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct MobAssets {
    /// Quantised base colour -> material (shaded by the time of day).
    palette: HashMap<[u8; 3], Handle<StandardMaterial>>,
    meshes: HashMap<[i32; 3], Handle<Mesh>>,
    hurt: Option<Handle<StandardMaterial>>,
}

fn quantize(c: [f32; 3]) -> [u8; 3] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct MobSpawner<'w, 's> {
    commands: Commands<'w, 's>,
    assets: ResMut<'w, MobAssets>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
}

impl MobSpawner<'_, '_> {
    fn material(&mut self, color: [f32; 3]) -> Handle<StandardMaterial> {
        let key = quantize(color);
        let materials = &mut self.materials;
        self.assets
            .palette
            .entry(key)
            .or_insert_with(|| {
                let [r, g, b] = key.map(|v| v as f32 / 255.0);
                materials.add(StandardMaterial { base_color: Color::srgb(r, g, b), unlit: true, ..default() })
            })
            .clone()
    }

    fn mesh(&mut self, size: Vec3) -> Handle<Mesh> {
        let key = [(size.x * 1000.0) as i32, (size.y * 1000.0) as i32, (size.z * 1000.0) as i32];
        let meshes = &mut self.meshes;
        self.assets.meshes.entry(key).or_insert_with(|| meshes.add(Cuboid::new(size.x, size.y, size.z))).clone()
    }

    /// Spawns a mob with its feet at `pos`.
    pub fn spawn(&mut self, kind: MobKind, pos: Vec3, home: Vec3, yaw: f32) -> Entity {
        if self.assets.hurt.is_none() {
            let hurt = self.materials.add(StandardMaterial { base_color: Color::srgb(0.95, 0.18, 0.12), unlit: true, ..default() });
            self.assets.hurt = Some(hurt);
        }
        let spec: ModelSpec = models::model(kind);
        let mut parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>, Transform, Option<LimbSpec>)> = Vec::new();
        for bx in &spec.boxes {
            parts.push((self.mesh(bx.size), self.material(bx.color), Transform::from_translation(bx.center), None));
        }
        for l in &spec.limbs {
            parts.push((self.mesh(l.size), self.material(l.color), Transform::from_translation(l.hip), Some(*l)));
        }
        self.commands
            .spawn((Mob::new(kind, home, yaw), GameEntity, Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)), Visibility::default()))
            .with_children(|root| {
                for (mesh, material, transform, limb) in parts {
                    match limb {
                        None => {
                            root.spawn((Mesh3d(mesh), PartMaterial(material.clone()), MeshMaterial3d(material), transform));
                        }
                        Some(l) => {
                            // The pivot sits at the hip; the box hangs below it.
                            root.spawn((Limb { phase: l.phase, raised: l.raised }, transform, Visibility::default())).with_children(|pivot| {
                                pivot.spawn((
                                    Mesh3d(mesh),
                                    PartMaterial(material.clone()),
                                    MeshMaterial3d(material),
                                    Transform::from_xyz(0.0, -l.size.y / 2.0, 0.0),
                                ));
                            });
                        }
                    }
                }
            })
            .id()
    }
}

pub struct MobsPlugin;

impl Plugin for MobsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MobAssets>()
            .init_resource::<MobRng>()
            .add_message::<DamageMob>()
            .add_systems(OnExit(AppState::InGame), |mut rng: ResMut<MobRng>| *rng = MobRng::default())
            .add_systems(
                Update,
                (spawn_mobs, mob_ai, apply_mob_damage, despawn_far, animate_limbs, tint_hurt, shade_mobs)
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

// -------------------------------------------------------------------------------------------------
// Spawning rules
// -------------------------------------------------------------------------------------------------

const MAX_PASSIVE: usize = 14;
const MAX_HOSTILE: usize = 8;
const MAX_VILLAGERS: usize = 4;
const DESPAWN_DISTANCE: f32 = 100.0;

/// A column position with room to stand: solid floor, two free blocks above, no water.
fn standable(world: &VoxelWorld, x: i32, y: i32, z: i32) -> Option<BlockId> {
    let floor = world.block(IVec3::new(x, y - 1, z))?;
    let free = |dy: i32| world.block(IVec3::new(x, y + dy, z)).is_some_and(|b| !b.is_solid() && !b.is_water());
    (floor.is_solid() && floor.is_opaque() && free(0) && free(1) && free(2)).then_some(floor)
}

#[allow(clippy::too_many_arguments)]
fn spawn_mobs(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    generator: Res<Generator>,
    tod: Res<TimeOfDay>,
    mode: Res<GameMode>,
    mut rng: ResMut<MobRng>,
    mut timer: Local<f32>,
    mut spawner: MobSpawner,
    player: Query<(&Player, &Transform)>,
    mobs: Query<(&Mob, &Transform)>,
) {
    let Ok((p, pt)) = player.single() else { return };
    if !p.active {
        return;
    }
    let passive = mobs.iter().filter(|(m, _)| !m.kind.hostile() && m.kind != MobKind::Villager).count();
    let hostile = mobs.iter().filter(|(m, _)| m.kind.hostile()).count();
    let villagers = mobs.iter().filter(|(m, _)| m.kind == MobKind::Villager).count();
    // Fill the world quickly at first, then trickle.
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = if passive + hostile < 6 { 0.35 } else { 1.8 };

    let origin = pt.translation;
    let angle = rng.unit() * std::f32::consts::TAU;
    let radius = rng.range(22.0, 46.0);
    let (x, z) = ((origin.x + angle.cos() * radius).floor() as i32, (origin.z + angle.sin() * radius).floor() as i32);

    // Villagers appear near village doors.
    if villagers < MAX_VILLAGERS && rng.unit() < 0.5 {
        let cell = crate::world::generation::villages::cell_of(origin.x as i32, origin.z as i32);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let Some(v) = generator.0.village_in_cell(cell + IVec2::new(dx, dz)) else { continue };
                let c = Vec3::new(v.center.x as f32, v.ground as f32 + 1.0, v.center.y as f32);
                if c.distance(origin) > 64.0 || mobs.iter().any(|(m, _)| m.kind == MobKind::Villager && m.home.distance(c) < 6.0) {
                    continue;
                }
                if let Some(spot) = v.door_spots.get(rng.next_u32() as usize % v.door_spots.len().max(1)) {
                    let pos = spot.as_vec3() + Vec3::new(0.5, 0.0, 0.5);
                    if standable(&world, spot.x, spot.y, spot.z).is_some() {
                        spawner.spawn(MobKind::Villager, pos, c, rng.unit() * 6.28);
                        return;
                    }
                }
            }
        }
    }

    let night = tod.daylight() < 0.35;
    let want_hostile = *mode == GameMode::Survival && hostile < MAX_HOSTILE && rng.unit() < 0.6;
    if want_hostile {
        // Surface at night, caves any time: look around the player's height for a dark floor.
        let dy = rng.range(-14.0, 6.0).round() as i32;
        let start = (origin.y as i32 + dy).clamp(5, 200);
        for y in (start - 8..=start).rev() {
            let Some(_floor) = standable(&world, x, y, z) else { continue };
            let exposed = world.sky_exposed(IVec3::new(x, y, z));
            if (exposed && night) || !exposed {
                spawner.spawn(MobKind::Zombie, Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5), origin, rng.unit() * 6.28);
            }
            return;
        }
        return;
    }
    if passive >= MAX_PASSIVE || night && rng.unit() < 0.7 {
        return;
    }
    let Some(h) = world.surface_height(x, z) else { return };
    if standable(&world, x, h + 1, z) != Some(BlockId::GRASS) {
        return;
    }
    let kind = [MobKind::Pig, MobKind::Cow, MobKind::Sheep][rng.next_u32() as usize % 3];
    // Animals come in small herds.
    let herd = 1 + (rng.next_u32() % 3) as i32;
    for i in 0..herd {
        let (hx, hz) = (x + i % 2, z + i / 2);
        if let Some(hh) = world.surface_height(hx, hz)
            && standable(&world, hx, hh + 1, hz) == Some(BlockId::GRASS)
        {
            spawner.spawn(kind, Vec3::new(hx as f32 + 0.5, hh as f32 + 1.0, hz as f32 + 0.5), origin, rng.unit() * 6.28);
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Behaviour
// -------------------------------------------------------------------------------------------------

const GRAVITY: f32 = 28.0;
const JUMP_SPEED: f32 = 8.2;
const ATTACK_RANGE: f32 = 1.7;
const SIGHT: f32 = 22.0;

#[allow(clippy::too_many_arguments)]
fn mob_ai(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    tod: Res<TimeOfDay>,
    mode: Res<GameMode>,
    mut rng: ResMut<MobRng>,
    player: Query<(&Player, &Transform), Without<Mob>>,
    mut mobs: Query<(&mut Mob, &mut Transform)>,
    mut damage: MessageWriter<DamagePlayer>,
) {
    let dt = time.delta_secs().min(0.05);
    let target = player.single().ok().filter(|(p, _)| p.active).map(|(_, t)| t.translation);
    for (mut m, mut t) in &mut mobs {
        // Mobs in unloaded terrain stand still.
        if world.block(t.translation.floor().as_ivec3()).is_none() {
            continue;
        }
        m.age += dt;
        m.hurt = (m.hurt - dt).max(0.0);
        m.cooldown = (m.cooldown - dt).max(0.0);
        let pos = t.translation;
        let kind = m.kind;
        let to_player = target.map(|p| Vec2::new(p.x - pos.x, p.z - pos.z));
        let dist = to_player.map_or(f32::INFINITY, |d| d.length());

        // --- Decide what to do.
        let mut want = Vec2::ZERO;
        let mut speed = kind.walk_speed();
        if kind.hostile() && *mode == GameMode::Survival && dist < SIGHT {
            m.ai = Ai::Chase;
            want = to_player.unwrap_or_default().normalize_or_zero();
            speed *= 1.2;
            if dist < ATTACK_RANGE && (target.unwrap_or_default().y - pos.y).abs() < 1.6 && m.cooldown == 0.0 {
                m.cooldown = 1.0;
                damage.write(DamagePlayer { amount: 3.0, knockback: Vec3::new(want.x, 0.0, want.y) * 5.0 });
            }
        } else {
            if m.ai == Ai::Chase {
                m.ai = Ai::Idle(1.0);
            }
            // Villagers drift back towards their village.
            let home_pull = (kind == MobKind::Villager)
                .then(|| Vec2::new(m.home.x - pos.x, m.home.z - pos.z))
                .filter(|d| d.length() > 12.0)
                .map(|d| d.normalize_or_zero());
            m.ai = match m.ai {
                Ai::Idle(left) if left - dt <= 0.0 => next_calm(&mut rng, home_pull),
                Ai::Idle(left) => Ai::Idle(left - dt),
                Ai::Wander { dir, left } if left - dt <= 0.0 => {
                    let _ = dir;
                    Ai::Idle(rng.range(1.0, 4.0))
                }
                Ai::Wander { dir, left } => Ai::Wander { dir, left: left - dt },
                Ai::Flee { dir, left } if left - dt <= 0.0 => {
                    let _ = dir;
                    Ai::Idle(1.0)
                }
                Ai::Flee { dir, left } => Ai::Flee { dir, left: left - dt },
                Ai::Chase => Ai::Idle(1.0),
            };
            match m.ai {
                Ai::Wander { dir, .. } => want = dir,
                Ai::Flee { dir, .. } => {
                    want = dir;
                    speed *= 1.9;
                }
                _ => {}
            }
        }

        // --- Move.
        let body = m.body(pos);
        let swimming = in_water(&*world, &body);
        let target_v = want * speed;
        let accel = if m.on_ground { 14.0 } else { 4.0 };
        let cur = Vec2::new(m.vel.x, m.vel.z);
        let delta = target_v - cur;
        let step = accel * dt;
        let new = if delta.length() <= step { target_v } else { cur + delta / delta.length() * step };
        m.vel.x = new.x;
        m.vel.z = new.y;
        if swimming {
            m.vel.y += (2.2 - m.vel.y).clamp(-20.0 * dt, 20.0 * dt);
        } else {
            m.vel.y = (m.vel.y - GRAVITY * dt).max(-40.0);
        }
        let result = move_body(&*world, &body, m.vel * dt);
        t.translation = result.pos;
        m.on_ground = result.on_ground;
        if result.hit_y {
            m.vel.y = 0.0;
        }
        let blocked = result.hit_x || result.hit_z;
        if result.hit_x {
            m.vel.x = 0.0;
        }
        if result.hit_z {
            m.vel.z = 0.0;
        }
        // Step up a block, or turn around when stuck.
        if blocked && want != Vec2::ZERO {
            if m.on_ground {
                m.vel.y = JUMP_SPEED;
            }
            if let Ai::Wander { left, .. } = m.ai
                && left < 100.0
                && rng.unit() < 0.02
            {
                m.ai = Ai::Wander { dir: rng.direction(), left };
            }
        }
        // Face where the mob is going.
        let horizontal = Vec2::new(m.vel.x, m.vel.z);
        m.speed = horizontal.length();
        if want != Vec2::ZERO {
            let yaw = (-want.x).atan2(-want.y);
            m.yaw = lerp_angle(m.yaw, yaw, 8.0 * dt);
        }
        m.phase += m.speed * dt * 2.6;
        t.rotation = Quat::from_rotation_y(m.yaw);

        // --- Zombies burn in the sun.
        if kind.hostile() && tod.daylight() > 0.8 && world.sky_exposed(result.pos.floor().as_ivec3()) {
            m.burn += dt;
            if m.burn >= 1.0 {
                m.burn = 0.0;
                m.health -= 2.0;
                m.hurt = 0.3;
            }
        } else {
            m.burn = 0.0;
        }
    }
}

fn apply_mob_damage(
    mut commands: Commands,
    mut hits: MessageReader<DamageMob>,
    mut mobs: Query<(Entity, &mut Mob, &Transform)>,
    mut rng: ResMut<MobRng>,
    mut xp: ResMut<Experience>,
    mut drops: DropSpawner,
) {
    for hit in hits.read() {
        let Ok((_, mut m, _)) = mobs.get_mut(hit.entity) else { continue };
        m.health -= hit.amount;
        m.hurt = 0.4;
        m.by_player = true;
        m.vel += hit.knockback + Vec3::Y * 3.5;
        if !m.kind.hostile() {
            let away = Vec2::new(hit.knockback.x, hit.knockback.z).normalize_or_zero();
            m.ai = Ai::Flee { dir: if away == Vec2::ZERO { rng.direction() } else { away }, left: 3.5 };
        }
    }
    // Remove everything that died (from hits or from the sun); only kills by the player give loot.
    for (e, m, t) in &mobs {
        if m.health > 0.0 {
            continue;
        }
        if m.by_player {
            let roll = rng.next_u32();
            if let Some(stack) = m.kind.loot(roll) {
                drops.spawn_scattered(stack, t.translation + Vec3::Y * 0.5, roll);
            }
            xp.add(m.kind.xp());
        }
        commands.entity(e).despawn();
    }
}

/// Mobs far behind the player disappear (they would only cost time).
fn despawn_far(mut commands: Commands, player: Query<&Transform, (With<Player>, Without<Mob>)>, mobs: Query<(Entity, &Transform), With<Mob>>) {
    let Ok(p) = player.single() else { return };
    for (e, t) in &mobs {
        if t.translation.distance(p.translation) > DESPAWN_DISTANCE {
            commands.entity(e).despawn();
        }
    }
}

/// Legs swing with the distance walked; zombies hold their arms out in front.
fn animate_limbs(mobs: Query<&Mob>, mut limbs: Query<(&Limb, &mut Transform, &ChildOf)>) {
    for (limb, mut t, parent) in &mut limbs {
        let Ok(m) = mobs.get(parent.parent()) else { continue };
        let swing = (m.speed / 3.0).min(1.0);
        let wave = (m.phase + limb.phase).sin();
        t.rotation = if limb.raised {
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 + wave * 0.08 * (1.0 + swing))
        } else {
            Quat::from_rotation_x(wave * 0.85 * swing)
        };
    }
}

/// A hit mob flashes red for a moment.
fn tint_hurt(
    assets: Res<MobAssets>,
    mut mobs: Query<(Entity, &mut Mob)>,
    children: Query<&Children>,
    mut parts: Query<(&PartMaterial, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(hurt) = &assets.hurt else { return };
    for (e, mut m) in &mut mobs {
        let want = m.hurt > 0.0;
        if want == m.hurt_shown {
            continue;
        }
        m.hurt_shown = want;
        for d in children.iter_descendants(e) {
            if let Ok((normal, mut material)) = parts.get_mut(d) {
                material.0 = if want { hurt.clone() } else { normal.0.clone() };
            }
        }
    }
}

/// The unlit mob materials follow the time of day.
fn shade_mobs(tod: Res<TimeOfDay>, assets: Res<MobAssets>, mut materials: ResMut<Assets<StandardMaterial>>, mut last: Local<f32>) {
    let l = 0.3 + 0.7 * tod.daylight();
    if (l - *last).abs() < 0.01 && !assets.is_changed() {
        return;
    }
    *last = l;
    for (key, handle) in &assets.palette {
        if let Some(mut m) = materials.get_mut(handle) {
            let [r, g, b] = key.map(|v| v as f32 / 255.0 * l);
            m.base_color = Color::srgb(r, g, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_boxes_from_outside_and_misses_beside_them() {
        let (lo, hi) = (Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 1.0, 0.5));
        let hit = ray_aabb(Vec3::new(0.0, 0.5, -5.0), Vec3::Z, lo, hi).unwrap();
        assert!((hit - 4.5).abs() < 1e-5);
        assert!(ray_aabb(Vec3::new(2.0, 0.5, -5.0), Vec3::Z, lo, hi).is_none());
        // Behind the origin is no hit.
        assert!(ray_aabb(Vec3::new(0.0, 0.5, 5.0), Vec3::Z, lo, hi).is_none());
        // Starting inside reports 0.
        assert_eq!(ray_aabb(Vec3::new(0.0, 0.5, 0.0), Vec3::Y, lo, hi), Some(0.0));
        // Parallel to a slab and outside it.
        assert!(ray_aabb(Vec3::new(0.0, 2.0, -5.0), Vec3::Z, lo, hi).is_none());
        // Diagonal.
        assert!(ray_aabb(Vec3::new(-3.0, 3.0, -3.0), Vec3::new(1.0, -1.0, 1.0).normalize(), lo, hi).is_some());
    }

    #[test]
    fn rng_is_deterministic_and_spread() {
        let mut a = MobRng::default();
        let mut b = MobRng::default();
        let xs: Vec<f32> = (0..1000).map(|_| a.unit()).collect();
        assert!(xs.iter().zip((0..1000).map(|_| b.unit())).all(|(x, y)| *x == y));
        assert!(xs.iter().all(|x| (0.0..1.0).contains(x)));
        let mean = xs.iter().sum::<f32>() / xs.len() as f32;
        assert!((mean - 0.5).abs() < 0.06, "mean {mean}");
        assert!((a.range(2.0, 3.0) - 2.5).abs() <= 0.5);
    }

    #[test]
    fn angles_turn_the_short_way() {
        use std::f32::consts::PI;
        assert!((lerp_angle(0.1, 0.5, 1.0) - 0.5).abs() < 1e-5);
        // From just below +pi to just above -pi is a small step, not a full turn back.
        let r = lerp_angle(PI - 0.1, -PI + 0.1, 0.5);
        assert!((r - PI).abs() < 1e-4, "{r}");
    }

    #[test]
    fn mobs_drop_what_they_should() {
        assert_eq!(MobKind::Pig.loot(0).unwrap().item, Item::RAW_PORK);
        assert_eq!(MobKind::Cow.loot(1).unwrap().item, Item::RAW_BEEF);
        assert!(MobKind::Villager.loot(5).is_none());
        assert!(MobKind::Zombie.loot(0).is_none() && MobKind::Zombie.loot(1).is_some());
        assert!(MobKind::Pig.loot(7).unwrap().count <= 3);
        assert!(MobKind::Zombie.hostile() && !MobKind::Pig.hostile());
        for k in MobKind::ALL {
            let (w, h) = k.size();
            assert!(w > 0.0 && h > 0.0 && k.max_health() > 0.0 && k.walk_speed() > 0.0);
        }
    }

    #[test]
    fn calm_mobs_alternate_between_wandering_and_standing_and_go_home() {
        let mut rng = MobRng::default();
        let (mut idle, mut wander) = (0, 0);
        for _ in 0..200 {
            match next_calm(&mut rng, None) {
                Ai::Idle(t) => {
                    idle += 1;
                    assert!((1.5..5.0).contains(&t));
                }
                Ai::Wander { dir, left } => {
                    wander += 1;
                    assert!((dir.length() - 1.0).abs() < 1e-4 && left > 1.0);
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(idle > 40 && wander > 40, "{idle}/{wander}");
        let home = Vec2::new(0.0, 1.0);
        assert!(matches!(next_calm(&mut rng, Some(home)), Ai::Wander { dir, .. } if dir == home));
    }
}
