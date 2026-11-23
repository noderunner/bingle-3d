//! `--showcase`: every model the client has for a piece, in each material its games give it,
//! with two-sided discs and their rings, the dice and a few playing cards of the usual deck,
//! laid out on the table with nothing
//! else: to look at an asset in the client's own light, as the asset pipeline asks before one
//! is called done (`assets-src/README.md`). A face is seen from one side here, as nowhere in
//! Blender, so this is where a card mapped or wound the wrong way shows.
//!
//! The pieces are found in the appearance files, so a new one is here by itself, and no game is
//! named. `--card-face` and `--card-back` try a picture (a JPEG or a PNG) on a card's sides.

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::gltf::GltfAssetLabel;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;

use crate::appearance::{Appearances, Stands};
use crate::board::BoardBuilt;
use crate::look::{self, MaterialSpec};
use crate::{assets, props};

pub struct ShowcasePlugin {
    /// Pictures to try on the cards' sides, from files outside the client.
    pub card_face: Option<PathBuf>,
    pub card_back: Option<PathBuf>,
}

#[derive(Resource, Clone)]
struct Pictures {
    face: Option<PathBuf>,
    back: Option<PathBuf>,
}

impl Plugin for ShowcasePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Pictures {
            face: self.card_face.clone(),
            back: self.card_back.clone(),
        })
        .add_systems(Startup, lay_out)
        .add_systems(Update, clear_the_view);
    }
}

/// Between two pieces in a row, between two rows, and between the rows and what lies in front;
/// and how many pieces a row holds.
const ACROSS: f32 = 0.034;
const BACK: f32 = 0.044;
const GAP: f32 = 0.03;
const ROW: usize = 8;

/// The cards lying face up, by suit and rank: one of each suit, and of each kind of face (an
/// ace, a court card, the fullest number card, and one that is not the same either way up).
const FACES: [(&str, &str); 4] = [("♠", "A"), ("♥", "Q"), ("♦", "10"), ("♣", "7")];

/// One piece to show: its model, what it is made of, and how its game stands it.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub model: String,
    pub material: MaterialSpec,
    pub stands: Stands,
    /// For a piece of two sides: its model's parts with their materials, in the model's order,
    /// and whether it lies turned over (`material` is then the side up's).
    pub sides: Option<(Vec<(String, MaterialSpec)>, bool)>,
}

/// Every piece model the appearances name, once in each material it is given: each
/// appearance's seats in turn, in the files' order.
pub fn pieces(all: &Appearances) -> Vec<Shown> {
    let mut out: Vec<Shown> = Vec::new();
    for look in &all.0 {
        for (word, seat) in &look.seats {
            let Some(theirs) = look.material(seat.pieces.as_ref()) else {
                continue;
            };
            for piece in look.pieces.values() {
                let sides = two_sides(look, piece, word);
                let material = match &sides {
                    Some((parts, turned)) => &parts[usize::from(*turned)].1,
                    None => look.material(piece.material.as_ref()).unwrap_or(theirs),
                };
                if out
                    .iter()
                    .any(|s| s.model == piece.model && s.material == *material)
                {
                    continue;
                }
                out.push(Shown {
                    model: piece.model.clone(),
                    material: material.clone(),
                    stands: piece.stands,
                    sides,
                });
            }
        }
    }
    out
}

/// A piece of two sides as a seat's lie: its model's parts with their materials, and whether its
/// second part's side is up. None for a piece of one, or one whose sides are not all there.
fn two_sides(
    look: &crate::appearance::Appearance,
    piece: &crate::appearance::PieceLook,
    word: &str,
) -> Option<(Vec<(String, MaterialSpec)>, bool)> {
    if piece.sides.is_empty() {
        return None;
    }
    let parts: Option<Vec<(String, MaterialSpec)>> = assets::model_parts(&piece.model)
        .into_iter()
        .map(|part| {
            let spec = look.materials.get(piece.sides.get(&part)?)?.clone();
            Some((part, spec))
        })
        .collect();
    let parts = parts.filter(|p| p.len() == 2)?;
    let turned = piece.up.get(word) == Some(&parts[1].0);
    Some((parts, turned))
}

/// Where each of `count` pieces stands on the table (x across, y towards the player): in rows
/// of `ROW` going back from y = 0, each row centred.
pub fn places(count: usize) -> Vec<Vec2> {
    (0..count)
        .map(|i| {
            let (row, column) = (i / ROW, i % ROW);
            let in_row = ROW.min(count - row * ROW);
            let x = (column as f32 - (in_row as f32 - 1.0) / 2.0) * ACROSS;
            Vec2::new(x, -(row as f32) * BACK)
        })
        .collect()
}

