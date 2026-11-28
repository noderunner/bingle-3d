//! `--test-hand N`: a made-up card game of three seats on the table in place of a server's, the
//! player holding N cards of the usual deck, the other two a few each, what two have taken in
//! front of them, and one card led to the trick; those of the led suit may be played. It names no game and needs no server: it is for
//! looking at the player's hand (`hand_strip.rs`) and the table round it, with `--frames` and
//! `--screenshot`. A click on a card plays it as far as the client goes: the move is sent to
//! whatever server there is, so give the run a `--server` nobody listens on.
//!
//! With `--test-deal`, the same game at its start instead (`deal.rs`): waiting to start, its
//! table bare, until frame 60 ([`DEAL_AT`]), when it starts with N cards dealt to each seat and
//! nothing played, so the deck is shuffled and dealt. In a run a script limits, time goes on by
//! a thirtieth of a second each frame whatever the machine, as in the showcase, so a picture at
//! a frame shows the same moment of the shuffle and the deal everywhere.

use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use serde_json::json;

use crate::appearance::{Appearance, Appearances};
use crate::login::Screen;
use crate::net::api::LegalSelection;
use crate::net::session::Opened;
use crate::net::{WatchStatus, open_table};
use crate::play::{Stage, Turn};

pub struct TestHandPlugin {
    /// How many cards the player holds.
    pub cards: usize,
    /// The game at its start, dealt at [`DEAL_AT`].
    pub deal: bool,
    /// Whether a script limits the run: time then goes on by a frame's worth each frame.
    pub scripted: bool,
}

/// The frame the game starts at, with `--test-deal`.
pub const DEAL_AT: u32 = 60;
/// How far time goes on each frame of a run a script limits.
const FRAME: std::time::Duration = std::time::Duration::from_nanos(1_000_000_000 / 30);

impl Plugin for TestHandPlugin {
    fn build(&self, app: &mut App) {
        let (cards, deal) = (self.cards, self.deal);
        app.add_systems(
            Startup,
            move |mut appearances: ResMut<Appearances>,
                  mut screen: ResMut<Screen>,
                  mut watch: ResMut<WatchStatus>| {
                appearances.0.push(look());
                *screen = Screen::Main;
                if deal {
                    open_table(&mut watch, waiting());
                    return;
                }
                open_table(&mut watch, opened(cards));
                watch.turn = Some(turn(&watch.table.as_ref().expect("laid").state.state));
            },
        );
        if deal {
            app.add_systems(
                Update,
                move |frames: Res<FrameCount>, mut watch: ResMut<WatchStatus>| {
                    if frames.0 == DEAL_AT {
                        open_table(&mut watch, started(cards));
                        watch.turn = Some(turn(&watch.table.as_ref().expect("laid").state.state));
                    }
                },
            );
        }
        if self.scripted {
            app.insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
        }
    }
}

/// The made-up game's id, which no server has.
pub const GAME: &str = "test-hand";

/// Its look: a hand each seat holds, what each has taken, and a trick in the middle.
pub fn look() -> Appearance {
    Appearance::parse(&format!(
        r#"
        games = ["{GAME}"]
        [cards]
        play = "card-"
        [cards.piles.hand]
        seat = [0.0, 0.4]
        spread = "held"
        [cards.piles.taken]
        seat = [0.2, 0.3]
        spread = "row"
        [cards.piles.trick]
        spread = "played"
        "#
    ))
    .expect("the made-up look reads")
}

const SUITS: [(&str, &str); 4] = [
    ("spades", "♠"),
    ("hearts", "♥"),
    ("clubs", "♣"),
    ("diamonds", "♦"),
];
const RANKS: [&str; 13] = [
    "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A",
];

/// The `i`th card of the usual deck, by suit then rank.
fn card_id(i: usize) -> String {
    let (suit, _) = SUITS[(i / 13) % 4];
    format!("{suit}-{}", RANKS[i % 13])
}

/// The game opened, seen from its second seat, which holds `cards` cards, up to 26 (the
/// deck's, from a few spades on: some of the led suit, some not); the first seat has led a
/// card.
pub fn opened(cards: usize) -> Opened {
    let hand: Vec<serde_json::Value> = (0..cards)
        .map(|i| json!({"id": card_id((i + 8) % 52)}))
        .collect();
    let others = |n: usize| vec![json!({}); n];
    let zones = json!([
        {"id": "hand", "owner": 1, "count": 4, "cards": others(4)},
        {"id": "hand", "owner": 2, "count": cards, "cards": hand},
        {"id": "hand", "owner": 3, "count": 5, "cards": others(5)},
        {"id": "taken", "owner": 2, "count": 3, "cards": [
            {"id": card_id(40), "up": true}, {"id": card_id(41), "up": true},
            {"id": card_id(42), "up": true}]},
        {"id": "taken", "owner": 1, "count": 1, "cards": [{"id": card_id(35), "up": true}]},
        {"id": "trick", "count": 1, "cards": [{"id": card_id(1), "up": true}],
         "captions": ["North"]}
    ]);
    game(zones, "active", 1)
}

/// The game waiting to start: nothing on the table yet.
pub fn waiting() -> Opened {
    game(json!([]), "waiting", 0)
}

