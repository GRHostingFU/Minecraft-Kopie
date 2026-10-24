//! Everything the player does to the world with the mouse: targeting, mining (instantly in
//! creative mode, with progress and tools in survival), attacking mobs, placing blocks, eating,
//! opening crafting tables and furnaces, tossing items and the hotbar.

use super::controller::{InputCapture, Player, PlayerCamera};
use crate::inventory::{HOTBAR_SLOTS, Inventory, OFFHAND};
use crate::items::{Item, ItemStack};
use crate::mobs::{DamageMob, Mob, ray_aabb};
use crate::physics::collision::Body;
use crate::physics::raycast::{RayHit, raycast};
use crate::session::{AppState, GameMode};
use crate::survival::drops::{DropSpawner, Particle};
use crate::survival::mining::MiningState;
use crate::survival::{Experience, MAX_FOOD, Vitals};
use crate::ui::screens::Screen;
use crate::world::VoxelWorld;
use crate::world::block::{BlockId, Shape};
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;

/// How far blocks can be reached in each game mode.
pub fn reach(mode: GameMode) -> f32 {
    match mode {
        GameMode::Creative => 6.0,
        GameMode::Survival => 5.0,
    }
}

/// How far mobs can be hit.
pub const MOB_REACH: f32 = 3.6;
const REPEAT_SECS: f32 = 0.2;
/// Pause after a broken block before the next one can be started (survival).
const BREAK_DELAY: f32 = 0.25;
const ATTACK_SECS: f32 = 0.5;
/// Seconds of chewing before food is eaten.
pub const EAT_SECS: f32 = 1.6;

/// Sent whenever the player uses the held item (left or right click); drives the hand animation.
#[derive(Message, Clone, Copy, Debug)]
pub struct HandSwing;

/// The block under the crosshair, if any.
#[derive(Resource, Default)]
pub struct Target(pub Option<RayHit>);

/// The mob under the crosshair (entity and distance), if it is closer than the targeted block.
#[derive(Resource, Default)]
pub struct MobTarget(pub Option<(Entity, f32)>);

#[derive(Resource, Default)]
struct ActionCooldown {
    place: f32,
    break_: f32,
    attack: f32,
}

/// Seconds spent chewing the current food item.
#[derive(Resource, Default)]
pub struct Eating(pub f32);

/// Everything needed to remove a block the way the player does.
#[derive(SystemParam)]
pub struct BlockBreaker<'w, 's> {
    pub world: ResMut<'w, VoxelWorld>,
    pub inventory: ResMut<'w, Inventory>,
    xp: ResMut<'w, Experience>,
    vitals: ResMut<'w, Vitals>,
    mode: Res<'w, GameMode>,
    drops: DropSpawner<'w, 's>,
}

