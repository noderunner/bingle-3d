//! A piece that stands for each seat, for a game whose look has `[markers]`: in front of its
//! seat on a table laid round its seats (`card_layout`), looking to the table's middle, and
//! wherever else the seat is marked (small, on a box of the grid it crossed: `choice_grid.rs`
//! puts a [`Marker`] there, and it is dressed here).
//!
//! A marker's first part is made of the look's `material`, or, once the view puts the seats on
//! sides (`sides`, the seats on one of them), of the material of its side, so partners are
//! seen to be; the parts the look names (`seat_parts`) are its seat's own colour. The seat the
//! view names (`ringed`) has a ring round its marker's foot.
//!
//! Clicks on the markers make the choices that name seats (`choose`: a team picked by its
//! members): a click on a seat's marker chooses it, or lets it go, and it hovers while chosen;
//! the choice is made as soon as no other names more seats than those chosen, or by a click on
//! the player's own marker (which, with none chosen, makes the choice that names no seat:
//! `alone`). `choices` and `click` have no Bevy in them.

use std::collections::{BTreeSet, HashMap};

use bevy::picking::Pickable;
use bevy::picking::events::PointerClick;
use bevy::picking::mesh_picking::ray_cast::SimplifiedMesh;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;

use crate::appearance::{Appearance, Appearances, MarkersLook};
use crate::assets;
use crate::board::BoardWood;
use crate::card_layout;
use crate::card_table;
use crate::cursor::{Feel, Sense, TableFeel, Under};
use crate::hand::{Carry, Hands, Rest};
use crate::look::{self, MaterialSpec};
use crate::net::api::{Gesture, Position};
use crate::net::{Net, WatchStatus};
use crate::play::{self, Stage};
use crate::props;
use crate::table::Table;

pub struct SeatMarkersPlugin;

impl Plugin for SeatMarkersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picked>()
            .init_resource::<Appearances>()
            .init_resource::<BoardWood>()
            .init_resource::<Under>()
            .init_resource::<TableFeel>()
            .add_observer(on_click)
            .add_systems(Update, sense.in_set(Sense))
            .add_systems(
                Update,
                (
                    let_go,
                    sync.run_if(
                        resource_changed::<WatchStatus>.or_eager(resource_changed::<Picked>),
                    ),
                    dress,
                )
                    .chain()
                    .before(Hands),
            );
    }
}

/// How big a small marker is against its model, when the look does not say.
const SMALL: f32 = 0.4;
/// How high a chosen marker hovers, and how much it bobs.
const CHOSEN_LIFT: f32 = 0.014;
const CHOSEN_BOB: f32 = 0.6;
/// A ring round a marker's foot: how far out its middle is, and how thick it is, against the
/// width of the marker's foot.
const RING_OUT: f32 = 0.68;
const RING_THICK: f32 = 0.085;
/// How wide a marker's foot is taken to be, for the box a click on it is found on, when its
/// model does not say.
const FOOT: f32 = 0.034;

/// What stands for a seat: in front of it, or small wherever else it is marked.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    pub seat: u32,
    pub small: bool,
}

/// The markers in front of the seats of the game watched, as seen from `viewer`.
#[derive(Component)]
struct MarkerRoot {
    room_code: String,
    viewer: usize,
}

/// A marker's parts as they are drawn now: for which side, and in which wood.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct Dressed {
    side: Side,
    ringed: bool,
    wood: &'static str,
}

/// A part of a marker, or its ring.
#[derive(Component)]
struct MarkerPart;

/// Which side a seat is on, once the view puts seats on sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Side {
    None,
    On,
    Off,
}

/// The seats chosen so far by clicks on their markers, at the move they were chosen in.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct Picked {
    pub seq: u64,
    pub seats: BTreeSet<u32>,
}

impl Picked {
    fn at(&self, seq: u64) -> BTreeSet<u32> {
        if self.seq == seq {
            self.seats.clone()
        } else {
            BTreeSet::new()
        }
    }
}

