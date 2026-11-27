//! Per-game appearance: how this client draws a game, beyond the generic defaults (`look.rs`).
//! Bingle's ADR 0124 leaves how things look to each client; these files are where this one keeps
//! a game's own look, as data (TOML), never code. A game without one is drawn the generic way.
//!
//! The files are `assets/appearance/*.toml`. Each names the games it is for by the server's
//! game id, and may give:
//! - `pieces.<piece id>`: the glTF `model` a piece is drawn as (one the game does not name is
//!   the generic stone), how it `stands` (`Stands`), and a `material` of its own, when it is not
//!   its seat's; `parts` of its model drawn in materials of their own, whoever's it is (a
//!   picture inlaid in a tile); or, for a piece of two sides (`sides.rs`), the material of each of its
//!   model's two parts (`sides`) and which side each seat's are up (`up`); and the piece a move
//!   of it `leaves` where it started (a disc dropped in a ring that then moves), shown while
//!   the move is built, since the server does not say;
//! - `seats.<color>.pieces` and `.goal`: the material a seat's pieces are made of, and that
//!   its goal's edges are stained; `.paint`, what the panels draw the color as (`colour` in a
//!   file of format 1, still read); `.parts`, the material of a part of its pieces' models, by
//!   the part's name, where one seat's differs from another's (a ring's trim, gold on one
//!   seat's and silver on the other's). A color no seat plays may have one too, for the pieces
//!   that are nobody's;
//! - `materials.<name>`: those materials (`look::MaterialSpec`);
//! - `board.on`: `points` for a game played where the board's lines cross, as Go is, rather
//!   than in its spaces (the default, `cells`); `table` for one played on the table itself, with
//!   no board, its positions unseen until lit. The web page's `style: points` and `style: free`
//!   are its own choice of the same; the API does not say it. `board.marks.<game id>`: positions marked on that
//!   game's board, as a Go board's star points are. `board.margin`: a wider margin of wood
//!   round the positions, in their spacing (room for a score kept on it);
//! - `bowls`: a bowl of its stones for each seat, beside the board: the bowl's model, the heap of
//!   stones in it, and the material the bowl is made of;
//! - `card.pictures.<game id>`: the picture on the game's card in the main screen's list (an
//!   image under `pictures/`); a game without one has a blank space there;
//! - `tints.<key>`: how a position the view tints with a key of the game's legend is drawn
//!   (`tints.rs`): a `piece` of this file laid on it (a tile), or a `stamp` of a material
//!   printed on it. A key without one is not drawn;
//! - `supply`: how what each seat has left to place lies beside the board (`lie`: on its
//!   `edge`, the default for a flat piece, or `flat`, its face up to be seen), and, for a game
//!   whose view counts it rather than its reserves, the view's `counter` and the choice that
//!   `take`s one (`supply.rs`);
//! - `score`: a seat's score as the view counts it (`counter`), kept on a row of dots on the
//!   board's margin at the seat's right hand, one for each point its win wants, each with a
//!   `piece` of the seat's own on it once that piece has left the board (`trays.rs`);
//! - `stock.counts`: the view's count of each kind of piece both seats have still to place (a
//!   draft's tiles), shown beside the board.
//! - `move_tiles`: the view's move cards, each a tile of wood beside the board with the squares
//!   a piece may go to stamped on a grid (`move_tiles.rs`): where they are in the view, the
//!   choice a click on one makes, and their wood and inks;
//! - `cards`: a card game's table, laid out here rather than by a vocabulary the client knows
//!   (`card_table.rs`): where each of its zones lies (`piles.<zone>.at`, in metres, the whole
//!   centred on the table by itself) and how its cards lie (`spread`), the choice a click on it
//!   makes (`click`), which choices are made together, several in any order, before the one
//!   that plays them (`together`: the start of their values), and the view's score sheet to
//!   show beside the game (`sheet`, by its name in the view). A game of several seats gives
//!   the zones each seat has (`piles.<zone>.seat`, on the seat's own terms), and is laid round
//!   the table, seen from the seat this client plays: a hand `held` (the player's fanned up
//!   to them, the others' standing in the air), a trick `played` (each card towards who
//!   played it), and the choice a click on a card in hand makes (`play`);
//! - `grid`: the view's board of choices, lines of boxes, as a board on the table
//!   (`choice_grid.rs`): where it lies, the picture of each box and line by the view's name
//!   for it, and the choice a click on a box makes;
//! - `markers`: a piece that stands for each seat, in front of it (`seat_markers.rs`): the
//!   piece by the seat's color word, the materials it is stained when the view puts seats
//!   on sides, and the choices clicks on them make (a team picked by its members);
//! - `panel.counters`: the view's counters of each seat shown under its name in the game's
//!   panel (a score, tricks taken).
//!
//! - `format`: the format of appearance file it is written in ([`FORMAT`]), which a look a
//!   server offers must say (`downloads.rs`): one written for a later format is left out.
//!
//! The private folder (plan, decision 2) adds files of its own, for the games of Bingle's
//! private overlay (ADR 0127), read here as these are; a game the server marks private is
//! shown only when one of them is for it (`Appearances::private_games`), or the server offers
//! a look for it. A look a server offers is read here too, once it is fetched and checked
//! (`Appearance::downloaded`).

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::Deserialize;

use crate::assets;
use crate::look::{self, MaterialSpec};

/// The format of appearance file this client reads. A file says the format it was written in
/// (`format`, 1 when it does not); one written for a later format is not read. Raise it when a
/// file this client reads would be read otherwise by an older one, and say what changed here:
/// - 1: the first (2026-11-26).
/// - 2: a seat's `colour` is its `paint` (2026-11-27, Bingle's ADR 0154). A file of format 1
///   that says `colour` is still read; a client that reads only 1 leaves a file of 2 out.
pub const FORMAT: u32 = 2;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    /// The format it is written in ([`FORMAT`]).
    pub format: Option<u32>,
    /// The games this is for, by the id the server gives them. A look a server offers is for
    /// the game it came with, whatever it says.
    #[serde(default)]
    pub games: Vec<String>,
    #[serde(default)]
    pub pieces: BTreeMap<String, PieceLook>,
    #[serde(default)]
    pub seats: BTreeMap<String, SeatLook>,
    #[serde(default)]
    pub materials: BTreeMap<String, MaterialSpec>,
    #[serde(default)]
    pub board: BoardLook,
    #[serde(default)]
    pub card: CardLook,
    /// A bowl of its stones for each seat, beside the board.
    pub bowls: Option<BowlLook>,
    /// The trays a seat keeps what it captures in (`trays.rs`), when the game counts that.
    #[serde(default)]
    pub trays: TrayLook,
    /// A seat's score kept on the board's margin with its own pieces, one on each of a row of
    /// dots, when the view counts it (`trays.rs`).
    pub score: Option<ScoreLook>,
    /// How the pieces a seat has left to place lie, and where they are counted when the view
    /// counts them rather than the reserves (`supply.rs`).
    pub supply: Option<SupplyLook>,
    /// How the walls the view says stand between positions are drawn, and placed (`fences.rs`).
    pub fences: Option<FenceLook>,
    /// The kind of piece what follows a move of two parts is shown as while the move is built
    /// (an arrow shot from where a piece landed), by its id in `pieces`. The server says only
    /// where a move aims, not that it puts anything there; without this nothing is shown there
    /// (a capture aimed at a piece where it stands).
    pub aimed: Option<String>,
    #[serde(default)]
    pub sounds: SoundLook,
    /// How the positions the view tints are drawn, by the legend's key (`tints.rs`).
    #[serde(default)]
    pub tints: BTreeMap<String, TintLook>,
    /// What both seats have still to place, counted in the view, beside the board.
    pub stock: Option<StockLook>,
    /// A card game's table, laid out as this file says (`card_table.rs`).
    pub cards: Option<CardsLook>,
    /// The view's move cards, drawn as tiles beside the board (`move_tiles.rs`).
    pub move_tiles: Option<MoveTilesLook>,
    /// The view's board of choices, as a board on the table (`choice_grid.rs`).
    pub grid: Option<GridLook>,
    /// A piece that stands for each seat, in front of it (`seat_markers.rs`).
    pub markers: Option<MarkersLook>,
    /// What the game's panel shows of each seat.
    #[serde(default)]
    pub panel: PanelLook,
    /// Read from the private folder (`assets::read_private`): its games are shown when the
    /// server marks them private.
    #[serde(skip)]
    pub private: bool,
    /// Offered by the server, fetched and checked (`downloads.rs`).
    #[serde(skip)]
    pub downloaded: bool,
}