impl BlockBreaker<'_, '_> {
    /// Breaks `pos` with the selected item. Returns whether a block was removed.
    pub fn break_block(&mut self, pos: IVec3) -> bool {
        let id = self.world.block_at(pos);
        if !id.is_breakable() {
            return false;
        }
        let held = self.inventory.held_item();
        let (_, harvested) = Item::mining(held, id);
        if !self.world.set_block(pos, BlockId::AIR) {
            return false;
        }
        self.drops.debris(pos, id);
        let centre = pos.as_vec3() + Vec3::splat(0.5);
        let roll = (pos.x as u32).wrapping_mul(0x9E37_79B1)
            ^ (pos.y as u32).wrapping_mul(0x85EB_CA6B)
            ^ (pos.z as u32).wrapping_mul(0xC2B2_AE35);
        if *self.mode == GameMode::Survival {
            if id == BlockId::FURNACE {
                for stack in self.inventory.remove_furnace(pos) {
                    self.drops.spawn_scattered(stack, centre, roll);
                }
            }
            if let Some(stack) = Item::loot(id, harvested, roll >> 4) {
                self.drops.spawn_scattered(stack, centre, roll);
            }
            if harvested {
                self.xp.add(Item::block_xp(id));
            }
            self.vitals.add_exhaustion(0.005);
            if let Some((kind, _)) = held.and_then(Item::tool_info)
                && id.info().hardness > 0.0
            {
                self.inventory.wear_selected(if kind == crate::items::ToolKind::Sword { 2 } else { 1 });
            }
        } else if id == BlockId::FURNACE {
            self.inventory.remove_furnace(pos);
        }
        // A torch on top loses its support.
        let above = pos + IVec3::Y;
        if self.world.block_at(above) == BlockId::TORCH {
            self.world.set_block(above, BlockId::AIR);
            if *self.mode == GameMode::Survival {
                self.drops.spawn_scattered(
                    ItemStack::new(Item::block(BlockId::TORCH), 1),
                    above.as_vec3() + Vec3::splat(0.5),
                    roll,
                );
            }
        }
        true
    }
}

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Target>()
            .init_resource::<MobTarget>()
            .init_resource::<ActionCooldown>()
            .init_resource::<Eating>()
            .add_message::<HandSwing>()
            .add_systems(
                Update,
                (hotbar_input, item_keys, update_target, mine_or_attack, use_item, draw_highlight, update_particles)
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

fn hotbar_input(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    capture: Res<InputCapture>,
    mut inventory: ResMut<Inventory>,
) {
    if !capture.0 {
        return;
    }
    const DIGITS: [KeyCode; HOTBAR_SLOTS] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    for (i, k) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*k) && inventory.selected != i {
            inventory.selected = i;
        }
    }
    if scroll.delta.y != 0.0 {
        inventory.cycle(-scroll.delta.y.signum() as i32);
    }
}

/// `Q` tosses one item (Ctrl+Q the whole stack), `X` swaps the hand with the off hand.
fn item_keys(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<InputCapture>,
    mut inventory: ResMut<Inventory>,
    cam: Query<&GlobalTransform, With<PlayerCamera>>,
    mut drops: DropSpawner,
) {
    if !capture.0 {
        return;
    }
    if keys.just_pressed(KeyCode::KeyX) {
        inventory.swap_offhand();
    }
    if keys.just_pressed(KeyCode::KeyQ)
        && let Ok(cam) = cam.single()
        && let Some(stack) = inventory.selected_stack()
    {
        let all = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let count = if all { stack.count } else { 1 };
        for _ in 0..count {
            inventory.take_one_selected();
        }
        let dir = cam.forward().as_vec3();
        drops.spawn_delayed(
            ItemStack { count, ..stack },
            cam.translation() + dir * 0.4 - Vec3::Y * 0.3,
            dir * 5.5 + Vec3::Y * 1.5,
            2.0,
        );
    }
}

