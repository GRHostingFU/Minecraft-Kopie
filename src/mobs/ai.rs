//! Mob behaviour: what a creeper and a skeleton decide every frame.
//!
//! Both follow the same pattern: look for the player, then either chase along an A* route
//! (re-planned when the player moves away from its end or the mob gets stuck), keep a
//! distance, wander, or - for the creeper - light its fuse.

use super::pathfinding::{PathOptions, find_path};
use super::{
    ARROW_GRAVITY, ARROW_SPEED, Brain, CREEPER_ABORT_DISTANCE, CREEPER_FUSE_SECS, CREEPER_IGNITE_DISTANCE, CREEPER_POWER, Creeper, Explode,
    Mob, MobAssets, MobKind, PathBudget, Skeleton, next_rand, spawn_arrow,
};
use crate::audio::{PlaySound, Sound};
use crate::physics::raycast::raycast;
use crate::player::controller::Player;
use crate::player::health::Health;
use crate::world::{BlockQuery, VoxelWorld};
use bevy::prelude::*;

/// How far mobs notice the player.
const SIGHT: f32 = 16.0;
/// Skeletons keep within this range to shoot.
const SHOOT_RANGE: f32 = 16.0;
const RETREAT_DISTANCE: f32 = 5.0;
const APPROACH_DISTANCE: f32 = 11.0;

/// Is the straight line between two points free of blocks?
pub fn has_line_of_sight<W: BlockQuery>(w: &W, from: Vec3, to: Vec3) -> bool {
    let d = to - from;
    let len = d.length();
    len < 0.1 || raycast(w, from, d, len).is_none_or(|h| h.distance >= len - 0.35)
}

/// Is there firm, safe ground a step ahead in direction `dir` (no pit, no lava)?
pub fn safe_ahead<W: BlockQuery>(w: &W, pos: Vec3, dir: Vec2) -> bool {
    let ahead = pos + Vec3::new(dir.x, 0.0, dir.y).normalize_or_zero() * 0.8;
    let cell = IVec3::new(ahead.x.floor() as i32, (pos.y + 0.01).floor() as i32, ahead.z.floor() as i32);
    (1..=2).any(|d| w.block(cell - IVec3::new(0, d, 0)).is_some_and(|b| b.is_solid()))
        && !w.block(cell).is_some_and(|b| b.is_lava())
        && !w.block(cell - IVec3::Y).is_some_and(|b| b.is_lava())
}

/// Horizontal velocity that follows the mob's current path.
fn follow(brain: &mut Brain, pos: Vec3, speed: f32) -> Vec2 {
    while let Some(next) = brain.path.get(brain.path_index) {
        let centre = Vec2::new(next.x as f32 + 0.5, next.z as f32 + 0.5);
        let to = centre - Vec2::new(pos.x, pos.z);
        let level = (pos.y - next.y as f32).abs() < 1.3;
        if to.length() < 0.35 && level {
            brain.path_index += 1;
        } else {
            return to.normalize_or_zero() * speed;
        }
    }
    Vec2::ZERO
}

/// Chases `goal` along a planned route. Searches are rationed by `budget`.
#[allow(clippy::too_many_arguments)]
fn steer<W: BlockQuery>(brain: &mut Brain, world: &W, budget: &mut u32, pos: Vec3, goal: IVec3, speed: f32, dt: f32, rng: &mut u32) -> Vec2 {
    brain.repath -= dt;
    brain.stuck += dt;
    if brain.stuck >= 1.0 {
        // No progress for a second although the mob wants to move: plan again.
        if brain.wish != Vec2::ZERO && (pos - brain.last_pos).length() < 0.2 {
            brain.repath = 0.0;
            brain.path.clear();
        }
        brain.last_pos = pos;
        brain.stuck = 0.0;
    }
    let goal_moved = brain.goal.is_none_or(|g| (g.x - goal.x).abs() + (g.z - goal.z).abs() > 2 || (g.y - goal.y).abs() > 2);
    let path_done = brain.path_index >= brain.path.len();
    if (goal_moved || path_done || brain.repath <= 0.0) && *budget > 0 {
        *budget -= 1;
        brain.repath = 0.6 + next_rand(rng) * 0.6;
        let start = IVec3::new(pos.x.floor() as i32, (pos.y + 0.01).floor() as i32, pos.z.floor() as i32);
        match find_path(world, start, goal, PathOptions::default()) {
            Some(r) => {
                brain.path = r.cells;
                brain.path_index = 0;
                brain.goal = Some(goal);
            }
            None => {
                brain.path.clear();
                brain.goal = Some(goal);
            }
        }
    }
    // Close enough (and nothing in the way): walk straight at it.
    let flat = Vec2::new(goal.x as f32 + 0.5 - pos.x, goal.z as f32 + 0.5 - pos.z);
    if flat.length() < 2.0 && (goal.y as f32 - pos.y).abs() < 1.5 {
        return flat.normalize_or_zero() * speed;
    }
    follow(brain, pos, speed)
}