/// The view's board of choices (`ext.view.<view>`: `lines`, each with an `id`, an `icon` and
/// its `boxes`, each with an `id`, an `icon`, a `state` (`open`, `crossed`, `covered` for one
/// out of the game, or `chosen`, the one left) and the seat it was crossed `by`), as a board
/// lying on the table: a tile for each box with its picture on it, a line's own picture at its
/// head, and on a box a seat crossed that seat's marker, small (`markers`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridLook {
    pub view: String,
    /// The choice a click on an open box makes: this, the line's id, `-`, the box's id.
    pub choice: String,
    /// Its middle on the table (x across, z towards the player's side), in metres.
    pub at: [f32; 2],
    /// A box's side, in metres (34 mm when not said).
    #[serde(rename = "box")]
    pub box_size: Option<f32>,
    /// The picture of a box or a line, by the view's name for it (its `icon`): a file of the
    /// client's, square, covering a tile. One with none is a blank tile.
    #[serde(default)]
    pub pictures: BTreeMap<String, String>,
    /// The picture on a box that is out of the game.
    pub covered: Option<String>,
    /// What the board is made of, and what rims the box left on a line: materials of this
    /// file's. The board's wood, and brass, without.
    pub material: Option<String>,
    pub rim: Option<String>,
}

/// A piece that stands for each seat, in front of it, and wherever else the seat is marked (on
/// a box of the grid it crossed, small).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkersLook {
    /// The piece of each seat, by the seat's color word: its id in `pieces`.
    pub pieces: BTreeMap<String, String>,
    /// Where a seat's stands, on the seat's own terms: to its right, and from the table's
    /// middle towards it, in metres.
    pub seat: [f32; 2],
    /// How big the one in front of a seat is against its model (1 when not said), and how
    /// big a small one (0.4 when not said).
    pub size: Option<f32>,
    pub small: Option<f32>,
    /// What a marker's first part is made of: a material of this file's.
    pub material: String,
    /// The parts of its model drawn in its seat's own material (`seats.<word>.pieces`, or the
    /// seat's color as a lacquer): a plinth, and what else tells whose it is from across the
    /// table (its eyes). One name, or several.
    #[serde(default, alias = "seat_part", deserialize_with = "one_or_several")]
    pub seat_parts: Vec<String>,
    /// The view's list of the seats on one side (`ext.view.<sides>`), once there are sides:
    /// the markers of those seats are made of `on`, the others' of `off`, in place of
    /// `material`, so partners are seen to be.
    pub sides: Option<String>,
    pub on: Option<String>,
    pub off: Option<String>,
    /// The seat the view names (`ext.view.<ringed>`, a seat's number) has a ring round its
    /// marker's foot, of this material (`ring`).
    pub ringed: Option<String>,
    pub ring: Option<String>,
    /// The choices clicks on the markers make: the start of their values, which go on with
    /// the seats they name, joined by `-` (`team-2-3`). A click on a seat's marker chooses
    /// it, or lets it go; the choice is made once no other names more seats than those
    /// chosen, or by a click on the player's own marker.
    #[serde(default)]
    pub choose: Vec<String>,
    /// The choice that names no seat, made by a click on the player's own marker with none
    /// chosen.
    pub alone: Option<String>,
}

/// A name, or a list of them.
fn one_or_several<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Names {
        One(String),
        Several(Vec<String>),
    }
    Ok(match Names::deserialize(d)? {
        Names::One(name) => vec![name],
        Names::Several(names) => names,
    })
}

/// What the game's panel shows of each seat beyond its name: the view's counters of the seat
/// (`ext.view.seats.<seat>.counters`), by id, in this order, each with the view's label.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelLook {
    #[serde(default)]
    pub counters: Vec<String>,
}

/// A card game's table as this file lays it out: its piles, the choices clicks on them make,
/// and its score sheet.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardsLook {
    /// Choices made together, several in any order, before the one that plays them: the start
    /// of their values (a set of pins, then the ball that knocks them down). The server lists
    /// a move's in its own order; the client sends them so.
    pub together: Option<String>,
    /// The view's score sheet, by its name in the view: frames, each with its marks and, once
    /// scored, its total.
    pub sheet: Option<String>,
    /// The choice a click on a card the player sees makes: this, then the card's kind (a card
    /// of their hand played, discarded or passed, whichever the game is asking for).
    pub play: Option<String>,
    /// How far the cloth reaches from the table's middle, for a table laid round its seats,
    /// when that is farther than its piles (room for what else stands on it).
    pub radius: Option<f32>,
    /// Each zone's pile, by the zone's id.
    pub piles: BTreeMap<String, PileLook>,
}

impl CardsLook {
    /// Whether the table is laid round its seats: some pile is each seat's own.
    pub fn seated(&self) -> bool {
        self.piles.values().any(|p| p.seat.is_some())
    }

    /// The pile a zone of the position is drawn as: a seat's own (`hand:2`) by the zone's id.
    pub fn pile(&self, zone: &str) -> Option<&PileLook> {
        self.piles.get(zone.split(':').next().unwrap_or(zone))
    }
}

/// Where a zone's pile lies (its first card's middle, x and z in metres: any origin, the whole
/// is centred), how its cards lie, and the choice a click on it makes. Or, for a zone each
/// seat has one of (`seat`), where each lies on its seat's own terms: to the seat's right, and
/// from the table's middle towards it; the table is then laid round its seats, the middle the
/// origin of every `at`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PileLook {
    #[serde(default)]
    pub at: [f32; 2],
    pub seat: Option<[f32; 2]>,
    #[serde(default)]
    pub spread: crate::card_layout::Spread,
    pub click: Option<String>,
    /// Shown only while this other zone is empty (the trick before, until the next is led);
    /// a card it shows is not also drawn where else the position has it.
    pub unless: Option<String>,
}

/// How a position the view tints with one key is drawn: a piece of the file's laid on it (a
/// tile, nobody's), or a stamp of a material on it, `size` of the pitch across.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TintLook {
    pub piece: Option<String>,
    pub stamp: Option<String>,
    pub size: Option<f32>,
}

/// Move tiles: tiles that show, as squares stamped on a grid, where a piece may go, one for
/// each card of the view's (a game played with move cards), in front of the seat that holds it
/// and one at the side; a click on one makes its choice (`move_tiles.rs`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveTilesLook {
    /// The view's cards, by their name in it: `{ hands: { <seat>: [card] }, side: card }`, a
    /// card `{ id, name, moves: [[right, forward]], stamp }`: each move from where the piece
    /// stands, as its holder sees it, and the color word of the seat its stamp is.
    pub view: String,
    /// The choice a click on a tile makes: this, then the card's id.
    pub choice: String,
    /// What the tiles are made of, by a material of this file's; a pale wood without one.
    pub material: Option<String>,
    /// The ink the squares a piece may go to are stamped in, and the ink of the square it
    /// stands on; a red and a black ink without them.
    pub ink: Option<String>,
    pub piece_ink: Option<String>,
}

/// What both seats have still to place, shared (a draft's tiles): the view's map of each kind of
/// piece to how many are left (`ext.view.<counts>`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockLook {
    pub counts: String,
}

/// Sounds of its own for playing a seat, each a file of the client's, in place of every game's
/// (`sounds.rs`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoundLook {
    /// A piece picked up.
    pub lift: Option<String>,
    /// A piece set down where it goes, or one placed.
    pub put: Option<String>,
    /// The move sent.
    pub play: Option<String>,
    /// The game over, the player having won, or lost.
    pub win: Option<String>,
    pub lose: Option<String>,
    /// A piece captured, set down in its captor's tray.
    pub capture: Option<String>,
}

/// The bowls the seats keep their stones in (`bowls.rs`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BowlLook {
    /// The bowl, and the heap of stones in it: glTF files, the first mesh of each.
    pub model: String,
    pub stones: String,
    /// What the bowl is made of. The stones are their seat's.
    pub material: String,
    /// How big the bowl is against its model (1 when not said): smaller where a seat has a
    /// bowl for each of several colors.
    pub size: Option<f32>,
}

/// A seat's pieces left to place: how they lie beside the board, and, when the view counts
/// them (a game whose rules are written in Go), the counter that says how many and the choice
/// (an action's id) that places one, both or neither.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplyLook {
    pub counter: Option<String>,
    pub take: Option<String>,
    #[serde(default)]
    pub lie: Lie,
}

