//! `--test-hand N`: a made-up card game of three seats on the table in place of a server's, the
//! player holding N cards of the usual deck, the other two a few each, and one card led to the
//! trick; those of the led suit may be played. It names no game and needs no server: it is for
//! looking at the player's hand (`hand_strip.rs`) and the table round it, with `--frames` and
//! `--screenshot`. A click on a card plays it as far as the client goes: the move is sent to
//! whatever server there is, so give the run a `--server` nobody listens on.

use bevy::prelude::*;
use serde_json::json;

use crate::appearance::{Appearance, Appearances};
use crate::login::Screen;
use crate::net::api::LegalSelection;
use crate::net::session::Opened;
use crate::net::{WatchStatus, open_table};
use crate::play::{Stage, Turn};

pub struct TestHandPlugin(pub usize);

impl Plugin for TestHandPlugin {
    fn build(&self, app: &mut App) {
        let cards = self.0;
        app.add_systems(
            Startup,
            move |mut appearances: ResMut<Appearances>,
                  mut screen: ResMut<Screen>,
                  mut watch: ResMut<WatchStatus>| {
                appearances.0.push(look());
                *screen = Screen::Main;
                open_table(&mut watch, opened(cards));
                watch.turn = Some(turn(&watch.table.as_ref().expect("laid").state.state));
            },
        );
    }
}

/// The made-up game's id, which no server has.
pub const GAME: &str = "test-hand";

/// Its look: a hand each seat holds, and a trick in the middle.
pub fn look() -> Appearance {
    Appearance::parse(&format!(
        r#"
        games = ["{GAME}"]
        [cards]
        play = "card-"
        [cards.piles.hand]
        seat = [0.0, 0.4]
        spread = "held"
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

/// The game opened, seen from its second seat, which holds `cards` cards (the deck's, from a
/// few spades on: some of the led suit, some not); the first seat has led a card.
pub fn opened(cards: usize) -> Opened {
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
    let hand: Vec<serde_json::Value> = (0..cards)
        .map(|i| json!({"id": card_id((i + 8) % 52)}))
        .collect();
    let others = |n: usize| vec![json!({}); n];
    let zones = json!([
        {"id": "hand", "owner": 1, "count": 4, "cards": others(4)},
        {"id": "hand", "owner": 2, "count": cards, "cards": hand},
        {"id": "hand", "owner": 3, "count": 5, "cards": others(5)},
        {"id": "trick", "count": 1, "cards": [{"id": card_id(1), "up": true}],
         "captions": ["North"]}
    ]);
    let state = json!({
        "state": {"cells": {}, "ext": {"view": {"table": zones}}},
        "status": "active", "move_seq": 1, "impulse_seat": 2
    });
    let game = json!({"game_def_id": GAME, "status": "active", "seats": [
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
}
