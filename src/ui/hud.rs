//! In-game HUD: crosshair, the 9-slot hotbar with its selection frame, hearts, hunger, armor and
//! breath rows, the experience bar, damage and underwater tints and a debug overlay (F3).

use super::font::PixelText;
use super::screens::Screen;
use super::widgets::{SlotView, spawn_slot};
use crate::inventory::{HOTBAR_SLOTS, Inventory, OFFHAND};
use crate::mobs::Mob;
use crate::player::controller::Player;
use crate::player::interaction::{MobTarget, Target};
use crate::rendering::AtlasHandle;
use crate::rendering::atlas::ui_sprite_rect;
use crate::rendering::chunk_mesh::MeshState;
use crate::rendering::sky::TimeOfDay;
use crate::session::{AppState, GameMode};
use crate::survival::{Experience, MAX_AIR, Vitals, armor_points};
use crate::world::block::tile;
use crate::world::streaming::WorldSettings;
use crate::world::{BlockId, BlockQuery, VoxelWorld};
use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct DebugText;

#[derive(Component)]
struct HeldNameText;

/// The white frame around the selected hotbar slot.
#[derive(Component)]
struct SelectionFrame;

/// A row that only exists in survival mode.
#[derive(Component)]
struct VitalsRow;

/// One icon of a row of ten: which row and which position.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Icon {
    Heart(usize),
    Food(usize),
    Armor(usize),
    Bubble(usize),
}

#[derive(Component)]
struct XpFill;

#[derive(Component)]
struct XpLevel;

#[derive(Component)]
struct HurtFlash;

#[derive(Component)]
struct WaterOverlay;

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
                (
                    toggle_hud_visibility,
                    update_hotbar_selection,
                    update_held_name,
                    update_vitals_icons,
                    update_xp,
                    update_overlays,
                    toggle_debug,
                    update_debug_text,
                )
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

pub const SLOT_SIZE: f32 = 48.0;
pub const SLOT_GAP: f32 = 4.0;
const HOTBAR_WIDTH: f32 = HOTBAR_SLOTS as f32 * SLOT_SIZE + (HOTBAR_SLOTS - 1) as f32 * SLOT_GAP;
const ICON: f32 = 27.0;
const ICON_STEP: f32 = 24.0;
const HOTBAR_BOTTOM: f32 = 12.0;

fn sprite(atlas: &Handle<Image>, tile_id: u8) -> ImageNode {
    ImageNode { image: atlas.clone(), rect: Some(ui_sprite_rect(tile_id)), image_mode: NodeImageMode::Stretch, ..default() }
}

fn icon_row(parent: &mut ChildSpawnerCommands, atlas: &Handle<Image>, make: impl Fn(usize) -> Icon, tile_id: u8, reverse: bool) {
    for i in 0..10 {
        let slot = if reverse { 9 - i } else { i };
        parent.spawn((
            make(slot),
            sprite(atlas, tile_id),
            Node { width: Val::Px(ICON), height: Val::Px(ICON), margin: UiRect::right(Val::Px(ICON_STEP - ICON)), flex_shrink: 0.0, ..default() },
        ));
    }
}

