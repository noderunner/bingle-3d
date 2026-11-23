//! A piece of two sides: a disc light on one side and dark on the other, which a seat plays
//! its own side up, and which is turned over when it changes hands. Its model's two parts are
//! its upper and lower halves, each the other's mirror image, so it lies either side up as the
//! model is, its two parts' materials swapped; only turning it over moves it. Which part is
//! which material, and which side each seat's are up, are its look's (`PieceLook::sides`,
//! `up`). No Bevy systems here: `board.rs` dresses the pieces and turns them.

use std::f32::consts::PI;

use bevy::math::{Quat, Vec3};
use bevy::transform::components::Transform;

/// How long a piece takes to turn over, from the moment it starts.
pub const FLIP_TIME: f32 = 0.45;
/// How long the first waits, for the piece played to be set down, and how much longer each
/// waits for every cell it is further from it, so they go over one after another outwards.
pub const FLIP_WAIT: f32 = 0.3;
pub const FLIP_EACH: f32 = 0.09;
/// How high a piece is raised as it turns over (metres at the full pitch): enough for a disc
/// across a whole cell to clear the board as it stands on its edge.
pub const FLIP_LIFT: f32 = 0.016;

/// What the model's upper half and its lower half are made of, of `parts` (its first part's,
/// then its second's), with the second part's side up when `turned`.
pub fn halves<T: Clone>(parts: &[T; 2], turned: bool) -> (T, T) {
    let (up, down) = if turned { (1, 0) } else { (0, 1) };
    (parts[up].clone(), parts[down].clone())
}

/// The axis a piece turns over about: across the way from where the move was played (`from`)
/// to it, so it tips over away from there; across the table if there is no such way.
pub fn flip_axis(from: Option<Vec3>, to: Vec3) -> Vec3 {
    let away = from.map(|f| (to - f).with_y(0.0)).unwrap_or(Vec3::ZERO);
    Vec3::Y.cross(away).try_normalize().unwrap_or(Vec3::X)
}

/// How long a piece waits before it starts to turn over: by how many cells (`pitch` apart) it
/// is from where the move was played.
pub fn flip_delay(from: Option<Vec3>, to: Vec3, pitch: f32) -> f32 {
    let cells = from.map_or(0.0, |f| (to - f).with_y(0.0).length() / pitch.max(1e-6));
    FLIP_WAIT + FLIP_EACH * (cells - 1.0).max(0.0)
}

/// Where a piece turning over is, `secs` after it was told to, starting after `delay`: from
/// where it lies (`rest`) half a turn about `axis`, its new side down, as its old side was up,
/// raised by up to `lift` as it goes over, and down where it lies again, its new side up.
/// Whether it is done.
pub fn flip_pose(
    rest: Transform,
    axis: Vec3,
    lift: f32,
    delay: f32,
    secs: f32,
) -> (Transform, bool) {
    let t = ((secs - delay) / FLIP_TIME).clamp(0.0, 1.0);
    if t >= 1.0 {
        return (rest, true);
    }
    // Slow to start and slow to land, as a hand turns one over.
    let e = t * t * (3.0 - 2.0 * t);
    let mut at = rest;
    at.rotation = Quat::from_axis_angle(axis, PI * (1.0 + e)) * rest.rotation;
    at.translation.y += lift * (PI * e).sin();
    (at, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turned_piece_has_its_materials_swapped() {
        let parts = ["light", "dark"];
        assert_eq!(halves(&parts, false), ("light", "dark"));
        assert_eq!(halves(&parts, true), ("dark", "light"));
    }

    #[test]
    fn a_flip_starts_upside_down_rises_and_ends_where_it_lies() {
        let rest = Transform::from_xyz(0.1, 0.002, -0.05).with_rotation(Quat::from_rotation_y(0.7));
        let axis = flip_axis(Some(Vec3::ZERO), rest.translation);
        // Waiting: its new side down, where it lies.
        let (at, done) = flip_pose(rest, axis, 0.016, 0.3, 0.1);
        assert!(!done);
        assert!((at.rotation * Vec3::Y).distance(Vec3::NEG_Y) < 1e-5);
        assert!(at.translation.distance(rest.translation) < 1e-6);
        // Half way: on its edge, as high as it goes.
        let (at, _) = flip_pose(rest, axis, 0.016, 0.3, 0.3 + FLIP_TIME / 2.0);
        assert!((at.rotation * Vec3::Y).y.abs() < 1e-4, "on its edge");
        assert!((at.translation.y - rest.translation.y - 0.016).abs() < 1e-5);
        // Done: exactly where it lies.
        assert_eq!(
            flip_pose(rest, axis, 0.016, 0.3, 0.3 + FLIP_TIME),
            (rest, true)
        );
    }

    #[test]
    fn it_tips_away_from_the_move_and_the_further_ones_wait_longer() {
        let axis = flip_axis(Some(Vec3::ZERO), Vec3::new(0.056, 0.0, 0.0));
        assert!(
            axis.dot(Vec3::X).abs() < 1e-6 && axis.y.abs() < 1e-6,
            "across the way"
        );
        // A quarter turn about it carries the top away from where the move was played.
        let top = Quat::from_axis_angle(axis, PI / 2.0) * Vec3::Y;
        assert!(top.x > 0.99, "{top}");
        assert_eq!(flip_axis(None, Vec3::ONE), Vec3::X);
        assert_eq!(flip_axis(Some(Vec3::ONE), Vec3::ONE), Vec3::X);

        let pitch = 0.028;
        let next = flip_delay(Some(Vec3::ZERO), Vec3::new(pitch, 0.0, 0.0), pitch);
        let far = flip_delay(Some(Vec3::ZERO), Vec3::new(3.0 * pitch, 0.0, 0.0), pitch);
        assert!((next - FLIP_WAIT).abs() < 1e-6);
        assert!((far - next - 2.0 * FLIP_EACH).abs() < 1e-5);
        assert_eq!(flip_delay(None, Vec3::ONE, pitch), FLIP_WAIT);
    }
}
