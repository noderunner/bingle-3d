//! What the player holds while a move is built: a piece picked up, lifted over its space and
//! bobbing gently; a piece put down where it was moved, carried there over the board and lowered
//! onto it, quickly but plainly, before the move is sent; and what the move aims (an arrow), held
//! under the pointer, bobbing too, the pointer's own arrow hidden while it takes its place.
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
use crate::sounds::Cue;

pub struct HandPlugin;

/// The hand's systems, which the board orders after it says what is held.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hands;

impl Plugin for HandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PointerOnBoard>()
            .add_message::<Cue>()
            .add_observer(on_move)
            .add_observer(on_over)
            .add_observer(on_out)
            .add_systems(
                Update,
                ((set_off, carry).chain(), hold, show_cursor).in_set(Hands),
            );
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
/// A piece put down goes across the board at the height it is held, then is lowered onto it:
/// how long each takes, in seconds.
pub const ACROSS: f32 = 0.2;
pub const LOWER: f32 = 0.12;
/// How long a piece taken up off the board first rises straight up, clear of its neighbours,
/// when its route goes over them.
pub const RISE: f32 = 0.1;

/// How a piece put down gets there, when that is not the way a move's piece does: after a
/// wait, raised `over` metres higher than it would be (first straight up), taking `across`
/// seconds over, and sounding as `lands` when it is set down. A captured stone goes so to its
/// captor's tray.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Route {
    pub delay: f32,
    pub over: f32,
    pub across: f32,
    pub lands: Cue,
}

impl Default for Route {
    fn default() -> Self {
        Route {
            delay: 0.0,
            over: 0.0,
            across: ACROSS,
            lands: Cue::Put,
        }
    }
}

impl Route {
    fn rise(&self) -> f32 {
        if self.over > 0.0 { RISE } else { 0.0 }
    }

    /// How long it takes once it sets off.
    pub fn takes(&self) -> f32 {
        self.rise() + self.across + LOWER
    }
}

/// Where a piece put down is `secs` after it set off from `from` for `to`: across at the height
/// it was held (or a lift over where it goes, if it was not held), easing in and out, then
/// lowered, faster as it goes, as a hand sets a piece down. `None` once it is there.
#[cfg(test)]
pub fn travel(from: Transform, to: Transform, secs: f32) -> Option<Transform> {
    travel_by(from, to, secs, &Route::default())
}

/// Where a piece put down is `secs` after it set off, going by `route`: before it sets off
/// (`secs` below 0) where it was.
pub fn travel_by(from: Transform, to: Transform, secs: f32, route: &Route) -> Option<Transform> {
    if secs >= route.takes() {
        return None;
    }
    if secs < 0.0 {
        return Some(from);
    }
    let high = from.translation.y.max(to.translation.y + LIFT * to.scale.y) + route.over;
    let rise = route.rise();
    if secs < rise {
        // Straight up, slowing as it gets there.
        let f = secs / rise;
        let mut at = from;
        at.translation.y += (high - from.translation.y) * f * (2.0 - f);
        return Some(at);
    }
    let (from, secs) = if rise > 0.0 {
        (
            from.with_translation(from.translation.with_y(high)),
            secs - rise,
        )
    } else {
        (from, secs)
    };
    Some(over_and_down(from, to, secs, high, route.across))
}

/// Across to over `to` at `high` in `across` seconds, then lowered onto it.
fn over_and_down(from: Transform, to: Transform, secs: f32, high: f32, across: f32) -> Transform {
    let mut at = to;
    if secs < across {
        let f = secs / across;
        let eased = f * f * (3.0 - 2.0 * f);
        let over = to.translation.with_y(high);
        at.translation = from.translation.lerp(over, eased);
        // Up to the height first, if it was not held there.
        at.translation.y = from.translation.y + (high - from.translation.y) * eased.sqrt();
        at.rotation = from.rotation.slerp(to.rotation, eased);
        at.scale = from.scale.lerp(to.scale, eased);
    } else {
        let f = ((secs - across) / LOWER).min(1.0);
        at.translation.y = high + (to.translation.y - high) * f * f;
    }
    at
}

