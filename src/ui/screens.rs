//! Which full-screen UI (if any) is open, and everything that follows from it: whether the mouse
//! is captured by the game, whether the world simulation is paused, window mode changes.

use crate::inventory::Inventory;
use crate::player::controller::{InputCapture, Player, PlayerCamera};
use crate::save::SaveRequest;
use crate::session::{AppState, GameMode};
use crate::settings::GameSettings;
use crate::survival::PlayerDied;
use crate::survival::drops::DropSpawner;
use bevy::prelude::*;
use bevy::window::{
    CursorGrabMode, CursorOptions, MonitorSelection, PresentMode, PrimaryWindow, WindowFocused, WindowMode,
};

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Screen {
    /// The main menu.
    #[default]
    Title,
    Worlds,
    CreateWorld,
    /// The options, reached from the title screen or the pause menu (see [`OptionsOrigin`]).
    Options,
    /// No menu: the mouse looks around, clicks break and place blocks.
    Playing,
    Inventory,
    Workbench,
    Furnace,
    Pause,
    Death,
}

/// Where the options page returns to.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub struct OptionsOrigin(pub Screen);

impl Default for OptionsOrigin {
    fn default() -> Self {
        Self(Screen::Title)
    }
}

impl Screen {
    /// Menus that freeze the world (only meaningful while a world is running).
    pub fn pauses_game(self) -> bool {
        matches!(self, Screen::Pause | Screen::Options | Screen::Death)
    }

    /// Screens that hold items: the crafting grid or a furnace is emptied when they close.
    pub fn is_container(self) -> bool {
        matches!(self, Screen::Inventory | Screen::Workbench | Screen::Furnace)
    }

    /// The screen reached by pressing Esc.
    pub fn after_escape(self, origin: Screen) -> Screen {
        match self {
            Screen::Title => Screen::Title,
            Screen::Worlds => Screen::Title,
            Screen::CreateWorld => Screen::Worlds,
            Screen::Options => origin,
            Screen::Playing => Screen::Pause,
            Screen::Inventory | Screen::Workbench | Screen::Furnace | Screen::Pause => Screen::Playing,
            Screen::Death => Screen::Death,
        }
    }
}

/// The systems that decide which screen is open; widgets and pages run after them.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScreensSet;

pub struct ScreensPlugin;

impl Plugin for ScreensPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Screen>()
            .init_resource::<OptionsOrigin>()
            .add_systems(
                Update,
                (
                    screen_input,
                    open_death_screen,
                    close_containers,
                    pause_on_focus_loss,
                    toggle_fullscreen_key,
                    sync_capture,
                    sync_time_pause,
                    apply_window_settings,
                )
                    .chain()
                    .in_set(ScreensSet),
            );
    }
}

fn screen_input(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<AppState>>,
    mut screen: ResMut<Screen>,
    origin: Res<OptionsOrigin>,
    mut mode: ResMut<GameMode>,
    mut saves: MessageWriter<SaveRequest>,
    mut inventory: ResMut<Inventory>,
    player: Query<&Player>,
    focused: Res<super::widgets::FocusedField>,
) {
    let in_game = *state.get() == AppState::InGame;
    let active = player.single().is_ok_and(|p| p.active);
    if keys.just_pressed(KeyCode::Escape) {
        // Escape first leaves a text field.
        if focused.0.is_some() {
            return;
        }
        let before = *screen;
        *screen = screen.after_escape(origin.0);
        // Esc opens the pause menu and saves the world, so a crash or quit afterwards loses nothing.
        if in_game && before == Screen::Playing {
            saves.write(SaveRequest::default());
        }
        if before == Screen::Options && !in_game && *screen == Screen::Pause {
            *screen = Screen::Title;
        }
        return;
    }
    if !in_game || focused.0.is_some() {
        return;
    }
    if keys.just_pressed(KeyCode::KeyE) && active {
        match *screen {
            Screen::Playing => {
                inventory.set_craft_size(2);
                *screen = Screen::Inventory;
            }
            Screen::Inventory | Screen::Workbench | Screen::Furnace => *screen = Screen::Playing,
            _ => {}
        }
    }
    // Cheat key: switch between creative and survival while playing.
    if keys.just_pressed(KeyCode::F4) && *screen == Screen::Playing {
        *mode = mode.flip();
    }
}

fn open_death_screen(mut died: MessageReader<PlayerDied>, mut screen: ResMut<Screen>) {
    if died.read().next().is_some() {
        *screen = Screen::Death;
    }
}

