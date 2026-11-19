//! The board and pieces of the game being watched, built from the game definition and the
//! position alone (Bingle's ADR 0124): the board's kind and its positions' layout coordinates say
//! where every cell is, and each stack's pieces are drawn in their seat's colour.
//!
//! Nothing here names a game. A hex board's cells are hexagons and a grid's squares, and any other
//! board's positions are round spots; the same code draws every board of each kind.
//!
//! While this client's seat is on impulse, the positions a click would be taken on are lit, and
//! the move being built is shown as see-through pieces in the seat's colour: where its selections
//! put something, and under the pointer, where a click would.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

use bevy::picking::Pickable;
use bevy::prelude::*;

use crate::look::seat_colour;
use crate::net::WatchStatus;
use crate::net::api::{Board, BoardPosition, GameInfo, PieceOnBoard};
use crate::play::{Hovering, Spot, Stage};
use crate::table::Table;

pub struct BoardPlugin;

impl Plugin for BoardPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<BoardBuilt>().add_systems(
            Update,
            (
                sync_board.run_if(resource_changed::<WatchStatus>),
                show_turn
                    .run_if(resource_changed::<WatchStatus>.or_eager(resource_changed::<Hovering>)),
                drop_pieces,
            )
                .chain(),
        );
    }
}

/// Metres between neighbouring cells, unless the board would then be too big for the table.
pub const PITCH: f32 = 0.028;
/// The widest a board may be, so a large one still fits in front of the player.
pub const MAX_SIZE: f32 = 0.5;

/// The base the cells stand on, and the cells on it.
const BASE_HEIGHT: f32 = 0.016;
const CELL_HEIGHT: f32 = 0.003;
/// The top of the cells, where pieces stand.
pub const SURFACE: f32 = BASE_HEIGHT + CELL_HEIGHT;

/// A board has been put on the table, centred on the origin: the camera may frame it.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct BoardBuilt {
    /// Its width (X) and depth (Z), in metres.
    pub size: Vec2,
}

/// Where every position of a board is on the table.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Metres per layout unit: the distance between neighbours.
    pub pitch: f32,
    /// Each position's centre on the table plane (y = 0), the board centred on the origin.
    pub centres: HashMap<String, Vec3>,
    /// The width (X) and depth (Z) its cells cover.
    pub size: Vec2,
}

impl Layout {
    /// Lays a board's positions out on the table: layout x to the right (+X), layout y (which
    /// grows downwards) towards the player (+Z), so the top of the board is the far side.
    pub fn new(positions: &[BoardPosition]) -> Self {
        let (min, max) = positions.iter().fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(lo, hi), p| {
                let v = Vec2::new(p.x, p.y);
                (lo.min(v), hi.max(v))
            },
        );
        if positions.is_empty() {
            return Layout {
                pitch: PITCH,
                centres: HashMap::new(),
                size: Vec2::ZERO,
            };
        }
        // One unit more than the span: the cells at the edges are a whole cell wide.
        let span = max - min + 1.0;
        let pitch = PITCH.min(MAX_SIZE / span.max_element());
        let middle = (min + max) / 2.0;
        let centres = positions
            .iter()
            .map(|p| {
                let v = (Vec2::new(p.x, p.y) - middle) * pitch;
                (p.id.clone(), Vec3::new(v.x, 0.0, v.y))
            })
            .collect();
        Layout {
            pitch,
            centres,
            size: span * pitch,
        }
    }
}

/// The shape of a board's cells, from its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellShape {
    Hexagon,
    Square,
    Spot,
}

impl CellShape {
    pub fn of(board: &Board) -> Self {
        match board.kind.as_str() {
            "hex" => CellShape::Hexagon,
            "grid" => CellShape::Square,
            _ => CellShape::Spot,
        }
    }

    /// A cell's mesh, standing on y = 0, `scale` of the full size that would touch its neighbours.
    fn mesh(self, pitch: f32, scale: f32, height: f32) -> Mesh {
        let size = pitch * scale;
        match self {
            // Neighbours are one pitch apart across flat sides, so the hexagon's inradius is half
            // a pitch. Its points are towards the far and near sides, as the layout's rows are.
            CellShape::Hexagon => {
                Extrusion::new(RegularPolygon::new(size / 3f32.sqrt(), 6), height)
                    .mesh()
                    .build()
                    .rotated_by(Quat::from_rotation_x(-FRAC_PI_2))
            }
            CellShape::Square => Cuboid::new(size, height, size).mesh().build(),
            CellShape::Spot => Cylinder::new(size * 0.3, height).mesh().build(),
        }
        .translated_by(Vec3::Y * height / 2.0)
    }
}

