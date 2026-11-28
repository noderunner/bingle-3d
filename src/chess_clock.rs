//! A chess clock on the table in a timed game of two seats: a wooden case beside the board with
//! an analog face at each seat's end, and an amber lamp lit under the face whose time runs. It
//! only shows: the server keeps the time, and a click on the clock does nothing. The panel's
//! times stay (`game_panel.rs`); a game of more seats, or one timed only by the move, has no
//! clock. It stands by default; *Settings* leaves it out (`ChessClockOn`, `chess_clock` in
//! `config.toml`), and a run a script limits has it whatever is kept.
//!
//! A face reads as a clock counting down to twelve: it shows twelve o'clock less the time left
//! (`hands`), so two hours is the hour hand at ten, and the last minute the second hand's last
//! turn to the twelve, where the flag stands until nothing is left and then falls. The button
//! over the face whose time runs stands up and the other is down, as on a real clock.
//!
//! It stands at the first seat's right hand, at the middle of that side, beyond the board and
//! whatever lies there along its length (`place`, from `board::BesideBoard`), its long side
//! between the seats and its faces looking across the board: the first seat's face at its end.
//! The case is made of the board's wood (`board::BoardWood`), as the trays are, and the lamps are
//! tuned for a camera that blooms and one that does not (`scene::BloomOn`).
//!
//! The model is the asset pipeline's (`assets-src/blender/chess_clock.py`): the case one node,
//! and each hand, flag and button a node of its own, turned about its own pivot. `hands`,
//! `stands_for` and `place` have no Bevy in them.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Instant;

use bevy::gltf::GltfAssetLabel;
use bevy::light::NotShadowCaster;
use bevy::picking::Pickable;
use bevy::prelude::*;

use crate::assets;
use crate::board::{BesideBoard, BoardOutline, BoardWood, Footing};
use crate::look::{self, MaterialSpec};
use crate::net::WatchStatus;
use crate::net::api::{Clock, SeatDef};
use crate::scene::BloomOn;
use crate::table::Table;

pub const MODEL: &str = "models/chess_clock.glb";
/// The case's length (along its faces) and depth at its foot, in metres (`chess_clock.py`).
pub const LENGTH: f32 = 0.160;
pub const DEPTH: f32 = 0.058;
/// The space left between the clock and what it stands beside.
const GAP: f32 = 0.03;
/// How far a button goes down, and how fast it gets there (a share of the way a second).
const TRAVEL: f32 = 0.005;
const PRESS: f32 = 18.0;
/// How far a flag turns as it falls.
const FALLEN: f32 = 2.6;
/// Twelve hours: a face shows no more.
const FULL_FACE: f64 = 12.0 * 3600.0;

pub struct ChessClockPlugin {
    /// A run a script limits (`--frames`, `--screenshot`): the clock stands, whatever is kept.
    pub limited: bool,
}

impl Plugin for ChessClockPlugin {
    fn build(&self, app: &mut App) {
        let kept = app
            .world()
            .get_resource::<Kept>()
            .copied()
            .unwrap_or_default();
        app.insert_resource(ChessClockOn(chosen(self.limited, kept.0)))
            .init_resource::<Drawn>()
            .init_resource::<BloomOn>()
            .init_resource::<BoardOutline>()
            .init_resource::<BesideBoard>()
            .init_resource::<BoardWood>()
            .add_systems(
                Update,
                (tune_lamps, sync_clock, read_table, show_time).chain(),
            );
    }
}

/// What `config.toml` keeps (`chess_clock`), put here by the network's plugin, which reads it.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Kept(pub Option<bool>);

/// Whether a timed game of two seats has a chess clock beside the board: what *Settings*
/// changes.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChessClockOn(pub bool);

/// The run's choice: on in a run a script limits, else as kept, else on.
pub fn chosen(limited: bool, kept: Option<bool>) -> bool {
    limited || kept.unwrap_or(true)
}

/// Where a face's hands and flag stand: each hand's turn from the twelve, clockwise as the face
/// is seen, in radians, and whether the flag still stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hands {
    pub hour: f32,
    pub minute: f32,
    pub second: f32,
    pub flag_up: bool,
}

