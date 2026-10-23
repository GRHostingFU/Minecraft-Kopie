//! Which full-screen UI (if any) is open, and everything that follows from it: whether the mouse
//! is captured by the game, whether the world simulation is paused, window mode changes.

use crate::inventory::Inventory;
use crate::player::controller::{InputCapture, Player};
use crate::save::SaveRequest;
use crate::settings::GameSettings;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, MonitorSelection, PrimaryWindow, WindowFocused, WindowMode};

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Screen {
    /// No menu: the mouse looks around, clicks break and place blocks.
    #[default]
    Playing,
    Inventory,
    Pause,
    Options,
}

impl Screen {
    /// Menus that freeze the world.
    pub fn pauses_game(self) -> bool {
        matches!(self, Screen::Pause | Screen::Options)
    }

    /// The screen reached by pressing Esc.
    pub fn after_escape(self) -> Screen {
        match self {
            Screen::Playing => Screen::Pause,
            Screen::Inventory | Screen::Pause => Screen::Playing,
            Screen::Options => Screen::Pause,
        }
    }
}

pub struct ScreensPlugin;

impl Plugin for ScreensPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Screen>().add_systems(
            Update,
            (screen_input, pause_on_focus_loss, toggle_fullscreen_key, sync_capture, sync_time_pause, apply_fullscreen)
                .chain()
                .before(super::widgets::refresh_slots),
        );
    }
}

fn screen_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut screen: ResMut<Screen>,
    mut inventory: ResMut<Inventory>,
    mut saves: MessageWriter<SaveRequest>,
    player: Query<&Player>,
) {
    let active = player.single().is_ok_and(|p| p.active);
    let before = *screen;
    if keys.just_pressed(KeyCode::Escape) {
        *screen = screen.after_escape();
        // Esc opens the pause menu and saves the world, so a crash or quit afterwards loses nothing.
        if before == Screen::Playing {
            saves.write(SaveRequest { then_quit: false });
        }
    } else if keys.just_pressed(KeyCode::KeyE) && active {
        match *screen {
            Screen::Playing => *screen = Screen::Inventory,
            Screen::Inventory => *screen = Screen::Playing,
            _ => {}
        }
    }
    if before == Screen::Inventory && *screen != Screen::Inventory {
        // Whatever is still on the cursor goes back into the inventory.
        inventory.return_cursor();
    }
}

fn toggle_fullscreen_key(keys: Res<ButtonInput<KeyCode>>, mut settings: ResMut<GameSettings>) {
    if keys.just_pressed(KeyCode::F11) {
        settings.fullscreen = !settings.fullscreen;
    }
}

/// Alt-tabbing away opens the menu instead of leaving the game running unattended.
fn pause_on_focus_loss(
    mut screen: ResMut<Screen>,
    mut focus: MessageReader<WindowFocused>,
    mut was_focused: Local<bool>,
) {
    for f in focus.read() {
        // Only after the window has actually been focused once (a window that never gets focus,
        // e.g. under a bare X server, must not start paused).
        if !f.focused && *was_focused && *screen == Screen::Playing {
            *screen = Screen::Pause;
        }
        if f.focused {
            *was_focused = true;
        }
    }
}

fn sync_capture(
    screen: Res<Screen>,
    player: Query<&Player>,
    mut capture: ResMut<InputCapture>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let want = *screen == Screen::Playing && player.single().is_ok_and(|p| p.active);
    if capture.0 != want {
        capture.0 = want;
    }
    let Ok(mut cursor) = cursor.single_mut() else { return };
    let grab = if want { CursorGrabMode::Locked } else { CursorGrabMode::None };
    if cursor.grab_mode != grab {
        cursor.grab_mode = grab;
    }
    if cursor.visible == want {
        cursor.visible = !want;
    }
}

/// The pause menu freezes physics, animations and the day/night clock.
fn sync_time_pause(screen: Res<Screen>, mut time: ResMut<Time<Virtual>>) {
    if screen.pauses_game() != time.is_paused() {
        if screen.pauses_game() {
            time.pause();
        } else {
            time.unpause();
        }
    }
}

fn apply_fullscreen(settings: Res<GameSettings>, mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if !settings.is_changed() {
        return;
    }
    let Ok(mut window) = windows.single_mut() else { return };
    let want = if settings.fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
    if window.mode != want {
        window.mode = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_navigates_back_one_level() {
        assert_eq!(Screen::Playing.after_escape(), Screen::Pause);
        assert_eq!(Screen::Pause.after_escape(), Screen::Playing);
        assert_eq!(Screen::Inventory.after_escape(), Screen::Playing);
        assert_eq!(Screen::Options.after_escape(), Screen::Pause);
        assert!(Screen::Pause.pauses_game() && Screen::Options.pauses_game());
        assert!(!Screen::Inventory.pauses_game() && !Screen::Playing.pauses_game());
    }
}