/// Aimless walking: pick a nearby cell, walk there, rest, repeat.
#[allow(clippy::too_many_arguments)]
fn wander<W: BlockQuery>(brain: &mut Brain, world: &W, budget: &mut u32, pos: Vec3, speed: f32, dt: f32, rng: &mut u32) -> Vec2 {
    brain.wander_wait -= dt;
    if brain.wander_goal.is_none() {
        if brain.wander_wait > 0.0 {
            return Vec2::ZERO;
        }
        let angle = next_rand(rng) * std::f32::consts::TAU;
        let dist = 4.0 + next_rand(rng) * 6.0;
        brain.wander_goal = Some(IVec3::new(
            (pos.x + angle.cos() * dist).floor() as i32,
            (pos.y + 0.01).floor() as i32,
            (pos.z + angle.sin() * dist).floor() as i32,
        ));
        brain.goal = None;
    }
    let goal = brain.wander_goal.expect("set above");
    let wish = steer(brain, world, budget, pos, goal, speed * 0.55, dt, rng);
    let arrived = Vec2::new(goal.x as f32 + 0.5 - pos.x, goal.z as f32 + 0.5 - pos.z).length() < 1.0;
    if arrived || (brain.path_index >= brain.path.len() && brain.goal == Some(goal) && wish == Vec2::ZERO) {
        brain.wander_goal = None;
        brain.wander_wait = 2.0 + next_rand(rng) * 4.0;
        brain.path.clear();
        return Vec2::ZERO;
    }
    wish
}

fn feet_cell(pos: Vec3) -> IVec3 {
    IVec3::new(pos.x.floor() as i32, (pos.y + 0.01).floor() as i32, pos.z.floor() as i32)
}

