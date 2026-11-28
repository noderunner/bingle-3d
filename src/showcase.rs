//! `--showcase`: every model the client has for a piece, in each material its games give it,
//! with two-sided discs and their rings, the dice, a chess clock, a few playing cards of the
//! usual deck, and the tallest piece passing between two portals over and over (`portal.rs`),
//! laid out on the table with nothing
//! else: to look at an asset in the client's own light, as the asset pipeline asks before one
//! is called done (`assets-src/README.md`). A face is seen from one side here, as nowhere in
//! Blender, so this is where a card mapped or wound the wrong way shows.
//!
//! The pieces are found in the appearance files, so a new one is here by itself, and no game is
//! named. `--card-face` and `--card-back` try a picture (a JPEG or a PNG) on a card's sides.
//!
//! In a run a script limits (`--frames`, `--screenshot`), time goes on by a thirtieth of a second
//! each frame whatever the machine (`TimeUpdateStrategy::ManualDuration`), so a picture at a frame
//! shows the same moment of the passage everywhere: the first starts at frame 60
//! (`PASSAGE_START`), and each lasts `Passage::length` and a second more.

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use crate::appearance::{Appearances, Stands};
use crate::board::BoardBuilt;
use crate::look::{self, MaterialSpec};
use crate::portal::{self, Passage, Portal};
use crate::scene::BloomOn;
use crate::{assets, chess_clock, props};

pub struct ShowcasePlugin {
    /// Pictures to try on the cards' sides, from files outside the client.
    pub card_face: Option<PathBuf>,
    pub card_back: Option<PathBuf>,
    /// Whether a script limits the run: time then goes on by a frame's worth each frame.
    pub scripted: bool,
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
        .add_systems(Update, (clear_the_view, pass_on));
        if self.scripted {
            app.insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
        }
    }
}

/// How far time goes on each frame of a run a script limits.
const FRAME: std::time::Duration = std::time::Duration::from_nanos(1_000_000_000 / 30);

/// When the first passage starts, in seconds (frame 60 of a run a script limits), and the
/// pause after each before the next.
pub const PASSAGE_START: f32 = 2.0;
const PASSAGE_PAUSE: f32 = 1.0;
/// The passage: coming down onto the first portal, as a piece moved does.
const PASSAGE: Passage = Passage { arrive: 0.28 };

