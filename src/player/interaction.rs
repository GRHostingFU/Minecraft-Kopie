//! Block targeting, breaking (with debris particles and item pickup), placing from the
//! inventory and hotbar input.

use super::controller::{InputCapture, Player, PlayerCamera};
use crate::audio::{PlaySound, Sound};
use crate::effects::DebrisSpawner;
use crate::inventory::{HOTBAR_SLOTS, Inventory, Item, ItemStack};
use crate::mobs::MobTarget;
use crate::mobs::combat::{attack_mobs, update_mob_target};
use crate::physics::collision::Body;
use crate::physics::raycast::{RayHit, raycast};
use crate::redstone::RedstoneSimulation;
use crate::redstone::rules::has_support;
use crate::world::block::{BlockId, Kind, select_box};
use crate::world::placement::placement_state;
use crate::world::{BlockAccess, VoxelWorld};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;

pub const REACH: f32 = 6.0;
const REPEAT_SECS: f32 = 0.2;

/// Sent whenever the player uses the held item (left or right click); drives the hand animation.
#[derive(Message, Clone, Copy, Debug)]
pub struct HandSwing;

/// The block under the crosshair, if any.
#[derive(Resource, Default)]
pub struct Target(pub Option<RayHit>);

#[derive(Resource, Default)]
struct ActionCooldown(f32);

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Target>()
            .init_resource::<ActionCooldown>()
            .add_message::<HandSwing>()
            .add_systems(
                Update,
                (hotbar_input, update_target, update_mob_target, block_actions, attack_mobs, draw_highlight).chain(),
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

fn update_target(world: Res<VoxelWorld>, cam: Query<&GlobalTransform, With<PlayerCamera>>, mut target: ResMut<Target>) {
    target.0 = cam.single().ok().and_then(|g| raycast(&*world, g.translation(), g.forward().as_vec3(), REACH));
}

#[allow(clippy::too_many_arguments)]
fn block_actions(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    capture: Res<InputCapture>,
    target: Res<Target>,
    mob_target: Res<MobTarget>,
    mut inventory: ResMut<Inventory>,
    mut world: ResMut<VoxelWorld>,
    mut redstone: ResMut<RedstoneSimulation>,
    player: Query<(&Player, &Transform)>,
    cam: Query<&GlobalTransform, With<PlayerCamera>>,
    mut cooldown: ResMut<ActionCooldown>,
    mut debris: DebrisSpawner,
    mut swings: MessageWriter<HandSwing>,
    mut sounds: MessageWriter<PlaySound>,
) {
    cooldown.0 = (cooldown.0 - time.delta_secs()).max(0.0);
    let Ok((p, pt)) = player.single() else { return };
    // The click that closes a menu must not also break a block.
    if !capture.0 || !p.active {
        return;
    }
    let ready = cooldown.0 == 0.0;
    let left = mouse.just_pressed(MouseButton::Left) || (mouse.pressed(MouseButton::Left) && ready);
    let right = mouse.just_pressed(MouseButton::Right) || (mouse.pressed(MouseButton::Right) && ready);
    // Clicking at the sky still swings the arm, like in Minecraft.
    let Some(hit) = target.0 else {
        if mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right) {
            swings.write(HandSwing);
        }
        return;
    };

    if left {
        swings.write(HandSwing);
        // A mob in front of the block is hit instead (see `mobs::combat`).
        if mob_target.in_front_of(Some(hit.distance)) {
            return;
        }
        let id = world.block_at(hit.block);
        if id.is_breakable() && world.set_block(hit.block, BlockId::AIR) {
            cooldown.0 = REPEAT_SECS;
            if let Some(item) = Item::drop_for(id) {
                // A full inventory simply loses the drop (there are no item entities yet).
                inventory.add(ItemStack::new(item, 1));
            }
            debris.debris(hit.block, id);
            sounds.write(PlaySound::at(Sound::Break(id), hit.block.as_vec3() + Vec3::splat(0.5)));
        }
    } else if right && hit.normal != IVec3::ZERO {
        swings.write(HandSwing);
        let clicked = world.block_at(hit.block);
        // Levers and buttons are used, unless sneaking (then the held block is placed against them).
        if mouse.just_pressed(MouseButton::Right)
            && !p.sneaking
            && matches!(clicked.kind(), Kind::Lever { .. } | Kind::Button { .. })
            && redstone.0.use_block(&mut *world, hit.block)
        {
            cooldown.0 = REPEAT_SECS;
            return;
        }
        let at = hit.place_pos();
        let look = cam.single().map(|g| g.forward().as_vec3()).unwrap_or(Vec3::NEG_Z);
        let body = Body::player(pt.translation);
        let Some(held) = inventory.held_item().and_then(Item::as_block) else { return };
        let Some(state) = placement_state(held, hit.normal, look) else { return };
        let free = world.block_at(at).is_replaceable();
        let fits = !state.is_solid() || !body.intersects_block(at);
        let supported = !state.is_model() || has_support(|q| world.block_at(q), at, state);
        if free && fits && supported && (0..crate::world::CHUNK_H as i32).contains(&at.y) && world.set(at, state) {
            inventory.take_one_selected();
            cooldown.0 = REPEAT_SECS;
            sounds.write(PlaySound::at(Sound::Place(state), at.as_vec3() + Vec3::splat(0.5)));
        }
    }
}

fn draw_highlight(target: Res<Target>, world: Res<VoxelWorld>, mut gizmos: Gizmos) {
    if let Some(hit) = target.0 {
        let id = world.block_at(hit.block);
        // Thin parts are outlined where they really are.
        let (lo, hi) = if id.is_model() { select_box(id) } else { (Vec3::ZERO, Vec3::ONE) };
        gizmos.cube(
            Transform::from_translation(hit.block.as_vec3() + (lo + hi) * 0.5).with_scale(hi - lo + Vec3::splat(0.004)),
            Color::srgba(0.0, 0.0, 0.0, 0.85),
        );
    }
}
