//! The container screens (key `E`, crafting table, furnace): one panel whose sections are shown or
//! hidden depending on what is open — armor, mannequin and 2x2 crafting; the creative block
//! directory with its tabs; the 3x3 crafting table; the furnace — plus the 27 main slots and the
//! hotbar. Mouse handling follows Minecraft: click to pick up / place / swap / merge, right-click
//! to split or place one, shift-click to quick-move, drag across slots to spread a stack.

use super::font::PixelText;
use super::screens::Screen;
use super::widgets::*;
use crate::inventory::crafting::SMELT_SECS;
use crate::inventory::creative::{self, Tab};
use crate::inventory::{
    ARMOR_SLOTS, ARMOR_START, CRAFT_OUT, CRAFT_START, ClickButton, FURNACE_FUEL, FURNACE_IN, FURNACE_OUT, HOTBAR_SLOTS,
    Inventory, OFFHAND,
};
use crate::items::{Item, ItemStack};
use crate::player::controller::PlayerCamera;
use crate::rendering::AtlasHandle;
use crate::rendering::atlas::tile_rect;
use crate::session::{AppState, GameMode};
use crate::survival::drops::DropSpawner;
use crate::world::block::tile;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui::widget::NodeImageMode;
use bevy::window::PrimaryWindow;

const SLOT: f32 = 48.0;
const GAP: f32 = 4.0;
/// Columns and rows of the creative item grid.
const GRID_COLS: usize = 9;
const GRID_ROWS: usize = 5;

#[derive(Component)]
struct InventoryRoot;

/// The bordered panel; clicks outside of it toss the stack on the cursor.
#[derive(Component)]
struct PanelNode;

#[derive(Component)]
struct TitleText;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum Section {
    Tabs,
    SurvivalTop,
    CreativeGrid,
    Workbench,
    Furnace,
    Main,
}

/// The floating copy of the stack that is "in the hand".
#[derive(Component)]
struct CursorStack;

#[derive(Component)]
struct CursorIcon;

#[derive(Component)]
struct CursorCount;

#[derive(Component)]
struct Tooltip;

/// One body part of the mannequin: the armor slot (`0..4`) that recolours it and its normal colour.
#[derive(Component)]
struct MannequinPart {
    slot: usize,
    base: Color,
}

/// A cell of the creative item grid.
#[derive(Component)]
struct CreativeCell(usize);

#[derive(Component)]
struct CreativeCellIcon(usize);

/// Creative tab buttons: `0..5` are the item tabs, `5` is the player's own inventory.
#[derive(Component)]
struct CreativeTabButton(usize);

#[derive(Component)]
struct FlameFill;

#[derive(Component)]
struct ArrowFill;

/// The creative tab currently shown (index into [`creative::tabs`], or `INVENTORY_TAB`).
#[derive(Resource, Default, Debug)]
pub struct CreativeState {
    pub tab: usize,
    /// First visible row of the item grid.
    pub scroll: usize,
}

const INVENTORY_TAB: usize = 5;

/// A drag across slots that started with a stack on the cursor.
#[derive(Resource, Default, Debug)]
struct DragState(Option<Drag>);

#[derive(Debug)]
struct Drag {
    button: ClickButton,
    slots: Vec<usize>,
}

/// Which sections of the panel are visible.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Layout {
    pub tabs: bool,
    pub survival_top: bool,
    pub creative_grid: bool,
    pub workbench: bool,
    pub furnace: bool,
    pub main: bool,
}

pub fn layout(screen: Screen, mode: GameMode, creative_tab: usize) -> Layout {
    match screen {
        Screen::Inventory if mode == GameMode::Creative && creative_tab != INVENTORY_TAB => {
            Layout { tabs: true, creative_grid: true, ..default() }
        }
        Screen::Inventory => Layout { tabs: mode == GameMode::Creative, survival_top: true, main: true, ..default() },
        Screen::Workbench => Layout { workbench: true, main: true, ..default() },
        Screen::Furnace => Layout { furnace: true, main: true, ..default() },
        _ => Layout::default(),
    }
}

pub struct InventoryScreenPlugin;

