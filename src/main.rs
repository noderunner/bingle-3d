//! Bingle 3D: a native client for the Bingle game server.
//!
//! Stage 1 of docs/plan.md: a window with a lit 3D scene, on Linux and Windows.

mod camera;
mod cli;
mod hud;
mod run_limit;
mod scene;

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
    .add_plugins((
        scene::ScenePlugin,
        camera::OrbitCameraPlugin,
        hud::HudPlugin,
    ));
    if let Some(limit) = run_limit::RunLimit::from_options(&opts) {
        app.add_plugins(run_limit::RunLimitPlugin(limit));
    }
    app.run()
}
