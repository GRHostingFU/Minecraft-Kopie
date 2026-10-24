//! The title screen: big blocky logo, a spinning splash line and the three entry buttons.

use super::spawn_page;
use crate::session::AppState;
use crate::ui::font::{PixelText, text_size};
use crate::ui::screens::{OptionsOrigin, Screen};
use crate::ui::widgets::{TEXT_DIM, TEXT_GOLD, UiAction, UiClick, spawn_button};
use bevy::prelude::*;

const SPLASHES: &[&str] = &[
    "Clean-room!",
    "Made of noise!",
    "Greedy meshing!",
    "No textures were harmed!",
    "Bring a pickaxe!",
    "Procedural!",
    "Dig down (carefully)!",
    "Beware the night!",
    "Written in Rust!",
    "Also try the villages!",
];

#[derive(Component)]
struct Splash {
    phase: f32,
}

/// Draws `text` as a chunky logo: three stacked copies fake a block extrusion.
fn spawn_logo(parent: &mut ChildSpawnerCommands, text: &str, scale: f32, color: Color) {
    let depth = (scale * 0.8).round();
    let size = text_size(text, scale, false) + Vec2::splat(depth * 2.0);
    parent
        .spawn(Node { width: Val::Px(size.x), height: Val::Px(size.y), flex_shrink: 0.0, ..default() })
        .with_children(|logo| {
            for (layer, shade) in [(2.0, 0.22), (1.0, 0.42), (0.0, 1.0)] {
                let off = depth * layer;
                let c = color.to_srgba();
                logo.spawn((
                    PixelText::new(text).scale(scale).no_shadow().color(Color::srgb(
                        c.red * shade,
                        c.green * shade,
                        c.blue * shade,
                    )),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(depth * 2.0 - off),
                        top: Val::Px(depth * 2.0 - off),
                        ..default()
                    },
                ));
            }
        });
}

pub struct TitlePlugin;

impl Plugin for TitlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_title).add_systems(Update, (pulse_splash, title_clicks));
    }
}

fn spawn_title(mut commands: Commands) {
    let page = spawn_page(&mut commands, Screen::Title, 0.18, 30);
    let pick = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as usize)
        % SPLASHES.len();
    commands.entity(page).with_children(|root| {
        root.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            margin: UiRect::bottom(Val::Px(18.0)),
            ..default()
        })
        .with_children(|logo| {
            spawn_logo(logo, "MINECRAFT", 9.0, Color::srgb(0.80, 0.80, 0.84));
            spawn_logo(logo, "KOPIE", 6.0, Color::srgb(0.95, 0.80, 0.30));
            // The splash sits at the right corner of the logo, tilted.
            logo.spawn((
                Splash { phase: 0.0 },
                PixelText::new(SPLASHES[pick]).scale(3.0).color(TEXT_GOLD),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(-120.0),
                    bottom: Val::Px(70.0),
                    ..default()
                },
                UiTransform::from_rotation(Rot2::degrees(-16.0)),
            ));
        });
        spawn_button(root, Screen::Title, UiAction::Singleplayer, "Singleplayer", 420.0);
        spawn_button(root, Screen::Title, UiAction::OpenOptions, "Options...", 420.0);
        spawn_button(root, Screen::Title, UiAction::QuitGame, "Quit Game", 420.0);
        root.spawn((
            PixelText::new("Minecraft Kopie 0.2  -  a clean-room voxel sandbox").scale(2.0).color(TEXT_DIM),
            Node { position_type: PositionType::Absolute, left: Val::Px(10.0), bottom: Val::Px(8.0), ..default() },
        ));
        root.spawn((
            PixelText::new("No original assets, textures or code.").scale(2.0).color(TEXT_DIM),
            Node { position_type: PositionType::Absolute, right: Val::Px(10.0), bottom: Val::Px(8.0), ..default() },
        ));
    });
}

fn pulse_splash(time: Res<Time<Real>>, mut splash: Query<(&mut Splash, &mut UiTransform)>) {
    for (mut s, mut t) in &mut splash {
        s.phase += time.delta_secs() * 4.0;
        let pulse = 1.0 + 0.07 * s.phase.sin();
        t.scale = Vec2::splat(pulse);
    }
}

fn title_clicks(
    mut clicks: MessageReader<UiClick>,
    mut screen: ResMut<Screen>,
    mut origin: ResMut<OptionsOrigin>,
    mut exit: MessageWriter<AppExit>,
    state: Res<State<AppState>>,
) {
    for UiClick(action) in clicks.read() {
        match action {
            UiAction::Singleplayer => *screen = Screen::Worlds,
            UiAction::OpenOptions if *screen == Screen::Title => {
                origin.0 = *screen;
                *screen = Screen::Options;
            }
            UiAction::QuitGame => {
                exit.write(AppExit::Success);
            }
            UiAction::BackToTitle if *state.get() == AppState::MainMenu => *screen = Screen::Title,
            _ => {}
        }
    }
}
