//! The inventory screen (key `E`): main inventory, hotbar, four armor slots and a mannequin that
//! shows what is equipped. Mouse handling follows Minecraft: click to pick up / place / swap /
//! merge, right-click to split or place one, shift-click to quick-move, drag across slots to
//! spread a stack.

use super::screens::Screen;
use super::widgets::*;
use crate::inventory::{ARMOR_SLOTS, ARMOR_START, ClickButton, HOTBAR_SLOTS, Inventory, ItemStack};
use crate::rendering::AtlasHandle;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::window::PrimaryWindow;

const SLOT: f32 = 44.0;
const GAP: f32 = 4.0;

#[derive(Component)]
struct InventoryRoot;

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

/// A drag across slots that started with a stack on the cursor.
#[derive(Resource, Default, Debug)]
struct DragState(Option<Drag>);

#[derive(Debug)]
struct Drag {
    button: ClickButton,
    slots: Vec<usize>,
}

pub struct InventoryScreenPlugin;

impl Plugin for InventoryScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DragState>().add_systems(PostStartup, spawn_inventory_screen).add_systems(
            Update,
            (show_inventory, inventory_mouse, style_slots, update_cursor_stack, update_mannequin).chain(),
        );
    }
}

fn spawn_inventory_screen(mut commands: Commands, atlas: Res<AtlasHandle>) {
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
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(18.0)),
                    row_gap: Val::Px(12.0),
                    border: UiRect::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(PANEL_BG),
                BorderColor::all(PANEL_BORDER),
            ))
            .with_children(|panel| {
                panel.spawn((Text::new("Inventory"), text_font(22.0), TextColor(Color::WHITE)));

                // Armor, mannequin, help.
                panel
                    .spawn(Node { column_gap: Val::Px(16.0), align_items: AlignItems::Stretch, ..default() })
                    .with_children(|top| {
                        top.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(GAP), ..default() })
                            .with_children(|col| {
                                for i in 0..ARMOR_SLOTS {
                                    spawn_slot(col, &atlas.0, ARMOR_START + i, SLOT, true);
                                }
                            });
                        spawn_mannequin(top);
                        top.spawn((
                            Text::new(
                                "Left click   pick up / place\nRight click  split / place one\nShift click  quick move\nDrag         spread a stack\nE or Esc     close",
                            ),
                            text_font(13.0),
                            TextColor(TEXT_DIM),
                            Node { align_self: AlignSelf::FlexEnd, ..default() },
                        ));
                    });

                // Main inventory, three rows.
                panel.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(GAP), ..default() }).with_children(
                    |grid| {
                        for row in 0..3 {
                            grid.spawn(Node { column_gap: Val::Px(GAP), ..default() }).with_children(|r| {
                                for col in 0..HOTBAR_SLOTS {
                                    spawn_slot(r, &atlas.0, HOTBAR_SLOTS + row * HOTBAR_SLOTS + col, SLOT, true);
                                }
                            });
                        }
                    },
                );

                // Hotbar, separated from the main grid.
                panel.spawn(Node { column_gap: Val::Px(GAP), margin: UiRect::top(Val::Px(6.0)), ..default() }).with_children(
                    |hotbar| {
                        for i in 0..HOTBAR_SLOTS {
                            spawn_slot(hotbar, &atlas.0, i, SLOT, true);
                        }
                    },
                );
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
                ImageNode::new(atlas.0.clone()),
            ));
            c.spawn((
                CursorCount,
                Text::new(""),
                text_font(15.0),
                TextColor(Color::WHITE),
                TextShadow { offset: Vec2::splat(1.5), color: Color::BLACK },
                Node { position_type: PositionType::Absolute, right: Val::Px(0.0), bottom: Val::Px(-2.0), ..default() },
            ));
        });
    commands.spawn((
        Tooltip,
        GlobalZIndex(60),
        Text::new(""),
        text_font(16.0),
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgba(0.08, 0.02, 0.14, 0.92)),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            display: Display::None,
            ..default()
        },
    ));
}

const SKIN: Color = Color::srgb(0.80, 0.60, 0.46);
const CLOTH: Color = Color::srgb(0.20, 0.42, 0.62);
const PANTS: Color = Color::srgb(0.25, 0.25, 0.45);
const IRON: Color = Color::srgb(0.74, 0.77, 0.82);

