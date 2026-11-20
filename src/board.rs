//! The board and pieces of the game being watched, built from the game definition and the
//! position alone (Bingle's ADR 0124): the board's kind and its positions' layout coordinates say
//! where every cell is, and each stack's pieces are drawn in their seat's colour.
//!
//! Nothing here names a game. A hex board's cells are hexagons and a grid's squares, and any other
//! board's positions are round spots; the same code draws every board of each kind.
//!
//! How it looks comes from `look.rs`: one slab of wood with a rounded rim (`slab.rs`), the cells
//! cut into it with dark lines between them, and stones in each seat's colour. Along each goal's
//! edge the rim is stained in the seat's colour and the cells are shaded towards it. A game's
//! appearance file (`appearance.rs`) may choose the pieces' model, what each seat's pieces and rim
//! are made of, and its colour. The board's wood is the player's choice (`BoardWood`).
//!
//! A seat's goal is drawn from the game's `goals` and the board's `regions`: a strip in the seat's
//! colour along the outer sides of each region the seat must connect (a hex board's edges), and a
//! neutral one for a goal every seat shares. Only `connect` goals are drawn; any other type is
//! skipped, as the API asks.
//!
//! While this client's seat is on impulse, the positions a click would be taken on are lit, and
//! the move being built is shown as see-through pieces in the seat's colour: where its selections
//! put something, and under the pointer, where a click would.

use std::collections::{BTreeMap, HashMap};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

use bevy::ecs::system::SystemParam;
use bevy::gltf::GltfAssetLabel;
use bevy::picking::Pickable;
use bevy::prelude::*;

use crate::appearance::{Appearance, Appearances, PlayOn};
use crate::look::{self, MaterialSpec, seat_colour};
use crate::net::WatchStatus;
use crate::net::api::{Board, BoardPosition, GameInfo, PieceOnBoard};
use crate::play::{Hovering, Spot, Stage};
use crate::slab::{self, Rim};
use crate::table::Table;

pub struct BoardPlugin;

impl Plugin for BoardPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<BoardBuilt>()
            .add_systems(
                Update,
                (
                    sync_board.run_if(resource_changed::<WatchStatus>),
                    show_turn.run_if(
                        resource_changed::<WatchStatus>.or_eager(resource_changed::<Hovering>),
                    ),
                    drop_pieces,
                    change_wood.run_if(resource_changed::<BoardWood>),
                )
                    .chain(),
            )
            .init_resource::<BoardWood>();
    }
}

/// Metres between neighbouring cells, unless the board would then be too big for the table.
pub const PITCH: f32 = 0.028;
/// The widest a board may be, so a large one still fits in front of the player.
pub const MAX_SIZE: f32 = 0.5;

/// The slab's top, and the cells cut into it, standing a little proud of the lines between them.
pub const BOARD_HEIGHT: f32 = 0.03;
const CELL_HEIGHT: f32 = 0.0012;
/// The top of the cells, where pieces stand.
pub const SURFACE: f32 = BOARD_HEIGHT + CELL_HEIGHT;
/// How much of a cell is left between the lines.
const CELL_SCALE: f32 = 0.955;
/// The wood round the cells, as a share of the pitch (room for the coordinates), and how round
/// the slab's top edge is.
const MARGIN: f32 = 0.85;
const RIM_RADIUS: f32 = 0.005;
/// How dark the shading of a goal's cells is at the edge, and how far in it fades (in pitches):
/// a third of the way across the cell.
const SHADE_STRENGTH: f32 = 0.75;
const SHADE_FADE: f32 = CELL_SCALE / 3.0;

/// The wood beyond the outermost lines of a board played on its points, in pitches: room for a
/// stone on the edge and the coordinates beyond it.
const POINTS_MARGIN: f32 = 1.2;
/// A points board's lines, and its edge's (thicker, as on a Go board), in pitches.
const LINE_WIDTH: f32 = 0.035;
const EDGE_LINE_WIDTH: f32 = 0.07;
/// A marked position's dot (a Go board's star points), across, in pitches.
const MARK_SIZE: f32 = 0.2;

/// What a game is played on: its board's spaces (cells of a shape), or the points where its
/// lines cross. The server says which positions there are and where; which of these they are
/// drawn as is this client's choice (Bingle's ADR 0124), the game's appearance saying `points`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Cells(CellShape),
    Points,
}

impl Surface {
    pub fn of(board: &Board, look: Option<&Appearance>) -> Self {
        match look.map(|l| l.board.on) {
            Some(PlayOn::Points) => Surface::Points,
            _ => Surface::Cells(CellShape::of(board)),
        }
    }

