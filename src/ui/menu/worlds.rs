//! Singleplayer: the list of saved worlds and the form that creates a new one.

use super::{panel, spawn_page};
use crate::session::{
    AppState, GameMode, PendingGame, WorldEntry, delete_world, list_worlds, seed_from_text, unique_world_dir,
};
use crate::settings::SAVES_DIR;
use crate::ui::font::PixelText;
use crate::ui::screens::Screen;
use crate::ui::widgets::{
    ActionLabel, CreateForm, Disabled, FieldId, FocusedField, TEXT_DIM, TEXT_GOLD, UiAction, UiButton, UiClick,
    spawn_button, spawn_text_field,
};
use bevy::prelude::*;
use std::path::PathBuf;

const ROWS: usize = 5;
const ROW_W: f32 = 600.0;

/// The worlds found on disk and the state of the list widget.
#[derive(Resource, Default)]
pub struct WorldList {
    pub entries: Vec<WorldEntry>,
    pub selected: Option<usize>,
    pub scroll: usize,
    /// The delete button was pressed once and waits for a second click.
    pub confirm_delete: bool,
    /// A click on a row that was already selected starts the world.
    last_click: Option<(usize, f32)>,
}

impl WorldList {
    pub fn visible_range(&self) -> std::ops::Range<usize> {
        self.scroll..(self.scroll + ROWS).min(self.entries.len())
    }

    fn clamp_scroll(&mut self) {
        let max = self.entries.len().saturating_sub(ROWS);
        self.scroll = self.scroll.min(max);
    }

    /// Keeps the selection visible.
    fn reveal_selected(&mut self) {
        if let Some(i) = self.selected {
            if i < self.scroll {
                self.scroll = i;
            } else if i >= self.scroll + ROWS {
                self.scroll = i + 1 - ROWS;
            }
        }
        self.clamp_scroll();
    }
}

#[derive(Component)]
struct WorldRow(usize);

#[derive(Component)]
struct RowName(usize);

#[derive(Component)]
struct RowInfo(usize);

#[derive(Component)]
struct EmptyHint;

pub struct WorldsPlugin;

impl Plugin for WorldsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldList>()
            .add_systems(Startup, (spawn_world_list, spawn_create_page))
            .add_systems(Update, (refresh_on_open, scroll_list, update_rows, world_clicks, create_clicks).chain());
    }
}

fn spawn_world_list(mut commands: Commands) {
    let page = spawn_page(&mut commands, Screen::Worlds, 0.55, 30);
    commands.entity(page).with_children(|root| {
        root.spawn((
            PixelText::new("Select World").scale(4.0),
            Node { margin: UiRect::bottom(Val::Px(6.0)), ..default() },
        ));
        root.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            min_height: Val::Px(ROWS as f32 * 62.0),
            ..default()
        })
        .with_children(|list| {
            for i in 0..ROWS {
                list.spawn((
                    WorldRow(i),
                    UiButton { action: UiAction::SelectWorld(i), screen: Screen::Worlds },
                    bevy::ui::RelativeCursorPosition::default(),
                    Node {
                        width: Val::Px(ROW_W),
                        height: Val::Px(56.0),
                        border: UiRect::all(Val::Px(3.0)),
                        padding: UiRect::horizontal(Val::Px(12.0)),
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::Center,
                        row_gap: Val::Px(2.0),
                        ..default()
                    },
                    BackgroundColor(crate::ui::widgets::BUTTON_BG),
                    crate::ui::widgets::bevel(false),
                ))
                .with_children(|row| {
                    row.spawn((RowName(i), PixelText::new("")));
                    row.spawn((RowInfo(i), PixelText::new("").color(TEXT_DIM)));
                });
            }
            list.spawn((
                EmptyHint,
                PixelText::new("No worlds yet - create one!").color(TEXT_GOLD),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(20.0),
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ));
        });
        spawn_button(root, Screen::Worlds, UiAction::PlaySelected, "Play Selected World", ROW_W);
        root.spawn(Node { column_gap: Val::Px(8.0), ..default() }).with_children(|row| {
            spawn_button(row, Screen::Worlds, UiAction::OpenCreate, "Create New World", 296.0);
            spawn_button(row, Screen::Worlds, UiAction::DeleteSelected, "Delete World", 296.0);
        });
        spawn_button(root, Screen::Worlds, UiAction::BackToTitle, "Cancel", ROW_W);
    });
}