fn lay_out(
    mut commands: Commands,
    server: Res<AssetServer>,
    all: Res<Appearances>,
    pictures: Res<Pictures>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut built: MessageWriter<BoardBuilt>,
) {
    let pieces = pieces(&all);
    let depth = (pieces.len().saturating_sub(1) / ROW) as f32 * BACK;
    // In front of the pieces: the discs and rings, the dice, then the cards. The whole is
    // centred on the table.
    let rings_at = GAP + props::RING_SIZE.x / 2.0;
    let dice_at = rings_at + props::RING_SIZE.x / 2.0 + GAP + props::DIE_SIDE / 2.0;
    let cards_at = dice_at + props::DIE_SIDE / 2.0 + GAP + props::CARD_SIZE.y / 2.0;
    let front = cards_at + props::CARD_SIZE.y / 2.0;
    let shift = (depth - front) / 2.0;
    let root = commands
        .spawn((
            Name::new("Showcase"),
            Transform::from_xyz(0.0, 0.0, shift),
            Visibility::default(),
        ))
        .id();

    for (shown, spot) in pieces.iter().zip(places(pieces.len())) {
        let Some((low, _)) = assets::model_span(&shown.model) else {
            continue;
        };
        // Each as its model is, but one with a face turned a little, which shows it better
        // than square on.
        let turn = match shown.stands {
            Stands::Forward => 0.6,
            _ => 0.0,
        };
        let at =
            Transform::from_xyz(spot.x, -low, spot.y).with_rotation(Quat::from_rotation_y(turn));
        if let Some((parts, turned)) = &shown.sides {
            let parts: Vec<(&str, Handle<StandardMaterial>)> = parts
                .iter()
                .map(|(part, spec)| (part.as_str(), materials.add(spec.build(&server))))
                .collect();
            // Turned over as a disc is (`props::disc_shows`): about the axis across.
            let at = at.with_rotation(props::disc_shows(!turned));
            let whole = props::spawn(&mut commands, &server, &shown.model, &parts, at);
            commands.entity(whole).insert(ChildOf(root));
            continue;
        }
        commands.spawn((
            Name::new(shown.model.clone()),
            Mesh3d(
                server.load(
                    GltfAssetLabel::Primitive {
                        mesh: 0,
                        primitive: 0,
                    }
                    .from_asset(assets::path(&shown.model)),
                ),
            ),
            MeshMaterial3d(materials.add(shown.material.build(&server))),
            Transform::from_xyz(spot.x, -low, spot.y).with_rotation(Quat::from_rotation_y(turn)),
            ChildOf(root),
        ));
    }

    // The discs and rings, in their generic looks: a ring of each with a disc lying in it
    // showing its own side, discs showing either side, and the rings alone.
    let light = materials.add(look::disc_light().build(&server));
    let dark = materials.add(look::disc_dark().build(&server));
    let disc = [("light", light.clone()), ("dark", dark.clone())];
    let row = discs_and_rings();
    for (i, (ring, shows)) in row.iter().enumerate() {
        let x = (i as f32 - (row.len() as f32 - 1.0) / 2.0) * (props::RING_SIZE.x + 0.01);
        if let Some(light_ring) = ring {
            commands.spawn((
                Name::new(props::RING),
                Mesh3d(props::part(&server, props::RING, 0)),
                MeshMaterial3d(if *light_ring { &light } else { &dark }.clone()),
                Transform::from_xyz(x, props::RING_SIZE.z / 2.0, rings_at),
                ChildOf(root),
            ));
        }
        if let Some(light_up) = shows {
            let at = Transform::from_xyz(x, props::DISC_SIZE.y / 2.0, rings_at).with_rotation(
                props::disc_shows(*light_up) * Quat::from_rotation_y(0.4 * i as f32),
            );
            let whole = props::spawn(&mut commands, &server, props::DISC, &disc, at);
            commands.entity(whole).insert(ChildOf(root));
        }
    }

    // The dice, each showing its number, one to six.
    let die = [("body", look::die()), ("pips", look::pips())]
        .map(|(part, spec)| (part, materials.add(spec.build(&server))));
    for number in 1..=6u8 {
        let x = (number as f32 - 3.5) * (props::DIE_SIDE + 0.012);
        let at = Transform::from_xyz(x, props::DIE_SIDE / 2.0, dice_at).with_rotation(
            Quat::from_rotation_y(0.35 * (number as f32 - 3.0)) * props::die_showing(number),
        );
        let whole = props::spawn(&mut commands, &server, props::DIE, &die, at);
        commands.entity(whole).insert(ChildOf(root));
    }

    // The cards: a few of the usual deck face up, one face down, and a few in a pile.
    let mut side = |file: &Option<PathBuf>, without: MaterialSpec| {
        let mut material = without.build(&server);
        if let Some(file) = file {
            match picture(file) {
                Ok(image) => {
                    material.base_color = Color::WHITE;
                    material.base_color_texture = Some(images.add(image));
                }
                Err(e) => error!("{}: {e}", file.display()),
            }
        }
        materials.add(material)
    };
    // A picture given for the faces is on every one of them.
    let faces = FACES.map(|(suit, rank)| {
        let own = props::standard_face(suit, rank).filter(|_| pictures.face.is_none());
        side(
            &pictures.face,
            own.map_or_else(look::paper, |file| look::card_side(&file)),
        )
    });
    let back = side(&pictures.back, look::card_side(look::CARD_BACK));
    let edge = materials.add(look::paper().build(&server));
    let step = props::CARD_SIZE.x + 0.012;
    let count = FACES.len() + 2;
    let lie = |place: usize, layer: usize, yaw: f32, face_up: bool| {
        let x = (place as f32 - (count as f32 - 1.0) / 2.0) * step;
        let y = props::CARD_SIZE.z * (layer as f32 + 0.5);
        Transform::from_xyz(x, y, cards_at)
            .with_rotation(Quat::from_rotation_y(yaw) * props::card_lies(face_up))
    };
    let mut cards: Vec<(usize, Transform)> = (0..FACES.len())
        .map(|i| (i, lie(i, 0, 0.08 - 0.05 * i as f32, true)))
        .collect();
    cards.push((0, lie(FACES.len(), 0, -0.05, false)));
    cards.extend((0..10).map(|layer| {
        let yaw = 0.02 * (layer % 3) as f32;
        (layer % FACES.len(), lie(FACES.len() + 1, layer, yaw, false))
    }));
    for (face, at) in cards {
        let card = [
            ("face", faces[face].clone()),
            ("back", back.clone()),
            ("edge", edge.clone()),
        ];
        let whole = props::spawn(&mut commands, &server, props::CARD, &card, at);
        commands.entity(whole).insert(ChildOf(root));
    }

    // The camera frames it as it frames a board.
    let widest = ROW.min(pieces.len()) as f32 * ACROSS;
    built.write(BoardBuilt {
        size: Vec2::new(widest.max(count as f32 * step), depth + front),
        middle: Vec2::ZERO,
    });
}

