//! Cards, dice, and discs with their rings: what lies on a table beside a board, or in place of
//! one. The models are the asset pipeline's (`assets-src/blender/playing_cards.py`, `dice.py`,
//! `discs.py`); this is how the client reads them: a model's parts, a card turned over, the
//! picture on the face of a card of the usual deck, a die turned to show a number, a disc
//! showing either side. Their generic looks are in `look.rs`.
//!
//! No game draws its dice, discs or rings with them yet; `showcase.rs` lays them out to look
//! at.

use std::f32::consts::PI;

use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;

use crate::assets;

/// A playing card of the usual size (5 to 7), lying face up about its middle with its top to
/// the far side of the table. Its parts are `face`, `back` and `edge`: a picture fills the face
/// or the back, whatever its size, and reads rightly on either.
pub const CARD: &str = "models/card.glb";
/// Across, top to bottom, and through, in metres.
pub const CARD_SIZE: Vec3 = Vec3::new(0.0635, 0.0889, 0.0003);

/// The picture for the face of a card of the usual deck, by its suit's symbol (♠ ♥ ♦ ♣) and
/// its rank's label (A, 2 to 10, J, Q, K) as the server gives them, or of a deck numbered from
/// 1 to 13 in the same suits (1, 11, 12 and 13 where the usual one has A, J, Q and K): one of
/// the pipeline's (`assets-src/blender/card_faces.py`). None for any other card: the client
/// has no picture of it.
pub fn standard_face(suit: &str, rank: &str) -> Option<String> {
    // A symbol may come with the mark that asks for its coloured form.
    let suit = match suit.trim_end_matches('\u{fe0f}') {
        "♠" => "spades",
        "♥" => "hearts",
        "♦" => "diamonds",
        "♣" => "clubs",
        _ => return None,
    };
    let rank = rank.trim().to_ascii_lowercase();
    let file = format!("textures/cards/faces/{suit}_{rank}_albedo.png");
    assets::exists(&file).then_some(file)
}

/// A die of six sides, about its middle. Its parts are `body` and `pips`.
pub const DIE: &str = "models/die.glb";
/// Across, in metres.
pub const DIE_SIDE: f32 = 0.016;

/// A disc light on one side and dark on the other, lying light side up about its middle. Its
/// parts are `light` and `dark`, each a face and the half of the edge beside it.
pub const DISC: &str = "models/disc.glb";
/// Across and through, in metres.
pub const DISC_SIZE: Vec2 = Vec2::new(0.015, 0.0036);

/// A ring a disc lies inside, about its middle. Its parts are `piece`, made of what its seat's
/// pieces are, and `trim`, a band round each edge of its top.
pub const RING: &str = "models/ring.glb";
/// Across, the hole across, and how tall, in metres.
pub const RING_SIZE: Vec3 = Vec3::new(0.025, 0.017, 0.004);

/// One part of a model's first mesh, by its place.
pub fn part(server: &AssetServer, file: &str, index: usize) -> Handle<Mesh> {
    server.load(
        GltfAssetLabel::Primitive {
            mesh: 0,
            primitive: index,
        }
        .from_asset(assets::path(file)),
    )
}

/// How a card lies: face up, as its model does, or turned over about its long axis, as a card
/// is turned.
pub fn card_lies(face_up: bool) -> Quat {
    if face_up {
        Quat::IDENTITY
    } else {
        Quat::from_rotation_z(PI)
    }
}

/// How a disc lies: light side up, as its model does, or turned over about the axis across the
/// table, dark side up.
pub fn disc_shows(light: bool) -> Quat {
    if light {
        Quat::IDENTITY
    } else {
        Quat::from_rotation_x(PI)
    }
}

/// The way the die's side with this number faces as its model lies: 1 up and 6 down, 2 to the
/// player and 5 to the far side, 3 to the right and 4 to the left (opposite sides add to seven).
pub fn die_side(number: u8) -> Option<Vec3> {
    Some(match number {
        1 => Vec3::Y,
        2 => Vec3::Z,
        3 => Vec3::X,
        4 => Vec3::NEG_X,
        5 => Vec3::NEG_Z,
        6 => Vec3::NEG_Y,
        _ => return None,
    })
}

/// The turn that brings a number up; none for a number a die does not have.
pub fn die_showing(number: u8) -> Quat {
    match die_side(number) {
        // Half a turn has no one shortest way: about the axis across the table.
        Some(side) if side == Vec3::NEG_Y => Quat::from_rotation_x(PI),
        Some(side) => Quat::from_rotation_arc(side, Vec3::Y),
        None => Quat::IDENTITY,
    }
}