fn spawn_hud(mut commands: Commands, atlas: Res<AtlasHandle>) {
    let atlas = atlas.0.clone();
    commands
        .spawn((
            HudRoot,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            Visibility::Hidden,
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

            // Hotbar, centred, with the selection frame on top.
            root.spawn(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(HOTBAR_BOTTOM),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|wrap| {
                wrap.spawn(Node { width: Val::Px(HOTBAR_WIDTH), height: Val::Px(SLOT_SIZE), column_gap: Val::Px(SLOT_GAP), ..default() })
                    .with_children(|row| {
                        for i in 0..HOTBAR_SLOTS {
                            spawn_slot(row, &atlas, i, SLOT_SIZE, false);
                        }
                        row.spawn((
                            SelectionFrame,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(-4.0),
                                top: Val::Px(-4.0),
                                width: Val::Px(SLOT_SIZE + 8.0),
                                height: Val::Px(SLOT_SIZE + 8.0),
                                border: UiRect::all(Val::Px(4.0)),
                                ..default()
                            },
                            BorderColor::all(Color::WHITE),
                        ));
                        // The off-hand slot sits to the left of the hotbar and only shows when used.
                        row.spawn(Node { position_type: PositionType::Absolute, left: Val::Px(-SLOT_SIZE - 14.0), ..default() })
                            .with_children(|off| {
                                spawn_slot(off, &atlas, OFFHAND, SLOT_SIZE, false);
                            });
                    });
            });

            // Hearts (left) and food (right) above the hotbar; armor and breath one row higher.
            let row_node = |bottom: f32| Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(bottom),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            };
            let inner = || Node { width: Val::Px(HOTBAR_WIDTH), justify_content: JustifyContent::SpaceBetween, ..default() };
            root.spawn((VitalsRow, row_node(HOTBAR_BOTTOM + SLOT_SIZE + 38.0))).with_children(|wrap| {
                wrap.spawn(inner()).with_children(|row| {
                    row.spawn(Node::default()).with_children(|hearts| icon_row(hearts, &atlas, Icon::Heart, tile::UI_HEART_FULL, false));
                    row.spawn(Node::default()).with_children(|food| icon_row(food, &atlas, Icon::Food, tile::UI_FOOD_FULL, true));
                });
            });
            root.spawn((VitalsRow, row_node(HOTBAR_BOTTOM + SLOT_SIZE + 38.0 + ICON + 3.0))).with_children(|wrap| {
                wrap.spawn(inner()).with_children(|row| {
                    row.spawn(Node::default()).with_children(|armor| icon_row(armor, &atlas, Icon::Armor, tile::UI_ARMOR_EMPTY, false));
                    row.spawn(Node::default()).with_children(|air| icon_row(air, &atlas, Icon::Bubble, tile::UI_BUBBLE, true));
                });
            });

            // Experience bar and level.
            root.spawn((VitalsRow, row_node(HOTBAR_BOTTOM + SLOT_SIZE + 12.0))).with_children(|wrap| {
                wrap.spawn((
                    Node { width: Val::Px(HOTBAR_WIDTH), height: Val::Px(12.0), border: UiRect::all(Val::Px(2.0)), ..default() },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
                    BorderColor::all(Color::srgb(0.12, 0.12, 0.12)),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        XpFill,
                        Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() },
                        BackgroundColor(Color::srgb(0.45, 0.95, 0.2)),
                    ));
                });
            });
            root.spawn((VitalsRow, row_node(HOTBAR_BOTTOM + SLOT_SIZE + 26.0 + ICON * 2.0 + 10.0))).with_children(|wrap| {
                wrap.spawn((XpLevel, PixelText::new("").scale(3.0).color(Color::srgb(0.5, 1.0, 0.25))));
            });

            // The item name floats above everything.
            root.spawn(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(HOTBAR_BOTTOM + SLOT_SIZE + 38.0 + ICON * 2.0 + 44.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|wrap| {
                wrap.spawn((HeldNameText, PixelText::new("").scale(3.0)));
            });

            // Full-screen tints.
            let full = || Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
            root.spawn((WaterOverlay, full(), BackgroundColor(Color::srgba(0.1, 0.25, 0.6, 0.0))));
            root.spawn((HurtFlash, full(), BackgroundColor(Color::srgba(0.8, 0.0, 0.0, 0.0))));
        });

    commands.spawn((
        DebugText,
        PixelText::new("").scale(2.0),
        Node { position_type: PositionType::Absolute, left: Val::Px(8.0), top: Val::Px(6.0), ..default() },
        GlobalZIndex(1),
        Visibility::Hidden,
    ));
}

