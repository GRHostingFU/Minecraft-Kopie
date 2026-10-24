//! Items lying in the world: spawned when blocks break, mobs die or the player tosses something,
//! pulled towards the player and picked up when close.

use crate::inventory::Inventory;
use crate::items::{Item, ItemStack};
use crate::physics::collision::{Body, in_water, move_body};
use crate::player::controller::Player;
use crate::player::viewmodel::{block_cube_mesh, icon_quad_mesh, is_cube_item};
use crate::rendering::AtlasHandle;
use crate::rendering::sky::TimeOfDay;
use crate::session::{AppState, GameEntity};
use crate::settings::GameSettings;
use crate::world::BlockId;
use crate::world::VoxelWorld;
use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use std::f32::consts::TAU;

const LIFETIME: f32 = 300.0;
const PICKUP_DELAY: f32 = 0.6;
const PICKUP_RADIUS: f32 = 1.25;
const MAGNET_RADIUS: f32 = 2.6;
const MAX_DROPS: usize = 220;
const GRAVITY: f32 = 22.0;

#[derive(Component)]
pub struct ItemDrop {
    pub stack: ItemStack,
    pub age: f32,
    pub vel: Vec3,
    pub pickup_delay: f32,
}

/// The visible part (cube or flat icon) that spins and bobs above the physical position.
#[derive(Component)]
struct DropModel {
    phase: f32,
}

/// A little cube of debris flying off a broken block.
#[derive(Component)]
pub struct Particle {
    pub vel: Vec3,
    pub life: f32,
}

const MAX_PARTICLES: usize = 400;

#[derive(Resource, Default)]
pub struct ParticleAssets {
    mesh: Option<Handle<Mesh>>,
    materials: HashMap<u8, Handle<StandardMaterial>>,
}

#[derive(Resource, Default)]
pub struct DropAssets {
    meshes: HashMap<Item, Handle<Mesh>>,
    block_material: Option<Handle<StandardMaterial>>,
    icon_material: Option<Handle<StandardMaterial>>,
}

/// Everything needed to put an item into the world.
#[derive(SystemParam)]
pub struct DropSpawner<'w, 's> {
    commands: Commands<'w, 's>,
    assets: ResMut<'w, DropAssets>,
    particle_assets: ResMut<'w, ParticleAssets>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    atlas: Res<'w, AtlasHandle>,
    particles: Query<'w, 's, (), With<Particle>>,
    settings: Res<'w, GameSettings>,
}

impl DropSpawner<'_, '_> {
    pub fn spawn(&mut self, stack: ItemStack, pos: Vec3, vel: Vec3) {
        self.spawn_delayed(stack, pos, vel, PICKUP_DELAY);
    }

    /// Like [`spawn`](Self::spawn), but the item cannot be picked up for `delay` seconds (tossed items).
    pub fn spawn_delayed(&mut self, stack: ItemStack, pos: Vec3, vel: Vec3, delay: f32) {
        let atlas = self.atlas.0.clone();
        let textured = |mode| StandardMaterial {
            base_color_texture: Some(atlas.clone()),
            unlit: true,
            alpha_mode: mode,
            double_sided: true,
            cull_mode: None,
            ..default()
        };
        let block_material =
            self.assets.block_material.get_or_insert_with(|| self.materials.add(textured(AlphaMode::Opaque))).clone();
        let icon_material =
            self.assets.icon_material.get_or_insert_with(|| self.materials.add(textured(AlphaMode::Mask(0.5)))).clone();
        let cube = is_cube_item(stack.item);
        let mesh = self
            .assets
            .meshes
            .entry(stack.item)
            .or_insert_with(|| {
                self.meshes.add(match stack.item.as_block() {
                    Some(b) if cube => block_cube_mesh(b),
                    _ => icon_quad_mesh(stack.item.icon_tile()),
                })
            })
            .clone();
        let (scale, material) = if cube { (0.3, block_material) } else { (0.42, icon_material) };
        let phase = (pos.x * 12.9898 + pos.z * 78.233).sin().abs() * TAU;
        self.commands
            .spawn((
                ItemDrop { stack, age: 0.0, vel, pickup_delay: delay },
                GameEntity,
                Transform::from_translation(pos),
                Visibility::default(),
            ))
            .with_children(|p| {
                p.spawn((
                    DropModel { phase },
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_xyz(0.0, 0.25, 0.0).with_scale(Vec3::splat(scale)),
                ));
            });
    }

    /// Spawns the cubes that fly off a broken block (fewer or none with the particle option).
    pub fn debris(&mut self, block: IVec3, id: BlockId) {
        let count = (10.0 * self.settings.particles.factor()).round() as usize;
        if count == 0 || self.particles.iter().count() >= MAX_PARTICLES {
            return;
        }
        let mesh =
            self.particle_assets.mesh.get_or_insert_with(|| self.meshes.add(Cuboid::new(0.12, 0.12, 0.12))).clone();
        let materials = &mut self.materials;
        let material = self
            .particle_assets
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
        for _ in 0..count {
            let offset = Vec3::new(rnd(), rnd(), rnd());
            let vel = Vec3::new(rnd() - 0.5, rnd() * 0.8 + 0.2, rnd() - 0.5) * 4.0;
            self.commands.spawn((
                Particle { vel, life: 0.6 + rnd() * 0.4 },
                GameEntity,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(block.as_vec3() + offset),
            ));
        }
    }

