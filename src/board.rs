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
use bevy::prelude::*;

use crate::appearance::{Appearance, Appearances, PieceLook, PlayOn, Stands};
use crate::assets;
use crate::hand::{Carry, HandPlugin, Hands, Held, LIFT, Rest, Route};
use crate::look::{self, MaterialSpec, seat_colour};
use crate::net::WatchStatus;
use crate::net::api::{Board, BoardPosition, GameInfo, Gesture, LegalSelection, PieceOnBoard};
use crate::play::{Choice, Hovering, Spot, Stage, TakeUp, Turn};
use crate::slab::{self, Rim};
use crate::sounds::Cue;
use crate::table::Table;
use crate::trays;

pub struct BoardPlugin;

impl Plugin for BoardPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(HandPlugin)
            .configure_sets(
                Update,
                Hands.after(show_turn).after(bowl_stones).after(show_fences),
            )
            .add_message::<BoardBuilt>()
            .add_systems(
                Update,
                (
                    sync_board.run_if(resource_changed::<WatchStatus>),
                    show_turn.run_if(
                        resource_changed::<WatchStatus>
                            .or_eager(resource_changed::<Hovering>)
                            .or_eager(resource_changed::<ShowLegalMoves>),
                    ),
                    bowl_stones.run_if(resource_changed::<WatchStatus>),
                    show_supply.run_if(resource_changed::<WatchStatus>),
                    show_fences.run_if(resource_changed::<WatchStatus>),
                    drop_pieces,
                    change_wood.run_if(resource_changed::<BoardWood>),
                )
                    .chain(),
            )
            .init_resource::<BoardWood>()
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

/// A cell of the board, which is lit while a click on it would be taken, and otherwise its own
/// material.
#[derive(Component)]
struct Cell {
    id: String,
    rest: Handle<StandardMaterial>,
    lit: Handle<StandardMaterial>,
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
}

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
    /// Where a stone taken from each colour's bowl waits, by colour word.
    bowls: HashMap<String, Vec3>,
    /// Each seat's supply beside the board, by its colour word, and each kind of piece's pool;
    /// how far the supply reaches to either side, for the camera.
    supply: HashMap<String, Vec<SupplyPool>>,
    pools: HashMap<String, String>,
    beside: f32,
    /// How walls between positions are drawn and put down, for a game whose look has fences.
    fences: Option<FenceKit>,
    /// Each kind of piece's colour word, for a game whose pieces have their own (Blooms').
    piece_colour: HashMap<String, String>,
    /// Each colour word's see-through material.
    ghosts: HashMap<String, Handle<StandardMaterial>>,
    /// Each seat's tray, in seat order, for a game that counts what each captures (none
    /// otherwise); the pool the count is kept in; how big the pieces in them are.
    trays: Vec<Tray>,
    counter: String,
    tray_scale: f32,
    /// How far towards the player the first seat's tray reaches, if there are trays.
    front: f32,
}

/// Fences: one's mesh and material, where its foot is below its middle (metres at the full
/// pitch), where one taken from the bowl waits (over its rim), the side of its square each
/// fence-building choice puts it on (a step to the neighbour beyond), and a side a click would
/// put one on, lit.
struct FenceKit {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    foot: f32,
    over: Vec3,
    sides: HashMap<String, Vec2>,
    side_mesh: Handle<Mesh>,
    side_lit: Handle<StandardMaterial>,
}

/// A wall the position has, drawn as a fence, by its name (`fences::key`).
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct WallAt(String);

/// A fence taken from the bowl: waiting over it, or (once a side is chosen) put on that side,
/// named as the wall it makes.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct HeldFence(Option<String>);

