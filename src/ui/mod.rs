//! HUD: crosshair, hotbar and a small debug overlay (toggle with F3).

use crate::player::controller::Player;
use crate::player::interaction::Target;
use crate::player::{HOTBAR_SLOTS, Hotbar};
use crate::rendering::AtlasHandle;
use crate::rendering::atlas::tile_rect;
use crate::rendering::chunk_mesh::MeshState;
use crate::rendering::sky::TimeOfDay;
use crate::world::VoxelWorld;
use crate::world::block::Face;
use bevy::prelude::*;
use bevy::text::FontSize;

#[derive(Component)]
struct HotbarSlot(usize);

#[derive(Component)]
struct DebugText;

#[derive(Resource)]
struct DebugVisible(bool);

#[derive(Resource, Default)]
struct SmoothedFps(f32);

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DebugVisible(true))
            .init_resource::<SmoothedFps>()
            .add_systems(PostStartup, spawn_ui)
            .add_systems(Update, (update_hotbar, toggle_debug, update_debug_text));
    }
}

const SLOT_SIZE: f32 = 56.0;

fn spawn_ui(mut commands: Commands, atlas: Res<AtlasHandle>, hotbar: Res<Hotbar>) {
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
    commands.spawn((bar(20.0, 2.0), BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8))));
    commands.spawn((bar(2.0, 20.0), BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8))));

    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(14.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            for i in 0..HOTBAR_SLOTS {
                let block = hotbar.slots[i];
                let rect = tile_rect(block.tile(Face::Side));
                row.spawn((
                    HotbarSlot(i),
                    Node {
                        width: Val::Px(SLOT_SIZE),
                        height: Val::Px(SLOT_SIZE),
                        border: UiRect::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                    BorderColor::all(Color::srgba(0.2, 0.2, 0.2, 0.9)),
                ))
                .with_children(|slot| {
                    slot.spawn((
                        Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                        ImageNode { rect: Some(rect), ..ImageNode::new(atlas.0.clone()) },
                    ));
                });
            }
        });

    commands.spawn((
        DebugText,
        Text::new(""),
        TextFont { font_size: FontSize::Px(16.0), ..default() },
        TextColor(Color::WHITE),
        Node { position_type: PositionType::Absolute, left: Val::Px(8.0), top: Val::Px(6.0), ..default() },
    ));
    commands.spawn((
        Text::new("WASD move  Space jump  F fly  Ctrl sprint  T time-lapse  1-5/wheel select  LMB break  RMB place  F3 debug  Esc release mouse"),
        TextFont { font_size: FontSize::Px(13.0), ..default() },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
        Node { position_type: PositionType::Absolute, right: Val::Px(8.0), top: Val::Px(6.0), ..default() },
    ));
}

fn update_hotbar(hotbar: Res<Hotbar>, mut slots: Query<(&HotbarSlot, &mut BorderColor)>) {
    if !hotbar.is_changed() {
        return;
    }
    for (slot, mut border) in &mut slots {
        *border = if slot.0 == hotbar.selected {
            BorderColor::all(Color::WHITE)
        } else {
            BorderColor::all(Color::srgba(0.2, 0.2, 0.2, 0.9))
        };
    }
}

fn toggle_debug(keys: Res<ButtonInput<KeyCode>>, mut visible: ResMut<DebugVisible>) {
    if keys.just_pressed(KeyCode::F3) {
        visible.0 = !visible.0;
    }
}

#[allow(clippy::too_many_arguments)]
fn update_debug_text(
    time: Res<Time>,
    visible: Res<DebugVisible>,
    mut fps: ResMut<SmoothedFps>,
    player: Query<(&Player, &Transform)>,
    world: Res<VoxelWorld>,
    meshes: Res<MeshState>,
    tod: Res<TimeOfDay>,
    target: Res<Target>,
    hotbar: Res<Hotbar>,
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
    text.0 = format!(
        "{:.0} fps\nXYZ {:.1} {:.1} {:.1}\nchunks {} loaded, {} meshed ({} quads), {} queued, {} meshing\ntime {:02}:{:02}  {}{}\nlooking at {}   holding {}",
        fps.0,
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
        if p.flying {
            "flying "
        } else if p.in_water {
            "swimming "
        } else {
            ""
        },
        if p.active { "" } else { "(generating terrain...)" },
        looking,
        hotbar.current().name(),
    );
}