/// The row of discs and rings: for each place, a ring (light or dark) or none, and a disc
/// lying there showing its light side or its dark, or none.
fn discs_and_rings() -> [(Option<bool>, Option<bool>); 6] {
    [
        (Some(true), Some(true)),
        (Some(false), Some(false)),
        (None, Some(true)),
        (None, Some(false)),
        (Some(true), None),
        (Some(false), None),
    ]
}

/// A picture from a file outside the client, to try on a card: any the client can read.
fn picture(file: &Path) -> Result<Image, String> {
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "svg" {
        return Err(
            "an SVG is not a picture the client reads: make it a PNG or a JPEG first".into(),
        );
    }
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    Image::from_buffer(
        &bytes,
        ImageType::Extension(&extension),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|e| e.to_string())
}

/// Nothing but the table is in the picture: the interface (the login screen, at start) stands
/// aside.
fn clear_the_view(mut roots: Query<&mut Visibility, (With<Node>, Without<ChildOf>)>) {
    for mut visibility in &mut roots {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_model_is_shown_once_in_each_material_it_is_given() {
        let all = Appearances::built_in();
        let shown = pieces(&all);
        for look in &all.0 {
            for piece in look.pieces.values() {
                assert!(
                    shown.iter().any(|s| s.model == piece.model),
                    "{}",
                    piece.model
                );
            }
        }
        for (i, a) in shown.iter().enumerate() {
            for b in &shown[i + 1..] {
                assert!(
                    a.model != b.model || a.material != b.material,
                    "{} twice",
                    a.model
                );
            }
        }
        // A stone is in several games' looks: slate and shell for those played with them (shown
        // once each, however many games), Mycelium's jade, and Blooms' four minerals.
        let stones = shown
            .iter()
            .filter(|s| s.model == look::PIECE_MODEL)
            .count();
        assert_eq!(stones, 7);
    }

    #[test]
    fn the_rows_go_back_from_the_front_each_centred_and_nothing_touches() {
        let all = places(2 * ROW + 3);
        assert_eq!(all.len(), 2 * ROW + 3);
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(a.distance(*b) > 0.03, "{a} and {b}");
            }
        }
        for row in all.chunks(ROW) {
            let middle = row.iter().map(|p| p.x).sum::<f32>() / row.len() as f32;
            assert!(middle.abs() < 1e-5, "a row of {} is centred", row.len());
            assert!(row.iter().all(|p| p.y == row[0].y));
        }
        assert!(all[0].y > all[ROW].y && all[ROW].y > all[2 * ROW].y);
        assert!(places(0).is_empty());
    }

    #[test]
    fn a_picture_that_cannot_be_read_says_why() {
        let svg = picture(Path::new("face.svg")).unwrap_err();
        assert!(svg.contains("SVG"), "{svg}");
        assert!(picture(Path::new("/no/such/file.png")).is_err());
    }
}
