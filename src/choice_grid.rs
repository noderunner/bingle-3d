//! A board of choices, for a game whose look has `[grid]`: the view's lines of boxes
//! (`ext.view.<view>`), lying on the table as a board of wood with a tile for each box, its
//! picture on it (the look's, by the view's name for it), and each line's own picture at its
//! head. A box a seat crossed has that seat's marker standing on it, small
//! (`seat_markers::Marker`), carried there from the marker in front of the seat; a box out of
//! the game is covered; the box left on a line is rimmed. A click on a box makes the look's
//! choice for it (the look's `choice`, the line's id, `-`, the box's id). A box that may be
//! crossed is lit while the pointer is over it (all of them, with the legal moves lit), and the
//! pointer's hand points at it. The pointer over a box, or a line's head, shows a card of
//! words beside it: what the line decides and what that box means, as the view says
//! (`label`, `hint`), so a picture need not be guessed at.
//!
//! `read`, `places` and `words` have no Bevy in them.

use std::collections::HashMap;

use bevy::picking::Pickable;
use bevy::picking::events::PointerClick;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde_json::Value;

use crate::appearance::{Appearances, GridLook};
use crate::board::{BoardWood, CellShape, ShowLegalMoves};
use crate::browser::colors;
use crate::card_table::CLOTH;
use crate::cursor::{Feel, Sense, TableFeel, Under};
use crate::fonts::UiFonts;
use crate::hand::{Carry, Hands, Rest, Route};
use crate::highlight::{Lit, Outline};
use crate::look::{self, MaterialSpec};
use crate::net::api::{Gesture, Position};
use crate::net::{Net, WatchStatus};
use crate::play::{self, Stage};
use crate::seat_markers::{self, Marker};
use crate::table::Table;

pub struct ChoiceGridPlugin;

impl Plugin for ChoiceGridPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Appearances>()
            .init_resource::<BoardWood>()
            .init_resource::<ShowLegalMoves>()
            .init_resource::<Under>()
            .init_resource::<TableFeel>()
            .add_observer(on_click)
            .add_systems(Startup, spawn_card)
            .add_systems(Update, (sense.in_set(Sense), explain.after(Sense)))
            .add_systems(
                Update,
                (
                    sync.run_if(
                        resource_changed::<WatchStatus>.or_eager(resource_changed::<BoardWood>),
                    ),
                    light.run_if(
                        resource_changed::<WatchStatus>
                            .or_eager(resource_changed::<ShowLegalMoves>)
                            .or_eager(resource_changed::<Under>),
                    ),
                )
                    .chain()
                    .before(Hands),
            );
    }
}

/// A box's side when the look does not say, and how far apart boxes are against it.
const BOX: f32 = 0.034;
const PITCH: f32 = 1.16;
/// How thick the board is, and how far its wood reaches beyond the outermost boxes.
const BOARD: f32 = 0.006;
const MARGIN: f32 = 0.008;
/// How thick a box's tile is; how far its picture stops short of its edge; how far the rim of
/// the box left on a line shows round it.
const TILE: f32 = 0.0016;
const PICTURE: f32 = 0.94;
const RIM: f32 = 0.0018;
/// A line's own picture, at its head, against a box.
const HEAD: f32 = 0.8;
/// How much higher than the hand carries a piece a marker goes on its way to its box.
const OVER: f32 = 0.03;

/// How a box of the grid stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Open,
    /// Crossed off, by this seat.
    Crossed(u32),
    /// Out of the game.
    Covered,
    /// The one left on its line.
    Chosen,
}

/// A box of the grid: its id, the view's name for its picture, what the view calls it and
/// says it means, and how it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridBox {
    pub id: String,
    pub icon: String,
    pub label: String,
    pub hint: String,
    pub state: State,
}

/// A line of the grid, with what the view calls it and says it decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub id: String,
    pub icon: String,
    pub label: String,
    pub hint: String,
    pub boxes: Vec<GridBox>,
}

/// What is said of a box when the pointer is over it, or of its line at the line's head
/// (`index` none): a heading (the line, and the box) and what it means: the box's own hint,
/// else its line's. None when the view says nothing of it.
pub fn words(lines: &[Line], line: usize, index: Option<usize>) -> Option<(String, String)> {
    let of = lines.get(line)?;
    let (heading, meaning) = match index {
        None => (of.label.clone(), of.hint.as_str()),
        Some(i) => {
            let b = of.boxes.get(i)?;
            let heading = match (of.label.is_empty(), b.label.is_empty()) {
                (false, false) => format!("{}: {}", of.label, b.label),
                (true, _) => b.label.clone(),
                (_, true) => of.label.clone(),
            };
            let meaning = if b.hint.is_empty() { &of.hint } else { &b.hint };
            (heading, meaning.as_str())
        }
    };
    (!heading.is_empty() || !meaning.is_empty()).then(|| (heading, meaning.to_string()))
}