fn update_target(
    world: Res<VoxelWorld>,
    mode: Res<GameMode>,
    cam: Query<&GlobalTransform, With<PlayerCamera>>,
    mobs: Query<(Entity, &Transform, &Mob)>,
    mut target: ResMut<Target>,
    mut mob_target: ResMut<MobTarget>,
) {
    let Ok(g) = cam.single() else { return };
    let (origin, dir) = (g.translation(), g.forward().as_vec3());
    let hit = raycast(&*world, origin, dir, reach(*mode));
    let mob = mobs
        .iter()
        .filter_map(|(e, t, m)| {
            let (hw, h) = m.kind.size();
            let (lo, hi) = (t.translation - Vec3::new(hw, 0.0, hw), t.translation + Vec3::new(hw, h, hw));
            ray_aabb(origin, dir, lo, hi).filter(|d| *d <= MOB_REACH).map(|d| (e, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    // A mob behind a wall cannot be hit.
    mob_target.0 = mob.filter(|(_, d)| hit.is_none_or(|h| *d < h.distance));
    target.0 = hit;
}

#[allow(clippy::too_many_arguments)]
fn mine_or_attack(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    capture: Res<InputCapture>,
    mode: Res<GameMode>,
    target: Res<Target>,
    mob_target: Res<MobTarget>,
    mut mining: ResMut<MiningState>,
    mut cooldown: ResMut<ActionCooldown>,
    mut breaker: BlockBreaker,
    mut swings: MessageWriter<HandSwing>,
    mut mob_hits: MessageWriter<DamageMob>,
    mut swing_timer: Local<f32>,
    player: Query<(&Player, &Transform)>,
    cam: Query<&GlobalTransform, With<PlayerCamera>>,
) {
    let dt = time.delta_secs();
    cooldown.break_ = (cooldown.break_ - dt).max(0.0);
    cooldown.attack = (cooldown.attack - dt).max(0.0);
    *swing_timer = (*swing_timer - dt).max(0.0);
    let Ok((p, _)) = player.single() else { return };
    // The click that closes a menu must not also break a block.
    if !capture.0 || !p.active {
        mining.reset();
        return;
    }
    // A click that is released within the same frame still counts as a press (slow frames).
    let left_click = mouse.just_pressed(MouseButton::Left);
    let left_down = mouse.pressed(MouseButton::Left) || left_click;
    if !left_down {
        mining.reset();
        return;
    }

    // Mobs first: a click on one hits it.
    if let Some((entity, _)) = mob_target.0 {
        mining.reset();
        if left_click || cooldown.attack == 0.0 {
            if cooldown.attack > 0.0 {
                return;
            }
            cooldown.attack = ATTACK_SECS;
            swings.write(HandSwing);
            let held = breaker.inventory.held_item();
            let falling = !p.on_ground && !p.flying && p.vel.y < -0.1;
            let amount = held.map_or(1.0, Item::attack_damage) * if falling { 1.5 } else { 1.0 };
            let forward = cam.single().map_or(Vec3::NEG_Z, |c| c.forward().as_vec3());
            let push = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero() * 5.0;
            mob_hits.write(DamageMob { entity, amount, knockback: push });
            if *mode == GameMode::Survival {
                breaker.vitals.add_exhaustion(0.1);
                if held.and_then(Item::tool_info).is_some() {
                    breaker.inventory.wear_selected(
                        if held.and_then(Item::tool_info).is_some_and(|(k, _)| k == crate::items::ToolKind::Sword) {
                            1
                        } else {
                            2
                        },
                    );
                }
            }
        }
        return;
    }

    // Clicking at the sky still swings the arm, like in Minecraft.
    let Some(hit) = target.0 else {
        mining.reset();
        if left_click {
            swings.write(HandSwing);
        }
        return;
    };
    let id = breaker.world.block_at(hit.block);
    if !id.is_breakable() {
        mining.reset();
        if left_click {
            swings.write(HandSwing);
        }
        return;
    }
    match *mode {
        GameMode::Creative => {
            if left_click || cooldown.break_ == 0.0 {
                swings.write(HandSwing);
                if breaker.break_block(hit.block) {
                    cooldown.break_ = REPEAT_SECS;
                }
            }
        }
        GameMode::Survival => {
            let held = breaker.inventory.held_item();
            let (required, _) = Item::mining(held, id);
            if mining.target != Some(hit.block) {
                mining.target = Some(hit.block);
                mining.progress = 0.0;
                mining.required = required;
            } else if mining.required > 0.0 && (mining.required - required).abs() > 1e-4 {
                // Switched tools halfway: keep the share of the work that is done.
                mining.progress *= required / mining.required;
                mining.required = required;
            }
            if cooldown.break_ > 0.0 {
                return;
            }
            mining.progress += dt;
            if *swing_timer == 0.0 {
                swings.write(HandSwing);
                *swing_timer = 0.28;
            }
            if mining.progress >= mining.required && breaker.break_block(hit.block) {
                mining.reset();
                cooldown.break_ = BREAK_DELAY;
            }
        }
    }
}

fn can_place_on(world: &VoxelWorld, block: BlockId, at: IVec3) -> bool {
    match block.shape() {
        // A torch needs something solid underneath.
        Shape::Torch => world.block_at(at - IVec3::Y).is_solid(),
        _ => true,
    }
}

#[allow(clippy::too_many_arguments)]
fn use_item(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    capture: Res<InputCapture>,
    mode: Res<GameMode>,
    target: Res<Target>,
    mut cooldown: ResMut<ActionCooldown>,
    mut eating: ResMut<Eating>,
    mut screen: ResMut<Screen>,
    mut inventory: ResMut<Inventory>,
    mut vitals: ResMut<Vitals>,
    mut world: ResMut<VoxelWorld>,
    mut swings: MessageWriter<HandSwing>,
    player: Query<(&Player, &Transform)>,
) {
    let dt = time.delta_secs();
    cooldown.place = (cooldown.place - dt).max(0.0);
    let Ok((p, pt)) = player.single() else { return };
    let click = mouse.just_pressed(MouseButton::Right);
    if !capture.0 || !p.active || !(mouse.pressed(MouseButton::Right) || click) {
        eating.0 = 0.0;
        return;
    }

    // Crafting tables and furnaces open instead of placing (sneak to place against them).
    if click
        && !p.sneaking
        && let Some(hit) = target.0
    {
        match world.block_at(hit.block) {
            BlockId::CRAFTING_TABLE => {
                inventory.set_craft_size(3);
                *screen = Screen::Workbench;
                return;
            }
            BlockId::FURNACE => {
                inventory.open_furnace(hit.block);
                *screen = Screen::Furnace;
                return;
            }
            _ => {}
        }
    }

    // The hand, or the off hand when the hand is empty.
    let slot = if inventory.selected_stack().is_some() { inventory.selected } else { OFFHAND };
    let Some(stack) = inventory.slots[slot] else {
        eating.0 = 0.0;
        return;
    };
    let creative = *mode == GameMode::Creative;

    if let Some((hunger, saturation)) = stack.item.food() {
        if creative || vitals.food >= MAX_FOOD {
            eating.0 = 0.0;
            return;
        }
        eating.0 += dt;
        if eating.0 >= EAT_SECS {
            eating.0 = 0.0;
            inventory.take_one(slot);
            vitals.eat(hunger, saturation);
        }
        return;
    }
    eating.0 = 0.0;

    let Some(block) = stack.item.as_block() else { return };
    if !click && cooldown.place > 0.0 {
        return;
    }
    let Some(hit) = target.0 else {
        if click {
            swings.write(HandSwing);
        }
        return;
    };
    if hit.normal == IVec3::ZERO {
        return;
    }
    swings.write(HandSwing);
    let at = hit.place_pos();
    let body = Body::player(pt.translation);
    let blocked_by_player = block.is_solid() && body.intersects_block(at);
    if world.block_at(at).is_replaceable()
        && !blocked_by_player
        && (0..crate::world::CHUNK_H as i32).contains(&at.y)
        && can_place_on(&world, block, at)
        && world.set_block(at, block)
    {
        if !creative {
            inventory.take_one(slot);
        }
        cooldown.place = REPEAT_SECS;
    }
}

fn draw_highlight(target: Res<Target>, mob_target: Res<MobTarget>, mut gizmos: Gizmos) {
    if mob_target.0.is_some() {
        return;
    }
    if let Some(hit) = target.0 {
        let center = hit.block.as_vec3() + Vec3::splat(0.5);
        gizmos.cube(
            Transform::from_translation(center).with_scale(Vec3::splat(1.004)),
            Color::srgba(0.0, 0.0, 0.0, 0.85),
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::Chunk;

    #[test]
    fn reach_is_shorter_in_survival() {
        assert!(reach(GameMode::Survival) < reach(GameMode::Creative));
        assert!(MOB_REACH < reach(GameMode::Survival));
    }

    #[test]
    fn torches_need_a_floor_and_everything_else_does_not() {
        let mut w = VoxelWorld::default();
        w.insert_chunk(IVec2::ZERO, Chunk::empty());
        w.set_block(IVec3::new(2, 10, 2), BlockId::STONE);
        assert!(can_place_on(&w, BlockId::TORCH, IVec3::new(2, 11, 2)));
        assert!(!can_place_on(&w, BlockId::TORCH, IVec3::new(5, 11, 2)));
        assert!(can_place_on(&w, BlockId::STONE, IVec3::new(5, 11, 2)));
        assert!(can_place_on(&w, BlockId::GLASS, IVec3::new(5, 40, 2)));
    }

    #[test]
    fn items_that_are_not_blocks_are_not_placeable() {
        assert!(Item::STICK.as_block().is_none());
        assert!(matches!(Item::COOKED_BEEF.kind(), Some(crate::items::ItemKind::Food { .. })));
    }
}
