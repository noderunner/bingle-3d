//! Where a card game's piles lie on the table, and each card of a pile: squared, fanned, or
//! each beyond the one under it towards the player. A patience's piles are laid by its
//! vocabulary (`patience::layout`); another game's by its appearance file (`[cards]`,
//! `Layout::placed`), and a game of several seats' round the table, each seat's piles in front
//! of it (`Layout::seated`): the viewer's hand fanned and tilted up to them, another seat's
//! standing in the air with its back to the table, and what is played in the middle, each card
//! towards who played it. No Bevy systems in it. One unit is one metre, the table's middle the
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

/// How far each card of a row lies to the right of the one before it: its corner's rank and
/// suit are seen.
pub const ROW_STEP: f32 = 0.021;
/// A hand held: how far each card is beyond the one before it, at the most; how wide the whole
/// fan is, at the most; how far it is tilted up to its holder; how much each card is turned
/// from the one before it, and how far the fan's ends curve towards its holder.
pub const HELD_STEP: f32 = 0.024;
pub const HELD_WIDTH: f32 = 0.34;
pub const HELD_TILT: f32 = 0.6;
const HELD_TURN: f32 = 0.022;
const HELD_CURVE: f32 = 0.55;
/// How high over the table a card of a held hand is, each a little over the one before it so
/// none cuts through another.
const HELD_CLEAR: f32 = 0.004;
const HELD_OVER: f32 = 0.0007;
/// Another seat's hand, standing in the air: how far each card is beyond the one before it and
/// how wide the fan is, at the most, and how high its lower edge is over the table.
pub const STAND_STEP: f32 = 0.013;
pub const STAND_WIDTH: f32 = 0.2;
pub const STAND_HIGH: f32 = 0.05;
/// How far from a played pile's middle a card lies, towards the seat that played it.
pub const PLAYED_OUT: f32 = 0.062;

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
    /// Side by side, each over the one before it, the row's middle where the pile is (what a
    /// seat has taken, shown).
    Row,
    /// A hand: the viewer's fanned in front of them and tilted up to them, another seat's
    /// standing in the air, its back to the table.
    Held,
    /// Played to the middle (a trick): each card a little towards the seat that played it.
    Played,
}

/// Where a pile lies: the middle of its first card on the table, how its cards lie, and how
/// it is turned about the vertical (a seat's piles face it: 0 for the player's side).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pile {
    pub at: Vec2,
    pub spread: Spread,
    pub turn: f32,
    /// The viewer's own, of a seat's piles: its hand is theirs to read.
    pub own: bool,
    /// Its place is printed on the cloth, and takes a click when it is empty.
    pub place: bool,
}

impl Pile {
    /// A pile on the player's side's terms, as every pile of a game for one seat is.
    pub fn flat(at: Vec2, spread: Spread) -> Self {
        Pile {
            at,
            spread,
            turn: 0.0,
            own: false,
            place: true,
        }
    }
}

/// How a seat's side of the table is turned about the vertical, for the seat `k` places after
/// the viewer's of `seats` in the order of play: the viewer's is the player's side (0), and
/// play goes to the left, as round a table.
pub fn seat_turn(k: usize, seats: usize) -> f32 {
    -std::f32::consts::TAU * k as f32 / seats.max(1) as f32
}

/// The way from the table's middle to the seat whose side is turned `turn` (x, z), and to
/// that seat's right hand.
pub fn seat_frame(turn: f32) -> (Vec2, Vec2) {
    (
        Vec2::new(turn.sin(), turn.cos()),
        Vec2::new(turn.cos(), -turn.sin()),
    )
}

/// A place given on a seat's own terms (to its right, and from the table's middle towards it)
/// as a place on the table.
pub fn from_seat(turn: f32, at: Vec2) -> Vec2 {
    let (out, right) = seat_frame(turn);
    right * at.x + out * at.y
}

/// A pile of a look: a zone, where it lies, how, and whether each seat has one of its own (its
/// place then on the seat's own terms).
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub zone: String,
    pub at: Vec2,
    pub spread: Spread,
    pub each_seat: bool,
}