/// The view's grid: its lines, in order. Empty when the view has none.
pub fn read(grid: &Value) -> Vec<Line> {
    let words = |v: &Value| v.as_str().unwrap_or_default().to_string();
    grid["lines"]
        .as_array()
        .map(|lines| {
            lines
                .iter()
                .map(|line| Line {
                    id: words(&line["id"]),
                    icon: words(&line["icon"]),
                    label: words(&line["label"]),
                    hint: words(&line["hint"]),
                    boxes: line["boxes"]
                        .as_array()
                        .map(|boxes| {
                            boxes
                                .iter()
                                .map(|b| GridBox {
                                    id: words(&b["id"]),
                                    icon: words(&b["icon"]),
                                    label: words(&b["label"]),
                                    hint: words(&b["hint"]),
                                    state: match (b["state"].as_str(), b["by"].as_u64()) {
                                        (Some("crossed"), Some(by)) => State::Crossed(by as u32),
                                        (Some("covered"), _) => State::Covered,
                                        (Some("chosen"), _) => State::Chosen,
                                        _ => State::Open,
                                    },
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Where the grid's parts lie about its middle (x across, z towards the player), for boxes
/// `pitch` apart: how wide and deep the boxes reach, each line's head, and each line's boxes.
/// The first line is the farthest, and a line's head is at its left.
pub fn places(lines: &[Line], pitch: f32) -> (Vec2, Vec<Vec2>, Vec<Vec<Vec2>>) {
    let columns = 1 + lines.iter().map(|l| l.boxes.len()).max().unwrap_or(0);
    let x = |c: usize| (c as f32 - (columns as f32 - 1.0) / 2.0) * pitch;
    let z = |r: usize| (r as f32 - (lines.len() as f32 - 1.0) / 2.0) * pitch;
    let heads = (0..lines.len()).map(|r| Vec2::new(x(0), z(r))).collect();
    let boxes = lines
        .iter()
        .enumerate()
        .map(|(r, line)| {
            (0..line.boxes.len())
                .map(|c| Vec2::new(x(c + 1), z(r)))
                .collect()
        })
        .collect();
    (
        Vec2::new(columns as f32, lines.len() as f32) * pitch,
        heads,
        boxes,
    )
}

/// The choice a click on a box makes.
pub fn choice(look: &GridLook, line: &str, id: &str) -> Gesture {
    Gesture {
        kind: "action".into(),
        value: format!("{}{line}-{id}", look.choice),
    }
}

/// The grid of the game watched: the lines and boxes it was built for, and in which wood.
#[derive(Component)]
struct GridRoot {
    room_code: String,
    shape: Vec<(String, Vec<String>)>,
    wood: &'static str,
}

/// A box's tile: its line and its place in it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct Tile {
    line: usize,
    index: usize,
}

/// The tile at a line's head, with the line's own picture.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct Head {
    line: usize,
}

/// The choice a click on a box's tile makes, when the move may make it.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct Pick(Gesture);

/// The card of words shown beside the pointer, and its two lines.
#[derive(Component)]
struct WordsCard;

#[derive(Component)]
struct Heading;

#[derive(Component)]
struct Meaning;

/// The rim of a box, seen when it is the one left on its line.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct Rim {
    line: usize,
    index: usize,
}

/// The marker on a box: of the seat that crossed it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct OnBox {
    line: usize,
    index: usize,
}

fn shape(lines: &[Line]) -> Vec<(String, Vec<String>)> {
    lines
        .iter()
        .map(|l| {
            (
                l.id.clone(),
                l.boxes
                    .iter()
                    // A box out of the game is drawn another way: built again if that changes.
                    .map(|b| format!("{}{}", b.id, u8::from(b.state == State::Covered)))
                    .collect(),
            )
        })
        .collect()
}

/// The look's grid and the view's, for the game on the table.
fn grid_of<'a>(
    table: &Table,
    position: &Position,
    appearances: &'a Appearances,
) -> Option<(&'a GridLook, Vec<Line>)> {
    let file = appearances.for_game(&table.info.id)?;
    let look = file.grid.as_ref()?;
    let lines = read(&position.ext["view"][&look.view]);
    (!lines.is_empty()).then_some((look, lines))
}

#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    appearances: Res<Appearances>,
    wood: Res<BoardWood>,
    server: Res<AssetServer>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    roots: Query<(Entity, &GridRoot, &Transform)>,
    mut rims: Query<(&Rim, &mut Visibility)>,
    on_boxes: Query<(Entity, &OnBox, &Marker)>,
) {
    let found = watch
        .table
        .as_ref()
        .and_then(|t| Some((t, grid_of(t, watch.shown()?, &appearances)?)));
    let Some((table, (look, lines))) = found else {
        for (root, ..) in &roots {
            commands.entity(root).despawn();
        }
        return;
    };
    let side = look.box_size.unwrap_or(BOX);
    let pitch = side * PITCH;
    let (_, _, at_boxes) = places(&lines, pitch);
    let wanted = shape(&lines);
    let current = roots
        .iter()
        .find(|(_, r, _)| r.room_code == table.room_code && r.shape == wanted && r.wood == wood.0);
    let (root, origin, fresh) = match current {
        Some((root, _, t)) => (root, t.translation, false),
        None => {
            for (old, ..) in &roots {
                commands.entity(old).despawn();
            }
            let file = appearances.for_game(&table.info.id);
            let named = |name: Option<&String>| file.and_then(|f| f.material(name)).cloned();
            let root = build(
                &mut commands,
                table,
                look,
                &lines,
                wood.0,
                (named(look.material.as_ref()), named(look.rim.as_ref())),
                &server,
                &mut meshes,
                &mut materials,
            );
            (root, Vec3::new(look.at[0], CLOTH, look.at[1]), true)
        }
    };
    for (rim, mut seen) in &mut rims {
        let chosen = lines
            .get(rim.line)
            .and_then(|l| l.boxes.get(rim.index))
            .is_some_and(|b| b.state == State::Chosen);
        let wanted = if chosen {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        seen.set_if_neq(wanted);
    }
    // The markers on the boxes crossed: one put on each newly crossed, carried there from its
    // seat's own, and taken off any no longer crossed.
    let crossed = |on: &OnBox| match lines.get(on.line).and_then(|l| l.boxes.get(on.index)) {
        Some(GridBox {
            state: State::Crossed(by),
            ..
        }) => Some(*by),
        _ => None,
    };
    let mut have: HashMap<(usize, usize), u32> = HashMap::new();
    for (entity, on, marker) in &on_boxes {
        if !fresh && crossed(on) == Some(marker.seat) {
            have.insert((on.line, on.index), marker.seat);
        } else if !fresh {
            commands.entity(entity).despawn();
        }
    }
    let markers = appearances
        .for_game(&table.info.id)
        .and_then(|f| f.markers.as_ref());
    let Some(markers) = markers else {
        return;
    };
    let small = seat_markers::small(markers);
    for (r, line) in lines.iter().enumerate() {
        for (c, b) in line.boxes.iter().enumerate() {
            let State::Crossed(by) = b.state else {
                continue;
            };
            if have.get(&(r, c)) == Some(&by) {
                continue;
            }
            let at = at_boxes[r][c];
            // Looking to the player's side, so who crossed it is seen from where they sit.
            let to = Transform::from_xyz(at.x, BOARD + TILE, at.y)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
                .with_scale(Vec3::splat(small));
            let from = seat_markers::place_of(table, markers, by)
                .filter(|_| !fresh)
                .map(|t| Transform {
                    translation: t.translation - origin,
                    scale: Vec3::splat(small),
                    ..t
                });
            let mut marker = commands.spawn((
                Name::new(format!("Marker on {} {}", line.id, b.id)),
                OnBox { line: r, index: c },
                Marker {
                    seat: by,
                    small: true,
                },
                from.unwrap_or(to),
                Rest(to),
                Visibility::default(),
                ChildOf(root),
            ));
            if from.is_some() {
                marker.insert((
                    Carry::Put(to),
                    Route {
                        over: OVER,
                        ..Route::default()
                    },
                ));
            }
        }
    }
}

/// The board, each line's head and each box's tile with its picture and its rim.
#[allow(clippy::too_many_arguments)]
fn build(
    commands: &mut Commands,
    table: &Table,
    look: &GridLook,
    lines: &[Line],
    wood: &'static str,
    (board, rim): (Option<MaterialSpec>, Option<MaterialSpec>),
    server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    let side = look.box_size.unwrap_or(BOX);
    let (reach, heads, boxes) = places(lines, side * PITCH);
    let at = Vec3::new(look.at[0], CLOTH, look.at[1]);
    let root = commands
        .spawn((
            Name::new("Choice grid"),
            GridRoot {
                room_code: table.room_code.clone(),
                shape: shape(lines),
                wood,
            },
            Transform::from_translation(at),
            Visibility::default(),
        ))
        .id();
    let size = reach - Vec2::splat(side * (PITCH - 1.0)) + Vec2::splat(2.0 * MARGIN);
    let board_spec = board.unwrap_or(MaterialSpec {
        wood: Some(look::BOARD.into()),
        ..default()
    });
    let middle = Vec3::Y * BOARD / 2.0;
    commands.spawn((
        Name::new("Grid board"),
        Mesh3d(meshes.add(look::world_uvs(
            Cuboid::new(size.x, BOARD, size.y).mesh().build(),
            at + middle,
        ))),
        MeshMaterial3d(materials.add(board_spec.on_board(wood).build(server))),
        Transform::from_translation(middle),
        Pickable::IGNORE,
        ChildOf(root),
    ));
    let paper = materials.add(look::paper().build(server));
    let rim = materials.add(
        rim.unwrap_or(MaterialSpec {
            tint: Some([0.83, 0.62, 0.22]),
            roughness: Some(0.35),
            glow: Some([0.10, 0.065, 0.012]),
            ..default()
        })
        .build(server),
    );
    let tile = meshes.add(Cuboid::new(side, TILE, side));
    let rim_mesh = meshes.add(Cuboid::new(side + 2.0 * RIM, TILE * 0.8, side + 2.0 * RIM));
    let picture = meshes.add(
        Plane3d::default()
            .mesh()
            .size(side * PICTURE, side * PICTURE),
    );
    let mut pictures: HashMap<String, Handle<StandardMaterial>> = HashMap::new();
    let mut picture_of = |file: Option<&String>| -> Option<Handle<StandardMaterial>> {
        let file = file?;
        Some(
            pictures
                .entry(file.clone())
                .or_insert_with(|| materials.add(look::card_side(file).build(server)))
                .clone(),
        )
    };
    let mut lay = |commands: &mut Commands, at: Vec2, scale: f32, file: Option<&String>| {
        let id = commands
            .spawn((
                Mesh3d(tile.clone()),
                MeshMaterial3d(paper.clone()),
                Transform::from_xyz(at.x, BOARD + TILE / 2.0 * scale, at.y)
                    .with_scale(Vec3::splat(scale)),
                // The pointer finds it: what it means is said beside it.
                Pickable::default(),
                ChildOf(root),
            ))
            .id();
        if let Some(material) = picture_of(file) {
            commands.spawn((
                Mesh3d(picture.clone()),
                MeshMaterial3d(material),
                Transform::from_xyz(0.0, TILE / 2.0 + 0.0001, 0.0),
                Pickable::IGNORE,
                ChildOf(id),
            ));
        }
        id
    };
    for (r, line) in lines.iter().enumerate() {
        let head = lay(commands, heads[r], HEAD, look.pictures.get(&line.icon));
        commands
            .entity(head)
            .insert((Name::new(format!("Line {}", line.id)), Head { line: r }));
        for (c, b) in line.boxes.iter().enumerate() {
            let covered = b.state == State::Covered;
            let file = if covered {
                look.covered.as_ref()
            } else {
                look.pictures.get(&b.icon)
            };
            let id = lay(commands, boxes[r][c], 1.0, file);
            let mut tile = commands.entity(id);
            tile.insert((
                Name::new(format!("Box {} {}", line.id, b.id)),
                Tile { line: r, index: c },
                Outline::space(
                    crate::slab::cell_corners(CellShape::Square, side),
                    TILE / 2.0,
                ),
            ));
            if !covered {
                tile.insert(Pick(choice(look, &line.id, &b.id)));
            }
            commands.spawn((
                Rim { line: r, index: c },
                Mesh3d(rim_mesh.clone()),
                MeshMaterial3d(rim.clone()),
                Transform::from_xyz(boxes[r][c].x, BOARD + TILE * 0.4, boxes[r][c].y),
                if b.state == State::Chosen {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
                Pickable::IGNORE,
                ChildOf(root),
            ));
        }
    }
    root
}

/// The choices the move being built may make now.
fn legal(watch: &WatchStatus) -> Vec<Gesture> {
    watch
        .turn
        .as_ref()
        .filter(|t| t.stage == Stage::Choosing)
        .map(|t| t.choices())
        .unwrap_or_default()
}

/// The box that may be crossed is lit while the pointer is over it, and every one of them
/// with the legal moves lit.
fn light(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    show: Res<ShowLegalMoves>,
    under: Res<Under>,
    tiles: Query<(Entity, &Pick, Has<Lit>), With<Tile>>,
) {
    let choices = legal(&watch);
    let pointed = (!under.ui)
        .then(|| under.chain.iter().find(|e| tiles.contains(**e)).copied())
        .flatten();
    for (entity, pick, lit) in &tiles {
        let wanted = choices.contains(&pick.0) && (show.0 || pointed == Some(entity));
        match (wanted, lit) {
            (true, false) => {
                commands.entity(entity).insert(Lit);
            }
            (false, true) => {
                commands.entity(entity).remove::<Lit>();
            }
            _ => {}
        }
    }
}

fn on_click(
    click: On<PointerClick>,
    tiles: Query<&Pick, With<Tile>>,
    net: Res<Net>,
    mut watch: ResMut<WatchStatus>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    if let Ok(pick) = tiles.get(click.entity)
        && legal(&watch).contains(&pick.0)
    {
        play::gesture(&net, &mut watch, &pick.0);
    }
}

/// Says that a click on a box that may be crossed chooses it: the hand points (`cursor.rs`).
fn sense(
    under: Res<Under>,
    tiles: Query<&Pick, With<Tile>>,
    watch: Res<WatchStatus>,
    mut table: ResMut<TableFeel>,
) {
    let Some(pick) = under.chain.iter().find_map(|e| tiles.get(*e).ok()) else {
        return;
    };
    let feel = if legal(&watch).contains(&pick.0) {
        Feel::Point
    } else {
        Feel::Open
    };
    table.set_if_neq(TableFeel(feel));
}

/// How wide the card of words is, how far from the pointer, and how near the window's edge
/// it may come, in logical pixels.
const CARD_WIDTH: f32 = 300.0;
const CARD_OFF: Vec2 = Vec2::new(22.0, 18.0);
const CARD_EDGE: f32 = 8.0;
/// About how tall the card is, for keeping it inside the window when its words are not yet
/// laid out.
const CARD_TALL: f32 = 120.0;

/// The card of words, hidden until the pointer is over something the view explains.
fn spawn_card(mut commands: Commands, fonts: Option<Res<UiFonts>>) {
    let Some(fonts) = fonts else {
        return;
    };
    commands.spawn((
        Name::new("Grid words"),
        WordsCard,
        crate::login::MainOnly,
        // Only words: clicks and the wheel go through to the table.
        Pickable::IGNORE,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            width: px(CARD_WIDTH),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            padding: UiRect::axes(px(12), px(10)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(colors::PANEL),
        BorderColor::all(colors::BORDER),
        GlobalZIndex(40),
        children![
            (
                Heading,
                Text::new(""),
                fonts.semibold(15.0),
                TextColor(colors::TEXT),
                Pickable::IGNORE,
            ),
            (
                Meaning,
                Text::new(""),
                fonts.regular(13.0),
                TextColor(colors::TEXT),
                Pickable::IGNORE,
            ),
        ],
    ));
}

/// Where the card's top left corner goes for a pointer at `pointer` in a window of `window`
/// logical pixels: below and to the right of the pointer, or on its other side where that
/// would leave the window.
pub fn card_at(pointer: Vec2, window: Vec2, tall: f32) -> Vec2 {
    let fits = |at: f32, size: f32, room: f32| at + size + CARD_EDGE <= room;
    let x = if fits(pointer.x + CARD_OFF.x, CARD_WIDTH, window.x) {
        pointer.x + CARD_OFF.x
    } else {
        pointer.x - CARD_OFF.x - CARD_WIDTH
    };
    let y = if fits(pointer.y + CARD_OFF.y, tall, window.y) {
        pointer.y + CARD_OFF.y
    } else {
        pointer.y - CARD_OFF.y - tall
    };
    Vec2::new(x.max(CARD_EDGE), y.max(CARD_EDGE))
}

/// What words are shown of: a box's tile, or a line's head.
type Explained = (Option<&'static Tile>, Option<&'static Head>);
type OnGrid = Or<(With<Tile>, With<Head>)>;

/// Shows the card of words beside the pointer while it is over a box or a line's head the
/// view says something of.
#[allow(clippy::too_many_arguments)]
fn explain(
    under: Res<Under>,
    watch: Res<WatchStatus>,
    appearances: Res<Appearances>,
    windows: Query<&Window, With<PrimaryWindow>>,
    tiles: Query<Explained, OnGrid>,
    mut card: Query<(&mut Node, &ComputedNode), With<WordsCard>>,
    mut heading: Query<&mut Text, (With<Heading>, Without<Meaning>)>,
    mut meaning: Query<&mut Text, (With<Meaning>, Without<Heading>)>,
) {
    let Ok((mut node, laid)) = card.single_mut() else {
        return;
    };
    let said = (!under.ui)
        .then(|| under.chain.iter().find_map(|e| tiles.get(*e).ok()))
        .flatten()
        .and_then(|(tile, head)| {
            let table = watch.table.as_ref()?;
            let (_, lines) = grid_of(table, watch.shown()?, &appearances)?;
            match (tile, head) {
                (Some(t), _) => words(&lines, t.line, Some(t.index)),
                (None, Some(h)) => words(&lines, h.line, None),
                (None, None) => None,
            }
        });
    let pointer = windows
        .single()
        .ok()
        .and_then(|w| Some((w.cursor_position()?, w.size())));
    let (Some((title, body)), Some((pointer, window))) = (said, pointer) else {
        if node.display != Display::None {
            node.display = Display::None;
        }
        return;
    };
    if let Ok(mut text) = heading.single_mut()
        && text.0 != title
    {
        text.0 = title;
    }
    if let Ok(mut text) = meaning.single_mut()
        && text.0 != body
    {
        text.0 = body;
    }
    let tall = match laid.size().y * laid.inverse_scale_factor() {
        t if t > 1.0 => t,
        _ => CARD_TALL,
    };
    let at = card_at(pointer, window, tall);
    let (left, top) = (px(at.x), px(at.y));
    if node.display != Display::Flex || node.left != left || node.top != top {
        node.display = Display::Flex;
        node.left = left;
        node.top = top;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn view() -> Value {
        json!({"lines": [
            {"id": "who", "icon": "line:who", "boxes": [
                {"id": "1", "icon": "seat:1", "state": "crossed", "by": 2},
                {"id": "2", "icon": "seat:2", "state": "chosen"},
                {"id": "3", "icon": "seat:3", "state": "covered"}]},
            {"id": "worth", "icon": "line:worth", "boxes": [
                {"id": "1", "icon": "worth:1", "state": "open"},
                {"id": "-2", "icon": "worth:-2", "state": "open"}]}
        ]})
    }

    #[test]
    fn the_views_grid_is_read_line_by_line_with_how_each_box_stands() {
        let lines = read(&view());
        assert_eq!(lines.len(), 2);
        assert_eq!(
            (lines[0].id.as_str(), lines[0].icon.as_str()),
            ("who", "line:who")
        );
        let states: Vec<State> = lines[0].boxes.iter().map(|b| b.state).collect();
        assert_eq!(states, [State::Crossed(2), State::Chosen, State::Covered]);
        assert_eq!(lines[1].boxes[1].id, "-2");
        assert_eq!(lines[1].boxes[1].state, State::Open);
        assert!(read(&Value::Null).is_empty());
        assert!(read(&json!({"lines": 3})).is_empty());
    }

    #[test]
    fn the_lines_lie_one_below_another_each_with_its_head_at_its_left() {
        let lines = read(&view());
        let (reach, heads, boxes) = places(&lines, 0.04);
        // Four columns (a head and the longest line's three boxes) and two rows.
        assert!((reach - Vec2::new(0.16, 0.08)).length() < 1e-6);
        assert!(heads[0].y < heads[1].y, "the first line is the farthest");
        assert!((heads[0].y + heads[1].y).abs() < 1e-6, "about its middle");
        assert!((heads[0].x + 0.06).abs() < 1e-6);
        assert!((boxes[0][0].x - heads[0].x - 0.04).abs() < 1e-6);
        assert!((boxes[0][2].x - 0.06).abs() < 1e-6);
        assert_eq!(boxes[1].len(), 2);
        assert_eq!(boxes[1][0].x, boxes[0][0].x, "in columns");
    }

    #[test]
    fn a_click_on_a_box_makes_the_looks_choice_for_its_line_and_itself() {
        let look = GridLook {
            choice: "strike-".into(),
            ..Default::default()
        };
        let made = choice(&look, "worth", "-2");
        assert_eq!(
            (made.kind.as_str(), made.value.as_str()),
            ("action", "strike-worth--2")
        );
    }

    /// A game of three seats with markers and a grid, as the server describes it, with the
    /// view `view`.
    fn table(seq: u64, view: serde_json::Value) -> Table {
        use serde_json::json;
        let info = json!({
            "id": "g", "name": "A game", "engine": "ruleset",
            "seats": [{"seat": 1, "color": "red", "label": "Fox"},
                      {"seat": 2, "color": "blue", "label": "Owl"},
                      {"seat": 3, "color": "green", "label": "Elk"}],
            "board": {"kind": "grid", "positions": [{"id": "0,0", "x": 0, "y": 0}]}
        });
        let state = json!({
            "state": {"cells": {}, "ext": {"view": view}},
            "status": "active", "move_seq": seq, "impulse_seat": 1
        });
        let game = json!({"game_def_id": "g", "status": "active", "seats": [
            {"seat": 1, "color": "red", "name": "Ana", "yours": true},
            {"seat": 2, "color": "blue", "name": "Ben"},
            {"seat": 3, "color": "green", "name": "Cy"}]});
        Table::new(crate::net::session::Opened {
            room_code: "AB12CD".into(),
            info: serde_json::from_value(info).unwrap(),
            game: serde_json::from_value(game).unwrap(),
            state: serde_json::from_value(state).unwrap(),
            bots: vec![],
        })
    }

    const FILE: &str = r#"
        games = ["g"]
        [grid]
        view = "grid"
        choice = "cross-"
        at = [-0.2, 0.0]
        [pieces.fox]
        model = "models/chess/knight.glb"
        [markers]
        seat = [-0.2, 0.3]
        material = "wood"
        sides = "team"
        on = "pale"
        off = "dark"
        ringed = "bonus"
        ring = "pale"
        choose = ["side-"]
        alone = "side-alone"
        [markers.pieces]
        red = "fox"
        blue = "fox"
        green = "fox"
        [materials.wood]
        [materials.pale]
        [materials.dark]
    "#;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .add_message::<crate::sounds::Cue>()
            .insert_resource(Appearances(vec![
                crate::appearance::Appearance::parse(FILE).unwrap(),
            ]))
            .insert_resource(crate::net::Net::for_test().0)
            .init_resource::<WatchStatus>();
        app
    }

    fn grid(first: serde_json::Value) -> serde_json::Value {
        json!({"grid": {"lines": [
            {"id": "who", "icon": "line:who", "boxes": [
                first,
                {"id": "2", "icon": "seat:2", "state": "open"},
                {"id": "3", "icon": "seat:3", "state": "covered"}]},
            {"id": "worth", "icon": "line:worth", "boxes": [
                {"id": "1", "icon": "worth:1", "state": "chosen"},
                {"id": "-2", "icon": "worth:-2", "state": "crossed", "by": 2}]}
        ]}})
    }

    #[test]
    fn the_grid_lies_on_the_table_with_a_marker_on_each_box_crossed_and_a_rim_on_the_one_left() {
        let mut app = app();
        app.add_plugins(ChoiceGridPlugin);
        let open = json!({"id": "1", "icon": "seat:1", "state": "open"});
        app.world_mut().resource_mut::<WatchStatus>().table = Some(table(1, grid(open)));
        app.update();
        app.update();
        let world = app.world_mut();
        // A tile for each box, and a click on any but the one out of the game makes its choice.
        assert_eq!(world.query::<&Tile>().iter(world).count(), 5);
        let mut choices: Vec<String> = world
            .query_filtered::<&Pick, With<Tile>>()
            .iter(world)
            .map(|c| c.0.value.clone())
            .collect();
        choices.sort();
        assert_eq!(
            choices,
            [
                "cross-who-1",
                "cross-who-2",
                "cross-worth--2",
                "cross-worth-1"
            ]
        );
        let rims: Vec<Rim> = world
            .query::<(&Rim, &Visibility)>()
            .iter(world)
            .filter(|(_, v)| **v != Visibility::Hidden)
            .map(|(r, _)| *r)
            .collect();
        assert_eq!(
            rims,
            [Rim { line: 1, index: 0 }],
            "the box left on its line"
        );
        // The box already crossed has its seat's marker, small, where it stands.
        let on: Vec<(OnBox, Marker, bool)> = world
            .query::<(&OnBox, &Marker, Has<Carry>)>()
            .iter(world)
            .map(|(o, m, c)| (*o, *m, c))
            .collect();
        let small = Marker {
            seat: 2,
            small: true,
        };
        assert_eq!(on, [(OnBox { line: 1, index: 1 }, small, false)]);

        // Another is crossed: its seat's marker is carried there from in front of the seat.
        let crossed = json!({"id": "1", "icon": "seat:1", "state": "crossed", "by": 3});
        app.world_mut().resource_mut::<WatchStatus>().table = Some(table(2, grid(crossed)));
        app.update();
        let world = app.world_mut();
        let new: Vec<(Transform, Rest, Marker)> = world
            .query_filtered::<(&Transform, &Rest, &Marker), With<Carry>>()
            .iter(world)
            .map(|(t, r, m)| (*t, *r, *m))
            .collect();
        assert_eq!(new.len(), 1);
        let (from, to, marker) = new[0];
        assert_eq!(
            marker,
            Marker {
                seat: 3,
                small: true
            }
        );
        assert!(
            from.translation.distance(to.0.translation) > 0.1,
            "from its seat"
        );
        assert!(
            (to.0.translation.y - BOARD - TILE).abs() < 1e-6,
            "on its box"
        );
        assert_eq!(world.query::<&OnBox>().iter(world).count(), 2);
        assert_eq!(
            world.query::<&GridRoot>().iter(world).count(),
            1,
            "the same board"
        );

        // A new round: every box open again, and the markers gone.
        let mut fresh = grid(json!({"id": "1", "icon": "seat:1", "state": "open"}));
        fresh["grid"]["lines"][1]["boxes"][1]["state"] = json!("open");
        app.world_mut().resource_mut::<WatchStatus>().table = Some(table(3, fresh));
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&OnBox>().iter(world).count(), 0);
    }

    #[test]
    fn with_the_legal_moves_lit_the_boxes_that_may_be_crossed_are() {
        let mut app = app();
        app.add_plugins(ChoiceGridPlugin);
        let open = json!({"id": "1", "icon": "seat:1", "state": "open"});
        let mut turn = crate::play::Turn::new(1);
        turn.options = vec![crate::net::api::LegalSelection {
            notation: "x".into(),
            action: "cross".into(),
            gestures: vec![choice(
                &GridLook {
                    choice: "cross-".into(),
                    ..Default::default()
                },
                "who",
                "2",
            )],
            ..Default::default()
        }];
        turn.stage = Stage::Choosing;
        {
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.table = Some(table(1, grid(open)));
            watch.turn = Some(turn);
        }
        app.update();
        app.update();
        let lit = |app: &mut App| {
            let world = app.world_mut();
            world
                .query_filtered::<&Pick, (With<Tile>, With<Lit>)>()
                .iter(world)
                .map(|c| c.0.value.clone())
                .collect::<Vec<_>>()
        };
        assert!(lit(&mut app).is_empty(), "not unless asked");
        app.world_mut().resource_mut::<ShowLegalMoves>().0 = true;
        app.update();
        assert_eq!(lit(&mut app), ["cross-who-2"]);
    }

    #[test]
    fn what_a_box_means_is_said_in_the_views_words_its_own_or_its_lines() {
        let lines = read(&json!({"lines": [
            {"id": "who", "label": "First player", "hint": "Who leads.", "boxes": [
                {"id": "1", "label": "Fox", "hint": "Fox leads."},
                {"id": "2", "label": "Owl"},
                {"id": "3"}]},
            {"id": "bare", "boxes": [{"id": "x"}]}
        ]}));
        assert_eq!(lines[0].boxes[0].hint, "Fox leads.");
        let said = |line, index| words(&lines, line, index);
        assert_eq!(
            said(0, Some(0)),
            Some(("First player: Fox".into(), "Fox leads.".into()))
        );
        assert_eq!(
            said(0, Some(1)),
            Some(("First player: Owl".into(), "Who leads.".into())),
            "its line's, with none of its own"
        );
        assert_eq!(
            said(0, Some(2)),
            Some(("First player".into(), "Who leads.".into()))
        );
        assert_eq!(
            said(0, None),
            Some(("First player".into(), "Who leads.".into()))
        );
        assert_eq!(said(1, Some(0)), None, "nothing is said of it");
        assert_eq!(said(1, None), None);
        assert_eq!(said(0, Some(9)), None);
        assert_eq!(said(7, None), None);
    }

    #[test]
    fn the_card_of_words_is_beside_the_pointer_and_stays_in_the_window() {
        let window = Vec2::new(1280.0, 720.0);
        assert_eq!(
            card_at(Vec2::new(400.0, 300.0), window, 90.0),
            Vec2::new(400.0, 300.0) + CARD_OFF
        );
        // Near the right edge it goes to the pointer's left; near the bottom, above it.
        let right = card_at(Vec2::new(1200.0, 300.0), window, 90.0);
        assert!((right.x + CARD_WIDTH + CARD_OFF.x - 1200.0).abs() < 1e-3);
        let low = card_at(Vec2::new(400.0, 700.0), window, 90.0);
        assert!((low.y + 90.0 + CARD_OFF.y - 700.0).abs() < 1e-3);
        // And never off the window's top or left.
        let corner = card_at(Vec2::new(5.0, 5.0), Vec2::new(200.0, 60.0), 90.0);
        assert_eq!(corner, Vec2::splat(CARD_EDGE));
    }

    #[test]
    fn a_box_that_may_be_crossed_is_lit_and_pointed_at_while_the_pointer_is_over_it() {
        let mut app = app();
        app.add_plugins(ChoiceGridPlugin);
        let open = json!({"id": "1", "icon": "seat:1", "state": "open"});
        let look = GridLook {
            choice: "cross-".into(),
            ..Default::default()
        };
        let mut turn = crate::play::Turn::new(1);
        turn.options = vec![crate::net::api::LegalSelection {
            notation: "x".into(),
            action: "cross".into(),
            gestures: vec![choice(&look, "who", "2")],
            ..Default::default()
        }];
        turn.stage = Stage::Choosing;
        {
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.table = Some(table(1, grid(open)));
            watch.turn = Some(turn);
        }
        app.update();
        app.update();
        let tile = |app: &mut App, value: &str| {
            let world = app.world_mut();
            world
                .query_filtered::<(Entity, &Pick), With<Tile>>()
                .iter(world)
                .find(|(_, p)| p.0.value == value)
                .unwrap()
                .0
        };
        let (may, may_not) = (tile(&mut app, "cross-who-2"), tile(&mut app, "cross-who-1"));
        let point = |app: &mut App, at: Entity| {
            *app.world_mut().resource_mut::<Under>() = Under {
                chain: vec![at],
                ui: false,
            };
            *app.world_mut().resource_mut::<TableFeel>() = TableFeel::default();
            app.update();
            app.update();
        };
        point(&mut app, may);
        assert!(app.world().get::<Lit>(may).is_some());
        assert_eq!(app.world().resource::<TableFeel>().0, Feel::Point);
        // One that may not be crossed is neither lit nor pointed at, and the other goes out.
        point(&mut app, may_not);
        assert!(app.world().get::<Lit>(may_not).is_none());
        assert!(app.world().get::<Lit>(may).is_none());
        assert_eq!(app.world().resource::<TableFeel>().0, Feel::Open);
        // Every tile is found by the pointer, the lines' heads too: each has words to show.
        let world = app.world_mut();
        assert_eq!(world.query::<&Head>().iter(world).count(), 2);
    }
}
