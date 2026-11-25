//! A board of choices, for a game whose look has `[grid]`: the view's lines of boxes
//! (`ext.view.<view>`), lying on the table as a board of wood with a tile for each box, its
//! picture on it (the look's, by the view's name for it), and each line's own picture at its
//! head. A box a seat crossed has that seat's marker standing on it, small
//! (`seat_markers::Marker`), carried there from the marker in front of the seat; a box out of
//! the game is covered; the box left on a line is rimmed. A click on a box makes the look's
//! choice for it (`play::Choice`: the look's `choice`, the line's id, `-`, the box's id).
//!
//! `read` and `places` have no Bevy in them.

use std::collections::HashMap;

use bevy::picking::Pickable;
use bevy::prelude::*;
use serde_json::Value;

use crate::appearance::{Appearances, GridLook};
use crate::board::{BoardWood, CellShape, ShowLegalMoves};
use crate::card_table::CLOTH;
use crate::hand::{Carry, Hands, Rest, Route};
use crate::highlight::{Lit, Outline};
use crate::look::{self, MaterialSpec};
use crate::net::WatchStatus;
use crate::net::api::{Gesture, Position};
use crate::play::{Choice, Stage};
use crate::seat_markers::{self, Marker};
use crate::table::Table;

pub struct ChoiceGridPlugin;

impl Plugin for ChoiceGridPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Appearances>()
            .init_resource::<BoardWood>()
            .init_resource::<ShowLegalMoves>()
            .add_systems(
                Update,
                (
                    sync.run_if(
                        resource_changed::<WatchStatus>.or_eager(resource_changed::<BoardWood>),
                    ),
                    light.run_if(
                        resource_changed::<WatchStatus>
                            .or_eager(resource_changed::<ShowLegalMoves>),
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

/// A box of the grid: its id, the view's name for its picture, and how it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridBox {
    pub id: String,
    pub icon: String,
    pub state: State,
}

/// A line of the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub id: String,
    pub icon: String,
    pub boxes: Vec<GridBox>,
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
                    boxes: line["boxes"]
                        .as_array()
                        .map(|boxes| {
                            boxes
                                .iter()
                                .map(|b| GridBox {
                                    id: words(&b["id"]),
                                    icon: words(&b["icon"]),
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
                Pickable::IGNORE,
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
            .insert(Name::new(format!("Line {}", line.id)));
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
                tile.insert((Pickable::default(), Choice(choice(look, &line.id, &b.id))));
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

/// With the legal moves lit, the boxes a click would be taken on are.
fn light(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    show: Res<ShowLegalMoves>,
    tiles: Query<(Entity, &Choice, Has<Lit>), With<Tile>>,
) {
    let choices = watch
        .turn
        .as_ref()
        .filter(|t| show.0 && t.stage == Stage::Choosing)
        .map(|t| t.choices())
        .unwrap_or_default();
    for (entity, choice, lit) in &tiles {
        match (choices.contains(&choice.0), lit) {
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
            .query_filtered::<&Choice, With<Tile>>()
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
                .query_filtered::<&Choice, (With<Tile>, With<Lit>)>()
                .iter(world)
                .map(|c| c.0.value.clone())
                .collect::<Vec<_>>()
        };
        assert!(lit(&mut app).is_empty(), "not unless asked");
        app.world_mut().resource_mut::<ShowLegalMoves>().0 = true;
        app.update();
        assert_eq!(lit(&mut app), ["cross-who-2"]);
    }
}
