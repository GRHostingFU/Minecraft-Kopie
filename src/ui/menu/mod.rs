//! The main menu and everything reachable from it: title screen, world list, world creation and
//! the options (which the pause menu reuses).

pub mod options;
pub mod title;
pub mod worlds;

use super::screens::{Screen, ScreensSet};
use bevy::prelude::*;

/// A full-screen page that is shown while its screen is open.
#[derive(Component)]
pub struct PageRoot(pub Screen);

/// Spawns the root node of a page, hidden until its screen opens. `dim` darkens what is behind it.
pub fn spawn_page(commands: &mut Commands, screen: Screen, dim: f32, z: i32) -> Entity {
    commands
        .spawn((
            PageRoot(screen),
            GlobalZIndex(z),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, dim)),
        ))
        .id()
}

/// A bordered panel container.
pub fn panel() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(36.0), Val::Px(24.0)),
            row_gap: Val::Px(10.0),
            border: UiRect::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(super::widgets::PANEL_BG),
        BorderColor::all(super::widgets::PANEL_BORDER),
    )
}

pub fn show_pages(screen: Res<Screen>, mut pages: Query<(&PageRoot, &mut Node)>) {
    if !screen.is_changed() {
        return;
    }
    for (page, mut node) in &mut pages {
        let want = if page.0 == *screen { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((title::TitlePlugin, worlds::WorldsPlugin, options::OptionsPlugin))
            .add_systems(Update, show_pages.after(ScreensSet));
    }
}