    /// The height pieces stand on.
    pub fn height(self) -> f32 {
        match self {
            Surface::Cells(_) => SURFACE,
            Surface::Points => BOARD_HEIGHT,
        }
    }

    /// How far a position's space reaches from its middle, in pitches: towards the far and near
    /// sides (`along`) or to either side. A point's is a stone's on it.
    pub fn reach(self, along: bool) -> f32 {
        match self {
            Surface::Cells(CellShape::Hexagon) if along => 1.0 / 3f32.sqrt(),
            Surface::Cells(CellShape::Spot) => 0.3,
            Surface::Cells(_) => 0.5,
            Surface::Points => 0.4,
        }
    }

    /// Where the flat of the board ends beyond the outermost position's middle, in pitches: the
    /// margin of wood, short of the rounded rim.
    pub fn flat_to(self, along: bool, pitch: f32) -> f32 {
        let margin = match self {
            Surface::Cells(_) => self.reach(along) + MARGIN,
            Surface::Points => POINTS_MARGIN,
        };
        margin - RIM_RADIUS / pitch
    }

    /// Where a coordinate goes beyond its position's middle, in pitches: half way between the
    /// position's reach and the end of the flat.
    pub fn label_at(self, along: bool, pitch: f32) -> f32 {
        (self.reach(along) + self.flat_to(along, pitch)) / 2.0
    }
}

/// The wood the board is made of: one of `look::BOARD_WOODS`, chosen by the player.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardWood(pub &'static str);

impl Default for BoardWood {
    fn default() -> Self {
        BoardWood(look::BOARD_WOODS[0])
    }
}

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

/// How thick the piece model is (`assets-src/blender/stones.py`), at the full pitch.
const STONE_THICKNESS: f32 = 0.0088;

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
    /// The model is made for the full pitch; a smaller board's pieces are smaller.
    stone_scale: f32,
    materials: HashMap<String, Handle<StandardMaterial>>,
    cell: Handle<StandardMaterial>,
    /// A cell a click would be taken on.
    target: Handle<StandardMaterial>,
    /// A piece of the move being built, in this client's seat's colour.
    ghost: Handle<StandardMaterial>,
    /// The materials made of the board's wood, to build again when another is chosen.
    woodwork: Vec<(Handle<StandardMaterial>, MaterialSpec)>,
    /// The height pieces stand on.
    surface: f32,
}

/// What a board is drawn with: where its meshes and materials go, where its files come from, and
/// the game's own look, if it has one.
struct Tools<'a> {
    server: &'a AssetServer,
    meshes: &'a mut Assets<Mesh>,
    materials: &'a mut Assets<StandardMaterial>,
    look: Option<&'a Appearance>,
    wood: &'static str,
    /// The materials made so far of the board's wood.
    woodwork: Vec<(Handle<StandardMaterial>, MaterialSpec)>,
}

impl Tools<'_> {
    fn material(&mut self, spec: &MaterialSpec) -> Handle<StandardMaterial> {
        let handle = self
            .materials
            .add(spec.on_board(self.wood).build(self.server));
        if spec.is_board_wood() {
            self.woodwork.push((handle.clone(), spec.clone()));
        }
        handle
    }

    /// The colour a seat is shown in: its appearance's, or its own.
    fn seat_colour(&self, colour: &str, seat: u32) -> Color {
        self.look
            .and_then(|l| l.seats.get(colour)?.colour)
            .map_or_else(
                || seat_colour(colour, seat),
                |[r, g, b]| Color::srgb(r, g, b),
            )
    }

    /// What a seat's pieces are made of: the appearance's material, or the seat's colour.
    fn piece_material(&self, colour: &str, seat: u32) -> MaterialSpec {
        self.look
            .and_then(|l| l.material(l.seats.get(colour).and_then(|s| s.pieces.as_ref())))
            .cloned()
            .unwrap_or_else(|| look::piece(seat_colour(colour, seat)))
    }
}

/// The resources a board is drawn with, as one system parameter.
#[derive(SystemParam)]
struct Drawing<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    server: Res<'w, AssetServer>,
    appearances: Res<'w, Appearances>,
    wood: Res<'w, BoardWood>,
}

