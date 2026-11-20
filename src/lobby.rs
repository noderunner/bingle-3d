//! The lobby: a game that has not started, as a player at its table sees it. Anybody seated may
//! move to an open seat, put one of the server's bots in it, or leave; the host may also clear
//! another seat and, in a game that waits for them, start it once every seat is taken.
//!
//! [`offers`] is what the panel offers, with no Bevy in it; [`draw`] puts it on the game's panel.
//! The server decides what is allowed and says no otherwise: the offers only leave out what it
//! would certainly refuse.

use bevy::prelude::*;

use crate::browser::{Action, button, buttons, chip, colours, line};
use crate::fonts::UiFonts;
use crate::net::LobbyCall;
use crate::net::session::Problem;
use crate::table::Table;

/// What the lobby offers this client.
#[derive(Debug, Clone, PartialEq)]
pub struct Offers {
    /// What can be done with each seat, those with nothing to offer left out.
    pub seats: Vec<SeatOffers>,
    pub start: Start,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeatOffers {
    pub seat: u32,
    /// The seat's label, and who sits there if anybody does.
    pub label: String,
    /// Each choice: what its button says, and the call it makes.
    pub calls: Vec<(String, LobbyCall)>,
}

/// How the game will start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// By itself, once the last seat is taken.
    WhenFull,
    /// It waits for this client, the host, and every seat is taken.
    Ready,
    /// It waits for this client, the host, but a seat is open.
    SeatsOpen,
    /// It waits for the host, who is somebody else (named, when the host is known).
    ByHost(Option<String>),
}

/// What the lobby offers, while the game waits to start and this client has a seat at it.
pub fn offers(table: &Table) -> Option<Offers> {
    if !table.in_lobby() {
        return None;
    }
    let hosting = table.hosting();
    let mine = table.yours().map(|s| s.seat);
    let mut seats = Vec::new();
    for def in &table.info.seats {
        let seat = def.seat;
        let mut calls = Vec::new();
        let label;
        if table.is_open(seat) {
            label = def.label.clone();
            calls.push(("Sit here".to_string(), LobbyCall::Sit(seat)));
            for bot in &table.bots {
                calls.push((
                    bot.label.clone(),
                    LobbyCall::Bot {
                        seat,
                        bot: bot.id.clone(),
                    },
                ));
            }
        } else {
            label = format!("{}: {}", def.label, table.player(seat));
            if hosting && Some(seat) != mine {
                calls.push(("Remove".to_string(), LobbyCall::Remove(seat)));
            }
        }
        if !calls.is_empty() {
            seats.push(SeatOffers { seat, label, calls });
        }
    }
    let full = table.info.seats.iter().all(|s| !table.is_open(s.seat));
    let start = match (table.game.lobby, hosting, full) {
        (false, _, _) => Start::WhenFull,
        (true, true, true) => Start::Ready,
        (true, true, false) => Start::SeatsOpen,
        (true, false, _) => Start::ByHost(
            table
                .game
                .host_seat
                .filter(|&s| !table.is_open(s))
                .map(|s| table.player(s)),
        ),
    };
    Some(Offers { seats, start })
}

