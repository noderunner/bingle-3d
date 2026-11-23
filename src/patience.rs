//! Patience (Bingle's ADR 0122): a game of one seat that moves cards between piles, which any
//! game may declare. Its piles are card zones the vocabulary names (the columns `t1`…, the
//! foundations `f1`…, the free cells `c1`…, the `stock`, the `waste` and the `done` pile of
//! finished runs), and a move is two gestures: the pile to lift from with how many of its top
//! cards (`from-t3-2`), then the pile they go to (`to-t5`, or `to-f` and `to-c` for whichever
//! foundation or free cell takes them); the stock's are one (`draw`, `recycle`, `deal`).
//!
//! This is where the piles lie on the table and which gesture a click on one makes, with no
//! Bevy systems in it; `card_table.rs` draws them. The server says which moves are legal: a
//! click makes a gesture only when one of them starts or goes on with it. No game is named.

use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::card_layout::{Layout, Pile, REACH, Spread};
use crate::net::api::{GameInfo, Gesture, LegalSelection};
use crate::props::CARD_SIZE;

/// Whether a game is played as a patience: it has cards, and its moves are the vocabulary's.
pub fn plays(info: &GameInfo) -> bool {
    info.cards.is_some() && info.actions.iter().any(|a| a.kind == "patience")
}

/// What a pile is, by its zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// What the deal left, face down: a click draws from it, turns the waste back, or deals.
    Stock,
    /// The cards drawn, face up: only the top one is played.
    Waste,
    /// Built up by suit from the ace.
    Foundation,
    /// A free cell: one card, any card.
    Cell,
    /// A column of the tableau, fanned towards the player.
    Column,
    /// The finished runs, off the table.
    Done,
}

/// The kind of a zone, and its number among its kind's (1 for the first column); none for a
/// zone the vocabulary does not name.
pub fn kind(zone: &str) -> Option<(Kind, usize)> {
    match zone {
        "stock" => return Some((Kind::Stock, 0)),
        "waste" => return Some((Kind::Waste, 0)),
        "done" => return Some((Kind::Done, 0)),
        _ => {}
    }
    let (letter, number) = zone.split_at_checked(1)?;
    let number: usize = number.parse().ok().filter(|n| *n > 0)?;
    let kind = match letter {
        "t" => Kind::Column,
        "f" => Kind::Foundation,
        "c" => Kind::Cell,
        _ => return None,
    };
    Some((kind, number))
}

/// A zone's name in a gesture: the waste is `w`.
fn pile_name(zone: &str) -> &str {
    if zone == "waste" { "w" } else { zone }
}

fn action(value: String) -> Gesture {
    Gesture {
        kind: "action".into(),
        value,
    }
}

/// The gesture that lifts the top `count` cards off `zone`.
pub fn lift(zone: &str, count: usize) -> Gesture {
    action(format!("from-{}-{count}", pile_name(zone)))
}

/// The gestures that put what is lifted on `zone`: on whichever foundation (or free cell)
/// takes it, then on that one.
pub fn puts(zone: &str) -> Vec<Gesture> {
    let own = action(format!("to-{}", pile_name(zone)));
    match kind(zone) {
        Some((Kind::Foundation, _)) => vec![action("to-f".into()), own],
        Some((Kind::Cell, _)) => vec![action("to-c".into()), own],
        _ => vec![own],
    }
}

/// What a click on the stock may do, in the order it is tried.
const STOCK: [&str; 3] = ["draw", "recycle", "deal"];

/// What is lifted, by the gestures entered towards a move: the zone and how many of its top
/// cards.
pub fn lifted(gestures: &[Gesture]) -> Option<(String, usize)> {
    let g = gestures.last().filter(|g| g.kind == "action")?;
    let rest = g.value.strip_prefix("from-")?;
    let (pile, count) = rest.rsplit_once('-')?;
    let zone = if pile == "w" { "waste" } else { pile };
    Some((zone.to_string(), count.parse().ok().filter(|n| *n > 0)?))
}

/// What a click does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Click {
    /// Makes this gesture.
    Gesture(Gesture),
    /// Puts what is lifted back where it was.
    PutBack,
    /// Nothing.
    Nothing,
}