fn spawn_create_page(mut commands: Commands) {
    let page = spawn_page(&mut commands, Screen::CreateWorld, 0.55, 30);
    commands.entity(page).with_children(|root| {
        root.spawn(panel()).with_children(|p| {
            p.spawn((
                PixelText::new("Create New World").scale(3.0),
                Node { margin: UiRect::bottom(Val::Px(8.0)), ..default() },
            ));
            p.spawn(PixelText::new("World Name").color(TEXT_DIM));
            spawn_text_field(p, Screen::CreateWorld, FieldId::WorldName, "New World", 24);
            p.spawn((
                PixelText::new("Seed (leave blank for a random world)").color(TEXT_DIM),
                Node { margin: UiRect::top(Val::Px(6.0)), ..default() },
            ));
            spawn_text_field(p, Screen::CreateWorld, FieldId::Seed, "random", 24);
            p.spawn(Node { height: Val::Px(8.0), ..default() });
            spawn_button(p, Screen::CreateWorld, UiAction::ToggleGameMode, "Game Mode: Survival", 380.0);
            spawn_button(p, Screen::CreateWorld, UiAction::CreateWorld, "Create New World", 380.0);
            spawn_button(p, Screen::CreateWorld, UiAction::CancelCreate, "Cancel", 380.0);
        });
    });
}

/// Rescans the saves folder whenever the world list opens.
fn refresh_on_open(
    screen: Res<Screen>,
    mut list: ResMut<WorldList>,
    mut form: ResMut<CreateForm>,
    mut focus: ResMut<FocusedField>,
) {
    if !screen.is_changed() {
        return;
    }
    match *screen {
        Screen::Worlds => {
            // Keep the selection on the same folder if possible.
            let keep = list.selected.and_then(|i| list.entries.get(i)).map(|e| e.dir.clone());
            list.entries = list_worlds(&PathBuf::from(SAVES_DIR));
            list.selected = keep
                .and_then(|d| list.entries.iter().position(|e| e.dir == d))
                .or(if list.entries.is_empty() { None } else { Some(0) });
            list.confirm_delete = false;
            list.last_click = None;
            list.reveal_selected();
        }
        Screen::CreateWorld => {
            *form = CreateForm::default();
            focus.0 = Some(FieldId::WorldName);
        }
        _ => {
            focus.0 = None;
        }
    }
}

fn scroll_list(
    screen: Res<Screen>,
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    mut list: ResMut<WorldList>,
) {
    if *screen != Screen::Worlds || scroll.delta.y == 0.0 {
        return;
    }
    let max = list.entries.len().saturating_sub(ROWS);
    list.scroll = if scroll.delta.y > 0.0 { list.scroll.saturating_sub(1) } else { (list.scroll + 1).min(max) };
}

fn mode_icon(m: GameMode) -> &'static str {
    match m {
        GameMode::Creative => "Creative",
        GameMode::Survival => "Survival",
    }
}

pub fn age_text(last_played: u64, now: u64) -> String {
    if last_played == 0 {
        return "never played".into();
    }
    let secs = now.saturating_sub(last_played);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86399 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86400),
    }
}

