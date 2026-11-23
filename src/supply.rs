//! What each seat has left to place, beside the board: the pieces of its reserve (Tak's stones
//! and capstones), in a line along the board's side at the seat's right hand, from its near
//! corner. A flat piece stands on its edge, the line of them a low wall; a tall one (a capstone)
//! stands upright, nearest the seat.
//!
//! The server says what each seat starts with (`GameInfo.reserves`), which pool each kind of
//! piece is taken from (`pieces[].pool`) and how many each seat has left (the position's
//! reserves). A click on a seat's own supply takes a piece from it, as a bowl does (`board.rs`).
//!
//! Which pools, and the layout, have no Bevy in them.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::net::api::GameInfo;

/// The space between the board and the supply, between one piece and the next along the line,
/// between one pool's pieces and the next's, and between rows.
pub const GAP: f32 = 0.03;
const BETWEEN: f32 = 0.0008;
const BETWEEN_POOLS: f32 = 0.012;
const BETWEEN_ROWS: f32 = 0.008;

/// A pool of pieces a seat keeps: its id, the kind of piece a click on it takes, and how many
/// each seat starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pool {
    pub id: String,
    pub piece: String,
    pub start: u32,
}

/// The pools of pieces each seat starts with some of, fewest first (so a capstone stands
/// nearest its seat). A click on one takes the plain kind of its pieces: the first another may
/// be put on (a flat stone, not a standing one), else the first. A pool no piece is taken from
/// is a counter's, not pieces.
pub fn pools(info: &GameInfo) -> Vec<Pool> {
    let mut out: Vec<Pool> = info
        .reserves
        .iter()
        .filter(|(_, start)| **start > 0)
        .filter_map(|(id, start)| {
            let kinds: Vec<_> = info.pieces.iter().filter(|p| &p.pool == id).collect();
            let piece = kinds.iter().find(|p| p.coverable).or(kinds.first())?;
            Some(Pool {
                id: id.clone(),
                piece: piece.id.clone(),
                start: *start,
            })
        })
        .collect();
    out.sort_by_key(|p| p.start);
    out
}

/// How a piece of a pool stands in the line: its length along the line, how far it reaches out
/// from it, and how it is turned (about its seat's way of looking) and raised to stand on the
/// table. `low` and `high` are its model's box, `scale` the board's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stance {
    pub along: f32,
    pub out: f32,
    pub turn: Quat,
    pub raise: f32,
    /// How high it stands.
    pub height: f32,
}

impl Stance {
    /// A flat piece (less than half as thick as it is wide) on its edge, its faces along the
    /// line; any other upright.
    pub fn of(low: Vec3, high: Vec3, scale: f32) -> Self {
        let size = (high - low) * scale;
        if size.y * 2.0 < size.x.min(size.z) {
            // A quarter turn about x: its thickness (y) goes along the line (z), its depth (z)
            // up.
            Stance {
                along: size.y,
                out: size.x,
                turn: Quat::from_rotation_x(FRAC_PI_2),
                raise: high.z * scale,
                height: size.z,
            }
        } else {
            Stance {
                along: size.z,
                out: size.x,
                turn: Quat::IDENTITY,
                raise: -low.y * scale,
                height: size.y,
            }
        }
    }
}

/// Where each piece of each pool goes, in the line's own terms: how far out from the board's
/// side (to the piece's middle) and how far along from the near corner. A line no longer than
/// `run` goes on in another row further out. `pools` is each pool's stance and how many.
pub fn line(pools: &[(Stance, usize)], run: f32) -> Vec<Vec<Vec2>> {
    let row = pools.iter().fold(0.0f32, |w, (s, _)| w.max(s.out)) + BETWEEN_ROWS;
    let (mut rows, mut along) = (0usize, 0.0f32);
    pools
        .iter()
        .enumerate()
        .map(|(i, (stance, count))| {
            if i > 0 && along > 0.0 {
                along += BETWEEN_POOLS;
            }
            (0..*count)
                .map(|_| {
                    if along > 0.0 && along + stance.along > run {
                        rows += 1;
                        along = 0.0;
                    }
                    let at = Vec2::new(rows as f32 * row + row / 2.0, along + stance.along / 2.0);
                    along += stance.along + BETWEEN;
                    at
                })
                .collect()
        })
        .collect()
}

