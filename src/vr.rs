//! The table in a headset, through OpenXR (SteamVR): `--vr`, in a build with the `vr` feature.
//!
//! First steps (docs/plan.md, VR): the environment chosen, at its real size, drawn for each
//! eye, with the player sat at the table, and a controller in each hand that points and
//! clicks, and a menu that opens in front of them (`vr_menu.rs`). The window stays as it is,
//! with the interface and the flat camera.
//!
//! **Where the player sits.** The headset's poses are in the room's own space, under the
//! tracking root. The root is placed so that the head, wherever it is in the room, is at the
//! eye point of someone sat at the table and turned to it (`seat_root`): once when the headset
//! is first tracked, and again whenever Home is pressed. So a chair, a sofa or the floor all
//! do, at the cost of the room's floor not being the environment's.
//!
//! **Controllers.** Each is a `RayPointer` (`ray_pointer.rs`) along its aim: the trigger is a
//! click on what it points at. One button plays the move being built and another goes back,
//! as Enter and Backspace do. What the client asks OpenXR for is a few actions (`Input`); which
//! button of which controller each is, is suggested per kind of controller (`BINDINGS`) and
//! is the runtime's to decide: SteamVR maps a controller it knows onto one of these, and lets
//! the player change it.

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

/// How fast a stick pushed all the way moves the player, in metres a second along the ground
/// and up or down, and turns them, in radians a second; and how far a stick must be pushed
/// (of 1) to count, since one at rest is seldom quite at its middle.
#[cfg(any(feature = "vr", test))]
const WALK_SPEED: f32 = 0.5;
#[cfg(any(feature = "vr", test))]
const LIFT_SPEED: f32 = 0.35;
#[cfg(any(feature = "vr", test))]
const TURN_SPEED: f32 = 1.0;
#[cfg(any(feature = "vr", test))]
const DEAD_ZONE: f32 = 0.2;

/// A stick's push along one axis, with nothing near its middle and all of it at its edge.
#[cfg(any(feature = "vr", test))]
fn push(value: f32) -> f32 {
    let size = value.abs();
    if size <= DEAD_ZONE {
        0.0
    } else {
        value.signum() * ((size - DEAD_ZONE) / (1.0 - DEAD_ZONE)).min(1.0)
    }
}

/// What the sticks ask for: `slide` to the player's right, `lift` up, `walk` ahead and
/// `turn` to the right, each from −1 to 1.
#[cfg(any(feature = "vr", test))]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Sticks {
    slide: f32,
    lift: f32,
    walk: f32,
    turn: f32,
}

#[cfg(any(feature = "vr", test))]
impl Sticks {
    /// The left stick walks the player ahead or back and slides them sideways; the right one
    /// turns them and lifts or lowers them.
    fn new(left: Vec2, right: Vec2) -> Self {
        Self {
            slide: push(left.x),
            walk: push(left.y),
            lift: push(right.y),
            turn: push(right.x),
        }
    }

    fn at_rest(&self) -> bool {
        *self == Self::default()
    }
}

/// The tracking root after the sticks have moved the player for `seconds`. `head` is the
/// head in the room's space, as for `seat_root`. Sideways and ahead are the head's own, along
/// the ground; a turn is about the head, so that what is under the player stays under them.
#[cfg(any(feature = "vr", test))]
fn moved_root(root: Transform, head: Transform, sticks: Sticks, seconds: f32) -> Transform {
    let head_at = root.transform_point(head.translation);
    let facing = heading(root.rotation * head.rotation * Vec3::NEG_Z).unwrap_or(0.0);
    let along = Quat::from_rotation_y(facing);
    let step = along * Vec3::new(sticks.slide, 0.0, -sticks.walk) * WALK_SPEED
        + Vec3::Y * sticks.lift * LIFT_SPEED;
    // To the right is clockwise from above: a negative angle about +y.
    let turn = Quat::from_rotation_y(-sticks.turn * TURN_SPEED * seconds);
    Transform {
        translation: head_at + turn * (root.translation - head_at) + step * seconds,
        rotation: turn * root.rotation,
        scale: root.scale,
    }
}

