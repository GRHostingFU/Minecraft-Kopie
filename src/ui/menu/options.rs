//! The options page (video, controls, audio) shared by the title screen and the pause menu.
//! Every control writes straight into [`GameSettings`] / [`WorldSettings`], so changes are live,
//! and a changed value is saved to `options.json` shortly afterwards.

use super::{panel, spawn_page};
use crate::settings::{self, GameSettings, OptionsFile, SAVES_DIR};
use crate::ui::font::PixelText;
use crate::ui::screens::{OptionsOrigin, Screen};
use crate::ui::widgets::{
    ActionLabel, OptionsTab, SLIDER_WIDTH, SliderKind, UiAction, UiClick, spawn_button, spawn_slider,
};
use crate::world::streaming::WorldSettings;
use bevy::prelude::*;
use std::path::PathBuf;

#[derive(Resource)]
pub struct CurrentTab(pub OptionsTab);

#[derive(Component)]
struct TabContent(OptionsTab);

/// The text a button with `action` shows right now (`None` for buttons with a fixed label).
pub fn label_for(action: UiAction, s: &GameSettings) -> Option<String> {
    let on = |b: bool| if b { "ON" } else { "OFF" };
    Some(match action {
        UiAction::ToggleFullscreen => format!("Fullscreen: {}", on(s.fullscreen)),
        UiAction::ToggleVsync => format!("VSync: {}", on(s.vsync)),
        UiAction::CycleParticles => format!("Particles: {}", s.particles.label()),
        UiAction::CycleSprintMode => format!("Sprint: {}", s.sprint_mode.label()),
        UiAction::CycleSneakMode => format!("Sneak: {}", s.sneak_mode.label()),
        UiAction::ToggleInvertMouse => format!("Invert Mouse: {}", on(s.invert_mouse)),
        _ => return None,
    })
}

/// Applies a click on one of the option buttons.
pub fn apply_action(action: UiAction, s: &mut GameSettings) {
    match action {
        UiAction::ToggleFullscreen => s.fullscreen = !s.fullscreen,
        UiAction::ToggleVsync => s.vsync = !s.vsync,
        UiAction::CycleParticles => s.particles = s.particles.next(),
        UiAction::CycleSprintMode => s.sprint_mode = s.sprint_mode.flip(),
        UiAction::CycleSneakMode => s.sneak_mode = s.sneak_mode.flip(),
        UiAction::ToggleInvertMouse => s.invert_mouse = !s.invert_mouse,
        _ => {}
    }
}

pub struct OptionsPlugin;

impl Plugin for OptionsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CurrentTab(OptionsTab::Video))
            .add_systems(Startup, spawn_options)
            .add_systems(Update, (option_clicks, update_labels, show_tab, persist_options).chain());
    }
}

fn spawn_options(mut commands: Commands) {
    let page = spawn_page(&mut commands, Screen::Options, 0.62, 40);
    commands.entity(page).with_children(|root| {
        root.spawn(panel()).with_children(|p| {
            p.spawn((PixelText::new("Options").scale(3.0), Node { margin: UiRect::bottom(Val::Px(4.0)), ..default() }));
            p.spawn(Node { column_gap: Val::Px(6.0), ..default() }).with_children(|tabs| {
                for (tab, name) in [(OptionsTab::Video, "Video"), (OptionsTab::Controls, "Controls"), (OptionsTab::Audio, "Audio")] {
                    spawn_button(tabs, Screen::Options, UiAction::Tab(tab), name, (SLIDER_WIDTH - 12.0) / 3.0);
                }
            });
            let content = || Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(8.0), min_height: Val::Px(292.0), margin: UiRect::vertical(Val::Px(6.0)), ..default() };
            p.spawn((TabContent(OptionsTab::Video), content())).with_children(|c| {
                spawn_slider(c, Screen::Options, SliderKind::RenderDistance);
                spawn_slider(c, Screen::Options, SliderKind::Fov);
                spawn_button(c, Screen::Options, UiAction::ToggleFullscreen, "Fullscreen: OFF", SLIDER_WIDTH);
                spawn_button(c, Screen::Options, UiAction::ToggleVsync, "VSync: ON", SLIDER_WIDTH);
                spawn_button(c, Screen::Options, UiAction::CycleParticles, "Particles: All", SLIDER_WIDTH);
            });
            p.spawn((TabContent(OptionsTab::Controls), content())).with_children(|c| {
                spawn_slider(c, Screen::Options, SliderKind::Sensitivity);
                spawn_button(c, Screen::Options, UiAction::CycleSprintMode, "Sprint: Hold", SLIDER_WIDTH);
                spawn_button(c, Screen::Options, UiAction::CycleSneakMode, "Sneak: Hold", SLIDER_WIDTH);
                spawn_button(c, Screen::Options, UiAction::ToggleInvertMouse, "Invert Mouse: OFF", SLIDER_WIDTH);
                c.spawn((
                    PixelText::new("WASD move, Space jump, Ctrl/2xW sprint, Shift sneak\nE inventory, Q drop, X swap hands, F fly (creative)").color(crate::ui::widgets::TEXT_DIM),
                    Node { margin: UiRect::top(Val::Px(6.0)), ..default() },
                ));
            });
            p.spawn((TabContent(OptionsTab::Audio), content())).with_children(|c| {
                spawn_slider(c, Screen::Options, SliderKind::MasterVolume);
                spawn_slider(c, Screen::Options, SliderKind::MusicVolume);
                spawn_slider(c, Screen::Options, SliderKind::WeatherVolume);
                c.spawn((
                    PixelText::new("The sound engine is not part of this build yet;\nthe volumes are stored for it.").color(crate::ui::widgets::TEXT_DIM),
                    Node { margin: UiRect::top(Val::Px(10.0)), ..default() },
                ));
            });
            spawn_button(p, Screen::Options, UiAction::OptionsDone, "Done", SLIDER_WIDTH);
        });
    });
}