/// Seat `seat`'s (of `seats`) supply on the table: each pool's places, from [`line`], beside a
/// board of this `outline`, along its side at the seat's right hand from the seat's near corner,
/// each turned as its stance says. Seats sit round the table as `bowls::places` has them.
pub fn places(
    seat: usize,
    seats: usize,
    outline: &[Vec2],
    pools: &[(Stance, usize)],
) -> Vec<Vec<Transform>> {
    let turn = TAU * seat as f32 / seats.max(1) as f32;
    let back = Vec2::new(turn.sin(), turn.cos());
    let right = Vec2::new(turn.cos(), -turn.sin());
    let side = outline.iter().fold(0.0f32, |x, p| x.max(p.dot(right)));
    let near = outline.iter().fold(0.0f32, |z, p| z.max(p.dot(back)));
    let far = outline.iter().fold(0.0f32, |z, p| z.min(p.dot(back)));
    line(pools, near - far)
        .into_iter()
        .zip(pools)
        .map(|(at, (stance, _))| {
            at.into_iter()
                .map(|p| {
                    let on = right * (side + GAP + p.x) + back * (near - p.y);
                    Transform::from_xyz(on.x, stance.raise, on.y)
                        .with_rotation(Quat::from_rotation_y(turn) * stance.turn)
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::api::PieceDef;

    fn tak() -> GameInfo {
        let piece = |id: &str, pool: &str, coverable: bool| PieceDef {
            id: id.into(),
            pool: pool.into(),
            coverable,
            ..Default::default()
        };
        GameInfo {
            id: "t".into(),
            name: "T".into(),
            seats: vec![],
            board: crate::net::api::Board {
                kind: "grid".into(),
                positions: vec![],
                regions: Default::default(),
            },
            actions: vec![],
            goals: vec![],
            // As the server lists them: by id.
            pieces: vec![
                piece("cap", "cap", false),
                piece("flat", "stone", true),
                piece("wall", "stone", false),
            ],
            counters: vec![],
            opening: None,
            reserves: [("stone", 21), ("cap", 1), ("captured", 0)]
                .map(|(k, v)| (k.to_string(), v))
                .into(),
            cards: None,
        }
    }

    #[test]
    fn a_seat_keeps_its_stones_and_capstones_and_a_click_takes_a_flat_stone() {
        let pools = pools(&tak());
        let got: Vec<(&str, &str, u32)> = pools
            .iter()
            .map(|p| (p.id.as_str(), p.piece.as_str(), p.start))
            .collect();
        assert_eq!(got, [("cap", "cap", 1), ("stone", "flat", 21)]);
        // None at the start (Tak 4×4's capstones), or a counter: not shown.
        let mut info = tak();
        info.reserves.insert("cap".into(), 0);
        assert_eq!(super::pools(&info).len(), 1);
    }

    #[test]
    fn a_flat_piece_stands_on_its_edge_and_a_tall_one_upright() {
        let flat = Stance::of(
            Vec3::new(-0.0093, -0.0033, -0.0093),
            Vec3::new(0.0093, 0.0033, 0.0093),
            1.0,
        );
        assert!(
            (flat.along - 0.0066).abs() < 1e-6,
            "its thickness along the line"
        );
        assert!(
            (flat.raise - 0.0093).abs() < 1e-6,
            "on its edge on the table"
        );
        // Its thickness turned along the line (z), its depth up.
        let up = flat.turn * Vec3::Z;
        assert!(up.y.abs() > 0.99, "{up}");
        let pawn = Stance::of(
            Vec3::new(-0.01, 0.0, -0.01),
            Vec3::new(0.01, 0.04, 0.01),
            0.5,
        );
        assert_eq!(pawn.turn, Quat::IDENTITY);
        assert!((pawn.along - 0.01).abs() < 1e-6 && pawn.raise == 0.0);
    }

    #[test]
    fn the_line_runs_from_the_near_corner_and_wraps_into_rows() {
        let thin = Stance {
            along: 0.01,
            out: 0.02,
            turn: Quat::IDENTITY,
            raise: 0.0,
            height: 0.02,
        };
        let lines = line(&[(thin, 1), (thin, 5)], 0.04);
        assert_eq!(lines.iter().map(Vec::len).collect::<Vec<_>>(), [1, 5]);
        let first = lines[0][0];
        assert!((first.y - 0.005).abs() < 1e-6, "from the corner: {first}");
        // The second pool after a gap, then on in a second row once the run is used.
        assert!(lines[1][0].y - first.y > 0.01 + BETWEEN_POOLS - 1e-6);
        let rows: Vec<f32> = lines[1].iter().map(|p| p.x).collect();
        assert!(rows[0] < rows[4], "a second row further out: {rows:?}");
        assert!(lines.iter().flatten().all(|p| p.y < 0.04 + 1e-6));
    }

    #[test]
    fn each_seats_supply_is_beside_the_board_at_its_right_hand_near_it() {
        let half = 0.1;
        let square = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, z)| Vec2::new(x * half, z * half))
            .to_vec();
        let thin = Stance {
            along: 0.01,
            out: 0.02,
            turn: Quat::IDENTITY,
            raise: 0.0,
            height: 0.02,
        };
        let mine = places(0, 2, &square, &[(thin, 3)]);
        let first = mine[0][0].translation;
        assert!(
            first.x > half + GAP,
            "beside the board to the right: {first}"
        );
        assert!(
            first.z > 0.0 && first.z < half,
            "from the near corner: {first}"
        );
        assert!(
            mine[0][2].translation.z < first.z,
            "going away from the seat"
        );
        let theirs = places(1, 2, &square, &[(thin, 3)]);
        let other = theirs[0][0].translation;
        assert!(
            other.distance(-first.with_y(0.0)) < 1e-5,
            "the other's opposite: {other}"
        );
    }
}
