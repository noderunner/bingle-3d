//! The game on the table, as the server last reported it, and how each event changes it.
//!
//! The client keeps no rules: every `move` and `state` event carries the whole position, so an
//! event replaces what is known. What the client does track is `move_seq`, so that an event that
//! is older than what it has is ignored, and one that skips ahead (a move missed) makes it load
//! the game again rather than trust a picture with a hole in it.

use std::time::Instant;

use crate::clock::Timing;
use crate::net::api::{BotInfo, Event, Game, GameInfo, GameSeat, GameState};
use crate::net::session::Opened;

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub room_code: String,
    pub info: GameInfo,
    pub game: Game,
    pub state: GameState,
    /// Where the last move was played, when it is known.
    pub last_move: Option<String>,
    /// The time each seat has, in a timed game.
    pub timing: Option<Timing>,
    /// The bots an open seat can be given before the game starts.
    pub bots: Vec<BotInfo>,
    /// The selections played, as the socket counts them (its first message, then each move's
    /// `seq`): what says an event is stale or that one was missed. Not the moves played
    /// (`state.move_seq`), of which each may be several selections. Until the socket says,
    /// taken to be the same.
    pub seq: Option<u64>,
}

/// What an event did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The table now stands as the event said.
    Changed,
    /// Nothing new: the event is about what the table already shows, or of a kind it ignores.
    Unchanged,
    /// The event shows that a move was missed: load the game's state again.
    Reload,
}

impl Table {
    pub fn new(opened: Opened) -> Self {
        let timing = Timing::new(
            opened.game.clock.clone(),
            &opened.game.impulse_deadline,
            &opened.game.server_time,
        );
        Table {
            timing,
            room_code: opened.room_code,
            info: opened.info,
            game: opened.game,
            state: opened.state,
            last_move: None,
            bots: opened.bots,
            seq: None,
        }
    }

    pub fn apply(&mut self, event: &Event) -> Step {
        let have = self.seq.unwrap_or(self.state.move_seq);
        match event {
            // The socket's own count of selections. More than the state read was taken to
            // have: one may have been missed (or a move was of several), so read it again.
            Event::Joined { move_seq } => {
                self.seq = Some(*move_seq);
                if *move_seq > have {
                    Step::Reload
                } else {
                    Step::Unchanged
                }
            }
            Event::Move { seq, .. } if *seq <= have => Step::Unchanged,
            Event::Move { seq, .. } if *seq > have + 1 => {
                // What is read again has this one in it.
                self.seq = Some(*seq);
                Step::Reload
            }
            Event::Move { seq, state } => {
                self.seq = Some(*seq);
                // Read before the event came, the state may be ahead of it already.
                if state.move_seq < self.state.move_seq {
                    return Step::Unchanged;
                }
                self.timed(state);
                self.last_move = arrived(&self.state, state);
                self.state = state.clone();
                Step::Changed
            }
            Event::State {
                state,
                seats,
                lobby,
                host_seat,
                ..
            } => {
                self.timed(state);
                self.state = state.clone();
                // A seat taken, changed or given up. The event does not say which seat is this
                // client's, so it is found by its player's name, which only one person at the
                // table goes by: after a move to another seat it is there, and after leaving, or
                // being cleared by the host, nowhere.
                if !seats.is_empty() {
                    let mine: Vec<String> = self
                        .game
                        .seats
                        .iter()
                        .filter(|s| s.yours)
                        .map(|s| s.name.clone())
                        .collect();
                    self.game.seats = seats
                        .iter()
                        .map(|s| GameSeat {
                            yours: s.bot.is_empty()
                                && mine.iter().any(|m| s.name.eq_ignore_ascii_case(m)),
                            ..s.clone()
                        })
                        .collect();
                }
                self.game.status = state.status.clone();
                self.game.lobby = *lobby;
                self.game.host_seat = *host_seat;
                // An undo or a new seat: the last move is no longer certain.
                self.last_move = None;
                Step::Changed
            }
            Event::Other(_) => Step::Unchanged,
        }
    }

