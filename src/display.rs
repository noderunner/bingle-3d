//! How the window stands on the screen: fullscreen (the default) or in a window, and at which
//! resolution. The escape menu's *Settings* change it (`escape_menu.rs`), and `config.toml` keeps
//! it (`fullscreen`, `resolution`).
//!
//! Fullscreen at the desktop's resolution (none chosen) is borderless: the window covers its
//! monitor at whatever resolution the monitor is in, so nothing is switched and nothing has to be
//! detected beforehand. Fullscreen at a resolution chosen is exclusive, switching the monitor to
//! a mode it offers of that size (the highest refresh rate of it); where the monitor offers none,
//! or the platform switches nothing (Wayland), the window is borderless at the desktop's
//! resolution instead. In a window, a resolution chosen is the window's size; none leaves it as
//! the player sizes it.
//!
//! A run limited by `--frames` or `--screenshot` is in a window unless `--fullscreen` says,
//! so a script's picture is the same size wherever it runs. A headset leaves the window alone.

use bevy::prelude::*;
use bevy::window::{
    Monitor, MonitorSelection, OnMonitor, PrimaryMonitor, PrimaryWindow, VideoMode,
    VideoModeSelection, WindowMode, WindowPosition,
};

/// The size of a window when nothing says otherwise, and in a run limited by a script.
pub const WINDOWED: (u32, u32) = (1280, 720);

/// The resolutions offered, in physical pixels: the usual ones of 16:9, 16:10 and 21:9 screens.
pub const COMMON: [(u32, u32); 10] = [
    (1280, 720),
    (1280, 800),
    (1366, 768),
    (1600, 900),
    (1680, 1050),
    (1920, 1080),
    (1920, 1200),
    (2560, 1440),
    (3440, 1440),
    (3840, 2160),
];

/// What `config.toml` keeps, put here by the network's plugin, which reads it.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Kept {
    pub fullscreen: Option<bool>,
    pub resolution: Option<(u32, u32)>,
}

/// How the window is to stand: what the settings show and change.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplaySettings {
    pub fullscreen: bool,
    /// None: the desktop's, in fullscreen; the player's sizing, in a window.
    pub resolution: Option<(u32, u32)>,
}

impl DisplaySettings {
    /// The run's: the command line's, else the config's, else fullscreen at the desktop's
    /// resolution; in a window, in a run a script limits.
    pub fn chosen(cli: Option<bool>, limited: bool, kept: Kept) -> Self {
        DisplaySettings {
            fullscreen: cli.unwrap_or(!limited && kept.fullscreen.unwrap_or(true)),
            resolution: kept.resolution,
        }
    }
}

