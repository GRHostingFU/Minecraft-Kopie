//! Turns the keyboard into player intent: movement axes, jump, and the sprint and sneak modes
//! (hold or toggle, as chosen in the options). The physics step in `controller` only sees the
//! resulting [`Controls`].

use super::controller::{InputCapture, Player};
use crate::session::{AppState, GameMode};
use crate::settings::{GameSettings, KeyMode};
use crate::survival::{SPRINT_FOOD, Vitals};
use bevy::prelude::*;

/// Two W presses within this many seconds start a sprint.
const DOUBLE_TAP_SECS: f32 = 0.3;

/// What the player wants this frame.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
pub struct Controls {
    /// Right / forward axes in `-1..=1`.
    pub wish: Vec2,
    pub jump: bool,
    /// Shift is down: descend while flying.
    pub descend: bool,
    pub sprint: bool,
    pub sneak: bool,
    /// The sprint key (flying speed boost).
    pub fast: bool,
}

/// Inputs of one frame for the sprint latch.
#[derive(Clone, Copy, Debug, Default)]
pub struct SprintInput {
    pub forward_pressed: bool,
    pub forward_held: bool,
    pub ctrl_held: bool,
    pub ctrl_pressed: bool,
    pub sneaking: bool,
    /// The previous physics step ran into a wall.
    pub blocked: bool,
    /// Hungry players cannot sprint.
    pub allowed: bool,
    pub mode: KeyMode,
}

/// The sprint "latch": once started it lasts until the player stops moving forward, sneaks or
/// runs into something.
#[derive(Resource, Default, Debug)]
pub struct SprintState {
    last_forward_tap: Option<f32>,
    pub latched: bool,
    /// The latch was started by holding the sprint key (only that kind ends when the key is released).
    via_key: bool,
    /// The previous physics step ran into a wall while sprinting.
    pub blocked: bool,
}

impl SprintState {
    /// `now` is in seconds. Returns whether the player is sprinting this frame.
    pub fn update(&mut self, now: f32, i: SprintInput) -> bool {
        if i.forward_pressed {
            if self.last_forward_tap.is_some_and(|t| now - t <= DOUBLE_TAP_SECS) {
                self.latched = true;
                self.via_key = false;
            }
            self.last_forward_tap = Some(now);
        }
        match i.mode {
            KeyMode::Hold => {
                if i.forward_held && i.ctrl_held {
                    self.latched = true;
                    self.via_key = true;
                }
                if self.via_key && !i.ctrl_held {
                    self.latched = false;
                }
            }
            KeyMode::Toggle => {
                if i.ctrl_pressed {
                    self.latched = !self.latched;
                    self.via_key = false;
                }
            }
        }
        if !i.forward_held || i.sneaking || i.blocked || !i.allowed {
            self.latched = false;
        }
        self.latched
    }
}

/// Sneaking, in hold or toggle mode.
#[derive(Resource, Default, Debug)]
pub struct SneakState {
    toggled: bool,
}

impl SneakState {
    pub fn update(&mut self, mode: KeyMode, held: bool, pressed: bool) -> bool {
        match mode {
            KeyMode::Hold => {
                self.toggled = false;
                held
            }
            KeyMode::Toggle => {
                if pressed {
                    self.toggled = !self.toggled;
                }
                self.toggled
            }
        }
    }

    pub fn reset(&mut self) {
        self.toggled = false;
    }
}

pub struct ControlsPlugin;

impl Plugin for ControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Controls>()
            .init_resource::<SprintState>()
            .init_resource::<SneakState>()
            .add_systems(Update, read_controls.run_if(in_state(AppState::InGame)));
    }
}

