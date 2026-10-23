//! Player-adjustable options. They are global (one `options.json` next to the worlds) and take
//! effect immediately: systems read [`GameSettings`] every frame, or react to its change flag.
//! The render distance is part of [`WorldSettings`](crate::world::streaming::WorldSettings) because the
//! streaming code owns it, but it is stored in the same file.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MIN_RENDER_DISTANCE: i32 = 2;
pub const MAX_RENDER_DISTANCE: i32 = 32;
pub const DEFAULT_RENDER_DISTANCE: i32 = 10;
pub const MIN_FOV: f32 = 60.0;
pub const MAX_FOV: f32 = 110.0;
pub const DEFAULT_FOV: f32 = 80.0;
pub const MIN_SENSITIVITY: f32 = 0.1;
pub const MAX_SENSITIVITY: f32 = 2.0;
/// Folder that holds every world and the options file.
pub const SAVES_DIR: &str = "saves";

/// How a key-driven mode (sprint, sneak) behaves.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum KeyMode {
    /// Active while the key is held.
    #[default]
    Hold,
    /// Each press switches the mode on or off.
    Toggle,
}

impl KeyMode {
    pub fn flip(self) -> Self {
        match self {
            KeyMode::Hold => KeyMode::Toggle,
            KeyMode::Toggle => KeyMode::Hold,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KeyMode::Hold => "Hold",
            KeyMode::Toggle => "Toggle",
        }
    }
}

/// How many debris particles blocks and mobs spawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ParticleLevel {
    #[default]
    All,
    Decreased,
    Minimal,
}

impl ParticleLevel {
    pub fn next(self) -> Self {
        match self {
            ParticleLevel::All => ParticleLevel::Decreased,
            ParticleLevel::Decreased => ParticleLevel::Minimal,
            ParticleLevel::Minimal => ParticleLevel::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ParticleLevel::All => "All",
            ParticleLevel::Decreased => "Decreased",
            ParticleLevel::Minimal => "Minimal",
        }
    }

    /// Share of the full particle count.
    pub fn factor(self) -> f32 {
        match self {
            ParticleLevel::All => 1.0,
            ParticleLevel::Decreased => 0.5,
            ParticleLevel::Minimal => 0.15,
        }
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameSettings {
    // Graphics
    /// Field of view in degrees (before the sprint widening).
    pub fov: f32,
    pub fullscreen: bool,
    pub vsync: bool,
    pub particles: ParticleLevel,
    // Controls and gameplay
    /// Multiplier on the base mouse speed; `1.0` is 100 %.
    pub mouse_sensitivity: f32,
    pub sprint_mode: KeyMode,
    pub sneak_mode: KeyMode,
    pub invert_mouse: bool,
    // Audio (stored for the sound system to come)
    pub master_volume: f32,
    pub music_volume: f32,
    pub weather_volume: f32,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            fov: DEFAULT_FOV,
            fullscreen: false,
            vsync: true,
            particles: ParticleLevel::All,
            mouse_sensitivity: 1.0,
            sprint_mode: KeyMode::Hold,
            sneak_mode: KeyMode::Hold,
            invert_mouse: false,
            master_volume: 1.0,
            music_volume: 0.5,
            weather_volume: 1.0,
        }
    }
}

impl GameSettings {
    /// Pulls every value into its valid range (guards against hand-edited files).
    pub fn sanitized(mut self) -> Self {
        let fix = |v: f32, lo: f32, hi: f32, default: f32| if v.is_finite() { v.clamp(lo, hi) } else { default };
        self.fov = fix(self.fov, MIN_FOV, MAX_FOV, DEFAULT_FOV);
        self.mouse_sensitivity = fix(self.mouse_sensitivity, MIN_SENSITIVITY, MAX_SENSITIVITY, 1.0);
        self.master_volume = fix(self.master_volume, 0.0, 1.0, 1.0);
        self.music_volume = fix(self.music_volume, 0.0, 1.0, 0.5);
        self.weather_volume = fix(self.weather_volume, 0.0, 1.0, 1.0);
        self
    }
}