impl Plugin for InventoryScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DragState>()
            .init_resource::<CreativeState>()
            .add_systems(PostStartup, spawn_inventory_screen)
            .add_systems(
                Update,
                (
                    update_layout,
                    inventory_mouse,
                    creative_input,
                    tick_furnaces,
                    style_slots,
                    update_creative_cells,
                    update_furnace_progress,
                    update_cursor_stack,
                    update_mannequin,
                )
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

fn section_node(display: Display) -> Node {
    Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(GAP), display, ..default() }
}

/// A rows x cols block of slots where cell `(r, c)` maps to inventory slot `index(r, c)`.
fn slot_grid(
    parent: &mut ChildSpawnerCommands,
    atlas: &Handle<Image>,
    rows: usize,
    cols: usize,
    index: impl Fn(usize, usize) -> usize,
) {
    parent.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(GAP), ..default() }).with_children(
        |grid| {
            for r in 0..rows {
                grid.spawn(Node { column_gap: Val::Px(GAP), ..default() }).with_children(|row| {
                    for c in 0..cols {
                        spawn_slot(row, atlas, index(r, c), SLOT, true);
                    }
                });
            }
        },
    );
}

fn atlas_image(atlas: &Handle<Image>, tile_id: u8) -> ImageNode {
    ImageNode { image: atlas.clone(), rect: Some(tile_rect(tile_id)), image_mode: NodeImageMode::Stretch, ..default() }
}