/// A blocky figure made of coloured rectangles (`MannequinPart` says which armor slot recolours it).
fn spawn_mannequin(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node { width: Val::Px(140.0), height: Val::Px(188.0), ..default() },
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

fn show_inventory(screen: Res<Screen>, mut root: Query<&mut Node, With<InventoryRoot>>) {
    if !screen.is_changed() {
        return;
    }
    if let Ok(mut node) = root.single_mut() {
        node.display = if *screen == Screen::Inventory { Display::Flex } else { Display::None };
    }
}

fn hovered_slot(slots: &Query<(&SlotView, &RelativeCursorPosition)>) -> Option<usize> {
    slots.iter().find(|(_, rcp)| rcp.cursor_over).map(|(s, _)| s.0)
}

fn inventory_mouse(
    screen: Res<Screen>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut inventory: ResMut<Inventory>,
    mut drag: ResMut<DragState>,
    slots: Query<(&SlotView, &RelativeCursorPosition)>,
) {
    if *screen != Screen::Inventory {
        drag.0 = None;
        return;
    }
    let hovered = hovered_slot(&slots);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    for (mb, button) in [(MouseButton::Left, ClickButton::Left), (MouseButton::Right, ClickButton::Right)] {
        if mouse.just_pressed(mb)
            && drag.0.is_none()
            && let Some(slot) = hovered
        {
            if shift && button == ClickButton::Left {
                inventory.shift_click(slot);
            } else if inventory.cursor.is_some() {
                // A stack is in the hand: this may become a drag across several slots, decided on release.
                drag.0 = Some(Drag { button, slots: vec![slot] });
            } else {
                inventory.click(slot, button);
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

fn style_slots(
    screen: Res<Screen>,
    drag: Res<DragState>,
    mut slots: Query<(&SlotView, &RelativeCursorPosition, &mut BackgroundColor)>,
) {
    if *screen != Screen::Inventory {
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
}

#[allow(clippy::too_many_arguments)]
fn update_cursor_stack(
    screen: Res<Screen>,
    inventory: Res<Inventory>,
    windows: Query<&Window, With<PrimaryWindow>>,
    slots: Query<(&SlotView, &RelativeCursorPosition)>,
    mut stack: Query<&mut Node, (With<CursorStack>, Without<Tooltip>)>,
    mut icon: Query<(&mut ImageNode, &mut Visibility), With<CursorIcon>>,
    mut count: Query<&mut Text, (With<CursorCount>, Without<Tooltip>)>,
    mut tooltip: Query<(&mut Node, &mut Text), With<Tooltip>>,
) {
    let (Ok(mut node), Ok((mut image, mut vis)), Ok(mut count), Ok((mut tip_node, mut tip_text))) =
        (stack.single_mut(), icon.single_mut(), count.single_mut(), tooltip.single_mut())
    else {
        return;
    };
    let open = *screen == Screen::Inventory;
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

    // Item name next to the pointer while hovering a filled slot with an empty hand.
    let hovered = slots.iter().find(|(_, r)| r.cursor_over).map(|(s, _)| s.0);
    match (held, hovered.and_then(|i| inventory.slots[i])) {
        (None, Some(s)) => {
            tip_node.display = Display::Flex;
            tip_node.left = Val::Px(pos.x + 14.0);
            tip_node.top = Val::Px(pos.y - 34.0);
            let name = s.item.name();
            if tip_text.0 != name {
                tip_text.0 = name.to_string();
            }
        }
        _ => tip_node.display = Display::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::INVENTORY_SLOTS;

    #[test]
    fn layout_constants_match_the_inventory_model() {
        assert_eq!(ARMOR_START, INVENTORY_SLOTS);
        assert_eq!(HOTBAR_SLOTS + 3 * HOTBAR_SLOTS, INVENTORY_SLOTS);
        // Armor column and mannequin share the same height.
        assert_eq!(ARMOR_SLOTS as f32 * SLOT + (ARMOR_SLOTS - 1) as f32 * GAP, 188.0);
    }
}
