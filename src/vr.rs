//! The table in a headset, through OpenXR (SteamVR): `--vr`, in a build with the `vr` feature.
//!
//! A first step (docs/plan.md, VR): the environment chosen, at its real size, drawn for each
//! eye, with the player sat at the table. The window stays as it is, with the interface and
//! the flat camera; nothing in the headset can be pointed at or picked up yet.
//!
//! **Where the player sits.** The headset's poses are in the room's own space, under the
//! tracking root. The root is placed so that the head, wherever it is in the room, is at the
//! eye point of someone sat at the table and turned to it (`seat_root`): once when the headset
//! is first tracked, and again whenever Home is pressed. So a chair, a sofa or the floor all
//! do, at the cost of the room's floor not being the environment's.

#[cfg(any(feature = "vr", test))]
use bevy::prelude::*;

/// How far above the table top the eyes of someone sat at it are, in metres: kneeling on a
/// cushion at a low table, or on a chair at a higher one.
#[cfg(any(feature = "vr", test))]
const EYE_HEIGHT: f32 = 0.45;
/// How far from the table's middle someone sits where the environment has no cushions, and
/// how far ahead of a cushion's middle the head of whoever kneels on it is.
#[cfg(any(feature = "vr", test))]
const SEAT_DISTANCE: f32 = 0.85;
#[cfg(any(feature = "vr", test))]
const HEAD_AHEAD: f32 = 0.1;

/// Where the eyes of the player sat at the table are: on the first seat's side (+z), as the
/// flat camera starts.
#[cfg(any(feature = "vr", test))]
fn eye_point(seat_distance: Option<f32>) -> Vec3 {
    let distance = seat_distance.unwrap_or(SEAT_DISTANCE) - HEAD_AHEAD;
    Vec3::new(0.0, EYE_HEIGHT, distance)
}

/// The angle round the vertical axis that turns "ahead" (−z) to this direction, if it has any
/// length along the ground.
#[cfg(any(feature = "vr", test))]
fn heading(direction: Vec3) -> Option<f32> {
    let flat = Vec2::new(direction.x, direction.z);
    (flat.length() > 1e-3).then(|| (-flat.x).atan2(-flat.y))
}

/// Where the tracking root goes so that a head at `head`, in the room's space, is at `eye` and
/// faces the table's middle. Only turned about the vertical: the room's "up" stays up.
#[cfg(any(feature = "vr", test))]
fn seat_root(head: Transform, eye: Vec3) -> Transform {
    let wanted = heading(-eye).unwrap_or(0.0);
    // Looking straight down or up, the head has no heading: leave the room as it is turned.
    let now = heading(head.rotation * Vec3::NEG_Z).unwrap_or(0.0);
    let rotation = Quat::from_rotation_y(wanted - now);
    Transform {
        translation: eye - rotation * head.translation,
        rotation,
        scale: Vec3::ONE,
    }
}

#[cfg(feature = "vr")]
pub use headset::{VrPlugin, with_headset};

#[cfg(feature = "vr")]
mod headset {
    use super::*;
    use crate::environment::{Chosen, Environments};
    use crate::scene::Headset;
    use bevy::app::PluginGroupBuilder;
    use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
    use bevy::window::{PresentMode, PrimaryWindow};
    use bevy::winit::{UpdateMode, WinitSettings};
    use bevy_mod_openxr::add_xr_plugins;
    use bevy_mod_openxr::init::OxrInitPlugin;
    use bevy_mod_openxr::types::{AppInfo, Version};
    use bevy_mod_xr::camera::XrCamera;
    use bevy_mod_xr::session::{XrState, XrTrackingRoot};

    /// Bevy's plugins with the headset's in them: the renderer is started on the device the
    /// OpenXR runtime asks for. With no runtime (SteamVR not running) the plugin says so in
    /// the log and the client runs in its window alone.
    pub fn with_headset(plugins: PluginGroupBuilder) -> PluginGroupBuilder {
        // Rendering in step with the simulation: the eyes' poses are a frame fresher.
        add_xr_plugins(plugins.disable::<PipelinedRenderingPlugin>()).set(OxrInitPlugin {
            app_info: AppInfo {
                name: "Bingle".into(),
                version: Version(0, 1, 0),
            },
            ..default()
        })
    }

    pub struct VrPlugin;