/// The face of a seat with `left` seconds: twelve o'clock less the time left, counted in whole
/// seconds as the panel's words are (`clock::words`), so the second hand ticks. Two hours is
/// the hour hand at ten and the minute hand at twelve; a minute, both just short of twelve and
/// the second hand at twelve with a turn to go. Twelve hours or more is just under twelve. The
/// flag stands until nothing is left.
pub fn hands(left: f64) -> Hands {
    let s = left.max(0.0).ceil().min(FULL_FACE - 1.0) as u64;
    let shown = (FULL_FACE as u64 - s) % FULL_FACE as u64;
    let turn = |of: u64, whole: u64| (of % whole) as f32 / whole as f32 * TAU;
    Hands {
        hour: turn(shown, 43_200),
        minute: turn(shown, 3600),
        second: turn(shown, 60),
        flag_up: s > 0,
    }
}

/// Whether a game has a clock on the table: two seats, each with a game clock of its own.
pub fn stands_for(seats: &[SeatDef], clock: Option<&Clock>) -> bool {
    let Some(clock) = clock else {
        return false;
    };
    seats.len() == 2
        && seats
            .iter()
            .all(|s| clock.seats.iter().any(|c| c.seat == s.seat))
}

/// Whether the watched game has a clock on the table, the setting aside.
pub fn stands(table: &Table) -> bool {
    stands_for(
        &table.info.seats,
        table.timing.as_ref().and_then(|t| t.clock.as_ref()),
    )
}

/// Where the clock's middle stands on the table (x, z), beside a board of this `outline` with
/// `taken` laid beside it: at the first seat's right hand (+x), at the middle of that side,
/// beyond the board and whatever lies there along the clock's length.
pub fn place(outline: &[Vec2], taken: &[Rect]) -> Vec2 {
    let band = LENGTH / 2.0 + GAP;
    let board = (0..=8)
        .map(|k| crate::bowls::width_at(outline, -band + band * k as f32 / 4.0))
        .fold(0.0f32, f32::max);
    let beside = taken
        .iter()
        .filter(|r| r.max.y > -band && r.min.y < band && r.max.x > 0.0)
        .fold(board, |x, r| x.max(r.max.x));
    Vec2::new(beside + GAP + DEPTH / 2.0, 0.0)
}

/// How far from the table's middle a clock standing at `at` reaches, for the camera.
pub fn reach(at: Vec2) -> f32 {
    at.x.abs() + DEPTH / 2.0
}

/// How the clock is turned on the table: its front (the model's +z) to the board, so its right
/// end (+x) is towards the first seat (+z).
pub fn turn() -> Quat {
    Quat::from_rotation_y(-FRAC_PI_2)
}

/// One end of the clock, as seen from its front: the right is the first seat's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Left,
    Right,
}

impl End {
    fn word(self) -> &'static str {
        match self {
            End::Left => "left",
            End::Right => "right",
        }
    }

    /// The end of the seat at this place in the game's order.
    pub fn of_seat(index: usize) -> End {
        if index == 0 { End::Right } else { End::Left }
    }
}

/// What a clock shows: each end's time left, and the end whose time runs, if any.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Showing {
    pub left: f64,
    pub right: f64,
    pub running: Option<End>,
}

impl Showing {
    fn at(&self, end: End) -> f64 {
        match end {
            End::Left => self.left,
            End::Right => self.right,
        }
    }

    /// Whether an end's button is down: while the other end's time runs, as on a real clock,
    /// whose button is pressed to start the other's.
    pub fn pressed(&self, end: End) -> bool {
        self.running.is_some_and(|e| e != end)
    }
}

/// A part of the clock that moves, where it stands as made.
#[derive(Component, Debug, Clone, Copy)]
struct Moving {
    kind: Kind,
    end: End,
    rest: Transform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Hour,
    Minute,
    Second,
    Flag,
    Button,
}

impl Kind {
    const ALL: [Kind; 5] = [
        Kind::Hour,
        Kind::Minute,
        Kind::Second,
        Kind::Flag,
        Kind::Button,
    ];

    fn word(self) -> &'static str {
        match self {
            Kind::Hour => "hour",
            Kind::Minute => "minute",
            Kind::Second => "second",
            Kind::Flag => "flag",
            Kind::Button => "button",
        }
    }
}

/// A lamp, under the face of its end, and its materials unlit and lit.
#[derive(Component, Debug, Clone)]
struct Lamp {
    end: End,
    dark: Handle<StandardMaterial>,
    lit: Handle<StandardMaterial>,
}

/// The clock this module keeps beside the watched game's board.
#[derive(Component)]
struct Watched;

