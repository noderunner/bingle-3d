//! A small wooden tray in front of each seat, between the board and its player, holding the
//! pieces the seat has captured: one square for each capture its win wants, so the tray shows at
//! a glance how many it has and how many it still needs.
//!
//! The server says what is counted (`GameInfo.counters`: a number each seat keeps, Blooms'
//! captured stones, and the count a win rule wants of it) and how many each seat has (the
//! position's reserves). It does not say which pieces were taken: the board sees that, as the
//! pieces that leave it when a seat's count goes up (`fill`). A game whose count was already up
//! when it was opened has its trays filled with the other seats' colours.
//!
//! The layout and `fill` have no Bevy in them; `board.rs` builds the trays with `spawn` and
//! carries the captured pieces into them.

use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::board::CellShape;
use crate::look;
use crate::slab::{self, Rim};

/// How high a tray stands, the space between it and the board, the wood round its squares (of
/// a square), and the most squares a row has.
pub const HEIGHT: f32 = 0.007;
pub const GAP: f32 = 0.02;
const MARGIN: f32 = 0.45;
const ONE_ROW: usize = 10;
/// The tray's pieces are this much smaller than the board's, unless the game's look says.
pub const SIZE: f32 = 0.75;
/// The most squares a tray is made with: a count wanted beyond it is not shown as a tray.
pub const MOST: u32 = 100;

/// How many squares across and how many rows a tray of `n` has: wide and shallow, one row of
/// up to ten, and two rows for more, the second one short when `n` is odd (15 is 8 by 2, 20 is
/// 10 by 2, 50 is 25 by 2).
pub fn grid(n: usize) -> (usize, usize) {
    match n {
        0 => (0, 0),
        1..=ONE_ROW => (n, 1),
        _ => (n.div_ceil(2), 2),
    }
}

/// The middle of each of a tray's `n` squares, each `side` across, about the tray's middle: x
/// to its seat's right and z towards its seat. The first is the nearest the board on the left,
/// and they fill a row at a time, towards the seat.
pub fn squares(n: usize, side: f32) -> Vec<Vec2> {
    let (across, rows) = grid(n);
    (0..n)
        .map(|i| {
            let (col, row) = (i % across, i / across);
            Vec2::new(
                (col as f32 - (across as f32 - 1.0) / 2.0) * side,
                (row as f32 - (rows as f32 - 1.0) / 2.0) * side,
            )
        })
        .collect()
}

/// How far a tray of `n` squares of `side` reaches either way from its middle: across, and
/// towards and away from its seat.
pub fn half_size(n: usize, side: f32) -> Vec2 {
    let (across, rows) = grid(n);
    Vec2::new(across as f32, rows as f32) * side / 2.0 + Vec2::splat(MARGIN * side)
}

/// Where each of `seats` seats' trays stands, and how it is turned: in front of the seat, its
/// middle `depth` (half the tray's) beyond the board's near edge as the seat sees it, the board
/// having this `outline`. The seats sit round the table as `bowls::places` has them.
pub fn places(seats: usize, outline: &[Vec2], depth: f32) -> Vec<(Vec2, f32)> {
    (0..seats)
        .map(|i| {
            let turn = TAU * i as f32 / seats as f32;
            let back = Vec2::new(turn.sin(), turn.cos());
            let near = outline.iter().fold(0.0f32, |z, p| z.max(p.dot(back)));
            (back * (near + GAP + depth), turn)
        })
        .collect()
}

/// Where a piece in a tray came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The board: the piece of that index among those that left it.
    Board(usize),
    /// Nobody saw: one of this colour word, a seat's the captor does not play.
    Unseen(String),
}

/// How one seat's tray changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Change {
    /// The pieces added, in the order they go in.
    pub add: Vec<Source>,
    /// How many come out, last first (a move taken back).
    pub drop: usize,
}

