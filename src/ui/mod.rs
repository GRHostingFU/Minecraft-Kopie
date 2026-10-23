//! All user interface: the HUD, the inventory screen, the pause/options menu and the
//! screen state machine that decides who receives the mouse.

pub mod hud;
pub mod inventory_screen;
pub mod pause_menu;
pub mod screens;
pub mod widgets;

use bevy::prelude::*;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            screens::ScreensPlugin,
            hud::HudPlugin,
            inventory_screen::InventoryScreenPlugin,
            pause_menu::PauseMenuPlugin,
        ))
        .add_systems(Update, widgets::refresh_slots);
    }
}