/// What the clocks are made of, but their case's wood: made once.
#[derive(Clone)]
pub struct Kit {
    face: Handle<StandardMaterial>,
    ink: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
    brass: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    lit: Handle<StandardMaterial>,
}

impl Kit {
    pub fn new(materials: &mut Assets<StandardMaterial>, server: &AssetServer, bloom: bool) -> Kit {
        let mut add = |spec: MaterialSpec| materials.add(spec.build(server));
        Kit {
            face: add(look::clock_face()),
            ink: add(look::clock_ink()),
            red: add(look::clock_red()),
            brass: add(look::clock_brass()),
            dark: add(look::clock_lamp(false, bloom)),
            lit: add(look::clock_lamp(true, bloom)),
        }
    }

    /// The material of a part of the case, or of a moving part by its kind.
    fn part(&self, name: &str, frame: &Handle<StandardMaterial>) -> Handle<StandardMaterial> {
        match name {
            "frame" => frame.clone(),
            "face" => self.face.clone(),
            "marks" | "hand" => self.ink.clone(),
            "second" | "flag" => self.red.clone(),
            "bezel" | "button" => self.brass.clone(),
            _ => self.dark.clone(),
        }
    }
}

/// Stands a clock at `at`, under `parent`, its case of `frame`: the case's parts, and each
/// moving part as made (`show_time` turns them to what its `Showing` says).
pub fn spawn(
    commands: &mut Commands,
    server: &AssetServer,
    kit: &Kit,
    frame: &Handle<StandardMaterial>,
    at: Transform,
    showing: Showing,
) -> Entity {
    let whole = commands
        .spawn((Name::new("Chess clock"), at, Visibility::default(), showing))
        .id();
    let primitive = |mesh: usize, primitive: usize| -> Handle<Mesh> {
        server.load(GltfAssetLabel::Primitive { mesh, primitive }.from_asset(assets::path(MODEL)))
    };
    for node in assets::model_nodes(MODEL) {
        if node.name == "case" {
            let case = commands
                .spawn((node.at, Visibility::default(), ChildOf(whole)))
                .id();
            for (i, part) in assets::mesh_parts(MODEL, node.mesh).iter().enumerate() {
                let mut e = commands.spawn((
                    Mesh3d(primitive(node.mesh, i)),
                    MeshMaterial3d(kit.part(part, frame)),
                    Pickable::IGNORE,
                    ChildOf(case),
                ));
                let end = match part.as_str() {
                    "lamp_left" => End::Left,
                    "lamp_right" => End::Right,
                    _ => continue,
                };
                e.insert((
                    Lamp {
                        end,
                        dark: kit.dark.clone(),
                        lit: kit.lit.clone(),
                    },
                    NotShadowCaster,
                ));
            }
            continue;
        }
        let Some((kind, end)) = Kind::ALL.iter().find_map(|k| {
            [End::Left, End::Right]
                .into_iter()
                .find(|e| node.name == format!("{}_{}", k.word(), e.word()))
                .map(|e| (*k, e))
        }) else {
            warn!("{MODEL}: a node {:?} the client does not know", node.name);
            continue;
        };
        let name = assets::mesh_parts(MODEL, node.mesh)
            .into_iter()
            .next()
            .unwrap_or_default();
        commands.spawn((
            Moving {
                kind,
                end,
                rest: node.at,
            },
            node.at,
            Visibility::default(),
            ChildOf(whole),
            children![(
                Mesh3d(primitive(node.mesh, 0)),
                MeshMaterial3d(kit.part(&name, frame)),
                Pickable::IGNORE,
            )],
        ));
    }
    whole
}