fn spawn_inventory_screen(mut commands: Commands, atlas: Res<AtlasHandle>) {
    let atlas = atlas.0.clone();
    commands
        .spawn((
            InventoryRoot,
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .with_children(|root| {
            root.spawn((
                PanelNode,
                RelativeCursorPosition::default(),
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(18.0)),
                    row_gap: Val::Px(10.0),
                    border: UiRect::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(PANEL_BG),
                BorderColor::all(PANEL_BORDER),
            ))
            .with_children(|panel| {
                panel.spawn((TitleText, PixelText::new("Inventory").scale(3.0)));

                // Creative tabs.
                panel
                    .spawn((Section::Tabs, Node { column_gap: Val::Px(GAP), display: Display::None, ..default() }))
                    .with_children(|tabs| {
                        for i in 0..6 {
                            tabs.spawn((
                                CreativeTabButton(i),
                                RelativeCursorPosition::default(),
                                Node {
                                    width: Val::Px(SLOT),
                                    height: Val::Px(SLOT),
                                    border: UiRect::all(Val::Px(2.0)),
                                    padding: UiRect::all(Val::Px(6.0)),
                                    ..default()
                                },
                                BackgroundColor(SLOT_BG),
                                bevel(false),
                            ))
                            .with_children(|t| {
                                let icon_tile = match creative::tabs().get(i) {
                                    Some(tab) => tab.icon.icon_tile(),
                                    None => tile::CHESTPLATE,
                                };
                                t.spawn((
                                    Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                                    atlas_image(&atlas, icon_tile),
                                ));
                            });
                        }
                    });

                // Armor, mannequin, off hand and the 2x2 crafting grid.
                panel
                    .spawn((
                        Section::SurvivalTop,
                        Node {
                            column_gap: Val::Px(16.0),
                            align_items: AlignItems::Stretch,
                            display: Display::None,
                            ..default()
                        },
                    ))
                    .with_children(|top| {
                        top.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(GAP), ..default() })
                            .with_children(|col| {
                                for i in 0..ARMOR_SLOTS {
                                    spawn_slot(col, &atlas, ARMOR_START + i, SLOT, true);
                                }
                            });
                        spawn_mannequin(top);
                        top.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::FlexEnd,
                            ..default()
                        })
                        .with_children(|col| {
                            spawn_slot(col, &atlas, OFFHAND, SLOT, true);
                        });
                        top.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            row_gap: Val::Px(8.0),
                            margin: UiRect::left(Val::Px(18.0)),
                            ..default()
                        })
                        .with_children(|craft| {
                            craft.spawn(PixelText::new("Crafting").color(TEXT_DIM));
                            craft
                                .spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(14.0), ..default() })
                                .with_children(|row| {
                                    slot_grid(row, &atlas, 2, 2, |r, c| CRAFT_START + r * 3 + c);
                                    spawn_arrow(row, &atlas);
                                    spawn_slot(row, &atlas, CRAFT_OUT, SLOT + 8.0, true);
                                });
                        });
                    });

                // Creative item grid.
                panel.spawn((Section::CreativeGrid, section_node(Display::None))).with_children(|grid| {
                    for r in 0..GRID_ROWS {
                        grid.spawn(Node { column_gap: Val::Px(GAP), ..default() }).with_children(|row| {
                            for c in 0..GRID_COLS {
                                let i = r * GRID_COLS + c;
                                row.spawn((
                                    CreativeCell(i),
                                    RelativeCursorPosition::default(),
                                    Node {
                                        width: Val::Px(SLOT),
                                        height: Val::Px(SLOT),
                                        border: UiRect::all(Val::Px(2.0)),
                                        padding: UiRect::all(Val::Px(SLOT * 0.07)),
                                        ..default()
                                    },
                                    BackgroundColor(SLOT_BG),
                                    bevel(true),
                                ))
                                .with_children(|cell| {
                                    cell.spawn((
                                        CreativeCellIcon(i),
                                        Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                                        ImageNode {
                                            image: atlas.clone(),
                                            image_mode: NodeImageMode::Stretch,
                                            ..default()
                                        },
                                        Visibility::Hidden,
                                    ));
                                });
                            }
                        });
                    }
                    grid.spawn(
                        PixelText::new(
                            "Mouse wheel scrolls   Click: stack   Right click: one   Shift+click: to inventory",
                        )
                        .color(TEXT_DIM),
                    );
                });

                // Crafting table.
                panel
                    .spawn((
                        Section::Workbench,
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(24.0),
                            justify_content: JustifyContent::Center,
                            display: Display::None,
                            padding: UiRect::vertical(Val::Px(14.0)),
                            ..default()
                        },
                    ))
                    .with_children(|wb| {
                        slot_grid(wb, &atlas, 3, 3, |r, c| CRAFT_START + r * 3 + c);
                        spawn_arrow(wb, &atlas);
                        spawn_slot(wb, &atlas, CRAFT_OUT, SLOT + 14.0, true);
                    });

                // Furnace.
                panel
                    .spawn((
                        Section::Furnace,
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(24.0),
                            justify_content: JustifyContent::Center,
                            display: Display::None,
                            padding: UiRect::vertical(Val::Px(8.0)),
                            ..default()
                        },
                    ))
                    .with_children(|f| {
                        f.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: Val::Px(6.0),
                            ..default()
                        })
                        .with_children(|col| {
                            spawn_slot(col, &atlas, FURNACE_IN, SLOT, true);
                            // The flame fills from the bottom as fuel burns.
                            col.spawn(Node { width: Val::Px(36.0), height: Val::Px(52.0), ..default() }).with_children(
                                |flame| {
                                    flame.spawn((
                                        Node {
                                            position_type: PositionType::Absolute,
                                            width: Val::Px(36.0),
                                            height: Val::Px(52.0),
                                            ..default()
                                        },
                                        ImageNode {
                                            rect: Some(flame_rect(tile::UI_FLAME_OFF)),
                                            ..atlas_image(&atlas, tile::UI_FLAME_OFF)
                                        },
                                    ));
                                    flame
                                        .spawn((
                                            FlameFill,
                                            Node {
                                                position_type: PositionType::Absolute,
                                                bottom: Val::Px(0.0),
                                                width: Val::Px(36.0),
                                                height: Val::Px(0.0),
                                                overflow: Overflow::clip(),
                                                ..default()
                                            },
                                        ))
                                        .with_children(|clip| {
                                            clip.spawn((
                                                Node {
                                                    position_type: PositionType::Absolute,
                                                    bottom: Val::Px(0.0),
                                                    width: Val::Px(36.0),
                                                    height: Val::Px(52.0),
                                                    ..default()
                                                },
                                                ImageNode {
                                                    rect: Some(flame_rect(tile::UI_FLAME)),
                                                    ..atlas_image(&atlas, tile::UI_FLAME)
                                                },
                                            ));
                                        });
                                },
                            );
                            spawn_slot(col, &atlas, FURNACE_FUEL, SLOT, true);
                        });
                        spawn_arrow(f, &atlas);
                        spawn_slot(f, &atlas, FURNACE_OUT, SLOT + 14.0, true);
                    });

                // Main inventory, three rows.
                panel.spawn((Section::Main, section_node(Display::None))).with_children(|main| {
                    slot_grid(main, &atlas, 3, HOTBAR_SLOTS, |r, c| HOTBAR_SLOTS + r * HOTBAR_SLOTS + c);
                });

                // Hotbar, separated from the main grid.
                panel.spawn(Node { margin: UiRect::top(Val::Px(4.0)), ..default() }).with_children(|bar| {
                    slot_grid(bar, &atlas, 1, HOTBAR_SLOTS, |_, c| c);
                });
            });
        });

    // Floating stack and tooltip live at the top level so they can leave the panel.
    commands
        .spawn((
            CursorStack,
            GlobalZIndex(50),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(SLOT - 6.0),
                height: Val::Px(SLOT - 6.0),
                display: Display::None,
                ..default()
            },
        ))
        .with_children(|c| {
            c.spawn((
                CursorIcon,
                Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                ImageNode { image: atlas.clone(), image_mode: NodeImageMode::Stretch, ..default() },
            ));
            c.spawn((
                CursorCount,
                PixelText::new(""),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(-2.0),
                    bottom: Val::Px(-2.0),
                    ..default()
                },
            ));
        });
    commands.spawn((
        Tooltip,
        GlobalZIndex(60),
        PixelText::new(""),
        BackgroundColor(Color::srgba(0.12, 0.02, 0.2, 0.94)),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            display: Display::None,
            ..default()
        },
    ));
}