/// What the scene shows, so the board is built again only when the game changes.
#[derive(Component)]
struct BoardRoot {
    room_code: String,
}

/// A piece on the board, by what it is and where.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct PieceAt {
    cell: String,
    layer: usize,
    piece: PieceOnBoard,
}

/// A cell of the board, which is lit while a click on it would be taken.
#[derive(Component)]
struct Cell(String);

/// A see-through piece: part of the move being built, or where a click would put one.
#[derive(Component)]
struct Ghost;

/// The mark on the cell of the last move.
#[derive(Component)]
struct LastMoveMark;

/// A piece on its way down to where it was played.
#[derive(Component)]
struct Dropping {
    timer: Timer,
    to: f32,
}

const DROP_HEIGHT: f32 = 0.06;
const DROP_TIME: Duration = Duration::from_millis(280);

/// The meshes and materials of the board being shown.
#[derive(Component)]
struct Kit {
    stone: Handle<Mesh>,
    stone_height: f32,
    materials: HashMap<String, Handle<StandardMaterial>>,
    cell: Handle<StandardMaterial>,
    /// A cell a click would be taken on.
    target: Handle<StandardMaterial>,
    /// A piece of the move being built, in this client's seat's colour.
    ghost: Handle<StandardMaterial>,
}

fn sync_board(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    // Together, to keep the argument count within clippy's limit.
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    roots: Query<(Entity, &BoardRoot, &Kit)>,
    pieces: Query<(Entity, &PieceAt)>,
    mut marks: Query<(&mut Transform, &mut Visibility), With<LastMoveMark>>,
    mut built: MessageWriter<BoardBuilt>,
) {
    let Some(table) = &watch.table else {
        for (root, ..) in &roots {
            commands.entity(root).despawn();
        }
        return;
    };
    let current = roots
        .iter()
        .find(|(_, shown, _)| shown.room_code == table.room_code);
    let Some((root, _, kit)) = current else {
        for (old, ..) in &roots {
            commands.entity(old).despawn();
        }
        let layout = Layout::new(&table.info.board.positions);
        let kit = build_board(&mut commands, &mut meshes, &mut materials, table, &layout);
        built.write(BoardBuilt { size: layout.size });
        // A board just built shows the position as it is, without dropping every piece.
        let root = kit.0;
        place_pieces(&mut commands, root, &kit.1, &layout, table, &[], false);
        commands.entity(root).insert(kit.1);
        return;
    };

    let layout = Layout::new(&table.info.board.positions);
    let shown: Vec<(Entity, &PieceAt)> = pieces.iter().collect();
    place_pieces(&mut commands, root, kit, &layout, table, &shown, true);
    for (mut t, mut vis) in &mut marks {
        match table.last_move.as_ref().and_then(|c| layout.centres.get(c)) {
            Some(centre) => {
                t.translation = *centre + Vec3::Y * (SURFACE + 0.0005);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

/// The pieces the position has and the scene does not are added (dropping in when `animate`),
/// and those the scene has and the position does not are taken away.
fn place_pieces(
    commands: &mut Commands,
    root: Entity,
    kit: &Kit,
    layout: &Layout,
    table: &Table,
    shown: &[(Entity, &PieceAt)],
    animate: bool,
) {
    let wanted = wanted_pieces(table);
    for (entity, at) in shown {
        if !wanted.contains(at) {
            commands.entity(*entity).despawn();
        }
    }
    for at in wanted {
        if shown.iter().any(|(_, s)| **s == at) {
            continue;
        }
        let Some(centre) = layout.centres.get(&at.cell) else {
            // A piece on a position the board does not have: not a board cell (a reserve, say),
            // which this stage does not draw yet.
            continue;
        };
        let rest = SURFACE + kit.stone_height * (at.layer as f32 + 0.5);
        let material = kit
            .materials
            .get(&at.piece.color)
            .cloned()
            .unwrap_or_default();
        let start = if animate { rest + DROP_HEIGHT } else { rest };
        let mut piece = commands.spawn((
            Name::new(format!("{} at {}", at.piece.piece, at.cell)),
            Mesh3d(kit.stone.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(Vec3::new(centre.x, start, centre.z)),
            // A click on a piece is a click on where it stands.
            Spot(at.cell.clone()),
            Pickable::default(),
            at,
            ChildOf(root),
        ));
        if animate {
            piece.insert(Dropping {
                timer: Timer::new(DROP_TIME, TimerMode::Once),
                to: rest,
            });
        }
    }
}

/// Every piece of the position, bottom of each stack first.
fn wanted_pieces(table: &Table) -> Vec<PieceAt> {
    table
        .state
        .state
        .cells
        .iter()
        .flat_map(|(cell, stack)| {
            stack.iter().enumerate().map(|(layer, piece)| PieceAt {
                cell: cell.clone(),
                layer,
                piece: piece.clone(),
            })
        })
        .collect()
}

fn build_board(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    table: &Table,
    layout: &Layout,
) -> (Entity, Kit) {
    let shape = CellShape::of(&table.info.board);
    let pitch = layout.pitch;
    let base_mesh = meshes.add(shape.mesh(pitch, 1.0, BASE_HEIGHT));
    let cell_mesh = meshes.add(shape.mesh(pitch, 0.9, CELL_HEIGHT));
    let base = materials.add(StandardMaterial {
        base_color: Color::srgb(0.42, 0.30, 0.20),
        perceptual_roughness: 0.8,
        ..default()
    });
    let cell = materials.add(StandardMaterial {
        base_color: Color::srgb(0.86, 0.80, 0.68),
        perceptual_roughness: 0.7,
        ..default()
    });
    let target = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.92, 0.80),
        emissive: LinearRgba::rgb(0.10, 0.12, 0.05),
        perceptual_roughness: 0.6,
        ..default()
    });

    let root = commands
        .spawn((
            Name::new(format!("Board of {}", table.info.name)),
            BoardRoot {
                room_code: table.room_code.clone(),
            },
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    // Round spots stand on one slab; hexagons and squares make their own base, which keeps the
    // board's outline (a hex board's rhombus, say).
    if shape == CellShape::Spot {
        let (lo, hi) = layout.centres.values().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), c| (lo.min(*c), hi.max(*c)),
        );
        if lo.x.is_finite() {
            let size = hi - lo + Vec3::splat(pitch);
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(size.x, BASE_HEIGHT, size.z))),
                MeshMaterial3d(base.clone()),
                Transform::from_translation((lo + hi) / 2.0 + Vec3::Y * BASE_HEIGHT / 2.0),
                ChildOf(root),
            ));
        }
    }
    for (id, centre) in &layout.centres {
        if shape != CellShape::Spot {
            commands.spawn((
                Mesh3d(base_mesh.clone()),
                MeshMaterial3d(base.clone()),
                Transform::from_translation(*centre),
                ChildOf(root),
            ));
        }
        commands.spawn((
            Name::new(format!("Cell {id}")),
            Mesh3d(cell_mesh.clone()),
            MeshMaterial3d(cell.clone()),
            Transform::from_translation(*centre + Vec3::Y * BASE_HEIGHT),
            Cell(id.clone()),
            Spot(id.clone()),
            Pickable::default(),
            ChildOf(root),
        ));
    }

    let mark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.98, 0.84, 0.30),
        emissive: LinearRgba::rgb(0.6, 0.45, 0.1),
        ..default()
    });
    commands.spawn((
        Name::new("Last move"),
        LastMoveMark,
        Mesh3d(meshes.add(Torus::new(pitch * 0.40, pitch * 0.47))),
        MeshMaterial3d(mark),
        Transform::default(),
        Visibility::Hidden,
        ChildOf(root),
    ));

    let stone_height = pitch * 0.28;
    let yours = table.yours();
    let ghost = materials.add(StandardMaterial {
        base_color: seat_colour(
            yours.map_or("", |s| s.color.as_str()),
            yours.map_or(1, |s| s.seat),
        )
        .with_alpha(0.5),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.35,
        ..default()
    });
    let kit = Kit {
        stone: meshes.add(Cylinder::new(pitch * 0.40, stone_height)),
        stone_height,
        materials: seat_materials(&table.info, materials),
        cell,
        target,
        ghost,
    };
    (root, kit)
}

