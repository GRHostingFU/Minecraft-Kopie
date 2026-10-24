//! Short-lived visual effects: block debris, smoke / spark bursts and the flash of an explosion.

use crate::world::block::BlockId;
use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

pub const MAX_PARTICLES: usize = 600;

#[derive(Component)]
pub struct Particle {
    pub vel: Vec3,
    pub life: f32,
    pub gravity: f32,
    pub size: f32,
}

#[derive(Resource, Default)]
pub struct ParticleAssets {
    mesh: Option<Handle<Mesh>>,
    /// Keyed by the colour quantised to 8 bits per channel.
    materials: HashMap<u32, Handle<StandardMaterial>>,
    sphere: Option<Handle<Mesh>>,
}

impl ParticleAssets {
    fn material(&mut self, materials: &mut Assets<StandardMaterial>, [r, g, b]: [f32; 3]) -> Handle<StandardMaterial> {
        let key = ((r * 255.0) as u32) << 16 | ((g * 255.0) as u32) << 8 | (b * 255.0) as u32;
        self.materials
            .entry(key)
            .or_insert_with(|| materials.add(StandardMaterial { base_color: Color::srgb(r, g, b), unlit: true, ..default() }))
            .clone()
    }
}

/// Everything needed to throw block debris around (bundled to keep system signatures short).
#[derive(SystemParam)]
pub struct DebrisSpawner<'w, 's> {
    commands: Commands<'w, 's>,
    assets: ResMut<'w, ParticleAssets>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    particles: Query<'w, 's, (), With<Particle>>,
}

impl DebrisSpawner<'_, '_> {
    /// Debris of the broken `id` at `cell`, unless too many particles are alive already.
    pub fn debris(&mut self, cell: IVec3, id: BlockId) {
        if self.particles.iter().count() < MAX_PARTICLES {
            spawn_debris(&mut self.commands, &mut self.assets, &mut self.meshes, &mut self.materials, cell, id);
        }
    }
}

/// Ask for a puff of particles: sparks, smoke or whatever `color` suggests.
#[derive(Message, Clone, Copy, Debug)]
pub struct Burst {
    pub center: Vec3,
    pub color: [f32; 3],
    pub count: usize,
    pub speed: f32,
    pub life: f32,
}

/// The expanding fireball of an explosion.
#[derive(Message, Clone, Copy, Debug)]
pub struct ExplosionFlash {
    pub center: Vec3,
    pub radius: f32,
}

#[derive(Component)]
struct Flash {
    age: f32,
    radius: f32,
}

const FLASH_SECS: f32 = 0.45;

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParticleAssets>()
            .add_message::<Burst>()
            .add_message::<ExplosionFlash>()
            .add_systems(Update, (spawn_bursts, spawn_flashes, update_particles, update_flashes));
    }
}

/// Cheap deterministic scatter generator.
pub struct Scatter(u32);

impl Scatter {
    pub fn new(seed: u32) -> Self {
        Self(seed | 1)
    }

    pub fn unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// Debris of a broken block.
pub fn spawn_debris(
    commands: &mut Commands,
    assets: &mut ParticleAssets,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    block: IVec3,
    id: BlockId,
) {
    let mesh = assets.mesh.get_or_insert_with(|| meshes.add(Cuboid::new(0.12, 0.12, 0.12))).clone();
    let material = assets.material(materials, id.base_color());
    let mut s = Scatter::new(
        (block.x as u32).wrapping_mul(73856093) ^ (block.y as u32).wrapping_mul(19349663) ^ (block.z as u32).wrapping_mul(83492791),
    );
    for _ in 0..10 {
        let offset = Vec3::new(s.unit(), s.unit(), s.unit());
        let vel = Vec3::new(s.unit() - 0.5, s.unit() * 0.8 + 0.2, s.unit() - 0.5) * 4.0;
        commands.spawn((
            Particle { vel, life: 0.6 + s.unit() * 0.4, gravity: 18.0, size: 1.0 },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(block.as_vec3() + offset),
        ));
    }
}

fn spawn_bursts(
    mut commands: Commands,
    mut requests: MessageReader<Burst>,
    mut assets: ResMut<ParticleAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<(), With<Particle>>,
    mut n: Local<u32>,
) {
    let mut alive = existing.iter().count();
    for b in requests.read() {
        let mesh = assets.mesh.get_or_insert_with(|| meshes.add(Cuboid::new(0.12, 0.12, 0.12))).clone();
        let material = assets.material(&mut materials, b.color);
        *n = n.wrapping_add(1);
        let mut s = Scatter::new(n.wrapping_mul(2_654_435_761));
        for _ in 0..b.count {
            if alive >= MAX_PARTICLES {
                return;
            }
            alive += 1;
            let dir = Vec3::new(s.unit() - 0.5, s.unit() - 0.3, s.unit() - 0.5).normalize_or_zero();
            commands.spawn((
                Particle { vel: dir * b.speed * (0.4 + s.unit() * 0.6), life: b.life * (0.6 + s.unit() * 0.4), gravity: 6.0, size: 0.8 + s.unit() * 1.4 },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(b.center + dir * 0.2),
            ));
        }
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
        p.vel.y -= p.gravity * dt;
        t.translation += p.vel * dt;
        // Particles shrink away during their last half second.
        let shrink = (p.life * 2.0).min(1.0);
        t.scale = Vec3::splat(p.size * shrink);
    }
}

fn spawn_flashes(
    mut commands: Commands,
    mut requests: MessageReader<ExplosionFlash>,
    mut assets: ResMut<ParticleAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for f in requests.read() {
        let mesh = assets.sphere.get_or_insert_with(|| meshes.add(Sphere::new(1.0).mesh().ico(2).expect("ico sphere"))).clone();
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.8, 0.4, 0.9),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        commands.spawn((
            Flash { age: 0.0, radius: f.radius },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(f.center).with_scale(Vec3::splat(0.1)),
        ));
    }
}

fn update_flashes(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(Entity, &mut Flash, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    for (e, mut f, mut t, m) in &mut q {
        f.age += time.delta_secs();
        let p = f.age / FLASH_SECS;
        if p >= 1.0 {
            materials.remove(&m.0);
            commands.entity(e).despawn();
            continue;
        }
        t.scale = Vec3::splat(f.radius * (0.2 + 0.9 * (1.0 - (1.0 - p).powi(3))));
        if let Some(mut mat) = materials.get_mut(&m.0) {
            // White-hot at first, then orange smoke fading out.
            let c = Color::srgba(1.0, 0.95 - 0.5 * p, 0.7 - 0.6 * p, (1.0 - p).powi(2) * 0.9);
            mat.base_color = c;
        }
    }
}