/// The choices that name seats, of those the move may make: each with the seats it names
/// (none, for the look's `alone`).
pub fn choices(look: &MarkersLook, all: &[Gesture]) -> Vec<(BTreeSet<u32>, Gesture)> {
    all.iter()
        .filter(|g| g.kind == "action")
        .filter_map(|g| {
            if look.alone.as_deref() == Some(g.value.as_str()) {
                return Some((BTreeSet::new(), g.clone()));
            }
            let rest = look
                .choose
                .iter()
                .find_map(|start| g.value.strip_prefix(start.as_str()))?;
            let seats: Option<BTreeSet<u32>> = rest.split('-').map(|s| s.parse().ok()).collect();
            Some((seats.filter(|s| !s.is_empty())?, g.clone()))
        })
        .collect()
}

/// What a click on a seat's marker does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Click {
    /// Makes this choice.
    Play(Gesture),
    /// The seats chosen are now these.
    Choose(BTreeSet<u32>),
    Nothing,
}

/// What a click on the marker of `seat` does, with the seats `chosen` so far, for the player
/// of the seat `own`: a seat some choice names is chosen, or let go, and the choice of exactly
/// the seats chosen is made once no other names them and more; the player's own marker, when
/// no choice names it, makes the choice of the seats chosen as they are.
pub fn click(
    options: &[(BTreeSet<u32>, Gesture)],
    chosen: &BTreeSet<u32>,
    seat: u32,
    own: u32,
) -> Click {
    let exactly = |seats: &BTreeSet<u32>| options.iter().find(|(s, _)| s == seats);
    if !options.iter().any(|(s, _)| s.contains(&seat)) {
        return match exactly(chosen) {
            Some((_, choice)) if seat == own => Click::Play(choice.clone()),
            _ => Click::Nothing,
        };
    }
    let mut now = chosen.clone();
    if now.remove(&seat) {
        return Click::Choose(now);
    }
    now.insert(seat);
    let more = options
        .iter()
        .any(|(s, _)| s.len() > now.len() && now.is_subset(s));
    match exactly(&now) {
        Some((_, choice)) if !more => Click::Play(choice.clone()),
        _ if more => Click::Choose(now),
        _ => Click::Nothing,
    }
}

/// The look's markers for the game on the table, if it has them.
fn markers_of<'a>(table: &Table, appearances: &'a Appearances) -> Option<&'a Appearance> {
    appearances
        .for_game(&table.info.id)
        .filter(|a| a.markers.is_some())
}

/// The model of a seat's marker, by the seat's colour word.
fn model<'a>(look: &'a Appearance, word: &str) -> Option<&'a str> {
    let id = look.markers.as_ref()?.pieces.get(word)?;
    Some(look.pieces.get(id)?.model.as_str())
}

/// Where the marker of the seat at place `i` of the game's seats stands on the table, and how
/// it is turned, on a table seen from `viewer`.
pub fn place(look: &MarkersLook, i: usize, seats: usize, viewer: usize) -> Transform {
    let seats = seats.max(1);
    let turn = card_layout::seat_turn((i + seats - viewer % seats) % seats, seats);
    let at = card_layout::from_seat(turn, Vec2::from_array(look.seat));
    Transform::from_xyz(at.x, card_table::CLOTH, at.y)
        .with_rotation(Quat::from_rotation_y(turn))
        .with_scale(Vec3::splat(look.size.unwrap_or(1.0)))
}

/// Where the marker of `seat` stands in front of it, for the game on the table.
pub fn place_of(table: &Table, look: &MarkersLook, seat: u32) -> Option<Transform> {
    let i = table.info.seats.iter().position(|s| s.seat == seat)?;
    Some(place(
        look,
        i,
        table.info.seats.len(),
        card_table::viewer(table),
    ))
}

/// The seats the view lists under `name`.
fn view_seats(position: &Position, name: &str) -> Vec<u32> {
    position.ext["view"][name]
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(|s| s.as_u64().map(|s| s as u32))
                .collect()
        })
        .unwrap_or_default()
}

fn side_of(position: &Position, look: &MarkersLook, seat: u32) -> Side {
    let Some(name) = &look.sides else {
        return Side::None;
    };
    let on = view_seats(position, name);
    if on.is_empty() {
        Side::None
    } else if on.contains(&seat) {
        Side::On
    } else {
        Side::Off
    }
}