/// One material per seat colour word. A piece in a colour no seat plays is drawn in the default
/// material, which no seat uses.
fn seat_materials(
    info: &GameInfo,
    materials: &mut Assets<StandardMaterial>,
) -> HashMap<String, Handle<StandardMaterial>> {
    info.seats
        .iter()
        .map(|s| {
            let handle = materials.add(StandardMaterial {
                base_color: seat_colour(&s.color, s.seat),
                perceptual_roughness: 0.35,
                ..default()
            });
            (s.color.clone(), handle)
        })
        .collect()
}

/// Lights the cells a click would be taken on, and shows the move being built.
fn show_turn(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    hovering: Res<Hovering>,
    roots: Query<(Entity, &Kit), With<BoardRoot>>,
    mut cells: Query<(&Cell, &mut MeshMaterial3d<StandardMaterial>)>,
    ghosts: Query<Entity, With<Ghost>>,
) {
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
    let (Some(table), Ok((root, kit))) = (&watch.table, roots.single()) else {
        return;
    };
    let turn = watch.turn.as_ref();
    let targets = turn.map(|t| t.targets()).unwrap_or_default();
    for (cell, mut material) in &mut cells {
        let wanted = if targets.contains(&cell.0) {
            &kit.target
        } else {
            &kit.cell
        };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }
    let Some(turn) = turn else { return };
    let mut shown: Vec<&str> = match turn.stage {
        Stage::Asking | Stage::Ready | Stage::Sending => turn.placed().collect(),
        Stage::Choosing | Stage::Failed => Vec::new(),
    };
    if let Some(cell) = hovering.0.as_deref().filter(|c| targets.contains(*c)) {
        shown.push(cell);
    }
    let layout = Layout::new(&table.info.board.positions);
    for cell in shown {
        let Some(centre) = layout.centres.get(cell) else {
            continue;
        };
        let below = table.state.state.cells.get(cell).map_or(0, Vec::len);
        let y = SURFACE + kit.stone_height * (below as f32 + 0.5);
        commands.spawn((
            Name::new(format!("Ghost at {cell}")),
            Ghost,
            Mesh3d(kit.stone.clone()),
            MeshMaterial3d(kit.ghost.clone()),
            Transform::from_translation(Vec3::new(centre.x, y, centre.z)),
            ChildOf(root),
        ));
    }
}

