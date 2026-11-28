//! A deck shuffled and its cards dealt, for any card game (`card_table.rs` plays them): what
//! the eye is shown of how the cards got where the position puts them. The server shuffles and
//! deals; no event says it did, and the order is the server's. So both are shown only where the
//! client can tell without guessing: at the start, when a game is seen at its first position
//! (the deck shuffled, then dealt), and when one move carries several cards from one pile to
//! several others (a row dealt from the stock). Every card shuffles and travels face down, and
//! turns up only on its way to where the position shows it to this client.
//!
//! The shuffle is one riffle: the deck cut into two halves laid side by side, their inner edges
//! lifted, the cards falling from them in turn into one deck, which is squared. A deal goes from
//! the deck's place round the piles one card at a time, in the order of `rank` (round the
//! seats from the game's first, or a patience's columns from the left), the cards that stay in
//! the deck staying. A press of the primary button, or the next position, ends both at once:
//! that press does nothing else. Neither makes a sound yet; the pipeline has none of a card.
//!
//! No Bevy systems in it: where each card is, and when.

use bevy::prelude::*;

use crate::patience;
use crate::props::{self, CARD_SIZE};

/// How long the shuffle takes, in seconds, and the most a deal does.
pub const SHUFFLE: f32 = 1.0;
pub const DEAL_MOST: f32 = 2.0;
/// How long after one another the cards of a deal set off, at the most: a short deal is not
/// slower than this.
pub const DEAL_STEP: f32 = 0.06;
/// How many cards one move carries from one pile to several others for it to be shown as a
/// deal.
pub const DEAL_LEAST: usize = 3;

/// The riffle's moments, as shares of [`SHUFFLE`]: the halves cut apart side by side; lifted
/// at their inner edges; the cards falling in turn from then until `FALLEN`, each taking
/// `FALL`; the deck squared by the end.
const CUT: f32 = 0.22;
const LIFTED: f32 = 0.34;
const FALLEN: f32 = 0.8;
const FALL: f32 = 0.08;
/// How far each half lies from the deck's middle, and where the cards falling from it land, in
/// card widths; how far its inner edge is lifted (radians).
const SPLIT: f32 = 0.56;
const LANDS: f32 = 0.12;
const TILT: f32 = 0.26;

/// Where the deck lies: the middle of its bottom card on the table, and how it is turned about
/// the vertical (as a pile is).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Deck {
    pub at: Vec2,
    pub turn: f32,
}

impl Deck {
    /// The card `j` from the bottom of the deck squared, face down, on a table at `base`: as
    /// the deck's pile lays it, when it is one.
    pub fn slot(&self, j: usize, base: f32) -> Transform {
        self.posed(Vec2::new(0.0, j as f32), 0.0, base)
    }

    /// A card `along` card widths to the deck's right and `high` cards' thickness up, its
    /// edge towards the deck's middle lifted by `tilt` (about its edge away from it).
    fn posed(&self, along_high: Vec2, tilt: f32, base: f32) -> Transform {
        let (along, high) = (along_high.x, along_high.y);
        let raise = CARD_SIZE.x / 2.0 * tilt.abs().sin();
        let turn = Quat::from_rotation_y(self.turn);
        let side = turn * Vec3::X * along * CARD_SIZE.x;
        let y = base + CARD_SIZE.z * (high + 0.5) + raise;
        Transform::from_translation(Vec3::new(self.at.x, y, self.at.y) + side)
            .with_rotation(turn * Quat::from_rotation_z(tilt) * props::card_lies(false))
    }

    /// Where the card `j` from the bottom of the deck after the riffle (of `n` cards) is, `secs`
    /// into it, on a table at `base`. Before it, the deck squared as it was; after it, `slot`.
    pub fn riffle(&self, j: usize, n: usize, secs: f32, base: f32) -> Transform {
        let u = (secs / SHUFFLE).clamp(0.0, 1.0);
        let (half, from) = before_riffle(j, n);
        // The left half (-1) lifts its right edge, the right half (+1) its left.
        let side = if half == 0 { -1.0 } else { 1.0 };
        let tilt = -side * TILT;
        let eased = |f: f32| {
            let f = f.clamp(0.0, 1.0);
            f * f * (3.0 - 2.0 * f)
        };
        if u < CUT {
            // Apart, each as high as it was: the top half over the bottom as they part.
            let f = eased(u / CUT);
            return self.posed(Vec2::new(side * SPLIT * f, from as f32), 0.0, base);
        }
        let falls = falls(j, n);
        if u < falls {
            // Lifted at the inner edge, each as high as it will be in the deck: the cards
            // still in a half are always over those fallen.
            let f = eased((u - CUT) / (LIFTED - CUT));
            let high = from as f32 + (j as f32 - from as f32) * f;
            return self.posed(Vec2::new(side * SPLIT, high), tilt * f, base);
        }
        if u < falls + FALL {
            let f = eased((u - falls) / FALL);
            let along = side * (SPLIT + (LANDS - SPLIT) * f);
            return self.posed(Vec2::new(along, j as f32), tilt * (1.0 - f), base);
        }
        // Fallen, then pushed square.
        let f = eased((u - FALLEN) / (1.0 - FALLEN));
        self.posed(Vec2::new(side * LANDS * (1.0 - f), j as f32), 0.0, base)
    }
}

