//! All user interface, drawn with the procedural pixel font: the HUD, the inventory and crafting
//! screens, the title screen and its menus, the pause and death screens.

pub mod font;
pub mod hud;
pub mod inventory_screen;
pub mod menu;
pub mod pause_menu;
pub mod screens;
pub mod widgets;

use bevy::prelude::*;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            font::PixelFontPlugin,
            screens::ScreensPlugin,
            widgets::WidgetsPlugin,
            menu::MenuPlugin,
            hud::HudPlugin,
            inventory_screen::InventoryScreenPlugin,
            pause_menu::PauseMenuPlugin,
        ));
    }
}
