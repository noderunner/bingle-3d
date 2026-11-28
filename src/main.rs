//! Bingle 3D: a native client for the Bingle game server.
//!
//! A main screen that connects to a Bingle server and lists its games, and a table in 3D on which
//! a game can be watched or played, from the API alone (docs/plan.md).

mod appearance;
mod arcs;
mod assets;
mod board;
mod bowls;
mod browser;
mod call;
mod camera;
mod card_layout;
mod card_table;
mod chess_clock;
mod choice_grid;
mod cli;
mod clock;
mod config;
mod coordinates;
mod cursor;
mod deal;
mod display;
mod downloads;
mod drag;
mod environment;
mod environment_choice;
mod escape_menu;
mod families;
mod fences;
mod fonts;
mod frame_pace;
mod friends;
mod game_panel;
mod gpu_memory;
mod graphics;
mod hand;
mod hand_strip;
mod highlight;
mod hud;
mod link;
mod link_channel;
mod link_handler;
mod link_open;
mod lobby;
mod login;
mod look;
mod move_tiles;
mod net;
mod new_game;
#[cfg(test)]
mod paint_contract;
mod patience;
mod play;
mod portal;
mod props;
// Used only by the headset so far.
#[cfg_attr(not(feature = "vr"), allow(dead_code))]
mod ray_pointer;
mod run_limit;
mod scene;
mod seat_markers;
mod setup;
mod showcase;
mod sides;
mod sign_in;
mod slab;
mod sounds;
mod supply;
mod table;
mod test_hand;
mod textures;
mod tints;
mod toggles;
mod trays;
mod voice;
mod voice_chat;
mod vr;
// These are used only by the headset so far.
#[cfg_attr(not(feature = "vr"), allow(dead_code))]
mod vr_keyboard;
#[cfg_attr(not(feature = "vr"), allow(dead_code))]
mod vr_panels;
mod warm;
mod wood_choice;
#[cfg_attr(not(feature = "vr"), allow(dead_code))]
mod world_panel;

use bevy::picking::mesh_picking::{MeshPickingPlugin, MeshPickingSettings};
use bevy::prelude::*;
use bevy::window::WindowResolution;

/// The private folder of games' looks (`assets::read_private`): the command line's, else the
/// variable's, else `config.toml`'s.
fn private_dir(opts: &cli::Options) -> Option<std::path::PathBuf> {
    opts.private_dir
        .clone()
        .or_else(|| {
            std::env::var_os(assets::PRIVATE_DIR_VAR)
                .filter(|v| !v.is_empty())
                .map(Into::into)
        })
        .or_else(|| {
            config::Store::load(config::Store::default_dir())
                .config
                .private_dir
        })
}

/// The game a link opens, once connected: its room, to sit in or to watch.
fn link_target(opts: &cli::Options) -> Option<net::Target> {
    let link = opts.link.as_ref()?.as_ref().ok()?;
    Some(match link.action {
        link::Action::Join => net::Target::Join(link.room.clone()),
        link::Action::Watch => net::Target::Room(link.room.clone()),
    })
}