fn toggle_hud_visibility(
    screen: Res<Screen>,
    mode: Res<GameMode>,
    inventory: Res<Inventory>,
    mut root: Query<&mut Visibility, With<HudRoot>>,
    mut rows: Query<&mut Node, With<VitalsRow>>,
    mut offhand: Query<(&SlotView, &mut Visibility), Without<HudRoot>>,
) {
    if let Ok(mut v) = root.single_mut() {
        let want = if *screen == Screen::Playing { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    // Hearts, hunger and experience only matter in survival.
    let show = if *mode == GameMode::Survival { Display::Flex } else { Display::None };
    for mut n in &mut rows {
        if n.display != show {
            n.display = show;
        }
    }
    for (slot, mut v) in &mut offhand {
        if slot.0 == OFFHAND {
            let want = if inventory.slots[OFFHAND].is_some() { Visibility::Inherited } else { Visibility::Hidden };
            if *v != want {
                *v = want;
            }
        }
    }
}

/// Moves the selection frame to the chosen slot.
fn update_hotbar_selection(inventory: Res<Inventory>, mut frame: Query<&mut Node, With<SelectionFrame>>) {
    if !inventory.is_changed() {
        return;
    }
    if let Ok(mut n) = frame.single_mut() {
        let left = Val::Px(inventory.selected as f32 * (SLOT_SIZE + SLOT_GAP) - 4.0);
        if n.left != left {
            n.left = left;
        }
    }
}

/// Briefly shows the name of the item that was just selected.
fn update_held_name(
    time: Res<Time>,
    inventory: Res<Inventory>,
    mut timer: ResMut<HeldNameTimer>,
    mut last: Local<Option<crate::items::Item>>,
    mut text: Query<&mut PixelText, With<HeldNameText>>,
) {
    let Ok(mut text) = text.single_mut() else { return };
    let held = inventory.held_item();
    if *last != held {
        *last = held;
        timer.0 = 2.0;
        PixelText::set(&mut text, &held.map_or(String::new(), |i| i.name()));
    }
    timer.0 = (timer.0 - time.delta_secs()).max(0.0);
    let alpha = timer.0.min(0.5) * 2.0;
    let color = Color::srgba(1.0, 1.0, 1.0, alpha);
    if text.color != color {
        text.color = color;
    }
}

/// Which sprite a row of ten icons shows for `value` (two points per icon): 0 full, 1 half, 2 empty.
pub fn icon_state(value: f32, index: usize) -> u8 {
    let left = value - 2.0 * index as f32;
    if left >= 2.0 {
        0
    } else if left >= 1.0 {
        1
    } else {
        2
    }
}

fn update_vitals_icons(vitals: Res<Vitals>, inventory: Res<Inventory>, mut icons: Query<(&Icon, &mut ImageNode, &mut Node)>) {
    if !vitals.is_changed() && !inventory.is_changed() {
        return;
    }
    let armor = armor_points(&inventory) as f32;
    let bubbles = (vitals.air / (MAX_AIR / 10.0)).ceil();
    let show_bubbles = vitals.air < MAX_AIR - 0.05;
    for (icon, mut image, mut node) in &mut icons {
        let (state, base, visible) = match *icon {
            Icon::Heart(i) => (icon_state(vitals.health, i), tile::UI_HEART_FULL, true),
            Icon::Food(i) => (icon_state(vitals.food, i), tile::UI_FOOD_FULL, true),
            Icon::Armor(i) => (icon_state(armor, i), tile::UI_ARMOR_FULL, armor > 0.0),
            Icon::Bubble(i) => (0, tile::UI_BUBBLE, show_bubbles && (i as f32) < bubbles),
        };
        let rect = Some(ui_sprite_rect(base + state));
        if image.rect != rect {
            image.rect = rect;
        }
        let want = if visible { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

fn update_xp(xp: Res<Experience>, mut fill: Query<&mut Node, With<XpFill>>, mut level: Query<&mut PixelText, With<XpLevel>>) {
    if let Ok(mut n) = fill.single_mut() {
        let w = Val::Percent(xp.progress() * 100.0);
        if n.width != w {
            n.width = w;
        }
    }
    if let Ok(mut t) = level.single_mut() {
        let l = xp.level();
        PixelText::set(&mut t, &if l > 0 { l.to_string() } else { String::new() });
    }
}

fn update_overlays(
    vitals: Res<Vitals>,
    world: Res<VoxelWorld>,
    player: Query<(&Player, &Transform)>,
    mut flash: Query<&mut BackgroundColor, (With<HurtFlash>, Without<WaterOverlay>)>,
    mut water: Query<&mut BackgroundColor, (With<WaterOverlay>, Without<HurtFlash>)>,
) {
    if let Ok(mut c) = flash.single_mut() {
        let a = (vitals.hurt_flash / 0.45).clamp(0.0, 1.0) * 0.38;
        let want = Color::srgba(0.8, 0.0, 0.0, a);
        if c.0 != want {
            c.0 = want;
        }
    }
    if let (Ok(mut c), Ok((p, t))) = (water.single_mut(), player.single()) {
        let eye = (t.translation + Vec3::Y * p.eye_height).floor().as_ivec3();
        let under = world.block(eye) == Some(BlockId::WATER);
        let want = Color::srgba(0.1, 0.25, 0.6, if under { 0.32 } else { 0.0 });
        if c.0 != want {
            c.0 = want;
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
    screen: Res<Screen>,
    mut fps: ResMut<SmoothedFps>,
    player: Query<(&Player, &Transform)>,
    world: Res<VoxelWorld>,
    meshes: Res<MeshState>,
    tod: Res<TimeOfDay>,
    target: Res<Target>,
    mob_target: Res<MobTarget>,
    mobs: Query<&Mob>,
    inventory: Res<Inventory>,
    settings: Res<WorldSettings>,
    mode: Res<GameMode>,
    mut text: Query<(&mut PixelText, &mut Visibility), With<DebugText>>,
    mut refresh: Local<f32>,
) {
    let Ok((mut text, mut vis)) = text.single_mut() else { return };
    let dt = time.delta_secs().max(1e-4);
    fps.0 = if fps.0 == 0.0 { 1.0 / dt } else { fps.0 * 0.95 + 0.05 / dt };
    let show = visible.0 && *screen == Screen::Playing;
    let want = if show { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    // Re-rendering the text image ten times a second is plenty.
    *refresh -= dt;
    if !show || *refresh > 0.0 {
        return;
    }
    *refresh = 0.1;
    let Ok((p, t)) = player.single() else { return };
    let s = meshes.stats;
    let hours = tod.t * 24.0;
    let looking = match (mob_target.0.and_then(|(e, _)| mobs.get(e).ok()), target.0) {
        (Some(m), _) => m.kind.name(),
        (None, Some(h)) => world.block_at(h.block).name(),
        _ => "-",
    };
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
    let held = inventory.held_item().map_or("nothing".to_string(), |i| i.name());
    let loading = if p.active { "" } else { "(generating terrain...)" };
    let shown = format!(
        "{:.0} fps   render distance {}   {}\nXYZ {:.1} {:.1} {:.1}\nchunks {} loaded, {} meshed ({} quads), {} queued, {} meshing\ntime {:02}:{:02}  {}{}  mobs {}\nlooking at {}   holding {}",
        fps.0,
        settings.render_distance,
        mode.label(),
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
        loading,
        mobs.iter().count(),
        looking,
        held,
    );
    PixelText::set(&mut text, &shown);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_of_ten_icons_fill_from_the_left() {
        // Seven points are three full icons and a half one.
        let states: Vec<u8> = (0..10).map(|i| icon_state(7.0, i)).collect();
        assert_eq!(states, [0, 0, 0, 1, 2, 2, 2, 2, 2, 2]);
        assert!((0..10).all(|i| icon_state(20.0, i) == 0));
        assert!((0..10).all(|i| icon_state(0.0, i) == 2));
        assert_eq!(icon_state(1.0, 0), 1, "one point is a half icon");
    }

    #[test]
    fn hotbar_layout_numbers_add_up() {
        assert_eq!(HOTBAR_WIDTH, 9.0 * 48.0 + 8.0 * 4.0);
        assert!(ICON_STEP < ICON);
    }
}