/// A side of a square a click would put the fence taken on.
#[derive(Component)]
struct SideTarget;

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
        let ghost = own.as_ref().map(|spec| {
            let mut m = spec.on_board(self.wood).build(self.server);
            m.base_color = m.base_color.with_alpha(0.5);
            m.alpha_mode = AlphaMode::Blend;
            self.materials.add(m)
        });
        PieceKit {
            ghost,
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
        // Framed with the player's tray in front of it, if it has one.
        let extra = (kit.1.front - layout.size.y / 2.0).max(0.0);
        built.write(BoardBuilt {
            size: Vec2::new(layout.size.x.max(2.0 * kit.1.beside), layout.size.y + extra),
            middle: Vec2::Y * extra / 2.0,
        });
        // A board just built shows the position as it is, without dropping every piece.
        let root = kit.0;
        place_pieces(&mut commands, root, &kit.1, &layout, table, &[], None, &[]);
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
    let wanted = wanted_pieces(table, kit);
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
        table,
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

/// The pieces the position has and the scene does not are added, and those the scene has and the
/// position does not are taken away, but those `kept` (gone to a tray). With `animate`, those
/// added drop in, but on the positions it names, where the move being built showed them already.
#[allow(clippy::too_many_arguments)]
fn place_pieces(
    commands: &mut Commands,
    root: Entity,
    kit: &Kit,
    layout: &Layout,
    table: &Table,
    shown: &[(Entity, &PieceAt)],
    animate: Option<&[&str]>,
    kept: &[Entity],
) {
    let wanted = wanted_pieces(table, kit);
    for (entity, at) in shown {
        if !wanted.contains(at) && !kept.contains(entity) {
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
        .map(|t| table.state.state.count(&t.seat, &kit.counter))
        .collect();
    let colours: Vec<Vec<String>> = kit.trays.iter().map(|t| t.colours.clone()).collect();
    let gone_colours: Vec<&str> = gone
        .iter()
        .map(|(_, at)| kit.colour_of(&at.piece.piece, &at.piece.color))
        .collect();
    let mut taken = Vec::new();
    for (seat, change) in trays::fill(&had, &counts, &colours, &gone_colours)
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
                                lands: Cue::Capture,
                            },
                            Carry::Put(to),
                        ));
                    taken.push(entity);
                }
                trays::Source::Unseen(colour) => {
                    let id = kit.kind_in(&colour);
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
    }
}