fn option_clicks(
    mut clicks: MessageReader<UiClick>,
    mut settings: ResMut<GameSettings>,
    mut tab: ResMut<CurrentTab>,
    mut screen: ResMut<Screen>,
    origin: Res<OptionsOrigin>,
) {
    for UiClick(action) in clicks.read() {
        match *action {
            UiAction::Tab(t) => tab.0 = t,
            UiAction::OptionsDone => *screen = origin.0,
            other => {
                let mut next = *settings;
                apply_action(other, &mut next);
                if next != *settings {
                    *settings = next;
                }
            }
        }
    }
}

fn update_labels(settings: Res<GameSettings>, mut labels: Query<(&ActionLabel, &mut PixelText)>) {
    for (label, mut text) in &mut labels {
        if let Some(s) = label_for(label.0, &settings) {
            PixelText::set(&mut text, &s);
        }
    }
}

fn show_tab(tab: Res<CurrentTab>, mut content: Query<(&TabContent, &mut Node)>) {
    if !tab.is_changed() {
        return;
    }
    for (c, mut node) in &mut content {
        let want = if c.0 == tab.0 { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

/// Writes `options.json` a moment after the last change.
fn persist_options(
    time: Res<Time<Real>>,
    settings: Res<GameSettings>,
    world: Res<WorldSettings>,
    mut last: Local<Option<OptionsFile>>,
    mut pending: Local<Option<f32>>,
) {
    let current = OptionsFile { render_distance: world.render_distance, settings: *settings };
    match *last {
        None => *last = Some(current),
        Some(saved) if saved != current => {
            // Wait for the sliders to settle before touching the disk.
            *pending = Some(time.elapsed_secs() + 0.6);
            *last = Some(current);
        }
        _ => {}
    }
    if let Some(at) = *pending
        && time.elapsed_secs() >= at
    {
        *pending = None;
        if let Err(e) = settings::save_options(&PathBuf::from(SAVES_DIR), &current) {
            warn!("cannot save options: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{KeyMode, ParticleLevel};

    #[test]
    fn every_option_button_has_a_live_label() {
        let mut s = GameSettings::default();
        for a in [
            UiAction::ToggleFullscreen,
            UiAction::ToggleVsync,
            UiAction::CycleParticles,
            UiAction::CycleSprintMode,
            UiAction::CycleSneakMode,
            UiAction::ToggleInvertMouse,
        ] {
            let before = label_for(a, &s).expect("label");
            apply_action(a, &mut s);
            let after = label_for(a, &s).unwrap();
            assert_ne!(before, after, "{a:?} changes its label when clicked");
        }
        assert_eq!(label_for(UiAction::Singleplayer, &s), None);
    }

    #[test]
    fn labels_follow_the_settings() {
        let s = GameSettings { fullscreen: true, vsync: false, particles: ParticleLevel::Minimal, sprint_mode: KeyMode::Toggle, ..default() };
        assert_eq!(label_for(UiAction::ToggleFullscreen, &s).unwrap(), "Fullscreen: ON");
        assert_eq!(label_for(UiAction::ToggleVsync, &s).unwrap(), "VSync: OFF");
        assert_eq!(label_for(UiAction::CycleParticles, &s).unwrap(), "Particles: Minimal");
        assert_eq!(label_for(UiAction::CycleSprintMode, &s).unwrap(), "Sprint: Toggle");
        assert_eq!(label_for(UiAction::CycleSneakMode, &s).unwrap(), "Sneak: Hold");
    }

    #[test]
    fn particles_cycle_through_all_levels() {
        let mut s = GameSettings::default();
        let seen: Vec<_> = (0..3)
            .map(|_| {
                apply_action(UiAction::CycleParticles, &mut s);
                s.particles
            })
            .collect();
        assert_eq!(seen, [ParticleLevel::Decreased, ParticleLevel::Minimal, ParticleLevel::All]);
    }
}
