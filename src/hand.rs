//! What the player holds while a move is built: a piece picked up, lifted over its space and
//! bobbing gently; a piece put down where it was moved, before the move is sent; and what the
//! move aims (an arrow) held under the pointer, bobbing too, the pointer's own arrow hidden
//! while it is over the board.
//!
//! The board (`board.rs`) says which piece is held or put where; this moves them there smoothly,
//! every frame. The pointer's place on the board comes from picking, so a ray pointer in a
//! headset holds the piece as the mouse does.

use std::f32::consts::TAU;

use bevy::picking::backend::HitData;
use bevy::picking::events::{PointerMove, PointerOut, PointerOver};
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};

use crate::play::Spot;

pub struct HandPlugin;

impl Plugin for HandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PointerOnBoard>()
            .add_observer(on_move)
            .add_observer(on_over)
            .add_observer(on_out)
            .add_systems(Update, (carry, hold, show_cursor));
    }
}

/// How high a piece picked up is lifted, and how far it bobs either way, at the full pitch.
pub const LIFT: f32 = 0.014;
pub const BOB: f32 = 0.0022;
/// How long one bob takes, up and down.
pub const BOB_PERIOD: f32 = 1.6;
/// How quickly a piece goes where it is wanted: the share of the way left it goes in a second is
/// `1 - e^-RATE`.
const RATE: f32 = 14.0;

/// How far above its held height a held piece is, `secs` into the game.
pub fn bob(secs: f32) -> f32 {
    BOB * (secs * TAU / BOB_PERIOD).sin()
}

/// The share of the way left a piece goes in `dt` seconds, eased: as quickly whatever the
/// frame rate.
pub fn approach(dt: f32) -> f32 {
    1.0 - (-RATE * dt).exp()
}

/// Where a piece on the board is set down when nothing holds it.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Rest(pub Transform);

/// A piece of the board the move being built holds.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub enum Carry {
    /// Picked up: held `lift` metres over where it rests, bobbing (scaled by `bob`, the board's
    /// scale).
    Held { lift: f32, scale: f32 },
    /// Put down where it is moved to, as it will stand there.
    Put(Transform),
}

/// What the move aims, held under the pointer while it is over the board: `lift` over the
/// board's surface (at `board`), bobbing by `scale` of [`BOB`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Held {
    pub board: f32,
    pub lift: f32,
    pub scale: f32,
}

/// Where the pointer is over the board, while it is: what it is on (a space or a piece), and
/// where it looks from, both in the world.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct PointerOnBoard(pub Option<(Vec3, Vec3)>);

/// Where on the board's surface (at height `board`) the pointer is, looking from `eye` at
/// `hit`: on a piece, the board just behind it.
pub fn on_surface(hit: Vec3, eye: Vec3, board: f32) -> Vec3 {
    let down = eye.y - hit.y;
    if down <= 1e-6 {
        return Vec3::new(hit.x, board, hit.z);
    }
    let t = (eye.y - board) / down;
    let at = eye + (hit - eye) * t;
    Vec3::new(at.x, board, at.z)
}

fn point(
    spots: &Query<(), With<Spot>>,
    eyes: &Query<&GlobalTransform>,
    entity: Entity,
    hit: &HitData,
    on: &mut PointerOnBoard,
) {
    let Some(at) = hit.position.filter(|_| spots.contains(entity)) else {
        return;
    };
    let eye = eyes
        .get(hit.camera)
        .map_or(at + Vec3::Y, |e| e.translation());
    if on.0 != Some((at, eye)) {
        on.0 = Some((at, eye));
    }
}

fn on_move(
    moved: On<PointerMove>,
    spots: Query<(), With<Spot>>,
    eyes: Query<&GlobalTransform>,
    mut on: ResMut<PointerOnBoard>,
) {
    point(&spots, &eyes, moved.entity, &moved.hit, &mut on);
}

fn on_over(
    over: On<PointerOver>,
    spots: Query<(), With<Spot>>,
    eyes: Query<&GlobalTransform>,
    mut on: ResMut<PointerOnBoard>,
) {
    point(&spots, &eyes, over.entity, &over.hit, &mut on);
}

/// Off a space or piece: off the board, unless the pointer is over another at once (its
/// `PointerOver` follows).
fn on_out(out: On<PointerOut>, spots: Query<(), With<Spot>>, mut on: ResMut<PointerOnBoard>) {
    if spots.contains(out.entity) && on.0.is_some() {
        on.0 = None;
    }
}

/// Moves each piece of the board towards where it is wanted: where it rests, held over it, or
/// put where it is moved to.
fn carry(time: Res<Time>, mut pieces: Query<(&Rest, Option<&Carry>, &mut Transform)>) {
    let k = approach(time.delta_secs());
    let secs = time.elapsed_secs();
    for (rest, carry, mut t) in &mut pieces {
        let wanted = match carry {
            None => rest.0,
            Some(Carry::Held { lift, scale }) => {
                let mut up = rest.0;
                up.translation.y += lift + bob(secs) * scale;
                up
            }
            Some(Carry::Put(to)) => *to,
        };
        if *t == wanted {
            continue;
        }
        let near = t.translation.distance_squared(wanted.translation) < 1e-10
            && t.rotation.angle_between(wanted.rotation) < 1e-4;
        if near && carry.is_none() {
            *t = wanted;
            continue;
        }
        t.translation = t.translation.lerp(wanted.translation, k);
        t.rotation = t.rotation.slerp(wanted.rotation, k);
        t.scale = t.scale.lerp(wanted.scale, k);
    }
}