#[allow(clippy::too_many_arguments)]
fn read_controls(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<InputCapture>,
    settings: Res<GameSettings>,
    mode: Res<GameMode>,
    vitals: Res<Vitals>,
    mut sprint: ResMut<SprintState>,
    mut sneak: ResMut<SneakState>,
    mut controls: ResMut<Controls>,
    player: Query<&Player>,
) {
    let Ok(p) = player.single() else { return };
    if !capture.0 || !p.active {
        *controls = Controls::default();
        sprint.latched = false;
        sneak.reset();
        return;
    }
    let axis = |pos: KeyCode, neg: KeyCode| keys.pressed(pos) as i32 as f32 - keys.pressed(neg) as i32 as f32;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let shift_pressed = keys.any_just_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let ctrl_pressed = keys.any_just_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);

    let wish = Vec2::new(axis(KeyCode::KeyD, KeyCode::KeyA), axis(KeyCode::KeyW, KeyCode::KeyS));
    let mut c = Controls { wish, jump: keys.pressed(KeyCode::Space), descend: shift, ..default() };
    if p.flying {
        c.fast = ctrl;
        sprint.latched = false;
        sneak.reset();
    } else {
        c.sneak = sneak.update(settings.sneak_mode, shift, shift_pressed);
        // Running into a wall ends the sprint (the previous step reported the collision).
        let blocked = std::mem::take(&mut sprint.blocked);
        c.sprint = sprint.update(
            time.elapsed_secs(),
            SprintInput {
                forward_pressed: keys.just_pressed(KeyCode::KeyW),
                forward_held: keys.pressed(KeyCode::KeyW),
                ctrl_held: ctrl,
                ctrl_pressed,
                sneaking: c.sneak,
                blocked,
                allowed: *mode == GameMode::Creative || vitals.food > SPRINT_FOOD,
                mode: settings.sprint_mode,
            },
        );
    }
    *controls = c;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(t: bool, held: bool) -> SprintInput {
        SprintInput { forward_pressed: t, forward_held: held, allowed: true, ..default() }
    }

    #[test]
    fn sprint_starts_on_double_tap_or_ctrl_and_stops_when_released_or_blocked() {
        let mut s = SprintState::default();
        // A single tap is just walking.
        assert!(!s.update(0.0, input(true, true)));
        assert!(!s.update(0.1, input(false, false)));
        // Second tap within the window sprints; holding W keeps it going.
        assert!(s.update(0.2, input(true, true)));
        assert!(s.update(0.5, input(false, true)));
        // Releasing W ends it.
        assert!(!s.update(0.6, input(false, false)));
        // Too slow for a double tap.
        assert!(!s.update(2.0, input(true, true)));
        assert!(!s.update(2.5, input(true, true)));
        // Ctrl + W sprints and (hold mode) ends when Ctrl is released.
        let mut s = SprintState::default();
        let ctrl = SprintInput { ctrl_held: true, ..input(true, true) };
        assert!(s.update(0.0, ctrl));
        assert!(!s.update(0.2, input(false, true)), "hold mode: releasing the key ends the sprint");
        // Hitting a wall or sneaking ends it.
        assert!(s.update(0.4, SprintInput { ctrl_held: true, ..input(false, true) }));
        assert!(!s.update(0.5, SprintInput { ctrl_held: true, blocked: true, ..input(false, true) }));
        assert!(s.update(0.6, SprintInput { ctrl_held: true, ..input(false, true) }));
        assert!(!s.update(0.7, SprintInput { ctrl_held: true, sneaking: true, ..input(false, true) }));
    }

    #[test]
    fn double_tap_sprint_survives_releasing_the_ctrl_key_in_hold_mode() {
        let mut s = SprintState::default();
        s.update(0.0, input(true, true));
        s.update(0.1, input(false, false));
        assert!(s.update(0.2, input(true, true)));
        // No Ctrl involved: the latch is not tied to the key.
        assert!(s.update(0.4, input(false, true)));
    }

    #[test]
    fn toggle_mode_latches_on_a_key_press() {
        let mut s = SprintState::default();
        let toggle = |pressed| SprintInput { ctrl_pressed: pressed, mode: KeyMode::Toggle, ..input(false, true) };
        assert!(!s.update(0.0, toggle(false)), "nothing happens without a key press");
        assert!(s.update(0.1, toggle(true)));
        assert!(s.update(0.2, toggle(false)), "stays on without holding anything");
        assert!(!s.update(0.3, toggle(true)), "a second press switches it off again");
        assert!(s.update(0.4, toggle(true)));
        // Stopping to walk forward ends it as well.
        assert!(!s.update(0.5, SprintInput { forward_held: false, ..toggle(false) }));
    }

    #[test]
    fn hungry_players_cannot_sprint() {
        let mut s = SprintState::default();
        let ctrl = SprintInput { ctrl_held: true, allowed: false, ..input(true, true) };
        assert!(!s.update(0.0, ctrl));
    }

    #[test]
    fn sneak_modes() {
        let mut sneak = SneakState::default();
        assert!(sneak.update(KeyMode::Hold, true, true));
        assert!(!sneak.update(KeyMode::Hold, false, false));
        // Toggle: one press on, still on after release, second press off.
        assert!(sneak.update(KeyMode::Toggle, true, true));
        assert!(sneak.update(KeyMode::Toggle, false, false));
        assert!(!sneak.update(KeyMode::Toggle, true, true));
        sneak.update(KeyMode::Toggle, true, true);
        sneak.reset();
        assert!(!sneak.update(KeyMode::Toggle, false, false));
    }
}
