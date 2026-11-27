//! The positions a game's view tints, each with a key of the game's legend (Courtfall's terrain
//! and its Eye), drawn as the game's look says (`[tints]`): a piece of the look laid on the
//! position (a tile, nobody's), or a stamp printed on it; and what both seats have still to
//! place, counted in the view (`[stock]`: a draft's tiles), laid out beside the board. Which
//! positions are tinted, and how many are left, are the server's; how they look is the client's
//! (Bingle's ADR 0124), and a key the look does not name is not drawn. No Bevy systems in it:
//! `board.rs` draws them.

use std::collections::BTreeMap;

use bevy::math::Vec2;

use crate::appearance::TintLook;
use crate::net::api::{Gesture, LegalSelection};

/// How a tint is drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Tinted {
    /// A piece of the look laid on the position, by its id.
    Laid(String),
    /// A stamp of a material printed on the position, so much of the pitch across.
    Stamp { material: String, size: f32 },
}

/// A stamp's size against the pitch, when the look does not say.
pub const STAMP_SIZE: f32 = 0.3;

impl Tinted {
    /// How a look draws its tint, if it says (`Appearance::problems` refuses one that says both).
    pub fn of(look: &TintLook) -> Option<Self> {
        match (&look.piece, &look.stamp) {
            (Some(id), _) => Some(Tinted::Laid(id.clone())),
            (None, Some(material)) => Some(Tinted::Stamp {
                material: material.clone(),
                size: look.size.unwrap_or(STAMP_SIZE),
            }),
            (None, None) => None,
        }
    }
}

/// The positions to draw a tint on: each tinted one whose key `drawn` says is drawn, with its key.
pub fn shown<'a>(
    tints: &'a BTreeMap<String, String>,
    drawn: impl Fn(&str) -> bool + 'a,
) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
    tints
        .iter()
        .filter(move |(_, key)| drawn(key))
        .map(|(at, key)| (at.as_str(), key.as_str()))
}

/// Between the board (and what stands beside it) and the stock, in pitches.
const GAP: f32 = 0.7;
/// Between neighbouring columns, and neighbouring tiles of a column, in pitches: a hexagon whose
/// points are to the far and near sides is a pitch across its flats, and 2/√3 of one from point
/// to point.
const COLUMN: f32 = 1.12;
const ROW: f32 = 1.3;

/// Where the stock's pieces lie (x, z), on the table beyond `beyond` metres to the first seat's
/// left: a column of each kind, in the order of `counts`, along the board's side, each centred on
/// the board's middle, with room for as many as it counts now (a stock only goes down). A kind
/// with none left keeps its column, empty, so the others do not move. Also how far from the
/// board's middle they reach.
pub fn stock(
    counts: &BTreeMap<String, u32>,
    pitch: f32,
    beyond: f32,
) -> (Vec<(String, Vec<Vec2>)>, f32) {
    let mut reach = beyond;
    let columns = counts
        .iter()
        .enumerate()
        .map(|(i, (kind, &n))| {
            let x = -(beyond + pitch * (GAP + 0.5 + COLUMN * i as f32));
            reach = reach.max(-x + pitch / 2.0);
            let middle = (n as f32 - 1.0) / 2.0;
            let slots = (0..n)
                .map(|k| Vec2::new(x, pitch * ROW * (k as f32 - middle)))
                .collect();
            (kind.clone(), slots)
        })
        .collect();
    (columns, reach)
}