/// The part of a flame tile that holds the flame, scaled up by the node (36 x 52 for 18 x 26).
fn flame_rect(tile_id: u8) -> Rect {
    let o = tile_rect(tile_id).min;
    Rect::from_corners(o + Vec2::new(7.0, 2.0), o + Vec2::new(25.0, 28.0))
}

fn arrow_rect(tile_id: u8) -> Rect {
    let o = tile_rect(tile_id).min;
    Rect::from_corners(o + Vec2::new(5.0, 8.0), o + Vec2::new(27.0, 23.0))
}

const ARROW_W: f32 = 66.0;
const ARROW_H: f32 = 45.0;

/// A grey arrow that fills with white from the left (furnace and crafting progress).
fn spawn_arrow(parent: &mut ChildSpawnerCommands, atlas: &Handle<Image>) {
    parent.spawn(Node { width: Val::Px(ARROW_W), height: Val::Px(ARROW_H), ..default() }).with_children(|a| {
        a.spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(ARROW_W),
                height: Val::Px(ARROW_H),
                ..default()
            },
            ImageNode { rect: Some(arrow_rect(tile::UI_ARROW)), ..atlas_image(atlas, tile::UI_ARROW) },
        ));
        a.spawn((
            ArrowFill,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(0.0),
                height: Val::Px(ARROW_H),
                overflow: Overflow::clip(),
                ..default()
            },
        ))
        .with_children(|clip| {
            clip.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ARROW_W),
                    height: Val::Px(ARROW_H),
                    ..default()
                },
                ImageNode { rect: Some(arrow_rect(tile::UI_ARROW_FULL)), ..atlas_image(atlas, tile::UI_ARROW_FULL) },
            ));
        });
    });
}

const SKIN: Color = Color::srgb(0.80, 0.60, 0.46);
const CLOTH: Color = Color::srgb(0.20, 0.42, 0.62);
const PANTS: Color = Color::srgb(0.25, 0.25, 0.45);
const IRON: Color = Color::srgb(0.74, 0.77, 0.82);

/// A blocky figure made of coloured rectangles (`MannequinPart` says which armor slot recolours it).
fn spawn_mannequin(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                width: Val::Px(140.0),
                height: Val::Px(ARMOR_SLOTS as f32 * SLOT + (ARMOR_SLOTS - 1) as f32 * GAP),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        ))
        .with_children(|m| {
            // (armor slot, x, y, w, h, base colour)
            let parts: [(usize, f32, f32, f32, f32, Color); 9] = [
                (0, 52.0, 14.0, 36.0, 36.0, SKIN),                          // head
                (1, 52.0, 52.0, 36.0, 54.0, CLOTH),                         // torso
                (1, 32.0, 52.0, 18.0, 54.0, CLOTH),                         // left arm
                (1, 90.0, 52.0, 18.0, 54.0, CLOTH),                         // right arm
                (2, 52.0, 108.0, 17.0, 46.0, PANTS),                        // left leg
                (2, 71.0, 108.0, 17.0, 46.0, PANTS),                        // right leg
                (3, 50.0, 154.0, 20.0, 14.0, Color::srgb(0.18, 0.18, 0.2)), // left boot
                (3, 70.0, 154.0, 20.0, 14.0, Color::srgb(0.18, 0.18, 0.2)), // right boot
                (0, 52.0, 14.0, 36.0, 12.0, Color::srgb(0.25, 0.17, 0.10)), // hair
            ];
            for (slot, x, y, w, h, color) in parts {
                m.spawn((
                    MannequinPart { slot, base: color },
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(x),
                        top: Val::Px(y),
                        width: Val::Px(w),
                        height: Val::Px(h),
                        ..default()
                    },
                    BackgroundColor(color),
                ));
            }
        });
}