/// A model of several parts, each given its material by its name (the name of the part's
/// material in the model's file). A part given none is left out.
pub fn spawn(
    commands: &mut Commands,
    server: &AssetServer,
    file: &str,
    materials: &[(&str, Handle<StandardMaterial>)],
    at: Transform,
) -> Entity {
    let whole = commands.spawn((at, Visibility::default())).id();
    for (index, name) in assets::model_parts(file).iter().enumerate() {
        let Some((_, material)) = materials.iter().find(|(part, _)| part == name) else {
            warn!("{file}: nothing for its part {name:?}");
            continue;
        };
        commands.spawn((
            Mesh3d(part(server, file, index)),
            MeshMaterial3d(material.clone()),
            ChildOf(whole),
        ));
    }
    whole
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_models_have_the_parts_the_client_names() {
        assert_eq!(assets::model_parts(CARD), ["face", "back", "edge"]);
        assert_eq!(assets::model_parts(DIE), ["body", "pips"]);
        assert_eq!(assets::model_parts(DISC), ["light", "dark"]);
        assert_eq!(assets::model_parts(RING), ["piece", "trim"]);
    }

    #[test]
    fn the_usual_deck_and_one_numbered_to_thirteen_have_a_face_for_each_card_and_no_other() {
        let usual = [
            "A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K",
        ];
        // What a deck numbered from 1 to 13 has where the usual one has its ace and courts.
        let numbered = ["1", "11", "12", "13"];
        let mut faces = Vec::new();
        for suit in ["♠", "♥", "♦", "♣"] {
            for rank in usual.into_iter().chain(numbered) {
                let face = standard_face(suit, rank);
                assert!(face.is_some(), "{rank} of {suit}");
                faces.extend(face);
            }
        }
        // Each its own, read as a colour, and nothing in the folder that no card shows.
        let all = assets::FILES
            .iter()
            .filter(|(file, _)| file.starts_with("textures/cards/faces/"));
        assert_eq!(all.count(), 52 + 16);
        faces.sort();
        faces.dedup();
        assert_eq!(faces.len(), 52 + 16);
        assert!(faces.iter().all(|f| crate::textures::is_colour(f)));

        assert_eq!(
            standard_face("♥\u{fe0f}", " q "),
            standard_face("♥", "Q"),
            "the symbol's coloured form, and a label as it happens to be written"
        );
        assert_ne!(
            standard_face("♥", "1"),
            standard_face("♥", "A"),
            "a 1 has 1 in its corners, an ace A"
        );
        assert_eq!(standard_face("♥", "14"), None);
        assert_eq!(standard_face("♥", "0"), None);
        assert_eq!(standard_face("♥", ""), None);
        assert_eq!(standard_face("hearts", "A"), None, "by symbol, not by id");
        assert_eq!(standard_face("★", "A"), None);
    }

    #[test]
    fn each_part_is_drawn_in_the_material_given_for_its_name() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>();
        let (face, back) = {
            let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            (
                materials.add(StandardMaterial::default()),
                materials.add(StandardMaterial::default()),
            )
        };
        let world = app.world_mut();
        let server = world.resource::<AssetServer>().clone();
        // Given in another order than the model's, and nothing for its edge.
        let given = [("back", back.clone()), ("face", face.clone())];
        let whole = spawn(
            &mut world.commands(),
            &server,
            CARD,
            &given,
            Transform::default(),
        );
        world.flush();
        let mut parts = world.query::<(&ChildOf, &MeshMaterial3d<StandardMaterial>)>();
        let drawn: Vec<_> = parts
            .iter(world)
            .filter(|(parent, _)| parent.parent() == whole)
            .map(|(_, material)| material.0.clone())
            .collect();
        assert_eq!(drawn, [face, back], "in the model's order; no edge");
    }

    #[test]
    fn the_models_are_the_sizes_the_client_takes_them_for() {
        let (low, high) = assets::model_span(CARD).unwrap();
        assert!(
            (high - low - CARD_SIZE.z).abs() < 1e-6,
            "a card's thickness"
        );
        assert!((high + low).abs() < 1e-6, "about its middle");
        let (low, high) = assets::model_span(DIE).unwrap();
        assert!((high - low - DIE_SIDE).abs() < 1e-5, "{}", high - low);
        let (low, high) = assets::model_span(DISC).unwrap();
        assert!((high - low - DISC_SIZE.y).abs() < 1e-5, "{}", high - low);
        assert!((high + low).abs() < 1e-5, "about its middle");
        let (low, high) = assets::model_span(RING).unwrap();
        assert!((high - low - RING_SIZE.z).abs() < 1e-5, "{}", high - low);
    }

    #[test]
    fn a_disc_lies_in_a_ring_and_rings_on_neighbouring_points_do_not_touch() {
        // With a millimetre to spare, so a disc dropped in a ring is not seen to cut it.
        const {
            assert!(RING_SIZE.y - DISC_SIZE.x >= 0.002 - 1e-6);
            assert!(RING_SIZE.x < 0.028, "a lattice of 28 mm");
            assert!(
                RING_SIZE.z > DISC_SIZE.y,
                "the ring stands above a disc inside it"
            );
        }
    }

    #[test]
    fn a_die_is_turned_to_show_each_number_and_opposite_sides_add_to_seven() {
        for number in 1..=6u8 {
            let side = die_side(number).unwrap();
            let up = die_showing(number) * side;
            assert!(up.distance(Vec3::Y) < 1e-5, "{number}: {up}");
            assert_eq!(die_side(7 - number), Some(-side), "{number}");
        }
        // As a die is made: 1, 2 and 3 go anticlockwise round the corner they share, seen from
        // outside it.
        let (one, two, three) = (Vec3::Y, Vec3::Z, Vec3::X);
        assert!(one.cross(two).dot(three) > 0.0);
        assert_eq!(die_side(0), None);
        assert_eq!(die_showing(9), Quat::IDENTITY);
    }

    #[test]
    fn a_disc_turned_over_shows_its_dark_side_up() {
        assert_eq!(disc_shows(true), Quat::IDENTITY);
        let over = disc_shows(false);
        assert!((over * Vec3::Y).distance(Vec3::NEG_Y) < 1e-5);
        assert!(
            (over * Vec3::X).distance(Vec3::X) < 1e-5,
            "turned about the axis across"
        );
    }

    #[test]
    fn a_card_turned_over_lies_on_its_face_with_its_top_where_it_was() {
        let over = card_lies(false);
        assert!((over * Vec3::Y).distance(Vec3::NEG_Y) < 1e-5);
        assert!(
            (over * Vec3::NEG_Z).distance(Vec3::NEG_Z) < 1e-5,
            "its top still to the far side"
        );
        assert_eq!(card_lies(true), Quat::IDENTITY);
    }
}