    /// Scatters `stack` like a broken block would: a small random hop.
    pub fn spawn_scattered(&mut self, stack: ItemStack, pos: Vec3, seed: u32) {
        let mut s = seed.wrapping_mul(2654435761).wrapping_add(0x9E37_79B9);
        let mut r = || {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            (s >> 8) as f32 / (1u32 << 24) as f32
        };
        self.spawn(stack, pos, Vec3::new((r() - 0.5) * 3.0, 3.0 + r() * 1.5, (r() - 0.5) * 3.0));
    }
}

pub struct DropsPlugin;

impl Plugin for DropsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DropAssets>().init_resource::<ParticleAssets>().add_systems(
            Update,
            (drop_physics, pickup_drops, animate_drops, shade_drops, cap_drops)
                .chain()
                .run_if(in_state(AppState::InGame)),
        );
    }
}

fn drop_physics(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    mut commands: Commands,
    mut drops: Query<(Entity, &mut ItemDrop, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    for (e, mut d, mut t) in &mut drops {
        d.age += dt;
        d.pickup_delay = (d.pickup_delay - dt).max(0.0);
        if d.age > LIFETIME || t.translation.y < -32.0 {
            commands.entity(e).despawn();
            continue;
        }
        let body = Body { pos: t.translation, half_width: 0.15, height: 0.3 };
        let swimming = in_water(&*world, &body);
        d.vel.y -= (if swimming { 2.0 } else { GRAVITY }) * dt;
        if swimming {
            d.vel *= (-3.0 * dt).exp();
            d.vel.y += 4.0 * dt;
        }
        let r = move_body(&*world, &body, d.vel * dt);
        t.translation = r.pos;
        if r.hit_x {
            d.vel.x = 0.0;
        }
        if r.hit_z {
            d.vel.z = 0.0;
        }
        if r.hit_y {
            d.vel.y = 0.0;
        }
        if r.on_ground {
            let k = (-9.0 * dt).exp();
            d.vel.x *= k;
            d.vel.z *= k;
        }
    }
}

fn pickup_drops(
    mut commands: Commands,
    mut inventory: ResMut<Inventory>,
    player: Query<(&Player, &Transform), Without<ItemDrop>>,
    mut drops: Query<(Entity, &mut ItemDrop, &Transform)>,
    time: Res<Time>,
) {
    let Ok((p, pt)) = player.single() else { return };
    if !p.active {
        return;
    }
    let centre = pt.translation + Vec3::Y * 0.9;
    for (e, mut d, t) in &mut drops {
        if d.pickup_delay > 0.0 {
            continue;
        }
        let to = centre - (t.translation + Vec3::Y * 0.15);
        let dist = to.length();
        if dist > MAGNET_RADIUS || !inventory.can_hold(d.stack.item) {
            continue;
        }
        if dist <= PICKUP_RADIUS {
            match inventory.add(d.stack) {
                None => {
                    commands.entity(e).despawn();
                }
                Some(rest) => d.stack = rest,
            }
        } else {
            // Gentle pull: faster the closer the item is.
            let pull = to / dist * (MAGNET_RADIUS - dist + 1.0) * 5.0;
            d.vel = d.vel.lerp(pull, (time.delta_secs() * 8.0).min(1.0));
        }
    }
}

fn animate_drops(time: Res<Time>, mut models: Query<(&DropModel, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (m, mut tr) in &mut models {
        tr.rotation = Quat::from_rotation_y(t * 1.6 + m.phase);
        tr.translation.y = 0.22 + (t * 2.0 + m.phase).sin() * 0.05;
    }
}

/// Dropped items are unlit, so they follow the time of day by tinting their materials.
fn shade_drops(tod: Res<TimeOfDay>, assets: Res<DropAssets>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let l = 0.28 + 0.72 * tod.daylight();
    for handle in [&assets.block_material, &assets.icon_material].into_iter().flatten() {
        if let Some(mut m) = materials.get_mut(handle) {
            let want = Color::srgb(l, l, l);
            if m.base_color != want {
                m.base_color = want;
            }
        }
    }
}

fn cap_drops(mut commands: Commands, drops: Query<(Entity, &ItemDrop)>) {
    let n = drops.iter().count();
    if n <= MAX_DROPS {
        return;
    }
    let mut by_age: Vec<(Entity, f32)> = drops.iter().map(|(e, d)| (e, d.age)).collect();
    by_age.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (e, _) in by_age.into_iter().take(n - MAX_DROPS) {
        commands.entity(e).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::block::BlockId;

    #[test]
    fn cubes_for_solid_blocks_icons_for_everything_else() {
        assert!(is_cube_item(Item::block(BlockId::STONE)));
        assert!(!is_cube_item(Item::block(BlockId::TORCH)));
        assert!(!is_cube_item(Item::block(BlockId::GLASS)));
        assert!(!is_cube_item(Item::block(BlockId::FENCE)));
        assert!(!is_cube_item(Item::DIAMOND));
    }
}