/// The choices taking a piece of `kind` from the stock may become: of the selections still
/// possible after the gestures `made`, each that places `kind`, by the choice it makes next or
/// straight after a click (Courtfall's `tile-green`, after the hex). Empty when none may be made.
pub fn takes(options: &[LegalSelection], made: &[Gesture], kind: &str) -> Vec<Gesture> {
    let n = made.len();
    let mut out: Vec<Gesture> = Vec::new();
    for o in options {
        if o.piece.as_deref() != Some(kind) || o.from.is_some() || !o.gestures.starts_with(made) {
            continue;
        }
        let choice = match o.gestures.get(n) {
            Some(g) if !g.is_pos() => Some(g),
            Some(_) => o.gestures.get(n + 1).filter(|g| !g.is_pos()),
            None => None,
        };
        if let Some(g) = choice
            && !out.contains(g)
        {
            out.push(g.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(piece: Option<&str>, stamp: Option<&str>, size: Option<f32>) -> TintLook {
        TintLook {
            piece: piece.map(String::from),
            stamp: stamp.map(String::from),
            size,
        }
    }

    #[test]
    fn a_tint_is_a_piece_laid_or_a_stamp() {
        assert_eq!(
            Tinted::of(&look(Some("green"), None, None)),
            Some(Tinted::Laid("green".into()))
        );
        assert_eq!(
            Tinted::of(&look(None, Some("ink"), Some(0.4))),
            Some(Tinted::Stamp {
                material: "ink".into(),
                size: 0.4
            })
        );
        assert_eq!(
            Tinted::of(&look(None, Some("ink"), None)),
            Some(Tinted::Stamp {
                material: "ink".into(),
                size: STAMP_SIZE
            })
        );
        assert_eq!(Tinted::of(&look(None, None, None)), None);
    }

    #[test]
    fn a_kind_taken_from_the_stock_becomes_the_choice_its_selections_make_after_the_click() {
        let lay = |hex: &str, kind: &str| LegalSelection {
            notation: format!("tile:{kind}@{hex}"),
            action: "tile".into(),
            to: Some(hex.into()),
            from: None,
            piece: Some(kind.into()),
            aim: None,
            gestures: vec![
                Gesture::pos(hex),
                Gesture {
                    kind: "action".into(),
                    value: format!("tile-{kind}"),
                },
            ],
            captures: Vec::new(),
            converts_to: Default::default(),
            path: Vec::new(),
        };
        let options = [lay("b3", "green"), lay("c3", "green"), lay("b3", "red")];
        let action = |v: &str| Gesture {
            kind: "action".into(),
            value: v.into(),
        };
        assert_eq!(takes(&options, &[], "green"), [action("tile-green")]);
        assert_eq!(takes(&options, &[], "red"), [action("tile-red")]);
        assert!(takes(&options, &[], "blue").is_empty());
        // After a click on a hex, what may still be laid there.
        assert_eq!(
            takes(&options, &[Gesture::pos("c3")], "green"),
            [action("tile-green")]
        );
        assert!(takes(&options, &[Gesture::pos("c3")], "red").is_empty());
    }

    #[test]
    fn only_the_keys_the_look_draws_are_shown() {
        let tints: BTreeMap<String, String> = [("d4", "eye"), ("c3", "green"), ("a6", "zone-1")]
            .into_iter()
            .map(|(a, k)| (a.to_string(), k.to_string()))
            .collect();
        let shown: Vec<_> = shown(&tints, |k| k == "eye" || k == "green").collect();
        assert_eq!(shown, [("c3", "green"), ("d4", "eye")]);
    }

    #[test]
    fn the_stock_lies_in_a_column_of_each_kind_beyond_the_board_to_the_left() {
        let pitch = 0.028;
        let counts: BTreeMap<String, u32> = [("blue", 3), ("green", 4), ("red", 0)]
            .into_iter()
            .map(|(k, n)| (k.to_string(), n))
            .collect();
        let (columns, reach) = stock(&counts, pitch, 0.2);
        let kinds: Vec<_> = columns.iter().map(|(k, s)| (k.as_str(), s.len())).collect();
        assert_eq!(kinds, [("blue", 3), ("green", 4), ("red", 0)]);
        for (_, slots) in &columns {
            // Beyond the board, to the left, each column centred on the board's middle.
            assert!(slots.iter().all(|s| s.x < -0.2 - pitch / 2.0));
            let z: f32 = slots.iter().map(|s| s.y).sum();
            assert!(z.abs() < 1e-6, "{z}");
            // Its tiles a little more than a tile apart: none touches another.
            for w in slots.windows(2) {
                assert!(w[1].y - w[0].y > pitch * 2.0 / 3f32.sqrt());
                assert_eq!(w[0].x, w[1].x);
            }
        }
        // Each column further out than the last, by more than a tile's width.
        assert!(columns[0].1[0].x - columns[1].1[0].x > pitch);
        // An empty column keeps its place: the reach counts it.
        assert!(reach > 0.2 + pitch * (GAP + 2.0 * COLUMN + 1.0) - 1e-5);
        let (none, reach) = stock(&BTreeMap::new(), pitch, 0.2);
        assert!(none.is_empty());
        assert_eq!(reach, 0.2);
    }
}
