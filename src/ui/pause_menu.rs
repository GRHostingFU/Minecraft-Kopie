//! The in-game menu (`Esc`): resume, options, save and quit. The options page has a render
//! distance slider (2-32 chunks), mouse sensitivity, FOV and a fullscreen toggle.

use super::screens::Screen;
use super::widgets::*;
use crate::save::{SaveRequest, SaveStatus};
use crate::settings::{
    GameSettings, MAX_FOV, MAX_RENDER_DISTANCE, MAX_SENSITIVITY, MIN_FOV, MIN_RENDER_DISTANCE, MIN_SENSITIVITY,
};
use crate::world::streaming::WorldSettings;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

#[derive(Component)]
struct PauseRoot;

#[derive(Component)]
struct OptionsRoot;

#[derive(Component)]
struct SaveStatusText;

/// The slider currently being dragged (the mouse may leave it while the button is held).
#[derive(Resource, Default)]
struct ActiveSlider(Option<SliderKind>);

pub struct PauseMenuPlugin;

impl Plugin for PauseMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveSlider>().add_systems(PostStartup, spawn_menus).add_systems(
            Update,
            (show_menus, menu_buttons, drag_sliders, update_slider_labels, update_button_labels, update_save_status)
                .chain(),
        );
    }
}

fn overlay(z: i32) -> impl Bundle {
    (
        GlobalZIndex(z),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.62)),
    )
}

fn panel() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(40.0), Val::Px(28.0)),
            row_gap: Val::Px(12.0),
            border: UiRect::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(PANEL_BG),
        BorderColor::all(PANEL_BORDER),
    )
}

fn spawn_menus(mut commands: Commands) {
    commands.spawn((PauseRoot, overlay(20))).with_children(|root| {
        root.spawn(panel()).with_children(|p| {
            p.spawn((
                Text::new("Game Menu"),
                text_font(28.0),
                TextColor(Color::WHITE),
                Node { margin: UiRect::bottom(Val::Px(8.0)), ..default() },
            ));
            spawn_button(p, MenuAction::Resume, "Back to Game", 340.0);
            spawn_button(p, MenuAction::Options, "Options...", 340.0);
            spawn_button(p, MenuAction::Save, "Save World", 340.0);
            spawn_button(p, MenuAction::SaveAndQuit, "Save and Quit", 340.0);
            p.spawn((
                SaveStatusText,
                Text::new(""),
                text_font(14.0),
                TextColor(TEXT_DIM),
                Node { margin: UiRect::top(Val::Px(6.0)), ..default() },
            ));
        });
    });

    commands.spawn((OptionsRoot, overlay(20))).with_children(|root| {
        root.spawn(panel()).with_children(|p| {
            p.spawn((
                Text::new("Options"),
                text_font(28.0),
                TextColor(Color::WHITE),
                Node { margin: UiRect::bottom(Val::Px(8.0)), ..default() },
            ));
            spawn_slider(p, SliderKind::RenderDistance);
            spawn_slider(p, SliderKind::Sensitivity);
            spawn_slider(p, SliderKind::Fov);
            spawn_button(p, MenuAction::ToggleFullscreen, "Fullscreen: OFF", SLIDER_WIDTH);
            spawn_button(p, MenuAction::BackToPause, "Done", SLIDER_WIDTH);
        });
    });
}

fn show_menus(
    screen: Res<Screen>,
    mut pause: Query<&mut Node, (With<PauseRoot>, Without<OptionsRoot>)>,
    mut options: Query<&mut Node, (With<OptionsRoot>, Without<PauseRoot>)>,
) {
    if !screen.is_changed() {
        return;
    }
    let show = |on: bool| if on { Display::Flex } else { Display::None };
    if let Ok(mut n) = pause.single_mut() {
        n.display = show(*screen == Screen::Pause);
    }
    if let Ok(mut n) = options.single_mut() {
        n.display = show(*screen == Screen::Options);
    }
}

/// Which buttons are live on which screen.
fn action_screen(action: MenuAction) -> Screen {
    match action {
        MenuAction::Resume | MenuAction::Options | MenuAction::Save | MenuAction::SaveAndQuit => Screen::Pause,
        MenuAction::ToggleFullscreen | MenuAction::BackToPause => Screen::Options,
    }
}

fn menu_buttons(
    mouse: Res<ButtonInput<MouseButton>>,
    mut screen: ResMut<Screen>,
    mut settings: ResMut<GameSettings>,
    mut saves: MessageWriter<SaveRequest>,
    mut buttons: Query<(&MenuButton, &RelativeCursorPosition, &mut BackgroundColor)>,
) {
    for (MenuButton(action), rcp, mut bg) in &mut buttons {
        let live = *screen == action_screen(*action);
        let hover = live && rcp.cursor_over;
        let want = if hover { BUTTON_BG_HOVER } else { BUTTON_BG };
        if bg.0 != want {
            bg.0 = want;
        }
        if !(hover && mouse.just_pressed(MouseButton::Left)) {
            continue;
        }
        match action {
            MenuAction::Resume => *screen = Screen::Playing,
            MenuAction::Options => *screen = Screen::Options,
            MenuAction::BackToPause => *screen = Screen::Pause,
            MenuAction::Save => {
                saves.write(SaveRequest { then_quit: false });
            }
            MenuAction::SaveAndQuit => {
                saves.write(SaveRequest { then_quit: true });
            }
            MenuAction::ToggleFullscreen => settings.fullscreen = !settings.fullscreen,
        }
    }
}