/// A piece put down on its way there: where it set off from, and how long ago.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Travel {
    pub from: Transform,
    pub secs: f32,
    pub route: Route,
}

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
    /// Picked up: held `lift` metres over where it rests, bobbing (scaled by `scale`, the
    /// board's).
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

/// A piece just put down sets off from where it is; one taken back from where it was put goes
/// back by itself.
fn set_off(
    mut commands: Commands,
    pieces: Query<(Entity, &Carry, &Transform, Option<&Route>), Changed<Carry>>,
    mut gone: RemovedComponents<Carry>,
) {
    for (entity, carry, t, route) in &pieces {
        let route = route.copied().unwrap_or_default();
        match carry {
            Carry::Put(_) => commands.entity(entity).insert(Travel {
                from: *t,
                secs: -route.delay,
                route,
            }),
            Carry::Held { .. } => commands.entity(entity).remove::<Travel>(),
        };
    }
    for entity in gone.read() {
        if let Ok(mut e) = commands.get_entity(entity) {
            e.remove::<Travel>();
        }
    }
}

/// Moves each piece of the board towards where it is wanted: where it rests, held over it, or
/// carried to where it is put.
fn carry(
    time: Res<Time>,
    mut pieces: Query<(&Rest, Option<&Carry>, Option<&mut Travel>, &mut Transform)>,
    mut cues: MessageWriter<Cue>,
) {
    let k = approach(time.delta_secs());
    let secs = time.elapsed_secs();
    for (rest, carry, trip, mut t) in &mut pieces {
        let wanted = match carry {
            None => rest.0,
            Some(Carry::Held { lift, scale }) => {
                let mut up = rest.0;
                up.translation.y += lift + bob(secs) * scale;
                up
            }
            Some(Carry::Put(to)) => {
                // Not set off yet: it waits where it is.
                let Some(mut trip) = trip else { continue };
                let takes = trip.route.takes();
                let landing = trip.secs < takes;
                trip.secs += time.delta_secs();
                // It touches the board: the sound of it set down.
                if landing && trip.secs >= takes {
                    cues.write(trip.route.lands);
                }
                if let Some(at) = travel_by(trip.from, *to, trip.secs, &trip.route) {
                    *t = at;
                    continue;
                }
                *to
            }
        };
        if *t == wanted {
            continue;
        }
        let near = t.translation.distance_squared(wanted.translation) < 1e-10
            && t.rotation.angle_between(wanted.rotation) < 1e-4;
        if near || matches!(carry, Some(Carry::Put(_))) {
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

    /// Every cue written, in order.
    #[derive(Resource, Default)]
    struct Heard(Vec<Cue>);

    fn hear(mut cues: MessageReader<Cue>, mut heard: ResMut<Heard>) {
        heard.0.extend(cues.read().copied());
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, HandPlugin))
            .init_resource::<Heard>()
            .add_systems(Update, hear.after(Hands));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            16,
        )));
        app
    }

    #[test]
    fn a_piece_put_down_goes_across_at_its_height_and_is_then_lowered() {
        let from = Transform::from_xyz(0.1, 0.05, 0.2);
        let to = Transform::from_xyz(-0.1, 0.03, 0.0);
        let at = |secs: f32| travel(from, to, secs).expect("on its way");
        assert_eq!(
            at(0.0).translation,
            from.translation,
            "it sets off from where it is"
        );
        // Across, at the height it was held: not down at the board on the way.
        let half = at(ACROSS / 2.0).translation;
        assert!((half.y - 0.05).abs() < 1e-6);
        assert!(half.x < 0.1 && half.x > -0.1);
        let over = at(ACROSS).translation;
        assert!(
            over.xz().distance(to.translation.xz()) < 1e-6,
            "over where it goes"
        );
        assert!((over.y - 0.05).abs() < 1e-6, "still held up");
        // Then lowered, faster as it goes, and there.
        let first = 0.05 - at(ACROSS + LOWER / 2.0).translation.y;
        assert!(first > 0.0 && first < 0.01, "slowly at first: {first}");
        assert_eq!(travel(from, to, ACROSS + LOWER), None);
        const { assert!(ACROSS + LOWER < 0.4, "quick") };

        // One that was not held is lifted over where it goes first, and not through the board.
        let low = Transform::from_xyz(0.1, 0.03, 0.2);
        let over = travel(low, to, ACROSS).unwrap().translation;
        assert!((over.y - (0.03 + LIFT)).abs() < 1e-6);
    }

    #[test]
    fn a_piece_going_by_a_route_waits_rises_clear_crosses_and_is_lowered() {
        let from = Transform::from_xyz(0.05, 0.035, 0.0);
        let to = Transform::from_xyz(0.0, 0.012, 0.3);
        let route = Route {
            delay: 0.2,
            over: 0.02,
            across: 0.3,
            lands: Cue::Capture,
        };
        let at = |secs: f32| travel_by(from, to, secs, &route).expect("on its way");
        assert_eq!(at(-0.1), from, "waiting where it is");
        // Straight up first, clear of its neighbours, before it moves across.
        let rising = at(RISE / 2.0).translation;
        assert_eq!(rising.xz(), from.translation.xz());
        assert!(rising.y > from.translation.y);
        let high = from.translation.y + route.over;
        let up = at(RISE).translation;
        assert!((up.y - high).abs() < 1e-6 && up.xz() == from.translation.xz());
        // Across at that height, then down onto where it goes.
        let half = at(RISE + route.across / 2.0).translation;
        assert!((half.y - high).abs() < 1e-6, "{half}");
        let over = at(RISE + route.across).translation;
        assert!(over.xz().distance(to.translation.xz()) < 1e-6);
        assert_eq!(route.takes(), RISE + 0.3 + LOWER);
        assert_eq!(travel_by(from, to, route.takes(), &route), None);
        // The default route is a move's piece's.
        assert_eq!(Route::default().takes(), ACROSS + LOWER);
    }

    #[test]
    fn a_piece_on_a_route_waits_before_setting_off_and_lands_with_its_sound() {
        let mut app = app();
        let to = Transform::from_xyz(0.0, 0.01, 0.2);
        let from = Transform::from_xyz(0.0, 0.03, 0.0);
        let stone = app
            .world_mut()
            .spawn((
                Rest(to),
                from,
                Route {
                    delay: 0.1,
                    over: 0.01,
                    across: 0.2,
                    lands: Cue::Capture,
                },
                Carry::Put(to),
            ))
            .id();
        for _ in 0..4 {
            app.update();
        }
        let t = *app.world().get::<Transform>(stone).unwrap();
        assert_eq!(t, from, "still waiting");
        for _ in 0..40 {
            app.update();
        }
        let t = *app.world().get::<Transform>(stone).unwrap();
        assert_eq!(t, to, "there");
        assert_eq!(app.world().resource::<Heard>().0, [Cue::Capture]);
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
        assert!(
            at(&mut app)
                .translation
                .xz()
                .distance(rest.translation.xz())
                < 1e-5
        );
        let held = at(&mut app).translation;

        // Put: carried across at its height, then lowered, in a third of a second.
        let to = Transform::from_xyz(-0.1, 0.03, 0.0);
        app.world_mut().entity_mut(piece).insert(Carry::Put(to));
        for _ in 0..7 {
            app.update();
        }
        let across = at(&mut app).translation;
        assert!(
            across.x < held.x && across.x > to.translation.x,
            "on its way: {across}"
        );
        assert!(
            across.y > to.translation.y + LIFT / 2.0,
            "held up as it goes: {across}"
        );
        assert!(
            app.world().resource::<Heard>().0.is_empty(),
            "silent until it lands"
        );
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(at(&mut app), to, "there");
        assert_eq!(
            app.world().resource::<Heard>().0,
            [Cue::Put],
            "it sounded once, as it touched the board"
        );

        app.world_mut().entity_mut(piece).remove::<Carry>();
        for _ in 0..240 {
            app.update();
        }
        assert_eq!(at(&mut app), rest, "set down exactly where it rests");
        assert!(app.world().get::<Travel>(piece).is_none());
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