impl SupplyLook {
    /// The view's counter and the choice that takes one, when the view counts the pieces.
    pub fn counted(&self) -> Option<(&str, &str)> {
        Some((self.counter.as_deref()?, self.take.as_deref()?))
    }
}

/// How a flat piece lies in a seat's supply (`supply::Stance`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lie {
    /// On its edge, its faces along the line: a low wall, taking little room.
    #[default]
    Edge,
    /// Face up, as it lies on the board, so what is on its face is seen.
    Flat,
}

/// The walls the view says stand between positions, drawn as fences: the model and material of
/// one, the bowl both seats take them from (its model, the heap in it, what the bowl is made of,
/// its size against its model) and the side of a square each action that builds one puts it on,
/// as the step to the neighbour beyond (layout units, y towards the near side).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FenceLook {
    pub model: String,
    pub material: String,
    pub bowl: String,
    pub heap: String,
    pub bowl_material: String,
    pub size: Option<f32>,
    #[serde(default)]
    pub sides: BTreeMap<String, [f32; 2]>,
}

/// The trays in front of the seats, for a game that counts what each captures (`trays.rs`):
/// every such game has them; this says how they look.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrayLook {
    /// What a tray is made of: a material of this file's. The board's wood when not said.
    pub material: Option<String>,
    /// How big its pieces, and so its squares, are against the board's (`trays::SIZE` when not
    /// said).
    pub size: Option<f32>,
}

/// A seat's score as the view counts it, kept with pieces of its own that left the board: a
/// dot on the board's margin at the seat's right hand for each point its win wants (the
/// counter's `max`), and on each dot one of those pieces, as the count rises.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreLook {
    /// The view's counter of each seat that is its score.
    pub counter: String,
    /// The kind of piece, by its id in `pieces`, that goes on a dot: one of the seat's own
    /// that leaves the board as its score rises.
    pub piece: String,
}

/// How a game shows on its card in the list of games to start.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardLook {
    /// The card's picture, by game id: a file of the client's.
    #[serde(default)]
    pub pictures: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardLook {
    /// Where the pieces go: in the spaces, or on the points where the lines cross.
    #[serde(default)]
    pub on: PlayOn,
    /// Positions marked on the board with a dot, by game id (a file may be for several games,
    /// each its own size).
    #[serde(default)]
    pub marks: BTreeMap<String, Vec<String>>,
    /// The cells in two materials by turns, as a chessboard's: the first for the corner nearest
    /// the player on the left and every other cell from there, the second for the rest.
    pub squares: Option<[String; 2]>,
    /// A border inlaid round the cells, as a chessboard's, in this material.
    pub border: Option<String>,
    /// The cells of a region the board names (Fist's keep), each in a material of its own, by
    /// the region's name: over the squares' materials, or the board's wood.
    #[serde(default)]
    pub regions: BTreeMap<String, String>,
    /// How far the wood reaches beyond the outermost spaces or points, in the spacing of the
    /// positions, when it is wider than the usual (room for a score kept on it).
    pub margin: Option<f32>,
}

