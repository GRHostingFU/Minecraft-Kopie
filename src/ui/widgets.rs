//! Small UI building blocks shared by the HUD, the inventory screens and the menus: buttons,
//! sliders, text fields and item slots, all drawn in the pixel font with bevelled borders.

use super::font::PixelText;
use super::screens::Screen;
use crate::inventory::Inventory;
use crate::items::ItemStack;
use crate::rendering::atlas::tile_rect;
use crate::session::GameMode;
use crate::settings::{
    GameSettings, MAX_FOV, MAX_RENDER_DISTANCE, MAX_SENSITIVITY, MIN_FOV, MIN_RENDER_DISTANCE, MIN_SENSITIVITY,
};
use crate::world::streaming::WorldSettings;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui::widget::NodeImageMode;

pub const PANEL_BG: Color = Color::srgba(0.09, 0.09, 0.11, 0.94);
pub const PANEL_BORDER: Color = Color::srgb(0.40, 0.40, 0.45);
pub const SLOT_BG: Color = Color::srgba(0.03, 0.03, 0.04, 0.86);
pub const SLOT_BG_HOVER: Color = Color::srgba(0.32, 0.32, 0.40, 0.95);
pub const TEXT_DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.62);
pub const TEXT_GOLD: Color = Color::srgb(1.0, 0.86, 0.25);
pub const BUTTON_BG: Color = Color::srgb(0.40, 0.40, 0.42);
pub const BUTTON_BG_HOVER: Color = Color::srgb(0.32, 0.38, 0.62);
pub const BUTTON_BG_OFF: Color = Color::srgb(0.20, 0.20, 0.22);
const BEVEL_LIGHT: Color = Color::srgb(0.74, 0.74, 0.78);
const BEVEL_DARK: Color = Color::srgb(0.12, 0.12, 0.14);

/// A beveled border: light top/left, dark bottom/right (swapped for `inset`).
pub fn bevel(inset: bool) -> BorderColor {
    let (a, b) = if inset { (BEVEL_DARK, BEVEL_LIGHT) } else { (BEVEL_LIGHT, BEVEL_DARK) };
    BorderColor { top: a, left: a, bottom: b, right: b }
}

// -------------------------------------------------------------------------------------------------
// Actions
// -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionsTab {
    Video,
    Controls,
    Audio,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiAction {
    // Title screen
    Singleplayer,
    OpenOptions,
    QuitGame,
    // World list
    SelectWorld(usize),
    PlaySelected,
    DeleteSelected,
    OpenCreate,
    BackToTitle,
    // World creation
    ToggleGameMode,
    CreateWorld,
    CancelCreate,
    // Options
    Tab(OptionsTab),
    ToggleFullscreen,
    ToggleVsync,
    CycleParticles,
    CycleSprintMode,
    CycleSneakMode,
    ToggleInvertMouse,
    OptionsDone,
    // Pause menu
    Resume,
    SaveWorld,
    SaveAndTitle,
    // Death screen
    Respawn,
    DeathToTitle,
}

/// A click on a button that is live on the current screen.
#[derive(Message, Clone, Copy, Debug)]
pub struct UiClick(pub UiAction);

#[derive(Component, Clone, Copy)]
pub struct UiButton {
    pub action: UiAction,
    pub screen: Screen,
}

/// The button is greyed out and ignores clicks.
#[derive(Component)]
pub struct Disabled;

/// The button shows the active choice (the open options tab): it keeps a white frame.
#[derive(Component)]
pub struct Selected;

/// Text of a button that changes with the settings (see `options::label_for`).
#[derive(Component)]
pub struct ActionLabel(pub UiAction);