/// How each seat's tray changes, when the seats' trays hold `had` and their counts are now
/// `counts`: a seat whose count went up takes pieces that left the board (`gone`, each by its
/// colour word) and are not of a colour it plays (`colours`, each seat's), each piece taken
/// once, and pieces nobody saw for the rest; one whose count went down gives the last back.
pub fn fill(had: &[usize], counts: &[u32], colours: &[Vec<String>], gone: &[&str]) -> Vec<Change> {
    let mut used = vec![false; gone.len()];
    had.iter()
        .zip(counts)
        .enumerate()
        .map(|(seat, (&had, &count))| {
            let count = count as usize;
            if count <= had {
                return Change {
                    add: vec![],
                    drop: had - count,
                };
            }
            let own = colours.get(seat).map_or(&[][..], Vec::as_slice);
            let others: Vec<&String> = colours
                .iter()
                .enumerate()
                .filter(|(s, _)| *s != seat)
                .flat_map(|(_, c)| c)
                .filter(|c| !own.contains(c))
                .collect();
            let add = (had..count)
                .map(|k| {
                    let seen =
                        (0..gone.len()).find(|&i| !used[i] && !own.iter().any(|c| c == gone[i]));
                    match seen {
                        Some(i) => {
                            used[i] = true;
                            Source::Board(i)
                        }
                        None => Source::Unseen(
                            others
                                .get(k % others.len().max(1))
                                .copied()
                                .or(own.first())
                                .map_or_else(String::new, |c| c.to_string()),
                        ),
                    }
                })
                .collect();
            Change { add, drop: 0 }
        })
        .collect()
}