/// `--register-link` and `--unregister-link`: done for the current user, said, and kept in the
/// config as *Settings* keeps it.
fn register_link(on: bool) -> AppExit {
    if !link_handler::supported() {
        eprintln!("bingle-3d: this build does not open game links");
        return AppExit::from_code(2);
    }
    let done = if on {
        std::env::current_exe()
            .map_err(|e| format!("where this program is cannot be read ({e})"))
            .and_then(|exe| {
                link_handler::register(&exe)
                    .map(|()| format!("bingle-3d:// links now open {}", exe.display()))
                    .map_err(|e| e.to_string())
            })
    } else {
        link_handler::unregister()
            .map(|()| "bingle-3d:// links no longer open this program".to_string())
            .map_err(|e| e.to_string())
    };
    match done {
        Ok(said) => {
            let mut store = config::Store::load(config::Store::default_dir());
            if let Err(e) = store.remember_open_links(on) {
                eprintln!("bingle-3d: the config could not be written ({e})");
            }
            println!("{said}");
            AppExit::Success
        }
        Err(e) => {
            eprintln!("bingle-3d: {e}");
            AppExit::from_code(1)
        }
    }
}

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
    // From a script: open game links with this program, or with nothing, and quit.
    if let Some(on) = opts.register_link {
        return register_link(on);
    }
    // A link clicked while a copy runs goes to that copy, which says it took it; on anything
    // else (none running, no answer in time) this copy starts on its own, as it always has.
    if let Some(text) = link_channel::hands_over(&opts) {
        match link_channel::hand_over(text) {
            Ok(()) => {
                println!("bingle-3d: the link was handed over to the copy already running");
                return AppExit::Success;
            }
            Err(link_channel::NotHanded::NoneRunning) => {}
            Err(why) => eprintln!("bingle-3d: game links: {why}: this copy opens the link"),
        }
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

    if opts.vr && !cfg!(feature = "vr") {
        eprintln!("bingle-3d: this build has no VR in it: build it with `--features vr`");
        return AppExit::from_code(2);
    }

    let mut app = App::new();
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bingle".into(),
                // As `display.rs` puts it, once the config is read.
                resolution: WindowResolution::new(display::WINDOWED.0, display::WINDOWED.1),
                ..default()
            }),
            ..default()
        })
        // Uploads wait in system memory, never in a small window into the card's.
        .set(gpu_memory::render_plugin());
    // Bevy's audio opens the speaker as it starts: not in a run that keeps the devices off.
    let plugins = if opts.no_audio_devices {
        plugins.disable::<bevy::audio::AudioPlugin>()
    } else {
        plugins
    };
    // In a headset: when asked, or when one is waiting (its runtime already running).
    #[cfg(feature = "vr")]
    let in_headset = opts.vr
        || (opts.looks_for_headset()
            && vr::runtime_running().is_some_and(|runtime| {
                eprintln!(
                    "bingle-3d: {runtime} is running: starting in the headset \
                     (--desktop stays in the window)"
                );
                true
            }));
    #[cfg(feature = "vr")]
    let plugins = if in_headset {
        vr::with_headset(plugins)
    } else {
        plugins
    };
    app.add_plugins(plugins);
    // After the headset's own plugins: one of them makes a schedule anew, and what was put in
    // it before would be lost.
    #[cfg(feature = "vr")]
    if in_headset {
        app.add_plugins(vr::VrPlugin);
    }
    app
        // Only what is marked `Pickable` (the board's cells and pieces) is picked, from cameras marked
        // for it, so the table and the lights' meshes never get in the way of a click.
        .add_plugins(MeshPickingPlugin)
        .insert_resource(MeshPickingSettings {
            require_markers: true,
            ..default()
        })
        // The files the scene is drawn with, and how each game looks.
        .add_plugins((
            assets::AssetsPlugin {
                private_dir: private_dir(&opts),
            },
            textures::TexturesPlugin,
            appearance::AppearancePlugin,
            wood_choice::WoodChoicePlugin,
            coordinates::CoordinatesPlugin,
            toggles::TogglesPlugin,
            // The screen before the main one.
            login::LoginPlugin,
        ))
        // The network first: it reads the config, which remembers where the table stands.
        .add_plugins(net::NetPlugin {
            server: opts.server.clone(),
            start: opts.start(),
            setup: opts.setup.clone(),
            link: opts.link.clone(),
        })
        // The looks a server offers, fetched as the games are listed and opened.
        .add_plugins(downloads::DownloadsPlugin)
        .insert_resource(camera::StartView(
            opts.view
                .as_deref()
                .and_then(|v| camera::StartView::parse(v).ok()),
        ))
        // Where the table stands.
        .add_plugins((
            environment::EnvironmentPlugin {
                chosen: opts.environment.clone(),
            },
            environment_choice::EnvironmentChoicePlugin,
        ))
        .add_plugins((
            fonts::FontsPlugin,
            scene::ScenePlugin,
            camera::OrbitCameraPlugin,
            hud::HudPlugin,
            browser::BrowserPlugin {
                new_game: opts.new_game.clone(),
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
            (
                board::BoardPlugin,
                board::WarmBoardsPlugin,
                card_table::CardTablePlugin,
                hand_strip::HandStripPlugin,
                (
                    choice_grid::ChoiceGridPlugin,
                    seat_markers::SeatMarkersPlugin,
                ),
                move_tiles::MoveTilesPlugin,
                highlight::HighlightPlugin,
                arcs::ArcsPlugin,
                portal::PortalPlugin,
            ),
            sounds::SoundsPlugin {
                on: !opts.no_audio_devices,
            },
            escape_menu::EscapeMenuPlugin,
            (play::PlayPlugin, drag::DragPlugin),
            game_panel::GamePanelPlugin {
                on_start: link_target(&opts).or(opts.watch.clone().map(|w| match w {
                    cli::Watch::Room(code) => net::Target::Room(code),
                    cli::Watch::Bots(game_def_id) => net::Target::Bots { game_def_id },
                    cli::Watch::PlayBots(game_def_id) => net::Target::AgainstBots { game_def_id },
                    cli::Watch::Host(game_def_id) => net::Target::Host { game_def_id },
                    cli::Watch::Join(code) => net::Target::Join(code),
                })),
            },
        ));
    // A headset's eyes never bloom, so what glows is tuned for them there.
    #[cfg(not(feature = "vr"))]
    let in_headset = false;
    // The pointer's hand, in place of the system's cursor: a headset's pointers are its
    // controllers' rays.
    if !in_headset {
        app.add_plugins(cursor::CursorPlugin);
    }
    app.add_plugins(link_open::LinkOpenPlugin {
        limited: opts.last_frame().is_some(),
        in_headset,
    });
    app.add_plugins(link_channel::LinkChannelPlugin {
        listen: link_channel::listens(&opts, in_headset),
    });
    app.add_plugins(display::DisplayPlugin {
        fullscreen: opts.fullscreen,
        limited: opts.last_frame().is_some(),
        in_headset,
    });
    app.add_plugins(graphics::GraphicsPlugin {
        limited: opts.last_frame().is_some(),
        no_bloom: opts.no_bloom,
        in_headset,
    });
    app.add_plugins(chess_clock::ChessClockPlugin {
        limited: opts.last_frame().is_some(),
    });
    app.add_plugins(frame_pace::FramePacePlugin {
        max_fps: opts.max_fps,
        vr_refresh_rate: opts.vr_refresh_rate,
        limited: opts.last_frame().is_some(),
        in_headset,
    });
    app.insert_resource(scene::BloomOn(!opts.no_bloom && !in_headset))
        .insert_resource(board::ShowLegalMoves(opts.legal_moves));
    if opts.showcase {
        app.add_plugins(showcase::ShowcasePlugin {
            card_face: opts.card_face.clone(),
            card_back: opts.card_back.clone(),
            scripted: opts.last_frame().is_some(),
        });
    }
    if let Some(cards) = opts.test_hand {
        app.add_plugins(test_hand::TestHandPlugin {
            cards,
            deal: opts.test_deal,
            scripted: opts.last_frame().is_some(),
        });
    }
    // What a frame costs, for finding what is slow: the frame's time, and each pass of its
    // drawing as the processor and the graphics card took it.
    if opts.frame_times {
        app.add_plugins((
            bevy::diagnostic::EntityCountDiagnosticsPlugin::default(),
            bevy::render::diagnostic::RenderDiagnosticsPlugin,
            bevy::diagnostic::LogDiagnosticsPlugin::default(),
        ));
    }
    if let Some(limit) = run_limit::RunLimit::from_options(&opts) {
        app.add_plugins(run_limit::RunLimitPlugin(limit));
    }
    app.run()
}