impl BoardLook {
    /// The positions marked on this game's board.
    pub fn marks(&self, game: &str) -> &[String] {
        self.marks.get(game).map_or(&[], Vec::as_slice)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayOn {
    #[default]
    Cells,
    Points,
    /// On the table itself: no board is drawn, and a position is seen only when it is lit.
    Table,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceLook {
    /// A glTF file; its first mesh is the piece. The client stands it by its foot, and what is
    /// on it in a stack by its height (`assets::model_span`).
    pub model: String,
    /// How it is set down.
    #[serde(default)]
    pub stands: Stands,
    /// What it is made of, whoever it belongs to; its seat's material without.
    pub material: Option<String>,
    /// For a piece of two sides (a disc light on one side and dark on the other, `sides.rs`):
    /// the material of each of its model's two parts, by the part's name. Not with `material`.
    #[serde(default)]
    pub sides: BTreeMap<String, String>,
    /// Which part's side is up on each seat's pieces, by the seat's color word; the model's
    /// first part's for a seat not named.
    #[serde(default)]
    pub up: BTreeMap<String, String>,
    /// Parts of its model after the first drawn in a material of their own, by the part's name:
    /// the same on every seat's piece (a picture inlaid in a tile), the first part being its
    /// seat's material. Not with `sides`. A part not named here is of its seat's material for
    /// that part (`SeatLook::parts`), or else of the piece's own.
    #[serde(default)]
    pub parts: BTreeMap<String, String>,
    /// The kind of piece, by its id in `pieces`, that a move of this one leaves where it
    /// started, in the mover's color (a disc dropped in a ring before the ring moves). The
    /// server says only where the move goes; this shows the rest while it is built.
    pub leaves: Option<String>,
}

/// How a piece is set down on its position.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stands {
    /// As a stone put down by hand: turned any way, leaning a degree or so, no two quite the
    /// same size.
    #[default]
    Loose,
    /// Upright, turned any way: a turned piece, which looks the same from every side.
    Upright,
    /// Upright and square to the board, as a block is set down by hand: a quarter turn any way,
    /// never quite true and never quite in the middle, so the blocks of a stack do not line up.
    Square,
    /// Upright, looking across the board from its seat's side: a piece with a face.
    Forward,
    /// True to the board, as a tile fitted to its space: never turned, never aside.
    Fitted,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatLook {
    /// The material of this seat's pieces.
    pub pieces: Option<String>,
    /// The material of the edges of this seat's goal.
    pub goal: Option<String>,
    /// What this client draws the color as wherever the interface shows the seat (the dot by
    /// its name), in sRGB: what its pieces look like, rather than what its color's name says.
    /// Format 1 called it `colour`.
    #[serde(alias = "colour")]
    pub paint: Option<[f32; 3]>,
    /// The material of a part of this seat's pieces' models after the first, by the part's
    /// name, on every piece whose model has such a part and whose look gives it nothing of its
    /// own (`PieceLook::parts`): a trim that is gold on one seat's pieces and silver on
    /// another's.
    #[serde(default)]
    pub parts: BTreeMap<String, String>,
}

/// What is wrong with a piece's parts of their own, if it has them: each must be one of its
/// model's parts after the first, of a material the file defines, and not on a piece of two
/// sides.
fn parts_problems(piece: &PieceLook, materials: &BTreeMap<String, MaterialSpec>) -> Vec<String> {
    let mut out = Vec::new();
    if piece.parts.is_empty() {
        return out;
    }
    if !piece.sides.is_empty() {
        out.push("parts and sides".into());
    }
    let parts = assets::model_parts(&piece.model);
    for (part, name) in &piece.parts {
        if !parts.iter().skip(1).any(|p| p == part) {
            out.push(format!(
                "part {part} is none of the model's after its first: {parts:?}"
            ));
        }
        if !materials.contains_key(name) {
            out.push(format!("no material {name}"));
        }
    }
    out
}

/// What is wrong with a piece's two sides, if it has them: they must be its model's two parts,
/// each of a material the file defines, and each side a seat has up one of them.
fn sides_problems(piece: &PieceLook, materials: &BTreeMap<String, MaterialSpec>) -> Vec<String> {
    let mut out = Vec::new();
    if piece.sides.is_empty() {
        if !piece.up.is_empty() {
            out.push("a side up, and no sides".into());
        }
        return out;
    }
    if piece.material.is_some() {
        out.push("a material and sides".into());
    }
    let mut parts = assets::model_parts(&piece.model);
    parts.sort();
    let named: Vec<&String> = piece.sides.keys().collect();
    if parts.len() != 2 || parts.iter().collect::<Vec<_>>() != named {
        out.push(format!(
            "sides {named:?}, but the model's parts are {parts:?}"
        ));
    }
    for name in piece.sides.values() {
        if !materials.contains_key(name) {
            out.push(format!("no material {name}"));
        }
    }
    for (word, side) in &piece.up {
        if !piece.sides.contains_key(side) {
            out.push(format!("seat {word}'s side up is no side: {side}"));
        }
    }
    out
}

/// Only the format an appearance file says it is written in: read before the rest, since a
/// file of a later format may say what this client cannot read.
#[derive(Deserialize)]
struct Format {
    format: Option<toml::Value>,
}

impl Appearance {
    pub fn parse(text: &str) -> Result<Self, String> {
        let said = toml::from_str::<Format>(text)
            .map_err(|e| e.to_string())?
            .format;
        if let Some(format) = said {
            match format.as_integer() {
                Some(n) if (1..=i64::from(FORMAT)).contains(&n) => {}
                Some(n) if n > i64::from(FORMAT) => {
                    return Err(format!(
                        "written for format {n} of the appearance file; this client reads {FORMAT}"
                    ));
                }
                _ => return Err(format!("format {format} is not a format")),
            }
        }
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// A look a server offers for this game: read from its appearance files (by their paths),
    /// for this game alone, with every file it names renamed by `file`, which gives where the
    /// client keeps one fetched (None for a file the look's folder does not have, or that was
    /// refused). It must say its format, and name only files it brought and the client's own
    /// (not its private folder's), and materials it has: a look that does not is left out, as
    /// a whole, and the game is drawn from the defaults.
    pub fn downloaded(
        game: &str,
        looks: &[(String, String)],
        file: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let (path, text) = Self::choose(game, looks)?;
        let mut look = Self::parse(text).map_err(|e| format!("{path}: {e}"))?;
        if look.format.is_none() {
            return Err(format!(
                "{path}: a look from a server says the format it is written in (format = {FORMAT})"
            ));
        }
        look.retarget(game);
        for name in look.files_mut() {
            *name = match file(name) {
                Some(kept) => kept,
                None if assets::FILES.iter().any(|(f, _)| f == name) => continue,
                // Nothing the client has: the look's problems say so.
                None => format!("{}missing/{name}", assets::DOWNLOADED),
            };
        }
        let problems = look.problems();
        if !problems.is_empty() {
            return Err(format!("{path}: {}", problems.join("; ")));
        }
        look.downloaded = true;
        Ok(look)
    }

    /// The path of the picture on this game's card that its look from a server names, if any.
    pub fn downloaded_picture(game: &str, looks: &[(String, String)]) -> Option<String> {
        let (_, text) = Self::choose(game, looks).ok()?;
        let mut look = Self::parse(text).ok()?;
        look.retarget(game);
        look.card.pictures.remove(game)
    }

    /// The appearance file of a server's look for this game: the one that names it, else the
    /// only one.
    fn choose<'a>(game: &str, looks: &'a [(String, String)]) -> Result<(&'a str, &'a str), String> {
        let named = looks.iter().find(|(_, text)| {
            toml::from_str::<Games>(text).is_ok_and(|g| g.games.iter().any(|g| g == game))
        });
        match (named, looks) {
            (Some((path, text)), _) | (None, [(path, text)]) => Ok((path, text)),
            (None, []) => Err("the server's look has no appearance file".into()),
            (None, _) => Err(
                "the server's look has several appearance files, and none names the game".into(),
            ),
        }
    }

    /// For this game alone: what is said of a game by its id is said of this one, when it is
    /// said of this one or of one game only (a pack's look need not know the id its game is
    /// given, with its version in it).
    fn retarget(&mut self, game: &str) {
        fn only<T>(map: &mut BTreeMap<String, T>, game: &str) {
            let mine = map.remove(game).or_else(|| match map.len() {
                1 => map.pop_first().map(|(_, v)| v),
                _ => None,
            });
            map.clear();
            if let Some(v) = mine {
                map.insert(game.to_string(), v);
            }
        }
        self.games = vec![game.to_string()];
        only(&mut self.card.pictures, game);
        only(&mut self.board.marks, game);
    }

    /// Every file it names: models, pictures, textures and sounds.
    pub fn files_mut(&mut self) -> Vec<&mut String> {
        let mut out: Vec<&mut String> = Vec::new();
        out.extend(self.pieces.values_mut().map(|p| &mut p.model));
        out.extend(self.card.pictures.values_mut());
        if let Some(b) = &mut self.bowls {
            out.extend([&mut b.model, &mut b.stones]);
        }
        if let Some(f) = &mut self.fences {
            out.extend([&mut f.model, &mut f.bowl, &mut f.heap]);
        }
        let s = &mut self.sounds;
        out.extend(
            [
                &mut s.lift,
                &mut s.put,
                &mut s.play,
                &mut s.win,
                &mut s.lose,
                &mut s.capture,
            ]
            .into_iter()
            .flatten(),
        );
        if let Some(g) = &mut self.grid {
            out.extend(g.pictures.values_mut());
            out.extend(&mut g.covered);
        }
        for m in self.materials.values_mut() {
            out.extend(
                [&mut m.albedo, &mut m.normal, &mut m.arm]
                    .into_iter()
                    .flatten(),
            );
        }
        out
    }

    /// What is wrong with it: names of materials it does not define, and files the client does
    /// not have.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(supply) = &self.supply
            && supply.counter.is_some() != supply.take.is_some()
        {
            out.push("supply: a counter and the choice that takes one, or neither".into());
        }
        for (word, seat) in &self.seats {
            for name in [&seat.pieces, &seat.goal]
                .into_iter()
                .flatten()
                .chain(seat.parts.values())
            {
                if !self.materials.contains_key(name) {
                    out.push(format!("seat {word}: no material {name}"));
                }
            }
            for part in seat.parts.keys() {
                let has = |piece: &PieceLook| {
                    piece.sides.is_empty()
                        && assets::model_parts(&piece.model)
                            .iter()
                            .skip(1)
                            .any(|p| p == part)
                };
                if !self.pieces.values().any(has) {
                    out.push(format!("seat {word}: no piece's model has a part {part}"));
                }
            }
        }
        for (name, material) in &self.materials {
            out.extend(
                material
                    .problems()
                    .into_iter()
                    .map(|p| format!("{name}: {p}")),
            );
        }
        for (id, piece) in &self.pieces {
            if !assets::exists(&piece.model) {
                out.push(format!("piece {id}: no model {}", piece.model));
            }
            if let Some(name) = &piece.material
                && !self.materials.contains_key(name)
            {
                out.push(format!("piece {id}: no material {name}"));
            }
            out.extend(
                sides_problems(piece, &self.materials)
                    .into_iter()
                    .chain(parts_problems(piece, &self.materials))
                    .map(|p| format!("piece {id}: {p}")),
            );
        }
        for (id, picture) in &self.card.pictures {
            if !assets::exists(picture) {
                out.push(format!("card of {id}: no picture {picture}"));
            }
            if !self.games.contains(id) {
                out.push(format!("card of {id}: not one of this file's games"));
            }
        }
        if let Some(bowls) = &self.bowls {
            for file in [&bowls.model, &bowls.stones] {
                if !assets::exists(file) {
                    out.push(format!("bowls: no model {file}"));
                }
            }
            if !self.materials.contains_key(&bowls.material) {
                out.push(format!("bowls: no material {}", bowls.material));
            }
        }
        for name in self
            .board
            .squares
            .iter()
            .flatten()
            .chain(&self.board.border)
            .chain(self.board.regions.values())
        {
            if !self.materials.contains_key(name) {
                out.push(format!("board: no material {name}"));
            }
        }
        if let Some(f) = &self.fences {
            for file in [&f.model, &f.bowl, &f.heap] {
                if !assets::exists(file) {
                    out.push(format!("fences: no model {file}"));
                }
            }
            for name in [&f.material, &f.bowl_material] {
                if !self.materials.contains_key(name) {
                    out.push(format!("fences: no material {name}"));
                }
            }
        }
        if let Some(name) = &self.trays.material
            && !self.materials.contains_key(name)
        {
            out.push(format!("trays: no material {name}"));
        }
        if self.trays.size.is_some_and(|s| !(0.2..=1.5).contains(&s)) {
            out.push("trays: a size from 0.2 to 1.5".into());
        }
        let sounds = &self.sounds;
        for file in [
            &sounds.lift,
            &sounds.put,
            &sounds.play,
            &sounds.win,
            &sounds.lose,
            &sounds.capture,
        ]
        .into_iter()
        .flatten()
        {
            if !assets::exists(file) {
                out.push(format!("sounds: no file {file}"));
            }
        }
        if let Some(id) = &self.aimed
            && !self.pieces.contains_key(id)
        {
            out.push(format!("aimed: no piece {id}"));
        }
        for (id, piece) in &self.pieces {
            if let Some(left) = &piece.leaves
                && (!self.pieces.contains_key(left) || left == id)
            {
                out.push(format!("piece {id}: leaves no other piece {left}"));
            }
        }
        if let Some(score) = &self.score
            && !self.pieces.contains_key(&score.piece)
        {
            out.push(format!("score: no piece {}", score.piece));
        }
        if self.board.margin.is_some_and(|m| !(0.5..=4.0).contains(&m)) {
            out.push("board: a margin from 0.5 to 4".into());
        }
        for (key, tint) in &self.tints {
            match (&tint.piece, &tint.stamp) {
                (Some(id), None) if !self.pieces.contains_key(id) => {
                    out.push(format!("tint {key}: no piece {id}"));
                }
                (None, Some(name)) if !self.materials.contains_key(name) => {
                    out.push(format!("tint {key}: no material {name}"));
                }
                (Some(_), Some(_)) | (None, None) => {
                    out.push(format!("tint {key}: a piece or a stamp"));
                }
                _ => {}
            }
            if tint.size.is_some_and(|s| !(0.05..=1.0).contains(&s)) {
                out.push(format!("tint {key}: a size from 0.05 to 1"));
            }
        }
        if let Some(tiles) = &self.move_tiles {
            for name in [&tiles.material, &tiles.ink, &tiles.piece_ink]
                .into_iter()
                .flatten()
            {
                if !self.materials.contains_key(name) {
                    out.push(format!("move tiles: no material {name}"));
                }
            }
        }
        if let Some(grid) = &self.grid {
            for (icon, file) in grid
                .pictures
                .iter()
                .map(|(i, f)| (i.as_str(), f))
                .chain(grid.covered.iter().map(|f| ("a covered box", f)))
            {
                if !assets::exists(file) {
                    out.push(format!("grid: no picture {file} for {icon}"));
                }
            }
            for name in [&grid.material, &grid.rim].into_iter().flatten() {
                if !self.materials.contains_key(name) {
                    out.push(format!("grid: no material {name}"));
                }
            }
            if grid.box_size.is_some_and(|b| !(0.01..=0.1).contains(&b)) {
                out.push("grid: a box from 0.01 to 0.1".into());
            }
        }
        if let Some(markers) = &self.markers {
            for (word, id) in &markers.pieces {
                if !self.pieces.contains_key(id) {
                    out.push(format!("markers: no piece {id} for {word}"));
                }
            }
            for name in [
                Some(&markers.material),
                markers.on.as_ref(),
                markers.off.as_ref(),
                markers.ring.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                if !self.materials.contains_key(name) {
                    out.push(format!("markers: no material {name}"));
                }
            }
            if markers.sides.is_some() && (markers.on.is_none() || markers.off.is_none()) {
                out.push("markers: sides, and not what each side's are made of".into());
            }
            if markers.ringed.is_some() != markers.ring.is_some() {
                out.push("markers: a ring's seat and its material, or neither".into());
            }
            if markers.small.is_some_and(|s| !(0.1..=1.0).contains(&s)) {
                out.push("markers: a small size from 0.1 to 1".into());
            }
            if markers.size.is_some_and(|s| !(0.5..=3.0).contains(&s)) {
                out.push("markers: a size from 0.5 to 3".into());
            }
        }
        if let Some(cards) = &self.cards {
            for (zone, pile) in &cards.piles {
                if let Some(other) = &pile.unless
                    && !cards.piles.contains_key(other)
                {
                    out.push(format!("cards: pile {zone} waits for no pile {other}"));
                }
            }
        }
        if self.games.is_empty() {
            out.push("for no game".into());
        }
        out
    }

    pub fn material(&self, name: Option<&String>) -> Option<&MaterialSpec> {
        name.and_then(|n| self.materials.get(n))
    }

    /// What a part of a piece's model after its first is made of, on the piece of the seat
    /// with this color word: the piece's own for it (`PieceLook::parts`), else the seat's
    /// (`SeatLook::parts`). None when neither says: it is of the piece's own material.
    pub fn part_material(
        &self,
        piece: &PieceLook,
        word: &str,
        part: &str,
    ) -> Option<&MaterialSpec> {
        let seats = || self.seats.get(word)?.parts.get(part);
        self.material(piece.parts.get(part).or_else(seats))
    }
}

/// Only the games an appearance file is for.
#[derive(Deserialize)]
struct Games {
    #[serde(default)]
    games: Vec<String>,
}

/// Every appearance the client has.
#[derive(Resource, Debug, Default)]
pub struct Appearances(pub Vec<Appearance>);

impl Appearances {
    /// The files compiled into the client.
    #[cfg(test)]
    pub fn built_in() -> Self {
        Self::read(assets::FILES.iter().copied(), |_| false)
    }

    /// The files compiled into the client and the private folder's.
    pub fn all() -> Self {
        Self::read(assets::all(), assets::is_private)
    }

    /// The appearance files of these. One that cannot be read is left out, and logged.
    fn read<'a>(
        files: impl Iterator<Item = (&'a str, &'a [u8])>,
        private: impl Fn(&str) -> bool,
    ) -> Self {
        let mut out = Vec::new();
        for (file, bytes) in files {
            if !file.starts_with("appearance/") {
                continue;
            }
            let parsed = std::str::from_utf8(bytes)
                .map_err(|_| "not UTF-8".to_string())
                .and_then(Appearance::parse);
            match parsed {
                Ok(mut a) => {
                    for problem in a.problems() {
                        warn!("{file}: {problem}");
                    }
                    a.private = private(file);
                    out.push(a);
                }
                Err(e) => error!("{file}: {e}"),
            }
        }
        Appearances(out)
    }

    /// The games the private folder has a look for, by id.
    pub fn private_games(&self) -> std::collections::HashSet<String> {
        self.0
            .iter()
            .filter(|a| a.private)
            .flat_map(|a| a.games.iter().cloned())
            .collect()
    }

    pub fn for_game(&self, id: &str) -> Option<&Appearance> {
        self.0.iter().find(|a| a.games.iter().any(|g| g == id))
    }

    /// The file of the picture on this game's card, if it has one.
    pub fn card_picture(&self, id: &str) -> Option<&str> {
        self.for_game(id)?.card.pictures.get(id).map(String::as_str)
    }

    /// The paint a seat of a game is shown in: its appearance's, or the standard look's.
    pub fn seat_paint(&self, game: &str, color: &str, seat: u32) -> Color {
        seat_paint(self.for_game(game), color, seat)
    }
}

/// The paint a seat whose color is `color` is shown in, with this look (if the game has one):
/// the look's, or the standard look's (`look::seat_paint`). With `look.rs`, the one place a
/// color becomes paint (Bingle's ADR 0154).
pub fn seat_paint(look: Option<&Appearance>, color: &str, seat: u32) -> Color {
    look.and_then(|a| a.seats.get(color)?.paint).map_or_else(
        || look::seat_paint(color, seat),
        |[r, g, b]| Color::srgb(r, g, b),
    )
}

pub struct AppearancePlugin;

impl Plugin for AppearancePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Appearances::all());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_appearance_reads_and_names_only_what_it_has() {
        let mut count = 0;
        for (file, _) in assets::FILES
            .iter()
            .filter(|(f, _)| f.starts_with("appearance/"))
        {
            let a = Appearance::parse(assets::text(file).unwrap())
                .unwrap_or_else(|e| panic!("{file}: {e}"));
            assert!(a.problems().is_empty(), "{file}: {:?}", a.problems());
            count += 1;
        }
        assert_eq!(Appearances::built_in().0.len(), count);
    }

    #[test]
    fn a_part_of_a_pieces_own_is_one_of_its_models_after_the_first_in_a_material_it_has() {
        let look = |parts: &str| {
            Appearance::parse(&format!(
                "games = [\"t\"]\n[pieces.die]\nmodel = \"models/die.glb\"\nparts = {{ {parts} }}\n\
                 [materials.ink]\ntint = [0.1, 0.1, 0.1]\n"
            ))
            .unwrap()
            .problems()
        };
        assert!(
            look("pips = \"ink\"").is_empty(),
            "{:?}",
            look("pips = \"ink\"")
        );
        let first = look("body = \"ink\"");
        assert!(first.iter().any(|p| p.contains("part body")), "{first:?}");
        let none = look("handle = \"ink\"");
        assert!(none.iter().any(|p| p.contains("part handle")), "{none:?}");
        let unmade = look("pips = \"paint\"");
        assert!(
            unmade.iter().any(|p| p.contains("no material paint")),
            "{unmade:?}"
        );
    }

    #[test]
    fn a_seats_part_is_one_some_pieces_model_has_and_a_pieces_own_comes_before_it() {
        let look = |seats: &str, own: &str| {
            Appearance::parse(&format!(
                "games = [\"t\"]\n[pieces.ring]\nmodel = \"models/ring.glb\"\n{own}\n\
                 [pieces.disc]\nmodel = \"models/disc.glb\"\n\
                 sides = {{ light = \"gold\", dark = \"silver\" }}\n\
                 [seats.black]\npieces = \"silver\"\n{seats}\n\
                 [seats.white]\npieces = \"gold\"\n\
                 [materials.gold]\ntint = [1.0, 0.8, 0.4]\nmetallic = 1.0\n\
                 [materials.silver]\ntint = [0.9, 0.9, 0.9]\nmetallic = 1.0\n"
            ))
            .unwrap()
        };
        let a = look("parts = { trim = \"gold\" }", "");
        assert!(a.problems().is_empty(), "{:?}", a.problems());
        assert_eq!(a.materials["gold"].metallic, Some(1.0));
        let ring = &a.pieces["ring"];
        // The seat's own for the part; a seat that gives none has the piece's own material.
        assert_eq!(
            a.part_material(ring, "black", "trim"),
            a.materials.get("gold")
        );
        assert_eq!(a.part_material(ring, "white", "trim"), None);
        // What the piece's look gives a part is every seat's.
        let b = look(
            "parts = { trim = \"gold\" }",
            "parts = { trim = \"silver\" }",
        );
        assert!(b.problems().is_empty(), "{:?}", b.problems());
        let ring = &b.pieces["ring"];
        for word in ["black", "white"] {
            assert_eq!(
                b.part_material(ring, word, "trim"),
                b.materials.get("silver")
            );
        }
        // A part no piece's model has after its first (a piece of two sides has none to give),
        // and a material the file does not have.
        for (part, said) in [
            ("rim", "a part rim"),
            ("piece", "a part piece"),
            ("dark", "a part dark"),
        ] {
            let problems = look(&format!("parts = {{ {part} = \"gold\" }}"), "").problems();
            assert!(problems.iter().any(|p| p.contains(said)), "{problems:?}");
        }
        let unmade = look("parts = { trim = \"brass\" }", "").problems();
        assert!(
            unmade.iter().any(|p| p.contains("no material brass")),
            "{unmade:?}"
        );
    }

    /// The private folder's looks, when the variable names one (as the private overlay's CI
    /// does): each reads, and names only what the client and the folder have.
    #[test]
    fn the_private_folders_appearances_read_and_name_only_what_they_have() {
        let Some(dir) = std::env::var_os(assets::PRIVATE_DIR_VAR).filter(|v| !v.is_empty()) else {
            return;
        };
        let (files, problems) = assets::read_private(std::path::Path::new(&dir)).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        assets::use_private(files);
        let mut count = 0;
        for (file, bytes) in assets::all() {
            if !(assets::is_private(file) && file.starts_with("appearance/")) {
                continue;
            }
            let a = Appearance::parse(std::str::from_utf8(bytes).unwrap())
                .unwrap_or_else(|e| panic!("{file}: {e}"));
            assert!(a.problems().is_empty(), "{file}: {:?}", a.problems());
            count += 1;
        }
        assert!(count > 0, "the private folder has no appearance files");
    }

    #[test]
    fn a_file_says_its_format_and_one_for_a_later_format_is_not_read() {
        assert_eq!(Appearance::parse("games = [\"a\"]").unwrap().format, None);
        assert_eq!(
            Appearance::parse(&format!("format = {FORMAT}\ngames = [\"a\"]"))
                .unwrap()
                .format,
            Some(FORMAT)
        );
        assert_eq!(
            Appearance::parse("format = 1\ngames = [\"a\"]")
                .unwrap()
                .format,
            Some(1),
            "an earlier format is read"
        );
        let next = FORMAT + 1;
        let later = Appearance::parse(&format!("format = {next}\ngames = [\"a\"]\nhovers = true"))
            .unwrap_err();
        assert!(
            later.contains(&format!("written for format {next}")),
            "{later}"
        );
        assert!(Appearance::parse("format = \"one\"").is_err());
        assert!(Appearance::parse("format = 0").is_err());
    }

    /// Format 1 called a seat's paint `colour`: a look a server offers may be written so, and
    /// still gives its seat that paint.
    #[test]
    fn a_seats_paint_is_read_from_format_1s_colour_too() {
        let seat = "[seats.white]\npieces = \"m\"\n";
        let mine = "\n[materials.m]\ntint = [1, 1, 1]";
        for text in [
            format!("format = 1\ngames = [\"g\"]\n{seat}colour = [0.75, 0.3, 0.16]{mine}"),
            format!("format = {FORMAT}\ngames = [\"g\"]\n{seat}paint = [0.75, 0.3, 0.16]{mine}"),
            format!("games = [\"g\"]\n{seat}colour = [0.75, 0.3, 0.16]{mine}"),
        ] {
            let look = Appearance::parse(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
            assert!(look.problems().is_empty(), "{:?}", look.problems());
            assert_eq!(look.seats["white"].paint, Some([0.75, 0.3, 0.16]));
            assert_eq!(
                seat_paint(Some(&look), "white", 1),
                Color::srgb(0.75, 0.3, 0.16),
                "{text}"
            );
            assert_eq!(
                seat_paint(Some(&look), "black", 2),
                look::seat_paint("black", 2),
                "a color the look leaves out has the standard look's paint"
            );
        }
        let both = format!("games = [\"g\"]\n{seat}colour = [1, 0, 0]\npaint = [0, 1, 0]{mine}");
        assert!(Appearance::parse(&both).is_err(), "one or the other");
    }

    #[test]
    fn a_look_from_a_server_is_for_its_game_and_names_only_what_it_brought_and_ours() {
        let (man, _) = assets::add_downloaded("downloads/test-look-man.glb", b"x".to_vec());
        assert_eq!(man, b"x");
        assets::add_downloaded("downloads/test-look-card.png", b"x".to_vec());
        let kept = |path: &str| {
            let name = match path {
                "models/man.glb" => "man.glb",
                "pictures/card.png" => "card.png",
                _ => return None,
            };
            Some(format!("downloads/test-look-{name}"))
        };
        let text = |extra: &str| {
            format!(
                "format = 1\ngames = [\"other\"]\n[pieces.man]\nmodel = \"models/man.glb\"\n\
                 [pieces.king]\nmodel = \"models/checkers/king.glb\"\n\
                 [card.pictures]\n\"x-1.0.0\" = \"pictures/card.png\"\n{extra}"
            )
        };
        let one = |text: String| vec![("appearance/x.toml".to_string(), text)];
        let look = Appearance::downloaded("x-2.0.0", &one(text("")), kept).unwrap();
        assert!(look.downloaded);
        assert_eq!(look.games, ["x-2.0.0"], "for the game it came with alone");
        assert_eq!(
            look.card.pictures,
            BTreeMap::from([("x-2.0.0".into(), "downloads/test-look-card.png".into())]),
            "the only picture it has is this game's"
        );
        assert_eq!(look.pieces["man"].model, "downloads/test-look-man.glb");
        assert_eq!(
            look.pieces["king"].model, "models/checkers/king.glb",
            "ours"
        );
        assert_eq!(
            Appearance::downloaded_picture("x-2.0.0", &one(text(""))).as_deref(),
            Some("pictures/card.png")
        );

        // Something it did not bring, nor the client has, and it is left out.
        let lacking = text("[pieces.queen]\nmodel = \"models/queen.glb\"\n");
        let e = Appearance::downloaded("x-2.0.0", &one(lacking), kept).unwrap_err();
        assert!(
            e.contains("no model downloads/missing/models/queen.glb"),
            "{e}"
        );
        // A look that does not say its format, or says a later one.
        let e = Appearance::downloaded("x", &one(text("").replace("format = 1", "")), kept);
        assert!(e.unwrap_err().contains("says the format"));
        let e = Appearance::downloaded(
            "x",
            &one(text("").replace("format = 1", "format = 9")),
            kept,
        );
        assert!(e.unwrap_err().contains("format 9"));

        // Of several files, the one that names the game; none naming it is no look.
        let several = vec![
            (
                "appearance/a.toml".to_string(),
                "format = 1\ngames = [\"a\"]".to_string(),
            ),
            (
                "appearance/b.toml".to_string(),
                "format = 1\ngames = [\"b\"]".to_string(),
            ),
        ];
        assert_eq!(
            Appearance::downloaded("b", &several, kept).unwrap().games,
            ["b"]
        );
        assert!(Appearance::downloaded("c", &several, kept).is_err());
        assert!(Appearance::downloaded("c", &[], kept).is_err());
    }

    #[test]
    fn every_file_a_look_names_is_among_its_files() {
        let mut look = Appearance::parse(
            r#"games = ["g"]
            [pieces.p]
            model = "a"
            [card.pictures]
            g = "b"
            [bowls]
            model = "c"
            stones = "d"
            material = "m"
            [fences]
            model = "e"
            bowl = "f"
            heap = "g"
            material = "m"
            bowl_material = "m"
            [sounds]
            lift = "h"
            put = "i"
            play = "j"
            win = "k"
            lose = "l"
            capture = "n"
            [grid]
            view = "v"
            choice = "c"
            at = [0, 0]
            pictures = { x = "o" }
            covered = "p"
            [materials.m]
            albedo = "q"
            normal = "r"
            arm = "s"
            "#,
        )
        .unwrap();
        let mut names: Vec<String> = look.files_mut().into_iter().map(|f| f.clone()).collect();
        names.sort();
        let all: Vec<String> = "abcdefghijklnopqrs".chars().map(String::from).collect();
        assert_eq!(names, all);
    }

    #[test]
    fn the_games_the_private_folder_has_looks_for_are_known() {
        let files = [
            ("appearance/ours.toml", &b"games = [\"ours\"]"[..]),
            (
                "appearance/made-up.toml",
                b"games = [\"made-up\", \"made-up-small\"]",
            ),
            ("models/made-up.glb", b"x"),
        ];
        let all = Appearances::read(files.into_iter(), |f| f.contains("made-up"));
        assert_eq!(all.0.len(), 2);
        assert_eq!(
            all.private_games(),
            std::collections::HashSet::from(["made-up", "made-up-small"].map(String::from))
        );
        assert!(Appearances::built_in().private_games().is_empty());
    }

    #[test]
    fn chess_and_its_variants_are_chessmen_on_a_chessboard() {
        let all = Appearances::built_in();
        for id in [
            "chess",
            "chess-960",
            "chess-crazyhouse",
            "chess-antichess",
            "chess-atomic",
            "chess-alice",
        ] {
            let a = all
                .for_game(id)
                .unwrap_or_else(|| panic!("{id} has a look"));
            assert!(a.board.squares.is_some(), "{id}'s board is checkered");
            for piece in ["pawn", "knight", "bishop", "rook", "queen", "king"] {
                let model = &a.pieces.get(piece).expect(piece).model;
                assert_eq!(
                    model,
                    &format!("models/chess/{piece}.glb"),
                    "{id}'s {piece}"
                );
            }
            assert!(a.tints.contains_key("check"), "{id} marks a king in check");
        }
    }

    #[test]
    fn a_game_finds_its_appearance_by_id() {
        let all = Appearances::built_in();
        let hex = all.for_game("hex-11").expect("Hex has one");
        assert!(hex.games.iter().any(|g| g == "hex-7"));
        assert!(all.for_game("no-such-game").is_none());
        assert_ne!(
            all.seat_paint("hex-11", "red", 1),
            look::seat_paint("red", 1),
            "Hex's red seat is shown as its stones are"
        );
        assert_eq!(
            all.seat_paint("no-such-game", "red", 1),
            look::seat_paint("red", 1)
        );
    }

    #[test]
    fn games_on_points_say_so_and_others_are_on_cells() {
        let all = Appearances::built_in();
        for id in ["go-9x9", "go-19x19", "pente", "gomoku", "meridians"] {
            let look = all
                .for_game(id)
                .unwrap_or_else(|| panic!("{id} has a look"));
            assert_eq!(look.board.on, PlayOn::Points, "{id}");
        }
        assert_eq!(all.for_game("hex-11").unwrap().board.on, PlayOn::Cells);
    }

    #[test]
    fn a_go_board_has_its_star_points_for_its_size() {
        let all = Appearances::built_in();
        let marks = |id: &str| all.for_game(id).unwrap().board.marks(id).to_vec();
        assert_eq!(marks("go-9x9").len(), 5);
        assert_eq!(marks("go-13x13").len(), 5);
        assert_eq!(marks("go-19x19").len(), 9);
        assert!(marks("go-19x19").contains(&"j10".to_string()), "the centre");
        assert!(marks("pente").is_empty());
        assert!(
            all.for_game("hex-11")
                .unwrap()
                .board
                .marks("hex-11")
                .is_empty()
        );
    }

    #[test]
    fn games_on_a_square_of_points_have_a_picture_and_others_none() {
        let all = Appearances::built_in();
        for id in ["go-9x9", "go-13x13", "go-19x19", "gomoku", "pente"] {
            let picture = all.card_picture(id).unwrap_or_else(|| panic!("{id}"));
            assert!(assets::exists(picture), "{id}: {picture}");
        }
        // A hexagonal board of points would be misdrawn by a square one's picture.
        assert_eq!(all.card_picture("meridians"), None);
        assert_eq!(all.card_picture("hex-11"), None);
        assert_eq!(all.card_picture("no-such-game"), None);
    }

    #[test]
    fn games_of_stones_keep_them_in_bowls() {
        let all = Appearances::built_in();
        for id in ["go-9x9", "meridians", "gomoku"] {
            let bowls = all.for_game(id).unwrap().bowls.as_ref();
            assert!(bowls.is_some_and(|b| assets::exists(&b.model)), "{id}");
        }
        let a = Appearance::parse(
            "games = [\"g\"]\n[bowls]\nmodel = \"models/none.glb\"\nstones = \"models/stone.glb\"\nmaterial = \"missing\"",
        )
        .unwrap();
        assert_eq!(a.problems().len(), 2, "{:?}", a.problems());
    }

    #[test]
    fn a_piece_of_two_sides_names_its_models_parts_and_each_seats_side_up() {
        let all = Appearances::built_in();
        let othello = all.for_game("othello").expect("Othello has one");
        assert!(othello.problems().is_empty(), "{:?}", othello.problems());
        assert!(
            all.for_game("reversi")
                .is_some_and(|a| std::ptr::eq(a, othello)),
            "and an older server's id for it finds the same"
        );
        let disc = &othello.pieces["disc"];
        assert_eq!(disc.up["black"], "dark");
        assert_eq!(disc.up["white"], "light");
        assert!(othello.bowls.is_none(), "no bowls of stones for discs");

        let wrong = |piece: &str| {
            let text =
                format!("games = [\"g\"]\n[materials.m]\ntint = [1, 1, 1]\n[pieces.disc]\n{piece}");
            Appearance::parse(&text).unwrap().problems()
        };
        let disc = "model = \"models/disc_large.glb\"\n";
        assert!(wrong(&format!("{disc}sides = {{ light = \"m\", dark = \"m\" }}")).is_empty());
        // Parts the model does not have; a material the file does not; a side up that is none;
        // a material as well as sides; a side up with no sides.
        for (piece, says) in [
            ("sides = { top = \"m\", dark = \"m\" }", "the model's parts"),
            ("sides = { light = \"m\", dark = \"x\" }", "no material x"),
            (
                "sides = { light = \"m\", dark = \"m\" }\nup = { black = \"top\" }",
                "is no side",
            ),
            (
                "sides = { light = \"m\", dark = \"m\" }\nmaterial = \"m\"",
                "a material and sides",
            ),
            ("up = { black = \"dark\" }", "no sides"),
        ] {
            let problems = wrong(&format!("{disc}{piece}"));
            assert!(
                problems.len() == 1 && problems[0].contains(says),
                "{piece}: {problems:?}"
            );
        }
        // A model of one part has no two sides.
        let stone = wrong("model = \"models/stone.glb\"\nsides = { light = \"m\", dark = \"m\" }");
        assert!(
            stone.iter().any(|p| p.contains("the model's parts")),
            "{stone:?}"
        );
    }

    #[test]
    fn each_kind_of_piece_has_its_model_and_how_it_stands() {
        let all = Appearances::built_in();
        let piece = |game: &str, id: &str| {
            all.for_game(game)
                .and_then(|a| a.pieces.get(id))
                .unwrap_or_else(|| panic!("{game}: {id}"))
        };
        // A stone lies as it falls; a block is square to the board; a piece with a face looks
        // across it.
        assert_eq!(piece("hex-11", "stone").stands, Stands::Loose);
        assert_eq!(piece("tak-5x5", "flat").stands, Stands::Square);
        assert_eq!(piece("tak-5x5", "cap").stands, Stands::Upright);
        // Tak's capstone is a pawn, as a game of pawns' is.
        assert_eq!(
            piece("tak-5x5", "cap").model,
            piece("breakthrough", "pawn").model
        );
        assert_eq!(piece("fist", "ace").stands, Stands::Forward);
        assert_ne!(
            piece("tak-5x5", "flat").model,
            piece("tak-5x5", "wall").model
        );
        assert_ne!(
            piece("draughts", "man").model,
            piece("draughts", "king").model
        );
        // One piece id, two games, two models: a file is for the games that agree.
        assert_ne!(
            piece("fist", "pawn").model,
            piece("breakthrough", "pawn").model
        );
        // A piece may be made of something of its own, and a color no seat plays have a look.
        let arrow = piece("amazons", "arrow");
        let look = all.for_game("amazons").unwrap();
        assert!(look.material(arrow.material.as_ref()).is_some());
        // A board may be checkered, and what a move aims at shown as a piece of its own.
        assert!(look.board.squares.is_some() && look.board.border.is_some());
        assert_eq!(look.aimed.as_deref(), Some("arrow"));
        // Its sounds are every game's (`sounds.rs`).
        assert_eq!(look.sounds, SoundLook::default());
        assert!(all.for_game("fist").unwrap().seats.contains_key("green"));
    }

    #[test]
    fn a_tint_is_a_piece_of_the_files_or_a_stamp_of_one_of_its_materials() {
        let all = Appearances::built_in();
        let look = all.for_game("courtfall-1.0.0").expect("Courtfall has one");
        assert_eq!(look.tints["green"].piece.as_deref(), Some("green"));
        assert_eq!(look.pieces["green"].stands, Stands::Fitted);
        assert!(
            look.materials
                .contains_key(look.tints["eye"].stamp.as_ref().unwrap())
        );
        assert_eq!(
            look.stock.as_ref().map(|s| s.counts.as_str()),
            Some("tiles_left")
        );
        let a = Appearance::parse(
            r#"
            games = ["g"]
            [tints.green]
            piece = "tile"
            [tints.eye]
            stamp = "ink"
            size = 3.0
            [tints.both]
            piece = "tile"
            stamp = "ink"
            [tints.neither]
            "#,
        )
        .unwrap();
        assert_eq!(a.problems().len(), 5, "{:?}", a.problems());
    }

    #[test]
    fn a_supply_may_lie_flat_and_be_counted_in_the_view() {
        let flat = Appearance::parse("games = [\"g\"]\n[supply]\nlie = \"flat\"").unwrap();
        let supply = flat.supply.as_ref().unwrap();
        assert_eq!((supply.lie, supply.counted()), (Lie::Flat, None));
        assert!(flat.problems().is_empty(), "{:?}", flat.problems());
        let counted =
            Appearance::parse("games = [\"g\"]\n[supply]\ncounter = \"stock\"\ntake = \"add\"")
                .unwrap();
        let supply = counted.supply.as_ref().unwrap();
        assert_eq!(supply.lie, Lie::Edge, "on its edge unless the look says");
        assert_eq!(supply.counted(), Some(("stock", "add")));
        let half = Appearance::parse("games = [\"g\"]\n[supply]\ncounter = \"stock\"").unwrap();
        assert_eq!(half.problems().len(), 1, "{:?}", half.problems());
        assert!(Appearance::parse("games = [\"g\"]\n[supply]\nlie = \"askew\"").is_err());
    }

    #[test]
    fn a_piece_may_leave_another_and_a_score_be_kept_on_a_wide_margin() {
        let a = Appearance::parse(
            r#"
            games = ["g"]
            [board]
            margin = 2.2
            [pieces.ring]
            model = "models/ring.glb"
            leaves = "disc"
            [pieces.disc]
            model = "models/disc.glb"
            [score]
            counter = "rings"
            piece = "ring"
            "#,
        )
        .unwrap();
        assert!(a.problems().is_empty(), "{:?}", a.problems());
        assert_eq!(a.board.margin, Some(2.2));
        assert_eq!(a.pieces["ring"].leaves.as_deref(), Some("disc"));
        let score = a.score.as_ref().unwrap();
        assert_eq!(
            (score.counter.as_str(), score.piece.as_str()),
            ("rings", "ring")
        );

        let wrong = Appearance::parse(
            r#"
            games = ["g"]
            [board]
            margin = 9.0
            [pieces.ring]
            model = "models/ring.glb"
            leaves = "ring"
            [pieces.disc]
            model = "models/disc.glb"
            leaves = "nothing"
            [score]
            counter = "rings"
            piece = "absent"
            "#,
        )
        .unwrap();
        assert_eq!(wrong.problems().len(), 4, "{:?}", wrong.problems());
    }

    #[test]
    fn mistakes_are_found() {
        let a = Appearance::parse(
            r#"
            games = ["g"]
            [pieces.stone]
            model = "models/none.glb"
            material = "absent"
            [seats.red]
            pieces = "missing"
            [card.pictures]
            other = "pictures/none.jpg"
            [board]
            squares = ["light", "dark"]
            border = "inlay"
            [board.regions]
            keep = "stained"
            [sounds]
            put = "sounds/none.ogg"
            "#,
        )
        .unwrap();
        assert_eq!(a.problems().len(), 10, "{:?}", a.problems());
        let a = Appearance::parse("games = [\"g\"]\naimed = \"arrow\"").unwrap();
        assert_eq!(a.problems(), ["aimed: no piece arrow"]);
        assert!(
            Appearance::parse("games = [\"g\"]\n[pieces.p]\nmodel = \"m\"\nstands = \"askew\"")
                .is_err(),
            "a way of standing the client does not know is refused"
        );
        assert!(
            Appearance::parse("games = []\ncolor = 1").is_err(),
            "unknown keys are refused, so a misspelling is not ignored"
        );
    }

    #[test]
    fn a_table_of_seats_its_grid_and_its_markers_read_and_what_they_name_is_checked() {
        let good = r#"
            games = ["g"]
            [cards]
            play = "card-"
            radius = 0.5
            [cards.piles.hand]
            seat = [0.0, 0.4]
            spread = "held"
            [cards.piles.trick]
            spread = "played"
            [cards.piles.before]
            spread = "played"
            unless = "trick"
            [grid]
            view = "grid"
            choice = "cross-"
            at = [-0.2, 0.0]
            box = 0.03
            [grid.pictures]
            "suit:a" = "textures/cards/faces/hearts_a_albedo.png"
            [pieces.fox]
            model = "models/chess/knight.glb"
            [markers]
            seat = [-0.2, 0.3]
            material = "wood"
            seat_part = "base"
            sides = "team"
            on = "pale"
            off = "dark"
            ringed = "bonus"
            ring = "pale"
            choose = ["side-"]
            alone = "side-alone"
            [markers.pieces]
            red = "fox"
            [materials.wood]
            [materials.pale]
            [materials.dark]
            [panel]
            counters = ["score"]
        "#;
        let a = Appearance::parse(good).unwrap();
        assert_eq!(a.problems(), Vec::<String>::new());
        let cards = a.cards.as_ref().unwrap();
        assert!(cards.seated());
        assert_eq!(cards.pile("hand:3").and_then(|p| p.seat), Some([0.0, 0.4]));
        assert_eq!(
            cards.pile("before").unwrap().unless.as_deref(),
            Some("trick")
        );
        assert_eq!(a.grid.as_ref().unwrap().box_size, Some(0.03));
        assert_eq!(a.panel.counters, ["score"]);
        // A marker's parts in its seat's color: one name, or several.
        assert_eq!(a.markers.as_ref().unwrap().seat_parts, ["base"]);
        let several = good.replace("seat_part = \"base\"", "seat_parts = [\"base\", \"eyes\"]");
        let markers = Appearance::parse(&several).unwrap().markers.unwrap();
        assert_eq!(markers.seat_parts, ["base", "eyes"]);

        let bad = good
            .replace("unless = \"trick\"", "unless = \"nothing\"")
            .replace("hearts_a_albedo", "no_such")
            .replace("red = \"fox\"", "red = \"owl\"")
            .replace("off = \"dark\"", "")
            .replace("ring = \"pale\"", "ring = \"gold\"")
            .replace("box = 0.03", "box = 3.0");
        let problems = Appearance::parse(&bad).unwrap().problems().join("; ");
        for wanted in [
            "pile before waits for no pile nothing",
            "no picture textures/cards/faces/no_such.png for suit:a",
            "no piece owl for red",
            "sides, and not what each side's are made of",
            "no material gold",
            "a box from 0.01 to 0.1",
        ] {
            assert!(problems.contains(wanted), "{wanted}: {problems}");
        }
        // A game for one seat's look is not laid round a table.
        let alone = Appearance::parse("games = [\"g\"]\n[cards.piles.a]\nat = [0.0, 0.0]").unwrap();
        assert!(!alone.cards.unwrap().seated());
    }
}
