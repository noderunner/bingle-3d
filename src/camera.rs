//! A camera that orbits a point on the table: drag with the right mouse button (or use the arrow
//! keys) to go round, and the wheel to come closer. Drag with the middle button, or the right
//! with Shift held (or use W, A, S, D), to slide the point it looks at across the table.
//!
//! The left button is left free for the gestures that pick pieces and positions (`play.rs`).
//! Over a panel the wheel is the panel's, to scroll it, not the camera's.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::MeshPickingCamera;
use bevy::prelude::*;

use crate::board::BoardBuilt;

pub struct OrbitCameraPlugin;

impl Plugin for OrbitCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, (frame_board, orbit_input, apply_orbit).chain());
    }
}

/// Where the camera is, as a point it looks at and a position on a sphere round that point.
/// Distances are in metres, angles in radians.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    pub focus: Vec3,
    pub radius: f32,
    /// Round the vertical axis; 0 looks from +Z towards the focus.
    pub yaw: f32,
    /// Above the table: 0 is level with it, π/2 straight down.
    pub pitch: f32,
}

impl OrbitCamera {
    pub const MIN_RADIUS: f32 = 0.2;
    pub const MAX_RADIUS: f32 = 3.0;
    pub const MIN_PITCH: f32 = 0.08;
    /// Just short of straight down, where yaw would stop meaning anything.
    pub const MAX_PITCH: f32 = FRAC_PI_2 - 0.05;

    /// Far enough back to see a board of this size (width and depth, in metres, centred on the
    /// focus) with some table round it, from the default angle.
    pub fn radius_to_frame(size: Vec2) -> f32 {
        (size.max_element() * 1.45).clamp(0.42, Self::MAX_RADIUS)
    }

    /// How far from the table's middle the camera may look: a little past the biggest board.
    pub const MAX_PAN: f32 = 0.6;

    /// Slides the point looked at across the table, by these shares of the distance to it: to
    /// the camera's right, and away from it. Seen from any angle, a drag moves the table under
    /// the pointer the same way.
    pub fn pan(&mut self, right: f32, away: f32) {
        let right_dir = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let away_dir = Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos());
        let focus = self.focus + (right_dir * right + away_dir * away) * self.radius;
        self.focus = focus.clamp_length_max(Self::MAX_PAN);
    }

    /// Turns by the given angles and moves in or out by a factor, keeping within limits.
    pub fn orbit(&mut self, d_yaw: f32, d_pitch: f32, zoom_factor: f32) {
        self.yaw = (self.yaw + d_yaw).rem_euclid(TAU);
        self.pitch = (self.pitch + d_pitch).clamp(Self::MIN_PITCH, Self::MAX_PITCH);
        self.radius = (self.radius * zoom_factor).clamp(Self::MIN_RADIUS, Self::MAX_RADIUS);
    }

    /// The camera's transform: on the sphere, looking at the focus, with +Y up.
    pub fn transform(&self) -> Transform {
        let offset = Vec3::new(
            self.radius * self.pitch.cos() * self.yaw.sin(),
            self.radius * self.pitch.sin(),
            self.radius * self.pitch.cos() * self.yaw.cos(),
        );
        Transform::from_translation(self.focus + offset).looking_at(self.focus, Vec3::Y)
    }
}

impl Default for OrbitCamera {
    /// Seated at the table, looking down at a board in front.
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            radius: 0.75,
            yaw: 0.0,
            pitch: 0.85,
        }
    }
}

fn spawn_camera(mut commands: Commands) {
    let orbit = OrbitCamera::default();
    commands.spawn((
        Camera3d::default(),
        // Bevy's default near plane (10 cm) would cut into the board when zoomed in close.
        Projection::from(PerspectiveProjection {
            near: 0.01,
            ..default()
        }),
        orbit.transform(),
        orbit,
        // The board's cells and pieces are picked through it.
        MeshPickingCamera,
    ));
}

