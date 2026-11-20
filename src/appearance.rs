//! Per-game appearance: how this client draws a game, beyond the generic defaults (`look.rs`).
//! Bingle's ADR 0124 leaves how things look to each client; these files are where this one keeps
//! a game's own look, as data (TOML), never code. A game without one is drawn the generic way.
//!
//! The files are `assets/appearance/*.toml`. Each names the games it is for by the server's
//! game id, and may give:
//! - `pieces.<piece id>.model`: the glTF model a piece is drawn as;
//! - `seats.<colour word>.pieces` and `.goal`: the material a seat's pieces are made of, and that
//!   its goal's edges are stained; `.colour`, the seat's colour in the panels;
//! - `materials.<name>`: those materials (`look::MaterialSpec`).
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
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceLook {
    /// A glTF file; its first mesh is the piece.
    pub model: String,
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
    fn mistakes_are_found() {
        let a = Appearance::parse(
            r#"
            games = ["g"]
            [pieces.stone]
            model = "models/none.glb"
            [seats.red]
            pieces = "missing"
            "#,
        )
        .unwrap();
        assert_eq!(a.problems().len(), 2, "{:?}", a.problems());
        assert!(
            Appearance::parse("games = []\ncolour = 1").is_err(),
            "unknown keys are refused, so a misspelling is not ignored"
        );
    }
}