/// What the client asks of a controller.
#[cfg(any(feature = "vr", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Input {
    /// Where it points.
    Aim,
    /// The trigger: a click on what it points at.
    Select,
    /// Play the move being built, as Enter does.
    Confirm,
    /// Go back a selection, as Backspace does.
    Back,
    /// A stick that walks the player ahead or back and slides them sideways.
    Shift,
    /// A stick that turns the player and lifts or lowers them.
    Turn,
    /// Sit at the table again, as Home does.
    Sit,
    /// Open the menu, and close it.
    Menu,
}

/// What is suggested for each kind of controller OpenXR's core knows that has a trigger: the
/// lower face button confirms and the upper goes back; the left hand's menu button, or pressing
/// its stick, opens the menu. Where there are sticks, the left one walks the player and slides
/// them sideways, the right one turns them and lifts or lowers them, and pressing the right one
/// sits them at the table again. A path that the kind of
/// controller does not have would have the runtime refuse all that is suggested for it.
#[cfg(any(feature = "vr", test))]
const BINDINGS: &[(&str, &[(Input, &str)])] = &[
    (
        "/interaction_profiles/oculus/touch_controller",
        &[
            (Input::Aim, "/user/hand/left/input/aim/pose"),
            (Input::Aim, "/user/hand/right/input/aim/pose"),
            (Input::Select, "/user/hand/left/input/trigger/value"),
            (Input::Select, "/user/hand/right/input/trigger/value"),
            (Input::Confirm, "/user/hand/left/input/x/click"),
            (Input::Confirm, "/user/hand/right/input/a/click"),
            (Input::Back, "/user/hand/left/input/y/click"),
            (Input::Back, "/user/hand/right/input/b/click"),
            (Input::Shift, "/user/hand/left/input/thumbstick"),
            (Input::Turn, "/user/hand/right/input/thumbstick"),
            (Input::Sit, "/user/hand/right/input/thumbstick/click"),
            (Input::Menu, "/user/hand/left/input/menu/click"),
            (Input::Menu, "/user/hand/left/input/thumbstick/click"),
        ],
    ),
    (
        "/interaction_profiles/valve/index_controller",
        &[
            (Input::Aim, "/user/hand/left/input/aim/pose"),
            (Input::Aim, "/user/hand/right/input/aim/pose"),
            (Input::Select, "/user/hand/left/input/trigger/value"),
            (Input::Select, "/user/hand/right/input/trigger/value"),
            (Input::Confirm, "/user/hand/left/input/a/click"),
            (Input::Confirm, "/user/hand/right/input/a/click"),
            (Input::Back, "/user/hand/left/input/b/click"),
            (Input::Back, "/user/hand/right/input/b/click"),
            (Input::Shift, "/user/hand/left/input/thumbstick"),
            (Input::Turn, "/user/hand/right/input/thumbstick"),
            (Input::Sit, "/user/hand/right/input/thumbstick/click"),
            (Input::Menu, "/user/hand/left/input/thumbstick/click"),
        ],
    ),
    (
        "/interaction_profiles/htc/vive_controller",
        &[
            (Input::Aim, "/user/hand/left/input/aim/pose"),
            (Input::Aim, "/user/hand/right/input/aim/pose"),
            (Input::Select, "/user/hand/left/input/trigger/value"),
            (Input::Select, "/user/hand/right/input/trigger/value"),
            (Input::Confirm, "/user/hand/left/input/trackpad/click"),
            (Input::Confirm, "/user/hand/right/input/trackpad/click"),
            (Input::Back, "/user/hand/right/input/menu/click"),
            (Input::Menu, "/user/hand/left/input/menu/click"),
        ],
    ),
    (
        "/interaction_profiles/khr/simple_controller",
        &[
            (Input::Aim, "/user/hand/left/input/aim/pose"),
            (Input::Aim, "/user/hand/right/input/aim/pose"),
            (Input::Select, "/user/hand/left/input/select/click"),
            (Input::Select, "/user/hand/right/input/select/click"),
            (Input::Back, "/user/hand/right/input/menu/click"),
            (Input::Menu, "/user/hand/left/input/menu/click"),
        ],
    ),
];

/// The two hands, as OpenXR names them.
#[cfg(any(feature = "vr", test))]
const HANDS: [&str; 2] = ["/user/hand/left", "/user/hand/right"];

