//! A bowl of stones for each color a seat plays, on the table beside the board, as Go stones
//! are kept: for the games whose appearance file says so (`[bowls]`), in every environment.
//!
//! A seat of one color has its bowl at its right hand, by the board's corner on that seat's
//! side; a seat of two (Blooms') has one at each hand, its first color on the left. Each holds a
//! heap of stones in its color's material. A click on a bowl takes a stone of its color, when
//! the move lets the player choose one (`play::Choice`); the stone then waits over the bowl. The
//! models are the asset pipeline's (`assets-src/blender/tableware.py`), made smaller by the
//! appearance's `size`.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::gltf::GltfAssetLabel;
use bevy::picking::Pickable;
use bevy::prelude::*;

use crate::appearance::BowlLook;
use crate::assets;
use crate::play::Choice;

/// The bowl's widest, across (`tableware.py`), and the space left between it and the board.
pub const BOWL_RADIUS: f32 = 0.066;
const GAP: f32 = 0.025;

/// How far across a board's outline (a closed polygon, x across and z towards the seat) reaches
/// either way from the middle at `z`: the widest of its edges there.
pub fn width_at(outline: &[Vec2], z: f32) -> f32 {
    let n = outline.len();
    let mut widest: f32 = 0.0;
    for i in 0..n {
        let (a, b) = (outline[i], outline[(i + 1) % n]);
        if (a.y - z) * (b.y - z) <= 0.0 && a.y != b.y {
            let x = a.x + (b.x - a.x) * (z - a.y) / (b.y - a.y);
            widest = widest.max(x.abs());
        }
    }
    widest
}

/// Where each seat's bowls stand on the table, and how each is turned: beside a board of this
/// `outline` (on the table, x and z), for seats sitting round the table as
/// `environment::seat_places` puts them, the first on the player's side (+z). `seats` is how many
/// bowls each seat has (one at its right hand, or two, at its left and right), each `radius`
/// across. Each stands by the board's edge on its seat's side, as close beside the board as its
/// outline lets it there, so a hexagonal board's narrowing near side brings them in.
pub fn places(seats: &[usize], outline: &[Vec2], radius: f32) -> Vec<Vec<(Vec2, f32)>> {
    let count = seats.len();
    seats
        .iter()
        .enumerate()
        .map(|(i, &bowls)| {
            let turn = TAU * i as f32 / count as f32;
            // Towards the seat, and to its right as it faces the board.
            let back = Vec2::new(turn.sin(), turn.cos());
            let right = Vec2::new(turn.cos(), -turn.sin());
            // The outline as the seat sees it: x to its right, z towards it.
            let seen: Vec<Vec2> = outline
                .iter()
                .map(|p| Vec2::new(p.dot(right), p.dot(back)))
                .collect();
            let near = seen.iter().fold(0.0f32, |z, p| z.max(p.y));
            let z = (near - radius).max(0.0);
            // As wide as the board is anywhere beside the bowl.
            let wide = (0..=8)
                .map(|k| width_at(&seen, z - radius + radius * k as f32 / 4.0))
                .fold(0.0f32, f32::max);
            let x = wide + GAP + radius;
            let sides: Vec<f32> = match bowls {
                0 => vec![],
                1 => vec![1.0],
                _ => vec![-1.0, 1.0],
            };
            sides
                .into_iter()
                .map(|side| (right * side * x + back * z, turn))
                .collect()
        })
        .collect()
}

/// A bowl, by the color word of the stones in it.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Bowl(pub String);

/// One bowl to stand: its color word, the material of its stones, and what a click on it
/// chooses, if anything.
pub struct Filled {
    pub color: String,
    pub stones: Handle<StandardMaterial>,
    pub choice: Option<crate::net::api::Gesture>,
    /// Whether it is this client's seat's: the stone the turn holds by itself waits over it.
    pub yours: bool,
}