/// The lobby on the game's panel, with why the server last said no, if it did.
pub fn draw(p: &mut ChildSpawnerCommands, f: &UiFonts, table: &Table, problem: Option<&Problem>) {
    let Some(offers) = offers(table) else {
        return;
    };
    p.spawn(line(
        f.semibold(15.0),
        colours::TEXT,
        "Before the game starts",
    ));
    for seat in &offers.seats {
        p.spawn(line(f.regular(13.0), colours::MUTED, &seat.label));
        p.spawn(buttons()).with_children(|row| {
            for (label, call) in &seat.calls {
                row.spawn(chip(f, label, Action::Lobby(call.clone()), false));
            }
        });
    }
    let how = match &offers.start {
        Start::WhenFull => "The game starts once every seat is taken.".to_string(),
        Start::Ready => "Every seat is taken.".to_string(),
        Start::SeatsOpen => "Every seat must be taken before you can start: wait for \
                             somebody to join, or put a bot in a seat."
            .to_string(),
        Start::ByHost(Some(host)) => format!("{host} starts the game."),
        Start::ByHost(None) => "The host starts the game.".to_string(),
    };
    p.spawn(line(f.regular(13.0), colours::MUTED, &how));
    if let Some(problem) = problem {
        p.spawn(line(f.regular(13.0), colours::WARN, &problem.message()));
    }
    p.spawn(buttons()).with_children(|row| {
        if offers.start == Start::Ready {
            row.spawn(button(f, "Start", Action::Lobby(LobbyCall::Start)));
        }
        row.spawn(button(f, "Leave", Action::Lobby(LobbyCall::Leave)));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::api::{Board, BotInfo, Game, GameInfo, GameSeat, GameState, SeatDef};
    use crate::net::session::Opened;

    fn seat(seat: u32, name: &str, bot: &str, yours: bool) -> GameSeat {
        GameSeat {
            seat,
            color: format!("c{seat}"),
            name: name.into(),
            bot: bot.into(),
            yours,
        }
    }

    /// A three-seat game waiting to start, with this client (Ana) in seat 1.
    fn table(lobby: bool, host_seat: u32, seats: Vec<GameSeat>) -> Table {
        let def = |seat: u32| SeatDef {
            seat,
            color: format!("c{seat}"),
            label: format!("Seat {seat}"),
        };
        Table::new(Opened {
            room_code: "AB12CD".into(),
            info: GameInfo {
                id: "g".into(),
                name: "A game".into(),
                seats: vec![def(1), def(2), def(3)],
                board: Board {
                    kind: "hex".into(),
                    positions: vec![],
                    regions: Default::default(),
                },
                actions: vec![],
                goals: vec![],
            },
            game: Game {
                game_def_id: "g".into(),
                status: "waiting".into(),
                seats,
                invite_url: String::new(),
                clock: None,
                impulse_deadline: String::new(),
                server_time: String::new(),
                move_time_seconds: None,
                lobby,
                host_seat: Some(host_seat),
            },
            state: GameState {
                status: "waiting".into(),
                ..Default::default()
            },
            bots: vec![BotInfo {
                id: "mcts".into(),
                label: "MCTS".into(),
            }],
        })
    }

    fn calls(offers: &Offers, seat: u32) -> Vec<LobbyCall> {
        offers
            .seats
            .iter()
            .find(|s| s.seat == seat)
            .map(|s| s.calls.iter().map(|(_, c)| c.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn the_host_may_fill_open_seats_clear_others_and_start_once_full() {
        let t = table(
            true,
            1,
            vec![seat(1, "Ana", "", true), seat(2, "Ben", "", false)],
        );
        let o = offers(&t).unwrap();
        assert_eq!(
            calls(&o, 1),
            [],
            "nothing to do with one's own seat but leave"
        );
        assert_eq!(calls(&o, 2), [LobbyCall::Remove(2)]);
        assert_eq!(
            calls(&o, 3),
            [
                LobbyCall::Sit(3),
                LobbyCall::Bot {
                    seat: 3,
                    bot: "mcts".into()
                }
            ]
        );
        assert_eq!(o.seats[0].label, "Seat 2: Ben");
        assert_eq!(o.start, Start::SeatsOpen);

        let full = table(
            true,
            1,
            vec![
                seat(1, "Ana", "", true),
                seat(2, "Ben", "", false),
                seat(3, "Seat 3 (MCTS)", "mcts", false),
            ],
        );
        let o = offers(&full).unwrap();
        assert_eq!(o.start, Start::Ready);
        assert_eq!(calls(&o, 3), [LobbyCall::Remove(3)], "a bot is cleared too");
    }

    #[test]
    fn a_guest_at_the_table_may_move_and_fill_seats_but_not_clear_them_or_start() {
        let t = table(
            true,
            2,
            vec![seat(1, "Ana", "", true), seat(2, "Ben", "", false)],
        );
        let o = offers(&t).unwrap();
        assert_eq!(calls(&o, 2), []);
        assert_eq!(calls(&o, 3).len(), 2);
        assert_eq!(o.start, Start::ByHost(Some("Ben".into())));
    }

    #[test]
    fn a_game_without_a_lobby_starts_by_itself_and_one_started_has_none() {
        let mut t = table(
            false,
            1,
            vec![seat(1, "Ana", "", true), seat(2, "Ben", "", false)],
        );
        assert_eq!(offers(&t).unwrap().start, Start::WhenFull);
        t.state.status = "active".into();
        assert_eq!(offers(&t), None);
        t.state.status = "waiting".into();
        t.game.seats[0].yours = false;
        assert_eq!(offers(&t), None, "a spectator has no lobby");
    }
}