/// When the card `j` from the bottom after a riffle of `n` begins to fall from its half, as a
/// share of the shuffle: in turn from the bottom.
fn falls(j: usize, n: usize) -> f32 {
    LIFTED + (FALLEN - LIFTED - FALL) * j as f32 / (n.max(2) - 1) as f32
}

/// Where the card `j` from the bottom after a riffle of `n` was before it: which half (0 the
/// bottom, cut to the left; 1 the top, to the right) and its place from the bottom of the deck.
/// The halves' cards fall in turn from the bottom, the left's first.
pub fn before_riffle(j: usize, n: usize) -> (usize, usize) {
    let lower = n.div_ceil(2);
    let upper = n - lower;
    // Interleaved while both have cards; the rest of the larger after.
    let pairs = 2 * upper.min(lower);
    if j < pairs {
        if j.is_multiple_of(2) {
            (0, j / 2)
        } else {
            (1, lower + j / 2)
        }
    } else {
        // Only the lower half has more (it has the odd card).
        (0, j - upper)
    }
}

/// The order piles are dealt to in a round: a patience's by its vocabulary (the columns from
/// the left), then round the seats in the game's order (`seats`, by number) from its first.
pub fn rank(zone: &str, seats: &[u32]) -> (Option<(patience::Kind, usize)>, usize, String) {
    let (base, seat) = match zone.split_once(':') {
        Some((base, seat)) => (base, seat.parse::<u32>().ok()),
        None => (zone, None),
    };
    let place = seat
        .and_then(|s| seats.iter().position(|x| *x == s))
        .unwrap_or(0);
    (patience::kind(zone), place, base.to_string())
}

/// The order cards are dealt in, round the piles one card at a time: each card given as its
/// pile's rank and its place among the cards that pile is dealt (0 the first). The how manyth
/// each is dealt, in the order given.
pub fn order<R: Ord>(cards: &[(R, usize)]) -> Vec<usize> {
    let mut by: Vec<usize> = (0..cards.len()).collect();
    by.sort_by(|a, b| {
        let (ra, ia) = &cards[*a];
        let (rb, ib) = &cards[*b];
        ia.cmp(ib).then(ra.cmp(rb))
    });
    let mut out = vec![0; cards.len()];
    for (k, i) in by.into_iter().enumerate() {
        out[i] = k;
    }
    out
}

/// How long after one another `n` cards of a deal set off, each taking `takes` to arrive, so
/// the whole takes [`DEAL_MOST`] at the most.
pub fn step(n: usize, takes: f32) -> f32 {
    if n < 2 {
        return DEAL_STEP;
    }
    DEAL_STEP.min(((DEAL_MOST - takes) / (n - 1) as f32).max(0.0))
}

/// A shuffle or a deal under way, for the game watched, in time's seconds; or the last one
/// done, so the same start is not shown twice (the table laid again in a headset).
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct Dealing {
    /// The game and the position (`move_seq`) dealt.
    pub room: String,
    pub seq: u64,
    pub deck: Deck,
    /// When it began, how long its shuffle takes (0 for none), how long after one another
    /// the cards set off, and when it is over, all in seconds.
    pub began: f32,
    pub shuffle: f32,
    pub step: f32,
    pub ends: f32,
    /// How many cards are in it.
    pub cards: usize,
    pub running: bool,
    /// A press ended it: the button is still down, and its click is nobody's.
    pub swallow: bool,
}

impl Dealing {
    /// Whether a click on the table now is taken by the deal, not by the game.
    pub fn holds_clicks(&self) -> bool {
        self.running || self.swallow
    }

    /// Whether the start of the game `room` has been shown already.
    pub fn shown_start(&self, room: &str) -> bool {
        self.room == room && self.seq == 0
    }
}

