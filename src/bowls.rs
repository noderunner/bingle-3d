//! A bowl of stones for each seat, on the table beside the board, as Go stones are kept: for the
//! games whose appearance file says so (`[bowls]`), in every environment. Only to look at: a
//! stone is still played by clicking where it goes.
//!
//! Each bowl is at its seat's right hand, by the board's corner on that seat's side, and holds a
//! heap of stones in the material of the seat's pieces. The models are the asset pipeline's
//! (`assets-src/blender/tableware.py`).

use std::f32::consts::TAU;

use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;

use crate::appearance::BowlLook;
use crate::assets;

/// The bowl's widest, across (`tableware.py`), and the space left between it and the board.
pub const BOWL_RADIUS: f32 = 0.066;
const GAP: f32 = 0.03;

/// Where each seat's bowl stands on the table, and how it is turned: beside a board that reaches
/// `half` from the table's middle (in x and in z), for seats sitting round the table as
/// `environment::seat_places` puts them, the first on the player's side (+z).
pub fn places(seats: usize, half: Vec2) -> Vec<(Vec2, f32)> {
    (0..seats)
        .map(|i| {
            let turn = TAU * i as f32 / seats as f32;
            // Towards the seat, and to its right as it faces the board.
            let back = Vec2::new(turn.sin(), turn.cos());
            let right = Vec2::new(turn.cos(), -turn.sin());
            let reach = |d: Vec2| d.x.abs() * half.x + d.y.abs() * half.y;
            let at = right * (reach(right) + GAP + BOWL_RADIUS)
                + back * (reach(back) - BOWL_RADIUS).max(0.0);
            (at, turn)
        })
        .collect()
}

/// Stands the bowls beside the board, as children of `root`: `stones` is each seat's material,
/// in seat order.
pub fn spawn(
    commands: &mut Commands,
    root: Entity,
    server: &AssetServer,
    look: &BowlLook,
    bowl: Handle<StandardMaterial>,
    stones: &[Handle<StandardMaterial>],
    half: Vec2,
) {
    let mesh = |file: &str| -> Handle<Mesh> {
        server.load(
            GltfAssetLabel::Primitive {
                mesh: 0,
                primitive: 0,
            }
            .from_asset(assets::path(file)),
        )
    };
    let (bowl_mesh, heap_mesh) = (mesh(&look.model), mesh(&look.stones));
    for (i, ((at, turn), stone)) in places(stones.len(), half)
        .into_iter()
        .zip(stones)
        .enumerate()
    {
        // Each heap turned its own way, so no two bowls are alike.
        let yaw = turn + 1.9 * i as f32;
        commands.spawn((
            Name::new(format!("Bowl of seat {}", i + 1)),
            Mesh3d(bowl_mesh.clone()),
            MeshMaterial3d(bowl.clone()),
            Transform::from_xyz(at.x, 0.0, at.y).with_rotation(Quat::from_rotation_y(yaw)),
            ChildOf(root),
            children![(Mesh3d(heap_mesh.clone()), MeshMaterial3d(stone.clone()))],
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_bowl_is_at_its_seats_right_hand_clear_of_the_board() {
        let half = Vec2::new(0.15, 0.12);
        let two = places(2, half);
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
        for (at, _) in places(4, half) {
            let outside = at.x.abs() > half.x + BOWL_RADIUS - 1e-5
                || at.y.abs() > half.y + BOWL_RADIUS - 1e-5;
            assert!(outside, "{at}");
        }
        assert!(places(0, half).is_empty());
    }
}