/// What a click on `zone` makes: on its card `from_top` from the top (1 for the top card), one
/// whose face is shown, or on the pile itself (its place, or a card face down in it). `held` is
/// what is lifted (its zone, and how many), `next` the gestures that may follow it, and
/// `options` every move there is. A card is lifted when a move starts with it; what is lifted
/// goes where a move takes it, or back where it came from when that is clicked. A click where
/// it may not go does nothing: it stays in hand.
pub fn click(
    zone: &str,
    from_top: Option<usize>,
    held: Option<(&str, usize)>,
    next: &[Gesture],
    options: &[LegalSelection],
) -> Click {
    let starts = |g: &Gesture| options.iter().any(|o| o.gestures.first() == Some(g));
    if kind(zone).is_some_and(|(k, _)| k == Kind::Stock) {
        return STOCK
            .into_iter()
            .map(|a| action(a.into()))
            .find(|g| starts(g))
            .map_or(Click::Nothing, Click::Gesture);
    }
    if let Some((from, _)) = held {
        if let Some(put) = puts(zone).into_iter().find(|g| next.contains(g)) {
            return Click::Gesture(put);
        }
        if zone == from {
            return Click::PutBack;
        }
    }
    match from_top.map(|n| lift(zone, n)) {
        Some(g) if starts(&g) => Click::Gesture(g),
        _ => Click::Nothing,
    }
}

/// The foundation a card of `suit` goes home to, when the view says which suit each is built
/// in (`suits`: the first foundation's first).
pub fn home(suit: &str, suits: &[&str]) -> Option<String> {
    let i = suits.iter().position(|s| *s == suit)?;
    Some(format!("f{}", i + 1))
}

/// The piles `next` may put what is lifted on, of those `zones`. Sent home to whichever
/// foundation takes it (`to-f`), it goes to `home` when that is known.
pub fn targets<'z>(
    zones: impl IntoIterator<Item = &'z str>,
    next: &[Gesture],
    home: Option<&str>,
) -> Vec<&'z str> {
    let own = |z: &str| next.contains(&action(format!("to-{}", pile_name(z))));
    zones
        .into_iter()
        .filter(|z| {
            let any = puts(z).iter().any(|g| next.contains(g));
            let foundation = kind(z).is_some_and(|(k, _)| k == Kind::Foundation);
            any && (own(z) || !foundation || home.is_none_or(|h| h == *z))
        })
        .collect()
}

/// Metres between the cards of a row, and between the two rows.
const GAP: f32 = 0.012;
const ROW_GAP: f32 = 0.02;