/// Leaving the inventory, a crafting table or a furnace returns what is lying in the crafting
/// grid and on the cursor; anything that does not fit is tossed out in front of the player.
fn close_containers(
    screen: Res<Screen>,
    mut previous: Local<Screen>,
    mut inventory: ResMut<Inventory>,
    cam: Query<&GlobalTransform, With<PlayerCamera>>,
    mut drops: DropSpawner,
) {
    if *previous == *screen {
        return;
    }
    let left = *previous;
    *previous = *screen;
    if !left.is_container() || screen.is_container() {
        return;
    }
    inventory.return_cursor();
    let mut overflow = inventory.clear_crafting();
    inventory.set_craft_size(2);
    inventory.close_furnace();
    overflow.extend(inventory.cursor.take());
    if let Ok(cam) = cam.single() {
        for stack in overflow {
            let dir = cam.forward().as_vec3();
            drops.spawn_delayed(stack, cam.translation() + dir * 0.5 - Vec3::Y * 0.3, dir * 4.0 + Vec3::Y * 1.5, 2.0);
        }
    }
}

fn toggle_fullscreen_key(keys: Res<ButtonInput<KeyCode>>, mut settings: ResMut<GameSettings>) {
    if keys.just_pressed(KeyCode::F11) {
        settings.fullscreen = !settings.fullscreen;
    }
}

/// Alt-tabbing away opens the menu instead of leaving the game running unattended.
fn pause_on_focus_loss(
    state: Res<State<AppState>>,
    mut screen: ResMut<Screen>,
    mut focus: MessageReader<WindowFocused>,
    mut was_focused: Local<bool>,
) {
    for f in focus.read() {
        // Only after the window has actually been focused once (a window that never gets focus,
        // e.g. under a bare X server, must not start paused).
        if !f.focused && *was_focused && *screen == Screen::Playing && *state.get() == AppState::InGame {
            *screen = Screen::Pause;
        }
        if f.focused {
            *was_focused = true;
        }
    }
}

fn sync_capture(
    screen: Res<Screen>,
    state: Res<State<AppState>>,
    player: Query<&Player>,
    mut capture: ResMut<InputCapture>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let want = *state.get() == AppState::InGame && *screen == Screen::Playing && player.single().is_ok_and(|p| p.active);
    if capture.0 != want {
        capture.0 = want;
    }
    let Ok(mut cursor) = cursor.single_mut() else { return };
    // The pointer is hidden and locked only while the player looks around.
    let grab = if want { CursorGrabMode::Locked } else { CursorGrabMode::None };
    if cursor.grab_mode != grab {
        cursor.grab_mode = grab;
    }
    if cursor.visible == want {
        cursor.visible = !want;
    }
}

/// The pause menu freezes physics, animations and the day/night clock.
fn sync_time_pause(screen: Res<Screen>, state: Res<State<AppState>>, mut time: ResMut<Time<Virtual>>) {
    let want = *state.get() == AppState::InGame && screen.pauses_game();
    if want != time.is_paused() {
        if want {
            time.pause();
        } else {
            time.unpause();
        }
    }
}

fn apply_window_settings(settings: Res<GameSettings>, mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if !settings.is_changed() {
        return;
    }
    let Ok(mut window) = windows.single_mut() else { return };
    let mode = if settings.fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
    if window.mode != mode {
        window.mode = mode;
    }
    let present = if settings.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
    if window.present_mode != present {
        window.present_mode = present;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_navigates_back_one_level() {
        let home = Screen::Title;
        assert_eq!(Screen::Playing.after_escape(home), Screen::Pause);
        assert_eq!(Screen::Pause.after_escape(home), Screen::Playing);
        assert_eq!(Screen::Inventory.after_escape(home), Screen::Playing);
        assert_eq!(Screen::Workbench.after_escape(home), Screen::Playing);
        assert_eq!(Screen::Furnace.after_escape(home), Screen::Playing);
        assert_eq!(Screen::Options.after_escape(Screen::Pause), Screen::Pause);
        assert_eq!(Screen::Options.after_escape(Screen::Title), Screen::Title);
        assert_eq!(Screen::CreateWorld.after_escape(home), Screen::Worlds);
        assert_eq!(Screen::Worlds.after_escape(home), Screen::Title);
        assert_eq!(Screen::Title.after_escape(home), Screen::Title);
        assert_eq!(Screen::Death.after_escape(home), Screen::Death, "the death screen cannot be dismissed");
    }

    #[test]
    fn the_world_is_frozen_in_menus_but_not_in_containers() {
        assert!(Screen::Pause.pauses_game() && Screen::Options.pauses_game() && Screen::Death.pauses_game());
        assert!(!Screen::Inventory.pauses_game() && !Screen::Playing.pauses_game() && !Screen::Workbench.pauses_game());
        assert!(Screen::Inventory.is_container() && Screen::Furnace.is_container() && !Screen::Pause.is_container());
    }
}