    /// The time an event reports, if it reports any; the time known before stands otherwise.
    fn timed(&mut self, state: &GameState) {
        if let Some(timing) = Timing::new(
            state.clock.clone(),
            &state.impulse_deadline,
            &state.server_time,
        ) {
            self.timing = Some(timing);
        }
    }

    /// The time `seat` has left at `now`, in a timed game, if that seat's time is counted.
    pub fn time_left(&self, seat: u32, now: Instant) -> Option<f64> {
        let running = (self.state.status == "active")
            .then_some(self.state.impulse_seat)
            .flatten();
        self.timing.as_ref()?.left(seat, running, now)
    }

    /// The state read again after [`Step::Reload`].
    pub fn reloaded(&mut self, state: GameState) -> bool {
        if state.move_seq < self.state.move_seq || state == self.state {
            return false;
        }
        self.state = state;
        self.last_move = None;
        true
    }

    /// The seat this client plays, if it has one: of several (a game played against oneself),
    /// the one on impulse, else the first.
    pub fn yours(&self) -> Option<&GameSeat> {
        let mut mine = self.game.seats.iter().filter(|s| s.yours);
        let on_impulse = (self.state.status == "active")
            .then_some(self.state.impulse_seat)
            .flatten();
        mine.clone()
            .find(|s| Some(s.seat) == on_impulse)
            .or_else(|| mine.next())
    }

    /// The game has not started, and this client has a seat at it: what the lobby is for.
    pub fn in_lobby(&self) -> bool {
        self.state.status == "waiting" && self.yours().is_some()
    }

    /// This client's seat is the host's.
    pub fn hosting(&self) -> bool {
        self.yours()
            .is_some_and(|s| self.game.host_seat == Some(s.seat))
    }

    /// An open seat anybody may take: not kept for a friend.
    pub fn open_to_anybody(&self) -> bool {
        self.info
            .seats
            .iter()
            .any(|s| self.is_open(s.seat) && self.game.kept_for(s.seat).is_none())
    }

    /// Whether nobody, person or bot, sits at `seat` yet.
    pub fn is_open(&self, seat: u32) -> bool {
        !self
            .game
            .seats
            .iter()
            .any(|s| s.seat == seat && (!s.name.is_empty() || !s.bot.is_empty()))
    }

    /// The game is waiting for this client's seat.
    pub fn to_play(&self) -> bool {
        self.state.status == "active"
            && self
                .yours()
                .is_some_and(|s| self.state.impulse_seat == Some(s.seat))
    }

    /// A game against the server's bots, which can be played again.
    pub fn against_bots(&self) -> bool {
        self.yours().is_some() && self.game.seats.iter().any(|s| !s.bot.is_empty())
    }

    pub fn finished(&self) -> bool {
        self.state.status == "finished"
    }

    /// The name at a seat, or the seat's label when nobody sits there.
    pub fn player(&self, seat: u32) -> String {
        self.game
            .seats
            .iter()
            .find(|s| s.seat == seat && !s.name.is_empty())
            .map(|s| s.name.clone())
            .unwrap_or_else(|| self.seat_label(seat))
    }

    pub fn seat_label(&self, seat: u32) -> String {
        self.info
            .seats
            .iter()
            .find(|s| s.seat == seat)
            .map(|s| s.label.clone())
            .unwrap_or_else(|| format!("Seat {seat}"))
    }

    /// Where the game stands, in words.
    pub fn summary(&self) -> String {
        let s = &self.state;
        match s.status.as_str() {
            "finished" => {
                let how = match s.end_reason.as_str() {
                    "" => String::new(),
                    r => format!(" ({})", r.replace(['_', '-'], " ")),
                };
                if let Some(w) = s.winner_seat {
                    format!("{} won{how}.", self.player(w))
                } else if s.all_lost {
                    format!("Everybody lost{how}.")
                } else if !s.drawn_seats.is_empty() {
                    let names: Vec<String> =
                        s.drawn_seats.iter().map(|&x| self.player(x)).collect();
                    format!("A draw between {}{how}.", names.join(" and "))
                } else {
                    format!("The game is over{how}.")
                }
            }
            "active" => match s.impulse_seat {
                Some(seat) => format!("{} to move.", self.player(seat)),
                None => "Playing.".into(),
            },
            "waiting" if self.info.seats.iter().any(|d| self.is_open(d.seat)) => {
                "Waiting for players.".into()
            }
            "waiting" if self.game.lobby => "Waiting for the start.".into(),
            "waiting" => "Waiting.".into(),
            other => format!("{}.", capitalised(other)),
        }
    }
}

