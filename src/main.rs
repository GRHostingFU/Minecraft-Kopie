use bevy::prelude::*;
use bevy::window::PresentMode;
use minecraft_kopie::VoxelGamePlugins;
use minecraft_kopie::session::{AutoStart, GameMode, PendingGame, seed_from_text};
use minecraft_kopie::settings::{self, SAVES_DIR};
use minecraft_kopie::world::streaming::WorldSettings;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: minecraft_kopie [--save-dir PATH [--seed N] [--creative]] [--render-distance N]\n\
    Without --save-dir the title screen opens. With it, that world is created or loaded and started directly.";

/// What the command line asks for.
struct Args {
    seed: Option<String>,
    render_distance: Option<i32>,
    save_dir: Option<PathBuf>,
    creative: bool,
}

fn parse_args() -> Args {
    let mut args = Args { seed: None, render_distance: None, save_dir: None, creative: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().unwrap_or_else(|| panic!("{name} needs a value\n{USAGE}"));
        match a.as_str() {
            "--seed" => args.seed = Some(value("--seed")),
            "--render-distance" => {
                args.render_distance =
                    Some(settings::clamp_render_distance(value("--render-distance").parse::<i32>().expect("integer")))
            }
            "--save-dir" => args.save_dir = Some(PathBuf::from(value("--save-dir"))),
            "--creative" => args.creative = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => panic!("unknown argument {other}\n{USAGE}"),
        }
    }
    args
}

fn main() {
    let args = parse_args();
    // Global options (video, controls, audio) are shared by every world.
    let options = settings::load_options(Path::new(SAVES_DIR));
    let render_distance = args.render_distance.unwrap_or(options.render_distance);

    let mut app = App::new();
    // The world settings of the title screen's panorama; a started world replaces them.
    app.insert_resource(WorldSettings {
        seed: 1,
        render_distance,
        save_dir: PathBuf::from(SAVES_DIR).join(".panorama"),
    })
    .insert_resource(options.settings);
    let autostart = args.save_dir.map(|dir| {
        let name = dir.file_name().map_or("World".to_string(), |n| n.to_string_lossy().into_owned());
        let mode = if args.creative { GameMode::Creative } else { GameMode::Survival };
        PendingGame { dir, seed: seed_from_text(args.seed.as_deref().unwrap_or("")), mode, name }
    });
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Minecraft Kopie".into(),
            present_mode: if options.settings.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync },
            ..default()
        }),
        // The save plugin intercepts the close button: it writes the world first, then exits.
        close_when_requested: false,
        ..default()
    }));
    if let Some(pending) = autostart {
        // `--save-dir` skips the title screen: the state machine starts inside that world.
        app.insert_resource(pending).insert_resource(AutoStart);
    }
    app.add_plugins(VoxelGamePlugins).run();
}
