use bevy::prelude::*;
use bevy::window::PresentMode;
use minecraft_kopie::VoxelGamePlugins;
use minecraft_kopie::settings::{DEFAULT_RENDER_DISTANCE, clamp_render_distance};
use minecraft_kopie::world::storage;
use minecraft_kopie::world::streaming::WorldSettings;
use std::path::PathBuf;

const USAGE: &str = "usage: minecraft_kopie [--seed N] [--render-distance N] [--save-dir PATH]";

fn parse_args() -> WorldSettings {
    let mut seed: Option<u32> = None;
    let mut render_distance: Option<i32> = None;
    let mut save_dir = PathBuf::from("saves/world1");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().unwrap_or_else(|| panic!("{name} needs a value\n{USAGE}"));
        match a.as_str() {
            "--seed" => seed = Some(value("--seed").parse().expect("seed must be a non-negative integer")),
            "--render-distance" => {
                render_distance =
                    Some(clamp_render_distance(value("--render-distance").parse::<i32>().expect("integer")))
            }
            "--save-dir" => save_dir = PathBuf::from(value("--save-dir")),
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => panic!("unknown argument {other}\n{USAGE}"),
        }
    }
    // An existing world keeps its seed and its last render distance (unless --render-distance is
    // given); a new one uses --seed or the clock.
    let level = storage::load_level(&save_dir);
    let seed = level.as_ref().map(|l| l.seed).or(seed).unwrap_or_else(|| {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_secs() as u32)
    });
    let render_distance = render_distance
        .or(level.and_then(|l| l.options.render_distance).map(clamp_render_distance))
        .unwrap_or(DEFAULT_RENDER_DISTANCE);
    WorldSettings { seed, render_distance, save_dir }
}

fn main() {
    let settings = parse_args();
    info!("world seed {}", settings.seed);
    App::new()
        .insert_resource(settings)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Minecraft Kopie".into(),
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            // The save plugin intercepts the close button: it writes the world first, then exits.
            close_when_requested: false,
            ..default()
        }))
        .add_plugins(VoxelGamePlugins)
        .run();
}