/// Every piece of the position, bottom of each stack first.
fn wanted_pieces(table: &Table, kit: &Kit) -> Vec<PieceAt> {
    let mut out = Vec::new();
    for (cell, stack) in &table.state.state.cells {
        let mut under = 0.0;
        for (layer, piece) in stack.iter().enumerate() {
            out.push(PieceAt {
                cell: cell.clone(),
                layer,
                piece: piece.clone(),
                under: (under * 1e6_f32).round() as u32,
            });
            under += kit.piece(&piece.piece).height;
        }
    }
    out
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
            let target = tools.material(&look::lit_cell());
            let cell_mesh = shape.mesh(pitch, CELL_SCALE, CELL_HEIGHT);
            // A chessboard's two materials by turns, if the game's look has them, each lit as
            // the board is.
            let squares = tools
                .look
                .and_then(|l| l.board.squares.clone())
                .map(|names| {
                    let look = tools.look.expect("a look has the names");
                    let specs = names.map(|n| look.materials.get(&n).cloned());
                    specs.map(|spec| {
                        let spec = spec?;
                        let lit = look::lit_square(&spec);
                        Some((tools.material(&spec), tools.material(&lit)))
                    })
                });
            let turns = squares
                .as_ref()
                .map(|_| by_turns(&table.info.board.positions))
                .unwrap_or_default();
            for (id, centre) in &layout.centres {
                let at = *centre + Vec3::Y * BOARD_HEIGHT;
                let (rest, lit) = squares
                    .as_ref()
                    .and_then(|s| s[turns.get(id).copied().unwrap_or(0)].clone())
                    .unwrap_or_else(|| (wood.clone(), target.clone()));
                commands.spawn((
                    Name::new(format!("Cell {id}")),
                    Mesh3d(tools.meshes.add(look::world_uvs(cell_mesh.clone(), at))),
                    MeshMaterial3d(rest.clone()),
                    Transform::from_translation(at),
                    Cell {
                        id: id.clone(),
                        rest,
                        lit,
                    },
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
                    Cell {
                        id: id.clone(),
                        rest: unseen.clone(),
                        lit: glowing.clone(),
                    },
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
    // capture its win wants.
    let counter = table
        .info
        .counters
        .iter()
        .find(|c| c.max > 0 && c.max <= trays::MOST);
    let tray_size = tools.look.and_then(|l| l.trays.size).unwrap_or(trays::SIZE);
    let mut seat_trays = Vec::new();
    let mut front = 0.0;
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
        && let Some(said) = tools.look.and_then(|l| l.supply.as_ref())
        && let Some(piece) = table.info.pieces.first()
    {
        // Room for what each seat has left and what it has on the board, the most of any.
        let cells = &table.state.state.cells;
        let start = table
            .info
            .seats
            .iter()
            .map(|s| {
                let left = table
                    .state
                    .state
                    .view_counter(s.seat, &said.counter)
                    .unwrap_or(0);
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
                    id: said.counter.clone(),
                    piece: piece.id.clone(),
                    start,
                },
                Some(said.counter.clone()),
                Gesture {
                    kind: "action".into(),
                    value: said.take.clone(),
                },
            ));
        }
    }
    if !kinds.is_empty() {
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
                    crate::supply::Stance::of(low, high, pitch / PITCH),
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
        let at = crate::bowls::places(&[2], &edge, radius)[0][0].0;
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
        let side_lit = tools.materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.78, 0.36),
            emissive: LinearRgba::rgb(0.9, 0.55, 0.12),
            ..default()
        });
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
            side_mesh: tools
                .meshes
                .add(Plane3d::new(Vec3::Y, Vec2::new(pitch * 0.42, pitch * 0.09))),
            side_lit,
        }
    });
    let pools: HashMap<String, String> = table
        .info
        .pieces
        .iter()
        .filter(|p| !p.pool.is_empty())
        .map(|p| (p.id.clone(), p.pool.clone()))
        .collect();
    let kit = Kit {
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
        bowls: bowls_at,
        piece_colour,
        ghosts,
        trays: seat_trays,
        counter: counter.map(|c| c.id.clone()).unwrap_or_default(),
        tray_scale: pitch / PITCH * tray_size,
        front,
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
fn show_turn(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    (hovering, legal): (Res<Hovering>, Res<ShowLegalMoves>),
    roots: Query<(Entity, &Kit), With<BoardRoot>>,
    mut cells: Query<(&Cell, &mut MeshMaterial3d<StandardMaterial>)>,
    ghosts: Query<Entity, With<Ghost>>,
    mut pieces: Query<(Entity, &PieceAt, &mut Spot, Option<&Carry>)>,
    held: Query<Entity, With<Held>>,
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
    for (cell, mut material) in &mut cells {
        let wanted = if lit && targets.contains(&cell.id) {
            &cell.lit
        } else {
            &cell.rest
        };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }

    let layout = Layout::new(&table.info.board.positions);
    let cells = &table.state.state.cells;
    // The position as the move being built leaves it, so what it puts down stands on what will be
    // there: each piece it moves taken off its space, and on where it is put.
    let mut after = cells.clone();
    let mut carried: HashMap<(String, usize), (Carry, String)> = HashMap::new();
    for c in turn.map(Turn::carried).unwrap_or_default() {
        let Some(piece) = after.get_mut(c.from).and_then(Vec::pop) else {
            continue;
        };
        let layer = after[c.from].len();
        let (carry, at) = match c.to.and_then(|to| Some((to, layout.centres.get(to)?))) {
            Some((to, centre)) => {
                let stack = after.entry(to.to_string()).or_default();
                let put = kit.set_down(
                    &piece.piece,
                    &piece.color,
                    *centre,
                    (to, stack.len()),
                    kit.stack_height(stack.iter()),
                );
                stack.push(piece);
                (Carry::Put(put), to)
            }
            None => {
                after.get_mut(c.from).expect("taken from it").push(piece);
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
        carried.insert((c.from.to_string(), layer), (carry, at.to_string()));
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
                    kit.stack_height(stack.iter()),
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
                        kit.stack_height(stack) + *above,
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
    for (entity, at, mut spot, now) in &mut pieces {
        match carried.get(&(at.cell.clone(), at.layer)) {
            Some((carry, cell)) => {
                // Only when it changes: a piece put down is carried there once.
                if now != Some(carry) {
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
    // What the move aims, held under the pointer while it is next.
    let aiming = turn.is_some_and(Turn::aiming);
    match (aiming, held.iter().next()) {
        (true, None) => {
            let id = kit.aimed.as_deref().unwrap_or_default();
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
        (false, Some(entity)) => commands.entity(entity).despawn(),
        _ => {}
    }

    let Some(turn) = turn else { return };
    let mut shown: Vec<&str> = match turn.stage {
        Stage::Asking | Stage::Ready | Stage::Sending => turn.placed().collect(),
        Stage::Choosing | Stage::Failed => Vec::new(),
    };
    // Under the pointer too, where a click would put something (not where it would pick one up).
    if let Some(cell) = hovering.0.as_deref().filter(|c| targets.contains(*c))
        && turn.arriving(cell).is_some()
    {
        shown.push(cell);
    }
    // What is placed from a bowl is carried there solid (`bowl_stones`), not shown see-through.
    let from_bowls: Vec<&str> = turn
        .placings()
        .filter(|s| kit.source_of_placing(s, colour).is_some())
        .filter_map(|s| s.to.as_deref())
        .collect();
    shown.retain(|cell| !from_bowls.contains(cell));
    for cell in shown {
        let Some(centre) = layout.centres.get(cell) else {
            continue;
        };
        // The kind of piece that would stand there: the one the selections name, the one that
        // would come from where they say, or what the move aims.
        let coming = turn.arriving(cell).unwrap_or_default();
        let id = if coming.aimed {
            kit.aimed.as_deref()
        } else {
            coming
                .piece
                .or_else(|| Some(cells.get(coming.from?)?.last()?.piece.as_str()))
        }
        .unwrap_or_default();
        let stack = after.get(cell).map_or(&[][..], Vec::as_slice);
        let at = kit.set_down(
            id,
            colour,
            *centre,
            (cell, stack.len()),
            kit.stack_height(stack),
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

/// A piece taken from a bowl or a seat's supply (its source, `Kit::source_of`): waiting over
/// it, or carried to the cell it is placed on.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct FromBowl {
    source: String,
    piece: String,
    cell: Option<String>,
}

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
        let Some((source, id)) = kit.source_of_placing(s, yours) else {
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
            kit.stack_height(stack),
        );
        let placed = FromBowl {
            source: source.clone(),
            piece: id.to_string(),
            cell: Some(cell.to_string()),
        };
        match take(&|f| f.cell.is_none() && f.source == source) {
            // The piece in hand goes on from where it waits, as the kind placed.
            Some((entity, held)) => {
                let mut e = commands.entity(entity);
                if held.piece != id {
                    e.insert(looks(id));
                }
                e.insert((placed, Carry::Put(to)));
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
    if let Some((source, id)) = turn.in_hand_choice().and_then(|g| kit.source_for(g, yours)) {
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
                    piece.insert((Pickable::default(), Choice(pool.take.clone())));
                }
            }
        }
    }
}

/// The walls the position has, each a fence on the side its two squares share (one that has
/// just appeared drops in, but one the player put there); and the move being built's: a fence
/// taken from the bowl waits over it, the sides it may go on lit, and once one is chosen it is
/// carried there and set down.
/// The fences `show_fences` keeps: the position's walls, the fence in hand, the sides lit.
type FenceParts<'w, 's> = (
    Query<'w, 's, (Entity, &'static WallAt)>,
    Query<'w, 's, (Entity, &'static HeldFence)>,
    Query<'w, 's, Entity, With<SideTarget>>,
);

fn show_fences(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    roots: Query<(Entity, &Kit), With<BoardRoot>>,
    (walls, held, sides): FenceParts,
    mut room: Local<String>,
) {
    for side in &sides {
        commands.entity(side).despawn();
    }
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
            (Some((key, to)), None) if !kept => {
                commands
                    .entity(entity)
                    .insert((HeldFence(Some(key.clone())), Carry::Put(*to)));
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
        let spawn = |commands: &mut Commands, key: Option<String>, carry: Carry| {
            commands.spawn((
                Name::new("Fence in hand"),
                HeldFence(key),
                Mesh3d(fence.mesh.clone()),
                MeshMaterial3d(fence.material.clone()),
                waiting,
                Rest(waiting),
                carry,
                ChildOf(root),
            ));
        };
        match (&chosen, &taken) {
            (Some((key, to)), _) => spawn(&mut commands, Some(key.clone()), Carry::Put(*to)),
            (None, Some(_)) => spawn(
                &mut commands,
                None,
                Carry::Held {
                    lift: LIFT * kit.scale,
                    scale: kit.scale,
                },
            ),
            _ => {}
        }
    }

    // Where the fence taken may go: the sides of the square the piece ends on, lit.
    if let (Some(choices), Some(turn)) = (&taken, turn) {
        let cell = turn
            .gestures
            .iter()
            .rev()
            .find(|g| g.is_pos())
            .map(|g| g.value.as_str());
        for choice in choices {
            let Some((_, at)) = cell.and_then(|c| kit.fence_on(&layout, c, &choice.value)) else {
                continue;
            };
            commands.spawn((
                Name::new(format!("Side for {}", choice.value)),
                SideTarget,
                Mesh3d(fence.side_mesh.clone()),
                MeshMaterial3d(fence.side_lit.clone()),
                Transform::from_translation(at.translation.with_y(kit.surface + 0.0004))
                    .with_rotation(at.rotation),
                Pickable::default(),
                Choice((*choice).clone()),
                ChildOf(root),
            ));
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
            };
            let mut turn = Turn::new(1);
            turn.answered(Selections {
                seat: 1,
                ply: 3,
                made: vec![],
                options: vec![place("flat", "c3"), place("wall", "Sc3")],
                complete: false,
                may_end: false,
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
            };
            let mut turn = Turn::new(1);
            turn.answered(Selections {
                seat: 1,
                ply: 3,
                made: vec![],
                options: vec![shoot("a2", "a3"), shoot("a2", "a1"), shoot("b2", "c3")],
                complete: false,
                may_end: false,
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
                let mut q = app
                    .world_mut()
                    .query::<(&Cell, &MeshMaterial3d<StandardMaterial>)>();
                let mut out: Vec<String> = q
                    .iter(app.world())
                    .filter(|(c, m)| m.0 == c.lit)
                    .map(|(c, _)| c.id.clone())
                    .collect();
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
                    ..Selections {
                        seat: 1,
                        ply: 3,
                        made: vec![],
                        options: vec![],
                        complete: false,
                        may_end: false,
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
            let mut q = app.world_mut().query::<(&HeldFence, &Carry)>();
            let (held, carry) = q.single(app.world()).unwrap();
            assert_eq!(held.0, None);
            assert!(matches!(carry, Carry::Held { .. }));
            let mut q = app
                .world_mut()
                .query_filtered::<&Choice, With<SideTarget>>();
            let mut lit: Vec<String> = q.iter(app.world()).map(|c| c.0.value.clone()).collect();
            lit.sort();
            assert_eq!(lit, ["fence-e", "fence-n"]);
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
            let mut q = app.world_mut().query_filtered::<(), With<SideTarget>>();
            assert_eq!(
                q.iter(app.world()).count(),
                0,
                "nothing lit once it is down"
            );
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
            let mut q = app.world_mut().query::<&Cell>();
            let materials: std::collections::HashSet<_> =
                q.iter(app.world()).map(|c| c.rest.clone()).collect();
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
            pieces: vec![],
            counters: vec![],
            opening: None,
            reserves: Default::default(),
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
