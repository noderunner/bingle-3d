//! Per-game appearance: how this client draws a game, beyond the generic defaults (`look.rs`).
//! Bingle's ADR 0124 leaves how things look to each client; these files are where this one keeps
//! a game's own look, as data (TOML), never code. A game without one is drawn the generic way.
//!
//! The files are `assets/appearance/*.toml`. Each names the games it is for by the server's
//! game id, and may give:
//! - `pieces.<piece id>`: the glTF `model` a piece is drawn as (one the game does not name is
//!   the generic stone), how it `stands` (`Stands`), and a `material` of its own, when it is not
//!   its seat's; `parts` of its model drawn in materials of their own, whoever's it is (an
//!   picture inlaid in a tile); or, for a piece of two sides (`sides.rs`), the material of each of its
//!   model's two parts (`sides`) and which side each seat's are up (`up`);
//! - `seats.<colour word>.pieces` and `.goal`: the material a seat's pieces are made of, and that
//!   its goal's edges are stained; `.colour`, the seat's colour in the panels. A colour word no
//!   seat plays may have one too, for the pieces that are nobody's;
//! - `materials.<name>`: those materials (`look::MaterialSpec`);
//! - `board.on`: `points` for a game played where the board's lines cross, as Go is, rather
//!   than in its spaces (the default, `cells`); `table` for one played on the table itself, with
//!   no board, its positions unseen until lit. The web page's `style: points` and `style: free`
//!   are its own choice of the same; the API does not say it. `board.marks.<game id>`: positions marked on that
//!   game's board, as a Go board's star points are.
//! - `bowls`: a bowl of its stones for each seat, beside the board: the bowl's model, the heap of
//!   stones in it, and the material the bowl is made of;
//! - `card.pictures.<game id>`: the picture on the game's card in the main screen's list (an
//!   image under `pictures/`); a game without one has a blank space there;
//! - `tints.<key>`: how a position the view tints with a key of the game's legend is drawn
//!   (`tints.rs`): a `piece` of this file laid on it (a tile), or a `stamp` of a material
//!   printed on it. A key without one is not drawn;
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
//!   show beside the game (`sheet`, by its name in the view).
//!
//! The private folder (plan, decision 2) adds files of its own, for the games of Bingle's
//! private overlay (ADR 0127), read here as these are; a game the server marks private is
//! shown only when one of them is for it (`Appearances::private_games`).

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::Deserialize;

use crate::assets;
use crate::look::{self, MaterialSpec};

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    /// The games this is for, by the id the server gives them.
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
    /// The pieces a seat has left to place, when the view counts them rather than the reserves
    /// (`supply.rs`).
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
    /// Read from the private folder (`assets::read_private`): its games are shown when the
    /// server marks them private.
    #[serde(skip)]
    pub private: bool,
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
    /// Each zone's pile, by the zone's id.
    pub piles: BTreeMap<String, PileLook>,
}

/// Where a zone's pile lies (its first card's middle, x and z in metres: any origin, the whole
/// is centred), how its cards lie, and the choice a click on it makes.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PileLook {
    pub at: [f32; 2],
    #[serde(default)]
    pub spread: crate::card_layout::Spread,
    pub click: Option<String>,
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
    /// stands, as its holder sees it, and the colour word of the seat its stamp is.
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
    /// bowl for each of several colours.
    pub size: Option<f32>,
}

/// A seat's pieces left to place, counted in the view (a game whose rules are written in Go):
/// the counter that says how many, and the choice (an action's id) that places one.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplyLook {
    pub counter: String,
    pub take: String,
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
    /// Which part's side is up on each seat's pieces, by the seat's colour word; the model's
    /// first part's for a seat not named.
    #[serde(default)]
    pub up: BTreeMap<String, String>,
    /// Parts of its model after the first drawn in a material of their own, by the part's name:
    /// the same on every seat's piece (an picture inlaid in a tile), the first part being its
    /// seat's material. Not with `sides`.
    #[serde(default)]
    pub parts: BTreeMap<String, String>,
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
    /// The seat's colour wherever the interface shows it (the dot by its name), in sRGB: what its
    /// pieces look like, rather than its colour word.
    pub colour: Option<[f32; 3]>,
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

impl Appearance {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// What is wrong with it: names of materials it does not define, and files the client does
    /// not have.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (word, seat) in &self.seats {
            for name in [&seat.pieces, &seat.goal].into_iter().flatten() {
                if !self.materials.contains_key(name) {
                    out.push(format!("seat {word}: no material {name}"));
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
        if self.games.is_empty() {
            out.push("for no game".into());
        }
        out
    }

    pub fn material(&self, name: Option<&String>) -> Option<&MaterialSpec> {
        name.and_then(|n| self.materials.get(n))
    }
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

    /// The colour a seat of a game is shown in: its appearance's, or the generic one.
    pub fn seat_colour(&self, game: &str, word: &str, seat: u32) -> Color {
        self.for_game(game)
            .and_then(|a| a.seats.get(word)?.colour)
            .map_or_else(
                || look::seat_colour(word, seat),
                |[r, g, b]| Color::srgb(r, g, b),
            )
    }
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
    fn a_game_finds_its_appearance_by_id() {
        let all = Appearances::built_in();
        let hex = all.for_game("hex-11").expect("Hex has one");
        assert!(hex.games.iter().any(|g| g == "hex-7"));
        assert!(all.for_game("no-such-game").is_none());
        assert_ne!(
            all.seat_colour("hex-11", "red", 1),
            look::seat_colour("red", 1),
            "Hex's red seat is shown as its stones are"
        );
        assert_eq!(
            all.seat_colour("no-such-game", "red", 1),
            look::seat_colour("red", 1)
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
        let reversi = all.for_game("reversi").expect("Reversi has one");
        assert!(reversi.problems().is_empty(), "{:?}", reversi.problems());
        let disc = &reversi.pieces["disc"];
        assert_eq!(disc.up["black"], "dark");
        assert_eq!(disc.up["white"], "light");
        assert!(reversi.bowls.is_none(), "no bowls of stones for discs");

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
        // A piece may be made of something of its own, and a colour no seat plays have a look.
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
            Appearance::parse("games = []\ncolour = 1").is_err(),
            "unknown keys are refused, so a misspelling is not ignored"
        );
    }
}
