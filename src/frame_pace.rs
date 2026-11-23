//! How many frames a second the window draws at most. A board game needs few, and the monitor's
//! own pace (vsync) can be 144 or 240 a second on a laptop, each frame drawing the whole table
//! with its shadows, ambient occlusion, anti-aliasing, depth of field and bloom: the fans spin
//! for nothing. So each frame ends by waiting until the next is due.
//!
//! The limit is `max_fps` in `config.toml`, or `--max-fps` for a run; 60 when neither says, and
//! 0 for none (the monitor's pace).
//!
//! The wait is the main world's, at the end of its frame; rendering, a frame behind on its own
//! thread, waits for it, so both keep the pace.
//!
//! A headset is paced by its runtime, which holds each frame until the display is ready for it:
//! a wait of our own there would only make frames miss their turn. What can be chosen is the
//! display's own rate (72, 90, 120 Hz…), which the runtime then keeps: `vr_refresh_rate` in
//! `config.toml`, or `--vr-refresh` for a run, asked of a runtime that offers it (OpenXR's
//! `XR_FB_display_refresh_rate`) by `vr.rs` as its session starts. Without either, or with a
//! runtime that does not offer it (SteamVR has a setting of its own), the runtime's rate.

use std::time::{Duration, Instant};

use bevy::prelude::*;

/// The limit when nothing says otherwise.
pub const DEFAULT_MAX_FPS: u32 = 60;

/// What `config.toml` keeps (`max_fps`, `vr_refresh_rate`), put here by the network's plugin,
/// which reads it.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Kept {
    pub max_fps: Option<u32>,
    pub vr_refresh_rate: Option<u32>,
}

/// The display rate to ask a headset for, in Hz: none leaves it as the runtime has it.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(not(feature = "vr"), allow(dead_code))]
pub struct HeadsetRate(pub Option<u32>);

/// Which of the rates a headset offers to ask for, for `wanted` Hz: the one nearest it, the
/// lower of two as near (it is the quieter), or none when it offers none.
#[cfg_attr(not(feature = "vr"), allow(dead_code))]
pub fn headset_rate(wanted: u32, offered: &[f32]) -> Option<f32> {
    let distance = |rate: f32| (rate - wanted as f32).abs();
    offered
        .iter()
        .copied()
        .filter(|rate| rate.is_finite() && *rate > 0.0)
        .min_by(|a, b| distance(*a).total_cmp(&distance(*b)).then(a.total_cmp(b)))
}

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

/// The pace: a limit in the window, a display rate asked for in a headset.
pub struct FramePacePlugin {
    /// `--max-fps`, which comes before the config's.
    pub max_fps: Option<u32>,
    /// `--vr-refresh`, which comes before the config's.
    pub vr_refresh_rate: Option<u32>,
    /// Drawing in a headset, which sets the pace.
    pub in_headset: bool,
}

impl Plugin for FramePacePlugin {
    fn build(&self, app: &mut App) {
        let kept = app
            .world()
            .get_resource::<Kept>()
            .copied()
            .unwrap_or_default();
        if self.in_headset {
            let rate = self.vr_refresh_rate.or(kept.vr_refresh_rate);
            app.insert_resource(HeadsetRate(rate.filter(|hz| *hz > 0)));
            return;
        }
        let fps = chosen(self.max_fps, kept.max_fps);
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
    fn a_headset_is_asked_for_the_rate_nearest_the_one_wanted() {
        let offered = [72.0, 80.0, 90.0, 120.0];
        assert_eq!(headset_rate(90, &offered), Some(90.0));
        assert_eq!(headset_rate(100, &offered), Some(90.0));
        assert_eq!(headset_rate(60, &offered), Some(72.0));
        assert_eq!(headset_rate(144, &offered), Some(120.0));
        // As near either way: the lower.
        assert_eq!(headset_rate(76, &offered), Some(72.0));
        // A rate as the runtime reports it, not quite a whole number.
        assert_eq!(headset_rate(90, &[72.0, 89.998, 120.0]), Some(89.998));
        assert_eq!(headset_rate(90, &[]), None);
    }

    #[test]
    fn in_a_headset_there_is_no_wait_only_a_rate_to_ask_for() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Kept {
                max_fps: Some(30),
                vr_refresh_rate: Some(90),
            })
            .add_plugins(FramePacePlugin {
                max_fps: None,
                vr_refresh_rate: None,
                in_headset: true,
            });
        assert!(app.world().get_resource::<FrameTime>().is_none());
        assert_eq!(
            *app.world().resource::<HeadsetRate>(),
            HeadsetRate(Some(90))
        );

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(FramePacePlugin {
                max_fps: None,
                vr_refresh_rate: Some(0),
                in_headset: true,
            });
        assert_eq!(*app.world().resource::<HeadsetRate>(), HeadsetRate(None));
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
            .insert_resource(Kept {
                max_fps: Some(50),
                vr_refresh_rate: Some(90),
            })
            .add_plugins(FramePacePlugin {
                max_fps: None,
                vr_refresh_rate: None,
                in_headset: false,
            });
        assert!(app.world().get_resource::<HeadsetRate>().is_none());
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