fn sync_board(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    mut drawing: Drawing,
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
        let Drawing {
            meshes,
            materials,
            server,
            appearances,
            wood,
        } = &mut drawing;
        let mut tools = Tools {
            server,
            meshes,
            materials,
            look: appearances.for_game(&table.info.id),
            wood: wood.0,
            woodwork: Vec::new(),
        };
        let kit = build_board(&mut commands, &mut tools, table, &layout);
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
                t.translation = *centre + Vec3::Y * (kit.surface + 0.0005);
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
        let rest = kit.surface + kit.stone_height * (at.layer as f32 + 0.5);
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
            Transform::from_translation(Vec3::new(centre.x, start, centre.z))
                .with_rotation(lie(&at.cell, at.layer))
                .with_scale(Vec3::splat(kit.stone_scale * size(&at.cell, at.layer))),
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

/// A number from 0 to 1 for a place on the board and what it is for: the same for the same
/// place, and different from place to place. FNV-1a: small, and the same everywhere.
fn chance(cell: &str, layer: usize, what: u8) -> f32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in cell.bytes().chain([layer as u8, what]) {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    // FNV's low bits are its weakest; fold the high ones in.
    h ^= h >> 15;
    (h % 10_000) as f32 / 10_000.0
}

/// How far a piece is turned about its axis, so no two stones' lines lie the same way.
fn yaw(cell: &str, layer: usize) -> f32 {
    chance(cell, layer, 0) * TAU
}

/// How a piece lies: turned about its axis, and leaning a degree or so, as no two stones put
/// down by hand lie quite alike.
fn lie(cell: &str, layer: usize) -> Quat {
    let lean_towards = chance(cell, layer, 1) * TAU;
    let lean = (chance(cell, layer, 2) * 1.2).to_radians();
    let axis = Vec3::new(lean_towards.cos(), 0.0, lean_towards.sin());
    Quat::from_axis_angle(axis, lean) * Quat::from_rotation_y(yaw(cell, layer))
}

/// How big a piece is, against the model: within 2 % either way, as stones are.
fn size(cell: &str, layer: usize) -> f32 {
    1.0 + (chance(cell, layer, 3) - 0.5) * 0.04
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
    tools: &mut Tools,
    table: &Table,
    layout: &Layout,
) -> (Entity, Kit) {
    let shape = CellShape::of(&table.info.board);
    let surface = Surface::of(&table.info.board, tools.look);
    let pitch = layout.pitch;
    // One material for the board's whole top, its cells and its margin alike, so the spaces are
    // the same wood in the same light as the wood round them.
    let wood = tools.material(&look::board());

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

    // The slab: round every cell, or every point, with a margin of wood and a rounded top edge.
    let centres: Vec<Vec2> = layout
        .centres
        .values()
        .map(|c| Vec2::new(c.x, c.z))
        .collect();
    let corners = slab::cell_corners(shape, pitch);
    let outline = match surface {
        Surface::Cells(_) => {
            let points: Vec<Vec2> = centres
                .iter()
                .flat_map(|c| corners.iter().map(move |k| *c + *k))
                .collect();
            slab::grown(&slab::hull(&points), pitch * MARGIN, 6)
        }
        Surface::Points => slab::grown(&slab::hull(&centres), pitch * POINTS_MARGIN, 6),
    };
    let rim = Rim {
        height: BOARD_HEIGHT,
        radius: RIM_RADIUS,
        steps: 5,
    };
    let strips = goal_strips(&table.info, layout, shape);
    if outline.len() >= 3 {
        commands.spawn((
            Name::new("Board"),
            Mesh3d(
                tools
                    .meshes
                    .add(look::world_uvs(slab::top(&outline, &rim), Vec3::ZERO)),
            ),
            MeshMaterial3d(wood.clone()),
            ChildOf(root),
        ));
        // Its sides, each stretch in the wood of the goal it runs along, if any.
        let mut by_owner: BTreeMap<Option<Option<u32>>, Vec<usize>> = BTreeMap::new();
        for i in 0..outline.len() {
            by_owner
                .entry(rim_owner(&outline, i, &strips))
                .or_default()
                .push(i);
        }
        for (owner, edges) in by_owner {
            let material = match owner {
                None => wood.clone(),
                Some(seat) => {
                    let spec = goal_material(tools, table, seat);
                    tools.material(&spec)
                }
            };
            commands.spawn((
                Name::new("Board's edge"),
                Mesh3d(tools.meshes.add(look::world_uvs(
                    slab::side(&outline, &rim, &edges),
                    Vec3::ZERO,
                ))),
                MeshMaterial3d(material),
                ChildOf(root),
            ));
        }
    }

    let lines = tools.material(&look::groove());
    let (cell, target) = match surface {
        Surface::Cells(_) => {
            // The lines between the cells: the whole of each cell, darkened, under the cells.
            commands.spawn((
                Name::new("Lines"),
                Mesh3d(tools.meshes.add(look::world_uvs(
                    slab::flat_cells(&centres, &corners, BOARD_HEIGHT + 0.0001),
                    Vec3::ZERO,
                ))),
                MeshMaterial3d(lines),
                ChildOf(root),
            ));
            let target = tools.material(&look::lit_cell());
            let cell_mesh = shape.mesh(pitch, CELL_SCALE, CELL_HEIGHT);
            for (id, centre) in &layout.centres {
                let at = *centre + Vec3::Y * BOARD_HEIGHT;
                commands.spawn((
                    Name::new(format!("Cell {id}")),
                    Mesh3d(tools.meshes.add(look::world_uvs(cell_mesh.clone(), at))),
                    MeshMaterial3d(wood.clone()),
                    Transform::from_translation(at),
                    Cell(id.clone()),
                    Spot(id.clone()),
                    Pickable::default(),
                    ChildOf(root),
                ));
            }
            (wood.clone(), target)
        }
        Surface::Points => {
            // The lines, cut into the board between neighbouring points.
            commands.spawn((
                Name::new("Lines"),
                Mesh3d(tools.meshes.add(look::world_uvs(
                    slab::grid_lines(
                        &centres,
                        pitch,
                        pitch * LINE_WIDTH,
                        pitch * EDGE_LINE_WIDTH,
                        BOARD_HEIGHT + 0.0001,
                    ),
                    Vec3::ZERO,
                ))),
                MeshMaterial3d(lines),
                ChildOf(root),
            ));
            // Each point is a disc a click lands on: unseen, but glowing softly while a click
            // there would be taken.
            let unseen = tools.materials.add(StandardMaterial {
                base_color: Color::NONE,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            });
            let glowing = tools.materials.add(StandardMaterial {
                base_color: Color::srgba(1.0, 0.86, 0.5, 0.22),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            });
            let disc = tools.meshes.add(
                Circle::new(pitch * 0.42)
                    .mesh()
                    .build()
                    .rotated_by(Quat::from_rotation_x(-FRAC_PI_2)),
            );
            for (id, centre) in &layout.centres {
                commands.spawn((
                    Name::new(format!("Point {id}")),
                    Mesh3d(disc.clone()),
                    MeshMaterial3d(unseen.clone()),
                    Transform::from_translation(*centre + Vec3::Y * (BOARD_HEIGHT + 0.0003)),
                    Cell(id.clone()),
                    Spot(id.clone()),
                    Pickable::default(),
                    ChildOf(root),
                ));
            }
            (unseen, glowing)
        }
    };

    // Marked positions (a Go board's star points): dots cut as the lines are.
    let marked: Vec<Vec2> = tools
        .look
        .map(|l| l.board.marks(&table.info.id))
        .unwrap_or_default()
        .iter()
        .filter_map(|id| layout.centres.get(id))
        .map(|c| Vec2::new(c.x, c.z))
        .collect();
    if !marked.is_empty() {
        // A round spot's corners are 0.3 of its size from its middle.
        let dot = slab::cell_corners(CellShape::Spot, pitch * MARK_SIZE / 0.6);
        let ink = tools.material(&look::groove());
        commands.spawn((
            Name::new("Marks"),
            Mesh3d(tools.meshes.add(look::world_uvs(
                slab::flat_cells(&marked, &dot, surface.height() + 0.00015),
                Vec3::ZERO,
            ))),
            MeshMaterial3d(ink),
            ChildOf(root),
        ));
    }

    // A goal's cells are shaded towards its edge; a points board has no cells to shade.
    if let Surface::Cells(_) = surface {
        shade_goals(commands, tools, root, table, layout, shape, &strips);
    }

    let mark = tools.materials.add(StandardMaterial {
        base_color: Color::srgb(0.98, 0.84, 0.30),
        emissive: LinearRgba::rgb(0.6, 0.45, 0.1),
        ..default()
    });
    commands.spawn((
        Name::new("Last move"),
        LastMoveMark,
        Mesh3d(tools.meshes.add(Torus::new(pitch * 0.40, pitch * 0.47))),
        MeshMaterial3d(mark),
        Transform::default(),
        Visibility::Hidden,
        ChildOf(root),
    ));

    // Every piece is drawn as one model for now, whatever its kind: the generic stone, or the
    // one the game's appearance gives.
    let model = tools
        .look
        .and_then(|l| l.pieces.values().next())
        .map_or(look::PIECE_MODEL, |p| p.model.as_str());
    let stone = tools.server.load(
        GltfAssetLabel::Primitive {
            mesh: 0,
            primitive: 0,
        }
        .from_asset(crate::assets::path(model)),
    );
    let stone_scale = pitch / PITCH;
    let yours = table.yours();
    let mut ghost = yours
        .map(|s| tools.piece_material(&s.color, s.seat))
        .unwrap_or_else(|| look::piece(seat_colour("", 1)))
        .build(tools.server);
    ghost.base_color = ghost.base_color.with_alpha(0.5);
    ghost.alpha_mode = AlphaMode::Blend;
    let materials = seat_materials(&table.info, tools);
    let ghost = tools.materials.add(ghost);
    let kit = Kit {
        stone,
        stone_height: STONE_THICKNESS * stone_scale,
        stone_scale,
        materials,
        cell,
        target,
        ghost,
        woodwork: std::mem::take(&mut tools.woodwork),
        surface: surface.height(),
    };
    (root, kit)
}

/// Whose goal the slab's edge `i` runs along: the seat (or `None` for a goal every seat shares)
/// of the goal region facing most nearly the way the edge does, among those with a cell side
/// facing that way, or no one's. A rounded corner between two regions' edges is split where it
/// faces both equally; of two regions facing the same way, the nearer has it.
fn rim_owner(outline: &[(Vec2, Vec2)], i: usize, strips: &[GoalStrip]) -> Option<Option<u32>> {
    let (a, na) = outline[i];
    let (b, nb) = outline[(i + 1) % outline.len()];
    let (middle, facing) = ((a + b) / 2.0, (na + nb).normalize_or_zero());
    let score = |s: &GoalStrip| {
        let aligned = (s.outward.dot(facing) * 1000.0).round();
        let near = Vec2::new(s.centre.x, s.centre.z).distance(middle);
        (aligned, -near)
    };
    strips
        .iter()
        .filter(|s| s.normal.dot(facing) > 0.3)
        .max_by(|x, y| {
            let (sx, sy) = (score(x), score(y));
            sx.0.total_cmp(&sy.0).then(sx.1.total_cmp(&sy.1))
        })
        .map(|s| s.seat)
}

/// What the rim along a seat's goal is made of: the appearance's material, or a stain in the
/// seat's colour (a plain dark one for a goal every seat shares).
fn goal_material(tools: &Tools, table: &Table, seat: Option<u32>) -> MaterialSpec {
    let seat = seat.and_then(|s| table.info.seats.iter().find(|d| d.seat == s));
    seat.and_then(|seat| {
        let look = tools.look?;
        look.material(look.seats.get(&seat.color)?.goal.as_ref())
            .cloned()
    })
    .unwrap_or_else(|| look::goal(seat.map(|seat| seat_colour(&seat.color, seat.seat))))
}

/// The outer side of a cell in a region a goal names: where the board shows that goal.
#[derive(Debug, Clone, PartialEq)]
pub struct GoalStrip {
    pub region: String,
    /// The seat whose goal the region is, or `None` when it is more than one seat's.
    pub seat: Option<u32>,
    /// The middle of the strip, on the table plane.
    pub centre: Vec3,
    /// The middle of the cell it is beside.
    pub cell: Vec3,
    /// The way the side faces, out of the board (X and Z).
    pub normal: Vec2,
    /// The way the region faces out of the board: square to the board's edge it lies along.
    pub outward: Vec2,
    pub length: f32,
}

/// How wide a goal strip is, as a share of the pitch.
const STRIP_WIDTH: f32 = 0.16;

/// Where the strips of the game's `connect` goals go. Each region's cells' sides that face off the
/// board are marked; a side of a corner cell that is in two regions (Hex's corners) goes to the
/// region it faces most, so each edge of the board shows one colour.
pub fn goal_strips(info: &GameInfo, layout: &Layout, shape: CellShape) -> Vec<GoalStrip> {
    // Which seat each region is the goal of.
    let mut owners: BTreeMap<&str, Option<u32>> = BTreeMap::new();
    for goal in info.goals.iter().filter(|g| g.kind == "connect") {
        for region in goal.named_regions() {
            let owner = owners.entry(region).or_insert(goal.seat);
            if *owner != goal.seat {
                *owner = None;
            }
        }
    }
    let flat = |v: Vec3| Vec2::new(v.x, v.z);
    let cells: Vec<Vec2> = layout.centres.values().map(|c| flat(*c)).collect();
    if cells.is_empty() {
        return Vec::new();
    }
    let middle = cells.iter().sum::<Vec2>() / cells.len() as f32;

    // Each goal region's cells, and the way it faces out of the board.
    struct Region<'a> {
        name: &'a str,
        seat: Option<u32>,
        cells: Vec<Vec2>,
        outward: Vec2,
    }
    let regions: Vec<Region> = owners
        .iter()
        .filter_map(|(name, seat)| {
            let cells: Vec<Vec2> = info
                .board
                .regions
                .get(*name)?
                .iter()
                .filter_map(|id| layout.centres.get(id).map(|c| flat(*c)))
                .collect();
            if cells.is_empty() {
                return None;
            }
            let outward = outward(&cells, middle)?;
            Some(Region {
                name,
                seat: *seat,
                cells,
                outward,
            })
        })
        .collect();

    let pitch = layout.pitch;
    let (sides, apothem, side_length): (Vec<Vec2>, f32, f32) = match shape {
        CellShape::Hexagon => (
            (0..6)
                .map(|i| Vec2::from_angle(i as f32 * TAU / 6.0))
                .collect(),
            pitch / 2.0,
            pitch / 3f32.sqrt(),
        ),
        CellShape::Square => (
            (0..4)
                .map(|i| Vec2::from_angle(i as f32 * TAU / 4.0))
                .collect(),
            pitch / 2.0,
            pitch,
        ),
        // Spots have no sides: one short strip beside each, facing out of the board.
        CellShape::Spot => (Vec::new(), pitch * 0.3, pitch * 0.5),
    };
    let off_board = |cell: Vec2, side: Vec2| {
        let beyond = cell + side * pitch;
        !cells.iter().any(|c| c.distance(beyond) < pitch * 0.25)
    };

    let mut strips = Vec::new();
    let mut taken: Vec<(Vec2, Vec2)> = Vec::new();
    for region in &regions {
        for &cell in &region.cells {
            let facing: Vec<Vec2> = if shape == CellShape::Spot {
                vec![region.outward]
            } else {
                sides
                    .iter()
                    .copied()
                    .filter(|&side| off_board(cell, side))
                    .collect()
            };
            for side in facing {
                // The region of this cell that the side faces most. A side that faces two
                // equally (a hex board's obtuse corners) goes to the first by name, so the
                // same board is always marked the same way.
                let mut best: Option<(&str, f32)> = None;
                for r in regions
                    .iter()
                    .filter(|r| r.cells.iter().any(|c| c.distance(cell) < 1e-5))
                {
                    let dot = side.dot(r.outward);
                    if best.is_none_or(|(_, d)| dot > d + 1e-3) {
                        best = Some((r.name, dot));
                    }
                }
                let best = best.map(|(name, _)| name);
                if best != Some(region.name) || side.dot(region.outward) < 0.3 {
                    continue;
                }
                if taken.contains(&(cell, side)) {
                    continue;
                }
                taken.push((cell, side));
                let at = cell + side * (apothem + pitch * STRIP_WIDTH / 2.0);
                strips.push(GoalStrip {
                    region: region.name.to_string(),
                    seat: region.seat,
                    centre: Vec3::new(at.x, 0.0, at.y),
                    cell: Vec3::new(cell.x, 0.0, cell.y),
                    outward: region.outward,
                    normal: side,
                    length: side_length,
                });
            }
        }
    }
    strips
}

