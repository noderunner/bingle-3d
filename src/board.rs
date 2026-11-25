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
//! appearance file (`appearance.rs`) may choose each kind of piece's model and how it stands,
//! what each seat's pieces and rim are made of, and its colour. The board's wood is the player's
//! choice (`BoardWood`).
//!
//! A piece stands by its model's foot, and what is on it in a stack is raised by its model's
//! height, so a stack may be of pieces of several kinds (flat blocks under one on its edge).
//!
//! A seat's goal is drawn from the game's `goals` and the board's `regions`: a strip in the seat's
//! colour along the outer sides of each region the seat must connect (a hex board's edges), and a
//! neutral one for a goal every seat shares. Only `connect` goals are drawn; any other type is
//! skipped, as the API asks.
//!
//! While this client's seat is on impulse, the positions a click would be taken on are lit, and
//! the move being built is shown as see-through pieces in the seat's colour: where its selections
//! put something, and under the pointer, where a click would. A piece picked up is lifted and
//! bobs over its space, and once put somewhere stands there (`hand.rs`); what the move then aims
//! (an arrow) is held under the pointer. A game's appearance may lay the cells in two materials
//! by turns, as a chessboard's (`squares`).

use std::collections::{BTreeMap, HashMap};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

use bevy::ecs::system::SystemParam;
use bevy::gltf::GltfAssetLabel;
use bevy::picking::Pickable;
use bevy::picking::mesh_picking::ray_cast::SimplifiedMesh;
use bevy::prelude::*;

use crate::appearance::{Appearance, Appearances, PieceLook, PlayOn, Stands};
use crate::assets;
use crate::hand::{BoardEdge, Carry, HandPlugin, Hands, Held, LIFT, PointerOnBoard, Rest, Route};
use crate::highlight::{Lit, Outline, Tone};
use crate::look::{self, MaterialSpec, seat_colour};
use crate::net::WatchStatus;
use crate::net::api::{
    Board, BoardPosition, GameInfo, Gesture, LegalSelection, PieceOnBoard, Position,
};
use crate::play::{Choice, Hovering, LineAim, SideAim, Spot, Stage, TakeToPlace, TakeUp, Turn};
use crate::slab::{self, Rim};
use crate::sounds::Cue;
use crate::table::Table;
use crate::tints::{self, Tinted};
use crate::trays;

pub struct BoardPlugin;

impl Plugin for BoardPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(HandPlugin)
            .configure_sets(
                Update,
                Hands
                    .after(show_turn)
                    .after(bowl_stones)
                    .after(show_fences)
                    .after(show_tints),
            )
            .add_message::<BoardBuilt>()
            // After everything that makes or changes a piece, and before where its parts are
            // drawn is worked out, so a piece is never seen undressed.
            .add_systems(
                PostUpdate,
                (dress_sides, dress_parts).before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                Update,
                (
                    sync_board.run_if(resource_changed::<WatchStatus>),
                    raise_cells.run_if(resource_changed::<WatchStatus>),
                    line_aim,
                    show_turn.run_if(
                        resource_changed::<WatchStatus>
                            .or_eager(resource_changed::<Hovering>)
                            .or_eager(resource_changed::<ShowLegalMoves>)
                            .or_eager(resource_changed::<LineAim>),
                    ),
                    show_taken.run_if(resource_changed::<WatchStatus>),
                    bowl_stones.run_if(resource_changed::<WatchStatus>),
                    show_supply.run_if(resource_changed::<WatchStatus>),
                    show_fences.run_if(resource_changed::<WatchStatus>),
                    show_tints.run_if(
                        resource_changed::<WatchStatus>.or_eager(resource_changed::<Hovering>),
                    ),
                    fence_aim,
                    drop_pieces,
                    flip_pieces,
                    change_wood.run_if(resource_changed::<BoardWood>),
                )
                    .chain(),
            )
            .init_resource::<BoardWood>()
            .init_resource::<BoardOutline>()
            .init_resource::<SideAim>()
            .init_resource::<LineAim>()
            .init_resource::<ShowLegalMoves>();
    }
}

/// Metres between neighbouring cells, unless the board would then be too big for the table.
pub const PITCH: f32 = 0.028;
/// The widest a board may be, so a large one still fits in front of the player.
pub const MAX_SIZE: f32 = 0.5;

/// The slab's top, and the cells cut into it, standing a little proud of the lines between them.
pub const BOARD_HEIGHT: f32 = 0.03;
/// Only just proud: higher, each line between cells is a trench walled on both sides, which
/// shadow and ambient occlusion darken, and those round the outside (walled on one side) look
/// thinner than those inside.
const CELL_HEIGHT: f32 = 0.0003;
/// The top of the cells, where pieces stand.
pub const SURFACE: f32 = BOARD_HEIGHT + CELL_HEIGHT;
/// How much of a cell is left between the lines.
const CELL_SCALE: f32 = 0.955;
/// The darkened cells under the cells, against the pitch. Between two cells the line is both
/// cells' margins; round the outside it would be one cell's alone, half as thick, so each is
/// grown by a margin more (they overlap between cells, unseen).
const LINE_SCALE: f32 = 2.0 - CELL_SCALE;
/// The wood round the cells, as a share of the pitch (room for the coordinates), and how round
/// the slab's top edge is.
const MARGIN: f32 = 0.85;
const RIM_RADIUS: f32 = 0.005;
/// How dark the shading of a goal's cells is at the edge, and how far in it fades (in pitches):
/// a third of the way across the cell.
const SHADE_STRENGTH: f32 = 0.75;
const SHADE_FADE: f32 = CELL_SCALE / 3.0;

/// A border round the cells (a chessboard's): where it starts and ends beyond them, in pitches,
/// short of the coordinates in the margin.
const BORDER: (f32, f32) = (0.07, 0.2);

/// The wood beyond the outermost lines of a board played on its points, in pitches: room for a
/// stone on the edge and the coordinates beyond it.
const POINTS_MARGIN: f32 = 1.2;
/// A points board's lines, and its edge's (thicker, as on a Go board), in pitches.
const LINE_WIDTH: f32 = 0.035;
const EDGE_LINE_WIDTH: f32 = 0.07;
/// A marked position's dot (a Go board's star points), across, in pitches.
const MARK_SIZE: f32 = 0.2;
/// How far over the board a point a click lands on lies, so it is picked before the board.
const POINT_RAISE: f32 = 0.0003;

/// A score's dots (`trays::marks`): how far beyond the outermost positions, and how far apart,
/// in pitches; room for a ring as wide as most of a pitch round each.
const SCORE_OUT: f32 = 1.2;
const SCORE_STEP: f32 = 1.15;

/// What a game is played on: its board's spaces (cells of a shape), or the points where its
/// lines cross. The server says which positions there are and where; which of these they are
/// drawn as is this client's choice (Bingle's ADR 0124), the game's appearance saying `points`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Cells(CellShape),
    Points,
    /// The table itself, with no board on it: the cells are there to click and light, unseen.
    Table(CellShape),
}

impl Surface {
    pub fn of(board: &Board, look: Option<&Appearance>) -> Self {
        match look.map(|l| l.board.on) {
            Some(PlayOn::Points) => Surface::Points,
            Some(PlayOn::Table) => Surface::Table(CellShape::of(board)),
            _ => Surface::Cells(CellShape::of(board)),
        }
    }

    /// The height pieces stand on.
    pub fn height(self) -> f32 {
        match self {
            Surface::Cells(_) => SURFACE,
            Surface::Points => BOARD_HEIGHT,
            Surface::Table(_) => 0.0,
        }
    }

    /// How far a position's space reaches from its middle, in pitches: towards the far and near
    /// sides (`along`) or to either side. A point's is a stone's on it.
    pub fn reach(self, along: bool) -> f32 {
        match self {
            Surface::Cells(CellShape::Hexagon) | Surface::Table(CellShape::Hexagon) if along => {
                1.0 / 3f32.sqrt()
            }
            Surface::Cells(CellShape::Spot) | Surface::Table(CellShape::Spot) => 0.3,
            Surface::Cells(_) | Surface::Table(_) => 0.5,
            Surface::Points => 0.4,
        }
    }

    /// How far the board's wood reaches beyond its outermost spaces' edges, or its outermost
    /// points, in pitches: as the game's look says (room for what it lays there), or the
    /// usual.
    pub fn margin(self, look: Option<&Appearance>) -> f32 {
        let usual = match self {
            Surface::Points => POINTS_MARGIN,
            Surface::Cells(_) | Surface::Table(_) => MARGIN,
        };
        match self {
            Surface::Table(_) => usual,
            _ => look.and_then(|l| l.board.margin).unwrap_or(usual),
        }
    }

    /// Where the flat of the board ends beyond the outermost position's middle, in pitches: the
    /// usual margin of wood, short of the rounded rim. A wider margin a look asks for is room
    /// for what it lays there: the coordinates keep to the lines.
    pub fn flat_to(self, along: bool, pitch: f32) -> f32 {
        let margin = match self {
            Surface::Cells(_) | Surface::Table(_) => self.reach(along) + MARGIN,
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

/// Whether the positions a click would be taken on are lit. Off by default: at the start of most
/// games every position is one, and the whole board lights up. The ghost stone under the pointer
/// still shows where a click would play.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ShowLegalMoves(pub bool);

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
    /// Its width (X) and depth (Z), in metres, with what stands in front of it for the player
    /// (their tray).
    pub size: Vec2,
    /// The middle of that on the table (x, z): the board's, or a little nearer the player.
    pub middle: Vec2,
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

/// Which of a chessboard's two materials each position is: 0 for the corner nearest the player
/// on the left (a chessboard's a1, dark) and every other position from there, 1 for the rest, by
/// the positions' layout coordinates.
pub fn by_turns(positions: &[BoardPosition]) -> HashMap<String, usize> {
    let left = positions.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    let near = positions
        .iter()
        .map(|p| p.y)
        .fold(f32::NEG_INFINITY, f32::max);
    positions
        .iter()
        .map(|p| {
            let steps = ((p.x - left) + (near - p.y)).round() as i64;
            (p.id.clone(), steps.rem_euclid(2) as usize)
        })
        .collect()
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

    /// A cell's shape `size` across, flat on y = 0 and facing up.
    fn flat(self, size: f32) -> Mesh {
        match self {
            CellShape::Hexagon => RegularPolygon::new(size / 3f32.sqrt(), 6).mesh().build(),
            CellShape::Square => Rectangle::new(size, size).mesh().build(),
            CellShape::Spot => Circle::new(size * 0.3).mesh().build(),
        }
        .rotated_by(Quat::from_rotation_x(-FRAC_PI_2))
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
    /// How much stands under it in its stack, in micrometres at the full pitch: a piece is put
    /// down again when what is under it changes height.
    under: u32,
}

/// A position of the board, which is lit while a click on it would be taken (`highlight::Lit`:
/// how that looks is `highlight.rs`'s).
#[derive(Component)]
struct Cell {
    id: String,
}

/// A see-through piece: part of the move being built, or where a click would put one.
#[derive(Component)]
struct Ghost;

/// A piece of a move sent and not yet played, shown solid where it goes: what arrives there when
/// it is played is not dropped in again.
#[derive(Component)]
struct Sent(String);

/// The mark on the cell of the last move.
#[derive(Component)]
struct LastMoveMark;

/// How thick the generic piece is (`assets-src/blender/stones.py`), at the full pitch, should
/// its model not say.
const STONE_THICKNESS: f32 = 0.0088;
/// A block set down square to the board by hand: how far from true it may be turned, either
/// way, and how far from its position's middle it may lie, each way across the board (metres at
/// the full pitch). Enough that the blocks of a stack do not line up; too little for one to
/// leave its space.
const SQUARE_OFF: f32 = 0.08;
const SQUARE_ASIDE: f32 = 0.0013;
/// How far from true a piece looking across the board may be turned, either way.
const FORWARD_OFF: f32 = 0.09;

/// A piece on its way down to where it was played.
#[derive(Component)]
struct Dropping {
    timer: Timer,
    to: f32,
}

const DROP_HEIGHT: f32 = 0.06;
const DROP_TIME: Duration = Duration::from_millis(280);

/// A kind of piece as the board draws it.
#[derive(Debug, Clone)]
struct PieceKit {
    mesh: Handle<Mesh>,
    /// Where the model's foot is below its origin, and how tall it is, at the full pitch.
    foot: f32,
    height: f32,
    stands: Stands,
    /// What it is made of, when that is not its seat's material, and the same see-through.
    material: Option<Handle<StandardMaterial>>,
    ghost: Option<Handle<StandardMaterial>>,
    /// Its two sides, for a piece of two sides (`sides.rs`): `mesh` is then its upper half.
    sides: Option<SidesKit>,
    /// Its model's parts in materials of their own (an inlay), each its mesh and its material,
    /// solid and see-through: `mesh` is then its first part.
    parts: Vec<PartKit>,
    /// A box of its model's size, which a click is found on in place of the model itself
    /// (`SimplifiedMesh`): Bevy's ray cast passes through a model's smallest triangles (an
    /// engraving's, in metres), so a tile lying face up could not be clicked on its face.
    hit: Handle<Mesh>,
}

/// A part of a piece's model in a material of its own, whoever's piece it is.
#[derive(Debug, Clone)]
struct PartKit {
    mesh: Handle<Mesh>,
    solid: Handle<StandardMaterial>,
    ghost: Handle<StandardMaterial>,
}

/// A piece with parts of their own, dressed (`dress_parts`): the children drawing them.
#[derive(Component)]
struct Parts(Vec<Entity>);

/// A piece of two sides as the board draws it: its model's lower half (the upper is
/// `PieceKit::mesh`), each part's material, solid and see-through, and whether each colour
/// word's pieces have the second part's side up.
#[derive(Debug, Clone)]
struct SidesKit {
    under: Handle<Mesh>,
    solid: [Handle<StandardMaterial>; 2],
    ghost: [Handle<StandardMaterial>; 2],
    turned: HashMap<String, bool>,
}

/// A piece of two sides, dressed (`dress_sides`): the child drawing its lower half.
#[derive(Component)]
struct Sides {
    under: Entity,
}

/// A piece of two sides turning over where it lies, its new side coming up (`sides::flip_pose`),
/// in place of its `Rest` until it is done.
#[derive(Component)]
struct Flipping {
    rest: Transform,
    axis: Vec3,
    lift: f32,
    delay: f32,
    secs: f32,
}

/// The outline on the table (x, z) of the board being shown, its margin round the cells too:
/// what lies beside it (the move tiles) keeps clear of it. Empty with no board.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct BoardOutline(pub Vec<Vec2>);

/// The meshes and materials of the board being shown.
#[derive(Component)]
struct Kit {
    /// The generic piece, and each kind the game's look gives a model, by its id.
    stone: PieceKit,
    pieces: HashMap<String, PieceKit>,
    /// The models are made for the full pitch; a smaller board's pieces are smaller.
    scale: f32,
    /// Each colour word's material, and the way its seat looks across the board (a turn about
    /// the vertical from the first seat's, which looks to the far side).
    materials: HashMap<String, Handle<StandardMaterial>>,
    facing: HashMap<String, f32>,
    /// A piece of the move being built, in this client's seat's colour.
    ghost: Handle<StandardMaterial>,
    /// The materials made of the board's wood, to build again when another is chosen.
    woodwork: Vec<(Handle<StandardMaterial>, MaterialSpec)>,
    /// The height pieces stand on.
    surface: f32,
    /// The kind of piece what a move aims is shown as, if the game's look says.
    aimed: Option<String>,
    /// The kind of piece a move of each kind leaves where it started, where the game's look
    /// says (a disc dropped in a ring that moves on).
    leaves: HashMap<String, String>,
    /// Where a stone taken from each colour's bowl waits, by colour word.
    bowls: HashMap<String, Vec3>,
    /// Each seat's supply beside the board, by its colour word, and each kind of piece's pool;
    /// how far the supply reaches to either side, for the camera.
    supply: HashMap<String, Vec<SupplyPool>>,
    pools: HashMap<String, String>,
    beside: f32,
    /// How walls between positions are drawn and put down, for a game whose look has fences.
    fences: Option<FenceKit>,
    /// The board's playing area's outline (its cells', or its points'), for what is held to
    /// stay over while the pointer is off the board.
    edge: Vec<Vec2>,
    /// The slab's own outline, its margin round the playing area too: what lies beside the
    /// board keeps clear of it (`BoardOutline`).
    outline: Vec<Vec2>,
    /// The width (X) and depth (Z) the camera frames: the board's, or the middle of the table
    /// for a game with no board.
    size: Vec2,
    /// A game with no board: its cells, unseen, are raised to the top of what stands on them,
    /// so a cell lit there is lit on top (`raise_cells`).
    on_table: bool,
    /// Each kind of piece's colour word, for a game whose pieces have their own (Blooms').
    piece_colour: HashMap<String, String>,
    /// Each colour word's see-through material.
    ghosts: HashMap<String, Handle<StandardMaterial>>,
    /// Which colour word each of the materials a piece is given is, and whether it is the
    /// see-through one: how a piece of two sides is dressed (`dress_sides`), wherever it was
    /// made.
    dress: HashMap<AssetId<StandardMaterial>, (String, bool)>,
    /// Each seat's tray, in seat order, for a game that counts what each captures (none
    /// otherwise); the pool the count is kept in; how big the pieces in them are.
    trays: Vec<Tray>,
    counter: String,
    tray_scale: f32,
    /// A score kept on dots on the board's margin in place of trays, for a game whose look
    /// says: the view's counter, and the kind of piece of a seat's own that goes on a dot.
    score: Option<(String, String)>,
    /// How far towards the player the first seat's tray reaches, if there are trays.
    front: f32,
    /// How each key of the view's tints is drawn, for a game whose look says (`tints.rs`).
    tints: HashMap<String, TintKit>,
    /// What both seats have still to place, beside the board, for a game whose look says.
    stock: Option<StockKit>,
}

/// How the positions the view tints with one key are drawn: a piece laid there (a tile), lit
/// as a cell is while a click there would be taken (its outline in its own space); or a stamp
/// printed there.
enum TintKit {
    Laid {
        piece: String,
        outline: Outline,
    },
    Stamp {
        mesh: Handle<Mesh>,
        material: Handle<StandardMaterial>,
    },
}

/// The view's count of what both seats have still to place (`ext.view.<counts>`), and where each
/// kind's pieces lie beside the board.
struct StockKit {
    counts: String,
    slots: Vec<(String, Vec<Vec2>)>,
}

/// A tint drawn on a position: where, and its key.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct TintAt {
    cell: String,
    key: String,
}

/// A piece of the stock beside the board: its kind, and its place in the kind's column.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct InStock {
    kind: String,
    index: usize,
}

/// A piece taken from the stock: held under the pointer, or (once placed) carried to its
/// position.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct FromStock {
    kind: String,
    cell: Option<String>,
}

/// Fences: one's mesh and material, where its foot is below its middle (metres at the full
/// pitch), where one taken from the bowl waits (over its rim), the side of its square each
/// fence-building choice puts it on (a step to the neighbour beyond).
struct FenceKit {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    foot: f32,
    over: Vec3,
    sides: HashMap<String, Vec2>,
}

impl FenceKit {
    /// How high a fence held under the pointer is over the board: lifted, its foot clear.
    fn held(&self, kit: &Kit) -> Held {
        Held {
            board: kit.surface,
            lift: (LIFT - self.foot) * kit.scale,
            scale: kit.scale,
        }
    }
}

/// The space between the bowl of fences and what stands beside the board.
const FENCE_BOWL_GAP: f32 = 0.025;

/// A see-through fence on the side a click would put the one taken on.
#[derive(Component)]
struct FenceGhost;

/// A wall the position has, drawn as a fence, by its name (`fences::key`).
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct WallAt(String);

/// A fence taken from the bowl: waiting over it, or (once a side is chosen) put on that side,
/// named as the wall it makes.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct HeldFence(Option<String>);

/// One pool of a seat's supply: its id, the kind of piece a click on it takes, where each of
/// its pieces stands, and how high they stand.
struct SupplyPool {
    id: String,
    piece: String,
    slots: Vec<Transform>,
    height: f32,
    /// The seat's number; the view's counter that says how many it has, when the reserves do
    /// not (`[supply]`); and the choice a click on it makes.
    seat: u32,
    counter: Option<String>,
    take: Gesture,
}

impl SupplyPool {
    /// How many the seat of colour word `colour` has in it, as the position says.
    fn count(&self, table: &Table, colour: &str) -> usize {
        let state = &table.state.state;
        match &self.counter {
            Some(id) => state.view_counter(self.seat, id).unwrap_or(0) as usize,
            None => state.count(colour, &self.id) as usize,
        }
    }
}

/// A piece of a seat's supply (by its colour word), of a pool, and its place in the line.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct InSupply {
    colour: String,
    pool: String,
    index: usize,
}

/// A seat's tray of what it captured: the colour word its count is kept under, the colours it
/// plays, and where each square's middle is.
struct Tray {
    seat: String,
    number: u32,
    colours: Vec<String>,
    squares: Vec<Vec3>,
}

/// A piece in a seat's tray (by the seat's place in the game's seats), and its place there.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct InTray {
    seat: usize,
    index: usize,
}

/// A captured piece's way to its tray: it waits for the move that took it to be set down, and
/// each after it a little longer, so several go one after another.
const CAPTURE_WAIT: f32 = 0.3;
const CAPTURE_EACH: f32 = 0.16;
const CAPTURE_ACROSS: f32 = 0.32;
/// How far over the board's other pieces it is raised (metres at the full pitch).
const CAPTURE_OVER: f32 = 0.018;

impl Kit {
    /// The colour word a piece of kind `id` is played in: its own, or else the player's.
    fn colour_of<'a>(&'a self, id: &str, yours: &'a str) -> &'a str {
        self.piece_colour.get(id).map_or(yours, String::as_str)
    }

    /// Where a stone of kind `id` is taken from, if it comes from a bowl: its colour word.
    fn bowl_of<'a>(&'a self, id: &str, yours: &'a str) -> Option<&'a str> {
        let colour = self.colour_of(id, yours);
        self.bowls.contains_key(colour).then_some(colour)
    }

    fn piece(&self, id: &str) -> &PieceKit {
        self.pieces.get(id).unwrap_or(&self.stone)
    }

    /// Where a piece of kind `id` that this client's seat (of colour word `yours`) places is
    /// taken from, if from somewhere to see: its colour's bowl (the colour word), or its pool of
    /// the seat's supply (`colour/pool`).
    /// Where what a choice places comes from (its colour's bowl, or `colour/pool` of the seat's
    /// supply), and the kind of piece it is: a
    /// choice of piece from its bowl or its pool of the seat's supply, or the choice a pool of
    /// the supply is taken with (Fendo's `add`).
    fn source_for(&self, choice: &Gesture, yours: &str) -> Option<(String, String)> {
        let pools = self.supply.get(yours).map_or(&[][..], Vec::as_slice);
        let pool = if choice.kind == "piece" {
            if let Some(colour) = self.bowl_of(&choice.value, yours) {
                return Some((colour.to_string(), choice.value.clone()));
            }
            let id = self.pools.get(&choice.value)?;
            let pool = pools.iter().find(|p| p.counter.is_none() && &p.id == id)?;
            return Some((format!("{yours}/{}", pool.id), choice.value.clone()));
        } else {
            pools.iter().find(|p| p.take == *choice)?
        };
        Some((format!("{yours}/{}", pool.id), pool.piece.clone()))
    }

    /// Where what a selection placing something comes from, and the kind of piece: by the first
    /// of its gestures that names a source.
    fn source_of_placing(&self, s: &LegalSelection, yours: &str) -> Option<(String, String)> {
        let (source, piece) = s.gestures.iter().find_map(|g| self.source_for(g, yours))?;
        Some((source, s.piece.clone().unwrap_or(piece)))
    }