pub fn clamp_render_distance(d: i32) -> i32 {
    d.clamp(MIN_RENDER_DISTANCE, MAX_RENDER_DISTANCE)
}

/// What `options.json` contains.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct OptionsFile {
    pub render_distance: i32,
    #[serde(flatten)]
    pub settings: GameSettings,
}

impl Default for OptionsFile {
    fn default() -> Self {
        Self { render_distance: DEFAULT_RENDER_DISTANCE, settings: GameSettings::default() }
    }
}

pub fn options_path(root: &Path) -> PathBuf {
    root.join("options.json")
}

/// Reads the options file; a missing or damaged file yields the defaults.
pub fn load_options(root: &Path) -> OptionsFile {
    let Ok(bytes) = std::fs::read(options_path(root)) else { return OptionsFile::default() };
    match serde_json::from_slice::<OptionsFile>(&bytes) {
        Ok(mut o) => {
            o.settings = o.settings.sanitized();
            o.render_distance = clamp_render_distance(o.render_distance);
            o
        }
        Err(e) => {
            bevy::log::warn!("options.json is damaged ({e}); using defaults");
            OptionsFile::default()
        }
    }
}

pub fn save_options(root: &Path, options: &OptionsFile) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let json = serde_json::to_vec_pretty(options).map_err(std::io::Error::other)?;
    let tmp = root.join("options.json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, options_path(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizing_clamps_and_rejects_nan() {
        let s = GameSettings { fov: 500.0, mouse_sensitivity: f32::NAN, fullscreen: true, ..default() }.sanitized();
        assert_eq!((s.fov, s.mouse_sensitivity, s.fullscreen), (MAX_FOV, 1.0, true));
        let low = GameSettings { fov: 10.0, master_volume: 7.0, music_volume: -1.0, ..default() }.sanitized();
        assert_eq!((low.fov, low.master_volume, low.music_volume), (MIN_FOV, 1.0, 0.0));
        assert_eq!(clamp_render_distance(99), 32);
        assert_eq!(clamp_render_distance(-3), 2);
    }

    #[test]
    fn options_round_trip_through_disk() {
        let dir = std::env::temp_dir().join(format!("mk_opts_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load_options(&dir), OptionsFile::default(), "no file means defaults");
        let o = OptionsFile {
            render_distance: 24,
            settings: GameSettings {
                fov: 95.0,
                vsync: false,
                particles: ParticleLevel::Minimal,
                sprint_mode: KeyMode::Toggle,
                sneak_mode: KeyMode::Toggle,
                invert_mouse: true,
                music_volume: 0.25,
                ..default()
            },
        };
        save_options(&dir, &o).unwrap();
        assert_eq!(load_options(&dir), o);
        // Garbage and out-of-range values never break the game.
        std::fs::write(options_path(&dir), "{ nope").unwrap();
        assert_eq!(load_options(&dir), OptionsFile::default());
        std::fs::write(options_path(&dir), r#"{"render_distance": 500, "fov": 1000.0}"#).unwrap();
        let clamped = load_options(&dir);
        assert_eq!((clamped.render_distance, clamped.settings.fov), (MAX_RENDER_DISTANCE, MAX_FOV));
        assert_eq!(clamped.settings.sprint_mode, KeyMode::Hold, "missing fields use defaults");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn mode_helpers() {
        assert_eq!(KeyMode::Hold.flip(), KeyMode::Toggle);
        assert_eq!(KeyMode::Toggle.flip().label(), "Hold");
        assert_eq!(ParticleLevel::All.next().next().next(), ParticleLevel::All);
        assert!(ParticleLevel::Minimal.factor() < ParticleLevel::Decreased.factor());
    }
}