/// The way a region faces out of the board: square to the line its cells lie along (a board's
/// edge), away from the board's middle. A region that is not a line faces the way its middle
/// lies from the board's.
fn outward(cells: &[Vec2], middle: Vec2) -> Option<Vec2> {
    let n = cells.len() as f32;
    let centre = cells.iter().sum::<Vec2>() / n;
    let away = centre - middle;
    // The covariance of the cells' positions: its main axis is the line they lie along.
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for c in cells {
        let d = *c - centre;
        xx += d.x * d.x;
        xy += d.x * d.y;
        yy += d.y * d.y;
    }
    let mean = (xx + yy) / 2.0;
    let spread = (((xx - yy) / 2.0).powi(2) + xy * xy).sqrt();
    let (major, minor) = (mean + spread, mean - spread);
    if cells.len() < 2 || minor > major * 0.1 {
        return away.try_normalize();
    }
    let along = Vec2::from_angle(0.5 * f32::atan2(2.0 * xy, xx - yy));
    let out = along.perp();
    Some(if out.dot(away) < 0.0 { -out } else { out })
}

/// Shades each goal's cells towards the board's edge they lie along, in the seat's colour: one
/// see-through layer over the cells per seat, so a corner cell on two seats' edges is shaded from
/// both, each from its own side.
fn shade_goals(
    commands: &mut Commands,
    tools: &mut Tools,
    root: Entity,
    table: &Table,
    layout: &Layout,
    shape: CellShape,
    strips: &[GoalStrip],
) {
    let pitch = layout.pitch;
    let corners = slab::cell_corners(shape, pitch * CELL_SCALE);
    let apothem = match shape {
        CellShape::Spot => pitch * 0.3,
        _ => pitch * CELL_SCALE / 2.0,
    };
    let mut by_seat: BTreeMap<Option<u32>, Mesh> = BTreeMap::new();
    for strip in strips {
        let colour = match strip
            .seat
            .and_then(|s| table.info.seats.iter().find(|d| d.seat == s))
        {
            Some(seat) => tools.seat_colour(&seat.color, seat.seat),
            None => Color::srgb(0.25, 0.22, 0.20),
        };
        let shading = slab::shading(
            strip.cell + Vec3::Y * (SURFACE + 0.00015),
            &corners,
            strip.normal,
            apothem,
            pitch * SHADE_FADE,
            colour.to_linear(),
            SHADE_STRENGTH,
        );
        match by_seat.get_mut(&strip.seat) {
            Some(all) => {
                if let Err(e) = all.merge(&shading) {
                    warn!("a goal's shading: {e}");
                }
            }
            None => {
                by_seat.insert(strip.seat, shading);
            }
        }
    }
    for (_, mesh) in by_seat {
        let material = tools.materials.add(StandardMaterial {
            // The colour and how much of it are the mesh's own, vertex by vertex.
            base_color: Color::WHITE,
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.4,
            ..default()
        });
        commands.spawn((
            Name::new("Goal shading"),
            Mesh3d(tools.meshes.add(mesh)),
            MeshMaterial3d(material),
            ChildOf(root),
        ));
    }
}

