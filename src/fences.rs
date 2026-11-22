//! Walls between positions, drawn as fences: what the view of a game whose rules are written in
//! Go says stands between two neighbouring squares (Fendo's fences, `Position::walls`), each a
//! slender rail along the side they share, a little short of it so the squares' corners show.
//! Where it stands has no Bevy systems in it; `board.rs` draws them, and the fence a player takes
//! from the bowl both seats share (`[fences]` in the game's look) and puts on a side of a square.

use bevy::prelude::*;

/// A wall's name, the same whichever way round its two positions are given.
pub fn key(a: &str, b: &str) -> String {
    if a <= b {
        format!("{a}|{b}")
    } else {
        format!("{b}|{a}")
    }
}

/// Where a fence on the side between squares at `a` and `b` (their middles on the table)
/// stands, and how far it is turned about the vertical from lying along x: half way between
/// them, along the side they share.
pub fn between(a: Vec3, b: Vec3) -> (Vec3, f32) {
    let across = b - a;
    let middle = (a + b) / 2.0;
    // Neighbours side by side (east and west) share a side running towards the seats.
    let yaw = if across.x.abs() > across.z.abs() {
        std::f32::consts::FRAC_PI_2
    } else {
        0.0
    };
    (middle, yaw)
}

/// The neighbour of a square at `at` one `step` away (in layout units: x across, y towards the
/// near side) on a board `pitch` apart, among `centres`: the position whose middle is there.
pub fn neighbour<'a>(
    centres: impl IntoIterator<Item = (&'a String, &'a Vec3)>,
    at: Vec3,
    step: Vec2,
    pitch: f32,
) -> Option<&'a str> {
    let there = at + Vec3::new(step.x, 0.0, step.y) * pitch;
    centres
        .into_iter()
        .find(|(_, c)| c.distance(there) < pitch * 0.25)
        .map(|(id, _)| id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wall_is_named_by_its_two_squares_either_way_round() {
        assert_eq!(key("d3", "e3"), "d3|e3");
        assert_eq!(key("e3", "d3"), "d3|e3");
    }

    #[test]
    fn a_fence_stands_half_way_along_the_side_two_squares_share() {
        let (middle, yaw) = between(Vec3::new(0.0, 0.03, 0.0), Vec3::new(0.028, 0.03, 0.0));
        assert_eq!(middle, Vec3::new(0.014, 0.03, 0.0));
        assert!(
            (yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-6,
            "running towards the seats"
        );
        let (middle, yaw) = between(Vec3::ZERO, Vec3::new(0.0, 0.0, -0.028));
        assert_eq!((middle, yaw), (Vec3::new(0.0, 0.0, -0.014), 0.0));
    }

    #[test]
    fn a_squares_neighbour_is_found_a_step_away() {
        let centres: Vec<(String, Vec3)> =
            [("d3", 0.0, 0.0), ("e3", 0.028, 0.0), ("d4", 0.0, -0.028)]
                .map(|(id, x, z)| (id.to_string(), Vec3::new(x, 0.03, z)))
                .to_vec();
        let at = centres[0].1;
        let find = |step| neighbour(centres.iter().map(|(a, b)| (a, b)), at, step, 0.028);
        assert_eq!(find(Vec2::new(1.0, 0.0)), Some("e3"));
        assert_eq!(
            find(Vec2::new(0.0, -1.0)),
            Some("d4"),
            "north is away from the near side"
        );
        assert_eq!(find(Vec2::new(-1.0, 0.0)), None, "the board's edge");
    }
}
