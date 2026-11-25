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

/// Where the bowl of fences stands on the table, to the first seat's left (x < 0), for a board
/// whose side there is `edge` from the middle and runs from `side.0` to `side.1` (z), where
/// `taken` are the stretches of that side (z) something already lies along (a seat's supply),
/// reaching `beyond` from the middle. Just off the board's edge, `gap` from it and from what lies
/// there, in the clear stretch nearest the side's middle that a bowl of `radius` fits; or, with
/// none, beyond what lies there.
pub fn bowl_place(
    edge: f32,
    side: (f32, f32),
    taken: &[(f32, f32)],
    beyond: f32,
    radius: f32,
    gap: f32,
) -> Vec2 {
    let mut taken: Vec<(f32, f32)> = taken.to_vec();
    taken.sort_by(|a, b| a.0.total_cmp(&b.0));
    // The clear stretches between what is taken, each narrowed to where a bowl's middle may be.
    let mut clear = Vec::new();
    let mut from = side.0;
    for &(lo, hi) in &taken {
        clear.push((from, lo - gap));
        from = from.max(hi + gap);
    }
    clear.push((from, side.1));
    clear
        .into_iter()
        .map(|(lo, hi)| (lo + radius, hi - radius))
        .filter(|(lo, hi)| lo <= hi)
        .map(|(lo, hi)| 0.0f32.clamp(lo, hi))
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        .map_or(Vec2::new(-(beyond + gap + radius), 0.0), |z| {
            Vec2::new(-(edge + gap + radius), z)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bowl_stands_just_off_the_board_where_the_side_is_clear() {
        // A side from -0.2 to 0.2, its far half taken by a supply reaching 0.27 out.
        let at = bowl_place(0.2, (-0.2, 0.2), &[(-0.2, 0.0)], 0.27, 0.04, 0.01);
        assert!((at.x - -0.25).abs() < 1e-6, "{at}");
        // As near the middle as clears the supply.
        assert!((at.y - 0.05).abs() < 1e-6, "{at}");
        // With nothing there, beside the middle of the side.
        assert_eq!(
            bowl_place(0.2, (-0.2, 0.2), &[], 0.2, 0.04, 0.01),
            Vec2::new(-0.25, 0.0)
        );
    }

    #[test]
    fn the_bowl_stands_beyond_a_side_with_no_room_for_it() {
        let taken = [(-0.2, -0.05), (0.0, 0.2)];
        let at = bowl_place(0.2, (-0.2, 0.2), &taken, 0.27, 0.04, 0.01);
        assert_eq!(at, Vec2::new(-0.32, 0.0));
    }

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