/// `1920x1080` as a size, for `config.toml`.
pub fn parse(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.trim().split_once(['x', 'X', '×'])?;
    let size = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// A size as `config.toml` keeps it.
pub fn format(size: (u32, u32)) -> String {
    format!("{}x{}", size.0, size.1)
}

/// A size as the settings show it.
pub fn label(size: (u32, u32)) -> String {
    format!("{} × {}", size.0, size.1)
}

/// The resolutions to offer on a monitor this size (none known: all of them), each with whether
/// it can be chosen: one larger than the monitor is left out; in fullscreen, one the monitor has
/// no mode for is greyed, where it lists its modes at all.
pub fn offered(
    monitor: Option<(u32, u32)>,
    modes: &[VideoMode],
    fullscreen: bool,
) -> Vec<((u32, u32), bool)> {
    COMMON
        .iter()
        .copied()
        .filter(|&(w, h)| monitor.is_none_or(|(mw, mh)| w <= mw && h <= mh))
        .map(|size| {
            let usable = !fullscreen || modes.is_empty() || mode_for(modes, size).is_some();
            (size, usable)
        })
        .collect()
}

/// The monitor's mode of this size with the highest refresh rate (and the deepest color).
pub fn mode_for(modes: &[VideoMode], size: (u32, u32)) -> Option<VideoMode> {
    modes
        .iter()
        .filter(|m| m.physical_size == UVec2::new(size.0, size.1))
        .max_by_key(|m| (m.refresh_rate_millihertz, m.bit_depth))
        .copied()
}

/// The window's mode for these settings, on a monitor offering `modes`.
pub fn window_mode(
    display: DisplaySettings,
    modes: &[VideoMode],
    monitor: MonitorSelection,
) -> WindowMode {
    if !display.fullscreen {
        return WindowMode::Windowed;
    }
    match display.resolution.and_then(|size| mode_for(modes, size)) {
        Some(mode) => WindowMode::Fullscreen(monitor, VideoModeSelection::Specific(mode)),
        None => WindowMode::BorderlessFullscreen(monitor),
    }
}

pub struct DisplayPlugin {
    /// `--fullscreen` or `--windowed`, which come before the config's.
    pub fullscreen: Option<bool>,
    /// A run a script limits (`--frames`, `--screenshot`).
    pub limited: bool,
    /// Drawing in a headset: the window is left as it is.
    pub in_headset: bool,
}

impl Plugin for DisplayPlugin {
    fn build(&self, app: &mut App) {
        let kept = app
            .world()
            .get_resource::<Kept>()
            .copied()
            .unwrap_or_default();
        let display = DisplaySettings::chosen(self.fullscreen, self.limited, kept);
        app.insert_resource(display);
        if self.in_headset {
            return;
        }
        // The window is made after the plugins are built: it starts as it is to stand. No
        // monitor is known yet, so fullscreen starts on the primary one, at its resolution; a
        // mode of its own follows once the monitor is known (`apply`).
        let world = app.world_mut();
        let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        if let Ok(mut window) = windows.single_mut(world) {
            window.mode = window_mode(display, &[], MonitorSelection::Primary);
            let size = display
                .resolution
                .filter(|_| !self.limited)
                .unwrap_or(WINDOWED);
            window.resolution.set_physical_resolution(size.0, size.1);
        }
        app.add_systems(Update, apply);
    }
}

/// The monitor the window is on, else the primary one.
fn monitor_of<'a>(
    on: Option<&OnMonitor>,
    monitors: &'a Query<(&Monitor, Has<PrimaryMonitor>)>,
) -> Option<&'a Monitor> {
    on.and_then(|on| monitors.get(on.0).ok().map(|(m, _)| m))
        .or_else(|| {
            monitors
                .iter()
                .find(|(_, primary)| *primary)
                .map(|(m, _)| m)
        })
}

/// The monitor the primary window is on, for the settings: its size and its modes.
pub fn current_monitor(
    window: &Query<Option<&OnMonitor>, With<PrimaryWindow>>,
    monitors: &Query<(&Monitor, Has<PrimaryMonitor>)>,
) -> Option<((u32, u32), Vec<VideoMode>)> {
    let on = window.single().ok().flatten();
    monitor_of(on, monitors).map(|m| ((m.physical_width, m.physical_height), m.video_modes.clone()))
}