/// Radians turned per pixel dragged.
const DRAG_SPEED: f32 = 0.005;
/// The share of the distance to the focus slid per pixel dragged, and per second with a key held.
const PAN_DRAG_SPEED: f32 = 0.0016;
const PAN_KEY_SPEED: f32 = 0.8;
/// Radians turned per second with an arrow key held.
const KEY_SPEED: f32 = 1.5;
/// How much one wheel notch brings the camera in.
const ZOOM_PER_LINE: f32 = 0.9;
/// Pixel-precise scrolling (touchpads) counts this many pixels as one notch.
const PIXELS_PER_LINE: f32 = 40.0;

#[expect(
    clippy::too_many_arguments,
    reason = "a system takes each input it reads as a parameter"
)]
fn orbit_input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    pointer: PointerOverUi,
    focus: Option<Res<InputFocus>>,
    menu: Option<Res<crate::escape_menu::EscapeMenu>>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    // The escape menu covers the table: the table stays as it is behind it.
    if !crate::escape_menu::closed(menu.as_deref()) {
        return;
    }
    let mut turn = Vec2::ZERO;
    let mut slide = Vec2::ZERO;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let panning =
        buttons.pressed(MouseButton::Middle) || (buttons.pressed(MouseButton::Right) && shift);
    if panning {
        // As if the table were held and dragged: it follows the pointer.
        slide += Vec2::new(-motion.delta.x, motion.delta.y) * PAN_DRAG_SPEED;
    } else if buttons.pressed(MouseButton::Right) {
        // Dragging right moves the camera left round the table, as if pulling the table.
        turn -= motion.delta * DRAG_SPEED;
    }
    // The keys are a text field's while one has the keyboard.
    let typing = focus.is_some_and(|f| f.get().is_some());
    let key = |k: KeyCode| if keys.pressed(k) && !typing { 1.0 } else { 0.0 };
    turn += Vec2::new(
        key(KeyCode::ArrowRight) - key(KeyCode::ArrowLeft),
        key(KeyCode::ArrowUp) - key(KeyCode::ArrowDown),
    ) * KEY_SPEED
        * time.delta_secs();
    slide += Vec2::new(
        key(KeyCode::KeyD) - key(KeyCode::KeyA),
        key(KeyCode::KeyW) - key(KeyCode::KeyS),
    ) * PAN_KEY_SPEED
        * time.delta_secs();

    let lines = match scroll.unit {
        _ if pointer.is_over_ui() => 0.0,
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / PIXELS_PER_LINE,
    };
    let zoom = ZOOM_PER_LINE.powf(lines);

    if turn == Vec2::ZERO && slide == Vec2::ZERO && lines == 0.0 {
        return;
    }
    for mut orbit in &mut cameras {
        orbit.orbit(turn.x, -turn.y, zoom);
        if slide != Vec2::ZERO {
            orbit.pan(slide.x, slide.y);
        }
    }
}

/// A new board on the table: every camera comes to where it sees it whole, looking from the
/// player's side.
fn frame_board(mut built: MessageReader<BoardBuilt>, mut cameras: Query<&mut OrbitCamera>) {
    let Some(board) = built.read().last() else {
        return;
    };
    for mut orbit in &mut cameras {
        *orbit = OrbitCamera {
            radius: OrbitCamera::radius_to_frame(board.size),
            ..default()
        };
    }
}

/// Whether the pointer is over a panel (any UI node that takes the pointer), where the wheel
/// scrolls the panel instead.
#[derive(SystemParam)]
struct PointerOverUi<'w, 's> {
    hovered: Res<'w, HoverMap>,
    nodes: Query<'w, 's, (), With<Node>>,
}

impl PointerOverUi<'_, '_> {
    fn is_over_ui(&self) -> bool {
        self.hovered
            .values()
            .any(|hits| hits.keys().any(|e| self.nodes.contains(*e)))
    }
}

