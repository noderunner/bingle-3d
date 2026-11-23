//! Where a card game's piles lie on the table, and each card of a pile: squared, fanned, or
//! each beyond the one under it towards the player. A patience's piles are laid by its
//! vocabulary (`patience::layout`); another game's by its appearance file (`[cards]`,
//! `Layout::placed`). No Bevy systems in it. One unit is one metre, the table's middle the
//! origin.

use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::props::CARD_SIZE;

/// How far each card of a column lies beyond the one under it: one face down shows its edge,
/// one face up its corner's rank and suit.
pub const DOWN_STEP: f32 = 0.011;
pub const UP_STEP: f32 = 0.021;
/// The longest a column reaches beyond its first card; a longer one is fanned closer.
pub const REACH: f32 = 0.3;
/// How far the waste's top cards are fanned to the right, one beyond another.
pub const FAN_STEP: f32 = 0.017;
/// How many of a fanned pile's top cards are fanned.
const FANNED: usize = 3;

/// How the cards of a pile lie.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Spread {
    /// Squarely on one another.
    #[default]
    Squared,
    /// The top few fanned to the right (the waste).
    Fanned,
    /// Each beyond the one under it, towards the player (a column).
    Down,
}

/// Where a pile lies: the middle of its first card on the table, and how its cards lie.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pile {
    pub at: Vec2,
    pub spread: Spread,
}

/// Where every pile of a game lies on the table, the whole centred on the origin.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub piles: BTreeMap<String, Pile>,
    /// The width (X) and depth (Z) it takes, its columns at their longest.
    pub size: Vec2,
    /// How much of that depth, on the player's side, is room for columns to grow into.
    pub room: f32,
}

impl Layout {
    /// Piles laid where they are said to lie (each its first card's middle), the whole moved so
    /// its middle is the table's.
    pub fn placed(piles: impl IntoIterator<Item = (String, Vec2, Spread)>) -> Self {
        let piles: Vec<(String, Vec2, Spread)> = piles.into_iter().collect();
        if piles.is_empty() {
            return Layout {
                piles: BTreeMap::new(),
                size: Vec2::ZERO,
                room: 0.0,
            };
        }
        let half = CARD_SIZE.xy() / 2.0;
        let (low, high) = piles.iter().fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(lo, hi), (_, at, spread)| {
                // A fanned pile reaches as far as its fan.
                let fan = match spread {
                    Spread::Fanned => Vec2::X * FAN_STEP * (FANNED - 1) as f32,
                    _ => Vec2::ZERO,
                };
                (lo.min(*at - half), hi.max(*at + half + fan))
            },
        );
        let middle = (low + high) / 2.0;
        Layout {
            piles: piles
                .into_iter()
                .map(|(zone, at, spread)| {
                    let at = at - middle;
                    (zone, Pile { at, spread })
                })
                .collect(),
            size: high - low,
            room: 0.0,
        }
    }

    /// Where each card of `zone` lies, bottom first, on a table whose top is at `base`: `up`
    /// says which lie face up (shown or not). Empty for a pile the layout does not have.
    pub fn lie(&self, zone: &str, up: &[bool], base: f32) -> Vec<Transform> {
        let Some(pile) = self.piles.get(zone) else {
            return Vec::new();
        };
        let n = up.len();
        let mut offsets = vec![Vec2::ZERO; n];
        match pile.spread {
            Spread::Squared => {}
            Spread::Fanned => {
                let first = n.saturating_sub(FANNED);
                for (i, o) in offsets.iter_mut().enumerate().skip(first) {
                    o.x = (i - first) as f32 * FAN_STEP;
                }
            }
            Spread::Down => {
                let steps: Vec<f32> = up
                    .iter()
                    .take(n.saturating_sub(1))
                    .map(|u| if *u { UP_STEP } else { DOWN_STEP })
                    .collect();
                let whole: f32 = steps.iter().sum();
                let k = if whole > REACH { REACH / whole } else { 1.0 };
                let mut z = 0.0;
                for (i, o) in offsets.iter_mut().enumerate() {
                    o.y = z;
                    z += steps.get(i).copied().unwrap_or(0.0) * k;
                }
            }
        }
        up.iter()
            .zip(offsets)
            .enumerate()
            .map(|(i, (u, o))| {
                let at = pile.at + o;
                let y = base + CARD_SIZE.z * (i as f32 + 0.5);
                Transform::from_xyz(at.x, y, at.y).with_rotation(crate::props::card_lies(*u))
            })
            .collect()
    }

    /// Where `adding` more cards, face up, would lie on `zone` as it is (`up`, as for
    /// [`Layout::lie`]).
    pub fn landing(&self, zone: &str, up: &[bool], adding: usize, base: f32) -> Vec<Transform> {
        let mut after = up.to_vec();
        after.extend(std::iter::repeat_n(true, adding));
        let mut all = self.lie(zone, &after, base);
        all.split_off(all.len().saturating_sub(adding))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piles_placed_where_a_look_says_are_centred_on_the_table() {
        let layout = Layout::placed([
            ("a".to_string(), Vec2::new(0.1, 0.2), Spread::Squared),
            ("b".to_string(), Vec2::new(0.3, 0.2), Spread::Squared),
            ("c".to_string(), Vec2::new(0.2, 0.4), Spread::Squared),
        ]);
        let at = |z: &str| layout.piles[z].at;
        assert!((at("a") + at("b")).x.abs() < 1e-6, "centred across");
        assert!((at("a").y - CARD_SIZE.y / 2.0 + layout.size.y / 2.0).abs() < 1e-6);
        assert!((at("c").y + CARD_SIZE.y / 2.0 - layout.size.y / 2.0).abs() < 1e-6);
        assert!((layout.size.x - 0.2 - CARD_SIZE.x).abs() < 1e-6);
        assert!(
            (at("b") - at("a") - Vec2::new(0.2, 0.0)).length() < 1e-6,
            "as far apart as said"
        );
        assert_eq!(layout.room, 0.0);
        assert!(Layout::placed([]).piles.is_empty());
    }
}