/// What the move aims follows the pointer over the board, bobbing; it is not shown off it.
fn hold(
    time: Res<Time>,
    on: Res<PointerOnBoard>,
    mut held: Query<(&Held, &mut Transform, &mut Visibility)>,
) {
    for (held, mut t, mut vis) in &mut held {
        let Some((hit, eye)) = on.0 else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let at = on_surface(hit, eye, held.board);
        let up = held.lift + bob(time.elapsed_secs()) * held.scale;
        let wanted = at + Vec3::Y * up;
        if *vis == Visibility::Hidden {
            // Where the pointer comes onto the board, not sliding in from where it left.
            t.translation = wanted;
        } else {
            let k = approach(time.delta_secs()) * 2.0;
            t.translation = t.translation.lerp(wanted, k.min(1.0));
        }
        vis.set_if_neq(Visibility::Inherited);
    }
}

/// The pointer's own arrow is hidden while what the move aims is held in its place.
fn show_cursor(
    on: Res<PointerOnBoard>,
    held: Query<(), With<Held>>,
    mut windows: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let visible = on.0.is_none() || held.is_empty();
    for mut cursor in &mut windows {
        if cursor.visible != visible {
            cursor.visible = visible;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    #[test]
    fn a_held_piece_bobs_gently_about_its_height() {
        assert!(bob(0.0).abs() < 1e-9);
        assert!(
            (bob(BOB_PERIOD / 4.0) - BOB).abs() < 1e-6,
            "up at a quarter"
        );
        assert!(
            (bob(BOB_PERIOD * 0.75) + BOB).abs() < 1e-6,
            "down at three quarters"
        );
        const { assert!(BOB < LIFT / 4.0, "it never comes near the board") };
    }

    #[test]
    fn a_piece_goes_as_quickly_whatever_the_frame_rate() {
        let left =
            |frames: u32, dt: f32| (0..frames).fold(1.0, |left, _| left * (1.0 - approach(dt)));
        let fast: f32 = left(120, 1.0 / 120.0);
        let slow: f32 = left(30, 1.0 / 30.0);
        assert!((fast - slow).abs() < 1e-5);
        assert!(fast < 1e-5, "there within a second");
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, HandPlugin));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            16,
        )));
        app
    }

    #[test]
    fn a_piece_is_lifted_put_where_it_moves_and_set_back_down() {
        let mut app = app();
        let rest = Transform::from_xyz(0.1, 0.03, 0.2);
        let piece = app.world_mut().spawn((Rest(rest), rest)).id();
        let at = |app: &mut App| *app.world().get::<Transform>(piece).unwrap();

        app.world_mut().entity_mut(piece).insert(Carry::Held {
            lift: LIFT,
            scale: 1.0,
        });
        for _ in 0..120 {
            app.update();
        }
        let up = at(&mut app).translation.y - rest.translation.y;
        assert!((up - LIFT).abs() <= BOB * 1.01, "held over it: {up}");

        let to = Transform::from_xyz(-0.1, 0.03, 0.0);
        app.world_mut().entity_mut(piece).insert(Carry::Put(to));
        for _ in 0..120 {
            app.update();
        }
        assert!(at(&mut app).translation.distance(to.translation) < 1e-4);

        app.world_mut().entity_mut(piece).remove::<Carry>();
        for _ in 0..240 {
            app.update();
        }
        assert_eq!(at(&mut app), rest, "set down exactly where it rests");
    }

    #[test]
    fn what_is_aimed_follows_the_pointer_and_hides_the_cursor() {
        let mut app = app();
        let window = app
            .world_mut()
            .spawn((Window::default(), CursorOptions::default(), PrimaryWindow))
            .id();
        let held = app
            .world_mut()
            .spawn((
                Held {
                    board: 0.031,
                    lift: 0.02,
                    scale: 1.0,
                },
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(held),
            Some(&Visibility::Hidden)
        );
        assert!(app.world().get::<CursorOptions>(window).unwrap().visible);

        // On a piece 2 cm tall, seen from above and in front: over the board behind it.
        let eye = Vec3::new(0.1, 0.5, 0.5);
        let hit = Vec3::new(0.1, 0.051, -0.05);
        app.world_mut().resource_mut::<PointerOnBoard>().0 = Some((hit, eye));
        app.update();
        let t = app.world().get::<Transform>(held).unwrap().translation;
        let under = on_surface(hit, eye, 0.031);
        assert!(under.z < hit.z, "behind the piece, as the eye sees it");
        assert!((t.x - under.x).abs() < 1e-6 && (t.z - under.z).abs() < 1e-6);
        assert!((t.y - 0.051).abs() <= BOB + 1e-6);
        assert_eq!(
            app.world().get::<Visibility>(held),
            Some(&Visibility::Inherited)
        );
        assert!(!app.world().get::<CursorOptions>(window).unwrap().visible);

        app.world_mut().resource_mut::<PointerOnBoard>().0 = None;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(held),
            Some(&Visibility::Hidden)
        );
        assert!(app.world().get::<CursorOptions>(window).unwrap().visible);

        // Nothing held: the cursor is the pointer's own, on the board or off it.
        app.world_mut().resource_mut::<PointerOnBoard>().0 = Some((Vec3::ZERO, Vec3::Y));
        app.world_mut().entity_mut(held).despawn();
        app.update();
        assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
    }
}