    /// Where a fence on the side between `a` and `b` stands, if both are on the board.
    fn fence_between(&self, layout: &Layout, a: &str, b: &str) -> Option<Transform> {
        let fence = self.fences.as_ref()?;
        let (middle, yaw) =
            crate::fences::between(*layout.centres.get(a)?, *layout.centres.get(b)?);
        Some(
            Transform::from_translation(middle.with_y(self.surface - fence.foot * self.scale))
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::splat(self.scale)),
        )
    }

    /// The wall the choice `choice` (a fence-building one) puts on a side of `cell`, and where
    /// its fence stands.
    fn fence_on(&self, layout: &Layout, cell: &str, choice: &str) -> Option<(String, Transform)> {
        let step = *self.fences.as_ref()?.sides.get(choice)?;
        let centre = *layout.centres.get(cell)?;
        let beyond = crate::fences::neighbour(&layout.centres, centre, step, layout.pitch)?;
        Some((
            crate::fences::key(cell, beyond),
            self.fence_between(layout, cell, beyond)?,
        ))
    }

    /// The pool of a seat's supply a source names.
    fn supply_pool(&self, source: &str) -> Option<&SupplyPool> {
        let (colour, pool) = source.split_once('/')?;
        self.supply.get(colour)?.iter().find(|p| p.id == pool)
    }

    /// Where a piece of kind `id` lies in seat `seat`'s tray, the `index`th in: in its square,
    /// as loose as on the board, and on the first ones in once every square has one.
    fn in_tray(&self, id: &str, colour: &str, seat: usize, index: usize) -> Option<Transform> {
        let squares = &self.trays.get(seat)?.squares;
        let centre = *squares.get(index % squares.len().max(1))?;
        let layer = index / squares.len();
        let kind = self.piece(id);
        let facing = self.facing.get(colour).copied().unwrap_or(0.0);
        let stance = stance(kind.stands, &format!("tray {seat}"), index, facing);
        let y = centre.y + (layer as f32 * kind.height - kind.foot * stance.size) * self.tray_scale;
        Some(
            Transform::from_xyz(centre.x, y, centre.z)
                .with_rotation(stance.turn)
                .with_scale(Vec3::splat(self.tray_scale * stance.size)),
        )
    }

    /// The kind of piece of colour word `colour`, for one nobody saw captured: the kind played in
    /// it, if its pieces have their own colours, or the generic stone.
    fn kind_in(&self, colour: &str) -> &str {
        self.piece_colour
            .iter()
            .find(|(_, c)| *c == colour)
            .map_or("", |(id, _)| id.as_str())
    }

    /// How high a position's floor is over the board, at the full pitch: the piece a tint lays
    /// there (a tile), if any, which what stands there stands on. `tints` is the view's.
    fn floor(&self, tints: &BTreeMap<String, String>, cell: &str) -> f32 {
        match tints.get(cell).and_then(|key| self.tints.get(key)) {
            Some(TintKit::Laid { piece, .. }) => self.piece(piece).height,
            _ => 0.0,
        }
    }

    /// How tall a stack of pieces is, bottom first, at the full pitch.
    fn stack_height<'a>(&self, stack: impl IntoIterator<Item = &'a PieceOnBoard>) -> f32 {
        stack.into_iter().map(|p| self.piece(&p.piece).height).sum()
    }

    /// Where a piece of this kind goes, on `under` (metres at the full pitch) of others on
    /// `centre`: standing on its foot, set down as its kind is.
    fn set_down(
        &self,
        id: &str,
        colour: &str,
        centre: Vec3,
        (cell, layer): (&str, usize),
        under: f32,
    ) -> Transform {
        let kind = self.piece(id);
        let facing = self.facing.get(colour).copied().unwrap_or(0.0);
        let stance = stance(kind.stands, cell, layer, facing);
        let y = self.surface + (under - kind.foot * stance.size) * self.scale;
        let aside = stance.aside * self.scale;
        Transform::from_translation(Vec3::new(centre.x + aside.x, y, centre.z + aside.y))
            .with_rotation(stance.turn)
            .with_scale(Vec3::splat(self.scale * stance.size))
    }
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

    /// A kind of piece: the model its look gives, or the generic stone.
    fn piece_kit(&mut self, look: Option<&PieceLook>) -> PieceKit {
        let model = look.map_or(look::PIECE_MODEL, |l| l.model.as_str());
        let (low, high) =
            assets::model_span(model).unwrap_or((-STONE_THICKNESS / 2.0, STONE_THICKNESS / 2.0));
        let own = look
            .and_then(|l| l.material.as_ref())
            .and_then(|name| self.look?.materials.get(name))
            .cloned();
        let ghost = own.as_ref().map(|spec| self.see_through(spec));
        let sides = look.and_then(|l| self.sides_kit(l));
        let parts = look.map(|l| self.parts_kit(l)).unwrap_or_default();
        let half = Vec3::new(0.013, STONE_THICKNESS / 2.0, 0.013);
        let (lo, hi) = assets::model_bounds(model).unwrap_or((-half, half));
        let hit = self.meshes.add(Cuboid::from_corners(lo, hi));
        PieceKit {
            hit,
            ghost,
            sides,
            parts,
            mesh: self.server.load(
                GltfAssetLabel::Primitive {
                    mesh: 0,
                    primitive: 0,
                }
                .from_asset(assets::path(model)),
            ),
            foot: low,
            height: high - low,
            stands: look.map(|l| l.stands).unwrap_or_default(),
            material: own.map(|spec| self.material(&spec)),
        }
    }

    /// A material, see-through: for a piece of the move being built.
    fn see_through(&mut self, spec: &MaterialSpec) -> Handle<StandardMaterial> {
        let mut m = spec.on_board(self.wood).build(self.server);
        m.base_color = m.base_color.with_alpha(0.5);
        m.alpha_mode = AlphaMode::Blend;
        self.materials.add(m)
    }

    /// A piece's two sides, if its look gives them: its model's second part, and each part's
    /// material by the look's `sides` (`Appearance::problems` says what is wrong with them).
    fn sides_kit(&mut self, look: &PieceLook) -> Option<SidesKit> {
        if look.sides.is_empty() {
            return None;
        }
        let parts = assets::model_parts(&look.model);
        let [first, second] = parts.as_slice() else {
            return None;
        };
        let spec = |part: &String| {
            self.look
                .and_then(|l| l.materials.get(look.sides.get(part)?))
                .cloned()
        };
        let specs = [spec(first)?, spec(second)?];
        Some(SidesKit {
            under: self.server.load(
                GltfAssetLabel::Primitive {
                    mesh: 0,
                    primitive: 1,
                }
                .from_asset(assets::path(&look.model)),
            ),
            solid: specs.clone().map(|s| self.material(&s)),
            ghost: specs.map(|s| self.see_through(&s)),
            turned: look
                .up
                .iter()
                .map(|(word, side)| (word.clone(), side == second))
                .collect(),
        })
    }

    /// A piece's parts in materials of their own, if its look gives them: each its model's part
    /// of that name (`Appearance::problems` says what is wrong with them).
    fn parts_kit(&mut self, look: &PieceLook) -> Vec<PartKit> {
        let parts = assets::model_parts(&look.model);
        let mut out = Vec::new();
        for (index, part) in parts.iter().enumerate().skip(1) {
            let Some(spec) = look
                .parts
                .get(part)
                .and_then(|name| self.look?.materials.get(name))
                .cloned()
            else {
                continue;
            };
            out.push(PartKit {
                mesh: self.server.load(
                    GltfAssetLabel::Primitive {
                        mesh: 0,
                        primitive: index,
                    }
                    .from_asset(assets::path(&look.model)),
                ),
                solid: self.material(&spec),
                ghost: self.see_through(&spec),
            });
        }
        out
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

/// A piece the scene shows: what and where it is, whether the move being built holds it, and
/// where a click on it is taken.
type Shown = (
    Entity,
    &'static PieceAt,
    Option<&'static Carry>,
    &'static Spot,
);

/// What else `sync_board` reads of the scene: the pieces shown, those of a move sent, those
/// taken from bowls, and those in trays.
type Others<'w, 's> = (
    Query<'w, 's, Shown>,
    Query<'w, 's, &'static Sent>,
    Query<'w, 's, &'static FromBowl>,
    Query<'w, 's, (Entity, &'static InTray)>,
);

fn sync_board(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    mut drawing: Drawing,
    roots: Query<(Entity, &BoardRoot, &Kit)>,
    (pieces, sent, from_bowls, in_trays): Others,
    mut marks: Query<(&mut Transform, &mut Visibility), With<LastMoveMark>>,
    mut built: MessageWriter<BoardBuilt>,
) {
    let Some(table) = &watch.table else {
        for (root, ..) in &roots {
            commands.entity(root).despawn();
        }
        commands.insert_resource(BoardEdge::default());
        commands.insert_resource(BoardOutline::default());
        return;
    };
    let previews = watch.turn.as_ref().map(Turn::previews).unwrap_or_default();
    // A card game's cards are laid in place of its board, which is nothing to look at
    // (`card_table.rs`).
    if crate::card_table::on_table(&table.info, drawing.appearances.for_game(&table.info.id)) {
        for (root, ..) in &roots {
            commands.entity(root).despawn();
        }
        return;
    }
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
        commands.insert_resource(BoardEdge(kit.1.edge.clone()));
        commands.insert_resource(BoardOutline(kit.1.outline.clone()));
        // Framed with the player's tray in front of it, if it has one, and the move tiles
        // round it (`move_tiles.rs`).
        let (mut front, mut beside) = (kit.1.front, kit.1.beside);
        if tools.look.is_some_and(|l| l.move_tiles.is_some()) {
            let (f, b) = crate::move_tiles::reaches(&kit.1.outline);
            (front, beside) = (front.max(f), beside.max(b));
        }
        let size = kit.1.size;
        let extra = (front - size.y / 2.0).max(0.0);
        built.write(BoardBuilt {
            size: Vec2::new(size.x.max(2.0 * beside), size.y + extra),
            middle: Vec2::Y * extra / 2.0,
        });
        // A board just built shows the position as it is, without dropping every piece.
        let root = kit.0;
        let wanted = wanted_pieces(table, &kit.1, &previews);
        place_pieces(
            &mut commands,
            root,
            &kit.1,
            &layout,
            (table, &wanted),
            &[],
            None,
            &[],
        );
        fill_trays(&mut commands, root, &kit.1, table, &[], &[]);
        commands.entity(root).insert(kit.1);
        return;
    };

    let layout = Layout::new(&table.info.board.positions);
    let shown: Vec<(Entity, &PieceAt)> = pieces.iter().map(|(e, at, ..)| (e, at)).collect();
    // Where the move just played was shown put down already: what arrives there is not dropped.
    let put: Vec<&str> = pieces
        .iter()
        .filter(|(_, _, carry, _)| matches!(carry, Some(Carry::Put(_))))
        .map(|(.., spot)| spot.0.as_str())
        .chain(sent.iter().map(|s| s.0.as_str()))
        .chain(from_bowls.iter().filter_map(|f| f.cell.as_deref()))
        .collect();
    // What left the board and was captured goes to its captor's tray, rather than away.
    let wanted = wanted_pieces(table, kit, &previews);
    let gone: Vec<(Entity, &PieceAt)> = shown
        .iter()
        .filter(|(_, at)| !wanted.contains(at))
        .copied()
        .collect();
    let held: Vec<(Entity, InTray)> = in_trays.iter().map(|(e, t)| (e, *t)).collect();
    let taken = fill_trays(&mut commands, root, kit, table, &gone, &held);
    place_pieces(
        &mut commands,
        root,
        kit,
        &layout,
        (table, &wanted),
        &shown,
        Some(&put),
        &taken,
    );
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

/// Where an unseen cell of a game with no board is: on the table, or on the top of what stands
/// there (`under`, at the full pitch, scaled by `scale`), so its light is seen.
fn cell_height(under: f32, scale: f32) -> f32 {
    CELL_LIFT + under * scale
}

/// How far over the table, or what stands there, an unseen cell lies.
const CELL_LIFT: f32 = 0.0003;

/// Raises each cell of a game with no board to the top of the stack on it, as the position has
/// it, so where a click would put a piece on others (one climbing) is lit where it is seen.
fn raise_cells(
    watch: Res<WatchStatus>,
    kits: Query<&Kit, With<BoardRoot>>,
    mut cells: Query<(&Cell, &mut Transform)>,
) {
    let (Some(table), Ok(kit)) = (&watch.table, kits.single()) else {
        return;
    };
    if !kit.on_table {
        return;
    }
    let stacks = &table.state.state.cells;
    let tints = table.state.state.tints();
    for (cell, mut t) in &mut cells {
        let under = stacks.get(&cell.id).map_or(0.0, |stack| {
            kit.floor(&tints, &cell.id) + kit.stack_height(stack)
        });
        let y = cell_height(under, kit.scale);
        if (t.translation.y - y).abs() > 1e-6 {
            t.translation.y = y;
        }
    }
}

/// The pieces the position has and the scene does not are added, and those the scene has and the
/// position does not are taken away, but those `kept` (gone to a tray). With `animate`, those
/// added drop in, but on the positions it names, where the move being built showed them already.
#[allow(clippy::too_many_arguments)]
fn place_pieces(
    commands: &mut Commands,
    root: Entity,
    kit: &Kit,
    layout: &Layout,
    (table, wanted): (&Table, &[PieceAt]),
    shown: &[(Entity, &PieceAt)],
    animate: Option<&[&str]>,
    kept: &[Entity],
) {
    // A piece of two sides that changed hands is turned over where it lies, rather than taken
    // away and another put down; at once on a board just built.
    let from = table
        .last_move
        .as_ref()
        .and_then(|cell| layout.centres.get(cell))
        .copied();
    let mut turned: Vec<&PieceAt> = Vec::new();
    for (entity, at) in shown {
        if wanted.contains(at) || kept.contains(entity) {
            continue;
        }
        let over = animate.and(
            wanted
                .iter()
                .find(|w| turns_over(kit, at, w) && !shown.iter().any(|(_, s)| *s == *w)),
        );
        match (over, layout.centres.get(&at.cell)) {
            (Some(now), Some(centre)) => {
                let rest = kit.set_down(
                    &now.piece.piece,
                    &now.piece.color,
                    *centre,
                    (&now.cell, now.layer),
                    now.under as f32 * 1e-6,
                );
                let flip = Flipping {
                    rest,
                    axis: crate::sides::flip_axis(from, *centre),
                    lift: crate::sides::FLIP_LIFT * kit.scale,
                    delay: crate::sides::flip_delay(from, *centre, layout.pitch),
                    secs: 0.0,
                };
                // Its new side down, as its old side was up, until it goes over.
                let (start, _) = crate::sides::flip_pose(rest, flip.axis, flip.lift, 1.0, 0.0);
                let material = kit.materials.get(&now.piece.color).cloned();
                commands
                    .entity(*entity)
                    .remove::<(Rest, Dropping)>()
                    .insert((
                        Name::new(format!("{} at {}", now.piece.piece, now.cell)),
                        MeshMaterial3d(material.unwrap_or_default()),
                        start,
                        flip,
                        now.clone(),
                    ));
                turned.push(now);
            }
            _ => commands.entity(*entity).despawn(),
        }
    }
    for at in wanted.iter().cloned() {
        if shown.iter().any(|(_, s)| **s == at) || turned.contains(&&at) {
            continue;
        }
        let Some(centre) = layout.centres.get(&at.cell) else {
            // A piece on a position the board does not have: not a board cell (a reserve, say),
            // which this stage does not draw yet.
            continue;
        };
        let kind = kit.piece(&at.piece.piece);
        let material = kind
            .material
            .clone()
            .or_else(|| kit.materials.get(&at.piece.color).cloned())
            .unwrap_or_default();
        let mut rest = kit.set_down(
            &at.piece.piece,
            &at.piece.color,
            *centre,
            (&at.cell, at.layer),
            at.under as f32 * 1e-6,
        );
        let to = rest.translation.y;
        let settled = Rest(rest);
        let animate = animate.is_some_and(|put| !put.contains(&at.cell.as_str()));
        if animate {
            rest.translation.y += DROP_HEIGHT;
        }
        let mut piece = commands.spawn((
            Name::new(format!("{} at {}", at.piece.piece, at.cell)),
            Mesh3d(kind.mesh.clone()),
            MeshMaterial3d(material),
            rest,
            // A click on a piece is a click on where it stands.
            Spot(at.cell.clone()),
            Pickable::default(),
            at,
            ChildOf(root),
        ));
        if animate {
            piece.insert(Dropping {
                timer: Timer::new(DROP_TIME, TimerMode::Once),
                to,
            });
        } else {
            piece.insert(settled);
        }
    }
}

/// Whether the piece shown as `was` is the piece `now` turned over: the same kind of piece, of
/// two sides, in the same place, another seat's.
fn turns_over(kit: &Kit, was: &PieceAt, now: &PieceAt) -> bool {
    was.cell == now.cell
        && was.layer == now.layer
        && was.under == now.under
        && was.piece.piece == now.piece.piece
        && was.piece.color != now.piece.color
        && kit.piece(&now.piece.piece).sides.is_some()
}

/// Each seat's tray brought up to its count: pieces that left the board (`gone`) and that a
/// seat's count going up says it captured are carried there from where they stood (returned, to
/// be kept), others it captured that nobody saw (a game opened part way through) laid there at
/// once, and the last taken out of a tray whose count went down. `held` is what the trays hold.
fn fill_trays(
    commands: &mut Commands,
    root: Entity,
    kit: &Kit,
    table: &Table,
    gone: &[(Entity, &PieceAt)],
    held: &[(Entity, InTray)],
) -> Vec<Entity> {
    if kit.trays.is_empty() {
        return Vec::new();
    }
    let had: Vec<usize> = (0..kit.trays.len())
        .map(|seat| held.iter().filter(|(_, t)| t.seat == seat).count())
        .collect();
    let counts: Vec<u32> = kit
        .trays
        .iter()
        .map(|t| match &kit.score {
            Some((counter, _)) => table
                .state
                .state
                .view_counter(t.number, counter)
                .unwrap_or(0),
            None => table.state.state.count(&t.seat, &kit.counter),
        })
        .collect();
    let colours: Vec<Vec<String>> = kit.trays.iter().map(|t| t.colours.clone()).collect();
    // A score kept with the seats' own pieces takes only its kind (a ring, not the row's
    // discs that went with it).
    let gone: Vec<(Entity, &PieceAt)> = gone
        .iter()
        .filter(|(_, at)| {
            kit.score
                .as_ref()
                .is_none_or(|(_, piece)| *piece == at.piece.piece)
        })
        .copied()
        .collect();
    let gone = gone.as_slice();
    let gone_colours: Vec<&str> = gone
        .iter()
        .map(|(_, at)| kit.colour_of(&at.piece.piece, &at.piece.color))
        .collect();
    let mut taken = Vec::new();
    let own = kit.score.is_some();
    for (seat, change) in trays::fill(&had, &counts, &colours, &gone_colours, own)
        .into_iter()
        .enumerate()
    {
        let mut last: Vec<&(Entity, InTray)> =
            held.iter().filter(|(_, t)| t.seat == seat).collect();
        last.sort_by_key(|(_, t)| std::cmp::Reverse(t.index));
        for (entity, _) in last.into_iter().take(change.drop) {
            commands.entity(*entity).despawn();
        }
        for (k, source) in change.add.into_iter().enumerate() {
            let index = had[seat] + k;
            match source {
                trays::Source::Board(i) => {
                    let (entity, at) = gone[i];
                    let Some(to) = kit.in_tray(&at.piece.piece, &at.piece.color, seat, index)
                    else {
                        continue;
                    };
                    commands
                        .entity(entity)
                        .remove::<(PieceAt, Spot, Pickable, Dropping)>()
                        .insert((
                            Name::new(format!("{} captured by seat {}", at.piece.piece, seat + 1)),
                            InTray { seat, index },
                            Rest(to),
                            Route {
                                delay: CAPTURE_WAIT + CAPTURE_EACH * taken.len() as f32,
                                over: CAPTURE_OVER * kit.scale,
                                across: CAPTURE_ACROSS,
                                lands: lands_in_tray(kit),
                            },
                            Carry::Put(to),
                        ));
                    taken.push(entity);
                }
                trays::Source::Unseen(colour) => {
                    let id = match &kit.score {
                        Some((_, piece)) => piece.as_str(),
                        None => kit.kind_in(&colour),
                    };
                    let Some(to) = kit.in_tray(id, &colour, seat, index) else {
                        continue;
                    };
                    let kind = kit.piece(id);
                    let material = kind
                        .material
                        .clone()
                        .or_else(|| kit.materials.get(&colour).cloned())
                        .unwrap_or_default();
                    commands.spawn((
                        Name::new(format!("{colour} captured by seat {}", seat + 1)),
                        Mesh3d(kind.mesh.clone()),
                        MeshMaterial3d(material),
                        to,
                        Rest(to),
                        InTray { seat, index },
                        ChildOf(root),
                    ));
                }
            }
        }
    }
    taken
}

/// The sound a piece carried to a tray makes as it lands there: a capture's, or where the look
/// keeps a score with the seats' own pieces (`[score]`), a point scored, cheered as a win is.
fn lands_in_tray(kit: &Kit) -> Cue {
    if kit.score.is_some() {
        Cue::Win
    } else {
        Cue::Capture
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

/// How a piece is set down on its position.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Stance {
    turn: Quat,
    /// How big it is against its model.
    size: f32,
    /// How far from the position's middle it is, across the board (x, z), in metres at the
    /// full pitch.
    aside: Vec2,
}

/// How a piece is set down, by the way its kind is: the same for the same place, and different
/// from place to place, so each piece of a stack has its own. `facing` is the way its seat looks
/// across the board, a turn about the vertical.
fn stance(stands: Stands, cell: &str, layer: usize, facing: f32) -> Stance {
    let off = |what: u8, by: f32| (chance(cell, layer, what) - 0.5) * 2.0 * by;
    let upright = |turn: f32| Stance {
        turn: Quat::from_rotation_y(turn),
        size: 1.0,
        aside: Vec2::ZERO,
    };
    match stands {
        Stands::Loose => Stance {
            turn: lie(cell, layer),
            size: size(cell, layer),
            aside: Vec2::ZERO,
        },
        Stands::Upright => upright(yaw(cell, layer)),
        Stands::Square => {
            let quarters = (chance(cell, layer, 4) * 4.0).floor();
            Stance {
                aside: Vec2::new(off(7, SQUARE_ASIDE), off(8, SQUARE_ASIDE)),
                ..upright(quarters * FRAC_PI_2 + off(5, SQUARE_OFF))
            }
        }
        Stands::Forward => upright(facing + off(6, FORWARD_OFF)),
        Stands::Fitted => upright(0.0),
    }
}

/// Every piece of the position, bottom of each stack first.
fn wanted_pieces(table: &Table, kit: &Kit, previews: &[&Position]) -> Vec<PieceAt> {
    let mut out = Vec::new();
    let tints = table.state.state.tints();
    for (cell, stack) in &table.state.state.cells {
        let mut under = kit.floor(&tints, cell);
        for (layer, piece) in stack.iter().enumerate() {
            let mut piece = piece.clone();
            // A piece of two sides that the move being built turns over (the discs a ring
            // jumps) is shown turned, as the server's preview has it, until the move is played
            // or taken back; and one it then takes away, as the last preview to have it had it.
            if let Some(after) = previews
                .iter()
                .find_map(|p| {
                    p.cells
                        .get(cell)?
                        .get(layer)
                        .filter(|a| a.piece == piece.piece)
                })
                .filter(|a| a.color != piece.color)
                .filter(|_| kit.piece(&piece.piece).sides.is_some())
            {
                piece.color = after.color.clone();
            }
            let height = kit.piece(&piece.piece).height;
            out.push(PieceAt {
                cell: cell.clone(),
                layer,
                piece,
                under: (under * 1e6_f32).round() as u32,
            });
            under += height;
        }
    }
    out
}

/// How far from the middle of the table the frame of a game with no board reaches, in pitches:
/// as wide as the web page's smallest picture of such a game, nine cells, and a little more.
const TABLE_FRAME: f32 = 5.0;

/// The corners of the cells a game with no board is framed by: those within `TABLE_FRAME` of the
/// middle of its positions (`centres`, each with the cell's `corners`).
fn table_frame(centres: &[Vec2], corners: &[Vec2], pitch: f32) -> Vec<Vec2> {
    centres
        .iter()
        .filter(|c| c.length() <= TABLE_FRAME * pitch + 1e-4)
        .flat_map(|c| corners.iter().map(move |k| *c + *k))
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
    let playing = match surface {
        Surface::Cells(_) | Surface::Table(_) => slab::hull(
            &centres
                .iter()
                .flat_map(|c| corners.iter().map(move |k| *c + *k))
                .collect::<Vec<_>>(),
        ),
        Surface::Points => slab::hull(&centres),
    };
    let margin = surface.margin(tools.look);
    let outline = match surface {
        Surface::Cells(_) | Surface::Points => slab::grown(&playing, pitch * margin, 6),
        // No board: what stands round it (the supply) and the camera keep to the middle of the
        // table, where play is, however far its positions reach.
        Surface::Table(_) => slab::grown(
            &slab::hull(&table_frame(&centres, &corners, pitch)),
            pitch * MARGIN,
            6,
        ),
    };
    let rim = Rim {
        height: BOARD_HEIGHT,
        radius: RIM_RADIUS,
        steps: 5,
    };
    let strips = goal_strips(&table.info, layout, shape);
    let on_table = matches!(surface, Surface::Table(_));
    if outline.len() >= 3 && !on_table {
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
    match surface {
        Surface::Cells(_) => {
            // The lines between the cells: each cell, darkened, under the cells, and grown so the
            // lines round the outside are as thick as those between cells.
            let line_corners = slab::cell_corners(shape, pitch * LINE_SCALE);
            commands.spawn((
                Name::new("Lines"),
                Mesh3d(tools.meshes.add(look::world_uvs(
                    slab::flat_cells(&centres, &line_corners, BOARD_HEIGHT + 0.0001),
                    Vec3::ZERO,
                ))),
                MeshMaterial3d(lines),
                ChildOf(root),
            ));
            let cell_mesh = shape.mesh(pitch, CELL_SCALE, CELL_HEIGHT);
            let cell_outline =
                Outline::space(slab::cell_corners(shape, pitch * CELL_SCALE), CELL_HEIGHT);
            // A chessboard's two materials by turns, if the game's look has them.
            let squares = tools
                .look
                .and_then(|l| l.board.squares.clone())
                .map(|names| {
                    let look = tools.look.expect("a look has the names");
                    let specs = names.map(|n| look.materials.get(&n).cloned());
                    specs.map(|spec| Some(tools.material(&spec?)))
                });
            let turns = squares
                .as_ref()
                .map(|_| by_turns(&table.info.board.positions))
                .unwrap_or_default();
            // A region's cells in their own material, if the game's look gives the region one.
            let regions: HashMap<&str, Handle<StandardMaterial>> = tools
                .look
                .map(|look| {
                    let named = &table.info.board.regions;
                    look.board
                        .regions
                        .iter()
                        .filter_map(|(region, name)| {
                            Some((named.get(region)?, look.materials.get(name)?.clone()))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
                .into_iter()
                .flat_map(|(cells, spec)| {
                    let material = tools.material(&spec);
                    cells.iter().map(move |c| (c.as_str(), material.clone()))
                })
                .collect();
            for (id, centre) in &layout.centres {
                let at = *centre + Vec3::Y * BOARD_HEIGHT;
                let material = regions
                    .get(id.as_str())
                    .cloned()
                    .or_else(|| {
                        squares
                            .as_ref()
                            .and_then(|s| s[turns.get(id).copied().unwrap_or(0)].clone())
                    })
                    .unwrap_or_else(|| wood.clone());
                commands.spawn((
                    Name::new(format!("Cell {id}")),
                    Mesh3d(tools.meshes.add(look::world_uvs(cell_mesh.clone(), at))),
                    MeshMaterial3d(material),
                    Transform::from_translation(at),
                    Cell { id: id.clone() },
                    cell_outline.clone(),
                    Spot(id.clone()),
                    Pickable::default(),
                    ChildOf(root),
                ));
            }
            // A border inlaid round the cells, if the game's look has one.
            let border = tools
                .look
                .and_then(|l| l.materials.get(l.board.border.as_ref()?))
                .cloned();
            if let Some(spec) = border {
                let points: Vec<Vec2> = centres
                    .iter()
                    .flat_map(|c| corners.iter().map(move |k| *c + *k))
                    .collect();
                let band = slab::band(
                    &slab::hull(&points),
                    pitch * BORDER.0,
                    pitch * BORDER.1,
                    BOARD_HEIGHT + 0.0002,
                );
                let material = tools.material(&spec);
                commands.spawn((
                    Name::new("Border"),
                    Mesh3d(tools.meshes.add(look::world_uvs(band, Vec3::ZERO))),
                    MeshMaterial3d(material),
                    ChildOf(root),
                ));
            }
        }
        Surface::Table(_) => {
            // Each position a cell a click lands on, unseen on the table; it glows while a click
            // there would be taken (`highlight.rs`).
            let unseen = tools.materials.add(StandardMaterial {
                base_color: Color::NONE,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            });
            let cell = tools.meshes.add(shape.flat(pitch * CELL_SCALE));
            let lit = Outline::space(slab::cell_corners(shape, pitch * CELL_SCALE), 0.0);
            for (id, centre) in &layout.centres {
                commands.spawn((
                    Name::new(format!("Cell {id}")),
                    Mesh3d(cell.clone()),
                    MeshMaterial3d(unseen.clone()),
                    Transform::from_translation(*centre + Vec3::Y * CELL_LIFT),
                    Cell { id: id.clone() },
                    lit.clone(),
                    Spot(id.clone()),
                    Pickable::default(),
                    ChildOf(root),
                ));
            }
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
            // Each point is a disc a click lands on, unseen; it glows while a click there
            // would be taken (`highlight.rs`).
            let unseen = tools.materials.add(StandardMaterial {
                base_color: Color::NONE,
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
                    Transform::from_translation(*centre + Vec3::Y * (BOARD_HEIGHT + POINT_RAISE)),
                    Cell { id: id.clone() },
                    Outline::point(pitch, 0.0),
                    OnPoint { pitch },
                    Spot(id.clone()),
                    Pickable::default(),
                    ChildOf(root),
                ));
            }
        }
    }

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

    // Each kind of piece the game's appearance gives a model is drawn as that; any other as the
    // generic stone.
    let stone = tools.piece_kit(None);
    let pieces: HashMap<String, PieceKit> = tools
        .look
        .map(|l| &l.pieces)
        .into_iter()
        .flatten()
        .map(|(id, piece)| (id.clone(), tools.piece_kit(Some(piece))))
        .collect();
    let yours = table.yours();
    let mut ghost = yours
        .map(|s| tools.piece_material(&s.color, s.seat))
        .unwrap_or_else(|| look::piece(seat_colour("", 1)))
        .build(tools.server);
    ghost.base_color = ghost.base_color.with_alpha(0.5);
    ghost.alpha_mode = AlphaMode::Blend;
    let materials = piece_materials(table, tools);
    let ghost = tools.materials.add(ghost);
    // Each colour word's see-through material, for what the move being built would put down.
    let ghosts: HashMap<String, Handle<StandardMaterial>> = materials
        .iter()
        .filter_map(|(word, handle)| {
            let mut m = tools.materials.get(handle)?.clone();
            m.base_color = m.base_color.with_alpha(0.5);
            m.alpha_mode = AlphaMode::Blend;
            Some((word.clone(), tools.materials.add(m)))
        })
        .collect();
    let mut dress: HashMap<AssetId<StandardMaterial>, (String, bool)> = HashMap::new();
    dress.insert(
        ghost.id(),
        (yours.map(|s| s.color.clone()).unwrap_or_default(), true),
    );
    for (word, handle) in &materials {
        dress.insert(handle.id(), (word.clone(), false));
    }
    for (word, handle) in &ghosts {
        dress.insert(handle.id(), (word.clone(), true));
    }
    let piece_colour: HashMap<String, String> = table
        .info
        .pieces
        .iter()
        .filter(|p| !p.color.is_empty())
        .map(|p| (p.id.clone(), p.color.clone()))
        .collect();
    // A bowl of each colour's stones beside the board, for a game whose look has them: a click
    // on one takes a stone of the piece in that colour.
    let edge: Vec<Vec2> = outline.iter().map(|(point, _)| *point).collect();
    let mut bowls_at = HashMap::new();
    if let Some(bowls) = tools.look.and_then(|l| l.bowls.as_ref())
        && let Some(spec) = tools.look.and_then(|l| l.materials.get(&bowls.material))
    {
        let bowl = tools.material(&spec.clone());
        let seats: Vec<Vec<crate::bowls::Filled>> = table
            .info
            .seats
            .iter()
            .map(|s| {
                let words = if s.colors.is_empty() {
                    std::slice::from_ref(&s.color)
                } else {
                    s.colors.as_slice()
                };
                words
                    .iter()
                    .map(|word| crate::bowls::Filled {
                        colour: word.clone(),
                        stones: materials.get(word).cloned().unwrap_or_default(),
                        choice: piece_colour
                            .iter()
                            .find(|(_, c)| *c == word)
                            .map(|(id, _)| Gesture {
                                kind: "piece".into(),
                                value: id.clone(),
                            }),
                    })
                    .collect()
            })
            .collect();
        bowls_at = crate::bowls::spawn(commands, root, tools.server, bowls, bowl, seats, &edge);
    }
    // A tray in front of each seat for a game that counts what each captures, a square for each
    // capture its win wants; or, for a game whose look keeps a score with the seats' own pieces,
    // a dot for each point on the margin at each seat's right hand.
    let score = tools.look.and_then(|l| l.score.as_ref());
    let counter = table
        .info
        .counters
        .iter()
        .find(|c| c.max > 0 && c.max <= trays::MOST)
        .filter(|_| score.is_none());
    let tray_size = match score {
        Some(_) => 1.0,
        None => tools.look.and_then(|l| l.trays.size).unwrap_or(trays::SIZE),
    };
    let mut seat_trays = Vec::new();
    let mut front = 0.0;
    let points = score
        .and_then(|s| table.state.state.view_counter_max(1, &s.counter))
        .filter(|n| (1..=trays::MOST).contains(n));
    if let Some(n) = points {
        let seats = &table.info.seats;
        let at = trays::marks(
            seats.len(),
            &playing,
            n as usize,
            pitch * SCORE_OUT,
            pitch * SCORE_STEP,
        );
        let dot = slab::cell_corners(CellShape::Spot, pitch * MARK_SIZE / 0.6);
        let all: Vec<Vec2> = at.iter().flatten().copied().collect();
        let ink = tools.material(&look::groove());
        commands.spawn((
            Name::new("Score's dots"),
            Mesh3d(tools.meshes.add(look::world_uvs(
                slab::flat_cells(&all, &dot, surface.height() + 0.00015),
                Vec3::ZERO,
            ))),
            MeshMaterial3d(ink),
            ChildOf(root),
        ));
        for (s, dots) in seats.iter().zip(at) {
            seat_trays.push(Tray {
                seat: s.color.clone(),
                number: s.seat,
                colours: if s.colors.is_empty() {
                    vec![s.color.clone()]
                } else {
                    s.colors.clone()
                },
                squares: dots
                    .iter()
                    .map(|d| Vec3::new(d.x, surface.height(), d.y))
                    .collect(),
            });
        }
    }
    if let Some(counter) = counter {
        let spec = tools
            .look
            .and_then(|l| l.materials.get(l.trays.material.as_ref()?))
            .cloned()
            .unwrap_or_else(look::tray);
        let [r, g, b] = spec.tint.unwrap_or([1.0; 3]);
        let lines = MaterialSpec {
            tint: Some([r * 0.42, g * 0.33, b * 0.27]),
            ..spec.clone()
        };
        let wood = (tools.material(&spec), tools.material(&lines));
        let (n, side) = (counter.max as usize, pitch * tray_size);
        let depth = trays::half_size(n, side).y;
        let seats = &table.info.seats;
        for (i, place) in trays::places(seats.len(), &edge, depth)
            .into_iter()
            .enumerate()
        {
            if i == 0 {
                front = place.0.y + depth;
            }
            let s = &seats[i];
            let squares = trays::spawn(
                commands,
                tools.meshes,
                root,
                wood.clone(),
                place,
                (n, side),
                i,
            );
            seat_trays.push(Tray {
                seat: s.color.clone(),
                number: s.seat,
                colours: if s.colors.is_empty() {
                    vec![s.color.clone()]
                } else {
                    s.colors.clone()
                },
                squares,
            });
        }
    }
    // Each seat's supply beside the board, for a game whose seats keep pieces to place.
    let mut supply: HashMap<String, Vec<SupplyPool>> = HashMap::new();
    let mut beside = 0.0f32;
    // How far to the first seat's left the supply reaches (the other seat's line, at its right).
    let mut left = edge.iter().fold(0.0f32, |l, p| l.max(-p.x));
    let mut kinds: Vec<(crate::supply::Pool, Option<String>, Gesture)> =
        crate::supply::pools(&table.info)
            .into_iter()
            .map(|k| {
                let take = Gesture::piece(&k.piece);
                (k, None, take)
            })
            .collect();
    // A game whose view counts each seat's pieces left to place, as its look says.
    if kinds.is_empty()
        && let Some((counter, take)) = tools
            .look
            .and_then(|l| l.supply.as_ref())
            .and_then(|s| s.counted())
        && let Some(piece) = table.info.pieces.first()
    {
        // Room for what each seat has left and what it has on the board, the most of any.
        let cells = &table.state.state.cells;
        let start = table
            .info
            .seats
            .iter()
            .map(|s| {
                let left = table.state.state.view_counter(s.seat, counter).unwrap_or(0);
                let on = cells
                    .values()
                    .flatten()
                    .filter(|p| p.color == s.color)
                    .count();
                left + on as u32
            })
            .max()
            .unwrap_or(0);
        if start > 0 {
            kinds.push((
                crate::supply::Pool {
                    id: counter.to_string(),
                    piece: piece.id.clone(),
                    start,
                },
                Some(counter.to_string()),
                Gesture {
                    kind: "action".into(),
                    value: take.to_string(),
                },
            ));
        }
    }
    if !kinds.is_empty() {
        let lie = tools
            .look
            .and_then(|l| l.supply.as_ref())
            .map(|s| s.lie)
            .unwrap_or_default();
        let stances: Vec<(crate::supply::Stance, usize)> = kinds
            .iter()
            .map(|(k, ..)| {
                let model = tools
                    .look
                    .and_then(|l| l.pieces.get(&k.piece))
                    .map_or(look::PIECE_MODEL, |p| p.model.as_str());
                let half = Vec3::new(0.013, STONE_THICKNESS / 2.0, 0.013);
                let (low, high) = assets::model_bounds(model).unwrap_or((-half, half));
                (
                    crate::supply::Stance::of(low, high, pitch / PITCH, lie),
                    k.start as usize,
                )
            })
            .collect();
        let seats = &table.info.seats;
        for (i, seat) in seats.iter().enumerate() {
            let places = crate::supply::places(i, seats.len(), &edge, &stances);
            for (((kind, counter, take), slots), (stance, _)) in
                kinds.iter().zip(places).zip(&stances)
            {
                for t in &slots {
                    beside = beside.max(t.translation.x.abs() + stance.out);
                    if t.translation.x < 0.0 {
                        left = left.max(-t.translation.x + stance.out / 2.0);
                    }
                }
                supply
                    .entry(seat.color.clone())
                    .or_default()
                    .push(SupplyPool {
                        id: kind.id.clone(),
                        piece: kind.piece.clone(),
                        slots,
                        height: stance.height,
                        seat: seat.seat,
                        counter: counter.clone(),
                        take: take.clone(),
                    });
            }
        }
    }
    // The bowl of fences both seats take from, at the first seat's left hand, for a game whose
    // look has fences.
    let fences = tools.look.and_then(|l| l.fences.as_ref()).map(|said| {
        let size = said.size.unwrap_or(1.0);
        let load = |file: &str| -> Handle<Mesh> {
            tools.server.load(
                GltfAssetLabel::Primitive {
                    mesh: 0,
                    primitive: 0,
                }
                .from_asset(assets::path(file)),
            )
        };
        let spec = |name: &str| {
            tools
                .look
                .and_then(|l| l.materials.get(name))
                .cloned()
                .unwrap_or_else(look::groove)
        };
        let (wood, bowl_wood) = (spec(&said.material), spec(&said.bowl_material));
        let (fence_material, bowl_material) = (tools.material(&wood), tools.material(&bowl_wood));
        let radius = crate::bowls::BOWL_RADIUS * size;
        // Shared: beside the middle of the board's side to the first seat's left, beyond any
        // supply there.
        let at = Vec2::new(-(left + FENCE_BOWL_GAP + radius), 0.0);
        beside = beside.max(at.x.abs() + radius);
        let choices: Vec<Gesture> = said
            .sides
            .keys()
            .map(|id| Gesture {
                kind: "action".into(),
                value: id.clone(),
            })
            .collect();
        commands.spawn((
            Name::new("Bowl of fences"),
            Mesh3d(load(&said.bowl)),
            MeshMaterial3d(bowl_material),
            Transform::from_xyz(at.x, 0.0, at.y).with_scale(Vec3::splat(size)),
            Pickable::default(),
            TakeUp(choices),
            ChildOf(root),
            children![(
                Mesh3d(load(&said.heap)),
                MeshMaterial3d(fence_material.clone()),
                Pickable::default(),
            )],
        ));
        let rim = assets::model_span(&said.bowl).map_or(0.04, |(_, high)| high) * size;
        let (low, _) = assets::model_span(&said.model).unwrap_or((-0.003, 0.003));
        let mut ghost = wood.on_board(tools.wood).build(tools.server);
        ghost.base_color = ghost.base_color.with_alpha(0.6);
        ghost.alpha_mode = AlphaMode::Blend;
        commands.spawn((
            Name::new("Fence aimed"),
            FenceGhost,
            Mesh3d(load(&said.model)),
            MeshMaterial3d(tools.materials.add(ghost)),
            Transform::default(),
            Visibility::Hidden,
            ChildOf(root),
        ));
        FenceKit {
            mesh: load(&said.model),
            material: fence_material,
            foot: low,
            over: Vec3::new(at.x, rim, at.y),
            sides: said
                .sides
                .iter()
                .map(|(id, [x, y])| (id.clone(), Vec2::new(*x, *y)))
                .collect(),
        }
    });
    // How the view's tints are drawn: a piece of the look laid on the position, lit as a cell
    // is while a click there would be taken; or a stamp printed on it.
    let mut tint_kits = HashMap::new();
    for (key, said) in tools.look.map(|l| &l.tints).into_iter().flatten() {
        let kit = match Tinted::of(said) {
            Some(Tinted::Laid(piece)) => {
                // The tile's top, as its model is: the board's cell's shape, as wide across as
                // the model.
                let bounds = tools
                    .look
                    .and_then(|l| l.pieces.get(&piece))
                    .and_then(|p| assets::model_bounds(&p.model));
                let (across, top) =
                    bounds.map_or((PITCH * CELL_SCALE, 0.0), |(lo, hi)| (hi.x - lo.x, hi.y));
                TintKit::Laid {
                    piece,
                    outline: Outline::space(slab::cell_corners(shape, across), top),
                }
            }
            Some(Tinted::Stamp { material, size }) => {
                let Some(spec) = tools.look.and_then(|l| l.materials.get(&material)).cloned()
                else {
                    continue;
                };
                TintKit::Stamp {
                    mesh: tools.meshes.add(
                        Circle::new(pitch * size / 2.0)
                            .mesh()
                            .resolution(48)
                            .build()
                            .rotated_by(Quat::from_rotation_x(-FRAC_PI_2)),
                    ),
                    material: tools.material(&spec),
                }
            }
            None => continue,
        };
        tint_kits.insert(key.clone(), kit);
    }
    // What both seats have still to place, beside the board to the first seat's left, beyond
    // anything there: room for as many of each kind as the view counts now.
    let stock = tools.look.and_then(|l| l.stock.as_ref()).and_then(|said| {
        let counts = table.state.state.view_counts(&said.counts);
        if counts.is_empty() {
            return None;
        }
        let fence_bowl = fences
            .as_ref()
            .map_or(0.0, |f| -f.over.x + crate::bowls::BOWL_RADIUS);
        let (slots, reach) = tints::stock(&counts, pitch, left.max(fence_bowl));
        beside = beside.max(reach);
        Some(StockKit {
            counts: said.counts.clone(),
            slots,
        })
    });
    let pools: HashMap<String, String> = table
        .info
        .pieces
        .iter()
        .filter(|p| !p.pool.is_empty())
        .map(|p| (p.id.clone(), p.pool.clone()))
        .collect();
    let size = if on_table {
        let (lo, hi) = outline.iter().fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(lo, hi), (p, _)| (lo.min(*p), hi.max(*p)),
        );
        hi - lo
    } else {
        layout.size
    };
    let kit = Kit {
        edge: playing,
        outline: edge,
        size,
        on_table,
        fences,
        supply,
        pools,
        beside,
        stone,
        pieces,
        scale: pitch / PITCH,
        materials,
        facing: facings(&table.info),
        ghost,
        woodwork: std::mem::take(&mut tools.woodwork),
        surface: surface.height(),
        aimed: tools.look.and_then(|l| l.aimed.clone()),
        leaves: tools
            .look
            .map(|l| {
                l.pieces
                    .iter()
                    .filter_map(|(id, p)| Some((id.clone(), p.leaves.clone()?)))
                    .collect()
            })
            .unwrap_or_default(),
        bowls: bowls_at,
        piece_colour,
        dress,
        ghosts,
        trays: seat_trays,
        counter: counter.map(|c| c.id.clone()).unwrap_or_default(),
        tray_scale: pitch / PITCH * tray_size,
        score: score.map(|s| (s.counter.clone(), s.piece.clone())),
        front,
        tints: tint_kits,
        stock,
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
    // A region every seat shares (both of Tak's pairs of sides) is nobody's to colour: shading it
    // would say nothing, so only a seat's own goals are marked.
    let regions: Vec<Region> = owners
        .iter()
        .filter(|(_, seat)| seat.is_some())
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

/// One material per colour word: each seat's, each the game's look gives one though no seat
/// plays it (a piece that is nobody's), and each a piece on the board has now. A piece in any
/// other colour is drawn in the default material, which none of these is.
fn piece_materials(table: &Table, tools: &mut Tools) -> HashMap<String, Handle<StandardMaterial>> {
    let mut words: Vec<(String, u32)> = table
        .info
        .seats
        .iter()
        .map(|s| (s.color.clone(), s.seat))
        .collect();
    let others = tools
        .look
        .map(|l| l.seats.keys())
        .into_iter()
        .flatten()
        .chain(table.state.state.cells.values().flatten().map(|p| &p.color));
    for word in others {
        if !words.iter().any(|(known, _)| known == word) {
            // Past the seats, so a word that is not a colour gets one of its own.
            words.push((word.clone(), words.len() as u32 + 1));
        }
    }
    words
        .into_iter()
        .map(|(word, seat)| {
            let spec = tools.piece_material(&word, seat);
            let material = tools.material(&spec);
            (word, material)
        })
        .collect()
}

/// The way each seat looks across the board, by its colour word: the first from the player's
/// side to the far side, the others from their places round the table
/// (`environment::seat_places`).
fn facings(info: &GameInfo) -> HashMap<String, f32> {
    let seats = info.seats.len().max(1) as f32;
    info.seats
        .iter()
        .enumerate()
        .map(|(i, s)| (s.color.clone(), TAU * i as f32 / seats))
        .collect()
}

/// Lights the cells a click would be taken on (when the player wants that, and always while a
/// piece is in hand), and shows the move being built: each piece it moves, held or put down;
/// what it puts down new, see-through; and what it aims, held under the pointer.
#[allow(clippy::too_many_arguments)]
/// What the move aims, held under the pointer (not a fence, nor a stone in hand).
type AimedHeld<'w, 's> = Query<
    'w,
    's,
    Entity,
    (
        With<Held>,
        Without<HeldFence>,
        Without<FromBowl>,
        Without<FromStock>,
        Without<PieceAt>,
    ),
>;

/// A piece as `show_turn` sees it: where it is, where a click on it is taken, how the move
/// being built carries it, and whether it is held under the pointer.
type TurnPiece = (
    Entity,
    &'static PieceAt,
    &'static mut Spot,
    Option<&'static Carry>,
    Has<Held>,
);

/// A cell as `show_turn` lights it: whether it is lit, whether a tile lies on it, and the tone
/// of its light.
type LitCell = (
    Entity,
    &'static Cell,
    Has<Lit>,
    Has<TintAt>,
    Option<&'static Tone>,
    (Option<&'static OnPoint>, &'static Outline),
);

/// A point of a board played on points, `pitch` apart: lit as a dot while nothing stands on
/// it, and round what stands there, at its top, while something does (`point_outline`).
#[derive(Component, Debug, Clone, Copy)]
struct OnPoint {
    pitch: f32,
}

/// How far out from a point what stands on it is lit, in pitches: just beyond a ring's rim.
const ON_POINT_LIT: f32 = 0.47;

/// A point's outline as it is lit: a dot when nothing stands on it (whose stack is `under`
/// high, in the cell's units), else round the piece, at its top, so the light is not hidden
/// under it.
fn point_outline(pitch: f32, under: Option<f32>) -> Outline {
    match under {
        None => Outline::point(pitch, 0.0),
        Some(top) => Outline::space(
            (0..24)
                .map(|i| Vec2::from_angle(-(i as f32) * TAU / 24.0) * pitch * ON_POINT_LIT)
                .collect(),
            top,
        ),
    }
}

/// Which line a click would choose, where the selections are lines chosen by what they take
/// (`Turn::lines`): of those through the piece under the pointer, the one whose middle is
/// nearest it (`play::nearest_line`).
fn line_aim(
    watch: Res<WatchStatus>,
    hovering: Res<Hovering>,
    pointer: Res<PointerOnBoard>,
    roots: Query<&Kit, With<BoardRoot>>,
    mut aim: ResMut<LineAim>,
) {
    let wanted = (|| {
        let table = watch.table.as_ref()?;
        let lines = watch.turn.as_ref()?.lines();
        let at = hovering.0.as_deref()?;
        let kit = roots.single().ok()?;
        let (hit, eye) = pointer.0?;
        let on = crate::hand::on_surface(hit, eye, kit.surface);
        let layout = Layout::new(&table.info.board.positions);
        let centre = |c: &str| layout.centres.get(c).map(|v| (v.x, v.z));
        crate::play::nearest_line(&lines, at, (on.x, on.z), centre).cloned()
    })();
    if aim.0 != wanted {
        aim.0 = wanted;
    }
}

fn show_turn(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    (hovering, legal, line): (Res<Hovering>, Res<ShowLegalMoves>, Res<LineAim>),
    roots: Query<(Entity, &Kit), With<BoardRoot>>,
    cells: Query<LitCell>,
    mut pieces: Query<TurnPiece>,
    (ghosts, held, in_flight): (Query<Entity, With<Ghost>>, AimedHeld, Query<&FromBowl>),
) {
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
    let (Some(table), Ok((root, kit))) = (&watch.table, roots.single()) else {
        return;
    };
    let turn = watch.turn.as_ref();
    let targets = turn.map(|t| t.targets()).unwrap_or_default();
    let lit = legal.0 || turn.is_some_and(Turn::holding);
    // Where a tile in hand would go is shown by the tile, see-through (`show_tints`), not lit.
    let previewed = hovering
        .0
        .as_deref()
        .filter(|_| turn.is_some_and(Turn::taken_after_click));
    // A cell a tile lies on is lit on the tile.
    let tiled: Vec<&str> = cells
        .iter()
        .filter(|(_, _, _, on_tile, ..)| *on_tile)
        .map(|(_, c, ..)| c.id.as_str())
        .collect();
    let stacks = &table.state.state.cells;
    let floors = table.state.state.tints();
    for (entity, cell, was, on_tile, tone, (point, outline)) in &cells {
        let id = cell.id.as_str();
        // A line pointed at is lit alone, whether the legal moves are or not.
        let wanted = match &line.0 {
            Some(aimed) => aimed.captures.contains(&cell.id),
            None => lit && targets.contains(&cell.id),
        } && previewed != Some(id)
            && (on_tile || !tiled.contains(&id));
        let toned = if wanted && turn.is_some_and(|t| t.takes_at(id)) {
            Tone::Take
        } else {
            Tone::Plain
        };
        // A point is lit round what stands on it; drawn again when that changes (`light`
        // draws a position again when its tone is set).
        let reshaped = point
            .filter(|_| wanted)
            .map(|p| {
                let under = stacks.get(id).filter(|s| !s.is_empty()).map(|s| {
                    (kit.floor(&floors, id) + kit.stack_height(s)) * kit.scale - POINT_RAISE
                });
                point_outline(p.pitch, under)
            })
            .filter(|o| o != outline);
        if let Some(o) = reshaped {
            commands.entity(entity).insert((o, toned, Lit));
        } else if wanted && !was {
            commands.entity(entity).insert((Lit, toned));
        } else if !wanted && was {
            commands.entity(entity).remove::<(Lit, Tone)>();
        } else if wanted && tone != Some(&toned) {
            commands.entity(entity).insert(toned);
        }
    }

    let layout = Layout::new(&table.info.board.positions);
    let cells = &table.state.state.cells;
    let tints = table.state.state.tints();
    // The position as the move being built leaves it, so what it puts down stands on what will be
    // there: each piece it moves taken off its space, and on where it is put.
    let mut after = cells.clone();
    let mut carried: HashMap<(String, usize), (Carry, String)> = HashMap::new();
    // Which piece of the position stands where the move being built has put it (by its place in
    // the position), so one moved again in the same impulse (a unit's second action) is the
    // same piece, carried on.
    let mut origin: HashMap<(String, usize), (String, usize)> = HashMap::new();
    for c in turn.map(Turn::carried).unwrap_or_default() {
        let Some(piece) = after.get_mut(c.from).and_then(Vec::pop) else {
            continue;
        };
        let layer = after[c.from].len();
        let key = origin
            .remove(&(c.from.to_string(), layer))
            .unwrap_or_else(|| (c.from.to_string(), layer));
        let (carry, at) = match c.to.and_then(|to| Some((to, layout.centres.get(to)?))) {
            Some((to, centre)) => {
                let stack = after.entry(to.to_string()).or_default();
                let put = kit.set_down(
                    &piece.piece,
                    &piece.color,
                    *centre,
                    (to, stack.len()),
                    kit.floor(&tints, to) + kit.stack_height(stack.iter()),
                );
                origin.insert((to.to_string(), stack.len()), key.clone());
                stack.push(piece);
                (Carry::Put(put), to)
            }
            None => {
                after.get_mut(c.from).expect("taken from it").push(piece);
                origin.insert((c.from.to_string(), layer), key.clone());
                let lift = LIFT * kit.scale;
                (
                    Carry::Held {
                        lift,
                        scale: kit.scale,
                    },
                    c.from,
                )
            }
        };
        carried.insert(key, (carry, at.to_string()));
    }
    // A stack moved: as many of its top pieces as are carried, lifted over it; each dropped
    // carried to where it was dropped, bottom first; what is left held over the last of those.
    for m in turn.map(Turn::stack_moves).unwrap_or_default() {
        let height = after.get(m.from).map_or(0, Vec::len);
        if m.count == 0 || m.count > height {
            continue;
        }
        let taken = after
            .get_mut(m.from)
            .expect("a stack there")
            .split_off(height - m.count);
        let lift = LIFT * kit.scale;
        let mut over: Option<(&str, f32)> = None;
        for (k, piece) in taken.into_iter().enumerate() {
            let layer = height - m.count + k;
            let key = (m.from.to_string(), layer);
            if let Some(&to) = m.drops.get(k)
                && let Some(centre) = layout.centres.get(to)
            {
                let stack = after.entry(to.to_string()).or_default();
                let put = kit.set_down(
                    &piece.piece,
                    &piece.color,
                    *centre,
                    (to, stack.len()),
                    kit.floor(&tints, to) + kit.stack_height(stack.iter()),
                );
                stack.push(piece);
                carried.insert(key, (Carry::Put(put), to.to_string()));
                over = Some((to, 0.0));
                continue;
            }
            match over {
                // Nothing dropped yet: lifted where it stands.
                None => {
                    carried.insert(
                        key,
                        (
                            Carry::Held {
                                lift,
                                scale: kit.scale,
                            },
                            m.from.to_string(),
                        ),
                    );
                }
                // Held over the last space dropped on, on what will be there.
                Some((cell, ref mut above)) => {
                    let Some(centre) = layout.centres.get(cell) else {
                        continue;
                    };
                    let stack = after.get(cell).map_or(&[][..], Vec::as_slice);
                    let mut at = kit.set_down(
                        &piece.piece,
                        &piece.color,
                        *centre,
                        (cell, stack.len() + k),
                        kit.floor(&tints, cell) + kit.stack_height(stack) + *above,
                    );
                    at.translation.y += lift;
                    *above += kit.piece(&piece.piece).height;
                    carried.insert(
                        key,
                        (
                            Carry::Over {
                                at,
                                scale: kit.scale,
                            },
                            cell.to_string(),
                        ),
                    );
                }
            }
        }
    }
    // The piece picked up to be moved (its selections do not have it clicked; or it leaves
    // something where it stood, as the game's look says): under the pointer, in place of its
    // arrow, and not in the way of a click on where it goes. Where the move being built has put
    // it, if it has moved it already.
    let leaves = |from: &str| {
        cells
            .get(from)
            .and_then(|s| s.last())
            .is_some_and(|p| kit.leaves.contains_key(&p.piece))
    };
    let picked = turn.and_then(|t| {
        t.carried()
            .into_iter()
            .find(|c| c.to.is_none() && leaves(c.from))
            .map(|c| c.from)
    });
    let lifted = turn.and_then(Turn::lifted).or(picked).and_then(|from| {
        let top = (from.to_string(), after.get(from)?.len().checked_sub(1)?);
        Some(origin.get(&top).cloned().unwrap_or(top))
    });
    for (entity, at, mut spot, now, in_hand) in &mut pieces {
        let up = lifted
            .as_ref()
            .is_some_and(|(cell, layer)| *cell == at.cell && *layer == at.layer);
        if up {
            if !in_hand {
                let kind = kit.piece(&at.piece.piece);
                commands.entity(entity).remove::<Carry>().insert((
                    Held {
                        board: kit.surface,
                        lift: (LIFT - kind.foot) * kit.scale,
                        scale: kit.scale,
                    },
                    Pickable::IGNORE,
                ));
            }
            continue;
        }
        if in_hand {
            commands
                .entity(entity)
                .remove::<Held>()
                .insert(Pickable::default());
        }
        match carried.get(&(at.cell.clone(), at.layer)) {
            Some((carry, cell)) => {
                // Only when it changes: a piece put down is carried there once (from under
                // the pointer, if it was held there).
                if now != Some(carry) || in_hand {
                    commands.entity(entity).insert(*carry);
                }
                // A click on a piece put down is a click on where it is now.
                if spot.0 != *cell {
                    spot.0 = cell.clone();
                }
            }
            None => {
                if now.is_some() {
                    commands.entity(entity).remove::<Carry>();
                }
                if spot.0 != at.cell {
                    spot.0 = at.cell.clone();
                }
            }
        }
    }

    let colour = table.yours().map_or("", |s| s.color.as_str());
    // What the move aims, held under the pointer while it is next: what the game's look says
    // it puts down. The server says only where it aims; without the look, nothing is put
    // there (Fist's pin captures the piece aimed at where it stands), and nothing is held.
    let aimed = kit.aimed.as_deref();
    let aiming = turn.is_some_and(Turn::aiming);
    match (aimed.filter(|_| aiming), held.iter().next()) {
        (Some(id), None) => {
            let kind = kit.piece(id);
            let material = kind
                .material
                .clone()
                .or_else(|| kit.materials.get(colour).cloned())
                .unwrap_or_default();
            commands.spawn((
                Name::new("Held"),
                Held {
                    board: kit.surface,
                    lift: (2.0 * LIFT - kind.foot) * kit.scale,
                    scale: kit.scale,
                },
                Mesh3d(kind.mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_scale(Vec3::splat(kit.scale)),
                Visibility::Hidden,
                ChildOf(root),
            ));
        }
        (None, Some(entity)) => commands.entity(entity).despawn(),
        _ => {}
    }

    let Some(turn) = turn else { return };
    // Over a piece a move may start with that leaves another where it stood (a ring, which
    // leaves a disc), what it would leave, see-through in its place: where the one held under
    // the pointer would go.
    if let Some(cell) = hovering.0.as_deref()
        && turn.moves_only().contains(&cell)
        && let Some(stack) = cells.get(cell).filter(|s| !s.is_empty())
        && let Some(centre) = layout.centres.get(cell)
    {
        let (moved, under) = stack.split_last().expect("not empty");
        if let Some(left) = kit.leaves.get(&moved.piece) {
            let kind = kit.piece(left);
            commands.spawn((
                Name::new(format!("{left} to leave on {cell}")),
                Ghost,
                Mesh3d(kind.mesh.clone()),
                MeshMaterial3d(
                    kind.ghost
                        .clone()
                        .or_else(|| kit.ghosts.get(&moved.color).cloned())
                        .unwrap_or_else(|| kit.ghost.clone()),
                ),
                kit.set_down(
                    left,
                    &moved.color,
                    *centre,
                    (cell, under.len()),
                    kit.floor(&tints, cell) + kit.stack_height(under),
                ),
                ChildOf(root),
            ));
        }
    }
    let mut shown: Vec<&str> = match turn.stage {
        Stage::Asking | Stage::Ready | Stage::Sending => turn.placed().collect(),
        Stage::Choosing | Stage::Failed => Vec::new(),
    };
    // Under the pointer too, where a click would put something (not where it would pick one up).
    // Not before anything is in hand, where what would go there comes from a bowl or the
    // supply: it is shown once it is taken.
    let in_hand = turn
        .in_hand_choice()
        .and_then(|g| kit.source_for(g, colour))
        .map(|(_, piece)| piece)
        .or_else(|| turn.auto_hand().map(String::from));
    // Nor where a click could place one of several kinds (Courtfall's tiles, chosen on the panel
    // once the hex is clicked): which is not known yet.
    let one_kind = |cell: &str| {
        let mut kinds = turn.could_place(cell).filter_map(|s| s.piece.as_deref());
        kinds.next().is_none_or(|first| kinds.all(|k| k == first))
    };
    // Where the piece every click moves would go, but not where a click acts on another piece
    // (Courtfall's strike, shove or shot at a unit): it does not go there.
    let acts_on = |cell: &str| {
        turn.unclicked_from().is_some() && cells.get(cell).is_some_and(|s| !s.is_empty())
    };
    if let Some(cell) = hovering.0.as_deref().filter(|c| targets.contains(*c))
        && !acts_on(cell)
        && turn.arriving(cell).is_some()
        && (in_hand.is_some()
            || (one_kind(cell)
                && !turn
                    .could_place(cell)
                    .any(|s| kit.source_of_placing(s, colour).is_some())))
    {
        shown.push(cell);
    }
    // What is placed from a bowl is carried there solid (`bowl_stones`), not shown see-through.
    let from_bowls: Vec<&str> = turn
        .placings()
        .filter(|s| kit.source_of_placing(s, colour).is_some())
        .filter_map(|s| s.to.as_deref())
        .collect();
    // Nor is what the stone held under the pointer is carried to (it is about to be, this frame).
    let from_hand = in_flight
        .iter()
        .any(|f| f.source == HAND && f.cell.is_none());
    // Nor what is placed from the stock beside the board: it is carried there (`show_tints`).
    let from_stock: Vec<&str> = kit
        .stock
        .as_ref()
        .map(|stock| {
            turn.placings()
                .filter(|s| {
                    s.piece
                        .as_ref()
                        .is_some_and(|p| stock.slots.iter().any(|(k, _)| k == p))
                })
                .filter_map(|s| s.to.as_deref())
                .collect()
        })
        .unwrap_or_default();
    shown.retain(|cell| {
        let made = turn.placings().any(|s| s.to.as_deref() == Some(*cell));
        if from_stock.contains(cell) {
            return false;
        }
        !from_bowls.contains(cell)
            && !in_flight.iter().any(|f| f.cell.as_deref() == Some(*cell))
            && !(made && from_hand)
    });
    for cell in shown {
        let Some(centre) = layout.centres.get(cell) else {
            continue;
        };
        // The kind of piece that would stand there: the one the selections name, the one that
        // would come from where they say, or what the move aims.
        let coming = turn.arriving(cell).unwrap_or_default();
        // Where the move aims, what the look says it puts there, or nothing.
        if coming.aimed && aimed.is_none() {
            continue;
        }
        // A selection's piece is what arrives only when it is a kind of piece: a move made by
        // a card names the card there, and what arrives is the piece standing where it starts.
        let a_piece = |id: &&str| {
            kit.pieces.contains_key(*id) || table.info.pieces.iter().any(|p| p.id == *id)
        };
        let id = if coming.aimed {
            aimed
        } else {
            coming
                .piece
                .filter(a_piece)
                .or_else(|| Some(cells.get(coming.from?)?.last()?.piece.as_str()))
                // What is in hand, when the selections do not say (Fendo's pawn, added).
                .or(in_hand.as_deref())
        }
        .unwrap_or_default();
        let stack = after.get(cell).map_or(&[][..], Vec::as_slice);
        let at = kit.set_down(
            id,
            colour,
            *centre,
            (cell, stack.len()),
            kit.floor(&tints, cell) + kit.stack_height(stack),
        );
        let kind = kit.piece(id);
        let word = kit.colour_of(id, placing_colour(table));
        // Once the move is sent it is shown as played, solid, without waiting for the server.
        let sent = turn.stage == Stage::Sending;
        let material = if sent {
            kind.material
                .clone()
                .or_else(|| kit.materials.get(word).cloned())
                .unwrap_or_default()
        } else {
            kind.ghost
                .clone()
                .or_else(|| kit.ghosts.get(word).cloned())
                .unwrap_or_else(|| kit.ghost.clone())
        };
        let mut ghost = commands.spawn((
            Name::new(format!("Ghost at {cell}")),
            Ghost,
            Mesh3d(kind.mesh.clone()),
            MeshMaterial3d(material),
            at,
            ChildOf(root),
        ));
        if sent {
            ghost.insert(Sent(cell.to_string()));
        }
    }
}

/// A piece the move being built takes, shown see-through.
#[derive(Component)]
struct Taken;

/// The pieces the move being built takes are see-through, so the player sees what the move
/// they confirm captures; the others as they are.
fn show_taken(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    kits: Query<&Kit, With<BoardRoot>>,
    mut pieces: Query<(
        Entity,
        &PieceAt,
        &mut MeshMaterial3d<StandardMaterial>,
        Has<Taken>,
    )>,
) {
    let (Some(table), Ok(kit)) = (&watch.table, kits.single()) else {
        return;
    };
    // What the server says the move takes (`Turn::taking`), but where a piece the move carries
    // lands on it (the piece arriving stands in its place), or where one it carries away stood
    // (what is taken there is what it left behind: `bowl_stones`).
    let taken: Vec<&str> = watch
        .turn
        .as_ref()
        .map(|t| {
            let landing: Vec<&str> = t.made.iter().filter_map(|s| s.to.as_deref()).collect();
            let left: Vec<&str> = t.carried().iter().map(|c| c.from).collect();
            t.taking()
                .into_iter()
                .filter(|c| !landing.contains(c) && !left.contains(c))
                .collect()
        })
        .unwrap_or_default();
    let cells = &table.state.state.cells;
    for (entity, at, mut material, was) in &mut pieces {
        let top = cells.get(&at.cell).map_or(0, |s| s.len().saturating_sub(1));
        let now = at.layer == top && taken.contains(&at.cell.as_str());
        if !now && !was {
            continue;
        }
        let kind = kit.piece(&at.piece.piece);
        let colour = &at.piece.color;
        // Its colour may change while it is taken (a disc turned back over as the preview
        // changes): it stays see-through in that one.
        let wanted = if now {
            kind.ghost
                .clone()
                .or_else(|| kit.ghosts.get(colour).cloned())
                .unwrap_or_else(|| kit.ghost.clone())
        } else {
            kind.material
                .clone()
                .or_else(|| kit.materials.get(colour).cloned())
                .unwrap_or_default()
        };
        if now && !was {
            commands.entity(entity).insert(Taken);
        } else if !now {
            commands.entity(entity).remove::<Taken>();
        }
        // A piece of two sides wears its upper side's own once dressed (`dress_sides`).
        let dressed = kind.sides.as_ref().map(|s| {
            let parts = if now { &s.ghost } else { &s.solid };
            crate::sides::halves(parts, s.turned.get(colour).copied().unwrap_or(false)).0
        });
        if material.0 != wanted && dressed.as_ref() != Some(&material.0) {
            material.0 = wanted;
        }
    }
}

/// A piece taken from a bowl or a seat's supply (its source, `Kit::source_of`): waiting over
/// it, or carried to the cell it is placed on.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct FromBowl {
    source: String,
    piece: String,
    cell: Option<String>,
}

/// The source of the stone a turn holds by itself in a game with no bowl to take it from.
const HAND: &str = "hand";

/// The colour word the pieces this client's seat places are played in: its own, but in an
/// opening in which each seat places another's (Tak's first stones), the other seat's.
fn placing_colour(table: &Table) -> &str {
    let yours = table.yours().map_or("", |s| s.color.as_str());
    let opening = table
        .info
        .opening
        .as_ref()
        .is_some_and(|o| o.owner == "opponent" && table.state.ply < o.plies);
    if !opening {
        return yours;
    }
    table
        .info
        .seats
        .iter()
        .map(|s| s.color.as_str())
        .find(|c| *c != yours)
        .unwrap_or(yours)
}

/// How many pieces the move being built takes from `source`: those it places, and the one in
/// hand.
fn taken_from(turn: Option<&Turn>, kit: &Kit, yours: &str, source: &str) -> usize {
    let Some(turn) = turn else { return 0 };
    let placed = turn
        .placings()
        .filter(|s| {
            kit.source_of_placing(s, yours)
                .is_some_and(|(from, _)| from == source)
        })
        .count();
    let held = turn
        .in_hand_choice()
        .and_then(|g| kit.source_for(g, yours))
        .is_some_and(|(from, _)| from == source);
    placed + usize::from(held)
}

/// The pieces the move being built takes from bowls or the supply: the one in hand waits over
/// where it was taken from, lifted and bobbing (another kind of the same pool chosen with the
/// wheel, it becomes that); each placed is carried from there to its cell and set down, solid.
/// One in hand that is then placed goes on from where it waits.
fn bowl_stones(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    roots: Query<(Entity, &Kit), With<BoardRoot>>,
    stones: Query<(Entity, &FromBowl)>,
) {
    let mut left: Vec<(Entity, FromBowl)> = stones.iter().map(|(e, f)| (e, f.clone())).collect();
    let (Some(table), Some(turn), Ok((root, kit))) =
        (&watch.table, watch.turn.as_ref(), roots.single())
    else {
        for (entity, _) in left {
            commands.entity(entity).despawn();
        }
        return;
    };
    let yours = table.yours().map_or("", |s| s.color.as_str());
    let placing = placing_colour(table);
    let layout = Layout::new(&table.info.board.positions);
    let cells = &table.state.state.cells;
    let had_hand = left.iter().any(|(_, f)| f.source == HAND);
    let mut take = |found: &dyn Fn(&FromBowl) -> bool| -> Option<(Entity, FromBowl)> {
        let i = left.iter().position(|(_, f)| found(f))?;
        Some(left.remove(i))
    };
    let looks = |id: &str| {
        let kind = kit.piece(id);
        let colour = kit.colour_of(id, placing);
        let material = kind
            .material
            .clone()
            .or_else(|| kit.materials.get(colour).cloned())
            .unwrap_or_default();
        (Mesh3d(kind.mesh.clone()), MeshMaterial3d(material))
    };
    // Where a piece from `source` waits: just over its bowl's rim, or over the place in the
    // supply it was taken from (the first left empty).
    let over = |source: &str| {
        let at = match kit.supply_pool(source) {
            Some(pool) => {
                let colour = source.split_once('/').map_or("", |(c, _)| c);
                let have = pool.count(table, colour);
                let shown = have.saturating_sub(taken_from(Some(turn), kit, yours, source));
                let slot = pool
                    .slots
                    .get(shown.min(pool.slots.len().saturating_sub(1)));
                slot.map_or(Vec3::ZERO, |t| t.translation.with_y(pool.height))
            }
            None => kit.bowls.get(source).copied().unwrap_or_default(),
        };
        Transform::from_translation(at + Vec3::Y * 0.012 * kit.scale)
            .with_scale(Vec3::splat(kit.scale))
    };
    for s in turn.placings() {
        let cell = s.to.as_deref().unwrap_or_default();
        let Some(centre) = layout.centres.get(cell) else {
            continue;
        };
        // From a bowl or the supply, or else the stone that was held under the pointer.
        let Some((source, id)) = kit
            .source_of_placing(s, yours)
            .or_else(|| had_hand.then(|| (HAND.to_string(), s.piece.clone().unwrap_or_default())))
        else {
            continue;
        };
        let id = id.as_str();
        if take(&|f| f.cell.as_deref() == Some(cell)).is_some() {
            continue;
        }
        let stack = cells.get(cell).map_or(&[][..], Vec::as_slice);
        let to = kit.set_down(
            id,
            kit.colour_of(id, placing),
            *centre,
            (cell, stack.len()),
            kit.floor(&table.state.state.tints(), cell) + kit.stack_height(stack),
        );
        let placed = FromBowl {
            source: source.clone(),
            piece: id.to_string(),
            cell: Some(cell.to_string()),
        };
        match take(&|f| f.cell.is_none() && f.source == source) {
            // The piece in hand goes on from where it waits (or from under the pointer), as the
            // kind placed.
            Some((entity, held)) => {
                let mut e = commands.entity(entity);
                if held.piece != id {
                    e.insert(looks(id));
                }
                e.remove::<Held>().insert((
                    placed,
                    Rest(to),
                    Carry::Put(to),
                    Visibility::Inherited,
                ));
            }
            None => {
                let at = over(&source);
                commands.spawn((
                    Name::new(format!("{id} for {cell}")),
                    placed,
                    looks(id),
                    at,
                    Rest(at),
                    Carry::Put(to),
                    ChildOf(root),
                ));
            }
        }
    }
    // What a move leaves where it started, as the game's look says (a disc dropped in the
    // ring that then moves on): put down there from the hand as the piece is picked up, in the
    // mover's colour, and see-through while a later selection of the move takes it off again
    // (in a row that the move made).
    let tints = table.state.state.tints();
    let taking = turn.taking();
    for c in turn.carried() {
        let Some(stack) = cells.get(c.from).filter(|s| !s.is_empty()) else {
            continue;
        };
        let (moved, under) = stack.split_last().expect("not empty");
        let (Some(left), Some(centre)) = (kit.leaves.get(&moved.piece), layout.centres.get(c.from))
        else {
            continue;
        };
        let to = kit.set_down(
            left,
            &moved.color,
            *centre,
            (c.from, under.len()),
            kit.floor(&tints, c.from) + kit.stack_height(under),
        );
        let kind = kit.piece(left);
        let material = if taking.contains(c.from) {
            kind.ghost
                .clone()
                .or_else(|| kit.ghosts.get(&moved.color).cloned())
                .unwrap_or_else(|| kit.ghost.clone())
        } else {
            kind.material
                .clone()
                .or_else(|| kit.materials.get(&moved.color).cloned())
                .unwrap_or_default()
        };
        let placed = FromBowl {
            source: HAND.to_string(),
            piece: left.clone(),
            cell: Some(c.from.to_string()),
        };
        let looks = (Mesh3d(kind.mesh.clone()), MeshMaterial3d(material));
        if let Some((entity, _)) = take(&|f| f.cell.as_deref() == Some(c.from)) {
            commands.entity(entity).insert(looks.1);
            continue;
        }
        match take(&|f| f.cell.is_none() && f.source == HAND) {
            // From under the pointer, where it was held.
            Some((entity, _)) => {
                commands.entity(entity).remove::<Held>().insert((
                    placed,
                    looks,
                    Rest(to),
                    Carry::Put(to),
                    Visibility::Inherited,
                ));
            }
            None => {
                commands.spawn((
                    Name::new(format!("{left} left on {}", c.from)),
                    placed,
                    looks,
                    to,
                    Rest(to),
                    ChildOf(root),
                ));
            }
        }
    }
    // What a move leaves, held before the piece it moves is picked up: every move begins by
    // picking up a piece that leaves the same kind.
    let leaving = {
        let mut kinds = turn.moves_only().into_iter().map(|from| {
            cells
                .get(from)
                .and_then(|s| s.last())
                .and_then(|p| kit.leaves.get(&p.piece))
        });
        match kinds.next() {
            Some(Some(first)) if kinds.all(|k| k == Some(first)) => Some(first.as_str()),
            _ => None,
        }
    };
    // The stone a turn holds by itself (every click places one), or what a move leaves: under
    // the pointer, in place of its arrow, from its bowl's source if it has one.
    if let Some(piece) = turn.auto_hand().or(leaving) {
        let source = kit
            .source_for(&Gesture::piece(piece), yours)
            .map_or_else(|| HAND.to_string(), |(s, _)| s);
        let held = FromBowl {
            source: source.clone(),
            piece: piece.to_string(),
            cell: None,
        };
        if take(&|f| f.cell.is_none() && f.source == source).is_none() {
            let kind = kit.piece(piece);
            commands.spawn((
                Name::new(format!("{piece} under the pointer")),
                held,
                looks(piece),
                Held {
                    board: kit.surface,
                    lift: (LIFT - kind.foot) * kit.scale,
                    scale: kit.scale,
                },
                Transform::from_scale(Vec3::splat(kit.scale)),
                Visibility::Hidden,
                ChildOf(root),
            ));
        }
    } else if let Some((source, id)) = turn.in_hand_choice().and_then(|g| kit.source_for(g, yours))
    {
        let id = id.as_str();
        let held = FromBowl {
            source: source.clone(),
            piece: id.to_string(),
            cell: None,
        };
        match take(&|f| f.cell.is_none() && f.source == source) {
            // Another kind of the same pool: the piece in hand becomes it.
            Some((entity, was)) => {
                if was.piece != id {
                    commands.entity(entity).insert((looks(id), held));
                }
            }
            None => {
                let at = over(&source);
                commands.spawn((
                    Name::new(format!("{id} in hand")),
                    held,
                    looks(id),
                    at,
                    Rest(at),
                    Carry::Held {
                        lift: LIFT * kit.scale,
                        scale: kit.scale,
                    },
                    ChildOf(root),
                ));
            }
        }
    }
    for (entity, _) in left {
        commands.entity(entity).despawn();
    }
}

/// Each seat's supply shows what it has left: the position's count, less what the move being
/// built takes from it (the piece in hand among them), each piece in its place in the line. A
/// click on this client's seat's own takes a piece of its pool.
fn show_supply(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    roots: Query<(Entity, &Kit), With<BoardRoot>>,
    shown: Query<(Entity, &InSupply)>,
) {
    let (Some(table), Ok((root, kit))) = (&watch.table, roots.single()) else {
        return;
    };
    if kit.supply.is_empty() {
        return;
    }
    let yours = table.yours().map_or("", |s| s.color.as_str());
    let turn = watch.turn.as_ref();
    let mut have: Vec<&InSupply> = Vec::new();
    for (colour, pools) in &kit.supply {
        for pool in pools {
            let mut count = pool.count(table, colour);
            if colour == yours {
                let source = format!("{colour}/{}", pool.id);
                count = count.saturating_sub(taken_from(turn, kit, yours, &source));
            }
            let count = count.min(pool.slots.len());
            for (entity, piece) in &shown {
                if piece.colour == *colour && piece.pool == pool.id {
                    if piece.index >= count {
                        commands.entity(entity).despawn();
                    } else {
                        have.push(piece);
                    }
                }
            }
            let kind = kit.piece(&pool.piece);
            let material = kind
                .material
                .clone()
                .or_else(|| kit.materials.get(colour).cloned())
                .unwrap_or_default();
            for index in 0..count {
                let there = InSupply {
                    colour: colour.clone(),
                    pool: pool.id.clone(),
                    index,
                };
                if have.contains(&&there) {
                    continue;
                }
                let mut piece = commands.spawn((
                    Name::new(format!("{} of {colour}'s supply", pool.piece)),
                    Mesh3d(kind.mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    pool.slots[index].with_scale(Vec3::splat(kit.scale)),
                    there,
                    ChildOf(root),
                ));
                if colour == yours {
                    piece.insert((
                        Pickable::default(),
                        Choice(pool.take.clone()),
                        SimplifiedMesh(kind.hit.clone()),
                    ));
                }
            }
        }
    }
}

/// What `show_tints` keeps of the stock: the pieces in it, and the one taken from it.
type StockParts<'w, 's> = (
    Query<'w, 's, Entity, With<InStock>>,
    Query<'w, 's, (Entity, &'static FromStock)>,
    Query<'w, 's, Entity, With<TileGhost>>,
    Res<'w, Hovering>,
);

/// A see-through piece of the stock's on the position the pointer is over, where the one in
/// hand would go.
#[derive(Component)]
struct TileGhost;

/// The positions the view tints, drawn as the game's look says (a tile laid there, or a stamp),
/// and the stock beside the board: what the view counts left of each kind, less what the move
/// being built places.
fn show_tints(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    roots: Query<(Entity, &Kit), With<BoardRoot>>,
    drawn: Query<(Entity, &TintAt)>,
    (stocked, in_hand, ghosts, hovering): StockParts,
) {
    let (Some(table), Ok((root, kit))) = (&watch.table, roots.single()) else {
        return;
    };
    if kit.tints.is_empty() && kit.stock.is_none() {
        return;
    }
    let layout = Layout::new(&table.info.board.positions);
    let tints = table.state.state.tints();
    // What the view stamps is what it points out, the preview's while a move is built (a line
    // the move makes, to be removed next); what it lays is the board's own (terrain),
    // which the move being built shows for itself.
    let preview = watch
        .turn
        .as_ref()
        .and_then(Turn::preview)
        .map(|p| p.tints());
    let laid = |key: &str| matches!(kit.tints.get(key), Some(TintKit::Laid { .. }));
    let stamped = |key: &str| matches!(kit.tints.get(key), Some(TintKit::Stamp { .. }));
    let wanted: Vec<TintAt> = tints::shown(&tints, laid)
        .chain(tints::shown(preview.as_ref().unwrap_or(&tints), stamped))
        .map(|(cell, key)| TintAt {
            cell: cell.to_string(),
            key: key.to_string(),
        })
        .collect();
    let mut have = Vec::new();
    for (entity, at) in &drawn {
        if wanted.contains(at) {
            have.push(at);
        } else {
            commands.entity(entity).despawn();
        }
    }
    for at in &wanted {
        let Some(centre) = layout.centres.get(&at.cell) else {
            continue;
        };
        if have.contains(&at) {
            continue;
        }
        let name = Name::new(format!("{} at {}", at.key, at.cell));
        // A click on it is a click on its position.
        let spot = (Spot(at.cell.clone()), Pickable::default());
        match &kit.tints[&at.key] {
            TintKit::Laid { piece, outline } => {
                let kind = kit.piece(piece);
                let rest = kind.material.clone().unwrap_or_default();
                commands.spawn((
                    name,
                    Mesh3d(kind.mesh.clone()),
                    MeshMaterial3d(rest),
                    kit.set_down(piece, "", *centre, (&at.cell, 0), 0.0),
                    Cell {
                        id: at.cell.clone(),
                    },
                    outline.clone(),
                    at.clone(),
                    spot,
                    ChildOf(root),
                ));
            }
            TintKit::Stamp { mesh, material } => {
                commands.spawn((
                    name,
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(*centre + Vec3::Y * (kit.surface + 0.00012)),
                    at.clone(),
                    spot,
                    ChildOf(root),
                ));
            }
        }
    }

    let Some(stock) = &kit.stock else { return };
    let counts = table.state.state.view_counts(&stock.counts);
    let turn = watch
        .turn
        .as_ref()
        .filter(|t| t.stage == Stage::Choosing || !t.made.is_empty());
    // What the move being built takes from the stock: the piece in hand (by the choice taken
    // up), and what it has placed, where.
    let choosing = turn.filter(|t| t.stage == Stage::Choosing);
    let takes: HashMap<&str, Vec<Gesture>> = stock
        .slots
        .iter()
        .map(|(kind, _)| {
            let takes = choosing
                .map(|t| tints::takes(&t.options, &t.gestures, kind))
                .unwrap_or_default();
            (kind.as_str(), takes)
        })
        .collect();
    let taken: Option<&str> = choosing.and_then(Turn::taken).and_then(|choices| {
        takes
            .iter()
            .find(|(_, t)| choices.iter().any(|c| t.contains(c)))
            .map(|(kind, _)| *kind)
    });
    let placed: Vec<(String, String)> = turn
        .map(|t| {
            t.placements()
                .filter_map(|(cell, kind)| {
                    let kind = kind.filter(|k| takes.contains_key(k))?;
                    Some((cell.to_string(), kind.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    // Where the piece in hand would go, see-through on the position under the pointer, if it
    // may go there.
    for entity in &ghosts {
        commands.entity(entity).despawn();
    }
    if let (Some(kind), Some(turn), Some(cell)) = (taken, choosing, hovering.0.as_deref())
        && turn.targets().contains(cell)
        && let Some(centre) = layout.centres.get(cell)
    {
        let piece = kit.piece(kind);
        commands.spawn((
            Name::new(format!("{kind} would go on {cell}")),
            TileGhost,
            Mesh3d(piece.mesh.clone()),
            MeshMaterial3d(
                piece
                    .ghost
                    .clone()
                    .or_else(|| piece.material.clone())
                    .unwrap_or_default(),
            ),
            kit.set_down(kind, "", *centre, (cell, 0), kit.floor(&tints, cell)),
            ChildOf(root),
        ));
    }
    let shown = |kind: &str| {
        let out =
            placed.iter().filter(|(_, k)| k == kind).count() + usize::from(taken == Some(kind));
        (counts.get(kind).copied().unwrap_or(0) as usize).saturating_sub(out)
    };
    // The stock as it stands, laid again: a piece of a kind the player may take now is taken
    // up by a click.
    for entity in &stocked {
        commands.entity(entity).despawn();
    }
    let rest_at = |kind: &str, index: usize, at: Vec2| {
        let mut put = kit.set_down(kind, "", Vec3::new(at.x, 0.0, at.y), (kind, index), 0.0);
        put.translation.y -= kit.surface;
        put
    };
    for (kind, slots) in &stock.slots {
        let count = shown(kind).min(slots.len());
        let piece = kit.piece(kind);
        let material = piece.material.clone().unwrap_or_default();
        for (index, at) in slots.iter().enumerate().take(count) {
            let mut spawned = commands.spawn((
                Name::new(format!("{kind} in the stock")),
                Mesh3d(piece.mesh.clone()),
                MeshMaterial3d(material.clone()),
                rest_at(kind, index, *at),
                InStock {
                    kind: kind.clone(),
                    index,
                },
                ChildOf(root),
            ));
            if !takes[kind.as_str()].is_empty() {
                spawned.insert((
                    Pickable::default(),
                    TakeToPlace(takes[kind.as_str()].clone()),
                ));
            }
        }
    }

    // The piece in hand: under the pointer, bobbing, until it is placed; then carried from
    // there (or, chosen on the panel, from its place in the stock) to where it goes.
    let put_at = |kind: &str, cell: &str| {
        let centre = layout.centres.get(cell)?;
        Some(kit.set_down(kind, "", *centre, (cell, 0), kit.floor(&tints, cell)))
    };
    let mut kept = false;
    for (entity, hand) in &in_hand {
        let goes = placed.iter().find(|(_, k)| *k == hand.kind);
        match (goes, &hand.cell) {
            (Some((cell, _)), Some(at)) if cell == at && !kept => kept = true,
            (Some((cell, kind)), None) if !kept => {
                let Some(to) = put_at(kind, cell) else {
                    commands.entity(entity).despawn();
                    continue;
                };
                commands.entity(entity).remove::<Held>().insert((
                    FromStock {
                        kind: kind.clone(),
                        cell: Some(cell.clone()),
                    },
                    Rest(to),
                    Carry::Put(to),
                    Visibility::Inherited,
                ));
                kept = true;
            }
            (None, None) if taken == Some(hand.kind.as_str()) && !kept => kept = true,
            _ => commands.entity(entity).despawn(),
        }
    }
    if kept {
        return;
    }
    if let Some((cell, kind)) = placed.first() {
        let index = shown(kind);
        let from = stock
            .slots
            .iter()
            .find(|(k, _)| k == kind)
            .and_then(|(_, slots)| slots.get(index))
            .map(|at| rest_at(kind, index, *at));
        if let (Some(from), Some(to)) = (from, put_at(kind, cell)) {
            let piece = kit.piece(kind);
            commands.spawn((
                Name::new(format!("{kind} put down")),
                FromStock {
                    kind: kind.clone(),
                    cell: Some(cell.clone()),
                },
                Mesh3d(piece.mesh.clone()),
                MeshMaterial3d(piece.material.clone().unwrap_or_default()),
                from,
                Rest(from),
                Carry::Put(to),
                ChildOf(root),
            ));
        }
    } else if let Some(kind) = taken {
        let piece = kit.piece(kind);
        commands.spawn((
            Name::new(format!("{kind} in hand")),
            FromStock {
                kind: kind.to_string(),
                cell: None,
            },
            Mesh3d(piece.mesh.clone()),
            MeshMaterial3d(piece.material.clone().unwrap_or_default()),
            Held {
                board: kit.surface,
                lift: (LIFT - piece.foot) * kit.scale,
                scale: kit.scale,
            },
            Transform::from_scale(Vec3::splat(kit.scale)),
            Visibility::Hidden,
            ChildOf(root),
        ));
    }
}

/// The walls the position has, each a fence on the side its two squares share (one that has
/// just appeared drops in, but one the player put there); and the move being built's: a fence
/// taken from the bowl waits over it, and once a side is chosen it is carried there and set
/// down. Where it may go is not lit: the see-through fence on the side aimed at shows it.
/// The fences `show_fences` keeps: the position's walls and the fence in hand.
type FenceParts<'w, 's> = (
    Query<'w, 's, (Entity, &'static WallAt)>,
    Query<'w, 's, (Entity, &'static HeldFence)>,
);

fn show_fences(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    roots: Query<(Entity, &Kit), With<BoardRoot>>,
    (walls, held): FenceParts,
    mut room: Local<String>,
) {
    let (Some(table), Ok((root, kit))) = (&watch.table, roots.single()) else {
        return;
    };
    let Some(fence) = kit.fences.as_ref() else {
        return;
    };
    let layout = Layout::new(&table.info.board.positions);
    let turn = watch.turn.as_ref();
    // A game just opened shows its walls as they stand, without dropping each in.
    let opened = *room != table.room_code;
    if opened {
        room.clone_from(&table.room_code);
    }

    // The move being built's fence: chosen (where it goes), or taken and waiting.
    let chosen: Option<(String, Transform)> = turn.and_then(|t| {
        t.made.iter().find_map(|s| {
            let cell = s.to.as_deref().or(s.from.as_deref())?;
            s.gestures
                .iter()
                .find_map(|g| kit.fence_on(&layout, cell, &g.value))
        })
    });
    let taken = turn.and_then(Turn::taken);
    let mut kept = false;
    for (entity, h) in &held {
        match (&chosen, &h.0) {
            (Some((key, _)), Some(k)) if k == key => kept = true,
            // Put down: from under the pointer, carried to the side chosen.
            (Some((key, to)), None) if !kept => {
                commands.entity(entity).remove::<Held>().insert((
                    HeldFence(Some(key.clone())),
                    Rest(*to),
                    Carry::Put(*to),
                    Visibility::Inherited,
                ));
                kept = true;
            }
            (None, None) if taken.is_some() && !kept => kept = true,
            _ => {
                commands.entity(entity).despawn();
            }
        }
    }
    let waiting = Transform::from_translation(fence.over + Vec3::Y * 0.012 * kit.scale)
        .with_scale(Vec3::splat(kit.scale));
    if !kept {
        let looks = (
            Mesh3d(fence.mesh.clone()),
            MeshMaterial3d(fence.material.clone()),
            ChildOf(root),
        );
        match (&chosen, &taken) {
            // Chosen with nothing in hand (the panel's button): from the bowl.
            (Some((key, to)), _) => {
                commands.spawn((
                    Name::new("Fence put down"),
                    HeldFence(Some(key.clone())),
                    looks,
                    waiting,
                    Rest(waiting),
                    Carry::Put(*to),
                ));
            }
            // Taken: under the pointer, in place of its arrow, as an aimed piece is.
            (None, Some(_)) => {
                commands.spawn((
                    Name::new("Fence in hand"),
                    HeldFence(None),
                    looks,
                    fence.held(kit),
                    Transform::from_scale(Vec3::splat(kit.scale)),
                    Visibility::Hidden,
                ));
            }
            _ => {}
        }
    }

    // The position's walls.
    let wanted: Vec<(String, Transform)> = table
        .state
        .state
        .walls()
        .iter()
        .filter_map(|(a, b)| Some((crate::fences::key(a, b), kit.fence_between(&layout, a, b)?)))
        .collect();
    let mut have: Vec<&str> = Vec::new();
    for (entity, wall) in &walls {
        if wanted.iter().any(|(k, _)| *k == wall.0) {
            have.push(&wall.0);
        } else {
            commands.entity(entity).despawn();
        }
    }
    for (key, at) in wanted {
        if have.contains(&key.as_str()) {
            continue;
        }
        // The player's own, just played, is where they put it already.
        let theirs = chosen.as_ref().is_some_and(|(k, _)| *k == key);
        let mut wall = commands.spawn((
            Name::new(format!("Fence {key}")),
            WallAt(key),
            Mesh3d(fence.mesh.clone()),
            MeshMaterial3d(fence.material.clone()),
            ChildOf(root),
        ));
        if theirs || opened {
            wall.insert((at, Rest(at)));
        } else {
            let mut high = at;
            high.translation.y += DROP_HEIGHT;
            wall.insert((
                high,
                Dropping {
                    timer: Timer::new(DROP_TIME, TimerMode::Once),
                    to: at.translation.y,
                },
            ));
        }
    }
}

/// The fence under the pointer, not the one aimed.
type InHandFence = (With<HeldFence>, With<Held>, Without<FenceGhost>);

/// How near a side the pointer must come for the fence taken to be aimed at it, against the
/// distance between neighbours.
const AIM_REACH: f32 = 0.7;

/// While a fence is taken up: which side it would go on (`play::SideAim`) — the one the
/// pointer is nearest, when it is near one, or the one the wheel turned to — shown by a
/// see-through fence there, and the fence under the pointer turned to lie as it would.
fn fence_aim(
    watch: Res<WatchStatus>,
    roots: Query<&Kit, With<BoardRoot>>,
    pointer: Res<PointerOnBoard>,
    mut aim: ResMut<SideAim>,
    mut ghosts: Query<(&mut Transform, &mut Visibility), With<FenceGhost>>,
    mut held: Query<&mut Transform, InHandFence>,
) {
    let places: Vec<(Gesture, Transform)> = (|| {
        let table = watch.table.as_ref()?;
        let turn = watch.turn.as_ref()?;
        let kit = roots.single().ok()?;
        let taken = turn.taken()?;
        let cell = turn.gestures.iter().rev().find(|g| g.is_pos())?;
        let layout = Layout::new(&table.info.board.positions);
        Some(
            taken
                .into_iter()
                .filter_map(|g| Some((g.clone(), kit.fence_on(&layout, &cell.value, &g.value)?.1)))
                .collect(),
        )
    })()
    .unwrap_or_default();
    if places.is_empty() {
        if *aim != SideAim::default() {
            *aim = SideAim::default();
        }
        for (_, mut vis) in &mut ghosts {
            vis.set_if_neq(Visibility::Hidden);
        }
        return;
    }
    let kit = roots.single().expect("a board");
    // The pointer near a side aims at it; away from them all, it leaves what the wheel chose.
    if pointer.is_changed()
        && let Some((hit, eye)) = pointer.0
    {
        let at = crate::hand::on_surface(hit, eye, kit.surface);
        let reach = AIM_REACH * PITCH * kit.scale;
        let nearest = places
            .iter()
            .map(|(g, t)| (g, t.translation.xz().distance(at.xz())))
            .filter(|(_, d)| *d < reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(g, _)| g.clone());
        match nearest {
            Some(g) => {
                let wanted = SideAim {
                    choice: Some(g),
                    by_wheel: false,
                };
                if *aim != wanted {
                    *aim = wanted;
                }
            }
            None if !aim.by_wheel && aim.choice.is_some() => *aim = SideAim::default(),
            None => {}
        }
    }
    let aimed = aim
        .choice
        .as_ref()
        .and_then(|c| places.iter().find(|(g, _)| g == c));
    if aimed.is_none() && aim.choice.is_some() {
        *aim = SideAim::default();
    }
    for (mut t, mut vis) in &mut ghosts {
        match aimed {
            Some((_, at)) => {
                if *t != *at {
                    *t = *at;
                }
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
    if let Some((_, at)) = aimed {
        for mut t in &mut held {
            if t.rotation != at.rotation {
                t.rotation = at.rotation;
            }
        }
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

/// Dresses each piece of two sides, wherever it was made and whatever it was given: a piece is
/// made, like any other, as its upper half in its colour word's material (or the see-through
/// one), and here that half is given the material of the side its colour has up, and a child the
/// lower half in the other's. Again whenever its material is changed (a piece the move takes
/// made see-through, a piece turned over given its new colour's).
#[allow(clippy::type_complexity)]
fn dress_sides(
    mut commands: Commands,
    kits: Query<&Kit, With<BoardRoot>>,
    pieces: Query<
        (
            Entity,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            Has<Pickable>,
            Option<&Sides>,
        ),
        Changed<MeshMaterial3d<StandardMaterial>>,
    >,
) {
    let Ok(kit) = kits.single() else {
        return;
    };
    let sided: Vec<&PieceKit> = kit.pieces.values().filter(|k| k.sides.is_some()).collect();
    if sided.is_empty() {
        return;
    }
    for (entity, mesh, material, pickable, dressed) in &pieces {
        let Some(sides) = sided
            .iter()
            .find(|k| k.mesh == mesh.0)
            .and_then(|k| k.sides.as_ref())
        else {
            continue;
        };
        // Given one of its sides' own: dressed already.
        let Some((colour, see_through)) = kit.dress.get(&material.0.id()) else {
            continue;
        };
        let turned = sides.turned.get(colour).copied().unwrap_or(false);
        let parts = if *see_through {
            &sides.ghost
        } else {
            &sides.solid
        };
        let (upper, lower) = crate::sides::halves(parts, turned);
        commands.entity(entity).insert(MeshMaterial3d(upper));
        match dressed {
            Some(dressed) => {
                commands
                    .entity(dressed.under)
                    .try_insert(MeshMaterial3d(lower));
            }
            None => {
                let mut under = commands.spawn((
                    Name::new("lower half"),
                    Mesh3d(sides.under.clone()),
                    MeshMaterial3d(lower),
                    Transform::default(),
                    ChildOf(entity),
                ));
                // Picked as the piece is: a click on it bubbles up to the piece.
                if pickable {
                    under.insert(Pickable::default());
                }
                let under = under.id();
                commands.entity(entity).insert(Sides { under });
            }
        }
    }
}

/// Dresses each piece with parts of their own, wherever it was made: a piece is made, like any
/// other, as its model's first part, and here it is given a child for each of its other parts in
/// its own material, see-through when the piece is. Again whenever the piece's material is
/// changed (made see-through, or solid again).
#[allow(clippy::type_complexity)]
fn dress_parts(
    mut commands: Commands,
    kits: Query<&Kit, With<BoardRoot>>,
    pieces: Query<
        (
            Entity,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            Has<Pickable>,
            Option<&Parts>,
        ),
        Changed<MeshMaterial3d<StandardMaterial>>,
    >,
) {
    let Ok(kit) = kits.single() else {
        return;
    };
    let with_parts: Vec<&PieceKit> = kit
        .pieces
        .values()
        .filter(|k| !k.parts.is_empty())
        .collect();
    if with_parts.is_empty() {
        return;
    }
    for (entity, mesh, material, pickable, dressed) in &pieces {
        let Some(kind) = with_parts.iter().find(|k| k.mesh == mesh.0) else {
            continue;
        };
        let see_through = kit
            .dress
            .get(&material.0.id())
            .is_some_and(|(_, see_through)| *see_through);
        let pick = |part: &PartKit| {
            MeshMaterial3d(if see_through {
                part.ghost.clone()
            } else {
                part.solid.clone()
            })
        };
        match dressed {
            Some(Parts(children)) => {
                for (child, part) in children.iter().zip(&kind.parts) {
                    commands.entity(*child).try_insert(pick(part));
                }
            }
            None => {
                let children = kind
                    .parts
                    .iter()
                    .map(|part| {
                        let mut child = commands.spawn((
                            Name::new("part"),
                            Mesh3d(part.mesh.clone()),
                            pick(part),
                            Transform::default(),
                            ChildOf(entity),
                        ));
                        // Picked as the piece is: a click on it bubbles up to the piece.
                        if pickable {
                            child.insert(Pickable::default());
                        }
                        child.id()
                    })
                    .collect();
                commands.entity(entity).insert(Parts(children));
            }
        }
    }
}

/// Turns each piece turning over (`Flipping`), and lays it where it lies once it is done, with
/// the soft sound of a piece set down.
fn flip_pieces(
    mut commands: Commands,
    time: Res<Time>,
    mut pieces: Query<(Entity, &mut Flipping, &mut Transform)>,
    mut cues: MessageWriter<Cue>,
) {
    for (entity, mut flip, mut t) in &mut pieces {
        flip.secs += time.delta_secs();
        let (at, done) =
            crate::sides::flip_pose(flip.rest, flip.axis, flip.lift, flip.delay, flip.secs);
        *t = at;
        if done {
            cues.write(Cue::Capture);
            commands
                .entity(entity)
                .remove::<Flipping>()
                .insert(Rest(flip.rest));
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
            commands
                .entity(entity)
                .remove::<Dropping>()
                .insert(Rest(*t));
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
    #[test]
    fn a_game_with_no_board_is_framed_by_the_cells_round_its_middle() {
        let pitch = 0.02;
        let centres: Vec<Vec2> = (-8..=8).map(|i| Vec2::new(i as f32 * pitch, 0.0)).collect();
        let corners = slab::cell_corners(CellShape::Square, pitch);
        let frame = table_frame(&centres, &corners, pitch);
        let reach = frame.iter().fold(0.0f32, |r, p| r.max(p.x.abs()));
        assert!(
            (reach - (TABLE_FRAME + 0.5) * pitch).abs() < 1e-5,
            "{reach}"
        );
        assert!(matches!(
            Surface::of(
                &Board {
                    kind: "hex".into(),
                    ..Default::default()
                },
                Some(&Appearance::parse("games = [\"t\"]\n[board]\non = \"table\"").unwrap())
            ),
            Surface::Table(CellShape::Hexagon)
        ));
        assert_eq!(Surface::Table(CellShape::Hexagon).height(), 0.0);
    }

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
    fn a_piece_is_set_down_as_its_kind_is() {
        let up = |turn: Quat| (turn * Vec3::Y).distance(Vec3::Y);
        // A stone, as before: turned any way, leaning a little, never quite one size, in the
        // middle of its position.
        let stone = stance(Stands::Loose, "c4", 0, 0.0);
        assert_eq!(
            (stone.turn, stone.size),
            (lie("c4", 0), super::size("c4", 0))
        );
        assert!(up(stone.turn) > 0.0 && up(stone.turn) < 0.03);
        assert_eq!(stone.aside, Vec2::ZERO);
        // The others stand upright, the model's own size.
        for stands in [Stands::Upright, Stands::Square, Stands::Forward] {
            for cell in ["a1", "c4", "h8"] {
                let set = stance(stands, cell, 1, 2.0);
                assert!(up(set.turn) < 1e-6, "{stands:?} at {cell}");
                assert_eq!(set.size, 1.0);
                assert_eq!(stance(stands, cell, 1, 2.0), set, "the same again");
            }
        }
        let yaw = |turn: Quat| turn.to_euler(EulerRot::YXZ).0;
        let cells = ["a1", "b2", "c3", "d4", "e5", "f6", "g7", "h8"];
        // A block is square to the board, give or take; not every one the same quarter turn.
        let mut quarters = Vec::new();
        for cell in cells {
            let turn = yaw(stance(Stands::Square, cell, 0, 0.0).turn);
            let quarter = (turn / FRAC_PI_2).round();
            assert!((turn - quarter * FRAC_PI_2).abs() <= SQUARE_OFF + 1e-5);
            quarters.push(quarter.rem_euclid(4.0) as i32);
        }
        quarters.dedup();
        assert!(quarters.len() > 1, "{quarters:?}");
        // A piece with a face looks the way its seat does, give or take; a turned one any way.
        // Both stand in the middle of their positions.
        for cell in cells {
            for facing in [0.0, FRAC_PI_2, 3.0] {
                let set = stance(Stands::Forward, cell, 0, facing);
                let off = (set.turn * Quat::from_rotation_y(-facing))
                    .to_axis_angle()
                    .1;
                assert!(off <= FORWARD_OFF + 1e-5, "{off}");
                assert_eq!(set.aside, Vec2::ZERO);
            }
            assert_eq!(stance(Stands::Upright, cell, 0, 0.0).aside, Vec2::ZERO);
        }
        let a = yaw(stance(Stands::Upright, "a1", 0, 0.0).turn);
        let b = yaw(stance(Stands::Upright, "h8", 0, 0.0).turn);
        assert!((a - b).abs() > 0.01);
    }

    #[test]
    fn the_blocks_of_a_stack_do_not_line_up() {
        // Ten blocks on one position, as a hand sets them down: each a little aside of the
        // middle and a little askew, no two alike, none far enough to leave its space.
        let stack: Vec<Stance> = (0..10)
            .map(|layer| stance(Stands::Square, "c3", layer, 0.0))
            .collect();
        let askew = |s: &Stance| {
            let turn = s.turn.to_euler(EulerRot::YXZ).0;
            turn - (turn / FRAC_PI_2).round() * FRAC_PI_2
        };
        for (i, a) in stack.iter().enumerate() {
            assert!(a.aside.abs().max_element() <= SQUARE_ASIDE, "{}", a.aside);
            for b in &stack[i + 1..] {
                assert!(a.aside.distance(b.aside) > 1e-5, "two in one place");
                assert!((askew(a) - askew(b)).abs() > 1e-4, "two turned alike");
            }
        }
        // Plainly so, not by a hair: among ten, some lie more than half a millimetre apart
        // and some are turned more than two degrees apart.
        let spread = |f: &dyn Fn(&Stance) -> f32| {
            let all: Vec<f32> = stack.iter().map(f).collect();
            all.iter().cloned().fold(f32::MIN, f32::max)
                - all.iter().cloned().fold(f32::MAX, f32::min)
        };
        assert!(spread(&|s| s.aside.x) > 0.0005 && spread(&|s| s.aside.y) > 0.0005);
        assert!(spread(&askew) > 2f32.to_radians());
        // A block 18.5 mm square, as far aside and askew as it may be, is inside its cell.
        let half = 0.0185 / 2.0 * (SQUARE_OFF.cos() + SQUARE_OFF.sin()) + SQUARE_ASIDE;
        assert!(half < PITCH * CELL_SCALE / 2.0, "{half}");
        // The same block in the same place lies the same way, whenever it is drawn.
        assert_eq!(stance(Stands::Square, "c3", 4, 0.0), stack[4]);
        assert_ne!(stance(Stands::Square, "d3", 4, 0.0), stack[4]);
    }

    #[test]
    fn each_seat_looks_across_the_board_from_its_place_at_the_table() {
        let mut info = hex3_with_goals();
        let seat = |seat: u32, word: &str| crate::net::api::SeatDef {
            seat,
            color: word.into(),
            label: word.into(),
            colors: vec![],
        };
        info.seats = vec![seat(1, "white"), seat(2, "black")];
        let two = facings(&info);
        assert_eq!(two["white"], 0.0, "the first, from the player's side");
        assert!((two["black"] - TAU / 2.0).abs() < 1e-6, "the second, back");
        info.seats.extend([seat(3, "red"), seat(4, "blue")]);
        assert!((facings(&info)["black"] - TAU / 4.0).abs() < 1e-6, "four");
        info.seats.clear();
        assert!(facings(&info).is_empty());
    }

    #[test]
    fn a_chessboards_squares_go_by_turns_from_the_near_left_corner() {
        // A layout's y grows towards the player: a1 is at the bottom left.
        let squares = by_turns(&[
            pos("a2", 0.0, 0.0),
            pos("b2", 1.0, 0.0),
            pos("a1", 0.0, 1.0),
            pos("b1", 1.0, 1.0),
        ]);
        assert_eq!(squares["a1"], 0);
        assert_eq!(squares["b2"], 0);
        assert_eq!(squares["a2"], 1);
        assert_eq!(squares["b1"], 1);
        assert!(by_turns(&[]).is_empty());
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
                    ..Default::default()
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
                        colors: vec![],
                    }],
                    board: Board {
                        kind: "hex".into(),
                        positions: hex3(),
                        regions: Default::default(),
                    },
                    actions: vec![],
                    goals: vec![],
                    pieces: vec![],
                    counters: vec![],
                    opening: None,
                    reserves: Default::default(),
                    cards: None,
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

        /// A game on a 3×3 grid for two seats, white then black, by a game's id, with these
        /// stacks: each piece as (kind, colour word), bottom first.
        fn grid_table(id: &str, cells: &[(&str, &[(&str, &str)])]) -> Table {
            let mut t = table();
            t.info.id = id.into();
            t.info.board.kind = "grid".into();
            t.info.board.positions = (0..9)
                .map(|i| {
                    let id = format!("{}{}", (b'a' + i % 3) as char, 3 - i / 3);
                    pos(&id, (i % 3) as f32, (i / 3) as f32)
                })
                .collect();
            t.info.seats = [(1, "white"), (2, "black")]
                .map(|(seat, word)| SeatDef {
                    seat,
                    color: word.into(),
                    label: word.into(),
                    colors: vec![],
                })
                .to_vec();
            t.state = stacks(1, cells);
            t
        }

        fn stacks(seq: u64, cells: &[(&str, &[(&str, &str)])]) -> GameState {
            let stack = |pieces: &[(&str, &str)]| {
                pieces
                    .iter()
                    .map(|(piece, color)| PieceOnBoard {
                        piece: piece.to_string(),
                        color: color.to_string(),
                    })
                    .collect()
            };
            GameState {
                state: Position {
                    cells: cells
                        .iter()
                        .map(|(cell, pieces)| (cell.to_string(), stack(pieces)))
                        .collect(),
                    ..Default::default()
                },
                status: "active".into(),
                move_seq: seq,
                ..Default::default()
            }
        }

        /// Each piece shown, by its cell and layer: its kind, where it is, and its mesh and
        /// material.
        type Drawn = (
            String,
            Transform,
            Handle<Mesh>,
            Handle<StandardMaterial>,
            Entity,
        );

        fn drawn(app: &mut App) -> HashMap<(String, usize), Drawn> {
            let mut q = app.world_mut().query::<(
                Entity,
                &PieceAt,
                &Transform,
                &Mesh3d,
                &MeshMaterial3d<StandardMaterial>,
            )>();
            q.iter(app.world())
                .map(|(entity, at, transform, mesh, material)| {
                    (
                        (at.cell.clone(), at.layer),
                        (
                            at.piece.piece.clone(),
                            *transform,
                            mesh.0.clone(),
                            material.0.clone(),
                            entity,
                        ),
                    )
                })
                .collect()
        }

        fn with_looks() -> App {
            let mut app = app();
            app.insert_resource(Appearances::built_in());
            app
        }

        #[test]
        fn each_kind_of_piece_is_its_own_model_and_a_stack_stands_on_what_is_under_it() {
            let mut app = with_looks();
            let flat = assets::model_span("models/tak/flat.glb").unwrap();
            let wall = assets::model_span("models/tak/wall.glb").unwrap();
            // Tak's capstone is a pawn.
            let cap = assets::model_span("models/chess/pawn.glb").unwrap();
            let (flat, wall, cap) = (flat.1 - flat.0, wall.1 - wall.0, cap.1 - cap.0);
            let cells: &[(&str, &[(&str, &str)])] = &[
                (
                    "a1",
                    &[("flat", "white"), ("flat", "black"), ("wall", "white")],
                ),
                ("b2", &[("flat", "black"), ("cap", "black")]),
            ];
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(grid_table("tak-5x5", cells));
            app.update();
            let shown = drawn(&mut app);
            assert_eq!(shown.len(), 5);
            let at = |cell: &str, layer: usize| shown[&(cell.to_string(), layer)].clone();
            let middle = |cell: &str, layer: usize| at(cell, layer).1.translation.y - SURFACE;
            // Each stands on its foot on what is under it: its middle half its height above.
            assert!((middle("a1", 0) - flat / 2.0).abs() < 1e-6);
            assert!((middle("a1", 1) - flat * 1.5).abs() < 1e-6);
            assert!((middle("a1", 2) - (2.0 * flat + wall / 2.0)).abs() < 1e-6);
            assert!((middle("b2", 1) - (flat + cap / 2.0)).abs() < 1e-6);
            // A flat stone, a wall and a capstone are three models; two flat stones are one.
            assert_eq!(at("a1", 0).2, at("a1", 1).2);
            assert_ne!(at("a1", 0).2, at("a1", 2).2);
            assert_ne!(at("b2", 0).2, at("b2", 1).2);
            assert_ne!(at("a1", 2).2, at("b2", 1).2);
            // Each seat's are its own wood.
            assert_eq!(at("a1", 0).3, at("a1", 2).3);
            assert_ne!(at("a1", 0).3, at("a1", 1).3);
            // Blocks stand upright, whatever way they are turned.
            for (_, transform, ..) in shown.values() {
                assert!((transform.rotation * Vec3::Y).distance(Vec3::Y) < 1e-5);
                assert_eq!(transform.scale, Vec3::ONE, "a block is its model's size");
            }
            // The blocks of a stack do not line up: each is a little aside of its position's
            // middle, its own way. The capstone, a turned piece, stands in the middle.
            let layout = Layout::new(&grid_table("tak-5x5", cells).info.board.positions);
            let aside = |cell: &str, layer: usize| {
                let off = at(cell, layer).1.translation - layout.centres[cell];
                Vec2::new(off.x, off.z)
            };
            for layer in 0..3 {
                let off = aside("a1", layer);
                assert!(off.length() > 1e-6 && off.abs().max_element() <= SQUARE_ASIDE);
            }
            assert!(aside("a1", 0).distance(aside("a1", 1)) > 1e-5);
            assert!(aside("a1", 1).distance(aside("a1", 2)) > 1e-5);
            assert_eq!(aside("b2", 1), Vec2::ZERO);

            // What is under a piece changes height: the piece is put down again, on it.
            let before = at("b2", 1).4;
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.table.as_mut().unwrap().apply(&Event::Move {
                seq: 2,
                state: stacks(2, &[("b2", &[("wall", "black"), ("cap", "black")])]),
            });
            app.update();
            let shown = drawn(&mut app);
            assert_eq!(shown.len(), 2);
            let capstone = &shown[&("b2".to_string(), 1)];
            assert_ne!(capstone.4, before, "put down again");
            assert!(capstone.1.translation.y > SURFACE + wall + cap / 2.0);
        }

        #[test]
        fn a_piece_with_no_look_is_a_stone_and_one_that_is_nobodys_has_a_colour() {
            let mut app = with_looks();
            let cells: &[(&str, &[(&str, &str)])] = &[
                ("a1", &[("king", "white")]),
                ("b2", &[("pawn", "green")]),
                ("c3", &[("oddity", "mauve")]),
                ("a3", &[("king", "black")]),
            ];
            app.world_mut().resource_mut::<WatchStatus>().table = Some(grid_table("fist", cells));
            app.update();
            let shown = drawn(&mut app);
            let at = |cell: &str| shown[&(cell.to_string(), 0)].clone();
            let nothing = Handle::<StandardMaterial>::default();
            // The pawn that is neither seat's has the look its game gives its colour word; a
            // piece the look does not know, in a colour no seat plays, is a stone of some colour.
            for cell in ["a1", "b2", "c3", "a3"] {
                assert_ne!(at(cell).3, nothing, "{cell}");
            }
            assert_ne!(at("b2").3, at("a1").3);
            assert_ne!(at("c3").3, at("b2").3);
            assert_ne!(at("c3").2, at("a1").2, "not a king's model");
            let stone = (SURFACE + STONE_THICKNESS / 2.0) - at("c3").1.translation.y;
            assert!(stone.abs() < 2e-4, "a stone lies on the board: {stone}");
            // A king looks across the board from its seat's side: the first seat's to the far
            // side, the second's back at it.
            let looks = |cell: &str| at(cell).1.rotation * Vec3::NEG_Z;
            assert!(looks("a1").z < -0.99, "{}", looks("a1"));
            assert!(looks("a3").z > 0.99, "{}", looks("a3"));
        }

        #[test]
        fn the_move_being_built_is_shown_as_the_piece_it_would_put_there() {
            use crate::net::api::{Gesture, LegalSelection, Selections};
            use crate::play::Turn;

            let mut app = with_looks();
            let cells: &[(&str, &[(&str, &str)])] = &[("b2", &[("flat", "white")])];
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(grid_table("tak-5x5", cells));
            app.update();
            let place = |piece: &str, notation: &str| LegalSelection {
                notation: notation.into(),
                action: "place".into(),
                to: Some("c3".into()),
                from: None,
                piece: Some(piece.into()),
                aim: None,
                gestures: vec![
                    Gesture {
                        kind: "piece".into(),
                        value: piece.into(),
                    },
                    Gesture::pos("c3"),
                ],
                captures: Vec::new(),
            };
            let mut turn = Turn::new(1);
            turn.answered(Selections {
                seat: 1,
                ply: 3,
                made: vec![],
                options: vec![place("flat", "c3"), place("wall", "Sc3")],
                complete: false,
                may_end: false,
                state: None,
            });
            turn.gesture(&Gesture {
                kind: "piece".into(),
                value: "wall".into(),
            });
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
            // Under the pointer: a wall, standing on the board.
            app.world_mut().resource_mut::<Hovering>().0 = Some("c3".into());
            app.update();
            let wall = assets::model_span("models/tak/wall.glb").unwrap();
            let mut ghosts = app
                .world_mut()
                .query_filtered::<(&Mesh3d, &Transform), With<Ghost>>();
            let shown: Vec<(Handle<Mesh>, Transform)> = ghosts
                .iter(app.world())
                .map(|(mesh, transform)| (mesh.0.clone(), *transform))
                .collect();
            assert_eq!(shown.len(), 1);
            let (mesh, transform) = shown[0].clone();
            assert!((transform.translation.y - (SURFACE - wall.0)).abs() < 1e-6);
            let stood = drawn(&mut app);
            assert_ne!(
                mesh,
                stood[&("b2".to_string(), 0)].2,
                "not a flat stone's model"
            );

            // Over a position a click would put nothing on, nothing is shown.
            app.world_mut().resource_mut::<Hovering>().0 = Some("b2".into());
            app.update();
            let mut ghosts = app.world_mut().query_filtered::<(), With<Ghost>>();
            assert_eq!(ghosts.iter(app.world()).count(), 0);
        }

        #[test]
        fn a_region_the_look_gives_a_material_has_its_cells_in_it() {
            // Fist's keep, in the middle of the board, in a chessboard's dark squares.
            let mut app = with_looks();
            let mut table = grid_table("fist", &[]);
            table
                .info
                .board
                .regions
                .insert("keep".into(), vec!["b2".into()]);
            app.world_mut().resource_mut::<WatchStatus>().table = Some(table);
            app.update();
            let mut q = app
                .world_mut()
                .query::<(&Cell, &MeshMaterial3d<StandardMaterial>)>();
            let materials: HashMap<String, Handle<StandardMaterial>> = q
                .iter(app.world())
                .map(|(c, m)| (c.id.clone(), m.0.clone()))
                .collect();
            assert_eq!(materials.len(), 9);
            assert_ne!(materials["b2"], materials["a1"], "the keep stands out");
            assert_eq!(materials["a1"], materials["c3"], "the rest are the board's");
            let look = app
                .world()
                .resource::<Appearances>()
                .for_game("fist")
                .cloned();
            assert_eq!(
                look.unwrap().board.regions.get("keep").map(String::as_str),
                Some("dark_square")
            );
        }

        /// A made-up game played with cards that name the move: a piece of its own model, and
        /// a region of the board inlaid in the wood that stands out against the board's.
        fn with_card_moves_look() -> App {
            let mut app = app();
            let look = Appearance::parse(
                r#"
games = ["t-cards"]

[pieces.chief]
model = "models/chess/king.glb"
stands = "forward"

[board.regions]
band = "inlay"

[materials.inlay]
wood = "contrast"
"#,
            )
            .unwrap();
            assert!(look.problems().is_empty(), "{:?}", look.problems());
            app.insert_resource(Appearances(vec![look]));
            let mut table = grid_table("t-cards", &[("a1", &[("chief", "white")])]);
            table
                .info
                .board
                .regions
                .insert("band".into(), vec!["b2".into()]);
            app.world_mut().resource_mut::<WatchStatus>().table = Some(table);
            app.update();
            app
        }

        #[test]
        fn a_region_inlaid_in_a_contrasting_wood_follows_the_boards_wood() {
            let mut app = with_card_moves_look();
            let texture = |app: &mut App, cell: &str| {
                let mut q = app
                    .world_mut()
                    .query::<(&Cell, &MeshMaterial3d<StandardMaterial>)>();
                let handle = q
                    .iter(app.world())
                    .find(|(c, _)| c.id == cell)
                    .map(|(_, m)| m.0.clone())
                    .expect("the cell");
                let materials = app.world().resource::<Assets<StandardMaterial>>();
                let texture = materials.get(&handle).unwrap().base_color_texture.clone();
                texture.and_then(|t| t.path().map(|p| p.to_string()))
            };
            let wood_of = |app: &mut App, cell: &str| {
                let path = texture(app, cell).expect("a wood's texture");
                look::WOODS
                    .iter()
                    .map(|w| w.name)
                    .find(|w| path.contains(&format!("/{w}_")))
                    .unwrap_or_else(|| panic!("{path}"))
            };
            // On the teak board the inlay is walnut; on a walnut board, teak.
            assert_eq!(wood_of(&mut app, "a1"), "teak");
            assert_eq!(wood_of(&mut app, "b2"), "walnut");
            app.insert_resource(BoardWood("walnut"));
            app.update();
            assert_eq!(wood_of(&mut app, "a1"), "walnut");
            assert_eq!(wood_of(&mut app, "b2"), "teak");
        }

        #[test]
        fn a_piece_moved_by_a_card_is_shown_as_itself_where_it_would_go() {
            use crate::net::api::{LegalSelection, Selections};
            use crate::play::Turn;

            // The selection's piece is the card that makes the move (as a game of move cards
            // names it), not a kind of piece: what arrives is the piece that stands where the
            // move starts.
            let mut app = with_card_moves_look();
            let step = |to: &str| LegalSelection {
                notation: format!("q a1-{to}"),
                action: "move".into(),
                to: Some(to.into()),
                from: Some("a1".into()),
                piece: Some("q".into()),
                aim: None,
                gestures: vec![
                    Gesture {
                        kind: "action".into(),
                        value: "card-q".into(),
                    },
                    Gesture::pos("a1"),
                    Gesture::pos(to),
                ],
                captures: Vec::new(),
            };
            let mut turn = Turn::new(1);
            turn.answered(Selections {
                seat: 1,
                ply: 1,
                made: vec![],
                options: vec![step("a2"), step("b1")],
                complete: false,
                may_end: false,
                state: None,
            });
            turn.gesture(&Gesture {
                kind: "action".into(),
                value: "card-q".into(),
            });
            turn.gesture(&Gesture::pos("a1"));
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
            app.world_mut().resource_mut::<Hovering>().0 = Some("a2".into());
            app.update();
            let chief = drawn(&mut app)[&("a1".to_string(), 0)].2.clone();
            let mut ghosts = app.world_mut().query_filtered::<&Mesh3d, With<Ghost>>();
            let shown: Vec<Handle<Mesh>> = ghosts.iter(app.world()).map(|m| m.0.clone()).collect();
            assert_eq!(shown, [chief], "the piece itself, not a stone");
        }

        #[test]
        fn what_lies_beside_the_board_is_kept_clear_of_its_margin() {
            let app = with_card_moves_look();
            let edge = &app.world().resource::<BoardEdge>().0;
            let outline = &app.world().resource::<BoardOutline>().0;
            assert!(!edge.is_empty() && !outline.is_empty());
            let reach = |points: &[Vec2], dir: Vec2| {
                points.iter().fold(f32::MIN, |far, p| far.max(p.dot(dir)))
            };
            let pitch = Layout::new(&grid_table("t-cards", &[]).info.board.positions).pitch;
            for dir in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
                let margin = reach(outline, dir) - reach(edge, dir);
                assert!(
                    (margin - pitch * MARGIN).abs() < 1e-4,
                    "the slab's edge is its margin beyond the cells: {margin}"
                );
            }
        }

        #[test]
        fn a_target_aimed_at_with_no_piece_in_the_look_has_nothing_put_on_it() {
            use crate::net::api::{Gesture, LegalSelection, Selections};
            use crate::play::Turn;

            // Fist's pin: a guard steps, then captures an enemy beside it where it stands. The
            // selection aims at the enemy, and nothing is put there: no stone, held or seen.
            let mut app = with_looks();
            let cells: &[(&str, &[(&str, &str)])] = &[
                ("a1", &[("yellow", "white")]),
                ("b3", &[("yellow", "black")]),
            ];
            app.world_mut().resource_mut::<WatchStatus>().table = Some(grid_table("fist", cells));
            app.update();
            let step = |aim: Option<&str>| {
                let mut gestures = vec![Gesture::pos("a1"), Gesture::pos("a2")];
                gestures.extend(aim.map(Gesture::pos));
                LegalSelection {
                    notation: format!("a1-a2{}", aim.map(|a| format!("x{a}")).unwrap_or_default()),
                    action: "move".into(),
                    to: Some("a2".into()),
                    from: Some("a1".into()),
                    piece: Some("yellow".into()),
                    aim: aim.map(Into::into),
                    gestures,
                    // As the server says it (API 1.10): a pin takes the piece it aims at.
                    captures: aim.map(|a| vec![a.into()]).unwrap_or_default(),
                }
            };
            let answer = |options: Vec<LegalSelection>| Selections {
                seat: 1,
                ply: 3,
                made: vec![],
                options,
                complete: false,
                may_end: false,
                state: None,
            };
            let counts = |app: &mut App| {
                let mut ghosts = app.world_mut().query_filtered::<(), With<Ghost>>();
                let mut held = app.world_mut().query_filtered::<(), With<Held>>();
                (
                    ghosts.iter(app.world()).count(),
                    held.iter(app.world()).count(),
                )
            };
            let click = |app: &mut App, at: &str| {
                let mut watch = app.world_mut().resource_mut::<WatchStatus>();
                watch.turn.as_mut().unwrap().gesture(&Gesture::pos(at));
                app.update();
            };

            // Stepped, it may stop there or pin: over the enemy, nothing is shown on it.
            let mut turn = Turn::new(1);
            turn.answered(answer(vec![step(None), step(Some("b3"))]));
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
            app.update();
            click(&mut app, "a1");
            click(&mut app, "a2");
            // The piece it may pin is lit red: a click there takes it.
            let mut q = app
                .world_mut()
                .query_filtered::<(&Cell, Option<&Tone>), With<Lit>>();
            let lit: Vec<(String, Option<Tone>)> = q
                .iter(app.world())
                .map(|(c, t)| (c.id.clone(), t.copied()))
                .collect();
            assert_eq!(lit, [("b3".to_string(), Some(Tone::Take))]);
            app.world_mut().resource_mut::<Hovering>().0 = Some("b3".into());
            app.update();
            assert_eq!(counts(&mut app), (0, 0), "nothing over the piece pinned");
            // Pinned: the move made shows nothing on it either.
            let enemy = |app: &mut App| {
                let mut q =
                    app.world_mut()
                        .query::<(&PieceAt, &MeshMaterial3d<StandardMaterial>, Has<Taken>)>();
                q.iter(app.world())
                    .find(|(at, ..)| at.cell == "b3")
                    .map(|(_, m, taken)| (m.0.clone(), taken))
                    .unwrap()
            };
            let (solid, taken) = enemy(&mut app);
            assert!(!taken, "not taken before the pin is chosen");
            click(&mut app, "b3");
            assert_eq!(counts(&mut app), (0, 0), "nothing on it once chosen");
            // The piece pinned is see-through until the move is played: what it takes.
            let ghost = {
                let mut kits = app.world_mut().query::<&Kit>();
                let kit = kits.single(app.world()).unwrap();
                let kind = kit.piece("yellow");
                kind.ghost
                    .clone()
                    .or_else(|| kit.ghosts.get("black").cloned())
                    .unwrap_or_else(|| kit.ghost.clone())
            };
            assert_eq!(enemy(&mut app), (ghost, true));
            // Taken back: solid again.
            {
                let mut watch = app.world_mut().resource_mut::<WatchStatus>();
                assert!(watch.turn.as_mut().unwrap().back());
            }
            app.update();
            assert_eq!(enemy(&mut app), (solid, false));

            // Where every move goes on to pin, nothing is held under the pointer for it.
            let mut turn = Turn::new(1);
            turn.answered(answer(vec![step(Some("b3"))]));
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
            app.world_mut().resource_mut::<Hovering>().0 = None;
            app.update();
            click(&mut app, "a1");
            click(&mut app, "a2");
            assert_eq!(counts(&mut app), (0, 0), "nothing held to aim");
        }

        #[test]
        fn a_piece_picked_up_is_held_then_put_where_it_goes_and_its_arrow_held() {
            use crate::net::api::{Gesture, LegalSelection, Selections};
            use crate::play::Turn;

            let mut app = with_looks();
            app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(16),
            ));
            let cells: &[(&str, &[(&str, &str)])] = &[("a1", &[("queen", "white")])];
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(grid_table("amazons", cells));
            app.update();
            let shoot = |to: &str, aim: &str| LegalSelection {
                notation: format!("a1-{to}/{aim}"),
                action: "move".into(),
                to: Some(to.into()),
                from: Some("a1".into()),
                piece: None,
                aim: Some(aim.into()),
                gestures: vec![Gesture::pos("a1"), Gesture::pos(to), Gesture::pos(aim)],
                captures: Vec::new(),
            };
            let mut turn = Turn::new(1);
            turn.answered(Selections {
                seat: 1,
                ply: 3,
                made: vec![],
                options: vec![shoot("a2", "a3"), shoot("a2", "a1"), shoot("b2", "c3")],
                complete: false,
                may_end: false,
                state: None,
            });
            let click = |app: &mut App, at: &str| {
                let mut watch = app.world_mut().resource_mut::<WatchStatus>();
                watch.turn.as_mut().unwrap().gesture(&Gesture::pos(at));
                app.update();
            };
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
            app.update();
            let mut kits = app.world_mut().query::<&Kit>();
            let kit = kits.single(app.world()).unwrap();
            let arrow = kit.pieces["arrow"].clone();
            let queen = |app: &mut App| {
                let mut q = app.world_mut().query::<(&PieceAt, Option<&Carry>, &Spot)>();
                let (_, carry, spot) = q.single(app.world()).unwrap();
                (carry.copied(), spot.0.clone())
            };
            let lit = |app: &mut App| {
                let mut q = app.world_mut().query_filtered::<&Cell, With<Lit>>();
                let mut out: Vec<String> = q.iter(app.world()).map(|c| c.id.clone()).collect();
                out.sort();
                out
            };
            assert_eq!(queen(&mut app), (None, "a1".into()));
            assert!(
                lit(&mut app).is_empty(),
                "nothing is lit before a piece is picked up"
            );

            // Picked up: held over its space, and where it may go lit.
            click(&mut app, "a1");
            assert!(matches!(queen(&mut app).0, Some(Carry::Held { .. })));
            assert_eq!(lit(&mut app), ["a2", "b2"]);

            // Put down: standing where it goes, a click on it a click there, and what it may
            // shoot at lit, the space it left among them.
            click(&mut app, "a2");
            let layout = Layout::new(&grid_table("amazons", cells).info.board.positions);
            let (carry, spot) = queen(&mut app);
            let Some(Carry::Put(put)) = carry else {
                panic!("not put down: {carry:?}");
            };
            assert_eq!(put.translation.xz(), layout.centres["a2"].xz());
            assert_eq!(spot, "a2");
            // Carried there once: a change elsewhere (the pointer moving) does not set it off
            // again.
            let mut q = app.world_mut().query::<(&PieceAt, &crate::hand::Travel)>();
            assert_eq!(q.iter(app.world()).count(), 1, "on its way");
            for _ in 0..30 {
                app.update();
            }
            app.world_mut().resource_mut::<Hovering>().0 = Some("a3".into());
            app.update();
            let mut q = app
                .world_mut()
                .query::<(&PieceAt, &crate::hand::Travel, &Transform)>();
            let (_, trip, t) = q.single(app.world()).unwrap();
            assert!(trip.secs > crate::hand::ACROSS + crate::hand::LOWER);
            assert_eq!(*t, put, "and there it stays");
            app.world_mut().resource_mut::<Hovering>().0 = None;
            app.update();
            assert_eq!(lit(&mut app), ["a1", "a3"]);
            // Where an arrow lands is lit plainly: it takes nothing.
            let mut tones = app.world_mut().query_filtered::<&Tone, With<Lit>>();
            assert!(tones.iter(app.world()).all(|t| *t == Tone::Plain));

            // The arrow is held for the pointer, in its own red; over a space it may land on,
            // a see-through one lies there, on the board the queen left.
            let mut held = app
                .world_mut()
                .query_filtered::<(&Mesh3d, &MeshMaterial3d<StandardMaterial>), With<Held>>();
            let (mesh, material) = held.single(app.world()).unwrap();
            assert_eq!(mesh.0, arrow.mesh);
            assert_eq!(Some(&material.0), arrow.material.as_ref());
            app.world_mut().resource_mut::<Hovering>().0 = Some("a1".into());
            app.update();
            let mut ghosts = app.world_mut().query_filtered::<(
                &Mesh3d,
                &MeshMaterial3d<StandardMaterial>,
                &Transform,
            ), With<Ghost>>();
            let (mesh, material, t) = ghosts.single(app.world()).unwrap();
            assert_eq!(mesh.0, arrow.mesh);
            assert_eq!(Some(&material.0), arrow.ghost.as_ref());
            assert!(
                (t.translation.y - (SURFACE - arrow.foot)).abs() < 2e-4,
                "on the board"
            );

            // Shot: nothing more is held, and the arrow waits to be confirmed.
            app.world_mut().resource_mut::<Hovering>().0 = None;
            click(&mut app, "a1");
            {
                let mut watch = app.world_mut().resource_mut::<WatchStatus>();
                let turn = watch.turn.as_mut().unwrap();
                turn.answered(Selections {
                    complete: true,
                    may_end: false,
                    state: None,
                    ..Selections {
                        seat: 1,
                        ply: 3,
                        made: vec![],
                        options: vec![],
                        complete: false,
                        may_end: false,
                        state: None,
                    }
                });
            }
            app.update();
            let mut held = app.world_mut().query_filtered::<(), With<Held>>();
            assert_eq!(held.iter(app.world()).count(), 0);
            let mut ghosts = app.world_mut().query_filtered::<(), With<Ghost>>();
            assert_eq!(ghosts.iter(app.world()).count(), 1);
            assert!(matches!(queen(&mut app).0, Some(Carry::Put(_))));

            // Sent: the arrow is shown solid at once, in its own red, without waiting for the
            // server.
            let sent = app
                .world_mut()
                .resource_mut::<WatchStatus>()
                .turn
                .as_mut()
                .unwrap()
                .submit();
            assert!(sent.is_some());
            app.update();
            let mut q = app
                .world_mut()
                .query_filtered::<&MeshMaterial3d<StandardMaterial>, With<Sent>>();
            let solid = q.single(app.world()).unwrap();
            assert_eq!(Some(&solid.0), arrow.material.as_ref());

            // Played: each piece is where it was shown, neither dropped in again.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn = None;
            watch.table.as_mut().unwrap().apply(&Event::Move {
                seq: 2,
                state: stacks(
                    2,
                    &[("a2", &[("queen", "white")]), ("a1", &[("arrow", "white")])],
                ),
            });
            app.update();
            assert_eq!(
                pieces(&mut app),
                [("a1".to_string(), false), ("a2".to_string(), false)]
            );
        }

        /// Blooms on a 3×3 grid, Warm (red, yellow) against Cool (blue, black), a capture's
        /// count of four winning, with these stones (each a cell and a colour) and counts.
        fn captures_table(stones: &[(&str, &str)], warm: u32, cool: u32) -> Table {
            let mut table = grid_table("blooms", &[]);
            table.info.seats = [
                (1, "red", ["red", "yellow"]),
                (2, "blue", ["blue", "black"]),
            ]
            .map(|(seat, word, colours)| SeatDef {
                seat,
                color: word.into(),
                label: word.into(),
                colors: colours.map(String::from).to_vec(),
            })
            .to_vec();
            table.info.pieces = ["red", "yellow", "blue", "black"]
                .map(|c| crate::net::api::PieceDef {
                    id: c.into(),
                    color: c.into(),
                    ..Default::default()
                })
                .to_vec();
            table.info.counters = vec![crate::net::api::Counter {
                id: "captured".into(),
                label: "Captured".into(),
                max: 4,
            }];
            table.state = captures_state(1, stones, warm, cool);
            table
        }

        fn captures_state(seq: u64, stones: &[(&str, &str)], warm: u32, cool: u32) -> GameState {
            let cells: Vec<(&str, &[(&str, &str)])> = stones
                .iter()
                .map(|(cell, colour)| {
                    let one: &[(&str, &str)] = match *colour {
                        "red" => &[("red", "red")],
                        "yellow" => &[("yellow", "yellow")],
                        "blue" => &[("blue", "blue")],
                        _ => &[("black", "black")],
                    };
                    (*cell, one)
                })
                .collect();
            let mut state = stacks(seq, &cells);
            state.state.reserves = serde_json::json!({
                "red": {"captured": warm},
                "blue": {"captured": cool},
            });
            state
        }

        fn in_trays(app: &mut App) -> Vec<(InTray, String, Transform)> {
            let mut q = app.world_mut().query::<(&InTray, &Name, &Rest)>();
            let mut out: Vec<_> = q
                .iter(app.world())
                .map(|(t, n, r)| (*t, n.to_string(), r.0))
                .collect();
            out.sort_by_key(|(t, ..)| (t.seat, t.index));
            out
        }

        #[test]
        fn stones_captured_are_carried_one_after_another_to_their_captors_tray() {
            let mut app = with_looks();
            app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(16),
            ));
            let before = [("a1", "blue"), ("b1", "black"), ("c3", "red")];
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(captures_table(&before, 0, 0));
            app.update();
            let mut trays = app.world_mut().query::<&Name>();
            let count = trays
                .iter(app.world())
                .filter(|n| n.starts_with("Tray of seat"))
                .count();
            assert_eq!(count, 2, "a tray for each seat");
            assert!(in_trays(&mut app).is_empty());

            // Warm plays b2 and captures a1 and b1.
            let after = [("b2", "yellow"), ("c3", "red")];
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.table.as_mut().unwrap().apply(&Event::Move {
                seq: 2,
                state: captures_state(2, &after, 2, 0),
            });
            app.update();
            let held = in_trays(&mut app);
            assert_eq!(held.len(), 2, "{held:?}");
            assert!(held.iter().all(|(t, ..)| t.seat == 0), "Warm's tray");
            assert!(held[0].1.starts_with("blue") && held[1].1.starts_with("black"));
            // In front of Warm (the player's side, +z), beyond the board, in two squares.
            let (first, second) = (held[0].2.translation, held[1].2.translation);
            assert!(first.z > 0.04 && first.y < BOARD_HEIGHT, "{first}");
            assert!(first.distance(second) > 0.01);
            let mut q = app.world_mut().query::<(&InTray, &Route, &Carry)>();
            let mut routes: Vec<(usize, Route)> = q
                .iter(app.world())
                .map(|(t, r, c)| {
                    assert!(matches!(c, Carry::Put(_)));
                    (t.index, *r)
                })
                .collect();
            routes.sort_by_key(|(i, _)| *i);
            assert_eq!(routes[0].1.lands, Cue::Capture);
            assert!(routes[1].1.delay > routes[0].1.delay, "one after another");
            // Nothing else changed: two stones left on the board.
            let mut on_board = app.world_mut().query::<&PieceAt>();
            assert_eq!(on_board.iter(app.world()).count(), 2);

            // In a second or so both are in the tray.
            for _ in 0..80 {
                app.update();
            }
            let mut q = app.world_mut().query::<(&InTray, &Transform, &Rest)>();
            for (_, t, rest) in q.iter(app.world()) {
                assert_eq!(*t, rest.0);
            }

            // Taken back: the last one out.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.table.as_mut().unwrap().apply(&Event::State {
                reason: "undo".into(),
                state: captures_state(3, &after, 1, 0),
                seats: vec![],
                lobby: false,
                host_seat: None,
            });
            app.update();
            app.update();
            let held = in_trays(&mut app);
            assert_eq!(held.len(), 1);
            assert_eq!(held[0].0.index, 0);
        }

        #[test]
        fn a_game_opened_with_captures_made_has_them_in_the_trays_already() {
            let mut app = with_looks();
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(captures_table(&[("b2", "red")], 3, 1));
            app.update();
            app.update();
            let held = in_trays(&mut app);
            let names: Vec<(usize, &str)> = held
                .iter()
                .map(|(t, n, _)| (t.seat, n.split(' ').next().unwrap()))
                .collect();
            assert_eq!(
                names,
                [(0, "blue"), (0, "black"), (0, "blue"), (1, "red")],
                "the other seat's colours by turns"
            );
            // Lying in them, not on their way.
            let mut q = app
                .world_mut()
                .query_filtered::<(), (With<InTray>, With<Carry>)>();
            assert_eq!(q.iter(app.world()).count(), 0);
        }

        #[test]
        fn a_tray_is_made_of_the_boards_wood_and_changes_with_it() {
            let mut app = with_looks();
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(captures_table(&[("b2", "red")], 0, 0));
            app.update();
            let mut q = app
                .world_mut()
                .query::<(&Name, &MeshMaterial3d<StandardMaterial>)>();
            let tray = q
                .iter(app.world())
                .find(|(n, _)| n.as_str() == "Tray of seat 1")
                .map(|(_, m)| m.0.clone())
                .expect("a tray");
            let mut kits = app.world_mut().query::<&Kit>();
            let kit = kits.single(app.world()).unwrap();
            let rebuilt = kit.woodwork.iter().find(|(h, _)| *h == tray);
            assert!(
                rebuilt.is_some_and(|(_, spec)| spec.is_board_wood()),
                "built again when another wood is chosen"
            );
        }

        /// Tak on a 3×3 grid: white (this client's) and black, flat and standing stones from
        /// one pool and the capstone from another, with these stacks and what each seat has
        /// left (stones, capstones).
        fn tak_table(
            cells: &[(&str, &[(&str, &str)])],
            white: (u32, u32),
            black: (u32, u32),
        ) -> Table {
            use crate::net::api::PieceDef;
            let mut table = grid_table("tak-5x5", cells);
            table.info.pieces = [
                ("cap", "cap", false),
                ("flat", "stone", true),
                ("wall", "stone", false),
            ]
            .map(|(id, pool, coverable)| PieceDef {
                id: id.into(),
                pool: pool.into(),
                coverable,
                ..Default::default()
            })
            .to_vec();
            table.info.reserves = [("stone", 3), ("cap", 1)]
                .map(|(k, v)| (k.to_string(), v))
                .into();
            table.state.state.reserves = serde_json::json!({
                "white": {"stone": white.0, "cap": white.1},
                "black": {"stone": black.0, "cap": black.1},
            });
            table.game.seats = vec![crate::net::api::GameSeat {
                seat: 1,
                color: "white".into(),
                name: "Ana".into(),
                bot: String::new(),
                yours: true,
            }];
            table.state.ply = 4;
            table
        }

        fn tak_turn(options: Vec<crate::net::api::LegalSelection>) -> Turn {
            let mut turn = Turn::new(1);
            turn.answered(crate::net::api::Selections {
                seat: 1,
                ply: 4,
                made: vec![],
                options,
                complete: false,
                may_end: false,
                state: None,
            });
            turn
        }

        fn option(
            gestures: &[(&str, &str)],
            from: Option<&str>,
        ) -> crate::net::api::LegalSelection {
            crate::net::api::LegalSelection {
                notation: "m".into(),
                action: "x".into(),
                to: None,
                from: from.map(Into::into),
                piece: gestures
                    .iter()
                    .find(|(k, _)| *k == "piece")
                    .map(|(_, v)| v.to_string()),
                aim: None,
                gestures: gestures
                    .iter()
                    .map(|(k, v)| Gesture {
                        kind: k.to_string(),
                        value: v.to_string(),
                    })
                    .collect(),
                captures: Vec::new(),
            }
        }

        fn supply(app: &mut App) -> Vec<(String, String, usize, Option<String>)> {
            let mut q = app.world_mut().query::<(&InSupply, Option<&Choice>)>();
            let mut out: Vec<_> = q
                .iter(app.world())
                .map(|(s, c)| {
                    (
                        s.colour.clone(),
                        s.pool.clone(),
                        s.index,
                        c.map(|c| c.0.value.clone()),
                    )
                })
                .collect();
            out.sort();
            out
        }

        #[test]
        fn each_seat_has_what_it_has_left_beside_the_board_and_a_click_takes_from_its_own() {
            let mut app = with_looks();
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(tak_table(&[], (3, 1), (2, 0)));
            app.update();
            let shown = supply(&mut app);
            let count = |colour: &str, pool: &str| {
                shown
                    .iter()
                    .filter(|s| s.0 == colour && s.1 == pool)
                    .count()
            };
            assert_eq!(
                [
                    count("white", "stone"),
                    count("white", "cap"),
                    count("black", "stone"),
                    count("black", "cap")
                ],
                [3, 1, 2, 0]
            );
            // The player's own take a flat stone, or the capstone; the other seat's nothing.
            for (colour, pool, _, choice) in &shown {
                let wanted = match (colour.as_str(), pool.as_str()) {
                    ("white", "stone") => Some("flat"),
                    ("white", "cap") => Some("cap"),
                    _ => None,
                };
                assert_eq!(choice.as_deref(), wanted, "{colour} {pool}");
            }
            // Beside the board, not on it.
            let mut q = app.world_mut().query::<(&InSupply, &Transform)>();
            for (piece, t) in q.iter(app.world()) {
                assert!(t.translation.x.abs() > 0.06, "{piece:?} {}", t.translation);
            }
            use bevy::camera::primitives::MeshAabb;
            // The player's own are clicked on a box of their model's size, not on the model's
            // own triangles, which a ray passes through where they are smallest.
            let flat = assets::model_bounds("models/tak/flat.glb").unwrap();
            let mut q = app
                .world_mut()
                .query::<(&InSupply, Option<&SimplifiedMesh>)>();
            let boxes: Vec<(String, String, Option<Handle<Mesh>>)> = q
                .iter(app.world())
                .map(|(s, m)| (s.colour.clone(), s.pool.clone(), m.map(|m| m.0.clone())))
                .collect();
            for (colour, pool, hit) in &boxes {
                assert_eq!(hit.is_some(), colour == "white", "{colour} {pool}");
                if let (Some(hit), "stone") = (hit, pool.as_str()) {
                    let meshes = app.world().resource::<Assets<Mesh>>();
                    let aabb = meshes.get(hit).unwrap().get_aabb().unwrap();
                    let (lo, hi) = (aabb.min(), aabb.max());
                    assert!(Vec3::from(lo).distance(flat.0) < 1e-6, "{lo:?}");
                    assert!(Vec3::from(hi).distance(flat.1) < 1e-6, "{hi:?}");
                }
            }

            // A flat stone taken: one fewer in the line, the stone in hand over its place; the
            // wheel makes it a standing stone.
            let options = vec![
                option(&[("piece", "flat"), ("pos", "a1")], None),
                option(&[("piece", "wall"), ("pos", "a1")], None),
                option(&[("piece", "cap"), ("pos", "a1")], None),
            ];
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn = Some(tak_turn(options));
            watch.turn.as_mut().unwrap().gesture(&Gesture {
                kind: "piece".into(),
                value: "flat".into(),
            });
            app.update();
            let shown = supply(&mut app);
            assert_eq!(
                shown
                    .iter()
                    .filter(|s| s.0 == "white" && s.1 == "stone")
                    .count(),
                2
            );
            let mut q = app.world_mut().query::<(&FromBowl, &Carry, &Rest)>();
            let (held, carry, rest) = q.single(app.world()).unwrap();
            assert_eq!(
                (held.source.as_str(), held.piece.as_str()),
                ("white/stone", "flat")
            );
            assert!(matches!(carry, Carry::Held { .. }));
            assert!(
                rest.0.translation.x > 0.06,
                "over the supply: {}",
                rest.0.translation
            );
            let pools: HashMap<String, String> =
                [("flat", "stone"), ("wall", "stone"), ("cap", "cap")]
                    .map(|(a, b)| (a.to_string(), b.to_string()))
                    .into();
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn.as_mut().unwrap().wheel(true, &pools);
            app.update();
            let mut q = app.world_mut().query::<&FromBowl>();
            assert_eq!(q.single(app.world()).unwrap().piece, "wall");
        }

        #[test]
        fn a_stack_is_lifted_some_put_back_and_each_dropped_carried_where_it_goes() {
            let mut app = with_looks();
            let two: &[(&str, &str)] = &[("flat", "black"), ("flat", "white")];
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(tak_table(&[("b2", two)], (3, 1), (2, 0)));
            app.update();
            let options = vec![
                option(&[("pos", "b2"), ("count", "1"), ("pos", "b3")], Some("b2")),
                option(
                    &[("pos", "b2"), ("count", "2"), ("pos", "b3"), ("pos", "b3")],
                    Some("b2"),
                ),
                option(
                    &[("pos", "b2"), ("count", "2"), ("pos", "b3"), ("pos", "b1")],
                    Some("b2"),
                ),
            ];
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn = Some(tak_turn(options));
            watch.turn.as_mut().unwrap().gesture(&Gesture::pos("b2"));
            app.update();
            let carried = |app: &mut App| {
                let mut q = app.world_mut().query::<(&PieceAt, Option<&Carry>)>();
                let mut out: Vec<(usize, Option<Carry>)> = q
                    .iter(app.world())
                    .map(|(at, c)| (at.layer, c.copied()))
                    .collect();
                out.sort_by_key(|(l, _)| *l);
                out
            };
            let both = carried(&mut app);
            assert!(
                both.iter()
                    .all(|(_, c)| matches!(c, Some(Carry::Held { .. }))),
                "{both:?}"
            );
            // The wheel leaves the bottom one on the board.
            let pools = HashMap::new();
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn.as_mut().unwrap().wheel(false, &pools);
            app.update();
            let one = carried(&mut app);
            assert_eq!(one[0].1, None);
            assert!(matches!(one[1].1, Some(Carry::Held { .. })));
            // Both again, then one dropped on b3: carried there; the other held over it, above.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let turn = watch.turn.as_mut().unwrap();
            turn.wheel(true, &pools);
            turn.gesture(&Gesture::pos("b3"));
            app.update();
            let dropped = carried(&mut app);
            let (Some(Carry::Put(put)), Some(Carry::Over { at, .. })) =
                (dropped[0].1, dropped[1].1)
            else {
                panic!("{dropped:?}");
            };
            let b3 = Layout::new(
                &app.world()
                    .resource::<WatchStatus>()
                    .table
                    .as_ref()
                    .unwrap()
                    .info
                    .board
                    .positions,
            )
            .centres["b3"];
            assert!(put.translation.xz().distance(b3.xz()) < 0.003);
            assert!(at.translation.xz().distance(b3.xz()) < 0.003);
            assert!(
                at.translation.y > put.translation.y + 0.005,
                "held above it"
            );
        }

        #[test]
        fn a_unit_whose_moves_do_not_click_it_is_picked_up_held_and_carried() {
            let mut app = with_looks();
            let mut table = grid_table(
                "courtfall-1.0.0",
                &[("a1", &[("queen", "white")]), ("c3", &[("queen", "black")])],
            );
            table.game.seats = vec![crate::net::api::GameSeat {
                seat: 1,
                color: "white".into(),
                name: "Ana".into(),
                bot: String::new(),
                yours: true,
            }];
            app.world_mut().resource_mut::<WatchStatus>().table = Some(table);
            let act = |hex: &str, action: &str| {
                let mut o = option(&[("pos", hex), ("action", action)], Some("a1"));
                o.to = Some(hex.into());
                o.piece = Some("queen".into());
                o
            };
            let mut turn = tak_turn(vec![
                act("b1", "move"),
                act("b2", "move"),
                act("c3", "strike"),
            ]);
            turn.gesture(&Gesture::pos("a1"));
            assert_eq!(turn.lifted(), Some("a1"));
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
            app.update();
            let queen = |app: &mut App| {
                let mut q = app
                    .world_mut()
                    .query::<(&PieceAt, Has<Held>, Option<&Carry>, &Pickable)>();
                q.iter(app.world())
                    .find(|(at, ..)| at.cell == "a1")
                    .map(|(_, held, carry, pick)| (held, carry.copied(), *pick == Pickable::IGNORE))
                    .unwrap()
            };
            // Under the pointer, and not in the way of a click on where it goes.
            assert_eq!(queen(&mut app), (true, None, true));
            let ghosts = |app: &mut App| {
                let mut q = app.world_mut().query_filtered::<&Transform, With<Ghost>>();
                q.iter(app.world())
                    .map(|t| t.translation)
                    .collect::<Vec<_>>()
            };
            // Over an empty hex it may go to, a see-through queen there; over the unit it would
            // strike, none.
            app.world_mut().resource_mut::<Hovering>().0 = Some("b2".into());
            app.update();
            assert_eq!(ghosts(&mut app).len(), 1);
            app.world_mut().resource_mut::<Hovering>().0 = Some("c3".into());
            app.update();
            assert!(ghosts(&mut app).is_empty());
            // Still under the pointer as it moves.
            assert_eq!(queen(&mut app), (true, None, true));
            // Moved: carried from under the pointer to where it goes, and clickable again.
            app.world_mut().resource_mut::<Hovering>().0 = None;
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn.as_mut().unwrap().gesture(&Gesture::pos("b2"));
            app.update();
            let (held, carry, ignored) = queen(&mut app);
            assert!(!held && !ignored);
            assert!(matches!(carry, Some(Carry::Put(_))));

            // Its second action, from where the first put it: picked up there, the same piece,
            // and carried on from there.
            let then = |hex: &str| {
                let mut o = option(&[("pos", hex), ("action", "move")], Some("b2"));
                o.to = Some(hex.into());
                o.piece = Some("queen".into());
                o
            };
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let turn = watch.turn.as_mut().unwrap();
            let made = turn.made.iter().map(|s| s.notation.clone()).collect();
            turn.answered(crate::net::api::Selections {
                seat: 1,
                ply: 4,
                made,
                options: vec![then("c2"), then("c1")],
                complete: false,
                may_end: true,
                state: None,
            });
            turn.gesture(&Gesture::pos("b2"));
            assert_eq!(turn.lifted(), Some("b2"));
            app.update();
            assert_eq!(
                queen(&mut app),
                (true, None, true),
                "the piece on a1, now on b2"
            );
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn.as_mut().unwrap().gesture(&Gesture::pos("c2"));
            app.update();
            let layout = Layout::new(
                &app.world()
                    .resource::<WatchStatus>()
                    .table
                    .as_ref()
                    .unwrap()
                    .info
                    .board
                    .positions,
            );
            match queen(&mut app) {
                (false, Some(Carry::Put(to)), false) => {
                    assert!(to.translation.xz().distance(layout.centres["c2"].xz()) < 1e-6);
                }
                other => panic!("{other:?}"),
            }
        }

        #[test]
        fn tints_are_tiles_and_a_stamp_and_the_drafts_stock_waits_beside_the_board() {
            let mut app = with_looks();
            let mut table = grid_table(
                "courtfall-1.0.0",
                &[("a1", &[("queen", "white")]), ("c1", &[("queen", "black")])],
            );
            table.state.state.ext = serde_json::json!({"view": {
                "tints": {"a1": "green", "b2": "eye", "c3": "zone-1"},
                "tiles_left": {"green": 2, "blue": 1, "red": 0},
            }});
            app.world_mut().resource_mut::<WatchStatus>().table = Some(table);
            app.update();
            let tinted = |app: &mut App| {
                let mut q = app.world_mut().query::<(&TintAt, Has<Cell>)>();
                let mut all: Vec<(String, String, bool)> = q
                    .iter(app.world())
                    .map(|(t, lit)| (t.cell.clone(), t.key.clone(), lit))
                    .collect();
                all.sort();
                all
            };
            // A tile on the green hex, lit as a cell is; a stamp on the Eye; a zone, which the
            // look does not draw, nothing.
            assert_eq!(
                tinted(&mut app),
                [
                    ("a1".to_string(), "green".to_string(), true),
                    ("b2".to_string(), "eye".to_string(), false)
                ]
            );
            // A unit on a tile stands on it.
            let (low, high) = assets::model_span("models/tile_hex.glb").unwrap();
            let shown = drawn(&mut app);
            let on_tile = shown[&("a1".to_string(), 0)].1.translation.y;
            let on_board = shown[&("c1".to_string(), 0)].1.translation.y;
            assert!((on_tile - on_board - (high - low)).abs() < 1e-6);
            let stock = |app: &mut App| {
                let mut q = app.world_mut().query::<(&InStock, &Transform)>();
                let mut all: Vec<(String, usize, Transform)> = q
                    .iter(app.world())
                    .map(|(s, t)| (s.kind.clone(), s.index, *t))
                    .collect();
                all.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
                all
            };
            // The tiles still to draft, beside the board on the table, as many as are left.
            let left = stock(&mut app);
            let kinds: Vec<_> = left.iter().map(|(k, i, _)| (k.as_str(), *i)).collect();
            assert_eq!(kinds, [("blue", 0), ("green", 0), ("green", 1)]);
            assert!(left.iter().all(|(.., t)| t.translation.x < -0.05));
            assert!(left.iter().all(|(.., t)| t.translation.y < SURFACE));

            // Over a hex any of the tiles may be laid on: nothing under the pointer, since which
            // is not chosen yet.
            let lay = |kind: &str| {
                let mut o = option(&[("pos", "c2"), ("action", &format!("tile-{kind}"))], None);
                o.to = Some("c2".into());
                o.piece = Some(kind.into());
                o
            };
            app.world_mut().resource_mut::<WatchStatus>().turn =
                Some(tak_turn(vec![lay("green"), lay("blue")]));
            app.world_mut().resource_mut::<Hovering>().0 = Some("c2".into());
            app.update();
            let mut q = app.world_mut().query_filtered::<(), With<Ghost>>();
            assert_eq!(q.iter(app.world()).count(), 0, "no stone under the pointer");
            app.world_mut().resource_mut::<Hovering>().0 = None;

            // A tile taken from the stock: held under the pointer, one fewer in the stock, and
            // only the hexes it may go on lit.
            let from_stock = |app: &mut App| {
                let mut q = app
                    .world_mut()
                    .query::<(&FromStock, Has<Held>, Option<&Carry>, &Mesh3d)>();
                q.iter(app.world())
                    .map(|(f, held, carry, mesh)| (f.clone(), held, carry.copied(), mesh.0.clone()))
                    .collect::<Vec<_>>()
            };
            let mut lay_c1 = lay("green");
            lay_c1.to = Some("c1".into());
            lay_c1.gestures[0] = Gesture::pos("c1");
            app.world_mut().resource_mut::<WatchStatus>().turn =
                Some(tak_turn(vec![lay("green"), lay("blue"), lay_c1]));
            app.update();
            let mut q = app.world_mut().query::<(&InStock, &TakeToPlace)>();
            let takes: Vec<(String, Vec<Gesture>)> = q
                .iter(app.world())
                .map(|(s, t)| (s.kind.clone(), t.0.clone()))
                .collect();
            assert_eq!(takes.len(), 3, "each piece the player may take: {takes:?}");
            let green = takes.iter().find(|(k, _)| k == "green").unwrap().1.clone();
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let turn = watch.turn.as_mut().unwrap();
            assert_eq!(turn.take_to_place(&green), crate::play::Outcome::Changed);
            assert_eq!(
                turn.targets().into_iter().collect::<Vec<_>>(),
                ["c1", "c2"],
                "where a green tile may go"
            );
            app.update();
            let held = from_stock(&mut app);
            assert_eq!(held.len(), 1);
            assert_eq!(held[0].0.kind, "green");
            assert!(held[0].1, "under the pointer");
            let server = app.world().resource::<AssetServer>();
            assert!(
                server
                    .get_path(held[0].3.id())
                    .is_some_and(|p| p.path().ends_with("tile_hex.glb"))
            );
            let kinds: Vec<_> = stock(&mut app).into_iter().map(|(k, ..)| k).collect();
            assert_eq!(
                kinds,
                ["blue", "green"],
                "the one in hand is out of the stock"
            );
            // Still in hand as the pointer goes over the board (the move shown again).
            app.world_mut().resource_mut::<Hovering>().0 = Some("c1".into());
            app.update();
            app.world_mut().resource_mut::<Hovering>().0 = None;
            app.update();
            assert_eq!(
                from_stock(&mut app).len(),
                1,
                "not taken for an aimed piece"
            );
            // Over a hex it may go on, a see-through tile there; over one it may not, none.
            let ghosts = |app: &mut App| {
                let mut q = app
                    .world_mut()
                    .query_filtered::<(&Transform, &Mesh3d), With<TileGhost>>();
                q.iter(app.world())
                    .map(|(t, m)| (t.translation, m.0.clone()))
                    .collect::<Vec<_>>()
            };
            app.world_mut().resource_mut::<Hovering>().0 = Some("c2".into());
            app.update();
            let shown = ghosts(&mut app);
            assert_eq!(shown.len(), 1);
            let layout = Layout::new(
                &app.world()
                    .resource::<WatchStatus>()
                    .table
                    .as_ref()
                    .unwrap()
                    .info
                    .board
                    .positions,
            );
            assert!(shown[0].0.xz().distance(layout.centres["c2"].xz()) < 1e-6);
            assert_eq!(shown[0].1, held[0].3, "the tile in hand's model");
            // In place of the cell's light there; the others it may go on still lit.
            let lit = |app: &mut App, id: &str| {
                let mut q = app.world_mut().query_filtered::<&Cell, With<Lit>>();
                q.iter(app.world()).any(|c| c.id == id)
            };
            assert!(!lit(&mut app, "c2"));
            assert!(lit(&mut app, "c1"));
            app.world_mut().resource_mut::<Hovering>().0 = Some("c1".into());
            app.update();
            let moved = ghosts(&mut app);
            assert_eq!(moved.len(), 1, "it follows the pointer");
            assert!(moved[0].0.xz().distance(layout.centres["c1"].xz()) < 1e-6);
            app.world_mut().resource_mut::<Hovering>().0 = Some("b2".into());
            app.update();
            assert!(ghosts(&mut app).is_empty(), "the Eye: not a target");
            app.world_mut().resource_mut::<Hovering>().0 = Some("b3".into());
            app.update();
            assert!(ghosts(&mut app).is_empty(), "nor where no green may go");
            app.world_mut().resource_mut::<Hovering>().0 = None;
            app.update();
            assert!(ghosts(&mut app).is_empty());
            // Laid with a click on a hex it may go on: carried there, not shown see-through.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let turn = watch.turn.as_mut().unwrap();
            assert_eq!(turn.gesture(&Gesture::pos("c2")), crate::play::Outcome::Ask);
            assert_eq!(
                (turn.made[0].to.as_deref(), turn.made[0].piece.as_deref()),
                (Some("c2"), Some("green"))
            );
            app.update();
            let carried = from_stock(&mut app);
            assert_eq!(carried.len(), 1);
            assert_eq!(carried[0].0.cell.as_deref(), Some("c2"));
            assert!(!carried[0].1);
            assert!(matches!(carried[0].2, Some(Carry::Put(_))));
            let mut q = app.world_mut().query_filtered::<(), With<Ghost>>();
            assert_eq!(q.iter(app.world()).count(), 0);

            // Chosen on the panel instead (the hex, then the tile's button): carried from the
            // stock, and one fewer there.
            let mut turn = tak_turn(vec![lay("blue")]);
            turn.gesture(&Gesture::pos("c2"));
            turn.gesture(&Gesture {
                kind: "action".into(),
                value: "tile-blue".into(),
            });
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
            app.update();
            let kinds: Vec<_> = stock(&mut app).into_iter().map(|(k, ..)| k).collect();
            assert_eq!(kinds, ["green", "green"]);
            let carried = from_stock(&mut app);
            assert_eq!(carried.len(), 1);
            assert_eq!(
                (carried[0].0.kind.as_str(), carried[0].0.cell.as_deref()),
                ("blue", Some("c2"))
            );
            let mut q = app.world_mut().query_filtered::<(), With<Ghost>>();
            assert_eq!(q.iter(app.world()).count(), 0);

            // Laid: the hex is tinted, and the stock as the view now counts it.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn = None;
            let table = watch.table.as_mut().unwrap();
            table.state.state.ext = serde_json::json!({"view": {
                "tints": {"a1": "green", "b2": "eye", "c2": "blue"},
                "tiles_left": {"green": 2, "blue": 0, "red": 0},
            }});
            app.update();
            assert_eq!(tinted(&mut app).len(), 3);
            assert_eq!(stock(&mut app).len(), 2);
            assert!(
                from_stock(&mut app).is_empty(),
                "laid: the hex's own tile now"
            );
            // Once the draft is over the view counts nothing, and the stock is gone.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let table = watch.table.as_mut().unwrap();
            table.state.state.ext = serde_json::json!({"view": {
                "tints": {"a1": "green", "c2": "blue"},
            }});
            app.update();
            assert!(stock(&mut app).is_empty());
            assert_eq!(tinted(&mut app).len(), 2, "the Eye covered");
        }

        #[test]
        fn fendos_pawns_in_stock_its_fences_and_a_fence_taken_from_the_bowl_and_put_down() {
            use crate::net::api::PieceDef;
            let mut app = with_looks();
            let mut table = grid_table("fendo", &[("b2", &[("piece", "white")])]);
            table.info.seats[1].color = "orange".into();
            table.info.pieces = vec![PieceDef {
                id: "piece".into(),
                pool: "piece".into(),
                coverable: true,
                ..Default::default()
            }];
            table.state.state.ext = serde_json::json!({"view": {
                "walls": [["a1", "b1"]],
                "seats": {
                    "1": {"counters": [{"id": "stock", "value": 2}]},
                    "2": {"counters": [{"id": "stock", "value": 1}]},
                },
            }});
            table.game.seats = vec![crate::net::api::GameSeat {
                seat: 1,
                color: "white".into(),
                name: "Ana".into(),
                bot: String::new(),
                yours: true,
            }];
            app.world_mut().resource_mut::<WatchStatus>().table = Some(table);
            app.update();
            // Each seat's pawns in stock beside the board; a click on the player's adds one.
            let shown = supply(&mut app);
            let white: Vec<_> = shown.iter().filter(|s| s.0 == "white").collect();
            assert_eq!(white.len(), 2);
            assert!(white.iter().all(|s| s.3.as_deref() == Some("add")));
            assert_eq!(shown.iter().filter(|s| s.0 == "orange").count(), 1);
            // The bowl both take fences from, and the position's fence between a1 and b1.
            let mut q = app.world_mut().query::<&TakeUp>();
            let sides: Vec<Gesture> = q.single(app.world()).unwrap().0.clone();
            assert_eq!(sides.len(), 4);
            let mut q = app.world_mut().query::<(&WallAt, &Transform)>();
            let (wall, at) = q.single(app.world()).unwrap();
            assert_eq!(wall.0, "a1|b1");
            let layout = Layout::new(
                &app.world()
                    .resource::<WatchStatus>()
                    .table
                    .as_ref()
                    .unwrap()
                    .info
                    .board
                    .positions,
            );
            let between = (layout.centres["a1"] + layout.centres["b1"]) / 2.0;
            assert!(at.translation.xz().distance(between.xz()) < 1e-5);

            // Over an empty square before a pawn is taken: nothing shown; with one in hand, a
            // see-through pawn, not the generic stone.
            let adds = vec![
                option(&[("action", "add"), ("pos", "a3")], None),
                option(&[("action", "add"), ("pos", "c3")], None),
            ];
            let mut adds: Vec<_> = adds
                .into_iter()
                .map(|mut o| {
                    o.to = Some(o.gestures[1].value.clone());
                    o
                })
                .collect();
            // And the piece's own move, so adding is a choice, not one made for the player.
            adds.push(option(&[("pos", "b2"), ("action", "fence-n")], Some("b2")));
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(tak_turn(adds));
            app.world_mut().resource_mut::<Hovering>().0 = Some("a3".into());
            app.update();
            let mut q = app.world_mut().query_filtered::<(), With<Ghost>>();
            assert_eq!(q.iter(app.world()).count(), 0, "no stone under the pointer");
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn.as_mut().unwrap().gesture(&Gesture {
                kind: "action".into(),
                value: "add".into(),
            });
            app.update();
            let pawn = assets::path("models/chess/pawn.glb");
            let mut q = app.world_mut().query_filtered::<&Mesh3d, With<Ghost>>();
            let ghost = q.single(app.world()).unwrap().0.clone();
            let server = app.world().resource::<AssetServer>();
            assert_eq!(
                server.get_path(ghost.id()).map(|p| p.path().to_path_buf()),
                server
                    .get_path(
                        server
                            .load::<Mesh>(
                                GltfAssetLabel::Primitive {
                                    mesh: 0,
                                    primitive: 0
                                }
                                .from_asset(pawn)
                            )
                            .id()
                    )
                    .map(|p| p.path().to_path_buf())
            );
            app.world_mut().resource_mut::<Hovering>().0 = None;

            // The piece on b2 stays, a fence taken: it waits, the sides it may go on lit.
            let options = vec![
                option(&[("pos", "b2"), ("action", "fence-n")], Some("b2")),
                option(&[("pos", "b2"), ("action", "fence-e")], Some("b2")),
            ];
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn = Some(tak_turn(options));
            let turn = watch.turn.as_mut().unwrap();
            turn.gesture(&Gesture::pos("b2"));
            turn.take_up(&sides);
            app.update();
            // Under the pointer, as an aimed piece is.
            let mut q = app.world_mut().query_filtered::<&HeldFence, With<Held>>();
            assert_eq!(q.single(app.world()).unwrap().0, None);
            // Where it may go is not lit: nothing of the board is.
            let mut q = app.world_mut().query_filtered::<(), With<Lit>>();
            assert_eq!(q.iter(app.world()).count(), 0, "the sides are not lit");
            // The pointer near b2's east side aims there: a see-through fence on it.
            let layout_now = layout.clone();
            let east = (layout_now.centres["b2"] + layout_now.centres["c2"]) / 2.0;
            let surface = Vec3::Y * SURFACE;
            app.world_mut().resource_mut::<PointerOnBoard>().0 =
                Some((east + surface, east + surface + Vec3::Y));
            app.update();
            assert_eq!(
                app.world()
                    .resource::<SideAim>()
                    .choice
                    .as_ref()
                    .map(|c| c.value.as_str()),
                Some("fence-e")
            );
            let mut q = app
                .world_mut()
                .query_filtered::<(&Transform, &Visibility), With<FenceGhost>>();
            let (ghost, shown) = q.single(app.world()).unwrap();
            assert_eq!(*shown, Visibility::Inherited);
            assert!(ghost.translation.xz().distance(east.xz()) < 1e-5);
            // Far from every side: aimed nowhere.
            let far = layout_now.centres["a3"] + surface;
            app.world_mut().resource_mut::<PointerOnBoard>().0 = Some((far, far + Vec3::Y));
            app.update();
            assert_eq!(app.world().resource::<SideAim>().choice, None);
            // Put on the east side: carried to the side b2 shares with c2.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn.as_mut().unwrap().gesture(&Gesture {
                kind: "action".into(),
                value: "fence-e".into(),
            });
            app.update();
            let mut q = app.world_mut().query::<(&HeldFence, &Carry)>();
            let (held, carry) = q.single(app.world()).unwrap();
            assert_eq!(held.0.as_deref(), Some("b2|c2"));
            let Carry::Put(to) = carry else {
                panic!("{carry:?}")
            };
            let side = (layout.centres["b2"] + layout.centres["c2"]) / 2.0;
            assert!(to.translation.xz().distance(side.xz()) < 1e-5);
        }

        #[test]
        fn a_stone_is_held_under_the_pointer_and_carried_from_there_where_it_is_placed() {
            // Go, with its bowls, and a game with no look of its own: the same.
            for id in ["go-9x9", "g"] {
                let mut app = with_looks();
                let mut table = grid_table(id, &[]);
                table.game.seats = vec![crate::net::api::GameSeat {
                    seat: 1,
                    color: "white".into(),
                    name: "Ana".into(),
                    bot: String::new(),
                    yours: true,
                }];
                app.world_mut().resource_mut::<WatchStatus>().table = Some(table);
                app.update();
                let mut options: Vec<_> = ["a1", "b2"]
                    .iter()
                    .map(|c| {
                        let mut o = option(&[("piece", "stone"), ("pos", c)], None);
                        o.to = Some(c.to_string());
                        o
                    })
                    .collect();
                options.push(option(&[("action", "pass")], None));
                app.world_mut().resource_mut::<WatchStatus>().turn = Some(tak_turn(options));
                app.update();
                let mut q = app.world_mut().query_filtered::<&FromBowl, With<Held>>();
                let held = q
                    .single(app.world())
                    .unwrap_or_else(|_| panic!("{id}: a stone held"));
                assert_eq!(held.piece, "stone", "{id}");
                // Placed on b2: carried there from under the pointer, solid, no ghost over it.
                let mut watch = app.world_mut().resource_mut::<WatchStatus>();
                let outcome = watch.turn.as_mut().unwrap().gesture(&Gesture::pos("b2"));
                assert_eq!(outcome, crate::play::Outcome::Ask, "{id}");
                app.update();
                let mut q = app.world_mut().query::<(&FromBowl, &Carry, Has<Held>)>();
                let (placed, carry, held) = q.single(app.world()).unwrap();
                assert_eq!(placed.cell.as_deref(), Some("b2"), "{id}");
                assert!(matches!(carry, Carry::Put(_)) && !held, "{id}");
                let mut q = app.world_mut().query_filtered::<(), With<Ghost>>();
                assert_eq!(
                    q.iter(app.world()).count(),
                    0,
                    "{id}: no see-through stone on it"
                );
            }
        }

        #[test]
        fn a_game_that_counts_nothing_has_no_trays() {
            let mut app = with_looks();
            app.world_mut().resource_mut::<WatchStatus>().table = Some(table());
            app.update();
            let mut trays = app.world_mut().query::<&Name>();
            assert!(
                !trays
                    .iter(app.world())
                    .any(|n| n.starts_with("Tray of seat"))
            );
        }

        #[test]
        fn a_stone_is_taken_from_its_bowl_waits_over_it_and_is_carried_to_its_cell() {
            use crate::net::api::{LegalSelection, PieceDef, Selections};
            use crate::play::{Choice, Turn};

            let mut app = with_looks();
            app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(16),
            ));
            let mut table = grid_table("blooms", &[]);
            table.info.seats = [
                (1, "red", ["red", "yellow"]),
                (2, "blue", ["blue", "black"]),
            ]
            .map(|(seat, word, colours)| SeatDef {
                seat,
                color: word.into(),
                label: word.into(),
                colors: colours.map(String::from).to_vec(),
            })
            .to_vec();
            table.info.pieces = ["red", "yellow", "blue", "black"]
                .map(|c| PieceDef {
                    id: c.into(),
                    color: c.into(),
                    ..Default::default()
                })
                .to_vec();
            table.game.seats = vec![crate::net::api::GameSeat {
                seat: 1,
                color: "red".into(),
                name: "Ana".into(),
                bot: String::new(),
                yours: true,
            }];
            app.world_mut().resource_mut::<WatchStatus>().table = Some(table);
            app.update();

            // A bowl for each colour, each taking a stone of its own.
            let mut q = app
                .world_mut()
                .query::<(&crate::bowls::Bowl, Option<&Choice>)>();
            let mut bowls: Vec<(String, String)> = q
                .iter(app.world())
                .map(|(b, c)| {
                    (
                        b.0.clone(),
                        c.map(|c| c.0.value.clone()).unwrap_or_default(),
                    )
                })
                .collect();
            bowls.sort();
            assert_eq!(
                bowls,
                [
                    ("black", "black"),
                    ("blue", "blue"),
                    ("red", "red"),
                    ("yellow", "yellow")
                ]
                .map(|(a, b)| (a.to_string(), b.to_string()))
            );

            let place = |piece: &str, cell: &str| LegalSelection {
                notation: format!("{piece}{cell}"),
                action: "place".into(),
                to: Some(cell.into()),
                from: None,
                piece: Some(piece.into()),
                aim: None,
                gestures: vec![
                    Gesture {
                        kind: "piece".into(),
                        value: piece.into(),
                    },
                    Gesture::pos(cell),
                ],
                captures: Vec::new(),
            };
            let mut turn = Turn::new(1);
            turn.answered(Selections {
                seat: 1,
                ply: 3,
                made: vec![],
                options: vec![
                    place("red", "a1"),
                    place("red", "b2"),
                    place("yellow", "a1"),
                ],
                complete: false,
                may_end: false,
                state: None,
            });
            turn.gesture(&Gesture {
                kind: "piece".into(),
                value: "red".into(),
            });
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
            app.update();
            let mut q = app
                .world_mut()
                .query::<(Entity, &FromBowl, &Carry, &Transform)>();
            let (held, from, carry, t) = q.single(app.world()).unwrap();
            assert_eq!((from.source.as_str(), from.cell.as_deref()), ("red", None));
            assert!(matches!(carry, Carry::Held { .. }));
            assert!(
                t.translation.x.abs() > 0.03,
                "beside the board, over its bowl"
            );

            // Placed: the same stone, carried to the cell, solid; nothing see-through there.
            {
                let mut watch = app.world_mut().resource_mut::<WatchStatus>();
                let turn = watch.turn.as_mut().unwrap();
                turn.gesture(&Gesture::pos("a1"));
                turn.answered(Selections {
                    seat: 1,
                    ply: 3,
                    made: vec!["reda1".into()],
                    options: vec![],
                    complete: true,
                    may_end: false,
                    state: None,
                });
            }
            app.update();
            let mut q = app.world_mut().query::<(Entity, &FromBowl, &Carry)>();
            let (placed, from, carry) = q.single(app.world()).unwrap();
            assert_eq!(placed, held, "the one in hand goes on");
            assert_eq!(from.cell.as_deref(), Some("a1"));
            let Carry::Put(to) = *carry else {
                panic!("not put down")
            };
            let layout = Layout::new(&grid_table("blooms", &[]).info.board.positions);
            assert_eq!(to.translation.xz(), layout.centres["a1"].xz());
            let mut ghosts = app.world_mut().query_filtered::<(), With<Ghost>>();
            assert_eq!(ghosts.iter(app.world()).count(), 0);

            // Played: the stone the server reports is where it was set down, not dropped again,
            // and the one from the bowl is gone.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn = None;
            watch.table.as_mut().unwrap().apply(&Event::Move {
                seq: 2,
                state: stacks(2, &[("a1", &[("red", "red")])]),
            });
            app.update();
            assert_eq!(pieces(&mut app), [("a1".to_string(), false)]);
            let mut q = app.world_mut().query::<&FromBowl>();
            assert_eq!(q.iter(app.world()).count(), 0);
        }

        #[test]
        fn a_game_whose_look_says_so_has_a_chessboard() {
            let mut app = with_looks();
            app.world_mut().resource_mut::<WatchStatus>().table = Some(grid_table("amazons", &[]));
            app.update();
            let mut q = app
                .world_mut()
                .query::<(&Cell, &MeshMaterial3d<StandardMaterial>)>();
            let rest: HashMap<String, Handle<StandardMaterial>> = q
                .iter(app.world())
                .map(|(c, m)| (c.id.clone(), m.0.clone()))
                .collect();
            assert_eq!(rest["a1"], rest["b2"]);
            assert_eq!(rest["a1"], rest["c1"]);
            assert_ne!(rest["a1"], rest["a2"]);
            assert_ne!(rest["a1"], rest["b1"]);
            let mut borders = app.world_mut().query::<&Name>();
            assert!(borders.iter(app.world()).any(|n| n.as_str() == "Border"));

            // Any other game's cells are all the board's wood.
            let mut app = with_looks();
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(grid_table("breakthrough", &[]));
            app.update();
            let mut q = app
                .world_mut()
                .query_filtered::<&MeshMaterial3d<StandardMaterial>, With<Cell>>();
            let materials: std::collections::HashSet<_> =
                q.iter(app.world()).map(|m| m.0.clone()).collect();
            assert_eq!(materials.len(), 1);
            let mut borders = app.world_mut().query::<&Name>();
            assert!(!borders.iter(app.world()).any(|n| n.as_str() == "Border"));
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

        /// Each piece of two sides shown, by its cell: its upper half's material and its lower
        /// half's, and the piece.
        fn sides(
            app: &mut App,
        ) -> HashMap<String, (AssetId<StandardMaterial>, AssetId<StandardMaterial>, Entity)>
        {
            let mut materials = app.world_mut().query::<&MeshMaterial3d<StandardMaterial>>();
            let halves: Vec<(String, Entity, Entity)> = app
                .world_mut()
                .query::<(Entity, &PieceAt, &Sides)>()
                .iter(app.world())
                .map(|(e, at, sides)| (at.cell.clone(), e, sides.under))
                .collect();
            halves
                .into_iter()
                .map(|(cell, piece, under)| {
                    let upper = materials.get(app.world(), piece).unwrap().0.id();
                    let lower = materials.get(app.world(), under).unwrap().0.id();
                    (cell, (upper, lower, piece))
                })
                .collect()
        }

        #[test]
        fn a_disc_lies_its_seats_side_up_and_one_that_changes_hands_is_turned_over() {
            let mut app = with_looks();
            app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(50),
            ));
            let cells: &[(&str, &[(&str, &str)])] = &[
                ("a1", &[("disc", "black")]),
                ("b1", &[("disc", "white")]),
                ("c3", &[("disc", "white")]),
            ];
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(grid_table("reversi", cells));
            app.update();
            app.update();
            let shown = sides(&mut app);
            assert_eq!(shown.len(), 3, "each disc dressed: {shown:?}");
            let (black, white) = (shown["a1"], shown["b1"]);
            // The same two materials, the other way up: the dark side up on a black disc.
            assert_ne!(black.0, black.1);
            assert_eq!((black.0, black.1), (white.1, white.0));
            assert_eq!(shown["c3"].0, white.0);
            // The lower half is picked as the piece is.
            let under = app.world().get::<Sides>(black.2).unwrap().under;
            assert!(app.world().get::<Pickable>(under).is_some());

            // Black plays c1, which turns b1 over.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.table.as_mut().unwrap().apply(&Event::Move {
                seq: 2,
                state: stacks(
                    2,
                    &[
                        ("a1", &[("disc", "black")]),
                        ("b1", &[("disc", "black")]),
                        ("c1", &[("disc", "black")]),
                        ("c3", &[("disc", "white")]),
                    ],
                ),
            });
            app.update();
            app.update();
            let shown = sides(&mut app);
            let turned = shown["b1"];
            assert_eq!(turned.2, white.2, "the same disc, not another put down");
            assert_eq!(
                (turned.0, turned.1),
                (black.0, black.1),
                "its colour's side up"
            );
            let rest = app.world().get::<Flipping>(turned.2).unwrap().rest;
            assert!(app.world().get::<Rest>(turned.2).is_none());
            // Until it goes over, it lies upside down: its old side up.
            let at = *app.world().get::<Transform>(turned.2).unwrap();
            assert!((at.rotation * Vec3::Y).y < -0.99);
            // Over by now, and where it lies, with its new side up.
            let mut highest = at.translation.y;
            for _ in 0..30 {
                app.update();
                let t = app.world().get::<Transform>(turned.2).unwrap();
                highest = highest.max(t.translation.y);
            }
            assert!(
                highest > rest.translation.y + 0.01,
                "raised as it went over"
            );
            assert!(app.world().get::<Flipping>(turned.2).is_none());
            assert_eq!(app.world().get::<Rest>(turned.2).unwrap().0, rest);
            let at = app.world().get::<Transform>(turned.2).unwrap();
            assert!((at.rotation * Vec3::Y).distance(Vec3::Y) < 1e-5);
            // The others were left as they were.
            assert_eq!(sides(&mut app)["c3"], shown["c3"]);
            assert_eq!(sides(&mut app)["a1"].2, black.2);
        }

        /// A game with no board (`board.on = "table"`) whose pieces have a part of their own:
        /// the die's pips, inked.
        fn with_table_look() -> App {
            let mut app = app();
            let look = Appearance::parse(
                r#"
games = ["t-table"]

[board]
on = "table"

[pieces.die]
model = "models/die.glb"
stands = "fitted"
parts = { pips = "ink" }

[materials.ink]
tint = [0.1, 0.2, 0.6]
"#,
            )
            .unwrap();
            assert!(look.problems().is_empty(), "{:?}", look.problems());
            app.insert_resource(Appearances(vec![look]));
            app
        }

        #[test]
        fn a_game_with_no_board_has_its_cells_unseen_on_the_table_and_raised_by_a_stack() {
            let mut app = with_table_look();
            let cells: &[(&str, &[(&str, &str)])] = &[
                ("a1", &[("die", "white")]),
                ("b1", &[("die", "white"), ("die", "black")]),
            ];
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(grid_table("t-table", cells));
            app.update();
            app.update();
            let mut names = app.world_mut().query::<&Name>();
            assert!(
                !names
                    .iter(app.world())
                    .any(|n| n.as_str() == "Board" || n.as_str() == "Lines"),
                "no slab and no lines"
            );
            let mut q = app.world_mut().query::<(
                &Cell,
                &Transform,
                &MeshMaterial3d<StandardMaterial>,
                Has<Pickable>,
            )>();
            let shown: HashMap<String, (f32, Handle<StandardMaterial>, bool)> = q
                .iter(app.world())
                .map(|(c, t, m, p)| (c.id.clone(), (t.translation.y, m.0.clone(), p)))
                .collect();
            assert_eq!(shown.len(), 9, "a cell for every position");
            assert!(shown.values().all(|(_, _, pickable)| *pickable));
            let materials = app.world().resource::<Assets<StandardMaterial>>();
            let (_, unseen, _) = &shown["c3"];
            assert!(materials.get(unseen).unwrap().base_color.alpha() == 0.0);
            // On the table where nothing stands; on the top of a die, or of two.
            let (low, high) = assets::model_span("models/die.glb").unwrap();
            let die = high - low;
            let (empty, one, two) = (shown["c3"].0, shown["a1"].0, shown["b1"].0);
            assert!((empty - CELL_LIFT).abs() < 1e-6, "{empty}");
            assert!(
                one > die * 0.9 && one < die * 1.1,
                "{one} for a die {die} tall"
            );
            assert!((two - CELL_LIFT - 2.0 * (one - CELL_LIFT)).abs() < 1e-5);
        }

        #[test]
        fn a_part_of_a_pieces_own_is_drawn_on_every_seats_piece_in_its_material() {
            let mut app = with_table_look();
            let cells: &[(&str, &[(&str, &str)])] =
                &[("a1", &[("die", "white")]), ("c3", &[("die", "black")])];
            app.world_mut().resource_mut::<WatchStatus>().table =
                Some(grid_table("t-table", cells));
            app.update();
            app.update();
            let shown = drawn(&mut app);
            let (white, black) = (&shown[&("a1".into(), 0)], &shown[&("c3".into(), 0)]);
            assert_ne!(white.3, black.3, "each in its seat's material");
            let part = |app: &App, piece: Entity| {
                let children = &app.world().get::<Parts>(piece).expect("dressed").0;
                assert_eq!(children.len(), 1);
                let child = children[0];
                assert!(
                    app.world().get::<Pickable>(child).is_some(),
                    "picked as the piece"
                );
                (
                    app.world().get::<Mesh3d>(child).unwrap().0.clone(),
                    app.world()
                        .get::<MeshMaterial3d<StandardMaterial>>(child)
                        .unwrap()
                        .0
                        .clone(),
                )
            };
            let (white_part, black_part) = (part(&app, white.4), part(&app, black.4));
            assert_eq!(white_part, black_part, "the same ink on both");
            assert_ne!(white_part.0, white.2, "its own part of the model");
            let ink = app
                .world()
                .resource::<Assets<StandardMaterial>>()
                .get(&white_part.1)
                .unwrap()
                .base_color
                .to_srgba();
            assert!(ink.blue > ink.red && ink.alpha == 1.0, "{ink:?}");
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

        /// A made-up game of rings and discs on a grid's points, played by white: a move of
        /// a ring leaves a disc where it stood, and each seat's rings removed are its score,
        /// kept on dots, three wanted.
        fn with_rings_look(cells: &[(&str, &[(&str, &str)])]) -> App {
            let mut app = app();
            let look = Appearance::parse(
                r#"
games = ["t-rings"]

[board]
on = "points"
margin = 2.0

[pieces.ring]
model = "models/ring.glb"
stands = "upright"
leaves = "disc"

[pieces.disc]
model = "models/disc.glb"
stands = "upright"
sides = { light = "light", dark = "dark" }
up = { black = "dark", white = "light" }

[materials.light]
tint = [0.9, 0.88, 0.82]

[materials.dark]
tint = [0.05, 0.05, 0.05]

[score]
counter = "rings"
piece = "ring"
"#,
            )
            .unwrap();
            assert!(look.problems().is_empty(), "{:?}", look.problems());
            app.insert_resource(Appearances(vec![look]));
            let mut table = grid_table("t-rings", cells);
            table.game.seats = vec![crate::net::api::GameSeat {
                seat: 1,
                color: "white".into(),
                name: "Ana".into(),
                bot: String::new(),
                yours: true,
            }];
            table.state.state.ext = rings_removed(0, 0);
            app.world_mut().resource_mut::<WatchStatus>().table = Some(table);
            app.update();
            app
        }

        /// The view's count of each seat's rings removed.
        fn rings_removed(white: u32, black: u32) -> serde_json::Value {
            let seat =
                |n: u32| serde_json::json!({"counters": [{"id": "rings", "value": n, "max": 3}]});
            serde_json::json!({"view": {"seats": {"1": seat(white), "2": seat(black)}}})
        }

        #[test]
        fn the_rings_a_seat_removes_go_on_the_dots_at_its_right_hand() {
            let mut app = with_rings_look(&[
                ("a1", &[("ring", "white")]),
                ("b2", &[("disc", "white")]),
                ("c3", &[("ring", "black")]),
            ]);
            let mut names = app.world_mut().query::<&Name>();
            assert!(
                names
                    .iter(app.world())
                    .any(|n| n.as_str() == "Score's dots")
            );
            assert!(in_trays(&mut app).is_empty());
            // White removes its ring on a1 (with a row, gone with it): its count is one.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let mut after = stacks(2, &[("c3", &[("ring", "black")])]);
            after.state.ext = rings_removed(1, 0);
            watch.table.as_mut().unwrap().apply(&Event::Move {
                seq: 2,
                state: after,
            });
            app.update();
            let held = in_trays(&mut app);
            assert_eq!(held.len(), 1, "the ring, not the disc: {held:?}");
            let (at, name, rest) = &held[0];
            assert_eq!((at.seat, at.index), (0, 0));
            assert!(name.starts_with("ring"), "{name}");
            // At the right hand of white (who sits at +z), beyond the points, on the board's
            // margin: the first dot, the nearest white.
            let layout = Layout::new(&grid_table("t-rings", &[]).info.board.positions);
            let right = layout
                .centres
                .values()
                .map(|c| c.x)
                .fold(f32::MIN, f32::max);
            let p = rest.translation;
            assert!(p.x > right + layout.pitch && p.z > 0.0, "{p}");
            assert!(
                p.y >= BOARD_HEIGHT - 1e-4,
                "on the board, not the table: {p}"
            );
            // It lands on its dot cheered, as a win is.
            let mut q = app.world_mut().query_filtered::<&Route, With<InTray>>();
            assert_eq!(q.single(app.world()).unwrap().lands, Cue::Win);
        }

        #[test]
        fn a_ring_leaves_its_disc_from_the_hand_where_it_stood_as_it_is_picked_up() {
            let mut app =
                with_rings_look(&[("a1", &[("ring", "white")]), ("c3", &[("ring", "black")])]);
            let step = |to: &str| {
                let mut o = option(&[("pos", "a1"), ("pos", to)], Some("a1"));
                o.to = Some(to.into());
                o
            };
            app.world_mut().resource_mut::<WatchStatus>().turn =
                Some(tak_turn(vec![step("a3"), step("b1")]));
            app.update();
            // The disc is in hand, under the pointer, before the ring is picked up.
            let mut q = app.world_mut().query_filtered::<&FromBowl, With<Held>>();
            let held = q.single(app.world()).expect("a disc held");
            assert_eq!((held.piece.as_str(), held.cell.as_deref()), ("disc", None));
            // Over the ring it may move, the disc it would leave, see-through in the ring; over
            // the other seat's, none.
            let ghosts = |app: &mut App| {
                let mut q = app.world_mut().query_filtered::<&Name, With<Ghost>>();
                q.iter(app.world())
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
            };
            app.world_mut().resource_mut::<Hovering>().0 = Some("a1".into());
            app.update();
            assert_eq!(ghosts(&mut app), ["disc to leave on a1"]);
            app.world_mut().resource_mut::<Hovering>().0 = Some("c3".into());
            app.update();
            assert!(ghosts(&mut app).is_empty());
            app.world_mut().resource_mut::<Hovering>().0 = None;
            // The ring picked up: the disc is put where it stood, the ring held instead.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn.as_mut().unwrap().gesture(&Gesture::pos("a1"));
            app.update();
            let mut q = app.world_mut().query::<(&FromBowl, &Carry, Has<Held>)>();
            let (left, carry, held) = q.single(app.world()).unwrap();
            assert_eq!(
                (left.piece.as_str(), left.cell.as_deref()),
                ("disc", Some("a1"))
            );
            assert!(matches!(carry, Carry::Put(_)) && !held);
            let mut q = app.world_mut().query_filtered::<&PieceAt, With<Held>>();
            let ring = q.single(app.world()).expect("the ring under the pointer");
            assert_eq!(
                (ring.cell.as_str(), ring.piece.piece.as_str()),
                ("a1", "ring")
            );
            // Put down on a3: carried there, the disc staying on a1.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let outcome = watch.turn.as_mut().unwrap().gesture(&Gesture::pos("a3"));
            assert_eq!(outcome, crate::play::Outcome::Ask);
            app.update();
            let mut q = app.world_mut().query::<(&PieceAt, &Carry, Has<Held>)>();
            let (_, carry, held) = q
                .iter(app.world())
                .find(|(at, ..)| at.piece.piece == "ring" && at.cell == "a1")
                .expect("the ring");
            assert!(matches!(carry, Carry::Put(to) if to.translation.distance(
                Layout::new(&grid_table("t-rings", &[]).info.board.positions).centres["a3"]
                    .with_y(to.translation.y)
            ) < 1e-5));
            assert!(!held);
            let mut q = app.world_mut().query::<&FromBowl>();
            assert_eq!(q.single(app.world()).unwrap().cell.as_deref(), Some("a1"));
            // Back to the start: nothing left on a1, the disc in hand again.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let turn = watch.turn.as_mut().unwrap();
            turn.back();
            turn.back();
            app.update();
            let mut q = app.world_mut().query::<(&FromBowl, Has<Held>)>();
            let (f, held) = q.single(app.world()).unwrap();
            assert!(f.cell.is_none() && held);
        }

        #[test]
        fn the_line_pointed_at_is_lit_whole_and_only_it() {
            let discs: Vec<(String, [(&str, &str); 1])> = ["a1", "a2", "a3", "b1", "b2", "b3"]
                .iter()
                .map(|c| (c.to_string(), [("disc", "white")]))
                .collect();
            let cells: Vec<(&str, &[(&str, &str)])> =
                discs.iter().map(|(c, d)| (c.as_str(), &d[..])).collect();
            let mut app = with_rings_look(&cells);
            let line = |col: &str| crate::net::api::LegalSelection {
                notation: format!("row:{col}1-{col}3"),
                captures: (1..=3).map(|i| format!("{col}{i}")).collect(),
                ..option(
                    &[("pos", &format!("{col}1")), ("pos", &format!("{col}3"))],
                    None,
                )
            };
            app.world_mut().resource_mut::<WatchStatus>().turn =
                Some(tak_turn(vec![line("a"), line("b")]));
            app.update();
            let lit = |app: &mut App| {
                let mut q = app.world_mut().query_filtered::<&Cell, With<Lit>>();
                let mut out: Vec<String> = q.iter(app.world()).map(|c| c.id.clone()).collect();
                out.sort();
                out
            };
            assert!(lit(&mut app).is_empty(), "nothing pointed at, nothing lit");
            let centre =
                Layout::new(&grid_table("t-rings", &[]).info.board.positions).centres["b2"]
                    .with_y(BOARD_HEIGHT);
            app.world_mut().resource_mut::<Hovering>().0 = Some("b2".into());
            app.world_mut().resource_mut::<PointerOnBoard>().0 = Some((centre, centre + Vec3::Y));
            app.update();
            app.update();
            assert_eq!(
                app.world()
                    .resource::<LineAim>()
                    .0
                    .as_ref()
                    .map(|o| o.notation.as_str()),
                Some("row:b1-b3")
            );
            assert_eq!(lit(&mut app), ["b1", "b2", "b3"]);
            // Over a disc no line takes, nothing.
            app.world_mut().resource_mut::<Hovering>().0 = Some("c2".into());
            app.update();
            app.update();
            assert_eq!(app.world().resource::<LineAim>().0, None);
            assert!(lit(&mut app).is_empty());
        }

        #[test]
        fn a_disc_the_move_being_built_turns_over_is_shown_turned_until_it_is_taken_back() {
            let mut app = with_rings_look(&[
                ("a1", &[("ring", "white")]),
                ("a2", &[("disc", "black")]),
                ("c3", &[("ring", "black")]),
            ]);
            let mut jump = option(&[("pos", "a1"), ("pos", "a3")], Some("a1"));
            jump.to = Some("a3".into());
            let mut turn = tak_turn(vec![jump]);
            turn.gesture(&Gesture::pos("a1"));
            assert_eq!(turn.gesture(&Gesture::pos("a3")), crate::play::Outcome::Ask);
            // The server's preview: the disc jumped is white's now.
            let mut preview = stacks(
                2,
                &[
                    ("a1", &[("disc", "white")]),
                    ("a2", &[("disc", "white")]),
                    ("a3", &[("ring", "white")]),
                    ("c3", &[("ring", "black")]),
                ],
            )
            .state;
            preview.ext = rings_removed(0, 0);
            turn.answered(crate::net::api::Selections {
                seat: 1,
                ply: 4,
                made: vec!["m".into()],
                options: vec![],
                complete: true,
                may_end: false,
                state: Some(preview),
            });
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
            app.update();
            let colour_at = |app: &mut App, cell: &str| {
                let mut q = app.world_mut().query::<(&PieceAt, Has<Flipping>)>();
                q.iter(app.world())
                    .find(|(at, _)| at.cell == cell)
                    .map(|(at, flipping)| (at.piece.color.clone(), flipping))
                    .unwrap()
            };
            assert_eq!(
                colour_at(&mut app, "a2"),
                ("white".into(), true),
                "turned over"
            );
            assert_eq!(colour_at(&mut app, "c3").0, "black", "the rest as they are");
            // Taken back: turned back.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            watch.turn.as_mut().unwrap().back();
            app.update();
            assert_eq!(colour_at(&mut app, "a2"), ("black".into(), true));
        }

        #[test]
        fn a_disc_turned_over_and_then_taken_stays_turned_and_see_through() {
            let mut app = with_rings_look(&[
                ("a1", &[("ring", "white")]),
                ("a2", &[("disc", "black")]),
                ("c3", &[("ring", "black")]),
            ]);
            let mut jump = option(&[("pos", "a1"), ("pos", "a3")], Some("a1"));
            jump.to = Some("a3".into());
            let mut turn = tak_turn(vec![jump]);
            turn.gesture(&Gesture::pos("a1"));
            turn.gesture(&Gesture::pos("a3"));
            let preview = |cells: &[(&str, &[(&str, &str)])]| {
                let mut p = stacks(2, cells).state;
                p.ext = rings_removed(0, 0);
                p
            };
            // The jump turns a2 white, and the move goes on: the line a2 is in is removed.
            let mut line = option(&[("pos", "a2")], None);
            line.captures = vec!["a2".into()];
            let answer = |made: &[&str], options, complete, state| crate::net::api::Selections {
                seat: 1,
                ply: 4,
                made: made.iter().map(|m| m.to_string()).collect(),
                options,
                complete,
                may_end: false,
                state,
            };
            turn.answered(answer(
                &["m"],
                vec![line],
                false,
                Some(preview(&[
                    ("a1", &[("disc", "white")]),
                    ("a2", &[("disc", "white")]),
                    ("a3", &[("ring", "white")]),
                ])),
            ));
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
            app.update();
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let turn = watch.turn.as_mut().unwrap();
            assert_eq!(turn.gesture(&Gesture::pos("a2")), crate::play::Outcome::Ask);
            app.update();
            // The move is whole: the server previews nothing more. The disc on a2 is still
            // shown white, as the last preview to have it had it, and see-through, as it goes.
            let mut watch = app.world_mut().resource_mut::<WatchStatus>();
            let turn = watch.turn.as_mut().unwrap();
            turn.answered(answer(&["m", "m"], vec![], true, None));
            app.update();
            app.update();
            let mut kits = app.world_mut().query::<&Kit>();
            let ghost = kits.single(app.world()).unwrap().pieces["disc"]
                .sides
                .as_ref()
                .unwrap()
                .ghost
                .clone();
            let mut q = app
                .world_mut()
                .query::<(&PieceAt, &MeshMaterial3d<StandardMaterial>, Has<Taken>)>();
            let (at, material, taken) = q
                .iter(app.world())
                .find(|(at, ..)| at.cell == "a2")
                .unwrap();
            assert_eq!(at.piece.color, "white");
            assert!(taken);
            assert!(ghost.contains(&material.0), "dressed see-through");
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
            pieces: vec![],
            counters: vec![],
            opening: None,
            reserves: Default::default(),
            cards: None,
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
        // The surround goal is not known; the other, every seat's, marks nothing.
        assert!(strips.is_empty(), "{strips:?}");
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
    fn the_lines_round_the_outside_are_as_thick_as_those_between_cells() {
        let pitch = PITCH;
        for shape in [CellShape::Square, CellShape::Hexagon] {
            // How far a cell's corners reach from its middle along x, at a scale.
            let reach = |scale: f32| {
                slab::cell_corners(shape, pitch * scale)
                    .iter()
                    .fold(0.0f32, |m, c| m.max(c.x))
            };
            // Across a flat side, along x for both (a hexagon's points are to the far and near
            // sides): neighbours there are a pitch apart.
            assert!((reach(1.0) - pitch / 2.0).abs() < 1e-7);
            let between = pitch - 2.0 * reach(CELL_SCALE);
            let outside = reach(LINE_SCALE) - reach(CELL_SCALE);
            assert!(
                (between - outside).abs() < 1e-7,
                "{shape:?}: {between} {outside}"
            );
            assert!(between > 0.0);
        }
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