fn update_mannequin(inventory: Res<Inventory>, mut parts: Query<(&MannequinPart, &mut BackgroundColor)>) {
    if !inventory.is_changed() {
        return;
    }
    for (part, mut color) in &mut parts {
        color.0 = if inventory.slots[ARMOR_START + part.slot].is_some() { IRON } else { part.base };
    }
}

#[allow(clippy::too_many_arguments)]
fn update_layout(
    screen: Res<Screen>,
    mode: Res<GameMode>,
    creative: Res<CreativeState>,
    mut root: Query<&mut Node, (With<InventoryRoot>, Without<Section>)>,
    mut sections: Query<(&Section, &mut Node), Without<InventoryRoot>>,
    mut title: Query<&mut PixelText, With<TitleText>>,
    mut tabs: Query<(&CreativeTabButton, &mut BorderColor, &mut BackgroundColor)>,
) {
    if let Ok(mut node) = root.single_mut() {
        let want = if screen.is_container() { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    let l = layout(*screen, *mode, creative.tab);
    for (section, mut node) in &mut sections {
        let show = match section {
            Section::Tabs => l.tabs,
            Section::SurvivalTop => l.survival_top,
            Section::CreativeGrid => l.creative_grid,
            Section::Workbench => l.workbench,
            Section::Furnace => l.furnace,
            Section::Main => l.main,
        };
        let want = if show { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    if let Ok(mut t) = title.single_mut() {
        let name = match *screen {
            Screen::Workbench => "Crafting".to_string(),
            Screen::Furnace => "Furnace".to_string(),
            _ if *mode == GameMode::Creative && creative.tab != INVENTORY_TAB => {
                creative::tabs().get(creative.tab).map_or("Creative", |t: &Tab| t.name).to_string()
            }
            _ => "Inventory".to_string(),
        };
        PixelText::set(&mut t, &name);
    }
    for (tab, mut border, mut bg) in &mut tabs {
        let selected = tab.0 == creative.tab;
        let want_border = if selected { BorderColor::all(Color::WHITE) } else { bevel(false) };
        if *border != want_border {
            *border = want_border;
        }
        let want_bg = if selected { SLOT_BG_HOVER } else { SLOT_BG };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
    }
}

fn hovered_slot(slots: &Query<(&SlotView, &RelativeCursorPosition)>) -> Option<usize> {
    slots.iter().find(|(_, rcp)| rcp.cursor_over).map(|(s, _)| s.0)
}

#[allow(clippy::too_many_arguments)]
fn inventory_mouse(
    screen: Res<Screen>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut inventory: ResMut<Inventory>,
    mut drag: ResMut<DragState>,
    slots: Query<(&SlotView, &RelativeCursorPosition)>,
    panel: Query<&RelativeCursorPosition, With<PanelNode>>,
    cam: Query<&GlobalTransform, With<PlayerCamera>>,
    mut drops: DropSpawner,
) {
    if !screen.is_container() {
        drag.0 = None;
        return;
    }
    let hovered = hovered_slot(&slots);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    for (mb, button) in [(MouseButton::Left, ClickButton::Left), (MouseButton::Right, ClickButton::Right)] {
        if mouse.just_pressed(mb) && drag.0.is_none() {
            if let Some(slot) = hovered {
                if shift && button == ClickButton::Left {
                    inventory.shift_click(slot);
                } else if inventory.cursor.is_some() && slot != CRAFT_OUT && slot != FURNACE_OUT {
                    // A stack is in the hand: this may become a drag across several slots, decided on release.
                    drag.0 = Some(Drag { button, slots: vec![slot] });
                } else {
                    inventory.click(slot, button);
                }
            } else if let Some(c) = inventory.cursor
                && panel.single().is_ok_and(|p| !p.cursor_over)
                && let Ok(cam) = cam.single()
            {
                // Clicking outside the panel tosses the stack (or one item) into the world.
                let (count, rest) = match button {
                    ClickButton::Left => (c.count, None),
                    ClickButton::Right => (1, (c.count > 1).then(|| ItemStack { count: c.count - 1, ..c })),
                };
                inventory.cursor = rest;
                let dir = cam.forward().as_vec3();
                drops.spawn_delayed(
                    ItemStack { count, ..c },
                    cam.translation() + dir * 0.5 - Vec3::Y * 0.3,
                    dir * 4.5 + Vec3::Y * 1.5,
                    2.0,
                );
            }
        }
        if let Some(d) = &mut drag.0
            && d.button == button
        {
            if mouse.pressed(mb)
                && let Some(slot) = hovered
                && !d.slots.contains(&slot)
            {
                d.slots.push(slot);
            }
            if mouse.just_released(mb) {
                let d = drag.0.take().expect("checked above");
                if d.slots.len() <= 1 {
                    inventory.click(d.slots[0], d.button);
                } else {
                    inventory.distribute(&d.slots, d.button == ClickButton::Right);
                }
            }
        }
    }
}

/// Furnaces keep cooking while their screen is open or closed (the world time pauses only in menus).
fn tick_furnaces(time: Res<Time>, mut inventory: ResMut<Inventory>) {
    // Only touch the resource when there is something to simulate (keeps change detection quiet).
    if inventory.furnaces.values().any(|f| f.is_active()) {
        inventory.tick_furnaces(time.delta_secs());
    }
}

/// The item shown in creative grid cell `cell` (taking the scroll position into account).
pub fn creative_item(tab: usize, scroll: usize, cell: usize) -> Option<Item> {
    creative::tabs().get(tab)?.items.get(scroll * GRID_COLS + cell).copied()
}

fn max_scroll(tab: usize) -> usize {
    let n = creative::tabs().get(tab).map_or(0, |t| t.items.len());
    n.div_ceil(GRID_COLS).saturating_sub(GRID_ROWS)
}

#[allow(clippy::too_many_arguments)]
fn creative_input(
    screen: Res<Screen>,
    mode: Res<GameMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    wheel: Res<AccumulatedMouseScroll>,
    mut state: ResMut<CreativeState>,
    mut inventory: ResMut<Inventory>,
    cells: Query<(&CreativeCell, &RelativeCursorPosition)>,
    tab_buttons: Query<(&CreativeTabButton, &RelativeCursorPosition)>,
) {
    if *screen != Screen::Inventory || *mode != GameMode::Creative {
        return;
    }
    let l = layout(*screen, *mode, state.tab);
    if let Some(button) = tab_buttons.iter().find(|(_, r)| r.cursor_over).map(|(b, _)| b.0)
        && mouse.just_pressed(MouseButton::Left)
    {
        state.tab = button;
        state.scroll = 0;
        return;
    }
    if !l.creative_grid {
        return;
    }
    if wheel.delta.y != 0.0 {
        let max = max_scroll(state.tab);
        state.scroll = if wheel.delta.y > 0.0 { state.scroll.saturating_sub(1) } else { (state.scroll + 1).min(max) };
    }
    let Some(cell) = cells.iter().find(|(_, r)| r.cursor_over).map(|(c, _)| c.0) else { return };
    let left = mouse.just_pressed(MouseButton::Left);
    let right = mouse.just_pressed(MouseButton::Right);
    if !left && !right {
        return;
    }
    // A stack on the cursor is put back (deleted) by clicking the directory.
    if inventory.cursor.is_some() {
        inventory.cursor = None;
        return;
    }
    let Some(item) = creative_item(state.tab, state.scroll, cell) else { return };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let count = if right { 1 } else { item.max_stack() };
    let stack = ItemStack::new(item, count);
    if shift && left {
        inventory.add(stack);
    } else {
        inventory.cursor = Some(stack);
    }
}

fn update_creative_cells(
    screen: Res<Screen>,
    state: Res<CreativeState>,
    mut icons: Query<(&CreativeCellIcon, &mut ImageNode, &mut Visibility)>,
) {
    if *screen != Screen::Inventory {
        return;
    }
    for (CreativeCellIcon(i), mut icon, mut vis) in &mut icons {
        match creative_item(state.tab, state.scroll, *i) {
            Some(item) => {
                let rect = Some(tile_rect(item.icon_tile()));
                if icon.rect != rect {
                    icon.rect = rect;
                }
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

fn update_furnace_progress(
    screen: Res<Screen>,
    inventory: Res<Inventory>,
    mut flame: Query<&mut Node, (With<FlameFill>, Without<ArrowFill>)>,
    mut arrows: Query<&mut Node, (With<ArrowFill>, Without<FlameFill>)>,
) {
    let furnace =
        if *screen == Screen::Furnace { inventory.open_furnace.and_then(|p| inventory.furnace_state(p)) } else { None };
    let (burn, cook) = furnace
        .map_or((0.0, 0.0), |f| (if f.burn_max > 0.0 { f.burn_left / f.burn_max } else { 0.0 }, f.cook / SMELT_SECS));
    for mut n in &mut flame {
        let h = Val::Px(52.0 * burn.clamp(0.0, 1.0));
        if n.height != h {
            n.height = h;
        }
    }
    // The crafting arrows stay empty; only the furnace arrow shows progress.
    let w = Val::Px(ARROW_W * cook.clamp(0.0, 1.0));
    for mut n in &mut arrows {
        if n.width != w {
            n.width = w;
        }
    }
}

fn style_slots(
    screen: Res<Screen>,
    drag: Res<DragState>,
    mut slots: Query<(&SlotView, &RelativeCursorPosition, &mut BackgroundColor), Without<CreativeCell>>,
    mut cells: Query<(&RelativeCursorPosition, &mut BackgroundColor), (With<CreativeCell>, Without<SlotView>)>,
) {
    if !screen.is_container() {
        return;
    }
    for (SlotView(i), rcp, mut bg) in &mut slots {
        let dragged = drag.0.as_ref().is_some_and(|d| d.slots.len() > 1 && d.slots.contains(i));
        let want = if dragged {
            Color::srgba(0.35, 0.50, 0.30, 0.95)
        } else if rcp.cursor_over {
            SLOT_BG_HOVER
        } else {
            SLOT_BG
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    for (rcp, mut bg) in &mut cells {
        let want = if rcp.cursor_over { SLOT_BG_HOVER } else { SLOT_BG };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

/// The text under the pointer: item name (and wear), or the name of a creative tab.
fn tooltip_text(stack: Option<ItemStack>, item: Option<Item>) -> Option<String> {
    let name = stack.map(|s| s.item).or(item)?.name();
    match stack.and_then(|s| s.item.durability().map(|d| (s.damage, d))) {
        Some((damage, max)) => Some(format!("{name}\nDurability: {}/{}", max.saturating_sub(damage), max)),
        None => Some(name),
    }
}

#[allow(clippy::too_many_arguments)]
fn update_cursor_stack(
    screen: Res<Screen>,
    inventory: Res<Inventory>,
    creative: Res<CreativeState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    slots: Query<(&SlotView, &RelativeCursorPosition)>,
    cells: Query<(&CreativeCell, &RelativeCursorPosition)>,
    tab_buttons: Query<(&CreativeTabButton, &RelativeCursorPosition)>,
    mut stack: Query<&mut Node, (With<CursorStack>, Without<Tooltip>)>,
    mut icon: Query<(&mut ImageNode, &mut Visibility), With<CursorIcon>>,
    mut count: Query<&mut PixelText, (With<CursorCount>, Without<Tooltip>)>,
    mut tooltip: Query<(&mut Node, &mut PixelText), With<Tooltip>>,
) {
    let (Ok(mut node), Ok((mut image, mut vis)), Ok(mut count), Ok((mut tip_node, mut tip_text))) =
        (stack.single_mut(), icon.single_mut(), count.single_mut(), tooltip.single_mut())
    else {
        return;
    };
    let open = screen.is_container();
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    let (Some(pos), true) = (cursor, open) else {
        node.display = Display::None;
        tip_node.display = Display::None;
        return;
    };

    let held: Option<ItemStack> = inventory.cursor;
    node.display = if held.is_some() { Display::Flex } else { Display::None };
    node.left = Val::Px(pos.x - (SLOT - 6.0) / 2.0);
    node.top = Val::Px(pos.y - (SLOT - 6.0) / 2.0);
    show_stack(held, &mut image, &mut vis, &mut count);

    // Item name next to the pointer while hovering something with an empty hand.
    let hovered_stack = slots.iter().find(|(_, r)| r.cursor_over).and_then(|(s, _)| inventory.slots[s.0]);
    let hovered_cell =
        cells.iter().find(|(_, r)| r.cursor_over).and_then(|(c, _)| creative_item(creative.tab, creative.scroll, c.0));
    let hovered_tab = tab_buttons.iter().find(|(_, r)| r.cursor_over).map(|(t, _)| t.0);
    let text = if held.is_some() {
        None
    } else if let Some(t) = hovered_tab {
        Some(creative::tabs().get(t).map_or("Inventory", |t| t.name).to_string())
    } else {
        tooltip_text(hovered_stack, hovered_cell)
    };
    match text {
        Some(t) => {
            tip_node.display = Display::Flex;
            tip_node.left = Val::Px(pos.x + 14.0);
            tip_node.top = Val::Px(pos.y - 40.0);
            PixelText::set(&mut tip_text, &t);
        }
        None => tip_node.display = Display::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_constants_match_the_inventory_model() {
        assert_eq!(ARMOR_START, crate::inventory::INVENTORY_SLOTS);
        assert_eq!(HOTBAR_SLOTS + 3 * HOTBAR_SLOTS, crate::inventory::INVENTORY_SLOTS);
        assert_eq!(GRID_COLS * GRID_ROWS, 45);
        const { assert!(CRAFT_OUT > CRAFT_START) };
    }

    #[test]
    fn the_right_sections_show_for_every_screen() {
        let survival = layout(Screen::Inventory, GameMode::Survival, 0);
        assert!(survival.survival_top && survival.main && !survival.tabs && !survival.creative_grid);
        let creative_tab = layout(Screen::Inventory, GameMode::Creative, 1);
        assert!(creative_tab.tabs && creative_tab.creative_grid && !creative_tab.main && !creative_tab.survival_top);
        let creative_inv = layout(Screen::Inventory, GameMode::Creative, INVENTORY_TAB);
        assert!(creative_inv.tabs && creative_inv.survival_top && creative_inv.main && !creative_inv.creative_grid);
        let bench = layout(Screen::Workbench, GameMode::Creative, 0);
        assert!(bench.workbench && bench.main && !bench.survival_top && !bench.tabs);
        let furnace = layout(Screen::Furnace, GameMode::Survival, 0);
        assert!(furnace.furnace && furnace.main && !furnace.workbench);
        assert_eq!(layout(Screen::Playing, GameMode::Survival, 0), Layout::default());
        assert_eq!(layout(Screen::Pause, GameMode::Creative, 0), Layout::default());
    }

    #[test]
    fn creative_cells_follow_tab_and_scroll() {
        let tabs = creative::tabs();
        assert_eq!(creative_item(0, 0, 0), tabs[0].items.first().copied());
        assert_eq!(creative_item(0, 0, GRID_COLS * GRID_ROWS + 100), None);
        assert_eq!(creative_item(99, 0, 0), None);
        // A tab with more than five rows scrolls; one that fits does not.
        let long = tabs.iter().position(|t| t.items.len() > GRID_COLS * GRID_ROWS);
        if let Some(i) = long {
            assert!(max_scroll(i) > 0);
            assert_eq!(creative_item(i, 1, 0), tabs[i].items.get(GRID_COLS).copied());
        }
        let short = tabs.iter().position(|t| t.items.len() <= GRID_COLS * GRID_ROWS).unwrap();
        assert_eq!(max_scroll(short), 0);
    }

    #[test]
    fn tooltips_name_items_and_show_wear() {
        use crate::items::{Tier, ToolKind};
        assert_eq!(tooltip_text(None, None), None);
        assert_eq!(tooltip_text(Some(ItemStack::new(Item::DIAMOND, 3)), None).unwrap(), "Diamond");
        let pick = ItemStack { damage: 10, ..ItemStack::new(Item::tool(ToolKind::Pickaxe, Tier::Wood), 1) };
        assert_eq!(tooltip_text(Some(pick), None).unwrap(), "Wooden Pickaxe\nDurability: 49/59");
        assert_eq!(tooltip_text(None, Some(Item::COAL)).unwrap(), "Coal");
    }
}
