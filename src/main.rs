//! Bingle 3D: a native client for the Bingle game server.
//!
//! Stage 4 of docs/plan.md: a panel that connects to a Bingle server and lists its games, and a
//! table in 3D on which a game can be watched, or played against the server's bots, from the API
//! alone.

mod board;
mod browser;
mod camera;
mod cli;
mod clock;
mod config;
mod families;
mod fonts;
mod game_panel;
mod hud;
mod lobby;
mod look;
mod net;
mod new_game;
mod play;
mod run_limit;
mod scene;
mod table;

use bevy::picking::mesh_picking::{MeshPickingPlugin, MeshPickingSettings};
use bevy::prelude::*;
use bevy::window::WindowResolution;

fn main() -> AppExit {
    let opts = match cli::Options::parse(std::env::args().skip(1)) {
        Ok(opts) => opts,
        Err(msg) => {
            eprintln!("bingle-3d: {msg}\n\n{}", cli::USAGE);
            return AppExit::from_code(2);
        }
    };
    if opts.help {
        println!("{}", cli::USAGE);
        return AppExit::Success;
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Bingle".into(),
            resolution: WindowResolution::new(1280, 720),
            ..default()
        }),
        ..default()
    }))
    // Only what is marked `Pickable` (the board's cells and pieces) is picked, from cameras marked
    // for it, so the table and the lights' meshes never get in the way of a click.
    .add_plugins(MeshPickingPlugin)
    .insert_resource(MeshPickingSettings {
        require_markers: true,
        ..default()
    })
    .add_plugins((
        fonts::FontsPlugin,
        scene::ScenePlugin,
        camera::OrbitCameraPlugin,
        hud::HudPlugin,
        net::NetPlugin {
            server: opts.server.clone(),
        },
        browser::BrowserPlugin {
            new_game: opts.new_game.clone(),
        },
        board::BoardPlugin,
        play::PlayPlugin,
        game_panel::GamePanelPlugin {
            on_start: opts.watch.clone().map(|w| match w {
                cli::Watch::Room(code) => net::Target::Room(code),
                cli::Watch::Bots(game_def_id) => net::Target::Bots { game_def_id },
                cli::Watch::PlayBots(game_def_id) => net::Target::AgainstBots { game_def_id },
                cli::Watch::Host(game_def_id) => net::Target::Host { game_def_id },
                cli::Watch::Join(code) => net::Target::Join(code),
            }),
        },
    ));
    if let Some(limit) = run_limit::RunLimit::from_options(&opts) {
        app.add_plugins(run_limit::RunLimitPlugin(limit));
    }
    app.run()
}