/// The plank the passage goes through: as thick as a board, two squares on it as far apart as
/// four of a board's, each of a board's size.
const PLANK_HEIGHT: f32 = crate::board::BOARD_HEIGHT;
const SQUARE: f32 = crate::board::PITCH;
const SQUARES_APART: f32 = 4.0 * SQUARE;
const PLANK: Vec2 = Vec2::new(SQUARES_APART + 2.0 * SQUARE, 2.0 * SQUARE);

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
    /// Its model's parts after the first that are given a material of their own, by the
    /// piece's look or the seat's (`Appearance::part_material`); any other is of `material`.
    pub parts: Vec<(String, MaterialSpec)>,
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
                let parts = match &sides {
                    Some(_) => Vec::new(),
                    None => assets::model_parts(&piece.model)
                        .into_iter()
                        .skip(1)
                        .filter_map(|part| {
                            let spec = look.part_material(piece, word, &part)?.clone();
                            Some((part, spec))
                        })
                        .collect(),
                };
                out.push(Shown {
                    model: piece.model.clone(),
                    material: material.clone(),
                    stands: piece.stands,
                    sides,
                    parts,
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

/// The piece passing between the showcase's portals: where it stands on each side, how far it
/// goes down, and where the portals open (z, under the root).
#[derive(Component)]
struct Passing {
    first: Transform,
    rest: Transform,
    depth: f32,
    square: f32,
    root: Entity,
}

/// Moves the piece along its passage, again and again, and opens the portals for each.
fn pass_on(
    mut commands: Commands,
    time: Res<Time>,
    mut pieces: Query<(&Passing, &mut Transform, &mut Visibility)>,
    mut began: Local<Option<i64>>,
) {
    let period = PASSAGE.length() + PASSAGE_PAUSE;
    let since = time.elapsed_secs() - PASSAGE_START;
    let which = (since / period).floor() as i64;
    let secs = since - which as f32 * period;
    for (passing, mut t, mut visibility) in &mut pieces {
        // Before the first, it stands where it starts.
        let stage = if since < 0.0 {
            portal::Stage::Sinking(0.0)
        } else {
            PASSAGE.stage(secs)
        };
        let drop = crate::board::DROP_HEIGHT;
        let (at, seen) = portal::pose(stage, passing.first, passing.rest, passing.depth, drop);
        *t = at;
        visibility.set_if_neq(if seen {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if since >= 0.0 && *began != Some(which) {
            *began = Some(which);
            let corners = crate::slab::cell_corners(crate::board::CellShape::Square, SQUARE);
            let y = PLANK_HEIGHT + portal::LIFT;
            for ((opens, stays), x) in PASSAGE
                .portals()
                .into_iter()
                .zip([-SQUARES_APART / 2.0, SQUARES_APART / 2.0])
            {
                let mut portal = Portal::new(corners.clone(), opens, stays);
                portal.secs = secs;
                commands.spawn((
                    Name::new("Portal"),
                    portal,
                    Transform::from_xyz(x, y, passing.square),
                    ChildOf(passing.root),
                ));
            }
        }
    }
}

fn lay_out(
    mut commands: Commands,
    server: Res<AssetServer>,
    all: Res<Appearances>,
    (pictures, bloom): (Res<Pictures>, Res<BloomOn>),
    (mut materials, mut meshes): (ResMut<Assets<StandardMaterial>>, ResMut<Assets<Mesh>>),
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
    let plank_at = cards_at + props::CARD_SIZE.y / 2.0 + GAP + PLANK.y / 2.0;
    let front = plank_at + PLANK.y / 2.0;
    // Behind the pieces: the chess clock.
    let clock_at = -(depth + BACK / 2.0 + GAP + chess_clock::DEPTH / 2.0);
    let behind = -clock_at + chess_clock::DEPTH / 2.0;
    let shift = (behind - front) / 2.0;
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
        // Every part of its model: those given a material of their own in it, the rest in
        // the piece's.
        let whole_of = materials.add(shown.material.build(&server));
        let names = assets::model_parts(&shown.model);
        let parts: Vec<(&str, Handle<StandardMaterial>)> = names
            .iter()
            .map(|name| {
                let own = shown.parts.iter().find(|(part, _)| part == name);
                let material = match own {
                    Some((_, spec)) => materials.add(spec.build(&server)),
                    None => whole_of.clone(),
                };
                (name.as_str(), material)
            })
            .collect();
        let whole = props::spawn(&mut commands, &server, &shown.model, &parts, at);
        commands
            .entity(whole)
            .insert((Name::new(shown.model.clone()), ChildOf(root)));
    }

    // The discs and rings, in their generic looks: a ring of each with a disc lying in it
    // showing its own side, discs showing either side, and the rings alone. A light ring's
    // trim is gold, a dark one's silver.
    let light = materials.add(look::disc_light().build(&server));
    let dark = materials.add(look::disc_dark().build(&server));
    let gold = materials.add(look::trim_gold().build(&server));
    let silver = materials.add(look::trim_silver().build(&server));
    let disc = [("light", light.clone()), ("dark", dark.clone())];
    let row = discs_and_rings();
    for (i, (ring, shows)) in row.iter().enumerate() {
        let x = (i as f32 - (row.len() as f32 - 1.0) / 2.0) * (props::RING_SIZE.x + 0.01);
        if let Some(light_ring) = ring {
            let (piece, trim) = if *light_ring {
                (&light, &gold)
            } else {
                (&dark, &silver)
            };
            let parts = [("piece", piece.clone()), ("trim", trim.clone())];
            let at = Transform::from_xyz(x, props::RING_SIZE.z / 2.0, rings_at);
            let whole = props::spawn(&mut commands, &server, props::RING, &parts, at);
            commands.entity(whole).insert(ChildOf(root));
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

    // The passage: a plank, two squares on it, and the tallest piece going from the left one
    // to the right one through a portal on each (`pass_on`).
    let wood = materials.add(look::tray().on_board(look::BOARD_WOODS[0]).build(&server));
    commands.spawn((
        Name::new("Plank"),
        Mesh3d(meshes.add(Cuboid::new(PLANK.x, PLANK_HEIGHT, PLANK.y))),
        MeshMaterial3d(wood),
        Transform::from_xyz(0.0, PLANK_HEIGHT / 2.0, plank_at),
        ChildOf(root),
    ));
    let tallest = pieces
        .iter()
        .filter(|s| s.sides.is_none())
        .filter_map(|s| Some((s, assets::model_span(&s.model)?)))
        .max_by(|(_, (a, b)), (_, (c, d))| (b - a).total_cmp(&(d - c)));
    if let Some((shown, (low, high))) = tallest {
        let material = materials.add(shown.material.build(&server));
        let names = assets::model_parts(&shown.model);
        let parts: Vec<(&str, Handle<StandardMaterial>)> = names
            .iter()
            .map(|name| {
                let own = shown.parts.iter().find(|(part, _)| part == name);
                let m = match own {
                    Some((_, spec)) => materials.add(spec.build(&server)),
                    None => material.clone(),
                };
                (name.as_str(), m)
            })
            .collect();
        let first = Transform::from_xyz(-SQUARES_APART / 2.0, PLANK_HEIGHT - low, plank_at)
            .with_rotation(Quat::from_rotation_y(0.6));
        let mut rest = first;
        rest.translation.x += SQUARES_APART;
        let piece = props::spawn(&mut commands, &server, &shown.model, &parts, first);
        commands.entity(piece).insert((
            Name::new("Passing"),
            ChildOf(root),
            Passing {
                first,
                rest,
                depth: high - low + 0.001,
                square: plank_at,
                root,
            },
        ));
    }

    // The chess clock, facing the player: one face with two hours left and its time running,
    // the other with nothing left and its flag fallen.
    let kit = chess_clock::Kit::new(&mut materials, &server, bloom.0);
    let frame = materials.add(look::tray().on_board(look::BOARD_WOODS[0]).build(&server));
    let clock = chess_clock::spawn(
        &mut commands,
        &server,
        &kit,
        &frame,
        Transform::from_xyz(0.0, 0.0, clock_at),
        chess_clock::Showing {
            left: 7200.0,
            right: 0.0,
            running: Some(chess_clock::End::Left),
        },
    );
    commands.entity(clock).insert(ChildOf(root));

    // The camera frames it as it frames a board.
    let widest = ROW.min(pieces.len()) as f32 * ACROSS;
    built.write(BoardBuilt {
        size: Vec2::new(widest.max(count as f32 * step), behind + front),
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
