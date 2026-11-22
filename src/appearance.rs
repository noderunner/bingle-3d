//! Per-game appearance: how this client draws a game, beyond the generic defaults (`look.rs`).
//! Bingle's ADR 0124 leaves how things look to each client; these files are where this one keeps
//! a game's own look, as data (TOML), never code. A game without one is drawn the generic way.
//!
//! The files are `assets/appearance/*.toml`. Each names the games it is for by the server's
//! game id, and may give:
//! - `pieces.<piece id>`: the glTF `model` a piece is drawn as (one the game does not name is
//!   the generic stone), how it `stands` (`Stands`), and a `material` of its own, when it is not
//!   its seat's;
//! - `seats.<colour word>.pieces` and `.goal`: the material a seat's pieces are made of, and that
//!   its goal's edges are stained; `.colour`, the seat's colour in the panels. A colour word no
//!   seat plays may have one too, for the pieces that are nobody's;
//! - `materials.<name>`: those materials (`look::MaterialSpec`);
//! - `board.on`: `points` for a game played where the board's lines cross, as Go is, rather
//!   than in its spaces (the default, `cells`). The web page's `style: points` is its own choice
//!   of the same; the API does not say it. `board.marks.<game id>`: positions marked on that
//!   game's board, as a Go board's star points are.
//! - `bowls`: a bowl of its stones for each seat, beside the board: the bowl's model, the heap of
//!   stones in it, and the material the bowl is made of;
//! - `card.pictures.<game id>`: the picture on the game's card in the main screen's list (an
//!   image under `pictures/`); a game without one has a blank space there.
//!
//! The private overlay's games (plan, decision 2) will add files from a folder; this is where
//! they will be read.

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
    /// The kind of piece what follows a move of two parts is shown as while the move is built
    /// (an arrow shot from where a piece landed), by its id in `pieces`. The server does not
    /// say what it puts down; without this it is the generic stone.
    pub aimed: Option<String>,
    #[serde(default)]
    pub sounds: SoundLook,
}

/// The sounds playing a seat makes, each a file of the client's (`sounds.rs`). Silent without.
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
        {
            if !self.materials.contains_key(name) {
                out.push(format!("board: no material {name}"));
            }
        }
        let sounds = &self.sounds;
        for file in [
            &sounds.lift,
            &sounds.put,
            &sounds.play,
            &sounds.win,
            &sounds.lose,
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
    /// The files compiled into the client. One that cannot be read is left out, and logged.
    pub fn built_in() -> Self {
        let mut out = Vec::new();
        for (file, _) in assets::FILES {
            if !file.starts_with("appearance/") {
                continue;
            }
            let parsed = assets::text(file)
                .ok_or_else(|| "not UTF-8".to_string())
                .and_then(Appearance::parse);
            match parsed {
                Ok(a) => {
                    for problem in a.problems() {
                        warn!("{file}: {problem}");
                    }
                    out.push(a);
                }
                Err(e) => error!("{file}: {e}"),
            }
        }
        Appearances(out)
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
        app.insert_resource(Appearances::built_in());
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
        // And may make sounds as it is played.
        assert!(look.sounds.lift.is_some() && look.sounds.put.is_some());
        assert!(look.sounds.play.is_some());
        assert!(look.sounds.win.is_some() && look.sounds.lose.is_some());
        assert!(all.for_game("fist").unwrap().seats.contains_key("green"));
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
            [sounds]
            put = "sounds/none.ogg"
            "#,
        )
        .unwrap();
        assert_eq!(a.problems().len(), 9, "{:?}", a.problems());
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