    impl Plugin for VrPlugin {
        fn build(&self, app: &mut App) {
            // The headset sets the pace: a window waiting for the monitor, or resting while
            // another has the keyboard, would hold every frame back.
            app.insert_resource(WinitSettings {
                focused_mode: UpdateMode::Continuous,
                unfocused_mode: UpdateMode::Continuous,
            })
            .add_observer(mark_eye)
            .add_systems(Startup, unpace_window)
            .add_systems(Update, (report_state, take_seat));
        }
    }

    fn unpace_window(mut windows: Query<&mut Window, With<PrimaryWindow>>) {
        for mut window in &mut windows {
            window.present_mode = PresentMode::AutoNoVsync;
        }
    }

    /// The plugin's cameras are eyes: the scene lights them as such.
    fn mark_eye(eye: On<Add<XrCamera>>, mut commands: Commands) {
        commands.entity(eye.entity).insert(Headset);
    }

    fn report_state(state: Option<Res<XrState>>, mut last: Local<Option<XrState>>) {
        let now = state.map(|s| *s);
        if now != *last {
            match now {
                Some(XrState::Unavailable) | None => {
                    warn!("no headset: OpenXR is not available (is SteamVR running?)")
                }
                Some(state) => info!("headset: {state:?}"),
            }
            *last = now;
        }
    }

    /// Sits the player at the table: when the headset is first tracked, when another
    /// environment is chosen, and when Home is pressed.
    fn take_seat(
        keys: Res<ButtonInput<KeyCode>>,
        chosen: Res<Chosen>,
        all: Res<Environments>,
        eyes: Query<&Transform, With<XrCamera>>,
        mut roots: Query<&mut Transform, (With<XrTrackingRoot>, Without<XrCamera>)>,
        mut seated: Local<bool>,
    ) {
        // Until the runtime has located them, the eyes are both at the room's origin.
        let tracked = eyes.iter().any(|eye| eye.translation != Vec3::ZERO);
        if !tracked {
            *seated = false;
            return;
        }
        if *seated && !chosen.is_changed() && !keys.just_pressed(KeyCode::Home) {
            return;
        }
        let Some(first) = eyes.iter().next() else {
            return;
        };
        let count = eyes.iter().count() as f32;
        let middle = eyes.iter().map(|eye| eye.translation).sum::<Vec3>() / count;
        let head = Transform::from_translation(middle).with_rotation(first.rotation);
        let seat_distance = all
            .chosen(&chosen)
            .and_then(|env| env.seating.as_ref())
            .map(|seating| seating.distance);
        for mut root in &mut roots {
            *root = seat_root(head, eye_point(seat_distance));
        }
        *seated = true;
        info!("headset: sat at the table (Home sits you again)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn the_eyes_are_over_the_cushion_and_a_little_ahead_of_it() {
        assert_eq!(eye_point(Some(0.86)), Vec3::new(0.0, EYE_HEIGHT, 0.76));
        assert_eq!(eye_point(None).z, SEAT_DISTANCE - HEAD_AHEAD);
    }

    #[test]
    fn a_head_anywhere_in_the_room_is_put_at_the_eye_point_facing_the_table() {
        let eye = eye_point(None);
        // Somewhere off the room's middle, at a chair's height, turned to the room's +x and
        // looking a little down.
        let head = Transform::from_xyz(1.3, 1.2, -0.4)
            .with_rotation(Quat::from_rotation_y(-FRAC_PI_2) * Quat::from_rotation_x(-0.3));
        let root = seat_root(head, eye);
        assert!(close(root.transform_point(head.translation), eye));
        // What the head looks along, in the world: towards the table (−z), still a little down.
        let ahead = root.rotation * head.rotation * Vec3::NEG_Z;
        assert!(ahead.x.abs() < 1e-4 && ahead.z < 0.0 && ahead.y < 0.0);
        // The room stays upright.
        assert!(close(root.rotation * Vec3::Y, Vec3::Y));
        // A step to the head's right in the room is a step to the right at the table.
        let step = root.transform_point(head.translation + head.rotation * Vec3::X);
        assert!(step.x > eye.x + 0.9);
    }

    #[test]
    fn a_head_looking_straight_down_leaves_the_room_as_it_is_turned() {
        let eye = eye_point(None);
        let head =
            Transform::from_xyz(0.2, 1.0, 0.3).with_rotation(Quat::from_rotation_x(-FRAC_PI_2));
        let root = seat_root(head, eye);
        assert!(close(root.transform_point(head.translation), eye));
        assert!(root.rotation.angle_between(Quat::IDENTITY) < 1e-4);
    }
}