/// The clock kept beside the watched game's board: the board it stands by, where, and in what
/// wood; and what clocks are made of, once made.
#[derive(Resource, Default)]
struct Drawn {
    stands: Option<(Entity, Vec2, &'static str)>,
    clock: Option<Entity>,
    kit: Option<Kit>,
}

/// Keeps the lamps' lit material tuned to whether the camera blooms.
fn tune_lamps(
    bloom: Res<BloomOn>,
    mut drawn: ResMut<Drawn>,
    server: Option<Res<AssetServer>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    let (Some(server), Some(mut materials)) = (server, materials) else {
        return;
    };
    match &drawn.kit {
        None => drawn.kit = Some(Kit::new(&mut materials, &server, bloom.0)),
        Some(kit) if bloom.is_changed() => {
            let lit = look::clock_lamp(true, bloom.0).build(&server);
            if let Some(mut m) = materials.get_mut(&kit.lit) {
                *m = lit;
            }
        }
        Some(_) => {}
    }
}

/// Stands the clock beside the watched game's board when it has one, and takes it away when it
/// no longer does; again when the board, what lies beside it or the wood changes.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system takes what it reads as parameters"
)]
fn sync_clock(
    mut commands: Commands,
    on: Res<ChessClockOn>,
    watch: Res<WatchStatus>,
    roots: Query<Entity, With<Footing>>,
    outline: Res<BoardOutline>,
    beside: Res<BesideBoard>,
    wood: Res<BoardWood>,
    mut drawn: ResMut<Drawn>,
    server: Option<Res<AssetServer>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    let wanted = match (&watch.table, roots.single()) {
        (Some(table), Ok(root)) if on.0 && stands(table) => {
            Some((root, place(&outline.0, &beside.0), wood.0))
        }
        _ => None,
    };
    if wanted == drawn.stands {
        return;
    }
    if let Some(old) = drawn.clock.take() {
        commands.entity(old).try_despawn();
    }
    drawn.stands = wanted;
    let (Some((root, at, wood)), Some(server), Some(mut materials), Some(kit)) =
        (wanted, server, materials, drawn.kit.clone())
    else {
        return;
    };
    let frame = materials.add(look::tray().on_board(wood).build(&server));
    let clock = spawn(
        &mut commands,
        &server,
        &kit,
        &frame,
        Transform::from_xyz(at.x, 0.0, at.y).with_rotation(turn()),
        Showing {
            left: 0.0,
            right: 0.0,
            running: None,
        },
    );
    commands.entity(clock).insert((Watched, ChildOf(root)));
    drawn.clock = Some(clock);
}

/// The end whose time runs in the watched game: the clock's running seat's, while the game is
/// being played.
fn running(table: &Table) -> Option<End> {
    let active = table.state.status == "active" && table.state.impulse_seat.is_some();
    let seat = table
        .timing
        .as_ref()?
        .clock
        .as_ref()?
        .running
        .filter(|_| active)?;
    let index = table.info.seats.iter().position(|s| s.seat == seat)?;
    Some(End::of_seat(index))
}

/// The watched game's times, on its clock.
fn read_table(watch: Res<WatchStatus>, mut clocks: Query<&mut Showing, With<Watched>>) {
    let Some(table) = &watch.table else {
        return;
    };
    let now = Instant::now();
    let left = |index: usize| {
        table
            .info
            .seats
            .get(index)
            .and_then(|s| table.time_left(s.seat, now))
            .unwrap_or(0.0)
    };
    let showing = Showing {
        right: left(0),
        left: left(1),
        running: running(table),
    };
    for mut shown in &mut clocks {
        shown.set_if_neq(showing);
    }
}