/// Stands the bowls beside the board, as children of `root`: each seat's, in seat order. Says
/// where a stone taken from each bowl waits, by its color: over the bowl's rim.
pub fn spawn(
    commands: &mut Commands,
    root: Entity,
    server: &AssetServer,
    look: &BowlLook,
    bowl: Handle<StandardMaterial>,
    seats: Vec<Vec<Filled>>,
    outline: &[Vec2],
) -> HashMap<String, Vec3> {
    let mesh = |file: &str| -> Handle<Mesh> {
        server.load(
            GltfAssetLabel::Primitive {
                mesh: 0,
                primitive: 0,
            }
            .from_asset(assets::path(file)),
        )
    };
    let size = look.size.unwrap_or(1.0);
    let rim = assets::model_span(&look.model).map_or(0.04, |(_, high)| high) * size;
    let (bowl_mesh, heap_mesh) = (mesh(&look.model), mesh(&look.stones));
    let counts: Vec<usize> = seats.iter().map(Vec::len).collect();
    let mut over = HashMap::new();
    let mut n = 0;
    for (seat, (filled, places)) in seats
        .into_iter()
        .zip(places(&counts, outline, BOWL_RADIUS * size))
        .enumerate()
    {
        for (one, (at, turn)) in filled.into_iter().zip(places) {
            // Each heap turned its own way, so no two bowls are alike.
            let yaw = turn + 1.9 * n as f32;
            n += 1;
            over.insert(one.color.clone(), Vec3::new(at.x, rim, at.y));
            let mut bowl = commands.spawn((
                Name::new(format!("Bowl of {} (seat {})", one.color, seat + 1)),
                Bowl(one.color),
                Mesh3d(bowl_mesh.clone()),
                MeshMaterial3d(bowl.clone()),
                Transform::from_xyz(at.x, 0.0, at.y)
                    .with_rotation(Quat::from_rotation_y(yaw))
                    .with_scale(Vec3::splat(size)),
                Pickable::default(),
                ChildOf(root),
                children![(
                    Mesh3d(heap_mesh.clone()),
                    MeshMaterial3d(one.stones),
                    Pickable::default(),
                )],
            ));
            // A press on it may carry its stone, a choice or the turn's own (`drag.rs`).
            if one.yours || one.choice.is_some() {
                bowl.insert(crate::drag::Source);
            }
            if let Some(choice) = one.choice {
                bowl.insert(Choice(choice));
            }
        }
    }
    over
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_bowl_is_at_its_seats_right_hand_clear_of_the_board() {
        let half = Vec2::new(0.15, 0.12);
        let one = |seats: usize| -> Vec<(Vec2, f32)> {
            places(&vec![1; seats], &rectangle(half), BOWL_RADIUS)
                .into_iter()
                .flatten()
                .collect()
        };
        let two = one(2);
        let (near, far) = (two[0].0, two[1].0);
        assert!(
            near.x > half.x + BOWL_RADIUS,
            "to the right of the board: {near}"
        );
        assert!(
            near.y > 0.0 && near.y < half.y,
            "on the player's side, by the corner"
        );
        assert!(far.distance(-near) < 1e-5, "the other seat's is opposite");
        // Four seats: one beside each side of the board, none on it.
        for (at, _) in one(4) {
            let outside = at.x.abs() > half.x + BOWL_RADIUS - 1e-5
                || at.y.abs() > half.y + BOWL_RADIUS - 1e-5;
            assert!(outside, "{at}");
        }
        assert!(one(0).is_empty());
    }

    fn rectangle(half: Vec2) -> Vec<Vec2> {
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| Vec2::new(x * half.x, y * half.y))
            .to_vec()
    }

    #[test]
    fn a_hexagonal_boards_bowls_come_in_where_its_near_side_narrows() {
        // Points to the sides, a flat side near the player: as Blooms' board.
        let hexagon: Vec<Vec2> = (0..6)
            .map(|k| Vec2::from_angle(std::f32::consts::TAU * k as f32 / 6.0) * 0.2)
            .collect();
        let r = 0.04;
        let at = places(&[2], &hexagon, r)[0].clone();
        let (left, right) = (at[0].0, at[1].0);
        let by_the_widest = 0.2 + GAP + r;
        assert!(
            right.x < by_the_widest - 0.04,
            "nearer than beside the widest: {right}"
        );
        assert!(left.x < 0.0 && (left.x + right.x).abs() < 1e-5);
        // Clear of the board, all round the bowl.
        for k in 0..32 {
            let p = right + Vec2::from_angle(k as f32 / 32.0 * std::f32::consts::TAU) * r;
            assert!(p.x > width_at(&hexagon, p.y) - 1e-5, "{p}");
        }
        assert!((width_at(&hexagon, 0.0) - 0.2).abs() < 1e-5);
        assert!(width_at(&hexagon, 0.3) == 0.0, "nothing beyond the board");
    }

    #[test]
    fn a_seat_of_two_colors_has_a_bowl_at_each_hand_the_first_on_the_left() {
        let half = Vec2::new(0.15, 0.12);
        let r = BOWL_RADIUS * 0.7;
        let all = places(&[2, 2], &rectangle(half), r);
        let (left, right) = (all[0][0].0, all[0][1].0);
        assert!(left.x < -half.x && right.x > half.x, "{left} {right}");
        assert!(left.y > 0.0 && right.y > 0.0, "both on the player's side");
        // The other seat's, opposite: its left is the player's right, far side.
        assert!(all[1][0].0.distance(-left) < 1e-5);
        // None of the four is on another or on the board.
        let flat: Vec<Vec2> = all.iter().flatten().map(|(p, _)| *p).collect();
        for (i, a) in flat.iter().enumerate() {
            assert!(a.x.abs() > half.x + r - 1e-5, "{a}");
            for b in &flat[i + 1..] {
                assert!(a.distance(*b) > 2.0 * r, "{a} {b}");
            }
        }
    }
}
