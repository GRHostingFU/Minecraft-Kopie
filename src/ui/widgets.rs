//! Small UI building blocks shared by the HUD, the inventory screen and the menus:
//! item slots, buttons and sliders.

use crate::inventory::{Inventory, ItemStack};
use crate::rendering::atlas::tile_rect;
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::RelativeCursorPosition;

pub const PANEL_BG: Color = Color::srgba(0.13, 0.13, 0.15, 0.97);
pub const PANEL_BORDER: Color = Color::srgb(0.38, 0.38, 0.42);
pub const SLOT_BG: Color = Color::srgba(0.05, 0.05, 0.06, 0.88);
pub const SLOT_BG_HOVER: Color = Color::srgba(0.30, 0.30, 0.36, 0.95);
pub const SLOT_BORDER: Color = Color::srgb(0.30, 0.30, 0.34);
pub const TEXT_DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.65);

/// The square behind a slot. Slots in interactive screens also carry a `RelativeCursorPosition`.
#[derive(Component)]
pub struct SlotView(pub usize);

#[derive(Component)]
pub struct SlotIcon(pub usize);

#[derive(Component)]
pub struct SlotCount(pub usize);

pub fn text_font(size: f32) -> TextFont {
    TextFont { font_size: FontSize::Px(size), ..default() }
}

/// Spawns one inventory slot (background, icon, stack count) as a child of `parent`.
pub fn spawn_slot(
    parent: &mut ChildSpawnerCommands,
    atlas: &Handle<Image>,
    index: usize,
    size: f32,
    interactive: bool,
) {
    let mut slot = parent.spawn((
        SlotView(index),
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            border: UiRect::all(Val::Px(2.0)),
            padding: UiRect::all(Val::Px(size * 0.09)),
            ..default()
        },
        BackgroundColor(SLOT_BG),
        BorderColor::all(SLOT_BORDER),
    ));
    if interactive {
        slot.insert(RelativeCursorPosition::default());
    }
    slot.with_children(|s| {
        s.spawn((
            SlotIcon(index),
            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            ImageNode::new(atlas.clone()),
            Visibility::Hidden,
        ));
        s.spawn((
            SlotCount(index),
            Text::new(""),
            text_font((size * 0.34).max(11.0)),
            TextColor(Color::WHITE),
            TextShadow { offset: Vec2::splat(1.5), color: Color::BLACK },
            Node { position_type: PositionType::Absolute, right: Val::Px(3.0), bottom: Val::Px(0.0), ..default() },
        ));
    });
}

/// Writes the contents of `stack` into an icon node and its count label.
pub fn show_stack(stack: Option<ItemStack>, icon: &mut ImageNode, vis: &mut Visibility, count: &mut Text) {
    match stack {
        Some(s) => {
            icon.rect = Some(tile_rect(s.item.icon_tile()));
            *vis = Visibility::Inherited;
            let label = if s.count > 1 { s.count.to_string() } else { String::new() };
            if count.0 != label {
                count.0 = label;
            }
        }
        None => {
            *vis = Visibility::Hidden;
            if !count.0.is_empty() {
                count.0.clear();
            }
        }
    }
}

/// Keeps every slot icon and count label in sync with the inventory.
pub fn refresh_slots(
    inventory: Res<Inventory>,
    mut icons: Query<(&SlotIcon, &mut ImageNode, &mut Visibility), Without<SlotCount>>,
    mut counts: Query<(&SlotCount, &mut Text)>,
) {
    if !inventory.is_changed() {
        return;
    }
    for (SlotIcon(i), mut icon, mut vis) in &mut icons {
        let stack = inventory.slots[*i];
        match stack {
            Some(s) => {
                let rect = Some(tile_rect(s.item.icon_tile()));
                if icon.rect != rect {
                    icon.rect = rect;
                }
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for (SlotCount(i), mut text) in &mut counts {
        let label = match inventory.slots[*i] {
            Some(s) if s.count > 1 => s.count.to_string(),
            _ => String::new(),
        };
        if text.0 != label {
            text.0 = label;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Buttons and sliders
// ---------------------------------------------------------------------------------------------

pub const BUTTON_BG: Color = Color::srgb(0.26, 0.26, 0.30);
pub const BUTTON_BG_HOVER: Color = Color::srgb(0.38, 0.40, 0.52);

/// A clickable menu button. The owning menu decides what each value means.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct MenuButton(pub MenuAction);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuAction {
    Resume,
    Options,
    Save,
    SaveAndQuit,
    ToggleFullscreen,
    BackToPause,
}

#[derive(Component)]
pub struct ButtonLabel(pub MenuAction);

pub fn spawn_button(parent: &mut ChildSpawnerCommands, action: MenuAction, label: &str, width: f32) {
    parent
        .spawn((
            MenuButton(action),
            RelativeCursorPosition::default(),
            Node {
                width: Val::Px(width),
                height: Val::Px(40.0),
                border: UiRect::all(Val::Px(2.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BUTTON_BG),
            BorderColor::all(PANEL_BORDER),
        ))
        .with_children(|b| {
            b.spawn((ButtonLabel(action), Text::new(label), text_font(18.0), TextColor(Color::WHITE)));
        });
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SliderKind {
    RenderDistance,
    Sensitivity,
    Fov,
}

#[derive(Component)]
pub struct Slider(pub SliderKind);

#[derive(Component)]
pub struct SliderFill(pub SliderKind);

#[derive(Component)]
pub struct SliderLabel(pub SliderKind);

pub const SLIDER_WIDTH: f32 = 340.0;

pub fn spawn_slider(parent: &mut ChildSpawnerCommands, kind: SliderKind) {
    parent
        .spawn((
            Slider(kind),
            RelativeCursorPosition::default(),
            Node {
                width: Val::Px(SLIDER_WIDTH),
                height: Val::Px(40.0),
                border: UiRect::all(Val::Px(2.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.08, 0.10)),
            BorderColor::all(PANEL_BORDER),
        ))
        .with_children(|s| {
            s.spawn((
                SliderFill(kind),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    width: Val::Percent(50.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.30, 0.46, 0.80)),
            ));
            s.spawn((SliderLabel(kind), Text::new(""), text_font(17.0), TextColor(Color::WHITE)));
        });
}
