//! Systems that turn game events into [`PlaySound`] requests: footsteps, landing, splashes,
//! circuit components, lava bubbles and menu clicks. Combat and explosion sounds are sent by the
//! mob and health systems themselves.

use super::synth::Rng;
use super::{PlaySound, Sound};
use crate::player::controller::Player;
use crate::ui::screens::Screen;
use crate::world::block::{BlockId, Kind};
use crate::world::{BlockChanged, BlockQuery, VoxelWorld};
use bevy::prelude::*;

/// Distance walked between two footsteps.
const WALK_STRIDE: f32 = 1.75;
const SPRINT_STRIDE: f32 = 2.2;
const SNEAK_STRIDE: f32 = 1.2;
/// Fall speed above which landing makes a sound.
const LANDING_SPEED: f32 = 7.0;

pub struct SoundTriggersPlugin;

impl Plugin for SoundTriggersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LavaAmbience>()
            .add_systems(Update, (footsteps, water_sounds, block_change_sounds, ui_click_sounds, lava_ambience));
    }
}

#[derive(Default)]
struct StepState {
    last: Option<Vec3>,
    walked: f32,
    was_on_ground: bool,
    last_fall_speed: f32,
}

fn footsteps(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    player: Query<(&Player, &Transform)>,
    mut state: Local<StepState>,
    mut out: MessageWriter<PlaySound>,
) {
    let Ok((p, t)) = player.single() else { return };
    if !p.active || time.delta_secs() == 0.0 {
        state.last = None;
        return;
    }
    let pos = t.translation;
    let under = |pos: Vec3| world.block_at(IVec3::new(pos.x.floor() as i32, (pos.y - 0.05).floor() as i32, pos.z.floor() as i32));
    // Landing: the frame the player touches the ground after falling fast.
    if p.on_ground && !state.was_on_ground && state.last_fall_speed < -LANDING_SPEED && !p.in_water {
        let volume = ((-state.last_fall_speed - LANDING_SPEED) / 12.0 + 0.6).min(1.2);
        out.write(PlaySound::global(Sound::Step(under(pos))).with_volume(volume));
        state.walked = 0.0;
    }
    state.was_on_ground = p.on_ground;
    state.last_fall_speed = p.vel.y;

    if let Some(last) = state.last {
        let moved = Vec2::new(pos.x - last.x, pos.z - last.z).length();
        if moved < 3.0 && !p.flying && (p.on_ground || p.in_water) {
            state.walked += moved;
            let stride = if p.sneaking {
                SNEAK_STRIDE
            } else if p.sprinting {
                SPRINT_STRIDE
            } else {
                WALK_STRIDE
            };
            if state.walked >= stride {
                state.walked -= stride;
                let sound = if p.in_water { Sound::Splash } else { Sound::Step(under(pos)) };
                let volume = match (p.in_water, p.sneaking) {
                    (true, _) => 0.25,
                    (_, true) => 0.3,
                    _ => 0.8,
                };
                out.write(PlaySound::global(sound).with_volume(volume));
            }
        }
    }
    state.last = Some(pos);
}

/// A splash when entering or leaving water, louder for faster dives.
fn water_sounds(
    player: Query<&Player>,
    mut was_in_water: Local<bool>,
    mut last_vy: Local<f32>,
    mut out: MessageWriter<PlaySound>,
) {
    let Ok(p) = player.single() else { return };
    if !p.active {
        return;
    }
    if p.in_water != *was_in_water {
        let volume = if p.in_water { (0.4 + (-*last_vy).max(0.0) / 15.0).min(1.0) } else { 0.35 };
        out.write(PlaySound::global(Sound::Splash).with_volume(volume));
    }
    *was_in_water = p.in_water;
    *last_vy = p.vel.y;
}

/// Clicks of levers, buttons and plates, pistons, and steam when fluids react.
fn block_change_sounds(mut changes: MessageReader<BlockChanged>, mut out: MessageWriter<PlaySound>) {
    let mut budget = 8;
    for BlockChanged(c) in changes.read() {
        if budget == 0 {
            break;
        }
        let at = c.pos.as_vec3() + Vec3::splat(0.5);
        let sound = match (c.old.kind(), c.new.kind()) {
            (Kind::Lever { on: a, .. }, Kind::Lever { on: b, .. }) if a != b => Some(Sound::LeverClick),
            (Kind::Button { pressed: a, .. }, Kind::Button { pressed: b, .. }) if a != b => Some(Sound::ButtonClick),
            (Kind::Plate { pressed: a }, Kind::Plate { pressed: b }) if a != b => Some(Sound::PlateClick),
            (Kind::Piston { extended: false, .. }, Kind::Piston { extended: true, .. }) => Some(Sound::PistonOut),
            (Kind::Piston { extended: true, .. }, Kind::Piston { extended: false, .. }) => Some(Sound::PistonIn),
            (old, _) if is_fluid_kind(old) && (c.new == BlockId::OBSIDIAN || c.new == BlockId::COBBLE) => Some(Sound::Fizz),
            _ => None,
        };
        if let Some(sound) = sound {
            out.write(PlaySound::at(sound, at));
            budget -= 1;
        }
    }
}

fn is_fluid_kind(k: Kind) -> bool {
    matches!(k, Kind::Water(_) | Kind::Lava(_))
}

fn ui_click_sounds(
    screen: Res<Screen>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut out: MessageWriter<PlaySound>,
    mut last_screen: Local<Option<Screen>>,
) {
    let clicked = mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right);
    let opened = last_screen.is_some_and(|s| s != *screen) && *screen != Screen::Playing;
    *last_screen = Some(*screen);
    if *screen != Screen::Playing && (clicked || opened) {
        out.write(PlaySound::global(Sound::UiClick));
    }
}

#[derive(Resource)]
struct LavaAmbience {
    timer: f32,
    rng: Rng,
}

impl Default for LavaAmbience {
    fn default() -> Self {
        Self { timer: 0.0, rng: Rng::new(0x1A7A_0001) }
    }
}

/// Bubbles pop on lava surfaces near the player.
fn lava_ambience(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    player: Query<&Transform, With<Player>>,
    mut state: ResMut<LavaAmbience>,
    mut out: MessageWriter<PlaySound>,
) {
    state.timer -= time.delta_secs();
    if state.timer > 0.0 {
        return;
    }
    state.timer = 0.35;
    let Ok(t) = player.single() else { return };
    let base = t.translation.floor().as_ivec3();
    for _ in 0..24 {
        let d = IVec3::new(
            state.rng.range(-9.0, 9.0) as i32,
            state.rng.range(-5.0, 5.0) as i32,
            state.rng.range(-9.0, 9.0) as i32,
        );
        let p = base + d;
        if world.block(p).is_some_and(BlockId::is_lava) && world.block(p + IVec3::Y).is_some_and(BlockId::is_air) {
            if state.rng.unit() < 0.5 {
                out.write(PlaySound::at(Sound::LavaPop, p.as_vec3() + Vec3::new(0.5, 1.0, 0.5)).with_volume(0.6));
            }
            break;
        }
    }
}