/// The one position whose stack changed and is not empty, if exactly one did: where a move put
/// a piece. A move that changes several (a capture, a slide) marks none, rather than guess.
fn arrived(before: &GameState, after: &GameState) -> Option<String> {
    let mut changed = after
        .state
        .cells
        .iter()
        .filter(|(id, stack)| !stack.is_empty() && before.state.cells.get(*id) != Some(stack));
    let first = changed.next()?.0.clone();
    changed.next().is_none().then_some(first)
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::api::{Board, GameSeat, PieceOnBoard, Position, SeatDef};

    fn stone(color: &str) -> PieceOnBoard {
        PieceOnBoard {
            piece: "stone".into(),
            color: color.into(),
        }
    }

    fn state(move_seq: u64, cells: &[(&str, &str)]) -> GameState {
        GameState {
            state: Position {
                cells: cells
                    .iter()
                    .map(|(id, c)| (id.to_string(), vec![stone(c)]))
                    .collect(),
                ..Default::default()
            },
            status: "active".into(),
            impulse_seat: Some(if move_seq.is_multiple_of(2) { 1 } else { 2 }),
            move_seq,
            ..Default::default()
        }
    }

    fn table(move_seq: u64) -> Table {
        Table::new(Opened {
            room_code: "AB12CD".into(),
            info: GameInfo {
                id: "g".into(),
                name: "A game".into(),
                seats: vec![
                    SeatDef {
                        seat: 1,
                        color: "red".into(),
                        label: "Red".into(),
                        colors: vec![],
                    },
                    SeatDef {
                        seat: 2,
                        color: "blue".into(),
                        label: "Blue".into(),
                        colors: vec![],
                    },
                ],
                board: Board {
                    kind: "hex".into(),
                    positions: vec![],
                    boards: vec![],
                    regions: Default::default(),
                },
                actions: vec![],
                goals: vec![],
                pieces: vec![],
                counters: vec![],
                opening: None,
                reserves: Default::default(),
                cards: None,
                engine: String::new(),
            },
            game: Game {
                game_def_id: "g".into(),
                status: "active".into(),
                invite_url: String::new(),
                clock: None,
                impulse_deadline: String::new(),
                server_time: String::new(),
                move_time_seconds: None,
                lobby: false,
                host_seat: Some(1),
                reservations: vec![],
                seats: vec![GameSeat {
                    seat: 1,
                    color: "red".into(),
                    name: "Ana".into(),
                    bot: String::new(),
                    yours: true,
                }],
            },
            state: state(move_seq, &[("a1", "red")]),
            bots: vec![],
        })
    }

    fn mv(seq: u64, cells: &[(&str, &str)]) -> Event {
        Event::Move {
            seq,
            state: state(seq, cells),
        }
    }

    #[test]
    fn the_next_move_is_applied_and_marks_where_it_went() {
        let mut t = table(1);
        assert_eq!(
            t.apply(&mv(2, &[("a1", "red"), ("b2", "blue")])),
            Step::Changed
        );
        assert_eq!(t.state.move_seq, 2);
        assert_eq!(t.last_move.as_deref(), Some("b2"));
        assert_eq!(t.state.state.cells.len(), 2);
    }

    #[test]
    fn a_move_already_shown_is_ignored() {
        let mut t = table(3);
        assert_eq!(t.apply(&mv(3, &[])), Step::Unchanged);
        assert_eq!(t.apply(&mv(1, &[])), Step::Unchanged);
        assert_eq!(t.state.state.cells.len(), 1, "nothing was taken away");
    }

    #[test]
    fn a_gap_asks_for_the_state_again() {
        let mut t = table(1);
        assert_eq!(t.apply(&mv(3, &[])), Step::Reload);
        assert_eq!(t.state.move_seq, 1, "the table is left as it was");
        // The socket subscribed after more had been played than the state read was taken to
        // have.
        assert_eq!(table(1).apply(&Event::Joined { move_seq: 2 }), Step::Reload);
        assert_eq!(
            table(1).apply(&Event::Joined { move_seq: 1 }),
            Step::Unchanged
        );

        assert!(t.reloaded(state(3, &[("a1", "red"), ("c3", "red")])));
        assert_eq!(t.state.move_seq, 3);
        assert!(
            !t.reloaded(state(2, &[])),
            "an older answer changes nothing"
        );
    }

    #[test]
    fn a_state_event_replaces_the_position_even_going_back() {
        let mut t = table(4);
        t.last_move = Some("a1".into());
        let undo = Event::State {
            reason: "undo".into(),
            state: state(3, &[]),
            seats: vec![],
            lobby: false,
            host_seat: Some(1),
        };
        assert_eq!(t.apply(&undo), Step::Changed);
        assert_eq!(t.state.move_seq, 3);
        assert!(t.state.state.cells.is_empty());
        assert_eq!(t.last_move, None);
    }

    #[test]
    fn a_seat_taken_shows_who_took_it_and_this_clients_seat_stays_its_own() {
        let mut t = table(0);
        t.state.status = "waiting".into();
        t.game.status = "waiting".into();
        let seat = |seat: u32, color: &str, name: &str| GameSeat {
            seat,
            color: color.into(),
            name: name.into(),
            bot: String::new(),
            yours: false,
        };
        let joined = Event::State {
            reason: "join".into(),
            state: state(0, &[]),
            seats: vec![seat(1, "red", "Ana"), seat(2, "blue", "Ben")],
            lobby: false,
            host_seat: Some(1),
        };
        assert_eq!(t.apply(&joined), Step::Changed);
        assert_eq!(t.player(2), "Ben");
        assert_eq!(t.yours().map(|s| s.seat), Some(1));
        assert_eq!(t.game.status, "active");
        assert!(t.to_play());
    }

    #[test]
    fn in_the_lobby_this_clients_seat_follows_its_name_from_seat_to_seat() {
        let mut t = table(0);
        t.state.status = "waiting".into();
        let seat = |seat: u32, name: &str, bot: &str| GameSeat {
            seat,
            color: if seat == 1 { "red" } else { "blue" }.into(),
            name: name.into(),
            bot: bot.into(),
            yours: false,
        };
        let event = |seats: Vec<GameSeat>, host_seat: Option<u32>| {
            let mut state = state(0, &[]);
            state.status = "waiting".into();
            state.impulse_seat = None;
            Event::State {
                reason: "seat".into(),
                state,
                seats,
                lobby: true,
                host_seat,
            }
        };
        assert!(t.in_lobby() && t.hosting());
        assert!(!t.is_open(1) && t.is_open(2));

        // Ana moves to the second seat; the first is open.
        t.apply(&event(vec![seat(2, "ana", "")], Some(2)));
        assert_eq!(t.yours().map(|s| s.seat), Some(2));
        assert!(t.game.lobby && t.hosting());
        assert!(t.is_open(1));
        assert_eq!(t.summary(), "Waiting for players.");

        // A bot is not this client, whatever it is called.
        t.apply(&event(
            vec![seat(1, "Ana", "mcts"), seat(2, "Ana", "")],
            Some(2),
        ));
        assert_eq!(t.yours().map(|s| s.seat), Some(2));
        assert!(!t.is_open(1));
        assert_eq!(t.summary(), "Waiting for the start.", "every seat is taken");

        // Cleared from the table by the host: watching it, no longer in its lobby.
        t.apply(&event(vec![seat(1, "Ben", "")], Some(1)));
        assert_eq!(t.yours(), None);
        assert!(!t.in_lobby() && !t.hosting());
    }

    #[test]
    fn the_summary_names_who_is_to_move_and_who_won() {
        let mut t = table(0);
        assert_eq!(t.summary(), "Ana to move.");
        t.state.impulse_seat = Some(2);
        assert_eq!(
            t.summary(),
            "Blue to move.",
            "an empty seat goes by its label"
        );
        t.state.status = "finished".into();
        t.state.impulse_seat = None;
        t.state.winner_seat = Some(1);
        t.state.end_reason = "connection".into();
        assert_eq!(t.summary(), "Ana won (connection).");
        t.state.winner_seat = None;
        t.state.drawn_seats = vec![1, 2];
        t.state.end_reason = String::new();
        assert_eq!(t.summary(), "A draw between Ana and Blue.");
        assert!(t.finished());
    }

    #[test]
    fn the_seat_this_client_plays_is_to_play_only_on_its_impulse() {
        let mut t = table(0);
        assert_eq!(t.yours().map(|s| s.seat), Some(1));
        assert!(t.to_play());
        t.state.impulse_seat = Some(2);
        assert!(!t.to_play());
        t.state.impulse_seat = Some(1);
        t.state.status = "finished".into();
        assert!(!t.to_play());
        t.game.seats[0].yours = false;
        assert_eq!(t.yours(), None);
        assert!(!t.against_bots());
    }

    #[test]
    fn a_client_holding_several_seats_plays_the_one_on_impulse() {
        let mut table = table(0);
        table.game.seats.push(GameSeat {
            seat: 2,
            color: "blue".into(),
            name: "Ana · Blue".into(),
            bot: String::new(),
            yours: true,
        });
        assert_eq!(table.yours().map(|s| s.seat), Some(1));
        assert!(table.to_play());
        assert_eq!(table.apply(&mv(1, &[("a1", "red")])), Step::Changed);
        assert_eq!(
            table.yours().map(|s| s.seat),
            Some(2),
            "the seat on impulse"
        );
        assert!(table.to_play());
        // Seats listed again by an event are still this client's, each by its own name.
        let seats = table.game.seats.clone();
        let event = Event::State {
            reason: "seat".into(),
            state: state(1, &[("a1", "red")]),
            seats: seats
                .iter()
                .map(|s| GameSeat {
                    yours: false,
                    ..s.clone()
                })
                .collect(),
            lobby: false,
            host_seat: Some(1),
        };
        table.apply(&event);
        assert!(table.game.seats.iter().all(|s| s.yours));
        // Over, or waiting: the first.
        table.state.status = "finished".into();
        assert_eq!(table.yours().map(|s| s.seat), Some(1));
    }

    /// A selection as the socket reports it: the `seq`th of the game, after which `played`
    /// moves have been played.
    fn selection(seq: u64, played: u64, cells: &[(&str, &str)]) -> Event {
        Event::Move {
            seq,
            state: GameState {
                move_seq: played,
                ply: played,
                ..state(played, cells)
            },
        }
    }

    #[test]
    fn a_move_of_several_selections_is_followed_by_the_sockets_count_not_the_moves_played() {
        // Opened after two moves, the first of two selections: the socket has counted three.
        let mut t = table(2);
        assert_eq!(t.apply(&Event::Joined { move_seq: 3 }), Step::Reload);
        assert!(
            !t.reloaded(state(2, &[("a1", "red")])),
            "nothing was missed"
        );
        // Two selections of one move: both are shown, and the move is played with the last.
        assert_eq!(t.apply(&selection(4, 2, &[("b2", "red")])), Step::Changed);
        assert_eq!((t.seq, t.state.move_seq), (Some(4), 2));
        assert_eq!(t.apply(&selection(5, 3, &[("c3", "red")])), Step::Changed);
        assert_eq!((t.seq, t.state.move_seq), (Some(5), 3));
        // Seen already; and a selection missed.
        assert_eq!(t.apply(&selection(5, 3, &[])), Step::Unchanged);
        assert_eq!(t.apply(&selection(7, 4, &[])), Step::Reload);
        assert!(t.reloaded(state(4, &[("d4", "red")])));
        assert_eq!(t.apply(&selection(8, 5, &[("e5", "red")])), Step::Changed);
        // A state read ahead of the socket is not taken back by what was played before it.
        let mut ahead = table(4);
        ahead.apply(&Event::Joined { move_seq: 4 });
        assert_eq!(ahead.apply(&selection(5, 3, &[])), Step::Unchanged);
        assert_eq!(ahead.state.move_seq, 4);
        assert_eq!(ahead.seq, Some(5));
    }
}