pub fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    screen: Screen,
    action: UiAction,
    label: &str,
    width: f32,
) -> Entity {
    parent
        .spawn((
            UiButton { action, screen },
            RelativeCursorPosition::default(),
            Node {
                width: Val::Px(width),
                height: Val::Px(40.0),
                border: UiRect::all(Val::Px(3.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(BUTTON_BG),
            bevel(false),
        ))
        .with_children(|b| {
            b.spawn((ActionLabel(action), PixelText::new(label)));
        })
        .id()
}

/// Colours the buttons of the visible screen and reports clicks.
pub fn button_system(
    screen: Res<Screen>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut buttons: Query<(
        &UiButton,
        &RelativeCursorPosition,
        &mut BackgroundColor,
        &mut BorderColor,
        Has<Disabled>,
        Has<Selected>,
    )>,
    mut clicks: MessageWriter<UiClick>,
) {
    for (button, rcp, mut bg, mut border, disabled, selected) in &mut buttons {
        let live = *screen == button.screen;
        let hover = live && rcp.cursor_over && !disabled;
        let want = if disabled {
            BUTTON_BG_OFF
        } else if hover {
            BUTTON_BG_HOVER
        } else {
            BUTTON_BG
        };
        if bg.0 != want {
            bg.0 = want;
        }
        let want_border = if hover || selected { BorderColor::all(Color::WHITE) } else { bevel(disabled) };
        if *border != want_border {
            *border = want_border;
        }
        if hover && mouse.just_pressed(MouseButton::Left) {
            clicks.write(UiClick(button.action));
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Sliders
// -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SliderKind {
    RenderDistance,
    Sensitivity,
    Fov,
    MasterVolume,
    MusicVolume,
    WeatherVolume,
}

#[derive(Component)]
pub struct Slider {
    pub kind: SliderKind,
    pub screen: Screen,
}

#[derive(Component)]
pub struct SliderFill(pub SliderKind);

#[derive(Component)]
pub struct SliderLabel(pub SliderKind);

pub const SLIDER_WIDTH: f32 = 380.0;

pub fn spawn_slider(parent: &mut ChildSpawnerCommands, screen: Screen, kind: SliderKind) {
    parent
        .spawn((
            Slider { kind, screen },
            RelativeCursorPosition::default(),
            Node {
                width: Val::Px(SLIDER_WIDTH),
                height: Val::Px(40.0),
                border: UiRect::all(Val::Px(3.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgb(0.07, 0.07, 0.09)),
            bevel(true),
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
                BackgroundColor(Color::srgb(0.27, 0.42, 0.74)),
            ));
            s.spawn((SliderLabel(kind), PixelText::new("")));
        });
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
        SliderKind::MasterVolume | SliderKind::MusicVolume | SliderKind::WeatherVolume => (t * 100.0).round() / 100.0,
    }
}

/// Inverse of [`slider_value`].
pub fn slider_fraction(kind: SliderKind, value: f32) -> f32 {
    let (lo, hi) = match kind {
        SliderKind::RenderDistance => (MIN_RENDER_DISTANCE as f32, MAX_RENDER_DISTANCE as f32),
        SliderKind::Sensitivity => (MIN_SENSITIVITY, MAX_SENSITIVITY),
        SliderKind::Fov => (MIN_FOV, MAX_FOV),
        SliderKind::MasterVolume | SliderKind::MusicVolume | SliderKind::WeatherVolume => (0.0, 1.0),
    };
    ((value - lo) / (hi - lo)).clamp(0.0, 1.0)
}

pub fn slider_current(kind: SliderKind, settings: &GameSettings, world: &WorldSettings) -> f32 {
    match kind {
        SliderKind::RenderDistance => world.render_distance as f32,
        SliderKind::Sensitivity => settings.mouse_sensitivity,
        SliderKind::Fov => settings.fov,
        SliderKind::MasterVolume => settings.master_volume,
        SliderKind::MusicVolume => settings.music_volume,
        SliderKind::WeatherVolume => settings.weather_volume,
    }
}

pub fn slider_text(kind: SliderKind, settings: &GameSettings, world: &WorldSettings) -> String {
    let v = slider_current(kind, settings, world);
    match kind {
        SliderKind::RenderDistance => format!("Render Distance: {} chunks", world.render_distance),
        SliderKind::Sensitivity => format!("Mouse Sensitivity: {:.0}%", v * 100.0),
        SliderKind::Fov => format!("FOV: {v:.0}"),
        SliderKind::MasterVolume => format!("Master Volume: {:.0}%", v * 100.0),
        SliderKind::MusicVolume => format!("Music: {:.0}%", v * 100.0),
        SliderKind::WeatherVolume => format!("Weather: {:.0}%", v * 100.0),
    }
}

/// The slider currently being dragged (the mouse may leave it while the button is held).
#[derive(Resource, Default)]
pub struct ActiveSlider(Option<SliderKind>);

pub fn drag_sliders(
    screen: Res<Screen>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut active: ResMut<ActiveSlider>,
    mut settings: ResMut<GameSettings>,
    mut world: ResMut<WorldSettings>,
    sliders: Query<(&Slider, &RelativeCursorPosition)>,
) {
    // A click that is released within the same frame still counts as a press.
    let down = mouse.pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Left);
    if !down || !sliders.iter().any(|(s, _)| s.screen == *screen) {
        active.0 = None;
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        active.0 = sliders.iter().find(|(s, r)| s.screen == *screen && r.cursor_over).map(|(s, _)| s.kind);
    }
    let Some(kind) = active.0 else { return };
    let Some(x) = sliders.iter().find(|(s, _)| s.kind == kind).and_then(|(_, r)| r.normalized).map(|n| n.x + 0.5)
    else {
        return;
    };
    let v = slider_value(kind, x);
    match kind {
        SliderKind::RenderDistance => {
            if world.render_distance != v as i32 {
                world.render_distance = v as i32;
            }
        }
        SliderKind::Sensitivity => set_if_changed(&mut settings, |s| &mut s.mouse_sensitivity, v),
        SliderKind::Fov => set_if_changed(&mut settings, |s| &mut s.fov, v),
        SliderKind::MasterVolume => set_if_changed(&mut settings, |s| &mut s.master_volume, v),
        SliderKind::MusicVolume => set_if_changed(&mut settings, |s| &mut s.music_volume, v),
        SliderKind::WeatherVolume => set_if_changed(&mut settings, |s| &mut s.weather_volume, v),
    }
}

/// Writes `value` only if it differs, so change detection stays quiet while the mouse rests.
fn set_if_changed(settings: &mut ResMut<GameSettings>, field: impl Fn(&mut GameSettings) -> &mut f32, value: f32) {
    if *field(settings.bypass_change_detection()) != value {
        *field(settings.as_mut()) = value;
    }
}

pub fn update_sliders(
    settings: Res<GameSettings>,
    world: Res<WorldSettings>,
    mut labels: Query<(&SliderLabel, &mut PixelText)>,
    mut fills: Query<(&SliderFill, &mut Node)>,
) {
    for (SliderLabel(kind), mut text) in &mut labels {
        let label = slider_text(*kind, &settings, &world);
        PixelText::set(&mut text, &label);
    }
    for (SliderFill(kind), mut node) in &mut fills {
        let w = Val::Percent(slider_fraction(*kind, slider_current(*kind, &settings, &world)) * 100.0);
        if node.width != w {
            node.width = w;
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Text fields
// -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldId {
    WorldName,
    Seed,
}

#[derive(Component)]
pub struct TextField {
    pub id: FieldId,
    pub screen: Screen,
    pub placeholder: &'static str,
    pub max_len: usize,
}

#[derive(Resource, Default)]
pub struct FocusedField(pub Option<FieldId>);

/// What the player typed into the world creation form.
#[derive(Resource, Debug, PartialEq)]
pub struct CreateForm {
    pub name: String,
    pub seed: String,
    pub mode: GameMode,
}

impl Default for CreateForm {
    fn default() -> Self {
        Self { name: "New World".into(), seed: String::new(), mode: GameMode::Survival }
    }
}

impl CreateForm {
    pub fn field_mut(&mut self, id: FieldId) -> &mut String {
        match id {
            FieldId::WorldName => &mut self.name,
            FieldId::Seed => &mut self.seed,
        }
    }

    pub fn field(&self, id: FieldId) -> &str {
        match id {
            FieldId::WorldName => &self.name,
            FieldId::Seed => &self.seed,
        }
    }
}

pub fn spawn_text_field(
    parent: &mut ChildSpawnerCommands,
    screen: Screen,
    id: FieldId,
    placeholder: &'static str,
    max_len: usize,
) {
    parent
        .spawn((
            TextField { id, screen, placeholder, max_len },
            RelativeCursorPosition::default(),
            Node {
                width: Val::Px(SLIDER_WIDTH),
                height: Val::Px(40.0),
                border: UiRect::all(Val::Px(3.0)),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgb(0.02, 0.02, 0.03)),
            bevel(true),
        ))
        .with_children(|f| {
            f.spawn((FieldText(id), PixelText::new("")));
        });
}

#[derive(Component)]
pub struct FieldText(pub FieldId);

/// Keeps only characters the pixel font can draw, in a world name or seed.
pub fn accepts_char(c: char) -> bool {
    c.is_ascii() && !c.is_ascii_control()
}

pub fn focus_fields(
    screen: Res<Screen>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut focused: ResMut<FocusedField>,
    fields: Query<(&TextField, &RelativeCursorPosition, &mut BorderColor)>,
) {
    let mut fields = fields;
    if mouse.just_pressed(MouseButton::Left) {
        let hit = fields.iter().find(|(f, r, _)| f.screen == *screen && r.cursor_over).map(|(f, ..)| f.id);
        // Clicking a field focuses it, clicking anywhere else (a button, the background) leaves it.
        if focused.0 != hit {
            focused.0 = hit;
        }
    }
    for (f, _, mut border) in &mut fields {
        let want = if focused.0 == Some(f.id) { BorderColor::all(Color::WHITE) } else { bevel(true) };
        if *border != want {
            *border = want;
        }
    }
}

/// Typing into the focused field.
pub fn text_input(
    mut keys: MessageReader<KeyboardInput>,
    mut focused: ResMut<FocusedField>,
    mut form: ResMut<CreateForm>,
    fields: Query<&TextField>,
) {
    let Some(id) = focused.0 else {
        keys.clear();
        return;
    };
    let max = fields.iter().find(|f| f.id == id).map_or(32, |f| f.max_len);
    for ev in keys.read() {
        if !ev.state.is_pressed() {
            continue;
        }
        match &ev.logical_key {
            Key::Backspace => {
                form.field_mut(id).pop();
            }
            Key::Enter => focused.0 = None,
            Key::Tab => {
                focused.0 = Some(match id {
                    FieldId::WorldName => FieldId::Seed,
                    FieldId::Seed => FieldId::WorldName,
                });
            }
            Key::Space => push_char(form.field_mut(id), ' ', max),
            Key::Character(s) => {
                for c in s.chars() {
                    push_char(form.field_mut(id), c, max);
                }
            }
            _ => {}
        }
    }
}

fn push_char(s: &mut String, c: char, max: usize) {
    if accepts_char(c) && s.chars().count() < max {
        s.push(c);
    }
}

pub fn update_field_text(
    time: Res<Time<Real>>,
    focused: Res<FocusedField>,
    form: Res<CreateForm>,
    fields: Query<&TextField>,
    mut texts: Query<(&FieldText, &mut PixelText)>,
) {
    let blink = ((time.elapsed_secs() * 2.0) as u32).is_multiple_of(2);
    for (FieldText(id), mut text) in &mut texts {
        let value = form.field(*id);
        let shown = if focused.0 == Some(*id) {
            format!("{value}{}", if blink { "_" } else { " " })
        } else if value.is_empty() {
            fields.iter().find(|f| f.id == *id).map_or(String::new(), |f| f.placeholder.to_string())
        } else {
            value.to_string()
        };
        let dim = value.is_empty() && focused.0 != Some(*id);
        PixelText::set(&mut text, &shown);
        let color = if dim { TEXT_DIM } else { Color::WHITE };
        if text.color != color {
            text.color = color;
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Item slots
// -------------------------------------------------------------------------------------------------

/// The square behind a slot. Slots in interactive screens also carry a `RelativeCursorPosition`.
#[derive(Component)]
pub struct SlotView(pub usize);

#[derive(Component)]
pub struct SlotIcon(pub usize);

#[derive(Component)]
pub struct SlotCount(pub usize);

/// The durability bar under the icon of a worn tool.
#[derive(Component)]
pub struct SlotWear(pub usize);

/// Spawns one inventory slot (background, icon, stack count, wear bar) as a child of `parent`.
pub fn spawn_slot(
    parent: &mut ChildSpawnerCommands,
    atlas: &Handle<Image>,
    index: usize,
    size: f32,
    interactive: bool,
) -> Entity {
    let mut slot = parent.spawn((
        SlotView(index),
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            border: UiRect::all(Val::Px(2.0)),
            padding: UiRect::all(Val::Px(size * 0.07)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(SLOT_BG),
        bevel(true),
    ));
    if interactive {
        slot.insert(RelativeCursorPosition::default());
    }
    slot.with_children(|s| {
        s.spawn((
            SlotIcon(index),
            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            ImageNode { image: atlas.clone(), image_mode: NodeImageMode::Stretch, ..default() },
            Visibility::Hidden,
        ));
        s.spawn((
            SlotWear(index),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(size * 0.12),
                bottom: Val::Px(2.0),
                width: Val::Px(0.0),
                height: Val::Px(3.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.2, 0.9, 0.2)),
            Visibility::Hidden,
        ));
        s.spawn((
            SlotCount(index),
            PixelText::new(""),
            Node { position_type: PositionType::Absolute, right: Val::Px(2.0), bottom: Val::Px(0.0), ..default() },
        ));
    });
    slot.id()
}

/// Writes the contents of `stack` into an icon node, its count label and its wear bar.
pub fn show_stack(stack: Option<ItemStack>, icon: &mut ImageNode, vis: &mut Visibility, count: &mut Mut<PixelText>) {
    match stack {
        Some(s) => {
            let rect = Some(tile_rect(s.item.icon_tile()));
            if icon.rect != rect {
                icon.rect = rect;
            }
            *vis = Visibility::Inherited;
            let label = if s.count > 1 { s.count.to_string() } else { String::new() };
            PixelText::set(count, &label);
        }
        None => {
            *vis = Visibility::Hidden;
            PixelText::set(count, "");
        }
    }
}

/// Colour of a durability bar: green when new, red when about to break.
pub fn wear_color(wear: f32) -> Color {
    let left = 1.0 - wear;
    Color::srgb((1.0 - left) * 1.6, (left * 1.6).min(0.9), 0.1)
}

/// Keeps every slot icon, count label and wear bar in sync with the inventory.
pub fn refresh_slots(
    inventory: Res<Inventory>,
    mut icons: Query<(&SlotIcon, &mut ImageNode, &mut Visibility), (Without<SlotCount>, Without<SlotWear>)>,
    mut counts: Query<(&SlotCount, &mut PixelText)>,
    mut wear: Query<
        (&SlotWear, &mut Node, &mut BackgroundColor, &mut Visibility),
        (Without<SlotIcon>, Without<SlotCount>),
    >,
    slots: Query<(&SlotView, &ComputedNode)>,
) {
    if !inventory.is_changed() {
        return;
    }
    for (SlotIcon(i), mut icon, mut vis) in &mut icons {
        match inventory.slots[*i] {
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
        PixelText::set(&mut text, &label);
    }
    for (SlotWear(i), mut node, mut bg, mut vis) in &mut wear {
        match inventory.slots[*i].and_then(|s| s.wear()).filter(|w| *w > 0.0) {
            Some(w) => {
                let size =
                    slots.iter().find(|(s, _)| s.0 == *i).map_or(44.0, |(_, c)| c.size().x * c.inverse_scale_factor());
                node.width = Val::Px(((1.0 - w) * (size * 0.76)).max(1.0));
                bg.0 = wear_color(w);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

pub struct WidgetsPlugin;

impl Plugin for WidgetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveSlider>()
            .init_resource::<FocusedField>()
            .init_resource::<CreateForm>()
            .add_message::<UiClick>()
            .add_systems(
                Update,
                (
                    focus_fields,
                    text_input,
                    button_system,
                    drag_sliders,
                    update_sliders,
                    update_field_text,
                    refresh_slots,
                )
                    .chain()
                    .after(super::screens::ScreensSet),
            );
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
    fn fov_slider_covers_60_to_110() {
        assert_eq!(slider_value(SliderKind::Fov, 0.0), 60.0);
        assert_eq!(slider_value(SliderKind::Fov, 1.0), 110.0);
    }

    #[test]
    fn slider_value_and_fraction_roundtrip() {
        for kind in [
            SliderKind::RenderDistance,
            SliderKind::Sensitivity,
            SliderKind::Fov,
            SliderKind::MasterVolume,
            SliderKind::MusicVolume,
            SliderKind::WeatherVolume,
        ] {
            for i in 0..=20 {
                let v = slider_value(kind, i as f32 / 20.0);
                let back = slider_value(kind, slider_fraction(kind, v));
                assert!((v - back).abs() < 1e-3, "{kind:?}: {v} vs {back}");
            }
        }
        assert_eq!(slider_value(SliderKind::Fov, 5.0), MAX_FOV, "positions beyond the track clamp");
        assert!((slider_value(SliderKind::Sensitivity, 0.5) - 1.05).abs() < 1e-4);
        assert_eq!(slider_value(SliderKind::MasterVolume, 0.5), 0.5);
    }

    #[test]
    fn slider_labels_show_the_live_values() {
        let s = GameSettings { fov: 95.0, mouse_sensitivity: 0.5, music_volume: 0.25, ..default() };
        let w = WorldSettings { seed: 1, render_distance: 12, save_dir: "x".into() };
        assert_eq!(slider_text(SliderKind::RenderDistance, &s, &w), "Render Distance: 12 chunks");
        assert_eq!(slider_text(SliderKind::Fov, &s, &w), "FOV: 95");
        assert_eq!(slider_text(SliderKind::Sensitivity, &s, &w), "Mouse Sensitivity: 50%");
        assert_eq!(slider_text(SliderKind::MusicVolume, &s, &w), "Music: 25%");
    }

    #[test]
    fn text_fields_take_printable_ascii_up_to_their_limit() {
        let mut s = String::new();
        for c in "Hi there\u{e9}\n!".chars() {
            push_char(&mut s, c, 8);
        }
        assert_eq!(s, "Hi there", "non-ASCII and control characters are dropped, the limit holds");
        assert!(accepts_char('~') && !accepts_char('\t') && !accepts_char('\u{1F600}'));
    }

    #[test]
    fn wear_bar_goes_from_green_to_red() {
        let green = wear_color(0.0).to_srgba();
        let red = wear_color(1.0).to_srgba();
        assert!(green.green > green.red && red.red > red.green);
    }

    #[test]
    fn create_form_starts_with_a_default_name_and_random_seed() {
        let f = CreateForm::default();
        assert_eq!((f.name.as_str(), f.seed.as_str(), f.mode), ("New World", "", GameMode::Survival));
    }
}
