//! Setting up a new game: the player's seat, who plays each of the others (a built-in bot, or
//! nobody yet, for a person to join), how the game is timed, and whether it starts by itself once
//! every seat is taken or waits in its lobby for the player's start. Signed in, a seat can also
//! be kept for a friend (Bingle's ADR 0107): the server then keeps the game waiting for the
//! player's start, and invites the friend.
//!
//! [`Form`] is the setup with no Bevy in it; [`draw`] puts it on the list panel in place of the
//! lists while it is open. What the form needs from the server (the game's seats, the bots) is
//! read by the network thread, which hands it to [`Form::prepared`].

use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::appearance::Appearances;
use crate::browser::{Action, button, buttons, chip, colours, line, seat_dot};
use crate::fonts::UiFonts;
use crate::net::api::{BotInfo, GameInfo};
use crate::net::session::Problem;
use crate::net::{GameSettings, TimeControl};

/// The game being set up, while the form is open.
#[derive(Resource, Debug, Default)]
pub struct NewGame {
    pub form: Option<Form>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub def_id: String,
    pub name: String,
    /// The game's seats, once read.
    pub info: Option<GameInfo>,
    pub bots: Vec<BotInfo>,
    /// Why the game's seats or the bots could not be read.
    pub problem: Option<Problem>,
    /// This client's seat.
    pub mine: u32,
    /// Who plays each seat that is not this client's: nobody, until somebody joins, unless set.
    bots_at: BTreeMap<u32, String>,
    /// Seats kept for friends, by handle; a friend has one seat at most.
    friends_at: BTreeMap<u32, String>,
    pub time: TimeControl,
    /// Wait for this client's start, rather than start once every seat is taken.
    pub lobby: bool,
}

/// Who plays a seat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fill {
    You,
    /// Nobody yet: the seat waits for a person.
    Open,
    Bot(String),
    /// Kept for this friend, by handle.
    Friend(String),
}

/// The ways to time a game the form offers, by what they are called.
pub const TIMES: &[(&str, TimeControl)] = &[
    ("No clock", TimeControl::None),
    (
        "5 min + 3 s",
        TimeControl::Clock {
            seconds: 300,
            increment: 3,
        },
    ),
    (
        "10 min + 5 s",
        TimeControl::Clock {
            seconds: 600,
            increment: 5,
        },
    ),
    (
        "30 min",
        TimeControl::Clock {
            seconds: 1800,
            increment: 0,
        },
    ),
    ("1 min a move", TimeControl::PerMove { seconds: 60 }),
    ("1 day a move", TimeControl::PerMove { seconds: 86_400 }),
];

impl Form {
    pub fn new(def_id: &str, name: &str) -> Self {
        Form {
            def_id: def_id.to_string(),
            name: name.to_string(),
            info: None,
            bots: vec![],
            problem: None,
            mine: 1,
            bots_at: BTreeMap::new(),
            friends_at: BTreeMap::new(),
            time: TimeControl::None,
            lobby: false,
        }
    }

    /// The game's seats and the server's bots have been read: start from the first seat, with
    /// every other one open.
    pub fn prepared(&mut self, info: GameInfo, bots: Vec<BotInfo>) {
        self.mine = info.seats.first().map_or(1, |s| s.seat);
        self.info = Some(info);
        self.bots = bots;
        self.bots_at.clear();
        self.friends_at.clear();
        self.problem = None;
    }

    pub fn fill(&self, seat: u32) -> Fill {
        if seat == self.mine {
            return Fill::You;
        }
        if let Some(friend) = self.friends_at.get(&seat) {
            return Fill::Friend(friend.clone());
        }
        match self.bots_at.get(&seat) {
            Some(bot) => Fill::Bot(bot.clone()),
            None => Fill::Open,
        }
    }

    /// Who plays `seat` from now on. Taking another seat leaves the old one open, and keeping a
    /// seat for a friend takes them from any other; this client's seat cannot be left without
    /// one.
    pub fn set(&mut self, seat: u32, fill: Fill) {
        if fill != Fill::You && seat == self.mine {
            return;
        }
        self.bots_at.remove(&seat);
        self.friends_at.remove(&seat);
        match fill {
            Fill::You => self.mine = seat,
            Fill::Open => {}
            Fill::Bot(bot) => {
                self.bots_at.insert(seat, bot);
            }
            Fill::Friend(handle) => {
                self.friends_at.retain(|_, h| *h != handle);
                self.friends_at.insert(seat, handle);
            }
        }
    }