#[allow(clippy::too_many_arguments)]
pub fn creeper_brain(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<VoxelWorld>,
    health: Res<Health>,
    mut budget: ResMut<PathBudget>,
    player: Query<(&Player, &Transform), Without<Mob>>,
    mut creepers: Query<(Entity, &mut Mob, &mut Brain, &Transform, &mut Creeper)>,
    mut explode: MessageWriter<Explode>,
    mut sounds: MessageWriter<PlaySound>,
    mut rng: Local<u32>,
) {
    let dt = time.delta_secs();
    if dt == 0.0 {
        return;
    }
    if *rng == 0 {
        *rng = 0xC4EE_9E41;
    }
    let target = player.single().ok().filter(|(p, _)| p.active && !health.dead).map(|(_, t)| t.translation);
    for (e, mut mob, mut brain, t, mut creeper) in &mut creepers {
        let pos = t.translation;
        if target.is_some_and(|p| p.distance(pos) > super::ACTIVE_DISTANCE) {
            continue;
        }
        let speed = MobKind::Creeper.speed();
        let mut wish = Vec2::ZERO;
        let mut face = None;
        mob.look_at = None;
        let mut engaged = false;
        if let Some(pp) = target {
            let eye = pos + Vec3::Y * 1.4;
            let player_eye = pp + Vec3::Y * 1.4;
            let to = pp - pos;
            let dist = to.length();
            let flat = Vec2::new(to.x, to.z);
            if dist <= SIGHT {
                engaged = true;
                mob.look_at = Some(pp + Vec3::Y * 1.6);
                if creeper.igniting {
                    creeper.fuse += dt;
                    face = Some(flat);
                    if dist > CREEPER_ABORT_DISTANCE {
                        creeper.igniting = false;
                    } else if creeper.fuse >= CREEPER_FUSE_SECS {
                        explode.write(Explode { center: pos + Vec3::Y * 0.9, power: CREEPER_POWER });
                        commands.entity(e).despawn();
                        continue;
                    }
                } else if dist <= CREEPER_IGNITE_DISTANCE && has_line_of_sight(&*world, eye, player_eye) {
                    creeper.igniting = true;
                    sounds.write(PlaySound::at(Sound::Hiss, pos).with_volume(0.9));
                } else {
                    wish = steer(&mut brain, &*world, &mut budget.0, pos, feet_cell(pp), speed, dt, &mut rng);
                    // Never walk off an edge or into lava while chasing.
                    if wish != Vec2::ZERO && !safe_ahead(&*world, pos, wish) && mob.on_ground {
                        wish = Vec2::ZERO;
                    }
                }
            }
        }
        if !engaged {
            wish = wander(&mut brain, &*world, &mut budget.0, pos, speed, dt, &mut rng);
            if wish != Vec2::ZERO && !safe_ahead(&*world, pos, wish) && mob.on_ground {
                wish = Vec2::ZERO;
                brain.wander_goal = None;
                brain.wander_wait = 1.0;
            }
        }
        if !creeper.igniting {
            creeper.fuse = (creeper.fuse - dt * 2.0).max(0.0);
        } else {
            wish = Vec2::ZERO;
        }
        brain.wish = wish;
        brain.face = face;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn skeleton_brain(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<VoxelWorld>,
    health: Res<Health>,
    assets: Option<Res<MobAssets>>,
    mut budget: ResMut<PathBudget>,
    player: Query<(&Player, &Transform), Without<Mob>>,
    mut skeletons: Query<(&mut Mob, &mut Brain, &Transform, &mut Skeleton)>,
    mut sounds: MessageWriter<PlaySound>,
    mut rng: Local<u32>,
) {
    let dt = time.delta_secs();
    if dt == 0.0 {
        return;
    }
    let Some(assets) = assets else { return };
    if *rng == 0 {
        *rng = 0x5CE1_E707;
    }
    let target = player.single().ok().filter(|(p, _)| p.active && !health.dead).map(|(_, t)| t.translation);
    for (mut mob, mut brain, t, mut sk) in &mut skeletons {
        let pos = t.translation;
        if target.is_some_and(|p| p.distance(pos) > super::ACTIVE_DISTANCE) {
            continue;
        }
        let speed = MobKind::Skeleton.speed();
        sk.cooldown -= dt;
        sk.strafe_timer -= dt;
        if sk.strafe_timer <= 0.0 {
            sk.strafe = if next_rand(&mut rng) < 0.5 { -1.0 } else { 1.0 };
            sk.strafe_timer = 1.0 + next_rand(&mut rng) * 1.8;
        }
        let mut wish = Vec2::ZERO;
        let mut face = None;
        let mut drawing = false;
        mob.look_at = None;
        let mut engaged = false;
        if let Some(pp) = target {
            let eye = pos + Vec3::Y * 1.7;
            let aim_point = pp + Vec3::Y * 1.1;
            let to = aim_point - eye;
            let dist = to.length();
            let flat = Vec2::new(to.x, to.z);
            let sees = dist <= SIGHT && has_line_of_sight(&*world, eye, aim_point);
            if sees {
                engaged = true;
                mob.look_at = Some(pp + Vec3::Y * 1.6);
                face = Some(flat);
                if dist < RETREAT_DISTANCE {
                    wish = -flat.normalize_or_zero() * speed;
                } else if dist > APPROACH_DISTANCE {
                    wish = steer(&mut brain, &*world, &mut budget.0, pos, feet_cell(pp), speed, dt, &mut rng);
                } else {
                    // Circle the player while shooting.
                    let perp = Vec2::new(-flat.y, flat.x).normalize_or_zero();
                    wish = perp * sk.strafe * speed * 0.6;
                }
                if wish != Vec2::ZERO && !safe_ahead(&*world, pos, wish) && mob.on_ground {
                    wish = Vec2::ZERO;
                    sk.strafe = -sk.strafe;
                }
                if dist <= SHOOT_RANGE {
                    drawing = sk.cooldown < 0.7;
                    if sk.cooldown <= 0.0 {
                        let forward = Vec3::new(-mob.yaw.sin(), 0.0, -mob.yaw.cos());
                        let origin = pos + Vec3::Y * 1.55 + forward * 0.4;
                        // Aim above the target to make up for the arrow's drop, then scatter a bit.
                        let flight = (aim_point - origin).length() / ARROW_SPEED;
                        let lifted = aim_point + Vec3::Y * 0.5 * ARROW_GRAVITY * flight * flight;
                        let mut dir = (lifted - origin).normalize_or_zero();
                        let spread = 0.012 * (dist / 8.0).clamp(0.5, 2.0);
                        dir += Vec3::new(next_rand(&mut rng) - 0.5, next_rand(&mut rng) - 0.5, next_rand(&mut rng) - 0.5) * 2.0 * spread;
                        spawn_arrow(&mut commands, &assets, origin, dir.normalize_or_zero() * ARROW_SPEED);
                        sounds.write(PlaySound::at(Sound::Shoot, pos));
                        sk.cooldown = 1.4 + next_rand(&mut rng) * 1.2;
                    }
                }
            } else if dist <= SIGHT * 1.6 {
                // Heard but not seen: go and look.
                engaged = true;
                wish = steer(&mut brain, &*world, &mut budget.0, pos, feet_cell(pp), speed, dt, &mut rng);
                if wish != Vec2::ZERO && !safe_ahead(&*world, pos, wish) && mob.on_ground {
                    wish = Vec2::ZERO;
                }
            }
        }
        if !engaged {
            wish = wander(&mut brain, &*world, &mut budget.0, pos, speed, dt, &mut rng);
            if wish != Vec2::ZERO && !safe_ahead(&*world, pos, wish) && mob.on_ground {
                wish = Vec2::ZERO;
                brain.wander_goal = None;
                brain.wander_wait = 1.0;
            }
        }
        let draw_target = if drawing { 1.0 } else { 0.0 };
        sk.draw += (draw_target - sk.draw) * (1.0 - (-dt * 9.0).exp());
        brain.wish = wish;
        brain.face = face;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::fluids::sim::tests::Map;
    use crate::world::block::BlockId;

    #[test]
    fn line_of_sight_is_blocked_by_walls_only() {
        let mut w = Map::flat();
        let (a, b) = (Vec3::new(0.5, 1.7, 0.5), Vec3::new(10.5, 1.7, 0.5));
        assert!(has_line_of_sight(&w, a, b));
        w.put(IVec3::new(5, 1, 0), BlockId::STONE);
        assert!(!has_line_of_sight(&w, a, b));
        w.put(IVec3::new(5, 1, 0), BlockId::AIR);
        w.put(IVec3::new(5, 1, 0), BlockId::WATER);
        assert!(has_line_of_sight(&w, a, b), "water does not block arrows or sight");
    }

    #[test]
    fn mobs_do_not_walk_into_pits_or_lava() {
        let mut w = Map::flat();
        let pos = Vec3::new(0.5, 1.0, 0.5);
        assert!(safe_ahead(&w, pos, Vec2::X));
        w.put(IVec3::new(1, 0, 0), BlockId::AIR);
        w.put(IVec3::new(1, -1, 0), BlockId::AIR);
        let mut deep = Map { blocks: Default::default(), floor: false };
        deep.put(IVec3::new(0, 0, 0), BlockId::STONE);
        assert!(!safe_ahead(&deep, pos, Vec2::X), "nothing below");
        let mut lava = Map::flat();
        lava.put(IVec3::new(1, 0, 0), BlockId::LAVA);
        assert!(!safe_ahead(&lava, pos, Vec2::X));
    }

    #[test]
    fn steering_follows_a_planned_route_around_an_obstacle() {
        let mut w = Map::flat();
        for z in -4..=4 {
            for y in 1..=3 {
                w.put(IVec3::new(4, y, z), BlockId::STONE);
            }
        }
        let mut brain = Brain::default();
        let mut budget = 3;
        let mut rng = 99u32;
        let mut pos = Vec3::new(0.5, 1.0, 0.5);
        let mut vel = Vec3::ZERO;
        let goal = IVec3::new(9, 1, 0);
        for step in 0..1500 {
            let wish = steer(&mut brain, &w, &mut budget, pos, goal, 3.0, 1.0 / 60.0, &mut rng);
            brain.wish = wish;
            budget = 3;
            super::super::step_mob(&w, &mut pos, &mut vel, (0.6, 1.95), wish, 1.0 / 60.0);
            if Vec2::new(pos.x - 9.5, pos.z - 0.5).length() < 1.2 {
                assert!(step > 60, "must detour, took {step} steps");
                return;
            }
        }
        panic!("never arrived, ended at {pos}");
    }

    #[test]
    fn wandering_moves_and_rests() {
        let w = Map::flat();
        let mut brain = Brain::default();
        let mut rng = 7u32;
        let mut budget = 3;
        let mut pos = Vec3::new(0.5, 1.0, 0.5);
        let mut vel = Vec3::ZERO;
        let mut moved = false;
        let mut rested = false;
        for _ in 0..3000 {
            let wish = wander(&mut brain, &w, &mut budget, pos, 3.0, 1.0 / 60.0, &mut rng);
            budget = 3;
            brain.wish = wish;
            moved |= wish != Vec2::ZERO;
            rested |= moved && wish == Vec2::ZERO;
            super::super::step_mob(&w, &mut pos, &mut vel, (0.6, 1.7), wish, 1.0 / 60.0);
        }
        assert!(moved && rested);
        assert!(pos.y > 0.99 && pos.y < 1.5);
    }
}