/// The game at its start: `cards` cards dealt to each seat (the player's, the second, from a
/// few spades on), nothing taken, nothing played.
pub fn started(cards: usize) -> Opened {
    let hand: Vec<serde_json::Value> = (0..cards)
        .map(|i| json!({"id": card_id((i + 8) % 52)}))
        .collect();
    let others = vec![json!({}); cards];
    let zones = json!([
        {"id": "hand", "owner": 1, "count": cards, "cards": others},
        {"id": "hand", "owner": 2, "count": cards, "cards": hand},
        {"id": "hand", "owner": 3, "count": cards, "cards": others},
        {"id": "taken", "owner": 1, "count": 0, "cards": []},
        {"id": "taken", "owner": 2, "count": 0, "cards": []},
        {"id": "taken", "owner": 3, "count": 0, "cards": []},
        {"id": "trick", "count": 0, "cards": []}
    ]);
    game(zones, "active", 0)
}

/// The game with the view's `zones`, its status and its moves played so far, the second seat
/// on impulse.
fn game(zones: serde_json::Value, status: &str, seq: u64) -> Opened {
    let types: Vec<serde_json::Value> = (0..52)
        .map(|i| {
            let (suit, _) = SUITS[i / 13];
            let rank = RANKS[i % 13];
            json!({"id": card_id(i), "suit": suit, "rank": rank, "label": rank,
                   "value": i % 13 + 2})
        })
        .collect();
    let suits: serde_json::Map<String, serde_json::Value> = SUITS
        .iter()
        .map(|(suit, symbol)| (suit.to_string(), json!({"label": suit, "symbol": symbol})))
        .collect();
    let info = json!({
        "id": GAME, "name": "A test of a hand",
        "seats": [{"seat": 1, "color": "red", "label": "North"},
                  {"seat": 2, "color": "blue", "label": "South"},
                  {"seat": 3, "color": "green", "label": "West"}],
        "board": {"kind": "grid", "positions": [{"id": "0,0", "x": 0, "y": 0}]},
        "actions": [{"id": "move", "type": "move", "label": "Play"}],
        "cards": {"suits": suits, "suit_order": SUITS.map(|s| s.0), "types": types,
                  "back": {}, "zones": [{"id": "hand", "scope": "seat"}]}
    });
    let state = json!({
        "state": {"cells": {}, "ext": {"view": {"table": zones, "panel_actions": ["card-"]}}},
        "status": status, "move_seq": seq, "impulse_seat": 2
    });
    let game = json!({"game_def_id": GAME, "status": status, "seats": [
        {"seat": 1, "color": "red", "name": "Ana"},
        {"seat": 2, "color": "blue", "name": "Ben", "yours": true},
        {"seat": 3, "color": "green", "name": "Cy"}
    ]});
    Opened {
        room_code: "TEST00".into(),
        info: serde_json::from_value(info).expect("the made-up game reads"),
        game: serde_json::from_value(game).expect("its seats read"),
        state: serde_json::from_value(state).expect("its position reads"),
        bots: vec![],
    }
}

/// The player's impulse: a card of the led suit (spades), from their hand.
pub fn turn(position: &crate::net::api::Position) -> Turn {
    let mut turn = Turn::new(1);
    turn.clicks_only = true;
    turn.stage = Stage::Choosing;
    turn.options = position
        .card_zones()
        .get("hand:2")
        .into_iter()
        .flatten()
        .filter(|c| c.id.starts_with("spades-"))
        .map(|c| LegalSelection {
            notation: c.id.clone(),
            action: "move".into(),
            gestures: vec![crate::net::api::Gesture {
                kind: "action".into(),
                value: format!("card-{}", c.id),
            }],
            ..Default::default()
        })
        .collect();
    turn
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Table;

    #[test]
    fn the_made_up_game_reads_and_its_player_holds_the_cards_asked_for() {
        let table = Table::new(opened(13));
        assert_eq!(table.yours().map(|s| s.seat), Some(2));
        let zones = table.state.state.card_zones();
        assert_eq!(zones["hand:2"].len(), 13);
        assert_eq!(zones["hand:1"].len(), 4);
        let turn = turn(&table.state.state);
        // Some of the hand may be played, and some not.
        assert!(!turn.options.is_empty() && turn.options.len() < 13);
        assert!(look().cards.is_some_and(|c| c.seated()));
        assert!(crate::card_table::on_table(&table.info, Some(&look())));
    }

    #[test]
    fn the_made_up_game_waits_bare_then_starts_with_every_seat_dealt_alike() {
        let waiting = Table::new(waiting());
        assert_eq!(waiting.state.status, "waiting");
        assert!(
            waiting
                .state
                .state
                .card_zones()
                .values()
                .all(|c| c.is_empty())
        );
        let table = Table::new(started(17));
        assert_eq!(
            (table.state.status.as_str(), table.state.move_seq),
            ("active", 0)
        );
        let zones = table.state.state.card_zones();
        for seat in 1..=3 {
            assert_eq!(zones[&format!("hand:{seat}")].len(), 17);
        }
        assert!(
            zones["hand:1"].iter().all(|c| c.id.is_empty()),
            "theirs unseen"
        );
        assert!(
            zones["hand:2"].iter().all(|c| !c.id.is_empty()),
            "the player's own"
        );
        assert!(zones["trick"].is_empty());
    }
}