/// Builds seat `seat`'s tray of `n` squares, each `side` across, on the table at `at` turned
/// `turn`, as children of `root`, of `wood` with its squares' lines cut in `lines`. Says where
/// each square's middle is on the table, on the tray's top.
pub fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    root: Entity,
    (wood, lines): (Handle<StandardMaterial>, Handle<StandardMaterial>),
    (at, turn): (Vec2, f32),
    (n, side): (usize, f32),
    seat: usize,
) -> Vec<Vec3> {
    let centres = squares(n, side);
    let corners = slab::cell_corners(CellShape::Square, side);
    let half = half_size(n, side) - Vec2::splat(MARGIN * side);
    let box_corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .map(|(x, z)| Vec2::new(x * half.x, z * half.y));
    let outline = slab::grown(&slab::hull(&box_corners), MARGIN * side, 4);
    let rim = Rim {
        height: HEIGHT,
        radius: 0.0015,
        steps: 4,
    };
    let place = Transform::from_xyz(at.x, 0.0, at.y).with_rotation(Quat::from_rotation_y(turn));
    let edges: Vec<usize> = (0..outline.len()).collect();
    let uv = |mesh: Mesh| look::world_uvs(mesh, place.translation);
    let tray = commands
        .spawn((
            Name::new(format!("Tray of seat {}", seat + 1)),
            Mesh3d(meshes.add(uv(slab::top(&outline, &rim)))),
            MeshMaterial3d(wood.clone()),
            place,
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Name::new("Tray's edge"),
        Mesh3d(meshes.add(uv(slab::side(&outline, &rim, &edges)))),
        MeshMaterial3d(wood.clone()),
        ChildOf(tray),
    ));
    // The lines between the squares: each square darkened, under a smaller one of the wood.
    commands.spawn((
        Name::new("Tray's lines"),
        Mesh3d(meshes.add(uv(slab::flat_cells(&centres, &corners, HEIGHT + 0.0001)))),
        MeshMaterial3d(lines),
        ChildOf(tray),
    ));
    let inner = slab::cell_corners(CellShape::Square, side * 0.9);
    commands.spawn((
        Name::new("Tray's squares"),
        Mesh3d(meshes.add(uv(slab::flat_cells(&centres, &inner, HEIGHT + 0.0002)))),
        MeshMaterial3d(wood),
        ChildOf(tray),
    ));
    centres
        .iter()
        .map(|c| place.transform_point(Vec3::new(c.x, HEIGHT + 0.0002, c.y)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tray_has_a_square_for_each_capture_wanted_in_full_rows() {
        // Blooms' boards: base 4, 5, 6, 8 and 10.
        assert_eq!(grid(15), (8, 2));
        assert_eq!(grid(20), (10, 2));
        assert_eq!(grid(25), (13, 2));
        assert_eq!(grid(40), (20, 2));
        assert_eq!(grid(50), (25, 2));
        // Few: one row.
        assert_eq!(grid(3), (3, 1));
        assert_eq!(grid(10), (10, 1));
        assert_eq!(grid(0), (0, 0));
        for n in 1..=100 {
            let (across, rows) = grid(n);
            assert!(across * rows >= n && across * (rows - 1) < n, "{n}");
            assert!(rows <= 2, "never more than two rows");
        }
    }

    #[test]
    fn the_squares_fill_from_the_board_side_left_first_inside_the_tray() {
        let side = 0.02;
        let at = squares(15, side);
        assert_eq!(at.len(), 15);
        // The first at the far left of the row nearest the board (z away from the seat).
        assert!(at[0].x < at[1].x && at[0].y == at[1].y);
        assert!(at[0].y < at[8].y, "the next row is nearer the seat");
        let half = half_size(15, side);
        for p in &at {
            assert!(p.x.abs() + side / 2.0 < half.x && p.y.abs() + side / 2.0 < half.y);
        }
        // Centred, a full tray (a short row leaves a square empty at its end).
        let full = squares(20, side);
        let middle = full.iter().sum::<Vec2>() / 20.0;
        assert!(middle.length() < 1e-6);
    }

    #[test]
    fn each_seats_tray_is_in_front_of_it_clear_of_the_board() {
        let hexagon: Vec<Vec2> = (0..6)
            .map(|k| Vec2::from_angle(TAU * k as f32 / 6.0) * 0.2)
            .collect();
        let depth = 0.03;
        let at = places(2, &hexagon, depth);
        let (mine, theirs) = (at[0].0, at[1].0);
        let near = 0.2 * (TAU / 6.0).sin();
        assert!(mine.x.abs() < 1e-6, "in the middle: {mine}");
        assert!((mine.y - (near + GAP + depth)).abs() < 1e-5, "{mine}");
        assert!(
            theirs.distance(-mine) < 1e-5,
            "the other's across the board"
        );
        assert!(
            (at[1].1 - TAU / 2.0).abs() < 1e-6,
            "turned to face its seat"
        );
    }

    fn words(seats: &[&[&str]]) -> Vec<Vec<String>> {
        seats
            .iter()
            .map(|s| s.iter().map(|w| w.to_string()).collect())
            .collect()
    }

    #[test]
    fn a_seat_whose_count_goes_up_takes_the_others_pieces_that_left_the_board() {
        let colours = words(&[&["red", "yellow"], &["blue", "black"]]);
        // Warm captured two of Cool's; a red stone left the board too (not Warm's to take).
        let gone = ["red", "black", "blue"];
        let changes = fill(&[3, 1], &[5, 1], &colours, &gone);
        assert_eq!(changes[0].add, [Source::Board(1), Source::Board(2)]);
        assert_eq!(changes[1], Change::default());
        // Both seats at once, each its own.
        let changes = fill(&[0, 0], &[1, 1], &colours, &gone);
        assert_eq!(changes[0].add, [Source::Board(1)]);
        assert_eq!(changes[1].add, [Source::Board(0)]);
    }

    #[test]
    fn captures_nobody_saw_are_the_other_seats_colours_and_a_take_back_empties_squares() {
        let colours = words(&[&["red", "yellow"], &["blue", "black"]]);
        // Opened with captures made already: the other seat's colours by turns.
        let changes = fill(&[0, 0], &[3, 0], &colours, &[]);
        assert_eq!(
            changes[0].add,
            ["blue", "black", "blue"].map(|c| Source::Unseen(c.into()))
        );
        // More counted than left the board.
        let changes = fill(&[2, 0], &[4, 0], &colours, &["black"]);
        assert_eq!(
            changes[0].add,
            [Source::Board(0), Source::Unseen("black".into())]
        );
        // A move taken back.
        assert_eq!(fill(&[4, 2], &[1, 2], &colours, &[])[0].drop, 3);
    }
}
