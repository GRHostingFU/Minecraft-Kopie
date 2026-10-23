//! The in-game menu (`Esc`) and the death screen.

use super::font::PixelText;
use super::menu::{panel, spawn_page};
use super::screens::{OptionsOrigin, Screen};
use super::widgets::{TEXT_DIM, UiAction, UiClick, spawn_button};
use crate::player::controller::RespawnRequest;
use crate::save::{SaveRequest, SaveStatus};
use crate::session::{AppState, GameMode};
use crate::survival::Experience;
use bevy::prelude::*;

#[derive(Component)]
struct SaveStatusText;

#[derive(Component)]
struct ModeText;

#[derive(Component)]
struct ScoreText;

pub struct PauseMenuPlugin;

impl Plugin for PauseMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_menus).add_systems(
            Update,
            (pause_clicks, update_save_status, update_mode_text, update_score_text).run_if(in_state(AppState::InGame)),
        );
    }
}

fn spawn_menus(mut commands: Commands) {
    let pause = spawn_page(&mut commands, Screen::Pause, 0.6, 40);
    commands.entity(pause).with_children(|root| {
        root.spawn(panel()).with_children(|p| {
            p.spawn((PixelText::new("Game Menu").scale(3.0), Node { margin: UiRect::bottom(Val::Px(6.0)), ..default() }));
            spawn_button(p, Screen::Pause, UiAction::Resume, "Back to Game", 400.0);
            spawn_button(p, Screen::Pause, UiAction::OpenOptions, "Options...", 400.0);
            spawn_button(p, Screen::Pause, UiAction::SaveWorld, "Save World", 400.0);
            spawn_button(p, Screen::Pause, UiAction::SaveAndTitle, "Save and Quit to Title", 400.0);
            p.spawn((ModeText, PixelText::new("").color(TEXT_DIM), Node { margin: UiRect::top(Val::Px(6.0)), ..default() }));
            p.spawn((SaveStatusText, PixelText::new("").color(TEXT_DIM)));
        });
    });

    let death = spawn_page(&mut commands, Screen::Death, 0.0, 45);
    commands.entity(death).insert(BackgroundColor(Color::srgba(0.45, 0.02, 0.02, 0.62))).with_children(|root| {
        root.spawn((PixelText::new("You died!").scale(8.0).color(Color::srgb(1.0, 0.85, 0.85)), Node { margin: UiRect::bottom(Val::Px(4.0)), ..default() }));
        root.spawn((ScoreText, PixelText::new("").scale(3.0), Node { margin: UiRect::bottom(Val::Px(18.0)), ..default() }));
        spawn_button(root, Screen::Death, UiAction::Respawn, "Respawn", 400.0);
        spawn_button(root, Screen::Death, UiAction::DeathToTitle, "Title Screen", 400.0);
    });
}

fn pause_clicks(
    mut clicks: MessageReader<UiClick>,
    mut screen: ResMut<Screen>,
    mut origin: ResMut<OptionsOrigin>,
    mut saves: MessageWriter<SaveRequest>,
    mut respawn: MessageWriter<RespawnRequest>,
) {
    for UiClick(action) in clicks.read() {
        match action {
            UiAction::Resume => *screen = Screen::Playing,
            UiAction::OpenOptions if *screen == Screen::Pause => {
                origin.0 = Screen::Pause;
                *screen = Screen::Options;
            }
            UiAction::SaveWorld => {
                saves.write(SaveRequest::default());
            }
            UiAction::SaveAndTitle | UiAction::DeathToTitle => {
                saves.write(SaveRequest { then_title: true, ..default() });
            }
            UiAction::Respawn => {
                respawn.write(RespawnRequest);
                *screen = Screen::Playing;
            }
            _ => {}
        }
    }
}

fn update_save_status(status: Res<SaveStatus>, mut text: Query<&mut PixelText, With<SaveStatusText>>) {
    if let Ok(mut t) = text.single_mut() {
        PixelText::set(&mut t, &status.message);
        let color = if status.ok || status.message.is_empty() { TEXT_DIM } else { Color::srgb(1.0, 0.4, 0.35) };
        if t.color != color {
            t.color = color;
        }
    }
}

fn update_mode_text(mode: Res<GameMode>, mut text: Query<&mut PixelText, With<ModeText>>) {
    if let Ok(mut t) = text.single_mut() {
        PixelText::set(&mut t, &format!("Game Mode: {}   (F4 switches)", mode.label()));
    }
}

fn update_score_text(xp: Res<Experience>, mut text: Query<&mut PixelText, With<ScoreText>>) {
    if let Ok(mut t) = text.single_mut() {
        PixelText::set(&mut t, &format!("Score: {}", xp.total));
    }
}
