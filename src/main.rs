//! Bingle 3D: a native client for the Bingle game server.
//!
//! Stage 4 of docs/plan.md: a panel that connects to a Bingle server and lists its games, and a
//! table in 3D on which a game can be watched, or played against the server's bots, from the API
//! alone.

mod appearance;
mod assets;
mod board;
mod browser;
mod call;
mod camera;
mod cli;
mod clock;
mod config;
mod coordinates;
mod families;
mod fonts;
mod friends;
mod game_panel;
mod hud;
mod lobby;
mod look;
mod net;
mod new_game;
mod play;
mod run_limit;
mod scene;
mod sign_in;
mod slab;
mod table;
mod textures;
mod toggles;
mod voice;
mod voice_chat;
mod wood_choice;

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
    if opts.voice_check {
        return match voice::check::run() {
            Ok(()) => AppExit::Success,
            Err(e) => {
                eprintln!("bingle-3d: voice check: {e}");
                AppExit::from_code(1)
            }
        };
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
    // The files the scene is drawn with, and how each game looks.
    .add_plugins((
        assets::AssetsPlugin,
        textures::TexturesPlugin,
        appearance::AppearancePlugin,
        wood_choice::WoodChoicePlugin,
        coordinates::CoordinatesPlugin,
        toggles::TogglesPlugin,
    ))
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
            sign_in: opts.sign_in,
        },
        sign_in::SignInPlugin,
        friends::FriendsPlugin {
            open: opts.friends || opts.talk.is_some() || opts.call.is_some(),
            talk: opts.talk.clone(),
        },
        call::CallPlugin {
            no_devices: opts.no_audio_devices,
            call: opts.call.clone(),
            relay_only: opts.relay_only,
        },
        voice_chat::VoiceChatPlugin {
            no_devices: opts.no_audio_devices,
            join: opts.voice,
            relay_only: opts.relay_only,
        },
        // Tab moves between the fields of a form.
        bevy::input_focus::tab_navigation::TabNavigationPlugin,
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
