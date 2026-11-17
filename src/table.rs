//! The game on the table, as the server last reported it, and how each event changes it.
//!
//! The client keeps no rules: every `move` and `state` event carries the whole position, so an
//! event replaces what is known. What the client does track is `move_seq`, so that an event that
//! is older than what it has is ignored, and one that skips ahead (a move missed) makes it load
//! the game again rather than trust a picture with a hole in it.

use crate::net::api::{Event, Game, GameInfo, GameState};
use crate::net::session::Opened;

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub room_code: String,
    pub info: GameInfo,
    pub game: Game,
    pub state: GameState,
    /// Where the last move was played, when it is known.
    pub last_move: Option<String>,
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
        Table {
            room_code: opened.room_code,
            info: opened.info,
            game: opened.game,
            state: opened.state,
            last_move: None,
        }
    }

    pub fn apply(&mut self, event: &Event) -> Step {
        let have = self.state.move_seq;
        match event {
            // Subscribed after more had been played than the state read: one was missed.
            Event::Joined { move_seq } if *move_seq > have => Step::Reload,
            Event::Joined { .. } => Step::Unchanged,
            Event::Move { seq, .. } if *seq <= have => Step::Unchanged,
            Event::Move { seq, .. } if *seq > have + 1 => Step::Reload,
            Event::Move { state, .. } => {
                self.last_move = arrived(&self.state, state);
                self.state = state.clone();
                Step::Changed
            }
            Event::State { state, .. } => {
                self.state = state.clone();
                // An undo or a new seat: the last move is no longer certain.
                self.last_move = None;
                Step::Changed
            }
            Event::Other(_) => Step::Unchanged,
        }
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
            "waiting" => "Waiting for players.".into(),
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
                    },
                    SeatDef {
                        seat: 2,
                        color: "blue".into(),
                        label: "Blue".into(),
                    },
                ],
                board: Board {
                    kind: "hex".into(),
                    positions: vec![],
                },
            },
            game: Game {
                game_def_id: "g".into(),
                seats: vec![GameSeat {
                    seat: 1,
                    color: "red".into(),
                    name: "Ana".into(),
                }],
            },
            state: state(move_seq, &[("a1", "red")]),
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
        assert_eq!(t.apply(&Event::Joined { move_seq: 2 }), Step::Reload);
        assert_eq!(t.apply(&Event::Joined { move_seq: 1 }), Step::Unchanged);

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
        };
        assert_eq!(t.apply(&undo), Step::Changed);
        assert_eq!(t.state.move_seq, 3);
        assert!(t.state.state.cells.is_empty());
        assert_eq!(t.last_move, None);
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
}