/// One material per seat colour word. A piece in a colour no seat plays is drawn in the default
/// material, which no seat uses.
fn seat_materials(info: &GameInfo, tools: &mut Tools) -> HashMap<String, Handle<StandardMaterial>> {
    info.seats
        .iter()
        .map(|s| {
            let spec = tools.piece_material(&s.color, s.seat);
            (s.color.clone(), tools.material(&spec))
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
        let y = kit.surface + kit.stone_height * (below as f32 + 0.5);
        commands.spawn((
            Name::new(format!("Ghost at {cell}")),
            Ghost,
            Mesh3d(kit.stone.clone()),
            MeshMaterial3d(kit.ghost.clone()),
            Transform::from_translation(Vec3::new(centre.x, y, centre.z))
                .with_rotation(lie(cell, below))
                .with_scale(Vec3::splat(kit.stone_scale)),
            ChildOf(root),
        ));
    }
}

/// Builds the board's wooden materials again in the wood just chosen.
fn change_wood(
    wood: Res<BoardWood>,
    server: Res<AssetServer>,
    kits: Query<&Kit>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for kit in &kits {
        for (handle, spec) in &kit.woodwork {
            if let Some(mut m) = materials.get_mut(handle) {
                *m = spec.on_board(wood.0).build(&server);
            }
        }
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
    use crate::net::api::Goal;

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
                .init_asset::<Image>()
                .init_resource::<Appearances>()
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
                        regions: Default::default(),
                    },
                    actions: vec![],
                    goals: vec![],
                },
                game: Game {
                    game_def_id: "g".into(),
                    status: "active".into(),
                    invite_url: String::new(),
                    clock: None,
                    impulse_deadline: String::new(),
                    server_time: String::new(),
                    move_time_seconds: None,
                    lobby: false,
                    host_seat: None,
                    reservations: vec![],
                    seats: vec![],
                },
                state: state(1, &["a1"]),
                bots: vec![],
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

    fn hex3_with_goals() -> GameInfo {
        let regions = [
            ("top", ["a1", "b1", "c1"]),
            ("bottom", ["a3", "b3", "c3"]),
            ("left", ["a1", "a2", "a3"]),
            ("right", ["c1", "c2", "c3"]),
        ];
        let goal = |seat, a: &str, b: &str| Goal {
            kind: "connect".into(),
            seat: Some(seat),
            regions: vec![vec![a.into(), b.into()]],
            of: vec![],
        };
        GameInfo {
            id: "g".into(),
            name: "A game".into(),
            seats: vec![],
            board: Board {
                kind: "hex".into(),
                positions: hex3(),
                regions: regions
                    .iter()
                    .map(|(n, cells)| (n.to_string(), cells.map(String::from).to_vec()))
                    .collect(),
            },
            actions: vec![],
            goals: vec![goal(1, "top", "bottom"), goal(2, "left", "right")],
        }
    }

    #[test]
    fn each_edge_of_a_hex_board_is_marked_once_in_its_seats_colour() {
        let info = hex3_with_goals();
        let layout = Layout::new(&info.board.positions);
        let strips = goal_strips(&info, &layout, CellShape::Hexagon);
        let of = |region: &str| -> Vec<&GoalStrip> {
            strips.iter().filter(|s| s.region == region).collect()
        };
        // Two sides of each cell face off the board, and every one is marked once. The side
        // between two edges at each obtuse corner goes to the first edge by name: bottom rather
        // than left, right rather than top.
        assert_eq!(strips.len(), 22);
        assert_eq!(of("top").len(), 5);
        assert_eq!(of("bottom").len(), 6);
        assert_eq!(of("left").len(), 5);
        assert_eq!(of("right").len(), 6);
        for (i, a) in strips.iter().enumerate() {
            for b in &strips[i + 1..] {
                assert!(a.centre.distance(b.centre) > 1e-5, "a side twice: {a:?}");
            }
        }
        assert!(
            of("top")
                .iter()
                .all(|s| s.normal.y < 0.0 && s.seat == Some(1))
        );
        assert!(
            of("bottom")
                .iter()
                .all(|s| s.normal.y > 0.0 && s.seat == Some(1))
        );
        assert!(
            of("left")
                .iter()
                .all(|s| s.normal.x < 0.0 && s.seat == Some(2))
        );
        assert!(
            of("right")
                .iter()
                .all(|s| s.normal.x > 0.0 && s.seat == Some(2))
        );
        // Outside the cells, just beyond their sides.
        let a1 = layout.centres["a1"];
        let top_of_a1 = strips
            .iter()
            .find(|s| s.region == "top" && s.centre.distance(a1) < PITCH)
            .unwrap();
        let out = top_of_a1.centre.distance(a1);
        assert!(out > PITCH / 2.0 && out < PITCH * 0.7, "{out}");
    }

    #[test]
    fn the_slabs_edges_go_to_the_goals_they_run_along() {
        let info = hex3_with_goals();
        let layout = Layout::new(&info.board.positions);
        let strips = goal_strips(&info, &layout, CellShape::Hexagon);
        let centres: Vec<Vec2> = layout
            .centres
            .values()
            .map(|c| Vec2::new(c.x, c.z))
            .collect();
        let corners = slab::cell_corners(CellShape::Hexagon, layout.pitch);
        let points: Vec<Vec2> = centres
            .iter()
            .flat_map(|c| corners.iter().map(move |k| *c + *k))
            .collect();
        let outline = slab::grown(&slab::hull(&points), layout.pitch * MARGIN, 6);
        let owners: Vec<_> = (0..outline.len())
            .map(|i| rim_owner(&outline, i, &strips))
            .collect();
        // Along the far edge (facing -Z), seat 1's; along the left (facing left and a little
        // towards the player, as the rows shift right), seat 2's. Round a corner, the region it
        // faces more.
        let left = Vec2::new(-0.866_025_4, 0.5);
        for (i, owner) in owners.iter().enumerate() {
            let (a, n) = outline[i];
            if n.distance(Vec2::NEG_Y) < 0.05 {
                assert_eq!(*owner, Some(Some(1)), "far edge at {a}");
            }
            if n.distance(left) < 0.05 {
                assert_eq!(*owner, Some(Some(2)), "left edge at {a}");
            }
        }
        assert!(owners.iter().all(|o| o.is_some()), "{owners:?}");
    }

    #[test]
    fn a_square_board_has_one_strip_per_cell_along_each_edge() {
        let mut info = hex3_with_goals();
        info.board.kind = "grid".into();
        info.board.positions = (0..3)
            .flat_map(|y| {
                (0..3).map(move |x| {
                    let id = format!("{}{}", (b'a' + x as u8) as char, y + 1);
                    pos(&id, x as f32, y as f32)
                })
            })
            .collect();
        let layout = Layout::new(&info.board.positions);
        let strips = goal_strips(&info, &layout, CellShape::of(&info.board));
        for region in ["top", "bottom", "left", "right"] {
            assert_eq!(
                strips.iter().filter(|s| s.region == region).count(),
                3,
                "{region}"
            );
        }
        let top = strips.iter().find(|s| s.region == "top").unwrap();
        assert!((top.normal - Vec2::new(0.0, -1.0)).length() < 1e-4);
    }

    #[test]
    fn goals_this_client_does_not_know_or_shared_by_every_seat() {
        let mut info = hex3_with_goals();
        let layout = Layout::new(&info.board.positions);
        info.goals[0].kind = "surround".into();
        info.goals[1].seat = None;
        let strips = goal_strips(&info, &layout, CellShape::Hexagon);
        assert!(
            strips
                .iter()
                .all(|s| s.region == "left" || s.region == "right")
        );
        assert!(strips.iter().all(|s| s.seat.is_none()));
        info.goals.clear();
        assert!(goal_strips(&info, &layout, CellShape::Hexagon).is_empty());
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
            regions: Default::default(),
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