fn apply_orbit(mut cameras: Query<(&OrbitCamera, &mut Transform), Changed<OrbitCamera>>) {
    for (orbit, mut transform) in &mut cameras {
        *transform = orbit.transform();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;

    #[test]
    fn the_camera_sits_on_its_sphere_and_looks_at_the_focus() {
        let orbit = OrbitCamera {
            focus: Vec3::new(0.1, 0.0, -0.2),
            radius: 1.5,
            yaw: 0.7,
            pitch: 0.4,
        };
        let t = orbit.transform();
        assert!((t.translation.distance(orbit.focus) - orbit.radius).abs() < EPS);
        let towards_focus = (orbit.focus - t.translation).normalize();
        assert!(t.forward().dot(towards_focus) > 1.0 - EPS);
        assert!(t.translation.y > orbit.focus.y, "above the table");
    }

    #[test]
    fn yaw_zero_looks_from_positive_z() {
        let t = OrbitCamera {
            pitch: 0.3,
            ..default()
        }
        .transform();
        assert!(t.translation.z > 0.0);
        assert!(t.translation.x.abs() < EPS);
    }

    #[test]
    fn orbiting_keeps_within_limits() {
        let mut orbit = OrbitCamera::default();
        orbit.orbit(0.0, 10.0, 0.0001);
        assert_eq!(orbit.pitch, OrbitCamera::MAX_PITCH);
        assert_eq!(orbit.radius, OrbitCamera::MIN_RADIUS);
        orbit.orbit(0.0, -10.0, 10_000.0);
        assert_eq!(orbit.pitch, OrbitCamera::MIN_PITCH);
        assert_eq!(orbit.radius, OrbitCamera::MAX_RADIUS);
    }

    #[test]
    fn panning_slides_the_focus_across_the_table_as_the_camera_faces() {
        let mut orbit = OrbitCamera::default();
        orbit.pan(0.1, 0.0);
        assert!(
            orbit.focus.x > 0.0 && orbit.focus.z.abs() < EPS,
            "right, from +Z"
        );
        assert_eq!(orbit.focus.y, 0.0, "along the table");
        let mut orbit = OrbitCamera::default();
        orbit.pan(0.0, 0.1);
        assert!(orbit.focus.z < 0.0, "away from the camera");
        // From the other side of the table, right is the other way.
        let mut orbit = OrbitCamera {
            yaw: std::f32::consts::PI,
            ..default()
        };
        orbit.pan(0.1, 0.0);
        assert!(orbit.focus.x < 0.0);
        let mut orbit = OrbitCamera::default();
        orbit.pan(100.0, 0.0);
        assert!(
            (orbit.focus.length() - OrbitCamera::MAX_PAN).abs() < EPS,
            "not off the table"
        );
    }

    #[test]
    fn a_bigger_board_is_seen_from_further_back() {
        let small = OrbitCamera::radius_to_frame(Vec2::new(0.2, 0.15));
        let big = OrbitCamera::radius_to_frame(Vec2::new(0.45, 0.35));
        assert!(small < big);
        assert!(OrbitCamera::radius_to_frame(Vec2::ZERO) >= OrbitCamera::MIN_RADIUS);
        assert!(OrbitCamera::radius_to_frame(Vec2::splat(9.0)) <= OrbitCamera::MAX_RADIUS);
    }

    #[test]
    fn a_new_board_frames_every_camera() {
        let mut app = App::new();
        app.add_message::<BoardBuilt>()
            .add_systems(Update, frame_board);
        let turned = OrbitCamera {
            yaw: 2.0,
            radius: 2.5,
            ..default()
        };
        let a = app.world_mut().spawn(turned).id();
        let b = app.world_mut().spawn(turned).id();
        app.world_mut().write_message(BoardBuilt {
            size: Vec2::new(0.3, 0.2),
        });
        app.update();
        for e in [a, b] {
            let orbit = app.world().get::<OrbitCamera>(e).unwrap();
            assert_eq!(orbit.yaw, 0.0);
            assert_eq!(
                orbit.radius,
                OrbitCamera::radius_to_frame(Vec2::new(0.3, 0.2))
            );
        }
    }

    #[test]
    fn yaw_wraps_round() {
        let mut orbit = OrbitCamera::default();
        orbit.orbit(TAU + 0.5, 0.0, 1.0);
        assert!((orbit.yaw - 0.5).abs() < EPS);
        orbit.orbit(-1.0, 0.0, 1.0);
        assert!((orbit.yaw - (TAU - 0.5)).abs() < EPS);
    }
}