/// Turns every clock's hands and flags to the time it shows, presses the button of the end
/// whose time does not run, and lights the lamp of the one whose does.
fn show_time(
    time: Res<Time>,
    clocks: Query<&Showing>,
    mut parts: Query<(&Moving, &ChildOf, &mut Transform)>,
    mut lamps: Query<(&Lamp, &ChildOf, &mut MeshMaterial3d<StandardMaterial>)>,
    parents: Query<&ChildOf, Without<Lamp>>,
) {
    for (part, parent, mut at) in &mut parts {
        let Ok(showing) = clocks.get(parent.parent()) else {
            continue;
        };
        let face = hands(showing.at(part.end));
        let turned =
            |angle: f32| part.rest * Transform::from_rotation(Quat::from_rotation_z(-angle));
        *at = match part.kind {
            Kind::Hour => turned(face.hour),
            Kind::Minute => turned(face.minute),
            Kind::Second => turned(face.second),
            Kind::Flag => turned(if face.flag_up { 0.0 } else { FALLEN }),
            Kind::Button => {
                let down = if showing.pressed(part.end) {
                    TRAVEL
                } else {
                    0.0
                };
                let to = part.rest.translation - Vec3::Y * down;
                let share = (time.delta_secs() * PRESS).min(1.0);
                Transform {
                    translation: at.translation.lerp(to, share),
                    ..part.rest
                }
            }
        };
    }
    for (lamp, case, mut material) in &mut lamps {
        // A lamp is a part of the case, which hangs from the clock.
        let Some(showing) = parents
            .get(case.parent())
            .ok()
            .and_then(|clock| clocks.get(clock.parent()).ok())
        else {
            continue;
        };
        let want = if showing.running == Some(lamp.end) {
            &lamp.lit
        } else {
            &lamp.dark
        };
        if material.0 != *want {
            material.0 = want.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::api::{Board, Game, GameInfo, GameState, SeatClock};
    use crate::net::session::Opened;

    const MINUTE: f32 = TAU / 60.0;

    fn near(a: f32, b: f32) -> bool {
        // The same turn, whichever way round it is said.
        let d = (a - b).rem_euclid(TAU);
        d < 1e-4 || TAU - d < 1e-4
    }

    #[test]
    fn a_face_shows_twelve_less_the_time_left() {
        // Two hours: the hour hand at ten, the minute and second hands at twelve.
        let two = hands(7200.0);
        assert!(near(two.hour, 10.0 / 12.0 * TAU), "{}", two.hour);
        assert!(near(two.minute, 0.0) && near(two.second, 0.0));
        assert!(two.flag_up);
        // A minute: the hour and minute hands just short of twelve, the second hand at twelve
        // with a turn to go.
        let one = hands(60.0);
        assert!(near(one.minute, -MINUTE), "{}", one.minute);
        assert!(near(one.hour, -MINUTE / 12.0));
        assert!(near(one.second, 0.0));
        assert!(one.flag_up);
        // Seconds tick, counted up as the panel's words are: 4.2 s reads as 5.
        let few = hands(4.2);
        assert!(near(few.second, -5.0 * MINUTE), "{}", few.second);
        // Nothing left: every hand at twelve, and the flag fallen.
        let none = hands(0.0);
        assert!(!none.flag_up);
        assert!(near(none.hour, 0.0) && near(none.minute, 0.0) && near(none.second, 0.0));
        assert_eq!(hands(-3.0), none);
        // A day is shown as just under twelve hours.
        let day = hands(86_400.0);
        assert!(day.flag_up && near(day.second, MINUTE), "{day:?}");
        assert!(day.hour < 1e-3, "{}", day.hour);
    }

    fn seats(n: u32) -> Vec<SeatDef> {
        (1..=n)
            .map(|seat| SeatDef {
                seat,
                color: format!("c{seat}"),
                label: String::new(),
                colors: vec![],
            })
            .collect()
    }

    fn clock(seats: &[(u32, f64)], running: Option<u32>) -> Clock {
        Clock {
            seats: seats
                .iter()
                .map(|&(seat, left)| SeatClock {
                    seat,
                    initial_seconds: 300,
                    seconds_left: left,
                })
                .collect(),
            running,
            increment_seconds: 0,
        }
    }

    #[test]
    fn only_a_game_of_two_seats_with_a_game_clock_has_one() {
        let two = clock(&[(1, 300.0), (2, 300.0)], Some(1));
        assert!(stands_for(&seats(2), Some(&two)));
        assert!(
            !stands_for(&seats(2), None),
            "timed by the move, or not at all"
        );
        assert!(!stands_for(&seats(3), Some(&two)), "more times than faces");
        assert!(!stands_for(&seats(1), Some(&two)));
        let one = clock(&[(1, 300.0)], Some(1));
        assert!(!stands_for(&seats(2), Some(&one)), "a seat with no clock");
    }

    #[test]
    fn it_is_on_unless_left_out_and_on_in_a_scripted_run() {
        assert!(chosen(false, None));
        assert!(!chosen(false, Some(false)));
        assert!(chosen(true, Some(false)));
    }

    fn rectangle(half: Vec2) -> Vec<Vec2> {
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| Vec2::new(x * half.x, y * half.y))
            .to_vec()
    }

    #[test]
    fn it_stands_beside_the_board_at_the_first_seats_right_clear_of_what_lies_there() {
        let half = Vec2::new(0.15, 0.15);
        let board = rectangle(half);
        let bare = place(&board, &[]);
        assert_eq!(bare.y, 0.0, "at the middle of the side");
        assert!((bare.x - (half.x + GAP + DEPTH / 2.0)).abs() < 1e-6);
        // A bigger board moves it out.
        assert!(place(&rectangle(Vec2::new(0.25, 0.25)), &[]).x > bare.x + 0.09);
        // A supply along that side, and a bowl by the corner within its length: beyond them.
        let supply = Rect::new(0.17, -0.14, 0.19, 0.14);
        let bowl = Rect::from_center_half_size(Vec2::new(0.26, 0.09), Vec2::splat(0.066));
        let at = place(&board, &[supply, bowl]);
        assert!((at.x - DEPTH / 2.0 - GAP - bowl.max.x).abs() < 1e-6, "{at}");
        // What lies beyond its length, or on the other side, leaves it where it was.
        let far = Rect::from_center_half_size(Vec2::new(0.26, 0.3), Vec2::splat(0.066));
        let left = Rect::new(-0.4, -0.05, -0.16, 0.05);
        assert_eq!(place(&board, &[far, left]), bare);
        // Clear of all of it: none of them reaches into the clock.
        let clock = Rect::from_center_half_size(at, Vec2::new(DEPTH, LENGTH) / 2.0);
        for r in [supply, bowl] {
            assert!(r.intersect(clock).is_empty(), "{r:?}");
        }
        assert!((reach(at) - (at.x + DEPTH / 2.0)).abs() < 1e-6);
    }

    #[test]
    fn the_first_seats_face_is_at_its_end_and_the_faces_look_across_the_board() {
        // The first seat sits on the player's side (+z), the clock to its right (+x).
        let turned = turn();
        assert!(
            (turned * Vec3::X).distance(Vec3::Z) < 1e-6,
            "the right end to the first seat"
        );
        assert!(
            (turned * Vec3::Z).distance(Vec3::NEG_X) < 1e-6,
            "the front to the board"
        );
        assert_eq!(End::of_seat(0), End::Right);
        assert_eq!(End::of_seat(1), End::Left);
        // The button over the face whose time runs is up, the other down; none runs, both up.
        let mut showing = Showing {
            left: 60.0,
            right: 60.0,
            running: Some(End::Right),
        };
        assert!(showing.pressed(End::Left) && !showing.pressed(End::Right));
        showing.running = None;
        assert!(!showing.pressed(End::Left) && !showing.pressed(End::Right));
    }

    #[test]
    fn the_model_has_the_parts_and_the_size_the_client_takes_it_for() {
        let nodes = assets::model_nodes(MODEL);
        let mut names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        names.sort_unstable();
        let mut wanted = vec!["case".to_string()];
        for end in [End::Left, End::Right] {
            for kind in Kind::ALL {
                wanted.push(format!("{}_{}", kind.word(), end.word()));
            }
        }
        wanted.sort_unstable();
        assert_eq!(names, wanted);
        let case = nodes.iter().find(|n| n.name == "case").unwrap();
        assert_eq!(
            assets::mesh_parts(MODEL, case.mesh),
            ["frame", "face", "marks", "bezel", "lamp_left", "lamp_right"]
        );
        let (low, high) = assets::mesh_bounds(MODEL, case.mesh).unwrap();
        assert!((high.x - low.x - LENGTH).abs() < 1e-4, "{low} {high}");
        // Its foot's front edge is eased.
        assert!((high.z - low.z - DEPTH).abs() < 0.002, "{low} {high}");
        assert!(low.y.abs() < 1e-5, "it stands on the table");
        // Each moving part is something the client gives a material.
        for node in nodes.iter().filter(|n| n.name != "case") {
            let parts = assets::mesh_parts(MODEL, node.mesh);
            assert!(
                matches!(
                    parts[..].first().map(String::as_str),
                    Some("hand" | "second" | "flag" | "button")
                ),
                "{}: {parts:?}",
                node.name
            );
        }
        // A face's hands turn about the same pivot, out of the face, which leans back.
        let pivot = |name: &str| nodes.iter().find(|n| n.name == name).unwrap().at;
        let (hour, second) = (pivot("hour_right"), pivot("second_right"));
        let out = hour.rotation * Vec3::Z;
        assert!(
            out.z > 0.9 && out.y > 0.2,
            "out of the front, tilted up: {out}"
        );
        let apart = second.translation - hour.translation;
        assert!(apart.cross(out).length() < 1e-5, "one above the other");
        assert!(hour.translation.x > 0.0, "the right end's on the right");
    }

    fn table(seats: u32, clock: Option<Clock>, running: Option<u32>) -> Table {
        let state = GameState {
            status: "active".into(),
            impulse_seat: running,
            ..GameState::default()
        };
        Table::new(Opened {
            room_code: "AB12CD".into(),
            info: GameInfo {
                id: "g".into(),
                name: "A game".into(),
                seats: self::seats(seats),
                board: Board {
                    kind: "grid".into(),
                    positions: vec![],
                    boards: vec![],
                    regions: Default::default(),
                },
                actions: vec![],
                goals: vec![],
                pieces: vec![],
                counters: vec![],
                opening: None,
                reserves: Default::default(),
                cards: None,
                engine: String::new(),
                clients: Vec::new(),
                endings: Default::default(),
            },
            game: Game {
                game_def_id: "g".into(),
                status: "active".into(),
                invite_url: String::new(),
                clock,
                impulse_deadline: String::new(),
                server_time: String::new(),
                move_time_seconds: None,
                lobby: false,
                host_seat: None,
                reservations: vec![],
                seats: vec![],
            },
            state,
            bots: vec![],
        })
    }

    fn app(table: Table) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ChessClockPlugin { limited: false },
        ))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_resource::<WatchStatus>()
        .insert_resource(BoardOutline(rectangle(Vec2::splat(0.15))));
        app.world_mut().spawn((
            Footing {
                surface: 0.016,
                tallest: 0.04,
                pitch: 0.028,
            },
            Transform::default(),
        ));
        app.world_mut().resource_mut::<WatchStatus>().table = Some(table);
        app
    }

    fn clocks(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&Watched>().iter(world).count()
    }

    #[test]
    fn the_watched_game_has_a_clock_its_hands_and_lamps_showing_its_times() {
        // The first seat has two hours, its time running; the second a minute.
        let timed = clock(&[(1, 7200.0), (2, 60.0)], Some(1));
        let mut app = app(table(2, Some(timed), Some(1)));
        app.update();
        app.update();
        assert_eq!(clocks(&mut app), 1);
        let world = app.world_mut();
        let showing = *world
            .query_filtered::<&Showing, With<Watched>>()
            .single(world)
            .unwrap();
        assert_eq!(showing.running, Some(End::Right));
        assert!((showing.right - 7200.0).abs() < 1.0 && showing.left == 60.0);
        // Its hour hand at ten, and the lamp under its face lit, the other not.
        let mut parts = world.query::<(&Moving, &Transform)>();
        for (part, at) in parts.iter(world) {
            let turned = (part.rest.rotation.inverse() * at.rotation)
                .to_scaled_axis()
                .z;
            match (part.kind, part.end) {
                (Kind::Hour, End::Right) => {
                    assert!(near(-turned, 10.0 / 12.0 * TAU), "{turned}")
                }
                (Kind::Minute, End::Left) => assert!(near(-turned, -MINUTE), "{turned}"),
                _ => {}
            }
        }
        let mut lamps = world.query::<(&Lamp, &MeshMaterial3d<StandardMaterial>)>();
        let lit: Vec<End> = lamps
            .iter(world)
            .filter(|(lamp, m)| m.0 == lamp.lit)
            .map(|(lamp, _)| lamp.end)
            .collect();
        assert_eq!(lit, [End::Right]);

        // Left out: gone, and back when it is wanted again.
        app.world_mut().resource_mut::<ChessClockOn>().0 = false;
        app.update();
        assert_eq!(clocks(&mut app), 0);
        app.world_mut().resource_mut::<ChessClockOn>().0 = true;
        app.update();
        assert_eq!(clocks(&mut app), 1);
        // A game over: no lamp is lit.
        app.world_mut()
            .resource_mut::<WatchStatus>()
            .table
            .as_mut()
            .unwrap()
            .state
            .status = "finished".into();
        app.update();
        let world = app.world_mut();
        let mut lamps = world.query::<(&Lamp, &MeshMaterial3d<StandardMaterial>)>();
        assert!(lamps.iter(world).all(|(lamp, m)| m.0 == lamp.dark));
    }

    #[test]
    fn a_game_of_three_seats_or_timed_by_the_move_has_none() {
        let three = clock(&[(1, 60.0), (2, 60.0), (3, 60.0)], Some(1));
        let mut app = app(table(3, Some(three), Some(1)));
        app.update();
        app.update();
        assert_eq!(clocks(&mut app), 0);
        let mut app = self::app(table(2, None, Some(1)));
        app.update();
        app.update();
        assert_eq!(clocks(&mut app), 0);
    }
}