fn ringed(position: &Position, look: &MarkersLook, seat: u32) -> bool {
    look.ringed
        .as_ref()
        .is_some_and(|name| position.ext["view"][name].as_u64() == Some(seat as u64))
}

/// The markers in front of the seats: made when a game with them is opened, taken down when
/// it is left, and lifted while their seats are chosen.
fn sync(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    appearances: Res<Appearances>,
    picked: Res<Picked>,
    roots: Query<(Entity, &MarkerRoot)>,
    markers: Query<(Entity, &Marker, Option<&Carry>)>,
) {
    let found = watch
        .table
        .as_ref()
        .and_then(|t| Some((t, markers_of(t, &appearances)?.markers.as_ref()?)));
    let Some((table, look)) = found else {
        for (root, _) in &roots {
            commands.entity(root).despawn();
        }
        return;
    };
    let viewer = card_table::viewer(table);
    let current = roots
        .iter()
        .any(|(_, r)| r.room_code == table.room_code && r.viewer == viewer);
    if !current {
        for (root, _) in &roots {
            commands.entity(root).despawn();
        }
        let root = commands
            .spawn((
                Name::new("Seat markers"),
                MarkerRoot {
                    room_code: table.room_code.clone(),
                    viewer,
                },
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        let seats = table.info.seats.len();
        for (i, seat) in table.info.seats.iter().enumerate() {
            let at = place(look, i, seats, viewer);
            commands.spawn((
                Name::new(format!("Marker of {}", seat.color)),
                Marker {
                    seat: seat.seat,
                    small: false,
                },
                at,
                Rest(at),
                Visibility::default(),
                ChildOf(root),
            ));
        }
        return;
    }
    let chosen = picked.at(table.state.move_seq);
    for (entity, marker, carry) in &markers {
        if marker.small {
            continue;
        }
        let lifted = matches!(carry, Some(Carry::Held { .. }));
        match (chosen.contains(&marker.seat), lifted) {
            (true, false) => {
                commands.entity(entity).insert(Carry::Held {
                    lift: CHOSEN_LIFT,
                    scale: CHOSEN_BOB,
                });
            }
            (false, true) => {
                commands.entity(entity).remove::<Carry>();
            }
            _ => {}
        }
    }
}

/// The materials of a game's markers, made once each.
#[derive(Default)]
struct Made(HashMap<(String, String, &'static str), Handle<StandardMaterial>>);

/// Draws each marker's parts, and draws them again when its seat's side, its ring or the
/// board's wood changes.
#[allow(clippy::too_many_arguments)]
fn dress(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    appearances: Res<Appearances>,
    wood: Res<BoardWood>,
    server: Res<AssetServer>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    markers: Query<(Entity, &Marker, Option<&Dressed>, Option<&Children>)>,
    parts: Query<(), With<MarkerPart>>,
    mut made: Local<Made>,
) {
    if markers.is_empty() {
        return;
    }
    let Some(table) = watch.table.as_ref() else {
        return;
    };
    let Some(file) = markers_of(table, &appearances) else {
        return;
    };
    let Some(look) = file.markers.as_ref() else {
        return;
    };
    let game = table.info.id.clone();
    // As the move being built leaves it, where the server previews that: a team just picked.
    let position = watch.shown().unwrap_or(&table.state.state);
    for (entity, marker, dressed, children) in &markers {
        let now = Dressed {
            side: side_of(position, look, marker.seat),
            ringed: !marker.small && ringed(position, look, marker.seat),
            wood: wood.0,
        };
        if dressed == Some(&now) {
            continue;
        }
        for child in children.into_iter().flatten() {
            if parts.contains(*child) {
                commands.entity(*child).despawn();
            }
        }
        let Some(seat) = table.info.seats.iter().find(|s| s.seat == marker.seat) else {
            continue;
        };
        let Some(model) = model(file, &seat.color) else {
            continue;
        };
        let mut material = |key: String, spec: &dyn Fn() -> MaterialSpec| {
            made.0
                .entry((game.clone(), key, wood.0))
                .or_insert_with(|| materials.add(spec().on_board(wood.0).build(&server)))
                .clone()
        };
        let named = |name: &String| file.materials.get(name).cloned().unwrap_or_default();
        let body_name = match now.side {
            Side::On => look.on.as_ref(),
            Side::Off => look.off.as_ref(),
            Side::None => None,
        }
        .unwrap_or(&look.material);
        let body = material(format!("body {body_name}"), &|| named(body_name));
        let own = material(format!("seat {}", seat.color), &|| {
            file.seats
                .get(&seat.color)
                .and_then(|s| s.pieces.as_ref())
                .map(named)
                .unwrap_or_else(|| lacquer(look::seat_paint(&seat.color, seat.seat)))
        });
        let (low, high) = assets::model_span(model).unwrap_or((0.0, 0.07));
        let hit = meshes.add(
            Cuboid::new(FOOT, high - low, FOOT)
                .mesh()
                .build()
                .translated_by(Vec3::Y * (low + high) / 2.0),
        );
        for (index, name) in assets::model_parts(model).iter().enumerate() {
            let of_seat = look.seat_parts.contains(name);
            let mut part = commands.spawn((
                MarkerPart,
                Mesh3d(props::part(&server, model, index)),
                MeshMaterial3d(if of_seat { own.clone() } else { body.clone() }),
                Transform::from_xyz(0.0, -low, 0.0),
                ChildOf(entity),
            ));
            if marker.small {
                part.insert(Pickable::IGNORE);
            } else {
                part.insert((Pickable::default(), SimplifiedMesh(hit.clone())));
            }
        }
        if now.ringed
            && let Some(name) = &look.ring
        {
            let ring = material(format!("ring {name}"), &|| named(name));
            commands.spawn((
                MarkerPart,
                Pickable::IGNORE,
                Mesh3d(meshes.add(Torus::new(
                    FOOT * (RING_OUT - RING_THICK),
                    FOOT * (RING_OUT + RING_THICK),
                ))),
                MeshMaterial3d(ring),
                Transform::from_xyz(0.0, FOOT * RING_THICK, 0.0),
                ChildOf(entity),
            ));
        }
        commands.entity(entity).insert(now);
    }
}

/// A seat's paint as a lacquer, for the part of a marker that is its seat's.
fn lacquer(paint: Color) -> MaterialSpec {
    let c = paint.to_srgba();
    MaterialSpec {
        tint: Some([c.red, c.green, c.blue]),
        roughness: Some(0.35),
        clearcoat: Some(0.6),
        clearcoat_roughness: Some(0.15),
        ..default()
    }
}

/// What a click on the marker of `seat` would do now, for the game watched.
fn click_now(watch: &WatchStatus, appearances: &Appearances, picked: &Picked, seat: u32) -> Click {
    let Some(table) = watch.table.as_ref() else {
        return Click::Nothing;
    };
    let Some(look) = markers_of(table, appearances).and_then(|a| a.markers.as_ref()) else {
        return Click::Nothing;
    };
    let Some(turn) = watch.turn.as_ref().filter(|t| t.stage == Stage::Choosing) else {
        return Click::Nothing;
    };
    let Some(own) = table.yours().map(|s| s.seat) else {
        return Click::Nothing;
    };
    let options = choices(look, &turn.choices());
    click(&options, &picked.at(table.state.move_seq), seat, own)
}

fn on_click(
    click: On<PointerClick>,
    markers: Query<&Marker>,
    net: Res<Net>,
    appearances: Res<Appearances>,
    mut watch: ResMut<WatchStatus>,
    mut picked: ResMut<Picked>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(marker) = markers.get(click.entity) else {
        return;
    };
    if marker.small {
        return;
    }
    let seq = watch.table.as_ref().map_or(0, |t| t.state.move_seq);
    match click_now(&watch, &appearances, &picked, marker.seat) {
        Click::Play(choice) => {
            picked.seats.clear();
            play::gesture(&net, &mut watch, &choice);
        }
        Click::Choose(seats) => *picked = Picked { seq, seats },
        Click::Nothing => {}
    }
}

/// Says what a click on the marker under the pointer would do (`cursor.rs`).
fn sense(
    under: Res<Under>,
    markers: Query<&Marker>,
    (watch, appearances, picked): (Res<WatchStatus>, Res<Appearances>, Res<Picked>),
    mut table: ResMut<TableFeel>,
) {
    let Some(marker) = under
        .chain
        .iter()
        .find_map(|e| markers.get(*e).ok())
        .filter(|m| !m.small)
    else {
        return;
    };
    let feel = match click_now(&watch, &appearances, &picked, marker.seat) {
        Click::Nothing => Feel::Open,
        Click::Play(_) | Click::Choose(_) => Feel::Point,
    };
    table.set_if_neq(TableFeel(feel));
}

/// Backspace lets go of the seats chosen.
fn let_go(keys: Option<Res<ButtonInput<KeyCode>>>, mut picked: ResMut<Picked>) {
    let pressed = keys.is_some_and(|k| k.just_pressed(KeyCode::Backspace));
    if pressed && !picked.seats.is_empty() {
        picked.seats.clear();
    }
}

/// How big a small marker is against its model, for a look.
pub fn small(look: &MarkersLook) -> f32 {
    look.small.unwrap_or(SMALL)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(value: &str) -> Gesture {
        Gesture {
            kind: "action".into(),
            value: value.into(),
        }
    }

    fn look() -> MarkersLook {
        MarkersLook {
            choose: vec!["side-".into(), "gift-".into()],
            alone: Some("side-alone".into()),
            seat: [0.1, 0.3],
            ..Default::default()
        }
    }

    fn set(seats: &[u32]) -> BTreeSet<u32> {
        seats.iter().copied().collect()
    }

    #[test]
    fn the_choices_that_name_seats_are_read_from_their_values() {
        let all = [
            act("side-alone"),
            act("side-2"),
            act("side-2-4"),
            act("gift-3"),
            act("card-red-9"),
            act("side-x"),
            Gesture::pos("a1"),
        ];
        let found = choices(&look(), &all);
        let seats: Vec<BTreeSet<u32>> = found.iter().map(|(s, _)| s.clone()).collect();
        assert_eq!(seats, [set(&[]), set(&[2]), set(&[2, 4]), set(&[3])]);
        assert_eq!(found[2].1, act("side-2-4"));
    }

    #[test]
    fn a_partner_is_chosen_by_a_click_when_no_choice_names_more() {
        // Four seats: the leader names one partner.
        let options = choices(&look(), &[act("side-2"), act("side-3"), act("side-4")]);
        assert_eq!(click(&options, &set(&[]), 3, 1), Click::Play(act("side-3")));
        assert_eq!(
            click(&options, &set(&[]), 1, 1),
            Click::Nothing,
            "nobody is no team here"
        );
        // Three seats: alone, by the player's own marker, or with one partner.
        let options = choices(&look(), &[act("side-alone"), act("side-2"), act("side-3")]);
        assert_eq!(
            click(&options, &set(&[]), 1, 1),
            Click::Play(act("side-alone"))
        );
        assert_eq!(click(&options, &set(&[]), 2, 1), Click::Play(act("side-2")));
        assert_eq!(click(&options, &set(&[]), 1, 2), Click::Nothing, "not ours");
    }

    #[test]
    fn a_team_of_two_or_three_is_chosen_seat_by_seat_and_made_by_the_players_own_marker() {
        // Five seats: one partner or two.
        let options = choices(
            &look(),
            &[
                act("side-2"),
                act("side-3"),
                act("side-4"),
                act("side-2-3"),
                act("side-2-4"),
                act("side-3-4"),
            ],
        );
        assert_eq!(
            click(&options, &set(&[]), 3, 1),
            Click::Choose(set(&[3])),
            "one more may follow"
        );
        assert_eq!(
            click(&options, &set(&[3]), 1, 1),
            Click::Play(act("side-3")),
            "the two of us"
        );
        assert_eq!(
            click(&options, &set(&[3]), 2, 1),
            Click::Play(act("side-2-3")),
            "sent in the server's order"
        );
        assert_eq!(
            click(&options, &set(&[3]), 3, 1),
            Click::Choose(set(&[])),
            "let go"
        );
        assert_eq!(click(&options, &set(&[]), 1, 1), Click::Nothing);
        assert_eq!(click(&options, &set(&[]), 5, 1), Click::Nothing);
    }

    #[test]
    fn a_choice_may_name_the_players_own_seat() {
        let options = choices(&look(), &[act("gift-1"), act("gift-4")]);
        assert_eq!(click(&options, &set(&[]), 1, 1), Click::Play(act("gift-1")));
        assert_eq!(click(&options, &set(&[]), 4, 1), Click::Play(act("gift-4")));
        assert_eq!(click(&options, &set(&[]), 2, 1), Click::Nothing);
    }

    #[test]
    fn each_seats_marker_stands_in_front_of_it_looking_to_the_middle() {
        let look = look();
        // Seen from the first seat of four: its own on the player's side, to its right.
        let own = place(&look, 0, 4, 0);
        assert!((own.translation.x - 0.1).abs() < 1e-6 && (own.translation.z - 0.3).abs() < 1e-6);
        // The next seat is on the player's left, and its marker to its own right: the far end
        // of that side.
        let next = place(&look, 1, 4, 0);
        assert!((next.translation.x + 0.3).abs() < 1e-6, "{next:?}");
        assert!((next.translation.z - 0.1).abs() < 1e-6, "{next:?}");
        // Looking to the middle: a model looks to its far side (-z).
        let ahead = next.rotation * Vec3::NEG_Z;
        assert!(ahead.distance(Vec3::X) < 1e-5, "{ahead}");
        // Seen from the second seat, that one is the player's.
        let mine = place(&look, 1, 4, 1);
        assert!(mine.translation.distance(own.translation) < 1e-6);
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

    #[test]
    fn a_marker_stands_for_each_seat_and_is_made_of_its_sides_wood_once_there_are_sides() {
        use serde_json::json;
        let mut app = app();
        app.add_plugins(SeatMarkersPlugin);
        app.world_mut().resource_mut::<WatchStatus>().table = Some(table(1, json!({})));
        app.update();
        app.update();
        let dressed = |app: &mut App| -> Vec<(u32, Side, bool)> {
            let world = app.world_mut();
            let mut all: Vec<(u32, Side, bool)> = world
                .query::<(&Marker, &Dressed)>()
                .iter(world)
                .map(|(m, d)| (m.seat, d.side, d.ringed))
                .collect();
            all.sort_by_key(|(seat, ..)| *seat);
            all
        };
        assert_eq!(
            dressed(&mut app),
            [
                (1, Side::None, false),
                (2, Side::None, false),
                (3, Side::None, false)
            ]
        );
        let parts = |app: &mut App| {
            let world = app.world_mut();
            world.query::<&MarkerPart>().iter(world).count()
        };
        assert_eq!(parts(&mut app), 3, "a model of one part each");

        // Sides are made, and one seat is named: partners are the same wood, and that seat's
        // marker has a ring.
        let sided = table(2, json!({"team": [1, 3], "bonus": 2}));
        app.world_mut().resource_mut::<WatchStatus>().table = Some(sided);
        app.update();
        app.update();
        assert_eq!(
            dressed(&mut app),
            [
                (1, Side::On, false),
                (2, Side::Off, true),
                (3, Side::On, false)
            ]
        );
        assert_eq!(parts(&mut app), 4, "drawn again, with the ring");
        let world = app.world_mut();
        let mut materials = world.query::<(&ChildOf, &MeshMaterial3d<StandardMaterial>)>();
        let mut markers = world.query::<(Entity, &Marker)>();
        let mut of = |seat: u32, world: &World| {
            let entity = markers.iter(world).find(|(_, m)| m.seat == seat).unwrap().0;
            materials
                .iter(world)
                .find(|(parent, _)| parent.parent() == entity)
                .unwrap()
                .1
                .0
                .clone()
        };
        let (first, second, third) = (of(1, world), of(2, world), of(3, world));
        assert_eq!(first, third);
        assert_ne!(first, second);

        // A seat chosen by a click hovers until the choice is made or let go.
        *app.world_mut().resource_mut::<Picked>() = Picked {
            seq: 2,
            seats: set(&[3]),
        };
        app.update();
        let world = app.world_mut();
        let lifted: Vec<u32> = world
            .query::<(&Marker, &Carry)>()
            .iter(world)
            .map(|(m, _)| m.seat)
            .collect();
        assert_eq!(lifted, [3]);

        // The game left: nothing stands.
        app.world_mut().resource_mut::<WatchStatus>().table = None;
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&Marker>().iter(world).count(), 0);
    }
}