/// Where every pile of a game lies on the table, the whole centred on the origin.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub piles: BTreeMap<String, Pile>,
    /// The width (X) and depth (Z) it takes, its columns at their longest.
    pub size: Vec2,
    /// How much of that depth, on the player's side, is room for columns to grow into.
    pub room: f32,
    /// Laid round the table for its seats (`Layout::seated`): the cloth is round.
    pub round: bool,
    /// The viewer's hand, when it is drawn along the bottom of the window instead of in the
    /// world (`Layout::lay_strip`): its zone.
    pub strip: Option<String>,
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
                round: false,
                strip: None,
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
                .map(|(zone, at, spread)| (zone, Pile::flat(at - middle, spread)))
                .collect(),
            size: high - low,
            room: 0.0,
            round: false,
            strip: None,
        }
    }

    /// Piles laid round the table for a game of several seats: `seats` are the game's, in the
    /// order of play, and `viewer` the place among them of the one the table is seen from
    /// (the player's side), which is the viewer's own when this client `plays` it (a table
    /// only watched has no hand to read). A pile each seat has (`hand:2`) lies in front of its
    /// seat, on the seat's own terms; any other where it is said to, the table's middle being
    /// the origin.
    pub fn seated(piles: &[Placed], seats: &[u32], viewer: usize, plays: bool) -> Self {
        let mut out = BTreeMap::new();
        let mut reach = 0.0f32;
        for pile in piles {
            // No place is printed on a table laid round its seats: nothing is put on an
            // empty one by a click.
            let flat = Pile {
                place: false,
                ..Pile::flat(pile.at, pile.spread)
            };
            if !pile.each_seat {
                reach = reach.max(pile.at.length() + PLAYED_OUT);
                out.insert(pile.zone.clone(), flat);
                continue;
            }
            for (i, seat) in seats.iter().enumerate() {
                let k = (i + seats.len() - viewer % seats.len().max(1)) % seats.len().max(1);
                let turn = seat_turn(k, seats.len());
                reach = reach.max(pile.at.length());
                out.insert(
                    format!("{}:{seat}", pile.zone),
                    Pile {
                        at: from_seat(turn, pile.at),
                        turn,
                        own: plays && k == 0,
                        place: false,
                        ..flat
                    },
                );
            }
        }
        // As far as the farthest pile's far corner, and half a hand to its sides.
        let radius = reach + CARD_SIZE.y / 2.0;
        Layout {
            piles: out,
            size: Vec2::splat(2.0 * radius),
            room: 0.0,
            round: true,
            strip: None,
        }
    }

    /// The viewer's own hand (a pile `held` that is theirs) drawn along the bottom of the
    /// window instead of in the world: its cards lie squared, face up, at the near edge of the
    /// table, where a card dealt into it is carried to and one played from it sets off from.
    /// Laid round its seats, after the table's size is settled.
    pub fn lay_strip(&mut self) {
        let edge = (self.size.y - CARD_SIZE.y) / 2.0;
        let Some((zone, pile)) = self
            .piles
            .iter_mut()
            .find(|(_, p)| p.own && p.spread == Spread::Held)
        else {
            return;
        };
        pile.spread = Spread::Squared;
        pile.at = from_seat(pile.turn, Vec2::new(0.0, edge.max(0.0)));
        self.strip = Some(zone.clone());
    }

    /// Where each card of `zone` lies, bottom first, on a table whose top is at `base`: `up`
    /// says which lie face up (shown or not). Empty for a pile the layout does not have.
    pub fn lie(&self, zone: &str, up: &[bool], base: f32) -> Vec<Transform> {
        self.lie_towards(zone, up, &[], base)
    }

    /// As [`Layout::lie`], with, for a pile played to the middle, how the side of the seat
    /// that played each card is turned (`towards`, as far as it is known): the card lies
    /// towards that seat, its foot to them. One with none lies in the pile's middle.
    pub fn lie_towards(
        &self,
        zone: &str,
        up: &[bool],
        towards: &[Option<f32>],
        base: f32,
    ) -> Vec<Transform> {
        let Some(pile) = self.piles.get(zone) else {
            return Vec::new();
        };
        let n = up.len();
        // The hand in the strip comes and goes face up.
        let all_up = vec![true; n];
        let up = if self.strip.as_deref() == Some(zone) {
            &all_up
        } else {
            up
        };
        if pile.spread == Spread::Held {
            return held(pile, n, base);
        }
        let mut offsets = vec![Vec2::ZERO; n];
        match pile.spread {
            Spread::Squared | Spread::Held | Spread::Played => {}
            Spread::Fanned => {
                let first = n.saturating_sub(FANNED);
                for (i, o) in offsets.iter_mut().enumerate().skip(first) {
                    o.x = (i - first) as f32 * FAN_STEP;
                }
            }
            Spread::Row => {
                let middle = (n as f32 - 1.0) / 2.0;
                for (i, o) in offsets.iter_mut().enumerate() {
                    o.x = (i as f32 - middle) * ROW_STEP;
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
                let y = base + CARD_SIZE.z * (i as f32 + 0.5);
                let lies = crate::props::card_lies(*u);
                let played = (pile.spread == Spread::Played)
                    .then(|| towards.get(i).copied().flatten())
                    .flatten();
                let (at, turn) = match played {
                    Some(turn) => (pile.at + seat_frame(turn).0 * PLAYED_OUT, turn),
                    None => (pile.at + from_seat(pile.turn, o), pile.turn),
                };
                Transform::from_xyz(at.x, y, at.y).with_rotation(Quat::from_rotation_y(turn) * lies)
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

/// A hand of `n` cards as its seat holds it, on a table whose top is at `base`: the viewer's
/// own fanned across in front of them, each card over the one to its left so its corner is
/// read, the whole tilted up to them and its ends curved towards them; another seat's upright
/// in the air, facing its seat, closer together.
fn held(pile: &Pile, n: usize, base: f32) -> Vec<Transform> {
    let (step, width, tilt) = if pile.own {
        (HELD_STEP, HELD_WIDTH, HELD_TILT)
    } else {
        (STAND_STEP, STAND_WIDTH, std::f32::consts::FRAC_PI_2)
    };
    let step = if n > 1 {
        step.min(width / (n as f32 - 1.0))
    } else {
        step
    };
    let middle = (n as f32 - 1.0) / 2.0;
    let half = CARD_SIZE.y / 2.0;
    (0..n)
        .map(|i| {
            let along = i as f32 - middle;
            let x = along * step;
            // The ends nearer the holder, as a fan held in one hand is an arc.
            let z = HELD_CURVE * x * x;
            // Its lower edge clear of the table (or as high as a hand stands), whatever its
            // tilt, and a little over the card before it.
            let clear = if pile.own { HELD_CLEAR } else { STAND_HIGH };
            let over = i as f32 * HELD_OVER;
            let y = base + clear + half * tilt.sin() + over * tilt.cos();
            let at = pile.at + from_seat(pile.turn, Vec2::new(x, z + over * tilt.sin()));
            let turn = Quat::from_rotation_y(pile.turn - along * HELD_TURN);
            Transform::from_xyz(at.x, y, at.y).with_rotation(turn * Quat::from_rotation_x(tilt))
        })
        .collect()
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

    fn seated() -> Layout {
        let piles = [
            Placed {
                zone: "hand".into(),
                at: Vec2::new(0.0, 0.4),
                spread: Spread::Held,
                each_seat: true,
            },
            Placed {
                zone: "won".into(),
                at: Vec2::new(0.2, 0.3),
                spread: Spread::Squared,
                each_seat: true,
            },
            Placed {
                zone: "trick".into(),
                at: Vec2::ZERO,
                spread: Spread::Played,
                each_seat: false,
            },
            Placed {
                zone: "shown".into(),
                at: Vec2::new(0.1, 0.0),
                spread: Spread::Row,
                each_seat: false,
            },
        ];
        // Four seats numbered 1, 2, 3, 5, seen from the second.
        Layout::seated(&piles, &[1, 2, 3, 5], 1, true)
    }

    #[test]
    fn a_table_of_several_seats_is_laid_round_from_the_viewers_seat_play_going_left() {
        let layout = seated();
        assert!(layout.round);
        assert_eq!(
            layout.piles.len(),
            10,
            "a hand and a pile each, and two shared"
        );
        let at = |z: &str| layout.piles[z].at;
        // The viewer's own on the player's side, the next seat's to the left, then the far
        // side, then the right.
        assert!((at("hand:2") - Vec2::new(0.0, 0.4)).length() < 1e-6);
        assert!((at("hand:3") - Vec2::new(-0.4, 0.0)).length() < 1e-6);
        assert!((at("hand:5") - Vec2::new(0.0, -0.4)).length() < 1e-6);
        assert!((at("hand:1") - Vec2::new(0.4, 0.0)).length() < 1e-6);
        assert!(layout.piles["hand:2"].own);
        assert!(!layout.piles["hand:3"].own && !layout.piles["trick"].own);
        // A seat's pile is to its own right: the far seat's is to the viewer's left.
        assert!((at("won:2") - Vec2::new(0.2, 0.3)).length() < 1e-6);
        assert!((at("won:5") - Vec2::new(-0.2, -0.3)).length() < 1e-6);
        assert!((at("won:3") - Vec2::new(-0.3, 0.2)).length() < 1e-6);
        // The table's middle is the origin: nothing is moved to centre the whole.
        assert_eq!(at("trick"), Vec2::ZERO);
        assert!(layout.piles.values().all(|p| !p.place), "no place printed");
        assert!(layout.size.x / 2.0 > 0.4 && layout.size.x == layout.size.y);
        // A table only watched has nobody's hand to read.
        let watched = Layout::seated(
            &[Placed {
                zone: "hand".into(),
                at: Vec2::new(0.0, 0.4),
                spread: Spread::Held,
                each_seat: true,
            }],
            &[1, 2],
            0,
            false,
        );
        assert!(watched.piles.values().all(|p| !p.own));
        assert!((watched.piles["hand:2"].at - Vec2::new(0.0, -0.4)).length() < 1e-6);
    }

    #[test]
    fn the_viewers_hand_is_fanned_up_to_them_and_the_others_stand_with_their_backs_to_the_table() {
        let layout = seated();
        let own = layout.lie("hand:2", &[false; 15], 0.002);
        assert_eq!(own.len(), 15);
        // Across, left to right, each over the one before it, no wider than a hand is held.
        for pair in own.windows(2) {
            assert!(pair[1].translation.x > pair[0].translation.x);
            assert!(pair[1].translation.y > pair[0].translation.y);
        }
        let width = own[14].translation.x - own[0].translation.x;
        assert!(width <= HELD_WIDTH + 1e-5 && width > 0.3, "{width}");
        assert!(
            (own[0].translation.x + own[14].translation.x).abs() < 1e-5,
            "centred"
        );
        // Its ends nearer the holder than its middle, and its lower edge clear of the table.
        assert!(own[0].translation.z > own[7].translation.z);
        for card in &own {
            let face = card.rotation * Vec3::Y;
            assert!(
                face.y > 0.5 && face.z > 0.3,
                "up, and towards the viewer: {face}"
            );
            let foot = card.translation + card.rotation * Vec3::Z * CARD_SIZE.y / 2.0;
            assert!(foot.y > 0.002, "{foot}");
        }
        // A few cards are no further apart than they need be.
        let few = layout.lie("hand:2", &[false; 3], 0.0);
        assert!((few[1].translation.x - few[0].translation.x - HELD_STEP).abs() < 1e-5);

        // The seat to the left: upright in the air, facing its seat, its top up.
        let left = layout.lie("hand:3", &[false; 12], 0.002);
        for card in &left {
            let face = card.rotation * Vec3::Y;
            assert!(face.x < -0.95 && face.y.abs() < 1e-4, "to its seat: {face}");
            let top = card.rotation * Vec3::NEG_Z;
            assert!(top.y > 0.99, "{top}");
            assert!(card.translation.y - CARD_SIZE.y / 2.0 >= STAND_HIGH - 1e-5);
        }
        let reach = left[11].translation.z - left[0].translation.z;
        assert!(
            reach.abs() <= STAND_WIDTH + 1e-5 && reach.abs() > 0.1,
            "{reach}"
        );
    }

    #[test]
    fn the_viewers_hand_in_the_strip_lies_squared_and_face_up_at_the_near_edge_of_the_table() {
        let mut layout = seated();
        layout.lay_strip();
        assert_eq!(layout.strip.as_deref(), Some("hand:2"));
        let edge = layout.size.y / 2.0 - CARD_SIZE.y / 2.0;
        let cards = layout.lie("hand:2", &[false; 13], 0.002);
        for card in &cards {
            assert!((card.translation.xz() - Vec2::new(0.0, edge)).length() < 1e-6);
            assert!((card.rotation * Vec3::Y).y > 0.99, "face up");
        }
        // Another seat's hand stands in the air as before.
        let left = layout.lie("hand:3", &[false; 2], 0.002);
        assert!((left[0].rotation * Vec3::NEG_Z).y > 0.99);
        // A table only watched has no hand of the viewer's to draw there.
        let mut watched = Layout::seated(
            &[Placed {
                zone: "hand".into(),
                at: Vec2::new(0.0, 0.4),
                spread: Spread::Held,
                each_seat: true,
            }],
            &[1, 2],
            0,
            false,
        );
        watched.lay_strip();
        assert_eq!(watched.strip, None);
    }

    #[test]
    fn a_card_played_lies_towards_the_seat_that_played_it_and_a_row_about_its_middle() {
        let layout = seated();
        let left = seat_turn(1, 4);
        let far = seat_turn(2, 4);
        let cards = layout.lie_towards("trick", &[true; 3], &[Some(left), Some(far), None], 0.0);
        assert!((cards[0].translation.x + PLAYED_OUT).abs() < 1e-6);
        assert!(cards[0].translation.z.abs() < 1e-6);
        assert!((cards[1].translation.z + PLAYED_OUT).abs() < 1e-6);
        assert_eq!(
            cards[2].translation.xz(),
            Vec2::ZERO,
            "nobody's: in the middle"
        );
        // Its foot to who played it, face up, and over the cards played before it.
        let foot = cards[0].rotation * Vec3::Z;
        assert!(foot.distance(Vec3::NEG_X) < 1e-5, "{foot}");
        assert!((cards[0].rotation * Vec3::Y).y > 0.99);
        assert!(cards[1].translation.y > cards[0].translation.y);

        let row = layout.lie("shown", &[true; 3], 0.0);
        assert!(
            (row[1].translation.x - 0.1).abs() < 1e-6,
            "about where the pile is"
        );
        assert!((row[2].translation.x - row[1].translation.x - ROW_STEP).abs() < 1e-6);
        assert!((row[0].translation.x + row[2].translation.x - 0.2).abs() < 1e-6);
    }
}