/// Maps a slider position `0..=1` to the value it controls.
pub fn slider_value(kind: SliderKind, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match kind {
        SliderKind::RenderDistance => {
            (MIN_RENDER_DISTANCE as f32 + t * (MAX_RENDER_DISTANCE - MIN_RENDER_DISTANCE) as f32).round()
        }
        SliderKind::Sensitivity => {
            let v = MIN_SENSITIVITY + t * (MAX_SENSITIVITY - MIN_SENSITIVITY);
            (v / 0.05).round() * 0.05
        }
        SliderKind::Fov => (MIN_FOV + t * (MAX_FOV - MIN_FOV)).round(),
    }
}

/// Inverse of [`slider_value`].
pub fn slider_fraction(kind: SliderKind, value: f32) -> f32 {
    let (lo, hi) = match kind {
        SliderKind::RenderDistance => (MIN_RENDER_DISTANCE as f32, MAX_RENDER_DISTANCE as f32),
        SliderKind::Sensitivity => (MIN_SENSITIVITY, MAX_SENSITIVITY),
        SliderKind::Fov => (MIN_FOV, MAX_FOV),
    };
    ((value - lo) / (hi - lo)).clamp(0.0, 1.0)
}

fn drag_sliders(
    screen: Res<Screen>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut active: ResMut<ActiveSlider>,
    mut settings: ResMut<GameSettings>,
    mut world: ResMut<WorldSettings>,
    sliders: Query<(&Slider, &RelativeCursorPosition)>,
) {
    // A click that is released within the same frame still counts as a press.
    if *screen != Screen::Options || !(mouse.pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Left)) {
        active.0 = None;
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        active.0 = sliders.iter().find(|(_, r)| r.cursor_over).map(|(s, _)| s.0);
    }
    let Some(kind) = active.0 else { return };
    let Some(x) = sliders.iter().find(|(s, _)| s.0 == kind).and_then(|(_, r)| r.normalized).map(|n| n.x + 0.5) else {
        return;
    };
    let v = slider_value(kind, x);
    match kind {
        SliderKind::RenderDistance => {
            if world.render_distance != v as i32 {
                world.render_distance = v as i32;
            }
        }
        SliderKind::Sensitivity => {
            if settings.mouse_sensitivity != v {
                settings.mouse_sensitivity = v;
            }
        }
        SliderKind::Fov => {
            if settings.fov != v {
                settings.fov = v;
            }
        }
    }
}

fn update_slider_labels(
    settings: Res<GameSettings>,
    world: Res<WorldSettings>,
    mut labels: Query<(&SliderLabel, &mut Text)>,
    mut fills: Query<(&SliderFill, &mut Node)>,
) {
    let value = |kind| match kind {
        SliderKind::RenderDistance => world.render_distance as f32,
        SliderKind::Sensitivity => settings.mouse_sensitivity,
        SliderKind::Fov => settings.fov,
    };
    for (SliderLabel(kind), mut text) in &mut labels {
        let label = match kind {
            SliderKind::RenderDistance => format!("Render Distance: {} chunks", world.render_distance),
            SliderKind::Sensitivity => format!("Mouse Sensitivity: {:.0}%", settings.mouse_sensitivity * 100.0),
            SliderKind::Fov => format!("FOV: {:.0}", settings.fov),
        };
        if text.0 != label {
            text.0 = label;
        }
    }
    for (SliderFill(kind), mut node) in &mut fills {
        let w = Val::Percent(slider_fraction(*kind, value(*kind)) * 100.0);
        if node.width != w {
            node.width = w;
        }
    }
}

fn update_button_labels(settings: Res<GameSettings>, mut buttons: Query<(&ButtonLabel, &mut Text)>) {
    for (ButtonLabel(action), mut text) in &mut buttons {
        if *action == MenuAction::ToggleFullscreen {
            let label = format!("Fullscreen: {}", if settings.fullscreen { "ON" } else { "OFF" });
            if text.0 != label {
                text.0 = label;
            }
        }
    }
}

fn update_save_status(status: Res<SaveStatus>, mut text: Query<(&mut Text, &mut TextColor), With<SaveStatusText>>) {
    if let Ok((mut text, mut color)) = text.single_mut() {
        if text.0 != status.message {
            text.0.clone_from(&status.message);
        }
        color.0 = if status.ok || status.message.is_empty() { TEXT_DIM } else { Color::srgb(1.0, 0.4, 0.35) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_distance_slider_covers_2_to_32_in_whole_chunks() {
        assert_eq!(slider_value(SliderKind::RenderDistance, 0.0), 2.0);
        assert_eq!(slider_value(SliderKind::RenderDistance, 1.0), 32.0);
        // Every integer in between is reachable (continuous drag, one-chunk steps).
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..=3000 {
            seen.insert(slider_value(SliderKind::RenderDistance, i as f32 / 3000.0) as i32);
        }
        assert_eq!(seen.len(), 31);
        assert_eq!((*seen.first().unwrap(), *seen.last().unwrap()), (2, 32));
    }

    #[test]
    fn slider_value_and_fraction_roundtrip() {
        for kind in [SliderKind::RenderDistance, SliderKind::Sensitivity, SliderKind::Fov] {
            for i in 0..=20 {
                let v = slider_value(kind, i as f32 / 20.0);
                let back = slider_value(kind, slider_fraction(kind, v));
                assert!((v - back).abs() < 1e-3, "{kind:?}: {v} vs {back}");
            }
        }
        assert_eq!(slider_value(SliderKind::Fov, 5.0), MAX_FOV, "positions beyond the track clamp");
        assert!((slider_value(SliderKind::Sensitivity, 0.5) - 1.05).abs() < 1e-4);
    }

    #[test]
    fn every_button_belongs_to_exactly_one_screen() {
        assert_eq!(action_screen(MenuAction::Resume), Screen::Pause);
        assert_eq!(action_screen(MenuAction::BackToPause), Screen::Options);
    }
}