#[cfg(feature = "vr")]
pub use headset::{VrPlugin, with_headset};

#[cfg(feature = "vr")]
mod headset {
    use super::*;
    use crate::environment::{Chosen, Environments};
    use crate::escape_menu::{self, EscapeMenu};
    use crate::net::{Net, WatchStatus};
    use crate::play;
    use crate::ray_pointer::{self, RayPointer, RayPointerPlugin};
    use crate::scene::Headset;
    use crate::vr_menu::{MenuPanel, SitAgain, VrMenu, VrMenuPlugin, place_in_front};
    use bevy::app::PluginGroupBuilder;
    use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
    use bevy::window::{PresentMode, PrimaryWindow};
    use bevy::winit::{UpdateMode, WinitSettings};
    use bevy_mod_openxr::action_binding::{
        OxrActionBindingPlugin, OxrSendActionBindings, OxrSuggestActionBinding,
    };
    use bevy_mod_openxr::action_set_attaching::OxrAttachActionSet;
    use bevy_mod_openxr::action_set_syncing::{OxrActionSetSyncSet, OxrSyncActionSet};
    use bevy_mod_openxr::init::OxrInitPlugin;
    use bevy_mod_openxr::resources::OxrInstance;
    use bevy_mod_openxr::session::OxrSession;
    use bevy_mod_openxr::types::{AppInfo, Version};
    use bevy_mod_openxr::{add_xr_plugins, openxr_session_running};
    use bevy_mod_xr::camera::XrCamera;
    use bevy_mod_xr::session::{
        XrPreDestroySession, XrSessionCreated, XrState, XrTrackingRoot, session_available,
    };
    use bevy_mod_xr::spaces::XrSpaceLocationFlags;

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
            // The plugin makes the schedule its bindings are suggested in anew when it is
            // added: systems put there before it would be dropped without a word, and the
            // controllers would do nothing.
            assert!(
                app.is_plugin_added::<OxrActionBindingPlugin>(),
                "VrPlugin goes after the plugins `with_headset` gives"
            );
            // The headset sets the pace: a window waiting for the monitor, or resting while
            // another has the keyboard, would hold every frame back.
            app.insert_resource(WinitSettings {
                focused_mode: UpdateMode::Continuous,
                unfocused_mode: UpdateMode::Continuous,
            })
            .add_plugins((RayPointerPlugin, VrMenuPlugin))
            .add_observer(mark_eye)
            .add_systems(
                Startup,
                (unpace_window, make_actions.run_if(session_available)),
            )
            .add_systems(OxrSendActionBindings, suggest_bindings)
            .add_systems(XrSessionCreated, give_hands)
            .add_systems(XrPreDestroySession, take_hands)
            .add_systems(
                PreUpdate,
                (
                    ask_for_actions.before(OxrActionSetSyncSet),
                    (read_hands, move_about, open_menu).after(OxrActionSetSyncSet),
                )
                    .run_if(openxr_session_running),
            )
            .add_systems(Update, (report_state, take_seat));
        }
    }

    /// The controllers' actions, made once with the OpenXR instance.
    #[derive(Resource)]
    struct Actions {
        set: openxr::ActionSet,
        aim: openxr::Action<openxr::Posef>,
        select: openxr::Action<f32>,
        confirm: openxr::Action<bool>,
        back: openxr::Action<bool>,
        shift: openxr::Action<openxr::Vector2f>,
        turn: openxr::Action<openxr::Vector2f>,
        sit: openxr::Action<bool>,
        menu: openxr::Action<bool>,
        hands: [openxr::Path; 2],
    }

    impl Actions {
        fn make(instance: &openxr::Instance) -> openxr::Result<Self> {
            let hands = [
                instance.string_to_path(HANDS[0])?,
                instance.string_to_path(HANDS[1])?,
            ];
            let set = instance.create_action_set("table", "At the table", 0)?;
            Ok(Self {
                aim: set.create_action("aim", "Point", &hands)?,
                select: set.create_action("select", "Click what is pointed at", &hands)?,
                confirm: set.create_action("confirm", "Play the move", &[])?,
                back: set.create_action("back", "Go back", &[])?,
                shift: set.create_action("shift", "Walk ahead and back, and sideways", &[])?,
                turn: set.create_action("turn", "Turn, and up and down", &[])?,
                menu: set.create_action("menu", "Open the menu", &[])?,
                sit: set.create_action("sit", "Sit at the table again", &[])?,
                set,
                hands,
            })
        }

        fn raw(&self, input: Input) -> openxr::sys::Action {
            match input {
                Input::Aim => self.aim.as_raw(),
                Input::Select => self.select.as_raw(),
                Input::Confirm => self.confirm.as_raw(),
                Input::Back => self.back.as_raw(),
                Input::Shift => self.shift.as_raw(),
                Input::Turn => self.turn.as_raw(),
                Input::Sit => self.sit.as_raw(),
                Input::Menu => self.menu.as_raw(),
            }
        }
    }

    fn make_actions(instance: Res<OxrInstance>, mut commands: Commands) {
        match Actions::make(&instance) {
            Ok(actions) => commands.insert_resource(actions),
            Err(e) => warn!("headset: no controllers, their actions could not be made: {e}"),
        }
    }

    fn suggest_bindings(
        actions: Option<Res<Actions>>,
        mut suggest: MessageWriter<OxrSuggestActionBinding>,
    ) {
        let Some(actions) = actions else {
            return;
        };
        info!(
            "headset: suggesting what the controllers' buttons do, for {} kinds of controller",
            BINDINGS.len()
        );
        for (profile, bindings) in BINDINGS {
            for (input, path) in *bindings {
                suggest.write(OxrSuggestActionBinding {
                    action: actions.raw(*input),
                    interaction_profile: (*profile).into(),
                    bindings: vec![(*path).into()],
                });
            }
        }
    }

    /// A controller: 0 is the left hand's.
    #[derive(Component)]
    struct Hand(usize);

    /// With a session: the actions are its own from here on, and each hand gets a pointer
    /// where its controller aims.
    fn give_hands(
        actions: Option<Res<Actions>>,
        session: Res<OxrSession>,
        mut attach: MessageWriter<OxrAttachActionSet>,
        mut commands: Commands,
        mut meshes: ResMut<Assets<Mesh>>,
        mut materials: ResMut<Assets<StandardMaterial>>,
    ) {
        let Some(actions) = actions else {
            return;
        };
        attach.write(OxrAttachActionSet(actions.set.clone()));
        for (number, hand) in actions.hands.iter().enumerate() {
            match session.create_action_space(&actions.aim, *hand, Isometry3d::IDENTITY) {
                Ok(space) => {
                    commands
                        .spawn((space, Hand(number), RayPointer::bundle(number as u8)))
                        .with_child(ray_pointer::beam(&mut meshes, &mut materials));
                    info!("headset: a pointer for {}", HANDS[number]);
                }
                Err(e) => warn!("headset: no pointer for {}: {e}", HANDS[number]),
            }
        }
    }

    fn take_hands(hands: Query<Entity, With<Hand>>, mut commands: Commands) {
        for hand in &hands {
            commands.entity(hand).despawn();
        }
    }

    fn ask_for_actions(actions: Option<Res<Actions>>, mut sync: MessageWriter<OxrSyncActionSet>) {
        if let Some(actions) = actions {
            sync.write(OxrSyncActionSet(actions.set.clone()));
        }
    }

    /// What the controllers say this frame: where each is and how far its trigger is in, for
    /// its pointer; and the buttons that play the move and go back.
    fn read_hands(
        actions: Option<Res<Actions>>,
        session: Res<OxrSession>,
        mut hands: Query<(&Hand, &XrSpaceLocationFlags, &mut RayPointer)>,
        menu: Option<Res<EscapeMenu>>,
        net: Res<Net>,
        mut watch: ResMut<WatchStatus>,
    ) {
        let Some(actions) = actions else {
            return;
        };
        for (hand, located, mut pointer) in &mut hands {
            let tracked = located.position_tracked && located.rotation_tracked;
            if pointer.tracked != tracked {
                pointer.tracked = tracked;
                let what = if tracked { "tracked" } else { "lost" };
                info!("headset: the controller of {} is {what}", HANDS[hand.0]);
            }
            let trigger = actions
                .select
                .state(&session, actions.hands[hand.0])
                .map_or(0.0, |s| if s.is_active { s.current_state } else { 0.0 });
            pointer.squeeze(trigger);
        }
        let pressed = |action: &openxr::Action<bool>| {
            action
                .state(&session, openxr::Path::NULL)
                .is_ok_and(|s| s.is_active && s.current_state && s.changed_since_last_sync)
        };
        if !escape_menu::closed(menu.as_deref()) {
            return;
        }
        if pressed(&actions.confirm) {
            info!("headset: the button that plays the move");
            play::submit(&net, &mut watch);
        }
        if pressed(&actions.back) {
            info!("headset: the button that goes back");
            play::back(&mut watch);
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

    /// Where the head is in the room's space: between the eyes, turned as they are.
    fn head_in_room<'a>(eyes: impl Iterator<Item = &'a Transform>) -> Option<Transform> {
        let eyes: Vec<&Transform> = eyes.collect();
        let first = eyes.first()?;
        let middle = eyes.iter().map(|eye| eye.translation).sum::<Vec3>() / eyes.len() as f32;
        Some(Transform::from_translation(middle).with_rotation(first.rotation))
    }

    /// The sticks move the player about the place, by moving the room they are tracked in.
    fn move_about(
        actions: Option<Res<Actions>>,
        session: Res<OxrSession>,
        time: Res<Time>,
        eyes: Query<&Transform, With<XrCamera>>,
        mut roots: Query<&mut Transform, (With<XrTrackingRoot>, Without<XrCamera>)>,
        mut sit: ResMut<SitAgain>,
    ) {
        let Some(actions) = actions else {
            return;
        };
        if pressed(&actions.sit, &session) {
            sit.0 = true;
        }
        let stick = |action: &openxr::Action<openxr::Vector2f>| {
            action
                .state(&session, openxr::Path::NULL)
                .ok()
                .filter(|s| s.is_active)
                .map_or(Vec2::ZERO, |s| {
                    Vec2::new(s.current_state.x, s.current_state.y)
                })
        };
        let sticks = Sticks::new(stick(&actions.shift), stick(&actions.turn));
        if sticks.at_rest() {
            return;
        }
        let Some(head) = head_in_room(eyes.iter()) else {
            return;
        };
        for mut root in &mut roots {
            *root = moved_root(*root, head, sticks, time.delta_secs());
        }
    }

    /// Whether a button has just been pressed.
    fn pressed(action: &openxr::Action<bool>, session: &OxrSession) -> bool {
        action
            .state(session, openxr::Path::NULL)
            .is_ok_and(|s| s.is_active && s.current_state && s.changed_since_last_sync)
    }

    type Panels<'w, 's> = Query<
        'w,
        's,
        &'static mut Transform,
        (With<MenuPanel>, Without<XrCamera>, Without<XrTrackingRoot>),
    >;

    /// The menu button opens the menu in front of the player, wherever they are, and closes it.
    fn open_menu(
        actions: Option<Res<Actions>>,
        session: Res<OxrSession>,
        eyes: Query<&Transform, With<XrCamera>>,
        roots: Query<&Transform, (With<XrTrackingRoot>, Without<XrCamera>)>,
        mut menu: ResMut<VrMenu>,
        mut panels: Panels,
    ) {
        if !actions.is_some_and(|a| pressed(&a.menu, &session)) {
            return;
        }
        menu.open = !menu.open;
        info!("headset: the menu button");
        if menu.open
            && let (Some(head), Ok(root)) = (head_in_room(eyes.iter()), roots.single())
        {
            let head = root.mul_transform(head);
            for mut panel in &mut panels {
                *panel = place_in_front(head);
            }
        }
    }

    /// Sits the player at the table: when the headset is first tracked, when another
    /// environment is chosen, and when Home or the controller's button for it is pressed.
    fn take_seat(
        keys: Res<ButtonInput<KeyCode>>,
        mut again: ResMut<SitAgain>,
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
        let asked = std::mem::take(&mut again.0) || keys.just_pressed(KeyCode::Home);
        if *seated && !chosen.is_changed() && !asked {
            return;
        }
        let Some(head) = head_in_room(eyes.iter()) else {
            return;
        };
        let seat_distance = all
            .chosen(&chosen)
            .and_then(|env| env.seating.as_ref())
            .map(|seating| seating.distance);
        for mut root in &mut roots {
            *root = seat_root(head, eye_point(seat_distance));
        }
        *seated = true;
        info!("headset: sat at the table (Home, or pressing the right stick, sits you again)");
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
    fn a_stick_near_its_middle_asks_for_nothing() {
        assert_eq!(push(0.15), 0.0);
        assert_eq!(push(-0.2), 0.0);
        assert!((push(1.0) - 1.0).abs() < 1e-6);
        assert!((push(-0.6) + 0.5).abs() < 1e-6);
        assert!(Sticks::new(Vec2::splat(0.1), Vec2::splat(-0.1)).at_rest());
        // The left stick walks and slides; the right one turns, lifts and lowers.
        let sticks = Sticks::new(Vec2::new(1.0, -1.0), Vec2::new(0.0, 1.0));
        assert_eq!(
            (sticks.slide, sticks.walk, sticks.lift, sticks.turn),
            (1.0, -1.0, 1.0, 0.0)
        );
    }

    /// Sat at the table, with the head somewhere in the room.
    fn sat() -> (Transform, Transform) {
        let head = Transform::from_xyz(0.4, 1.1, 0.2).with_rotation(Quat::from_rotation_y(0.7));
        (seat_root(head, eye_point(None)), head)
    }

    #[test]
    fn the_player_slides_sideways_and_is_lifted() {
        let (root, head) = sat();
        let eye = eye_point(None);
        let slide = Sticks {
            slide: 1.0,
            ..default()
        };
        let after = moved_root(root, head, slide, 1.0);
        // Facing the table (−z), the player's right is +x.
        assert!(close(
            after.transform_point(head.translation),
            eye + Vec3::X * WALK_SPEED
        ));
        assert!(after.rotation.angle_between(root.rotation) < 1e-5);
        let lift = Sticks {
            lift: -1.0,
            ..default()
        };
        let after = moved_root(root, head, lift, 2.0);
        assert!(close(
            after.transform_point(head.translation),
            eye - Vec3::Y * LIFT_SPEED * 2.0
        ));
    }

    #[test]
    fn the_player_turns_where_they_are_and_walks_where_they_face() {
        let (root, head) = sat();
        let eye = eye_point(None);
        let turn = Sticks {
            turn: 1.0,
            ..default()
        };
        let after = moved_root(root, head, turn, 0.5);
        assert!(
            close(after.transform_point(head.translation), eye),
            "turned about the head"
        );
        // To the right: what was ahead (−z) is now towards +x.
        let ahead = after.rotation * head.rotation * Vec3::NEG_Z;
        assert!(ahead.x > 0.4 && ahead.z < 0.0);
        assert!(close(after.rotation * Vec3::Y, Vec3::Y));
        // Ahead is then where the player faces, not where the table is.
        let walk = Sticks {
            walk: 1.0,
            ..default()
        };
        let walked = moved_root(after, head, walk, 1.0);
        let step = walked.transform_point(head.translation) - eye;
        assert!((step.length() - WALK_SPEED).abs() < 1e-4 && step.y.abs() < 1e-5);
        assert!(
            step.normalize()
                .dot(Vec3::new(ahead.x, 0.0, ahead.z).normalize())
                > 0.999
        );
    }

    #[test]
    fn every_kind_of_controller_opens_the_menu() {
        for (profile, bindings) in BINDINGS {
            assert!(
                bindings.iter().any(|(input, _)| *input == Input::Menu),
                "{profile}"
            );
        }
    }

    #[test]
    fn every_kind_of_controller_points_and_clicks_with_either_hand() {
        for (profile, bindings) in BINDINGS {
            assert!(profile.starts_with("/interaction_profiles/"));
            for hand in HANDS {
                for (input, ending) in [(Input::Aim, "/input/aim/pose"), (Input::Select, "")] {
                    assert!(
                        bindings.iter().any(|(i, path)| *i == input
                            && path.starts_with(hand)
                            && path.ends_with(ending)),
                        "{profile}: {input:?} for {hand}"
                    );
                }
            }
            for (_, path) in *bindings {
                assert!(HANDS.iter().any(|hand| path.starts_with(hand)), "{path}");
                // One button, one thing to do.
                assert_eq!(bindings.iter().filter(|(_, p)| p == path).count(), 1);
            }
        }
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