fn drop_pieces(
    mut commands: Commands,
    time: Res<Time>,
    mut pieces: Query<(Entity, &mut Dropping, &mut Transform)>,
) {
    for (entity, mut drop, mut t) in &mut pieces {
        drop.timer.tick(time.delta());
        let f = drop.timer.fraction();
        // Falls faster and faster, as a dropped stone does.
        t.translation.y = drop.to + DROP_HEIGHT * (1.0 - f * f);
        if drop.timer.is_finished() {
            t.translation.y = drop.to;
            commands.entity(entity).remove::<Dropping>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(id: &str, x: f32, y: f32) -> BoardPosition {
        BoardPosition {
            id: id.into(),
            x,
            y,
        }
    }

    /// A 3×3 hex board's positions as Bingle lays them out: each row half a cell to the right of
    /// the one above, rows √3/2 apart.
    fn hex3() -> Vec<BoardPosition> {
        let mut out = Vec::new();
        for r in 0..3 {
            for q in 0..3 {
                let id = format!("{}{}", (b'a' + q) as char, r + 1);
                out.push(pos(&id, q as f32 + r as f32 / 2.0, r as f32 * 0.866_025_4));
            }
        }
        out
    }

    #[test]
    fn neighbours_are_one_pitch_apart_and_the_board_is_centred() {
        let layout = Layout::new(&hex3());
        assert_eq!(layout.pitch, PITCH);
        let c = |id: &str| layout.centres[id];
        assert!((c("a1").distance(c("b1")) - PITCH).abs() < 1e-5);
        assert!(
            (c("a1").distance(c("a2")) - PITCH).abs() < 1e-5,
            "hex neighbours too"
        );
        assert!(c("b2").length() < 1e-5, "the middle cell is at the origin");
        assert!(c("a1").z < c("a3").z, "the top row is on the far side");
        assert!(c("a1").x < c("c1").x, "columns run to the right");
        assert!(layout.centres.values().all(|v| v.y == 0.0));
        // Three columns plus the last row's shift of one cell; three rows √3/2 apart.
        let expected = Vec2::new(4.0, 1.0 + 2.0 * 0.866_025_4) * PITCH;
        assert!(layout.size.distance(expected) < 1e-5, "{}", layout.size);
    }

    #[test]
    fn a_big_board_is_shrunk_to_fit_the_table() {
        let positions: Vec<_> = (0..40)
            .flat_map(|y| (0..40).map(move |x| pos(&format!("{x},{y}"), x as f32, y as f32)))
            .collect();
        let layout = Layout::new(&positions);
        assert!(layout.pitch < PITCH);
        let xs = layout.centres.values().map(|v| v.x);
        let width = xs.clone().fold(f32::MIN, f32::max) - xs.fold(f32::MAX, f32::min);
        assert!(width + layout.pitch <= MAX_SIZE + 1e-4, "{width}");
    }

    mod systems {
        use super::*;
        use crate::net::api::{Event, Game, GameState, Position, SeatDef};
        use crate::net::session::Opened;

        fn app() -> App {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, AssetPlugin::default(), BoardPlugin))
                .init_asset::<Mesh>()
                .init_asset::<StandardMaterial>()
                .init_resource::<WatchStatus>()
                .init_resource::<Hovering>();
            app
        }

        fn state(seq: u64, cells: &[&str]) -> GameState {
            GameState {
                state: Position {
                    cells: cells
                        .iter()
                        .map(|c| {
                            let stone = PieceOnBoard {
                                piece: "stone".into(),
                                color: "red".into(),
                            };
                            (c.to_string(), vec![stone])
                        })
                        .collect(),
                },
                status: "active".into(),
                move_seq: seq,
                ..Default::default()
            }
        }

        fn table() -> Table {
            Table::new(Opened {
                room_code: "AB12CD".into(),
                info: GameInfo {
                    id: "g".into(),
                    name: "A game".into(),
                    seats: vec![SeatDef {
                        seat: 1,
                        color: "red".into(),
                        label: "Red".into(),
                    }],
                    board: Board {
                        kind: "hex".into(),
                        positions: hex3(),
                    },
                    actions: vec![],
                },
                game: Game {
                    game_def_id: "g".into(),
                    seats: vec![],
                },
                state: state(1, &["a1"]),
            })
        }

        fn pieces(app: &mut App) -> Vec<(String, bool)> {
            let mut q = app.world_mut().query::<(&PieceAt, Option<&Dropping>)>();
            let mut out: Vec<_> = q
                .iter(app.world())
                .map(|(p, d)| (p.cell.clone(), d.is_some()))
                .collect();
            out.sort();
            out
        }

        #[test]
        fn a_table_is_built_framed_and_follows_the_moves() {
            let mut app = app();
            app.world_mut().resource_mut::<WatchStatus>().table = Some(table());
            app.update();
            let built: Vec<BoardBuilt> = app
                .world_mut()
                .resource_mut::<Messages<BoardBuilt>>()
                .drain()
                .collect();
            assert_eq!(built.len(), 1, "the camera is told about the new board");
            assert_eq!(
                pieces(&mut app),
                [("a1".to_string(), false)],
                "the position as it was, not dropping"
            );

            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let t = watch.table.as_mut().unwrap();
            t.apply(&Event::Move {
                seq: 2,
                state: state(2, &["a1", "b2"]),
            });
            app.update();
            assert_eq!(
                pieces(&mut app),
                [("a1".to_string(), false), ("b2".to_string(), true)],
                "only the new stone drops in"
            );
            assert!(
                app.world_mut()
                    .resource_mut::<Messages<BoardBuilt>>()
                    .drain()
                    .next()
                    .is_none(),
                "the same board is not built again"
            );

            app.world_mut().resource_mut::<WatchStatus>().table = None;
            app.update();
            assert!(pieces(&mut app).is_empty(), "the board goes with the game");
            let mut roots = app.world_mut().query::<&BoardRoot>();
            assert_eq!(roots.iter(app.world()).count(), 0);
        }
    }

    #[test]
    fn an_empty_board_lays_out_nothing() {
        assert!(Layout::new(&[]).centres.is_empty());
    }

    #[test]
    fn the_cell_shape_comes_from_the_board_kind() {
        let board = |kind: &str| Board {
            kind: kind.into(),
            positions: vec![],
        };
        assert_eq!(CellShape::of(&board("hex")), CellShape::Hexagon);
        assert_eq!(CellShape::of(&board("grid")), CellShape::Square);
        assert_eq!(CellShape::of(&board("explicit")), CellShape::Spot);
        assert_eq!(CellShape::of(&board("something-new")), CellShape::Spot);
    }

    #[test]
    fn a_hexagon_cell_touches_its_neighbours_across_flat_sides() {
        let mesh = CellShape::Hexagon.mesh(1.0, 1.0, 0.1);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(points)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the mesh has positions");
        };
        let (lo, hi) = points.iter().map(|p| Vec3::from_array(*p)).fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(p), hi.max(p)),
        );
        let size = hi - lo;
        assert!(
            (size.x - 1.0).abs() < 1e-4,
            "flat sides one pitch apart: {size}"
        );
        assert!(
            (size.z - 2.0 / 3f32.sqrt()).abs() < 1e-4,
            "points far and near: {size}"
        );
        assert!(lo.y.abs() < 1e-5 && (hi.y - 0.1).abs() < 1e-5);
    }
}