/// A card of a shuffle or a deal under way: its place from the bottom of the deck after the
/// shuffle, and the how manyth it is dealt (none for a card that stays in the deck).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dealt {
    pub slot: usize,
    pub nth: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_riffle_interleaves_the_two_halves_of_the_deck_and_moves_every_card_once() {
        // Every place before is some card's, once.
        for n in [1, 2, 5, 52, 53] {
            let mut seen: Vec<usize> = (0..n).map(|j| before_riffle(j, n).1).collect();
            seen.sort();
            assert_eq!(seen, (0..n).collect::<Vec<_>>(), "{n} cards");
        }
        // The bottom half to the left, the top to the right, falling in turn from the bottom.
        assert_eq!(before_riffle(0, 52), (0, 0));
        assert_eq!(before_riffle(1, 52), (1, 26));
        assert_eq!(before_riffle(2, 52), (0, 1));
        assert_eq!(before_riffle(51, 52), (1, 51));
        // An odd deck's last card is the lower half's.
        assert_eq!(before_riffle(52, 53), (0, 26));
    }

    #[test]
    fn the_shuffle_starts_from_the_deck_as_it_was_and_ends_squared_face_down() {
        let deck = Deck {
            at: Vec2::new(0.1, -0.05),
            turn: 0.4,
        };
        let n = 30;
        for j in 0..n {
            let (_, from) = before_riffle(j, n);
            let start = deck.riffle(j, n, 0.0, 0.002);
            assert!(
                start
                    .translation
                    .distance(deck.slot(from, 0.002).translation)
                    < 1e-6
            );
            let end = deck.riffle(j, n, SHUFFLE, 0.002);
            assert!(end.translation.distance(deck.slot(j, 0.002).translation) < 1e-6);
            assert!(end.rotation.angle_between(deck.slot(j, 0.002).rotation) < 1e-5);
            // Face down all the way through: the face never turns up.
            for i in 0..=40 {
                let t = deck.riffle(j, n, SHUFFLE * i as f32 / 40.0, 0.002);
                assert!((t.rotation * Vec3::Y).y < -0.9, "card {j} at {i}");
                assert!(t.translation.y > 0.002, "never through the table");
            }
        }
        // Halfway, the halves lie apart, either side of the middle.
        let mid = SHUFFLE * (LIFTED + 0.01);
        let left = deck.riffle(n - 2, n, mid, 0.0).translation;
        let right = deck.riffle(n - 1, n, mid, 0.0).translation;
        let across = Quat::from_rotation_y(deck.turn) * Vec3::X;
        let middle = Vec3::new(deck.at.x, 0.0, deck.at.y);
        assert!((left - middle).dot(across) < -CARD_SIZE.x / 2.0);
        assert!((right - middle).dot(across) > CARD_SIZE.x / 2.0);
    }

    #[test]
    fn a_card_still_in_a_half_is_over_every_card_fallen() {
        let deck = Deck::default();
        let n = 52;
        for i in 0..=60 {
            let u = LIFTED + (1.0 - LIFTED) * i as f32 / 60.0;
            let y = |j: usize| deck.riffle(j, n, u * SHUFFLE, 0.0).translation.y;
            let fallen: Vec<usize> = (0..n).filter(|j| u >= falls(*j, n) + FALL).collect();
            let waiting: Vec<usize> = (0..n).filter(|j| u < falls(*j, n)).collect();
            for low in &fallen {
                for high in &waiting {
                    assert!(y(*high) > y(*low), "at {u}: {high} under {low}");
                }
            }
            // The deck that forms is in its order from the bottom.
            for pair in fallen.windows(2) {
                assert!(y(pair[1]) > y(pair[0]));
            }
        }
    }

    #[test]
    fn a_deal_goes_round_the_piles_one_card_at_a_time_from_the_first_seat() {
        let seats = [3, 1, 2];
        // Two cards each to three seats, given in no order.
        let cards = [
            (rank("hand:1", &seats), 0),
            (rank("hand:1", &seats), 1),
            (rank("hand:3", &seats), 0),
            (rank("hand:3", &seats), 1),
            (rank("hand:2", &seats), 0),
            (rank("hand:2", &seats), 1),
        ];
        // Seat 3 is the game's first: it is dealt first, then 1, then 2, and round again.
        assert_eq!(order(&cards), [1, 4, 0, 3, 2, 5]);

        // A patience's columns from the left, as a column of seven is dealt by hand: one to
        // each, then one to each but the first.
        let cards: Vec<_> = [
            ("t2", 0),
            ("t1", 0),
            ("t2", 1),
            ("t3", 0),
            ("t3", 1),
            ("t3", 2),
        ]
        .iter()
        .map(|(z, i)| (rank(z, &[1]), *i))
        .collect();
        assert_eq!(order(&cards), [1, 0, 3, 2, 4, 5]);
    }

    #[test]
    fn a_whole_deck_to_four_hands_is_dealt_within_two_seconds_and_a_short_deal_is_not_slower() {
        let takes = 0.42;
        assert!(51.0 * step(52, takes) + takes <= DEAL_MOST + 1e-5);
        assert!(103.0 * step(104, takes) + takes <= DEAL_MOST + 1e-5);
        assert_eq!(step(5, takes), DEAL_STEP);
        assert_eq!(step(1, takes), DEAL_STEP);
        const { assert!(SHUFFLE <= 1.2, "a second or so") };
    }
}