/// Puts the window as the settings say when they change, and once the monitor it is on is
/// known (a resolution's mode is the monitor's).
fn apply(
    display: Res<DisplaySettings>,
    mut windows: Query<(&mut Window, Option<Ref<OnMonitor>>), With<PrimaryWindow>>,
    monitors: Query<(&Monitor, Has<PrimaryMonitor>)>,
    mut applied: Local<Option<DisplaySettings>>,
) {
    let Ok((mut window, on)) = windows.single_mut() else {
        return;
    };
    let monitor_known = on.as_ref().is_some_and(|on| on.is_added());
    if *applied == Some(*display) && !monitor_known {
        return;
    }
    let first = applied.is_none();
    *applied = Some(*display);
    // The start is as the plugin made the window; only a resolution's mode is left to it.
    if first && !(display.fullscreen && display.resolution.is_some()) {
        return;
    }
    let Some(monitor) = monitor_of(on.as_deref(), &monitors) else {
        if first {
            *applied = None;
        }
        return;
    };
    let mode = window_mode(*display, &monitor.video_modes, MonitorSelection::Current);
    if window.mode != mode {
        window.mode = mode;
    }
    if !display.fullscreen
        && let Some((w, h)) = display.resolution
        && (window.physical_width(), window.physical_height()) != (w, h)
    {
        window.resolution.set_physical_resolution(w, h);
        window.position = WindowPosition::Centered(MonitorSelection::Current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(w: u32, h: u32, hz: u32) -> VideoMode {
        VideoMode {
            physical_size: UVec2::new(w, h),
            bit_depth: 24,
            refresh_rate_millihertz: hz * 1000,
        }
    }

    #[test]
    fn fullscreen_unless_told_or_a_script_limits_the_run() {
        let none = Kept::default();
        assert!(
            DisplaySettings::chosen(None, false, none).fullscreen,
            "the default"
        );
        assert!(
            !DisplaySettings::chosen(None, true, none).fullscreen,
            "--frames"
        );
        assert!(DisplaySettings::chosen(Some(true), true, none).fullscreen);
        let windowed = Kept {
            fullscreen: Some(false),
            resolution: Some((1600, 900)),
        };
        let d = DisplaySettings::chosen(None, false, windowed);
        assert_eq!((d.fullscreen, d.resolution), (false, Some((1600, 900))));
        assert!(DisplaySettings::chosen(Some(true), false, windowed).fullscreen);
    }

    #[test]
    fn a_size_reads_and_writes() {
        assert_eq!(parse("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse(" 2560 × 1440 "), Some((2560, 1440)));
        assert_eq!(parse("0x1080"), None);
        assert_eq!(parse("big"), None);
        assert_eq!(parse(&format((1366, 768))), Some((1366, 768)));
        assert_eq!(label((1920, 1080)), "1920 × 1080");
    }

    #[test]
    fn no_resolution_larger_than_the_monitor_and_greyed_without_a_mode() {
        let sizes = |o: Vec<((u32, u32), bool)>| o.into_iter().map(|(s, _)| s).collect::<Vec<_>>();
        let on_1080 = sizes(offered(Some((1920, 1080)), &[], false));
        assert_eq!(on_1080.last(), Some(&(1920, 1080)));
        assert!(!on_1080.contains(&(1920, 1200)));
        assert_eq!(sizes(offered(None, &[], false)).len(), COMMON.len());

        let modes = [mode(1920, 1080, 60), mode(1280, 720, 60)];
        let full = offered(Some((1920, 1080)), &modes, true);
        assert!(full.contains(&((1280, 720), true)));
        assert!(full.contains(&((1600, 900), false)), "no mode of it");
        let windowed = offered(Some((1920, 1080)), &modes, false);
        assert!(
            windowed.iter().all(|(_, usable)| *usable),
            "any size in a window"
        );
    }

    #[test]
    fn the_window_mode() {
        let modes = [
            mode(1920, 1080, 60),
            mode(1920, 1080, 144),
            mode(1280, 720, 60),
        ];
        let cur = MonitorSelection::Current;
        let full = |resolution| DisplaySettings {
            fullscreen: true,
            resolution,
        };
        assert_eq!(
            window_mode(full(None), &modes, cur),
            WindowMode::BorderlessFullscreen(cur),
            "the desktop's resolution: nothing switched"
        );
        assert_eq!(
            window_mode(full(Some((1920, 1080))), &modes, cur),
            WindowMode::Fullscreen(cur, VideoModeSelection::Specific(mode(1920, 1080, 144))),
            "the fastest mode of the size"
        );
        assert_eq!(
            window_mode(full(Some((1600, 900))), &modes, cur),
            WindowMode::BorderlessFullscreen(cur),
            "no mode of it: the desktop's"
        );
        let windowed = DisplaySettings {
            fullscreen: false,
            resolution: Some((1280, 720)),
        };
        assert_eq!(window_mode(windowed, &modes, cur), WindowMode::Windowed);
    }

    fn app(display: DisplaySettings) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(display)
            .add_systems(Update, apply);
        app.world_mut().spawn((
            Window {
                resolution: bevy::window::WindowResolution::new(1280, 720),
                ..default()
            },
            PrimaryWindow,
        ));
        app
    }

    fn window(app: &mut App) -> Window {
        app.world_mut()
            .query_filtered::<&Window, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap()
            .clone()
    }

    #[test]
    fn a_change_of_settings_moves_the_window() {
        let mut app = app(DisplaySettings {
            fullscreen: false,
            resolution: None,
        });
        app.world_mut().spawn((
            Monitor {
                name: None,
                physical_height: 1080,
                physical_width: 1920,
                physical_position: IVec2::ZERO,
                refresh_rate_millihertz: Some(60_000),
                scale_factor: 1.0,
                video_modes: vec![mode(1920, 1080, 60)],
            },
            PrimaryMonitor,
        ));
        app.update();
        assert_eq!(window(&mut app).mode, WindowMode::Windowed);

        app.world_mut().resource_mut::<DisplaySettings>().fullscreen = true;
        app.update();
        assert_eq!(
            window(&mut app).mode,
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        );

        *app.world_mut().resource_mut::<DisplaySettings>() = DisplaySettings {
            fullscreen: false,
            resolution: Some((1600, 900)),
        };
        app.update();
        let w = window(&mut app);
        assert_eq!(w.mode, WindowMode::Windowed);
        assert_eq!((w.physical_width(), w.physical_height()), (1600, 900));
    }
}
