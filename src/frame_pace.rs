//! How many frames a second the window draws at most. A board game needs few, and the monitor's
//! own pace (vsync) can be 144 or 240 a second on a laptop, each frame drawing the whole table
//! with its shadows, ambient occlusion, anti-aliasing, depth of field and bloom: the fans spin
//! for nothing. So each frame ends by waiting until the next is due.
//!
//! The limit is `max_fps` in `config.toml`, or `--max-fps` for a run; 60 when neither says, and
//! 0 for none (the monitor's pace). In a headset there is none: the headset sets the pace.
//!
//! The wait is the main world's, at the end of its frame; rendering, a frame behind on its own
//! thread, waits for it, so both keep the pace.

use std::time::{Duration, Instant};

use bevy::prelude::*;

/// The limit when nothing says otherwise.
pub const DEFAULT_MAX_FPS: u32 = 60;

/// What `config.toml` keeps (`max_fps`), put here by the network's plugin, which reads it.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Kept(pub Option<u32>);

/// The longest a frame may be shortened to, or none to draw at the monitor's pace.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTime(pub Option<Duration>);

impl FrameTime {
    /// From frames a second: 0 is no limit.
    pub fn from_fps(fps: u32) -> Self {
        FrameTime((fps > 0).then(|| Duration::from_secs_f64(1.0 / f64::from(fps))))
    }
}

/// The run's limit: the command line's, else the config's, else the default.
pub fn chosen(cli: Option<u32>, kept: Option<u32>) -> u32 {
    cli.or(kept).unwrap_or(DEFAULT_MAX_FPS)
}

/// When the frame after one ending `now` is due, given when this one was (`due`), and how long
/// to wait for it. Frames keep a steady beat: each is due a frame's time after the last was
/// due, not after it ended, so the time the work of a frame takes is not added to its wait. One
/// that ends past when the next was due starts the beat again from now, rather than hurrying the
/// frames after it to catch up.
pub fn next(due: Option<Instant>, now: Instant, frame: Duration) -> (Instant, Duration) {
    let due = match due {
        Some(due) if due + frame > now => due + frame,
        _ => now,
    };
    (due, due.saturating_duration_since(now))
}

/// The pace, when there is one: not in a headset.
pub struct FramePacePlugin {
    /// `--max-fps`, which comes before the config's.
    pub cli: Option<u32>,
}

impl Plugin for FramePacePlugin {
    fn build(&self, app: &mut App) {
        let kept = app
            .world()
            .get_resource::<Kept>()
            .copied()
            .unwrap_or_default();
        let fps = chosen(self.cli, kept.0);
        info!(
            "drawing at most {}",
            if fps == 0 {
                "at the monitor's pace".to_string()
            } else {
                format!("{fps} frames a second")
            }
        );
        app.insert_resource(FrameTime::from_fps(fps))
            .add_systems(Last, wait_for_next_frame);
    }
}

fn wait_for_next_frame(limit: Res<FrameTime>, mut due: Local<Option<Instant>>) {
    let Some(frame) = limit.0 else {
        *due = None;
        return;
    };
    let (next_due, wait) = next(*due, Instant::now(), frame);
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
    *due = Some(next_due);
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(16);

    #[test]
    fn the_command_line_comes_before_the_config_and_both_before_the_default() {
        assert_eq!(chosen(None, None), 60);
        assert_eq!(chosen(None, Some(30)), 30);
        assert_eq!(chosen(Some(0), Some(30)), 0);
        assert_eq!(chosen(Some(144), None), 144);
    }

    #[test]
    fn none_is_no_limit() {
        assert_eq!(FrameTime::from_fps(0), FrameTime(None));
        let sixty = FrameTime::from_fps(60).0.unwrap();
        assert!((sixty.as_secs_f64() - 1.0 / 60.0).abs() < 1e-9);
    }

    #[test]
    fn the_first_frame_does_not_wait() {
        let now = Instant::now();
        assert_eq!(next(None, now, FRAME), (now, Duration::ZERO));
    }

    #[test]
    fn a_quick_frame_waits_for_the_beat() {
        let start = Instant::now();
        let now = start + Duration::from_millis(5);
        assert_eq!(
            next(Some(start), now, FRAME),
            (start + FRAME, Duration::from_millis(11))
        );
    }

    #[test]
    fn the_wait_counts_from_when_the_last_was_due() {
        // Due at 16 ms, its work done at 20: the next is due at 32, not 36.
        let start = Instant::now();
        let (due, _) = next(Some(start), start + FRAME, FRAME);
        let now = start + Duration::from_millis(20);
        assert_eq!(
            next(Some(due), now, FRAME),
            (start + 2 * FRAME, Duration::from_millis(12))
        );
    }

    #[test]
    fn a_frame_far_behind_starts_the_beat_again() {
        let start = Instant::now();
        let now = start + Duration::from_millis(100);
        assert_eq!(next(Some(start), now, FRAME), (now, Duration::ZERO));
    }

    #[test]
    fn a_headless_app_keeps_the_pace() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Kept(Some(50)))
            .add_plugins(FramePacePlugin { cli: None });
        assert_eq!(
            *app.world().resource::<FrameTime>(),
            FrameTime::from_fps(50)
        );
        let start = Instant::now();
        for _ in 0..6 {
            app.update();
        }
        // Six frames at 50 a second: the first is not held, the five after are, 20 ms each.
        assert!(start.elapsed() >= Duration::from_millis(100));
    }
}