/// Where every pile of a patience lies on the table, the whole centred on the origin: a row of
/// the stock, the waste and the free cells to the left and the foundations and the finished
/// runs to the right, beyond the columns, which may reach `REACH` towards the player.
/// The piles of these zones, the vocabulary's; any other is left out. The stock is laid
/// only if it has cards or there is a waste: a game dealt whole has none to show.
pub fn layout<'z>(zones: impl IntoIterator<Item = (&'z str, usize)>) -> Layout {
    let mut by_kind: BTreeMap<Kind, Vec<(usize, &str)>> = BTreeMap::new();
    let mut stock_cards = 0;
    for (zone, count) in zones {
        if let Some((k, n)) = kind(zone) {
            by_kind.entry(k).or_default().push((n, zone));
            if k == Kind::Stock {
                stock_cards = count;
            }
        }
    }
    for piles in by_kind.values_mut() {
        piles.sort();
    }
    let of = |k: Kind| -> Vec<&str> {
        by_kind
            .get(&k)
            .map(|p| p.iter().map(|(_, z)| *z).collect())
            .unwrap_or_default()
    };
    let waste = of(Kind::Waste);
    let mut left: Vec<&str> = Vec::new();
    if stock_cards > 0 || !waste.is_empty() {
        left.extend(of(Kind::Stock));
    }
    left.extend(&waste);
    left.extend(of(Kind::Cell));
    let mut right = of(Kind::Foundation);
    right.extend(of(Kind::Done));
    let columns = of(Kind::Column);
    // The waste's fan takes a place of its own.
    let fan = usize::from(!waste.is_empty());
    let places = columns.len().max(left.len() + fan + right.len()).max(1);

    let pitch = CARD_SIZE.x + GAP;
    let width = places as f32 * pitch - GAP;
    let depth = 2.0 * CARD_SIZE.y + ROW_GAP + REACH;
    let x = |i: usize| (i as f32 - (places as f32 - 1.0) / 2.0) * pitch;
    let top = -depth / 2.0 + CARD_SIZE.y / 2.0;
    let below = top + CARD_SIZE.y + ROW_GAP;

    let mut piles = BTreeMap::new();
    for (i, zone) in left.iter().enumerate() {
        let spread = if waste.contains(zone) {
            Spread::Fanned
        } else {
            Spread::Squared
        };
        piles.insert(
            zone.to_string(),
            Pile {
                at: Vec2::new(x(i), top),
                spread,
            },
        );
    }
    for (i, zone) in right.iter().enumerate() {
        let at = Vec2::new(x(places - right.len() + i), top);
        piles.insert(
            zone.to_string(),
            Pile {
                at,
                spread: Spread::Squared,
            },
        );
    }
    for (i, zone) in columns.iter().enumerate() {
        let at = Vec2::new(x(i), below);
        piles.insert(
            zone.to_string(),
            Pile {
                at,
                spread: Spread::Down,
            },
        );
    }
    Layout {
        piles,
        size: Vec2::new(width, depth),
        room: REACH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card_layout::{DOWN_STEP, FAN_STEP, UP_STEP};
    use crate::net::api::ActionDef;

    fn act(value: &str) -> Gesture {
        action(value.into())
    }

    /// A move of the vocabulary, as the server lists it.
    fn option(gestures: &[&str]) -> LegalSelection {
        LegalSelection {
            notation: gestures.join(" "),
            gestures: gestures.iter().map(|g| act(g)).collect(),
            ..LegalSelection::default()
        }
    }

    #[test]
    fn a_game_is_played_as_a_patience_when_it_has_cards_and_the_vocabularys_moves() {
        let mut info = GameInfo::default();
        assert!(!plays(&info));
        info.actions = vec![ActionDef {
            id: "patience".into(),
            label: "Move".into(),
            kind: "patience".into(),
        }];
        assert!(!plays(&info), "no cards");
        info.cards = Some(Default::default());
        assert!(plays(&info));
        info.actions[0].kind = "place".into();
        assert!(!plays(&info), "an action of the same name but another kind");
    }

    #[test]
    fn the_piles_are_known_by_their_zones() {
        assert_eq!(kind("t10"), Some((Kind::Column, 10)));
        assert_eq!(kind("f1"), Some((Kind::Foundation, 1)));
        assert_eq!(kind("c4"), Some((Kind::Cell, 4)));
        assert_eq!(kind("stock"), Some((Kind::Stock, 0)));
        assert_eq!(kind("waste"), Some((Kind::Waste, 0)));
        assert_eq!(kind("done"), Some((Kind::Done, 0)));
        for none in ["t0", "t", "x1", "hand:1", "", "tt1", "pina"] {
            assert_eq!(kind(none), None, "{none}");
        }
    }

    #[test]
    fn gestures_name_the_waste_w_and_a_foundation_or_cell_by_its_kind_first() {
        assert_eq!(lift("waste", 1), act("from-w-1"));
        assert_eq!(lift("t3", 2), act("from-t3-2"));
        assert_eq!(puts("f2"), [act("to-f"), act("to-f2")]);
        assert_eq!(puts("c1"), [act("to-c"), act("to-c1")]);
        assert_eq!(puts("t7"), [act("to-t7")]);
        assert_eq!(lifted(&[act("from-t10-3")]), Some(("t10".into(), 3)));
        assert_eq!(lifted(&[act("from-w-1")]), Some(("waste".into(), 1)));
        assert_eq!(lifted(&[act("draw")]), None);
        assert_eq!(lifted(&[act("from-t1-0")]), None);
        assert_eq!(lifted(&[]), None);
    }

    #[test]
    fn a_click_lifts_a_card_a_move_starts_with_and_puts_it_where_one_goes_on() {
        let options = [
            option(&["from-t1-1", "to-t2"]),
            option(&["from-t3-1", "to-f"]),
            option(&["from-t5-2", "to-t2"]),
            option(&["draw"]),
        ];
        let none: &[Gesture] = &[];
        // Nothing in hand.
        assert_eq!(
            click("t1", Some(1), None, none, &options),
            Click::Gesture(act("from-t1-1"))
        );
        assert_eq!(
            click("t5", Some(2), None, none, &options),
            Click::Gesture(act("from-t5-2")),
            "a run, by the card it starts at"
        );
        assert_eq!(
            click("t5", Some(1), None, none, &options),
            Click::Nothing,
            "the top card alone does not move"
        );
        assert_eq!(click("t2", None, None, none, &options), Click::Nothing);
        assert_eq!(
            click("stock", None, None, none, &options),
            Click::Gesture(act("draw"))
        );

        // The 1 of t1 in hand: it may go on t2.
        let next = [act("to-t2")];
        let held = Some(("t1", 1));
        assert_eq!(
            click("t2", Some(1), held, &next, &options),
            Click::Gesture(act("to-t2")),
            "on a card of the pile"
        );
        assert_eq!(
            click("t2", None, held, &next, &options),
            Click::Gesture(act("to-t2")),
            "on its place"
        );
        assert_eq!(
            click("t4", None, held, &next, &options),
            Click::Nothing,
            "where it may not go: it stays in hand"
        );
        assert_eq!(
            click("t1", None, held, &next, &options),
            Click::PutBack,
            "its own pile clicked: it goes back"
        );
        assert_eq!(
            click("t3", Some(1), held, &next, &options),
            Click::Gesture(act("from-t3-1")),
            "another card that moves is taken instead"
        );

        // The 1 of t3 in hand: home on whichever foundation is clicked.
        let next = [act("to-f")];
        let held = Some(("t3", 1));
        assert_eq!(
            click("f4", None, held, &next, &options),
            Click::Gesture(act("to-f"))
        );
        assert_eq!(click("f4", None, None, &[], &options), Click::Nothing);
    }

    #[test]
    fn the_stock_draws_turns_the_waste_back_or_deals_whichever_a_move_does() {
        let none: &[Gesture] = &[];
        let recycle = [option(&["recycle"]), option(&["from-w-1", "to-t1"])];
        assert_eq!(
            click("stock", None, None, none, &recycle),
            Click::Gesture(act("recycle"))
        );
        let deal = [option(&["deal"])];
        assert_eq!(
            click("stock", Some(1), Some(("t1", 1)), none, &deal),
            Click::Gesture(act("deal")),
            "even with a card in hand"
        );
        assert_eq!(click("stock", None, None, none, &[]), Click::Nothing);
    }

    #[test]
    fn the_targets_are_the_piles_what_is_lifted_may_go_on() {
        let zones = ["f1", "f2", "t1", "t2", "c1", "waste"];
        assert_eq!(
            targets(zones, &[act("to-f"), act("to-t2")], None),
            ["f1", "f2", "t2"]
        );
        assert_eq!(
            targets(zones, &[act("to-f"), act("to-t2")], Some("f2")),
            ["f2", "t2"],
            "home to its suit's foundation, when the view says which"
        );
        assert_eq!(
            targets(zones, &[act("to-f1")], Some("f2")),
            ["f1"],
            "a foundation named is a foundation named"
        );
        assert_eq!(targets(zones, &[act("to-c")], None), ["c1"]);
        assert!(targets(zones, &[act("draw")], None).is_empty());
        let suits = ["hearts", "spades", "diamonds", "clubs"];
        assert_eq!(home("diamonds", &suits).as_deref(), Some("f3"));
        assert_eq!(home("stars", &suits), None);
    }

    fn zones<'a>(names: &[(&'a str, usize)]) -> Vec<(&'a str, usize)> {
        names.to_vec()
    }

    #[test]
    fn a_klondike_lays_its_stock_and_waste_left_and_its_foundations_right_above_seven_columns() {
        let mut all = zones(&[("stock", 24), ("waste", 0)]);
        let names: Vec<String> = (1..=7)
            .map(|i| format!("t{i}"))
            .chain((1..=4).map(|i| format!("f{i}")))
            .collect();
        all.extend(names.iter().map(|n| (n.as_str(), 0)));
        let layout = layout(all);
        let at = |z: &str| layout.piles[z].at;
        assert_eq!(layout.piles.len(), 13);
        // One row: the stock above the first column, the last foundation above the last.
        assert_eq!(at("stock").x, at("t1").x);
        assert_eq!(at("waste").x, at("t2").x);
        assert_eq!(at("f4").x, at("t7").x);
        assert_eq!(at("f1").x, at("t4").x);
        assert!(at("stock").y < at("t1").y, "the row is beyond the columns");
        assert_eq!(at("stock").y, at("f1").y);
        assert_eq!(layout.piles["waste"].spread, Spread::Fanned);
        assert_eq!(layout.piles["t3"].spread, Spread::Down);
        // Centred, and each card clear of the next.
        assert!((at("t1").x + at("t7").x).abs() < 1e-6);
        assert!(at("t2").x - at("t1").x > CARD_SIZE.x);
        let reach = Vec2::new(
            at("t7").x + CARD_SIZE.x / 2.0,
            at("t1").y + REACH + CARD_SIZE.y / 2.0,
        );
        assert!((reach.x - layout.size.x / 2.0).abs() < 1e-6);
        assert!((reach.y - layout.size.y / 2.0).abs() < 1e-6);
        assert!((at("stock").y - CARD_SIZE.y / 2.0 + layout.size.y / 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_game_dealt_whole_lays_no_stock_and_its_free_cells_take_its_place() {
        let mut names: Vec<String> = (1..=8).map(|i| format!("t{i}")).collect();
        names.extend((1..=4).map(|i| format!("c{i}")));
        names.extend((1..=4).map(|i| format!("f{i}")));
        let mut all: Vec<(&str, usize)> = names.iter().map(|n| (n.as_str(), 0)).collect();
        all.push(("stock", 0));
        let layout = layout(all);
        assert!(!layout.piles.contains_key("stock"));
        let at = |z: &str| layout.piles[z].at;
        assert_eq!(at("c1").x, at("t1").x);
        assert_eq!(at("c4").x, at("t4").x);
        assert_eq!(at("f1").x, at("t5").x);
        assert_eq!(at("f4").x, at("t8").x);
    }

    #[test]
    fn a_game_of_ten_columns_lays_its_stock_and_its_finished_runs_at_either_end() {
        let names: Vec<String> = (1..=10).map(|i| format!("t{i}")).collect();
        let mut all: Vec<(&str, usize)> = names.iter().map(|n| (n.as_str(), 5)).collect();
        all.extend([("stock", 50), ("done", 0), ("hand:1", 3)]);
        let layout = layout(all);
        let at = |z: &str| layout.piles[z].at;
        assert_eq!(at("stock").x, at("t1").x);
        assert_eq!(at("done").x, at("t10").x);
        assert!(!layout.piles.contains_key("hand:1"), "not the vocabulary's");
    }

    #[test]
    fn a_column_fans_towards_the_player_and_closes_up_rather_than_reach_too_far() {
        let names: Vec<String> = (1..=7).map(|i| format!("t{i}")).collect();
        let layout = layout(names.iter().map(|n| (n.as_str(), 0)));
        let base = 0.002;
        let cards = layout.lie("t1", &[false, false, true, true], base);
        let z: Vec<f32> = cards.iter().map(|t| t.translation.z).collect();
        assert!((z[1] - z[0] - DOWN_STEP).abs() < 1e-6, "a card face down");
        assert!((z[2] - z[1] - DOWN_STEP).abs() < 1e-6);
        assert!(
            (z[3] - z[2] - UP_STEP).abs() < 1e-6,
            "one face up shows more"
        );
        assert!(
            cards
                .windows(2)
                .all(|w| w[1].translation.y > w[0].translation.y)
        );
        assert!((cards[0].translation.y - base - CARD_SIZE.z / 2.0).abs() < 1e-7);
        assert!((cards[0].rotation * Vec3::Y).y < 0.0, "face down");
        assert!((cards[3].rotation * Vec3::Y).y > 0.0, "face up");

        let long = layout.lie("t1", &[true; 30], base);
        let reach = long[29].translation.z - long[0].translation.z;
        assert!((reach - REACH).abs() < 1e-5, "{reach}");

        assert!(layout.lie("t9", &[true], base).is_empty(), "no such pile");
    }

    #[test]
    fn the_waste_fans_its_top_three_and_a_squared_pile_rises_card_by_card() {
        let layout = layout([("waste", 0), ("stock", 10), ("f1", 0)]);
        let cards = layout.lie("waste", &[true; 5], 0.0);
        let x: Vec<f32> = cards.iter().map(|t| t.translation.x).collect();
        let first = layout.piles["waste"].at.x;
        assert_eq!([x[0], x[1], x[2]], [first; 3]);
        assert!((x[3] - first - FAN_STEP).abs() < 1e-6);
        assert!((x[4] - first - 2.0 * FAN_STEP).abs() < 1e-6);
        let two = layout.lie("waste", &[true; 2], 0.0);
        assert!((two[1].translation.x - first - FAN_STEP).abs() < 1e-6);

        let pile = layout.lie("f1", &[true; 3], 0.0);
        assert!(
            pile.iter()
                .all(|t| t.translation.xz() == pile[0].translation.xz())
        );
        assert!(pile[2].translation.y > pile[1].translation.y);
    }

    #[test]
    fn what_lands_on_a_pile_lies_where_its_next_cards_would() {
        let names: Vec<String> = (1..=7).map(|i| format!("t{i}")).collect();
        let layout = layout(names.iter().map(|n| (n.as_str(), 0)));
        let up = [false, true];
        let landing = layout.landing("t2", &up, 2, 0.0);
        let whole = layout.lie("t2", &[false, true, true, true], 0.0);
        assert_eq!(landing, whole[2..]);
        assert_eq!(
            layout.landing("t2", &[], 1, 0.0),
            layout.lie("t2", &[true], 0.0)
        );
    }
}
