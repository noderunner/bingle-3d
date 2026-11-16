//! `--frames` and `--screenshot`: run for a fixed number of frames, optionally saving what the
//! window shows, then quit. Used by CI to prove that the app starts, renders and exits cleanly.

use std::path::PathBuf;

use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};

use crate::cli::Options;

/// How many frames to wait for a screenshot to arrive before giving up with an error.
const SCREENSHOT_TIMEOUT_FRAMES: u32 = 300;

#[derive(Resource, Debug, Clone)]
pub struct RunLimit {
    last_frame: u32,
    screenshot: Option<PathBuf>,
    requested: bool,
}

impl RunLimit {
    /// The limit the options ask for, or none if the app should run until it is closed.
    pub fn from_options(opts: &Options) -> Option<Self> {
        opts.last_frame().map(|last_frame| Self {
            last_frame,
            screenshot: opts.screenshot.clone(),
            requested: false,
        })
    }
}

pub struct RunLimitPlugin(pub RunLimit);

impl Plugin for RunLimitPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone())
            .add_systems(Last, stop_at_last_frame);
    }
}

fn stop_at_last_frame(
    mut commands: Commands,
    mut limit: ResMut<RunLimit>,
    frames: Res<FrameCount>,
    mut exit: MessageWriter<AppExit>,
) {
    if frames.0 < limit.last_frame {
        return;
    }
    let Some(path) = limit.screenshot.clone() else {
        info!("stopping after {} frames, as asked", frames.0);
        exit.write(AppExit::Success);
        return;
    };
    if !limit.requested {
        limit.requested = true;
        info!(
            "frame {}: taking a screenshot for {}",
            frames.0,
            path.display()
        );
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()))
            .observe(
                move |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                    // save_to_disk, the observer before this one, writes the file.
                    info!("captured the screenshot; quitting");
                    exit.write(AppExit::Success);
                },
            );
    } else if frames.0 >= limit.last_frame + SCREENSHOT_TIMEOUT_FRAMES {
        error!("no screenshot arrived within {SCREENSHOT_TIMEOUT_FRAMES} frames");
        exit.write(AppExit::error());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_options_no_limit() {
        assert!(RunLimit::from_options(&Options::default()).is_none());
    }

    #[test]
    fn stops_after_the_frames_asked_for() {
        let limit = RunLimit::from_options(&Options {
            frames: Some(3),
            ..default()
        })
        .unwrap();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, RunLimitPlugin(limit)));
        let mut exit = None;
        for _ in 0..10 {
            app.update();
            exit = app.should_exit();
            if exit.is_some() {
                break;
            }
        }
        assert_eq!(exit, Some(AppExit::Success));
        assert_eq!(app.world().resource::<FrameCount>().0, 3);
    }
}