    /// Some seat waits for a person to join: anybody, or a friend it is kept for.
    pub fn waits_for_people(&self) -> bool {
        self.info.as_ref().is_some_and(|info| {
            info.seats
                .iter()
                .any(|s| matches!(self.fill(s.seat), Fill::Open | Fill::Friend(_)))
        })
    }

    /// A seat is kept for a friend, so the game waits for this client's start whatever was
    /// chosen (the server keeps it waiting).
    pub fn must_wait(&self) -> bool {
        !self.friends_at.is_empty()
    }

    /// The game waits for this client's start.
    pub fn waits_for_start(&self) -> bool {
        self.lobby || self.must_wait()
    }

    /// What to ask the server for, once the game's seats are known.
    pub fn settings(&self) -> Option<GameSettings> {
        self.info.as_ref()?;
        Some(GameSettings {
            game_def_id: self.def_id.clone(),
            seat: self.mine,
            bots: self.bots_at.clone(),
            time: self.time,
            lobby: self.waits_for_start(),
            reserved: self.friends_at.clone(),
        })
    }
}

/// The form, in place of the lists. `friends` are the handles of the signed-in account's
/// friends, who a seat can be kept for (none for a guest).
pub fn draw(
    area: &mut ChildSpawnerCommands,
    f: &UiFonts,
    form: &Form,
    friends: &[String],
    appearances: &Appearances,
) {
    area.spawn(line(
        f.semibold(17.0),
        colours::TEXT,
        &format!("New game: {}", form.name),
    ));
    if let Some(problem) = &form.problem {
        area.spawn(line(f.regular(13.0), colours::WARN, &problem.message()));
        area.spawn(buttons()).with_children(|row| {
            row.spawn(button(
                f,
                "Try again",
                Action::NewGame {
                    def_id: form.def_id.clone(),
                    name: form.name.clone(),
                },
            ));
            row.spawn(button(f, "Cancel", Action::CancelNewGame));
        });
        return;
    }
    let Some(info) = &form.info else {
        area.spawn(line(f.regular(14.0), colours::MUTED, "Reading the game…"));
        area.spawn(buttons()).with_children(|row| {
            row.spawn(button(f, "Cancel", Action::CancelNewGame));
        });
        return;
    };

    for seat in &info.seats {
        let filled = form.fill(seat.seat);
        area.spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            margin: UiRect::top(px(4)),
            ..default()
        })
        .with_children(|block| {
            block
                .spawn(Node {
                    column_gap: px(8),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(seat_dot(
                        appearances.seat_colour(&form.def_id, &seat.color, seat.seat),
                        12.0,
                    ));
                    row.spawn(line(f.semibold(14.0), colours::TEXT, &seat.label));
                });
            block.spawn(buttons()).with_children(|row| {
                let mut offer = |label: &str, fill: Fill| {
                    let chosen = filled == fill;
                    row.spawn(chip(
                        f,
                        label,
                        Action::SetSeat {
                            seat: seat.seat,
                            fill,
                        },
                        chosen,
                    ));
                };
                offer("You", Fill::You);
                offer("A person", Fill::Open);
                for bot in &form.bots {
                    offer(&bot.label, Fill::Bot(bot.id.clone()));
                }
                for friend in friends {
                    offer(&format!("@{friend}"), Fill::Friend(friend.clone()));
                }
            });
        });
    }

    area.spawn(line(f.semibold(14.0), colours::TEXT, "Time"));
    area.spawn(buttons()).with_children(|row| {
        for (label, time) in TIMES {
            row.spawn(chip(f, label, Action::SetTime(*time), form.time == *time));
        }
    });

    area.spawn(line(f.semibold(14.0), colours::TEXT, "Start"));
    area.spawn(buttons()).with_children(|row| {
        if !form.must_wait() {
            row.spawn(chip(
                f,
                "When every seat is taken",
                Action::SetLobby(false),
                !form.lobby,
            ));
        }
        row.spawn(chip(
            f,
            "When I say",
            Action::SetLobby(true),
            form.waits_for_start(),
        ));
    });

    if form.must_wait() {
        area.spawn(line(
            f.regular(13.0),
            colours::MUTED,
            "A seat kept for a friend: they are invited, and the game waits for your Start.",
        ));
    }
    let starts = match (form.waits_for_people(), form.waits_for_start()) {
        (true, false) => {
            "The seats for a person wait for somebody to join; the game starts once every seat \
             is taken. The room code to send them is shown when the game is made."
        }
        (true, true) => {
            "The seats for a person wait for somebody to join. The game waits for your Start; \
             until then seats can be changed, and bots put in them."
        }
        (false, false) => "The game starts at once.",
        (false, true) => "The game waits for your Start; until then seats can be changed.",
    };
    area.spawn(line(f.regular(13.0), colours::MUTED, starts));
    area.spawn(buttons()).with_children(|row| {
        row.spawn(button(f, "Make the game", Action::StartGame));
        row.spawn(button(f, "Cancel", Action::CancelNewGame));
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_seat_kept_for_a_friend_takes_them_from_any_other_and_makes_the_game_wait() {
        let mut form = form();
        form.set(2, Fill::Friend("bob".into()));
        assert_eq!(form.fill(2), Fill::Friend("bob".into()));
        assert!(form.waits_for_people(), "for the friend");
        assert!(form.must_wait() && form.waits_for_start() && !form.lobby);

        form.set(3, Fill::Friend("bob".into()));
        assert_eq!(form.fill(2), Fill::Open, "a friend has one seat");
        form.set(2, Fill::Bot("mcts".into()));
        form.set(1, Fill::Friend("cy".into()));
        assert_eq!(form.fill(1), Fill::You, "not this client's own seat");
        let settings = form.settings().unwrap();
        assert_eq!(settings.reserved, BTreeMap::from([(3, "bob".to_string())]));
        assert_eq!(settings.bots, BTreeMap::from([(2, "mcts".to_string())]));
        assert!(settings.lobby);

        form.set(3, Fill::You);
        assert!(!form.must_wait(), "taking the friend's seat lets it go");
        assert_eq!(form.fill(3), Fill::You);
    }

    use super::*;
    use crate::net::api::{Board, SeatDef};

    fn form() -> Form {
        let seat = |seat: u32, color: &str| SeatDef {
            seat,
            color: color.into(),
            label: color.into(),
            colors: vec![],
        };
        let mut form = Form::new("hex-7", "Hex (7×7)");
        assert_eq!(
            form.settings(),
            None,
            "nothing to make before the seats are known"
        );
        form.prepared(
            GameInfo {
                id: "hex-7".into(),
                name: "Hex (7×7)".into(),
                seats: vec![seat(1, "red"), seat(2, "blue"), seat(3, "green")],
                board: Board {
                    kind: "hex".into(),
                    positions: vec![],
                    regions: Default::default(),
                },
                actions: vec![],
                goals: vec![],
                pieces: vec![],
            },
            vec![BotInfo {
                id: "mcts".into(),
                label: "MCTS".into(),
            }],
        );
        form
    }

    #[test]
    fn a_new_game_starts_in_the_first_seat_with_the_others_waiting_for_people() {
        let form = form();
        assert_eq!(form.fill(1), Fill::You);
        assert_eq!(form.fill(2), Fill::Open);
        assert!(form.waits_for_people());
        assert_eq!(
            form.settings(),
            Some(GameSettings {
                game_def_id: "hex-7".into(),
                seat: 1,
                bots: BTreeMap::new(),
                time: TimeControl::None,
                lobby: false,
                reserved: BTreeMap::new(),
            })
        );
    }

    #[test]
    fn bots_fill_seats_and_moving_seats_leaves_the_old_one_open() {
        let mut form = form();
        form.set(1, Fill::Bot("mcts".into()));
        assert_eq!(
            form.fill(1),
            Fill::You,
            "this client's seat keeps it until it moves"
        );
        form.set(2, Fill::You);
        assert_eq!(form.fill(1), Fill::Open);
        form.set(1, Fill::Bot("mcts".into()));
        form.set(3, Fill::Bot("mcts".into()));
        assert!(!form.waits_for_people());
        form.set(3, Fill::You);
        assert_eq!(form.fill(2), Fill::Open);
        form.time = TIMES[1].1;
        form.lobby = true;
        let settings = form.settings().unwrap();
        assert!(settings.lobby);
        assert!(settings.reserved.is_empty());
        assert_eq!(settings.seat, 3);
        assert_eq!(settings.bots, BTreeMap::from([(1, "mcts".to_string())]));
        assert_eq!(
            settings.time,
            TimeControl::Clock {
                seconds: 300,
                increment: 3
            }
        );
    }
}