#[allow(clippy::too_many_arguments)]
fn update_rows(
    list: Res<WorldList>,
    screen: Res<Screen>,
    mut rows: Query<(&WorldRow, &mut Node, &mut BorderColor), Without<EmptyHint>>,
    mut names: Query<(&RowName, &mut PixelText), Without<RowInfo>>,
    mut infos: Query<(&RowInfo, &mut PixelText), Without<RowName>>,
    mut hint: Query<&mut Node, (With<EmptyHint>, Without<WorldRow>)>,
    buttons: Query<(Entity, &UiButton, Has<Disabled>)>,
    mut labels: Query<(&ActionLabel, &mut PixelText), (Without<RowName>, Without<RowInfo>)>,
    mut commands: Commands,
) {
    if *screen != Screen::Worlds {
        return;
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    for (WorldRow(i), mut node, mut border) in &mut rows {
        let index = list.scroll + i;
        let want = if index < list.entries.len() { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
        if list.selected == Some(index) && index < list.entries.len() {
            *border = BorderColor::all(Color::WHITE);
        } else {
            *border = crate::ui::widgets::bevel(false);
        }
    }
    for (RowName(i), mut text) in &mut names {
        let label = list.entries.get(list.scroll + i).map_or("", |e| e.name.as_str()).to_string();
        PixelText::set(&mut text, &label);
    }
    for (RowInfo(i), mut text) in &mut infos {
        let label = list.entries.get(list.scroll + i).map_or(String::new(), |e| {
            format!("{}  -  seed {}  -  {}", mode_icon(e.mode), e.seed, age_text(e.last_played, now))
        });
        PixelText::set(&mut text, &label);
    }
    if let Ok(mut h) = hint.single_mut() {
        h.display = if list.entries.is_empty() { Display::Flex } else { Display::None };
    }
    // The play and delete buttons need a selection; delete asks twice.
    let has = list.selected.is_some();
    for (e, b, disabled) in &buttons {
        if matches!(b.action, UiAction::PlaySelected | UiAction::DeleteSelected) && disabled == has {
            if has {
                commands.entity(e).remove::<Disabled>();
            } else {
                commands.entity(e).insert(Disabled);
            }
        }
    }
    for (label, mut text) in &mut labels {
        if label.0 == UiAction::DeleteSelected {
            PixelText::set(&mut text, if list.confirm_delete { "Really delete? Click again" } else { "Delete World" });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn world_clicks(
    time: Res<Time<Real>>,
    mut clicks: MessageReader<UiClick>,
    mut screen: ResMut<Screen>,
    mut list: ResMut<WorldList>,
    mut commands: Commands,
    mut next: ResMut<NextState<AppState>>,
) {
    for UiClick(action) in clicks.read() {
        match *action {
            UiAction::SelectWorld(row) => {
                let index = list.scroll + row;
                if index >= list.entries.len() {
                    continue;
                }
                // Clicking the selected world again (a double click) starts it.
                let now = time.elapsed_secs();
                let double = matches!(list.last_click, Some((i, t)) if i == index && now - t < 0.6);
                list.selected = Some(index);
                list.confirm_delete = false;
                list.last_click = Some((index, now));
                if double {
                    start(&mut commands, &mut next, &list.entries[index]);
                }
            }
            UiAction::PlaySelected => {
                if let Some(e) = list.selected.and_then(|i| list.entries.get(i)) {
                    start(&mut commands, &mut next, e);
                }
            }
            UiAction::DeleteSelected => {
                let Some(i) = list.selected.filter(|i| *i < list.entries.len()) else { continue };
                if !list.confirm_delete {
                    list.confirm_delete = true;
                    continue;
                }
                let dir = list.entries[i].dir.clone();
                if let Err(e) = delete_world(&PathBuf::from(SAVES_DIR), &dir) {
                    error!("cannot delete {}: {e}", dir.display());
                }
                list.entries.remove(i);
                list.selected = if list.entries.is_empty() { None } else { Some(i.min(list.entries.len() - 1)) };
                list.confirm_delete = false;
                list.reveal_selected();
            }
            UiAction::OpenCreate => *screen = Screen::CreateWorld,
            UiAction::BackToTitle if *screen == Screen::Worlds => *screen = Screen::Title,
            _ => {}
        }
    }
}

fn start(commands: &mut Commands, next: &mut NextState<AppState>, e: &WorldEntry) {
    commands.insert_resource(PendingGame { dir: e.dir.clone(), seed: e.seed, mode: e.mode, name: e.name.clone() });
    next.set(AppState::InGame);
}

fn create_clicks(
    mut clicks: MessageReader<UiClick>,
    mut screen: ResMut<Screen>,
    mut form: ResMut<CreateForm>,
    mut commands: Commands,
    mut next: ResMut<NextState<AppState>>,
    mut labels: Query<(&crate::ui::widgets::ActionLabel, &mut PixelText)>,
) {
    for UiClick(action) in clicks.read() {
        match action {
            UiAction::ToggleGameMode => form.mode = form.mode.flip(),
            UiAction::CancelCreate => *screen = Screen::Worlds,
            UiAction::CreateWorld => {
                let name =
                    if form.name.trim().is_empty() { "New World".to_string() } else { form.name.trim().to_string() };
                let root = PathBuf::from(SAVES_DIR);
                let dir = unique_world_dir(&root, &name);
                commands.insert_resource(PendingGame { dir, seed: seed_from_text(&form.seed), mode: form.mode, name });
                next.set(AppState::InGame);
            }
            _ => {}
        }
    }
    for (label, mut text) in &mut labels {
        if label.0 == UiAction::ToggleGameMode {
            PixelText::set(&mut text, &format!("Game Mode: {}", form.mode.label()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(n: usize) -> WorldEntry {
        WorldEntry {
            dir: format!("saves/w{n}").into(),
            name: format!("World {n}"),
            seed: n as u32,
            mode: GameMode::Survival,
            last_played: n as u64,
        }
    }

    #[test]
    fn the_list_shows_a_window_of_five_and_keeps_the_selection_visible() {
        let mut l = WorldList { entries: (0..12).map(entry).collect(), ..default() };
        assert_eq!(l.visible_range(), 0..5);
        l.selected = Some(9);
        l.reveal_selected();
        assert_eq!(l.visible_range(), 5..10);
        l.selected = Some(1);
        l.reveal_selected();
        assert_eq!(l.visible_range(), 1..6);
        l.scroll = 99;
        l.clamp_scroll();
        assert_eq!(l.visible_range(), 7..12);
        let short = WorldList { entries: (0..2).map(entry).collect(), ..default() };
        assert_eq!(short.visible_range(), 0..2);
    }

    #[test]
    fn ages_are_human_readable() {
        assert_eq!(age_text(0, 1000), "never played");
        assert_eq!(age_text(990, 1000), "just now");
        assert_eq!(age_text(1000 - 300, 1000), "5 min ago");
        assert_eq!(age_text(1_000_000 - 7200, 1_000_000), "2 h ago");
        assert_eq!(age_text(1_000_000 - 3 * 86400, 1_000_000), "3 days ago");
    }
}
