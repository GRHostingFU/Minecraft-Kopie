//! In-game HUD: crosshair, 9-slot hotbar and a small debug overlay (toggle with F3).

use super::screens::Screen;
use super::widgets::{SlotView, TEXT_DIM, spawn_slot, text_font};
use crate::inventory::{HOTBAR_SLOTS, Inventory};
use crate::player::controller::Player;
use crate::player::interaction::Target;
use crate::rendering::AtlasHandle;
use crate::rendering::chunk_mesh::MeshState;
use crate::rendering::sky::TimeOfDay;
use crate::world::VoxelWorld;
use crate::world::streaming::WorldSettings;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct DebugText;

#[derive(Component)]
struct HeldNameText;

#[derive(Resource)]
struct DebugVisible(bool);

#[derive(Resource, Default)]
struct SmoothedFps(f32);

/// How long the name of a newly selected item stays on screen.
#[derive(Resource, Default)]
struct HeldNameTimer(f32);

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DebugVisible(true))
            .init_resource::<SmoothedFps>()
            .init_resource::<HeldNameTimer>()
            .add_systems(PostStartup, spawn_hud)
            .add_systems(
                Update,
                (update_hotbar_selection, update_held_name, toggle_hud_visibility, toggle_debug, update_debug_text),
            );
    }
}

const SLOT_SIZE: f32 = 48.0;

fn spawn_hud(mut commands: Commands, atlas: Res<AtlasHandle>) {
    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            Visibility::Inherited,
        ))
        .with_children(|root| {
            // Crosshair: two thin bars.
            let bar = |w: f32, h: f32| Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                width: Val::Px(w),
                height: Val::Px(h),
                margin: UiRect { left: Val::Px(-w / 2.0), top: Val::Px(-h / 2.0), ..default() },
                ..default()
            };
            root.spawn((bar(20.0, 2.0), BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8))));
            root.spawn((bar(2.0, 20.0), BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8))));

            root.spawn(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(14.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|row| {
                for i in 0..HOTBAR_SLOTS {
                    spawn_slot(row, &atlas.0, i, SLOT_SIZE, false);
                }
            });

            // The item name floats above the hotbar; the wrapper centres it.
            root.spawn(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(14.0 + SLOT_SIZE + 12.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|wrap| {
                wrap.spawn((
                    HeldNameText,
                    Text::new(""),
                    text_font(20.0),
                    TextColor(Color::WHITE),
                    TextShadow { offset: Vec2::splat(2.0), color: Color::BLACK },
                ));
            });
        });

    commands.spawn((
        DebugText,
        Text::new(""),
        text_font(16.0),
        TextColor(Color::WHITE),
        TextShadow { offset: Vec2::splat(1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
        Node { position_type: PositionType::Absolute, left: Val::Px(8.0), top: Val::Px(6.0), ..default() },
    ));
    commands.spawn((
        Text::new("WASD move  Space jump  Shift sneak  Ctrl/2xW sprint  E inventory  Esc menu  F fly  F3 debug"),
        text_font(13.0),
        TextColor(TEXT_DIM),
        Node { position_type: PositionType::Absolute, right: Val::Px(8.0), top: Val::Px(6.0), ..default() },
    ));
}

/// Highlights the selected hotbar slot. Only HUD slots are touched; the inventory screen's slots
/// carry a `RelativeCursorPosition` and manage their own colours.
fn update_hotbar_selection(
    inventory: Res<Inventory>,
    mut slots: Query<(&SlotView, &mut BorderColor), Without<RelativeCursorPosition>>,
) {
    if !inventory.is_changed() {
        return;
    }
    for (SlotView(i), mut border) in &mut slots {
        let selected = *i == inventory.selected;
        *border = BorderColor::all(if selected { Color::WHITE } else { Color::srgb(0.30, 0.30, 0.34) });
    }
}

/// Briefly shows the name of the item that was just selected.
fn update_held_name(
    time: Res<Time>,
    inventory: Res<Inventory>,
    mut timer: ResMut<HeldNameTimer>,
    mut last: Local<Option<crate::inventory::Item>>,
    mut text: Query<(&mut Text, &mut TextColor, &mut TextShadow), With<HeldNameText>>,
) {
    let Ok((mut text, mut color, mut shadow)) = text.single_mut() else { return };
    let held = inventory.held_item();
    if *last != held {
        *last = held;
        timer.0 = 2.0;
        text.0 = held.map_or(String::new(), |i| i.name().to_string());
    }
    timer.0 = (timer.0 - time.delta_secs()).max(0.0);
    let alpha = timer.0.min(0.5) * 2.0;
    color.0 = Color::srgba(1.0, 1.0, 1.0, alpha);
    shadow.color = Color::srgba(0.0, 0.0, 0.0, alpha);
}

fn toggle_hud_visibility(screen: Res<Screen>, mut root: Query<&mut Visibility, With<HudRoot>>) {
    if let Ok(mut v) = root.single_mut() {
        let want = if *screen == Screen::Playing { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

fn toggle_debug(keys: Res<ButtonInput<KeyCode>>, mut visible: ResMut<DebugVisible>) {
    if keys.just_pressed(KeyCode::F3) {
        visible.0 = !visible.0;
    }
}

#[allow(clippy::too_many_arguments)]
fn update_debug_text(
    time: Res<Time<Real>>,
    visible: Res<DebugVisible>,
    mut fps: ResMut<SmoothedFps>,
    player: Query<(&Player, &Transform)>,
    world: Res<VoxelWorld>,
    meshes: Res<MeshState>,
    tod: Res<TimeOfDay>,
    target: Res<Target>,
    inventory: Res<Inventory>,
    settings: Res<WorldSettings>,
    mut text: Query<&mut Text, With<DebugText>>,
) {
    let Ok(mut text) = text.single_mut() else { return };
    let dt = time.delta_secs().max(1e-4);
    fps.0 = if fps.0 == 0.0 { 1.0 / dt } else { fps.0 * 0.95 + 0.05 / dt };
    if !visible.0 {
        text.0.clear();
        return;
    }
    let Ok((p, t)) = player.single() else { return };
    let s = meshes.stats;
    let hours = tod.t * 24.0;
    let looking = target.0.map(|h| world.block_at(h.block).name()).unwrap_or("-");
    let state = if p.flying {
        "flying "
    } else if p.in_water {
        "swimming "
    } else if p.sneaking {
        "sneaking "
    } else if p.sprinting {
        "sprinting "
    } else {
        ""
    };
    text.0 = format!(
        "{:.0} fps   render distance {}\nXYZ {:.1} {:.1} {:.1}\nchunks {} loaded, {} meshed ({} quads), {} queued, {} meshing\ntime {:02}:{:02}  {}{}\nlooking at {}   holding {}",
        fps.0,
        settings.render_distance,
        t.translation.x,
        t.translation.y,
        t.translation.z,
        world.loaded_count(),
        s.chunks,
        s.quads,
        s.pending,
        s.in_flight,
        hours as u32,
        ((hours.fract()) * 60.0) as u32,
        state,
        if p.active { "" } else { "(generating terrain...)" },
        looking,
        inventory.held_item().map_or("nothing", |i| i.name()),
    );
}
