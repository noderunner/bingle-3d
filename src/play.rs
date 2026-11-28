//! Playing a seat: the move being built, from the legal selections the server lists, as the
//! player makes gestures (Bingle's ADR 0114). The client decides nothing about legality: it asks
//! `GET …/selections` what may be selected, narrows that list by the gestures made, and sends the
//! move the list allowed, by its notation.
//!
//! A [`Turn`] is plain data with no Bevy in it, so the whole of entering a move is tested without
//! a window. The systems at the end turn clicks on the board, keys and the panel's buttons into
//! gestures and commands on it, and send what it asks for to the network thread.
//!
//! A right-click (a press and release of the right button with the pointer all but still, so a
//! right-drag still turns the camera) goes back, as Backspace does.
//!
//! A click on a position stands for the choices that must come before it when only one way to
//! get there is legal: in a game where a stone is placed by choosing the stone and then the cell,
//! a click on the cell is enough (the web page takes such forced choices by itself too). The
//! selection that ends the impulse is previewed, and waits for the player to confirm it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::input_focus::InputFocus;
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;

use crate::camera::{PIXELS_PER_LINE, PointerOverUi, WheelTaken};
use crate::cursor::{Feel, Sense, TableFeel, Under};
use crate::drag::{self, Drag};
use crate::escape_menu::{self, EscapeMenu};
use crate::net::api::{Gesture, LegalSelection, Position, Selections};
use crate::net::session::Problem;
use crate::net::{self, Net, WatchStatus};

pub struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hovering>()
            .init_resource::<Drag>()
            .init_resource::<WheelTaken>()
            .init_resource::<Under>()
            .init_resource::<TableFeel>()
            .init_resource::<SideAim>()
            .init_resource::<LineAim>()
            .add_observer(on_click)
            .add_observer(on_over)
            .add_observer(on_out)
            .add_systems(
                Update,
                (
                    start_turn.run_if(resource_changed::<WatchStatus>),
                    keys,
                    right_click,
                    take_wheel.run_if(resource_changed::<WatchStatus>),
                    wheel,
                    sense.in_set(Sense),
                ),
            );
    }
}

/// Where the turn stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Waiting for the server to say what may be selected next.
    Asking,
    /// The player is entering a selection.
    Choosing,
    /// The selections made are a whole move, shown and waiting for the player to confirm.
    Ready,
    /// The move has been sent.
    Sending,
    /// The server could not say what may be selected; the player may ask again.
    Failed,
}

/// What a gesture or command did, and what it needs from the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// It was not one the legal selections allow, or not now.
    Ignored,
    /// The move being built changed.
    Changed,
    /// A selection was made: ask the server what may follow it.
    Ask,
    /// The player confirmed the move: send it.
    Submit,
}

/// What a selection would stand on a position: the kind of piece, when the selections say, and
/// where it comes from, when it moves. `aimed` when it is what follows a move of two parts (an
/// arrow shot from where a piece landed), which the selections do not name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Arriving<'a> {
    pub piece: Option<&'a str>,
    pub from: Option<&'a str>,
    pub aimed: bool,
}

/// A piece the player has picked up: where from, and where it has been put, once that is
/// chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Carried<'a> {
    pub from: &'a str,
    pub to: Option<&'a str>,
}

/// A stack picked up to move (Tak's): where from, how many of its top pieces, and where each
/// has been dropped so far, bottom first. Its gestures are the position, the count, then a
/// position for each piece dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackMove<'a> {
    pub from: &'a str,
    pub count: usize,
    pub drops: Vec<&'a str>,
}

impl<'a> StackMove<'a> {
    /// The stack move these gestures make, if they make one: a position, then a count, then
    /// only positions.
    pub fn of(gestures: &'a [Gesture]) -> Option<Self> {
        let i = gestures.iter().position(|g| g.kind == "count")?;
        let from = gestures.get(i.checked_sub(1)?).filter(|g| g.is_pos())?;
        let rest = &gestures[i + 1..];
        if !rest.iter().all(Gesture::is_pos) {
            return None;
        }
        Some(StackMove {
            from: &from.value,
            count: gestures[i].value.parse().ok()?,
            drops: rest.iter().map(|g| g.value.as_str()).collect(),
        })
    }
}

/// The impulse this client's seat is on, and the move being built for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    /// The table's `move_seq` when the turn began, so an answer for an earlier one is known.
    pub asked_at: u64,
    /// The ply the server says the game is at, once it has answered.
    pub ply: Option<u64>,
    /// The selections made so far, in order.
    pub made: Vec<LegalSelection>,
    /// What could be selected before each of those, whether the move could end there, and the
    /// server's preview then, to go back to.
    before: Vec<(Vec<LegalSelection>, bool, Option<Position>)>,
    /// What may be selected now.
    pub options: Vec<LegalSelection>,
    /// The gestures made towards the next selection.
    pub gestures: Vec<Gesture>,
    /// How many of those were taken for the player because nothing else was possible: going
    /// back does not take them off.
    forced: usize,
    pub stage: Stage,
    /// Whether the selections made may be sent as they are while more may still be chosen.
    pub may_end: bool,
    /// Why the last attempt went wrong, for the player.
    pub problem: Option<Problem>,
    /// The position after the selections made, as the server previews it (`preview`).
    after: Option<Position>,
    /// Something taken in hand that a later click puts somewhere (a fence from its bowl): the
    /// choices it may become, one of which that click makes. Not a gesture: nothing is chosen
    /// until it is put down.
    taken: Option<Vec<Gesture>>,
    /// Whether what is taken is put down by a click on the board, which its choice follows (a
    /// tile from the stock, laid on a hex), rather than chosen next (a fence, put on a side).
    to_place: bool,
    /// Whether the piece every click moves is picked up (`Turn::lifted`).
    lifted: bool,
    /// Choices that are taken up by themselves once they are all that may be chosen next (the
    /// fences, once the piece has moved: the game's look's).
    pub auto_take: Vec<Gesture>,
    /// Whether every gesture is the player's own, none taken for them however few moves
    /// there are, and the move is played as soon as it is whole: a patience's, where a click
    /// puts the cards in hand where they go (`patience.rs`).
    pub clicks_only: bool,
    /// Whether the move is sent as soon as the server says it is whole: its last choice was
    /// made by *Play* (`Turn::end`), which stands for it.
    ends: bool,
}

impl Turn {
    pub fn new(asked_at: u64) -> Self {
        Turn {
            asked_at,
            ply: None,
            made: Vec::new(),
            before: Vec::new(),
            options: Vec::new(),
            gestures: Vec::new(),
            forced: 0,
            stage: Stage::Asking,
            may_end: false,
            problem: None,
            after: None,
            taken: None,
            to_place: false,
            lifted: false,
            auto_take: Vec::new(),
            clicks_only: false,
            ends: false,
        }
    }

    /// The notations of the selections made, as the server is asked about them.
    pub fn made_notations(&self) -> Vec<String> {
        self.made.iter().map(|s| s.notation.clone()).collect()
    }

    /// The move, as it is sent: its selections' notations joined by `; `.
    pub fn notation(&self) -> String {
        self.made_notations().join("; ")
    }

    /// Where the selections made put something new (a piece placed, an arrow shot), to show it
    /// there before it is sent. A piece moved is [`Turn::carried`].
    pub fn placed(&self) -> impl Iterator<Item = &str> {
        self.made.iter().flat_map(|s| {
            let put = s.to.as_deref().filter(|_| s.from.is_none());
            put.into_iter().chain(s.aim.as_deref())
        })
    }

    /// The pieces the move being built picks up, so the board can show them in the player's
    /// hand or where they have been put: each a selection made moves, and the one the gestures
    /// towards the next have picked up, once every selection they could still become moves
    /// that piece (and puts it in one place, once that has been clicked).
    pub fn carried(&self) -> Vec<Carried<'_>> {
        let mut out: Vec<Carried> = self
            .made
            .iter()
            .filter(|s| StackMove::of(&s.gestures).is_none())
            .filter_map(|s| {
                Some(Carried {
                    from: s.from.as_deref()?,
                    to: Some(s.to.as_deref()?),
                })
            })
            .collect();
        out.extend(self.entering());
        out
    }

    /// The piece the gestures towards the next selection have picked up, once every selection
    /// they could still become moves it from where it was clicked: put where every one of them
    /// puts it, once that has been clicked, or else where it stands part-way through a chain
    /// (`stands_at`).
    fn entering(&self) -> Option<Carried<'_>> {
        if self.stage != Stage::Choosing || StackMove::of(&self.gestures).is_some() {
            return None;
        }
        let clicked = |at: &str| self.gestures.contains(&Gesture::pos(at));
        let agreed = |of: fn(&LegalSelection) -> Option<&String>| {
            let mut all = self.matching().map(of);
            let first = all.next()??;
            all.all(|o| o == Some(first))
                .then_some(first.as_str())
                .filter(|at| clicked(at))
        };
        let from = agreed(|s| s.from.as_ref())?;
        Some(Carried {
            from,
            to: agreed(|s| s.to.as_ref()).or_else(|| self.stands_at()),
        })
    }

    /// The landings the player has clicked so far of the piece the selection being entered
    /// moves, in order, as far as every selection it could still become agrees on them: each
    /// lists where its piece lands in its `path` (a chain of jumps: c3, then e5). Empty before
    /// the first landing is clicked, and where the selections have no path. It reads no rules,
    /// only the positions the server lists.
    pub fn legs(&self) -> Vec<&str> {
        if self.stage != Stage::Choosing {
            return Vec::new();
        }
        let mut matching = self.matching();
        let Some(first) = matching.next() else {
            return Vec::new();
        };
        let mut common: &[String] = &first.path;
        for o in matching {
            let same = common
                .iter()
                .zip(&o.path)
                .take_while(|(a, b)| a == b)
                .count();
            common = &common[..same];
        }
        // Clicked in order: the piece's own position comes first, then each landing.
        let mut clicked = 0;
        for g in self.gestures.iter().filter(|g| g.is_pos()) {
            if common.get(clicked) == Some(&g.value) {
                clicked += 1;
            }
        }
        common[..clicked].iter().map(String::as_str).collect()
    }

    /// Where the piece the selection being entered moves stands part-way through a chain: the
    /// last of its [`Turn::legs`]. A piece dropped or clicked on its first landing stands
    /// there, whether the chain may go on from there or end.
    pub fn stands_at(&self) -> Option<&str> {
        self.legs().last().copied()
    }

    /// Where the piece in hand on the board is, lifted over its space, while there is one: the
    /// piece every click moves, picked up (`lifted`); a piece picked up by a click on it and
    /// not yet put anywhere; or a piece part-way through a chain, over where it stands
    /// (`stands_at`), while its next landing may be clicked. Never while only a choice that is
    /// not a click may follow (what happens to the piece jumped): the piece then stands where
    /// it landed. What a press there may drag on (`grabs`).
    pub fn held_at(&self) -> Option<&str> {
        if let Some(at) = self.lifted() {
            return Some(at);
        }
        let entering = self.entering()?;
        let Some(to) = entering.to else {
            return Some(entering.from);
        };
        let legs = self.legs();
        if legs.last() != Some(&to) {
            return None;
        }
        let n = self.gestures.len();
        self.matching()
            .any(|o| {
                o.gestures
                    .get(n)
                    .is_some_and(|g| g.is_pos() && o.path.get(legs.len()) == Some(&g.value))
            })
            .then_some(to)
    }

    /// Where the stack picked up to move is, while some of it is still carried: over its own
    /// space before any of it is dropped, then over the last space one was dropped on.
    pub fn stack_at(&self) -> Option<&str> {
        let lifted = StackMove::of(&self.gestures).filter(|_| self.stage == Stage::Choosing)?;
        if lifted.drops.len() >= lifted.count {
            return None;
        }
        Some(lifted.drops.last().copied().unwrap_or(lifted.from))
    }

    /// Where what is in hand on the board is, whatever it is: the piece in hand (`held_at`),
    /// or the stack carried (`stack_at`). What a drag carries from a space.
    pub fn in_hand_at(&self) -> Option<&str> {
        self.held_at().or_else(|| self.stack_at())
    }

    /// Whether a press on `at` may carry a piece from there under the pointer, a drag: what is
    /// in hand stands there (`in_hand_at`), or a click there would pick it up so (another piece
    /// in place of the one in hand, too; a stack whole). What a click does decides it, by the
    /// same code.
    pub fn grabs(&self, at: &str) -> bool {
        if self.stage != Stage::Choosing {
            return false;
        }
        if self.in_hand_at() == Some(at) {
            return true;
        }
        let mut clicked = self.clone();
        clicked.gesture(&Gesture::pos(at)) == Outcome::Changed && clicked.in_hand_at() == Some(at)
    }

    /// Whether a click on `cell` moves what is in hand (`in_hand_at`) there, so a drag may let
    /// it go there: a selection the click could become puts the piece there (its `to`, or one
    /// of its `path`'s landings: a stack's next drop), not only what it aims at (an arrow shot,
    /// a piece pinned). `occupied`: something stands on `cell`, which the piece every click
    /// moves (Courtfall's unit, `unclicked_from`) acts on (a strike, a shove) without going
    /// there.
    pub fn carries_to(&self, cell: &str, occupied: bool) -> bool {
        if self.in_hand_at().is_none()
            || (occupied && self.unclicked_from().is_some())
            || !self.targets().contains(cell)
        {
            return false;
        }
        let n = self.gestures.len();
        let click = Gesture::pos(cell);
        self.matching().any(|o| {
            o.gestures[n..].iter().find(|g| g.is_pos()) == Some(&click)
                && o.aim.as_deref() != Some(cell)
                && (o.to.as_deref() == Some(cell) || o.path.iter().any(|p| p == cell))
        })
    }

    /// The stacks the move being built moves: each selection made that moves one, and the one
    /// being entered.
    pub fn stack_moves(&self) -> Vec<StackMove<'_>> {
        let mut out: Vec<StackMove> = self
            .made
            .iter()
            .filter_map(|s| StackMove::of(&s.gestures))
            .collect();
        if self.stage == Stage::Choosing
            && let Some(lifted) = StackMove::of(&self.gestures)
        {
            out.push(lifted);
        }
        out
    }

    /// What the wheel changes now: how many of a stack picked up are carried, before any is
    /// dropped (more one way, fewer the other: those left go back down on it), or the kind of
    /// piece in hand, to another of the same pool (`pools`: each kind's; Tak's flat stone and
    /// standing stone). `up` is the wheel turned away from the player.
    pub fn wheel(&mut self, up: bool, pools: &HashMap<String, String>) -> Outcome {
        if self.stage != Stage::Choosing {
            return Outcome::Ignored;
        }
        let n = self.gestures.len();
        // The count of a stack picked up, while nothing of it has been dropped.
        if n > self.forced.max(1) && self.gestures[n - 1].kind == "count" {
            let before = &self.gestures[..n - 1];
            let mut counts: Vec<usize> = self
                .options
                .iter()
                .filter(|o| o.gestures.starts_with(before))
                .filter_map(|o| o.gestures.get(n - 1))
                .filter(|g| g.kind == "count")
                .filter_map(|g| g.value.parse().ok())
                .collect();
            counts.sort_unstable();
            counts.dedup();
            let now: usize = self.gestures[n - 1].value.parse().unwrap_or(0);
            let next = if up {
                counts.into_iter().find(|c| *c > now)
            } else {
                counts.into_iter().rev().find(|c| *c < now)
            };
            return match next {
                Some(count) => {
                    self.gestures[n - 1] = Gesture {
                        kind: "count".into(),
                        value: count.to_string(),
                    };
                    Outcome::Changed
                }
                None => Outcome::Ignored,
            };
        }
        // The kind of piece in hand, among those of its pool the move may place.
        let Some(piece) = self.in_hand() else {
            return Outcome::Ignored;
        };
        let Some(pool) = pools.get(piece).filter(|p| !p.is_empty()) else {
            return Outcome::Ignored;
        };
        let i = self
            .gestures
            .iter()
            .rposition(|g| g.kind == "piece")
            .expect("a piece in hand");
        let before = &self.gestures[..i];
        let mut kinds: Vec<&Gesture> = Vec::new();
        for o in self
            .options
            .iter()
            .filter(|o| o.gestures.starts_with(before))
        {
            if let Some(g) = o.gestures.get(i)
                && g.kind == "piece"
                && pools.get(&g.value) == Some(pool)
                && !kinds.contains(&g)
            {
                kinds.push(g);
            }
        }
        if kinds.len() < 2 {
            return Outcome::Ignored;
        }
        let at = kinds
            .iter()
            .position(|g| **g == self.gestures[i])
            .unwrap_or(0);
        let next = if up {
            (at + 1) % kinds.len()
        } else {
            (at + kinds.len() - 1) % kinds.len()
        };
        let next = kinds[next].clone();
        self.gestures.truncate(i);
        self.gestures.push(next);
        self.take_forced();
        Outcome::Changed
    }

    /// Whether the wheel would change anything now (so the camera leaves it alone): it also
    /// goes through the places what is taken up may go (`SideAim`).
    pub fn wheel_changes(&self, pools: &HashMap<String, String>) -> bool {
        if self.taken().is_some_and(|t| t.len() > 1) {
            return true;
        }
        [true, false]
            .iter()
            .any(|up| self.clone().wheel(*up, pools) != Outcome::Ignored)
    }

    /// Whether the player has clicked towards the selection being entered: a piece picked up,
    /// say, whose moves the board then lights.
    pub fn holding(&self) -> bool {
        self.stage == Stage::Choosing
            && (self.last_click().is_some()
                || self.in_hand().is_some()
                || self.taken.is_some()
                || self.lifted().is_some())
    }

    /// The position of the piece every click on the board would move, where the selections do
    /// not have it clicked: each says it moves from there, and none of their gestures is a click
    /// on it (Courtfall's unit, once activated: a click on where it goes, then what it does).
    /// Only before anything is chosen towards the selection.
    pub fn unclicked_from(&self) -> Option<&str> {
        if self.stage != Stage::Choosing || self.gestures.len() != self.forced {
            return None;
        }
        let n = self.gestures.len();
        let mut from: Option<&str> = None;
        for o in self.matching() {
            if !o.gestures[n..].iter().any(Gesture::is_pos) {
                continue;
            }
            // What a move aims (an arrow from where a piece landed) is not the piece moved.
            if o.aim.is_some() {
                return None;
            }
            let here = o.from.as_deref()?;
            if from.is_some_and(|f| f != here) || o.gestures.contains(&Gesture::pos(here)) {
                return None;
            }
            from = Some(here);
        }
        from
    }

    /// The piece picked up to move, by its position, while it is: a click on the piece
    /// [`Turn::unclicked_from`] names picks it up (and again puts it down), so it is held while
    /// the player chooses where it goes.
    pub fn lifted(&self) -> Option<&str> {
        self.unclicked_from().filter(|_| self.lifted)
    }

    /// What the player has taken to place (from its bowl or a seat's supply), while it waits
    /// for where it goes: the kind of piece, or the choice that places one (Fendo's `add`). A
    /// choice the player made, not one taken for them, and before any click on the board.
    pub fn in_hand(&self) -> Option<&str> {
        self.in_hand_choice().map(|g| g.value.as_str())
    }

    /// The choice [`Turn::in_hand`] names.
    pub fn in_hand_choice(&self) -> Option<&Gesture> {
        if self.stage != Stage::Choosing || self.gestures.iter().any(Gesture::is_pos) {
            return None;
        }
        let i = self
            .gestures
            .iter()
            .rposition(|g| g.kind == "piece" || g.kind == "action")?;
        if i < self.forced {
            return None;
        }
        // It waits for where it goes: every selection it could still become goes on to a
        // position.
        let mut matching = self.matching().peekable();
        matching.peek()?;
        matching
            .all(|o| o.gestures[i + 1..].iter().any(Gesture::is_pos))
            .then(|| &self.gestures[i])
    }

    /// The kind of piece the player holds without taking it from anywhere, at the start of a
    /// selection where every click on the board places one piece of that kind, and that is all
    /// it does (Go, Hex, Othello: a stone, waiting over its bowl). Choices that are not clicks
    /// (pass, swap) may be there too; they are the panel's. Empty when the selections do not
    /// name the kind.
    pub fn auto_hand(&self) -> Option<&str> {
        if self.stage != Stage::Choosing
            || self.gestures.len() != self.forced
            || self.taken.is_some()
        {
            return None;
        }
        let n = self.gestures.len();
        // Every selection with a click in it: the same choices before it (the stone), then the
        // click, and nothing after.
        let mut before: Option<&[Gesture]> = None;
        let mut piece: Option<&str> = None;
        for o in self.matching() {
            let rest = &o.gestures[n..];
            let Some(click) = rest.iter().position(Gesture::is_pos) else {
                continue;
            };
            if click + 1 != rest.len() || o.from.is_some() || o.to.is_none() {
                return None;
            }
            if before.is_some_and(|b| b != &rest[..click]) {
                return None;
            }
            before = Some(&rest[..click]);
            let kind = o.piece.as_deref().unwrap_or("");
            if piece.is_some_and(|p| p != kind) {
                return None;
            }
            piece = Some(kind);
        }
        piece
    }

    /// Where the pieces the selection being entered may move stand, before anything is chosen
    /// towards it, when every selection still possible that has a click in it moves a piece and
    /// begins with a click on it (a ring, then where it goes). Empty when any does something
    /// else with a click. Choices that are not clicks (a pass) are the panel's.
    pub fn moves_only(&self) -> Vec<&str> {
        if !self.untouched() {
            return Vec::new();
        }
        let n = self.gestures.len();
        let mut out: Vec<&str> = Vec::new();
        for o in self.matching() {
            let Some(click) = o.gestures[n..].iter().find(|g| g.is_pos()) else {
                continue;
            };
            match (o.from.as_deref(), &o.to) {
                (Some(from), Some(_)) if click.value == from => {
                    if !out.contains(&from) {
                        out.push(from);
                    }
                }
                _ => return Vec::new(),
            }
        }
        out
    }

    /// The selections still possible that would place something (not move it) on `cell`.
    pub fn could_place(&self, cell: &str) -> impl Iterator<Item = &LegalSelection> {
        self.matching()
            .filter(move |s| s.from.is_none() && s.to.as_deref() == Some(cell))
    }

    /// The selections made that place something (not move it): each says where, and by its
    /// gestures where from (a bowl's piece, a supply's choice).
    pub fn placings(&self) -> impl Iterator<Item = &LegalSelection> {
        self.made
            .iter()
            .filter(|s| s.from.is_none() && s.to.is_some())
    }

    /// Takes up something a later click puts somewhere (a fence from its bowl): one of
    /// `choices`, while one of them may be chosen next. Taken up again, it is put back;
    /// something else taken up instead takes its place.
    pub fn take_up(&mut self, choices: &[Gesture]) -> Outcome {
        self.take(choices, false)
    }

    /// Takes up a piece to place (a tile from the stock): one of `choices`, which a click on
    /// the board makes as well as the click, while a selection goes on with a click and then one
    /// of them (the hex, then the tile), or with one of them next. Again, it is put back.
    pub fn take_to_place(&mut self, choices: &[Gesture]) -> Outcome {
        self.take(choices, true)
    }

    fn take(&mut self, choices: &[Gesture], to_place: bool) -> Outcome {
        if self.stage != Stage::Choosing {
            return Outcome::Ignored;
        }
        let had = (self.taken.take(), self.to_place);
        if had.0.as_deref() == Some(choices) {
            return Outcome::Changed;
        }
        let n = self.gestures.len();
        if !self
            .matching()
            .any(|o| taking(o, n, choices, to_place).is_some())
        {
            (self.taken, self.to_place) = had;
            return Outcome::Ignored;
        }
        self.taken = Some(choices.to_vec());
        self.to_place = to_place;
        Outcome::Changed
    }

    /// What is taken up, while it is: the choices it may become that may be made next, or
    /// straight after the next click.
    pub fn taken(&self) -> Option<Vec<&Gesture>> {
        if self.stage != Stage::Choosing {
            return None;
        }
        let choices = self.taken.as_ref()?;
        let n = self.gestures.len();
        let mut next: Vec<&Gesture> = Vec::new();
        for o in self.matching() {
            if let Some((_, g)) = taking(o, n, choices, self.to_place)
                && !next.contains(&g)
            {
                next.push(g);
            }
        }
        (!next.is_empty()).then_some(next)
    }

    /// Whether what is taken up is put down by a click on the board, which makes its choice
    /// after the click (a tile laid on a hex), rather than chosen next (a fence put on a side):
    /// the board then shows it see-through where the pointer is, in place of that cell's light.
    pub fn taken_after_click(&self) -> bool {
        let Some(choices) = self.taken.as_ref().filter(|_| self.to_place) else {
            return false;
        };
        let n = self.gestures.len();
        self.matching()
            .any(|o| taking(o, n, choices, true).is_some_and(|(click, _)| click.is_some()))
    }

    /// What the selections made place (not move): where, and the kind of piece when they say.
    pub fn placements(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.made
            .iter()
            .filter(|s| s.from.is_none())
            .filter_map(|s| Some((s.to.as_deref()?, s.piece.as_deref())))
    }

    /// What the move being built would stand on `cell`: by the last selection made that puts
    /// something there, else by the selections a click there could still become, as far as they
    /// agree. `None` when it would put nothing there (a click that picks a piece up, say).
    pub fn arriving(&self, cell: &str) -> Option<Arriving<'_>> {
        let clicked = |at: &Option<String>| {
            at.as_ref()
                .is_some_and(|at| self.gestures.contains(&Gesture::pos(at)))
        };
        // What lands there by `s`: what follows its move, once the move has been clicked (or
        // made), or what it moves or places.
        fn lands<'s>(s: &'s LegalSelection, cell: &str, moved: bool) -> Option<Arriving<'s>> {
            if s.aim.as_deref() == Some(cell) && moved {
                return Some(Arriving {
                    aimed: true,
                    ..Arriving::default()
                });
            }
            (s.to.as_deref() == Some(cell)).then_some(Arriving {
                piece: s.piece.as_deref(),
                from: s.from.as_deref(),
                aimed: false,
            })
        }
        if let Some(made) = self.made.iter().rev().find_map(|s| lands(s, cell, true)) {
            return Some(made);
        }
        let mut options = self
            .matching()
            .filter_map(|s| lands(s, cell, clicked(&s.to)));
        let mut out = options.next()?;
        for other in options {
            if other.piece != out.piece {
                out.piece = None;
            }
            if other.from != out.from {
                out.from = None;
            }
            out.aimed &= other.aimed;
        }
        Some(out)
    }

    /// The position after the selections made, as the server previews it, while any are and
    /// the move is not yet played: what its view asks next, the positions it tints, the pieces
    /// it turns over. (While the server is asked about the last selection, the one before it.)
    pub fn preview(&self) -> Option<&Position> {
        let building = self.stage != Stage::Failed;
        self.after
            .as_ref()
            .filter(|_| building && !self.made.is_empty())
    }

    /// The server's previews while the move is built, the latest first: after the selections
    /// made (none once the move is whole: the server previews only what it asks next), then
    /// after each fewer. A piece the move takes away is found in the last that still had it,
    /// as it stood then (a disc turned over by the move, then removed with its line).
    pub fn previews(&self) -> Vec<&Position> {
        if self.stage == Stage::Failed || self.made.is_empty() {
            return Vec::new();
        }
        let earlier = self.before.iter().rev().filter_map(|(_, _, p)| p.as_ref());
        self.after.iter().chain(earlier).collect()
    }

    /// Whether nothing is chosen yet towards the selection being entered: no click, nothing
    /// in hand or taken up, nothing picked up.
    pub fn untouched(&self) -> bool {
        self.stage == Stage::Choosing
            && self.gestures.len() == self.forced
            && self.taken.is_none()
            && self.lifted().is_none()
    }

    /// The selections that may be chosen now by what they take, when every one still possible
    /// is: each takes more pieces than it is clicked on, and is clicked only where it takes (a
    /// line of pieces removed, named by its ends). The player chooses one by pointing at any of
    /// its pieces (`nearest_line`), and a click there makes all its clicks at once
    /// (`Turn::enter_all`). Empty otherwise, and once anything is clicked towards the selection.
    pub fn lines(&self) -> Vec<&LegalSelection> {
        if !self.untouched() {
            return Vec::new();
        }
        let n = self.gestures.len();
        let all: Vec<&LegalSelection> = self.matching().collect();
        let by_taking = |o: &&LegalSelection| {
            let clicks = &o.gestures[n..];
            !clicks.is_empty()
                && o.captures.len() > clicks.len()
                && clicks
                    .iter()
                    .all(|g| g.is_pos() && o.captures.contains(&g.value))
        };
        if all.is_empty() || !all.iter().all(by_taking) {
            return Vec::new();
        }
        all
    }

    /// The server's answer to the last question.
    pub fn answered(&mut self, selections: Selections) -> Outcome {
        if self.stage != Stage::Asking {
            return Outcome::Ignored;
        }
        let ends = std::mem::take(&mut self.ends);
        self.ply = Some(selections.ply);
        self.problem = None;
        self.after = selections.state;
        self.may_end = selections.may_end && !selections.made.is_empty();
        if selections.complete {
            self.stage = Stage::Ready;
            return if self.clicks_only || ends {
                Outcome::Submit
            } else {
                Outcome::Changed
            };
        }
        // A move that may end here is played as it is, as a whole one is: a patience's move
        // that turns no card up may be followed by more in the same impulse, but each click
        // that puts cards down is a move of its own.
        if self.clicks_only && self.may_end {
            self.stage = Stage::Ready;
            return Outcome::Submit;
        }
        self.options = selections.options;
        self.stage = Stage::Choosing;
        self.start_selection();
        self.settle()
    }

    /// The server could not answer.
    pub fn ask_failed(&mut self, problem: Problem) {
        self.stage = Stage::Failed;
        self.problem = Some(problem);
    }

    /// The move was refused: start the turn again from nothing, saying why.
    pub fn send_failed(&mut self, problem: Problem) {
        *self = Turn {
            problem: Some(problem),
            ..self.afresh()
        };
    }

    /// The same turn from nothing, asking again, with what the game's look set on it.
    pub fn afresh(&self) -> Turn {
        Turn {
            auto_take: self.auto_take.clone(),
            clicks_only: self.clicks_only,
            ..Turn::new(self.asked_at)
        }
    }

    /// A gesture from the player.
    pub fn gesture(&mut self, gesture: &Gesture) -> Outcome {
        match self.stage {
            Stage::Choosing => self.enter(gesture),
            Stage::Ready => {
                // A click on the preview plays it, as a second click does on the web page: where
                // the last selection puts something, or on what it takes away (a piece removed).
                if gesture.is_pos()
                    && self
                        .made
                        .last()
                        .and_then(|s| s.aim.as_ref().or(s.to.as_ref()).or(s.from.as_ref()))
                        == Some(&gesture.value)
                {
                    return Outcome::Submit;
                }
                // Anything else the last selection could have been instead replaces it.
                let mut instead = self.clone();
                instead.back();
                match instead.enter(gesture) {
                    Outcome::Ignored => Outcome::Ignored,
                    outcome => {
                        *self = instead;
                        outcome
                    }
                }
            }
            _ => Outcome::Ignored,
        }
    }

    /// The gestures of a whole selection at once, when one listed is exactly those after the
    /// gestures made: a play the player put together in an order of their own (a set of pins
    /// chosen, then the ball), sent in the server's. Nothing is taken for the player on the way.
    pub fn enter_all(&mut self, gestures: &[Gesture]) -> Outcome {
        if self.stage != Stage::Choosing || gestures.is_empty() {
            return Outcome::Ignored;
        }
        let mut after = self.gestures.clone();
        after.extend_from_slice(gestures);
        if !self.options.iter().any(|o| o.gestures == after) {
            return Outcome::Ignored;
        }
        self.taken = None;
        self.lifted = false;
        self.gestures = after;
        self.settle()
    }

    /// Back, as Backspace or the panel's button: first the gestures towards a selection, then the
    /// selections made, one at a time, each to just before its last click (a piece moved and
    /// its arrow not yet shot). Nothing is sent, so nothing needs undoing on the server.
    pub fn back(&mut self) -> bool {
        self.ends = false;
        // What is taken up, or picked up, goes back first.
        if self.taken.take().is_some() {
            return true;
        }
        if self.lifted().is_some() {
            self.lifted = false;
            return true;
        }
        match self.stage {
            Stage::Choosing if self.gestures.len() > self.forced => {
                self.gestures
                    .truncate(self.last_click().unwrap_or(self.forced));
                true
            }
            Stage::Choosing | Stage::Ready | Stage::Asking => match self.before.pop() {
                Some((options, may_end, after)) => {
                    self.may_end = may_end;
                    self.after = after;
                    let undone = self.made.pop();
                    self.options = options;
                    self.stage = Stage::Choosing;
                    self.start_selection();
                    // A line chosen whole goes back whole: nothing of it was clicked alone.
                    if let Some(s) = undone.filter(|_| self.lines().is_empty())
                        && let Some(last) = s.gestures.iter().rposition(Gesture::is_pos)
                    {
                        let before = &s.gestures[..last];
                        if before.len() > self.gestures.len()
                            && before.starts_with(&self.gestures)
                            && before.iter().any(Gesture::is_pos)
                        {
                            self.gestures = before.to_vec();
                        }
                    }
                    true
                }
                None => false,
            },
            Stage::Sending | Stage::Failed => false,
        }
    }

    /// Whether [`Turn::back`] would take anything back.
    pub fn can_go_back(&self) -> bool {
        match self.stage {
            Stage::Choosing => {
                self.taken.is_some()
                    || self.lifted().is_some()
                    || self.gestures.len() > self.forced
                    || !self.made.is_empty()
            }
            Stage::Ready => true,
            _ => false,
        }
    }

    /// Whether the move may be sent now: it is whole, or it may end where it is (what is half
    /// entered towards another selection is let go).
    pub fn can_play(&self) -> bool {
        self.can_send() || self.ending_choice().is_some()
    }

    /// Whether the selections made are a move that may be sent as they are.
    fn can_send(&self) -> bool {
        match self.stage {
            Stage::Ready => true,
            Stage::Choosing => self.may_end && !self.made.is_empty(),
            _ => false,
        }
    }

    /// The player confirms the move: what to send, and the ply it was made at.
    pub fn submit(&mut self) -> Option<(String, u64)> {
        let ply = self.ply?;
        if !self.can_send() {
            return None;
        }
        self.gestures.clear();
        self.stage = Stage::Sending;
        Some((self.notation(), ply))
    }

    /// The one choice that *Play* makes, when something has been chosen towards the selection
    /// being entered (not at its start, where the one choice may be a pass or a swap), exactly
    /// one choice that is not a click on the board is offered, and making it completes the
    /// selection: the choice that ends a chain, or a step that pins nothing. The game's own
    /// words for it are never read.
    pub fn ending_choice(&self) -> Option<Gesture> {
        let n = self.gestures.len();
        if self.stage != Stage::Choosing
            || self.clicks_only
            || n <= self.forced
            || self.taken.is_some()
        {
            return None;
        }
        let mut choices = self.choices().into_iter();
        let (Some(choice), None) = (choices.next(), choices.next()) else {
            return None;
        };
        let mut leading = self
            .matching()
            .filter(|o| o.gestures.get(n) == Some(&choice));
        match (leading.next(), leading.next()) {
            (Some(only), None) if only.gestures.len() == n + 1 => Some(choice),
            _ => None,
        }
    }

    /// *Play* where it ends the selection: makes the choice `ending_choice` names, and the move
    /// is sent once the server says it is whole. If it does not, nothing is sent and the turn
    /// goes on as it would have after that choice's button.
    pub fn end(&mut self) -> Outcome {
        let Some(choice) = self.ending_choice() else {
            return Outcome::Ignored;
        };
        let outcome = self.enter(&choice);
        self.ends = outcome == Outcome::Ask;
        outcome
    }

    /// The positions a click on would be taken, for the board to show. With something taken up
    /// that a click puts down, only where it may go.
    pub fn targets(&self) -> BTreeSet<String> {
        if self.stage != Stage::Choosing {
            return BTreeSet::new();
        }
        let lines = self.lines();
        if !lines.is_empty() {
            return lines.iter().flat_map(|o| o.captures.clone()).collect();
        }
        let n = self.gestures.len();
        if self.taken_after_click() {
            let choices = self.taken.as_deref().unwrap_or_default();
            return self
                .matching()
                .filter_map(|o| taking(o, n, choices, true)?.0)
                .map(|g| g.value.clone())
                .collect();
        }
        let candidates: BTreeSet<&Gesture> = self
            .matching()
            .filter_map(|o| o.gestures[n..].iter().find(|g| g.is_pos()))
            .collect();
        candidates
            .into_iter()
            .filter(|g| self.path_to(g).is_some())
            .map(|g| g.value.clone())
            .collect()
    }

    /// The positions whose pieces the move being built takes, as far as is known (API 1.10's
    /// `captures`): those the selections made take, and once the player has chosen something
    /// towards the next, those every selection it could still become takes (a jump's piece
    /// captured, not spared; a pin's target once clicked is a selection made).
    pub fn taking(&self) -> BTreeSet<&str> {
        let mut out: BTreeSet<&str> = self
            .made
            .iter()
            .flat_map(|s| s.captures.iter().map(String::as_str))
            .collect();
        if self.stage == Stage::Choosing && self.gestures.len() > self.forced {
            let mut each = self.matching().map(|o| {
                o.captures
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>()
            });
            if let Some(first) = each.next() {
                out.extend(each.fold(first, |all, o| &all & &o));
            }
        }
        out
    }

    /// The positions whose pieces the move being built turns over where they stand, and the
    /// color word each turns to, as far as is known (API 1.20's `converts_to`): as `taking`,
    /// those the selections made turn over, and once something is chosen towards the next, those
    /// every selection it could still become turns to the same color.
    pub fn turning(&self) -> BTreeMap<&str, &str> {
        let mut out: BTreeMap<&str, &str> = self
            .made
            .iter()
            .flat_map(|s| {
                s.converts_to
                    .iter()
                    .map(|(c, to)| (c.as_str(), to.as_str()))
            })
            .collect();
        if self.stage == Stage::Choosing && self.gestures.len() > self.forced {
            let mut each = self.matching().map(|o| &o.converts_to);
            if let Some(first) = each.next() {
                let mut common: BTreeMap<&str, &str> = first
                    .iter()
                    .map(|(c, to)| (c.as_str(), to.as_str()))
                    .collect();
                for o in each {
                    common.retain(|c, to| o.get(*c).is_some_and(|t| t == to));
                }
                out.extend(common);
            }
        }
        out
    }

    /// Whether a click on `cell` takes the piece standing there: every selection a click there
    /// would go on to takes it (a pin's target; a capture where the piece lands).
    pub fn takes_at(&self, cell: &str) -> bool {
        // A line chosen by what it takes is lit as the player's choice, not as a capture.
        if self.stage != Stage::Choosing || !self.lines().is_empty() {
            return false;
        }
        let n = self.gestures.len();
        let click = Gesture::pos(cell);
        let mut there = self
            .matching()
            .filter(|o| o.gestures[n..].iter().find(|g| g.is_pos()) == Some(&click))
            .peekable();
        there.peek().is_some() && there.all(|o| o.captures.iter().any(|c| c == cell))
    }

    /// Whether a click on `at` would pick up what stands there, to move it: before anything is
    /// clicked towards the selection, a selection still possible moves a piece from there and
    /// begins with a click on it; or it is the piece every click moves, not yet lifted.
    pub fn picks_up(&self, at: &str) -> bool {
        if self.stage != Stage::Choosing || self.holding() {
            return false;
        }
        if self.unclicked_from() == Some(at) {
            return true;
        }
        let n = self.gestures.len();
        let click = Gesture::pos(at);
        self.targets().contains(at)
            && self.matching().any(|o| {
                o.from.as_deref() == Some(at)
                    && o.gestures[n..].iter().find(|g| g.is_pos()) == Some(&click)
            })
    }

    /// The choices that are not positions (a piece, a count, an action such as a swap), for the
    /// panel to offer as buttons, in the order the server lists them. A choice that a click on
    /// the board makes by itself is left out: where only stones can be placed, a click on a cell
    /// is enough, and a button for the stone would only be in the way.
    pub fn choices(&self) -> Vec<Gesture> {
        if self.stage != Stage::Choosing {
            return Vec::new();
        }
        let n = self.gestures.len();
        let mut out: Vec<Gesture> = Vec::new();
        for o in self.matching() {
            if let Some(g) = o.gestures.get(n)
                && !g.is_pos()
                && !out.contains(g)
                && !self.clicked_into(g)
            {
                out.push(g.clone());
            }
        }
        out
    }

    /// The choices the panel offers as buttons: `choices`, but for the one *Play* makes.
    pub fn panel_choices(&self) -> Vec<Gesture> {
        let ending = self.ending_choice();
        let mut out = self.choices();
        out.retain(|c| Some(c) != ending.as_ref());
        out
    }

    /// Whether every selection that `choice` leads to is reached by a click on the board, which
    /// makes the choice for the player.
    fn clicked_into(&self, choice: &Gesture) -> bool {
        let n = self.gestures.len();
        self.matching()
            .filter(|o| o.gestures.get(n) == Some(choice))
            .all(|o| {
                o.gestures[n..]
                    .iter()
                    .find(|g| g.is_pos())
                    .and_then(|pos| self.path_to(pos))
                    .is_some_and(|path| path.first() == Some(choice))
            })
    }

    /// The selections the gestures made so far could still become.
    fn matching(&self) -> impl Iterator<Item = &LegalSelection> {
        self.options
            .iter()
            .filter(|o| o.gestures.starts_with(&self.gestures))
    }

    /// The gestures `gesture` stands for, from here: itself, after any choices that are not
    /// positions which the one way to reach it needs. `None` when no selection goes that way, or
    /// when several would by different choices (then the player makes the choice first).
    fn path_to(&self, gesture: &Gesture) -> Option<Vec<Gesture>> {
        let n = self.gestures.len();
        let mut found: Option<&[Gesture]> = None;
        for o in self.matching() {
            let rest = &o.gestures[n..];
            let Some(i) = rest.iter().position(|g| g == gesture) else {
                continue;
            };
            let path = &rest[..=i];
            // Only choices from a list may be taken for the player, never another click.
            if path[..i].iter().any(Gesture::is_pos) {
                continue;
            }
            match found {
                None => found = Some(path),
                Some(f) if f == path => {}
                Some(_) => return None,
            }
        }
        found.map(<[Gesture]>::to_vec)
    }

    /// Where in the gestures the last click on a position towards this selection is.
    fn last_click(&self) -> Option<usize> {
        let i = self.gestures.iter().rposition(Gesture::is_pos)?;
        (i >= self.forced).then_some(i)
    }

    fn enter(&mut self, gesture: &Gesture) -> Outcome {
        // The piece every click moves, clicked: picked up, or put down again.
        if let Some(from) = self.unclicked_from()
            && *gesture == Gesture::pos(from)
            && self.path_to(gesture).is_none()
        {
            self.lifted = !self.lifted;
            return Outcome::Changed;
        }
        // Picked up, a click where it may not go leaves it in hand.
        if self.lifted().is_some() && gesture.is_pos() && self.path_to(gesture).is_none() {
            return Outcome::Ignored;
        }
        self.lifted = false;
        // A click again on the position last clicked, where it means nothing more, takes that
        // click back: a piece picked up is put down again.
        if let Some(i) = self.last_click()
            && self.gestures[i] == *gesture
            && self.path_to(gesture).is_none()
        {
            self.gestures.truncate(i);
            return Outcome::Changed;
        }
        // What is taken up and put down by a click on the board: a click where it may go makes
        // its choice too; anywhere else, nothing (it stays in hand).
        if gesture.is_pos() && self.taken_after_click() {
            let n = self.gestures.len();
            let choices = self.taken.as_deref().unwrap_or_default();
            let made: BTreeSet<Gesture> = self
                .matching()
                .filter_map(|o| match taking(o, n, choices, true)? {
                    (Some(click), choice) if click == gesture => Some(choice.clone()),
                    _ => None,
                })
                .collect();
            // One choice, or several the player must pick between (on the panel).
            let mut made = made.into_iter();
            let (Some(choice), None) = (made.next(), made.next()) else {
                return Outcome::Ignored;
            };
            self.taken = None;
            self.gestures.extend([gesture.clone(), choice]);
            self.take_forced();
            return self.settle();
        }
        self.taken = None;
        // So does the same choice again (a stone's bowl clicked twice): the stone goes back.
        if (gesture.kind == "piece" || gesture.kind == "action")
            && self.gestures.len() > self.forced
            && self.gestures.last() == Some(gesture)
            && self.path_to(gesture).is_none()
        {
            self.gestures.pop();
            return Outcome::Changed;
        }
        let path = match self.path_to(gesture) {
            Some(path) => path,
            // A click that does not follow on from the gestures made may start afresh (another
            // piece to move, say).
            None if self.gestures.len() > self.forced => {
                let mut fresh = self.clone();
                fresh.gestures.truncate(self.forced);
                match fresh.path_to(gesture) {
                    Some(path) => {
                        self.gestures.truncate(self.forced);
                        path
                    }
                    None => return Outcome::Ignored,
                }
            }
            None => return Outcome::Ignored,
        };
        self.gestures.extend(path);
        self.take_forced();
        self.take_all();
        let outcome = self.settle();
        if outcome == Outcome::Changed {
            self.take_if_only();
        }
        outcome
    }

    /// Takes up what `auto_take` names once nothing else may be chosen next: a fence, once the
    /// piece it is built beside has moved.
    fn take_if_only(&mut self) {
        let n = self.gestures.len();
        if self.taken.is_some() || self.auto_take.is_empty() || n <= self.forced {
            return;
        }
        let only = {
            let mut next = self.matching().map(|o| o.gestures.get(n)).peekable();
            next.peek().is_some() && next.all(|g| g.is_some_and(|g| self.auto_take.contains(g)))
        };
        if only {
            self.taken = Some(self.auto_take.clone());
        }
    }

    /// A stack just picked up is picked up whole, as much of it as may be carried: every
    /// selection still possible moves from where it was clicked and next says how many. The
    /// wheel then leaves some behind.
    fn take_all(&mut self) {
        let n = self.gestures.len();
        let Some(from) = self.gestures.last().filter(|g| g.is_pos()) else {
            return;
        };
        let mut most: Option<(usize, &Gesture)> = None;
        for o in self.matching() {
            let Some(g) = o.gestures.get(n).filter(|g| g.kind == "count") else {
                return;
            };
            if o.from.as_deref() != Some(from.value.as_str()) {
                return;
            }
            let count = g.value.parse().unwrap_or(0);
            if most.is_none_or(|(m, _)| count > m) {
                most = Some((count, g));
            }
        }
        if let Some((_, g)) = most {
            let g = g.clone();
            self.gestures.push(g);
        }
    }

    /// Starts entering a selection from the options there are.
    fn start_selection(&mut self) {
        self.taken = None;
        self.lifted = false;
        self.gestures.clear();
        self.take_forced();
        self.forced = self.gestures.len();
    }

    /// Takes a choice from a list for the player when every selection still possible makes it.
    fn take_forced(&mut self) {
        if self.clicks_only {
            return;
        }
        loop {
            let n = self.gestures.len();
            let forced = {
                let mut next = self.matching().map(|o| o.gestures.get(n));
                match next.next() {
                    Some(Some(first)) if !first.is_pos() && next.all(|g| g == Some(first)) => {
                        first.clone()
                    }
                    _ => return,
                }
            };
            self.gestures.push(forced);
        }
    }

    /// Makes the selection once the gestures spell out exactly one.
    fn settle(&mut self) -> Outcome {
        let chosen = {
            let mut matching = self.matching();
            match (matching.next(), matching.next()) {
                (Some(only), None) if only.gestures.len() == self.gestures.len() => only.clone(),
                _ => return Outcome::Changed,
            }
        };
        self.before.push((
            std::mem::take(&mut self.options),
            self.may_end,
            self.after.clone(),
        ));
        self.made.push(chosen);
        self.gestures.clear();
        self.stage = Stage::Asking;
        Outcome::Ask
    }
}

/// How `option` takes up one of `choices` from its gesture `n`: that choice next, or (for a
/// piece taken `to_place`) a click and then that choice. The click, if any, and the choice.
fn taking<'o>(
    option: &'o LegalSelection,
    n: usize,
    choices: &[Gesture],
    to_place: bool,
) -> Option<(Option<&'o Gesture>, &'o Gesture)> {
    let next = option.gestures.get(n)?;
    if choices.contains(next) {
        return Some((None, next));
    }
    let then = option.gestures.get(n + 1).filter(|_| to_place)?;
    (next.is_pos() && choices.contains(then)).then_some((Some(next), then))
}

/// Of the lines that may be chosen (`Turn::lines`), the one the pointer points at when it is
/// over the piece at `at`: of those that take it, the one whose middle is nearest `pointer`, so
/// that along a longer line the pieces chosen follow the pointer. `centre` is where a position
/// is on the board.
pub fn nearest_line<'a>(
    lines: &[&'a LegalSelection],
    at: &str,
    pointer: (f32, f32),
    centre: impl Fn(&str) -> Option<(f32, f32)>,
) -> Option<&'a LegalSelection> {
    let middle = |o: &LegalSelection| {
        let points: Vec<(f32, f32)> = o.captures.iter().filter_map(|c| centre(c)).collect();
        let n = points.len().max(1) as f32;
        let (x, z) = points
            .iter()
            .fold((0.0, 0.0), |(x, z), p| (x + p.0, z + p.1));
        (x / n, z / n)
    };
    let far = |o: &LegalSelection| {
        let (x, z) = middle(o);
        (x - pointer.0).powi(2) + (z - pointer.1).powi(2)
    };
    lines
        .iter()
        .filter(|o| o.captures.iter().any(|c| c == at))
        .min_by(|a, b| far(a).total_cmp(&far(b)))
        .copied()
}

/// The line a click on the board would choose (`Turn::lines`), the one the pointer points at
/// (`nearest_line`), with its pieces lit; the board says.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct LineAim(pub Option<LegalSelection>);

/// The position the pointer is over, if it is one on the board.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct Hovering(pub Option<String>);

/// A choice a click on something other than the board makes (a stone's bowl: a stone of its
/// color).
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Choice(pub Gesture);

/// Which of the places what is taken up may go a click would put it (a side of a square, for
/// a fence): the one nearest the pointer, or the one the wheel turned to. The board says.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct SideAim {
    pub choice: Option<Gesture>,
    /// Turned to with the wheel: it stands until the pointer comes near another.
    pub by_wheel: bool,
}

/// Something a click takes up to put down with a later click (a bowl of fences): one of these
/// choices, which the later click makes.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct TakeUp(pub Vec<Gesture>);

/// A piece a click takes up to place, by the choices it may become (a tile in the stock): a
/// click on the board then lays it (`Turn::take_to_place`).
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct TakeToPlace(pub Vec<Gesture>);

/// A position of the board, on its cell and on every piece that stands there: what a click on
/// either means.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Spot(pub String);

/// When this client's seat is on impulse, a turn begins and the server is asked what it may
/// select; when it is not, there is no turn.
pub(crate) fn start_turn(
    net: Res<Net>,
    appearances: Option<Res<crate::appearance::Appearances>>,
    mut watch: ResMut<WatchStatus>,
) {
    let seq = watch.table.as_ref().map(|t| t.state.move_seq);
    let ours = watch.table.as_ref().is_some_and(|t| t.to_play());
    match (&watch.turn, ours) {
        (Some(_), false) => watch.turn = None,
        (Some(turn), true) if Some(turn.asked_at) == seq => {}
        (_, true) => {
            let mut turn = Turn::new(seq.unwrap_or_default());
            // The fences of a game whose look has them are taken up by themselves.
            turn.auto_take = watch
                .table
                .as_ref()
                .and_then(|t| appearances.as_ref()?.for_game(&t.info.id)?.fences.as_ref())
                .map(|f| {
                    f.sides
                        .keys()
                        .map(|id| Gesture {
                            kind: "action".into(),
                            value: id.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            turn.clicks_only = watch.table.as_ref().is_some_and(|t| {
                let look = appearances.as_ref().and_then(|a| a.for_game(&t.info.id));
                crate::card_table::on_table(&t.info, look)
            });
            watch.turn = Some(turn);
            net::ask(&net, &watch);
        }
        (None, false) => {}
    }
}

/// Does what an outcome needs, after a gesture or a command changed the turn.
pub fn follow(net: &Net, watch: &mut ResMut<WatchStatus>, outcome: Outcome) {
    match outcome {
        Outcome::Ignored | Outcome::Changed => {}
        Outcome::Ask => net::ask(net, watch),
        Outcome::Submit => submit(net, watch),
    }
}

// These take the resource itself, so that one that changes nothing (a click on a cell that is
// not lit, a key with no move shown) does not rebuild the panel and the board.

/// A gesture, from the board or the panel. What it did.
pub fn gesture(net: &Net, watch: &mut ResMut<WatchStatus>, gesture: &Gesture) -> Outcome {
    let Some(turn) = watch.bypass_change_detection().turn.as_mut() else {
        return Outcome::Ignored;
    };
    let outcome = turn.gesture(gesture);
    if outcome != Outcome::Ignored {
        watch.set_changed();
    }
    follow(net, watch, outcome);
    outcome
}

/// A whole selection's gestures at once (`Turn::enter_all`). What it did.
pub fn enter_all(net: &Net, watch: &mut ResMut<WatchStatus>, gestures: &[Gesture]) -> Outcome {
    let Some(turn) = watch.bypass_change_detection().turn.as_mut() else {
        return Outcome::Ignored;
    };
    let outcome = turn.enter_all(gestures);
    if outcome != Outcome::Ignored {
        watch.set_changed();
    }
    follow(net, watch, outcome);
    outcome
}

/// What a click on the space `at` of the board does (on its cell, or on a piece standing
/// there), whether the click is a press and a release there or a drag let go there
/// (`drag.rs`): the line pointed at is chosen whole, by all its clicks; with something taken
/// up, it is put where it is aimed; else the click on the position. What it did.
pub fn click_at(
    net: &Net,
    watch: &mut ResMut<WatchStatus>,
    (aim, line): (&SideAim, &LineAim),
    at: &str,
) -> Outcome {
    if let Some(chosen) = &line.0
        && chosen.captures.iter().any(|c| c == at)
    {
        let n = watch.turn.as_ref().map_or(0, |t| t.gestures.len());
        let clicks = chosen.gestures.get(n..).unwrap_or_default().to_vec();
        return enter_all(net, watch, &clicks);
    }
    let taken = watch.turn.as_ref().is_some_and(|t| t.taken().is_some());
    match (&aim.choice, taken) {
        (Some(choice), true) => gesture(net, watch, &choice.clone()),
        _ => gesture(net, watch, &Gesture::pos(at)),
    }
}

/// The player confirms the move.
pub fn submit(net: &Net, watch: &mut ResMut<WatchStatus>) {
    let Some(turn) = watch.bypass_change_detection().turn.as_mut() else {
        return;
    };
    // Where one choice ends the selection, *Play* makes it, and the move goes once whole.
    let ended = turn.end();
    if ended != Outcome::Ignored {
        watch.set_changed();
        return follow(net, watch, ended);
    }
    if let Some((notation, ply)) = turn.submit() {
        watch.set_changed();
        net::play(net, watch, notation, ply);
    }
}

pub fn back(watch: &mut ResMut<WatchStatus>) {
    if let Some(turn) = watch.bypass_change_detection().turn.as_mut()
        && turn.back()
    {
        watch.set_changed();
    }
}

/// Asks the server again what may be selected, after it could not say.
pub fn ask_again(net: &Net, watch: &mut WatchStatus) {
    if let Some(turn) = watch.turn.as_mut() {
        *turn = turn.afresh();
        net::ask(net, watch);
    }
}

/// A click on what lies beside the board: a bowl's or a supply's choice, what a click takes
/// up. A click on the board's own spaces is a press and a release there, which `drag.rs` reads
/// (`click_at`), so that a drag and a click are told apart in one place; so is a drag from what
/// lies beside it, which is no click even let go where it was pressed.
pub(crate) fn on_click(
    click: On<PointerClick>,
    spots: Query<(), With<Spot>>,
    (choices, takes, places): (Query<&Choice>, Query<&TakeUp>, Query<&TakeToPlace>),
    (net, drag): (Res<Net>, Res<Drag>),
    mut watch: ResMut<WatchStatus>,
) {
    // Clicks bubble up to the board; only the cell or piece itself is a position, and its
    // click is the press's (`drag.rs`). A bowl's choice is the bowl's, whether the bowl or the
    // stones in it were clicked.
    if click.button != PointerButton::Primary
        || spots.contains(click.entity)
        || drag.0.as_ref().is_some_and(|p| p.left)
    {
        return;
    }
    if let Ok(choice) = choices.get(click.entity) {
        gesture(&net, &mut watch, &choice.0);
    } else if let Ok(take) = takes.get(click.entity)
        && let Some(turn) = watch.bypass_change_detection().turn.as_mut()
        && turn.take_up(&take.0) != Outcome::Ignored
    {
        watch.set_changed();
    } else if let Ok(take) = places.get(click.entity)
        && let Some(turn) = watch.bypass_change_detection().turn.as_mut()
        && turn.take_to_place(&take.0) != Outcome::Ignored
    {
        watch.set_changed();
    }
}

/// Something of the board's a click is taken on, as `on_click` sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clickable<'a> {
    Spot(&'a str),
    Choice(&'a Gesture),
    TakeUp(&'a [Gesture]),
    TakeToPlace(&'a [Gesture]),
}

/// What a click on `what` would do, for the pointer's hand: pick up a piece (one a click picks
/// up, or what a bowl, a supply or a stock gives), choose it (a position a click is taken on,
/// or where what is taken up is aimed), or nothing. `aimed`: something taken up is aimed at a
/// place a click on the board puts it.
pub fn feel(turn: Option<&Turn>, what: Clickable, aimed: bool) -> Feel {
    let Some(turn) = turn.filter(|t| t.stage == Stage::Choosing) else {
        return Feel::Open;
    };
    let takes = |outcome: Outcome| {
        if outcome == Outcome::Ignored {
            Feel::Open
        } else {
            Feel::Grab
        }
    };
    match what {
        Clickable::Spot(_) if turn.taken().is_some() && !turn.taken_after_click() => {
            if aimed {
                Feel::Point
            } else {
                Feel::Open
            }
        }
        // What a click picks up, or what is in hand there, which a press may drag on.
        Clickable::Spot(at) if turn.picks_up(at) || turn.in_hand_at() == Some(at) => Feel::Grab,
        Clickable::Spot(at) if turn.targets().contains(at) => Feel::Point,
        Clickable::Spot(_) => Feel::Open,
        Clickable::Choice(g) => takes(turn.clone().gesture(g)),
        Clickable::TakeUp(g) => takes(turn.clone().take_up(g)),
        Clickable::TakeToPlace(g) => takes(turn.clone().take_to_place(g)),
    }
}

/// Says what a click on the board under the pointer would do (`cursor.rs`), as `on_click`
/// finds what was clicked: the first thing outwards from what was hit that is the board's.
fn sense(
    under: Res<Under>,
    (spots, choices, takes, places): (
        Query<&Spot>,
        Query<&Choice>,
        Query<&TakeUp>,
        Query<&TakeToPlace>,
    ),
    (watch, aim): (Res<WatchStatus>, Res<SideAim>),
    mut table: ResMut<TableFeel>,
) {
    let what = under.chain.iter().find_map(|e| {
        if let Ok(spot) = spots.get(*e) {
            Some(Clickable::Spot(&spot.0))
        } else if let Ok(choice) = choices.get(*e) {
            Some(Clickable::Choice(&choice.0))
        } else if let Ok(take) = takes.get(*e) {
            Some(Clickable::TakeUp(&take.0))
        } else {
            places.get(*e).ok().map(|p| Clickable::TakeToPlace(&p.0))
        }
    });
    if let Some(what) = what {
        table.set_if_neq(TableFeel(feel(
            watch.turn.as_ref(),
            what,
            aim.choice.is_some(),
        )));
    }
}

fn on_over(over: On<PointerOver>, spots: Query<&Spot>, mut hovering: ResMut<Hovering>) {
    if let Ok(spot) = spots.get(over.entity)
        && hovering.0.as_ref() != Some(&spot.0)
    {
        hovering.0 = Some(spot.0.clone());
    }
}

fn on_out(out: On<PointerOut>, spots: Query<&Spot>, mut hovering: ResMut<Hovering>) {
    if let Ok(spot) = spots.get(out.entity)
        && hovering.0.as_ref() == Some(&spot.0)
    {
        hovering.0 = None;
    }
}

/// Enter plays the move shown; Backspace goes back (Esc is the escape menu's). Not while a text
/// field has the keyboard, nor while the escape menu is open. While the button is down after a
/// press on the board, Enter waits, and Backspace lets the press go (`drag::cancel`): what a
/// drag carries goes back as it was.
fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    menu: Option<Res<EscapeMenu>>,
    net: Res<Net>,
    (mut watch, mut drag): (ResMut<WatchStatus>, ResMut<Drag>),
) {
    if focus.is_some_and(|f| f.get().is_some()) || !escape_menu::closed(menu.as_deref()) {
        return;
    }
    if keys.just_pressed(KeyCode::Backspace) && drag::cancel(&mut drag, &mut watch) {
        return;
    }
    if drag.pressed() {
        return;
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
        submit(&net, &mut watch);
    }
    if keys.just_pressed(KeyCode::Backspace) {
        back(&mut watch);
    }
}

/// How far the pointer may move, in pixels, between pressing the right button and letting it go
/// for that to be a click (back) rather than a drag (the camera turning).
const CLICK_SLOP: f32 = 6.0;

/// A right-click over the table goes back, as Backspace does; not over a panel, nor while the
/// escape menu is open. While the primary button is down after a press on the board, it lets
/// the press go instead (`drag::cancel`). `pressed` is how far the pointer has moved since the
/// button went down over the table.
fn right_click(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    pointer: PointerOverUi,
    menu: Option<Res<EscapeMenu>>,
    mut pressed: Local<Option<f32>>,
    (mut watch, mut drag): (ResMut<WatchStatus>, ResMut<Drag>),
) {
    if buttons.just_pressed(MouseButton::Right) {
        *pressed = (!pointer.is_over_ui() && escape_menu::closed(menu.as_deref())).then_some(0.0);
    }
    if let Some(moved) = pressed.as_mut() {
        *moved += motion.delta.length();
    }
    if buttons.just_released(MouseButton::Right)
        && let Some(moved) = pressed.take()
        && moved < CLICK_SLOP
        && !drag::cancel(&mut drag, &mut watch)
    {
        back(&mut watch);
    }
}

/// Each kind of piece's reserve pool, by its id: what the wheel may swap a piece in hand for.
fn pools(watch: &WatchStatus) -> HashMap<String, String> {
    watch
        .table
        .as_ref()
        .map(|t| {
            t.info
                .pieces
                .iter()
                .map(|p| (p.id.clone(), p.pool.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// The wheel is the move's while it would change something there, and the camera's otherwise.
fn take_wheel(watch: Res<WatchStatus>, mut taken: ResMut<WheelTaken>) {
    let wanted = watch
        .turn
        .as_ref()
        .is_some_and(|t| t.wheel_changes(&pools(&watch)));
    taken.set_if_neq(WheelTaken(wanted));
}

/// The wheel over the table, while the move takes it: each notch one more of a stack picked up
/// carried (away from the player) or one fewer, or the piece in hand swapped for another of its
/// pool. A touchpad's pixels are counted into notches.
fn wheel(
    scroll: Res<AccumulatedMouseScroll>,
    taken: Res<WheelTaken>,
    pointer: PointerOverUi,
    menu: Option<Res<EscapeMenu>>,
    mut notches: Local<f32>,
    (mut watch, mut aim): (ResMut<WatchStatus>, ResMut<SideAim>),
) {
    if !taken.0 || pointer.is_over_ui() || !escape_menu::closed(menu.as_deref()) {
        *notches = 0.0;
        return;
    }
    *notches += match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / PIXELS_PER_LINE,
    };
    let pools = pools(&watch);
    while notches.abs() >= 1.0 {
        let up = *notches > 0.0;
        *notches -= notches.signum();
        let Some(turn) = watch.bypass_change_detection().turn.as_mut() else {
            return;
        };
        // What is taken up: on to the next place it may go.
        if let Some(places) = turn.taken() {
            let at = aim
                .choice
                .as_ref()
                .and_then(|c| places.iter().position(|p| *p == c));
            let next = match (at, up) {
                (None, _) => 0,
                (Some(i), true) => (i + 1) % places.len(),
                (Some(i), false) => (i + places.len() - 1) % places.len(),
            };
            *aim = SideAim {
                choice: Some(places[next].clone()),
                by_wheel: true,
            };
            continue;
        }
        if turn.wheel(up, &pools) != Outcome::Ignored {
            watch.set_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(kind: &str, value: &str) -> Gesture {
        Gesture {
            kind: kind.into(),
            value: value.into(),
        }
    }

    fn sel(notation: &str, to: Option<&str>, gestures: &[(&str, &str)]) -> LegalSelection {
        LegalSelection {
            notation: notation.into(),
            action: "x".into(),
            to: to.map(Into::into),
            from: None,
            aim: None,
            // As the server says it: the piece the selection's gestures choose.
            piece: gestures
                .iter()
                .find(|(k, _)| *k == "piece")
                .map(|(_, v)| v.to_string()),
            gestures: gestures.iter().map(|(k, v)| g(k, v)).collect(),
            captures: Vec::new(),
            converts_to: Default::default(),
            path: Vec::new(),
        }
    }

    fn place(cell: &str) -> LegalSelection {
        sel(cell, Some(cell), &[("piece", "stone"), ("pos", cell)])
    }

    /// A piece stepping from one position to another.
    fn step(from: &str, to: &str) -> LegalSelection {
        LegalSelection {
            from: Some(from.into()),
            ..sel(
                &format!("{from}-{to}"),
                Some(to),
                &[("pos", from), ("pos", to)],
            )
        }
    }

    /// One of Fist's jumps from a1: its legs, each a landing and what happens to the piece
    /// jumped (`capture` or `spare`), then `end`; what it captures as the server says it.
    fn jump(legs: &[(&str, &str, &str)]) -> LegalSelection {
        let mut gestures = vec![g("pos", "a1")];
        for (land, choice, _) in legs {
            gestures.push(g("pos", land));
            gestures.push(g("action", choice));
        }
        gestures.push(g("action", "end"));
        LegalSelection {
            notation: "jump".into(),
            action: "jump".into(),
            from: Some("a1".into()),
            to: legs.last().map(|(land, ..)| land.to_string()),
            captures: legs
                .iter()
                .filter(|(_, choice, _)| *choice == "capture")
                .map(|(.., over)| over.to_string())
                .collect(),
            gestures,
            ..Default::default()
        }
    }

    #[test]
    fn what_a_move_turns_over_is_known_once_every_selection_it_could_be_turns_it() {
        // An ace's jump over b2 to c3, the piece jumped captured, spared or converted.
        let convert = |choice: &str| LegalSelection {
            converts_to: (choice == "convert")
                .then(|| ("b2".to_string(), "white".to_string()))
                .into_iter()
                .collect(),
            ..jump(&[("c3", choice, "b2")])
        };
        let mut t = Turn::new(1);
        t.answered(answer(
            1,
            vec![convert("capture"), convert("spare"), convert("convert")],
        ));
        t.gesture(&g("pos", "a1"));
        t.gesture(&g("pos", "c3"));
        assert!(t.turning().is_empty(), "not known yet");
        let mut spared = t.clone();
        t.gesture(&g("action", "convert"));
        assert_eq!(t.turning(), BTreeMap::from([("b2", "white")]), "converted");
        t.gesture(&g("action", "end"));
        assert_eq!(t.made.len(), 1);
        assert_eq!(t.turning(), BTreeMap::from([("b2", "white")]), "made");
        spared.gesture(&g("action", "spare"));
        assert!(spared.turning().is_empty(), "spared: as it was");

        // A placement that outflanks discs: made by its one click, what it turns is known.
        let place = LegalSelection {
            converts_to: BTreeMap::from([
                ("d4".to_string(), "black".to_string()),
                ("d5".to_string(), "black".to_string()),
            ]),
            ..sel("d3", Some("d3"), &[("pos", "d3")])
        };
        let mut t = Turn::new(1);
        t.answered(answer(
            1,
            vec![place, sel("c4", Some("c4"), &[("pos", "c4")])],
        ));
        assert!(t.turning().is_empty(), "nothing chosen yet");
        t.gesture(&g("pos", "d3"));
        assert_eq!(
            t.turning(),
            BTreeMap::from([("d4", "black"), ("d5", "black")])
        );
    }

    #[test]
    fn what_a_move_takes_is_known_once_every_selection_it_could_be_takes_it() {
        // A jump over b2 to c3, the piece jumped captured or spared, and a second leg over c4
        // to c5, likewise.
        let mut t = Turn::new(1);
        t.answered(answer(
            1,
            vec![
                jump(&[("c3", "capture", "b2")]),
                jump(&[("c3", "spare", "b2")]),
                jump(&[("c3", "capture", "b2"), ("c5", "capture", "c4")]),
                jump(&[("c3", "capture", "b2"), ("c5", "spare", "c4")]),
                jump(&[("c3", "spare", "b2"), ("c5", "capture", "c4")]),
            ],
        ));
        assert!(t.taking().is_empty(), "nothing chosen yet");
        t.gesture(&g("pos", "a1"));
        t.gesture(&g("pos", "c3"));
        assert!(t.taking().is_empty(), "captured or spared: not known yet");
        assert!(!t.takes_at("c3"), "it lands there: it takes nothing there");
        t.gesture(&g("action", "capture"));
        assert_eq!(t.taking(), BTreeSet::from(["b2"]), "captured");
        let mut both = t.clone();
        // The second leg's piece spared: the move is whole, and takes the first alone.
        t.gesture(&g("pos", "c5"));
        t.gesture(&g("action", "spare"));
        assert_eq!(t.taking(), BTreeSet::from(["b2"]), "the second spared");
        // Captured too, and the chain ended: both.
        both.gesture(&g("pos", "c5"));
        both.gesture(&g("action", "capture"));
        both.gesture(&g("action", "end"));
        assert_eq!(both.made.len(), 1);
        assert_eq!(
            both.taking(),
            BTreeSet::from(["b2", "c4"]),
            "made: what it takes"
        );

        // A pin: the piece aimed at is taken by a click on it.
        let pin = LegalSelection {
            from: Some("a1".into()),
            aim: Some("b3".into()),
            captures: vec!["b3".into()],
            ..sel(
                "a1-a2xb3",
                Some("a2"),
                &[("pos", "a1"), ("pos", "a2"), ("pos", "b3")],
            )
        };
        let mut t = Turn::new(1);
        t.answered(answer(1, vec![pin, step("a1", "a3")]));
        t.gesture(&g("pos", "a1"));
        t.gesture(&g("pos", "a2"));
        assert!(t.takes_at("b3"));
        assert!(!t.takes_at("a2"));
    }

    #[test]
    fn the_hand_grabs_a_piece_a_click_picks_up_and_points_at_where_it_may_go() {
        let flat = sel("b2", Some("b2"), &[("piece", "flat"), ("pos", "b2")]);
        let wall = sel("Sb2", Some("b2"), &[("piece", "wall"), ("pos", "b2")]);
        let only = sel("c3", Some("c3"), &[("piece", "flat"), ("pos", "c3")]);
        let mut t = Turn::new(1);
        t.answered(answer(
            1,
            vec![step("a1", "a2"), step("a1", "a3"), flat, wall, only],
        ));
        let at = |t: &Turn, cell: &str| feel(Some(t), Clickable::Spot(cell), false);
        assert!(t.picks_up("a1"));
        assert!(!t.picks_up("a2") && !t.picks_up("b2"));
        assert_eq!(at(&t, "a1"), Feel::Grab, "a piece to move");
        assert_eq!(at(&t, "c3"), Feel::Point, "a cell a piece is placed on");
        assert_eq!(at(&t, "b2"), Feel::Open, "not until which piece is chosen");
        assert_eq!(at(&t, "a2"), Feel::Open, "nothing to click there yet");
        // What a bowl or a supply gives is picked up, when it may be chosen.
        let take =
            |t: &Turn, kind: &str| feel(Some(t), Clickable::Choice(&g("piece", kind)), false);
        assert_eq!(take(&t, "wall"), Feel::Grab);
        assert_eq!(take(&t, "cap"), Feel::Open);
        assert_eq!(
            feel(None, Clickable::Spot("a1"), false),
            Feel::Open,
            "not our turn"
        );

        // Picked up: where it may go is pointed at, and it is not picked up again.
        t.gesture(&g("pos", "a1"));
        assert!(!t.picks_up("a1"));
        assert_eq!(at(&t, "a2"), Feel::Point);
        assert_eq!(at(&t, "a3"), Feel::Point);
    }

    #[test]
    fn what_a_click_would_stand_on_a_position_is_known_as_far_as_the_selections_agree() {
        let mut t = Turn::new(1);
        let flat = sel("b2", Some("b2"), &[("piece", "flat"), ("pos", "b2")]);
        let wall = sel("Sb2", Some("b2"), &[("piece", "wall"), ("pos", "b2")]);
        let only = sel("c3", Some("c3"), &[("piece", "flat"), ("pos", "c3")]);
        t.answered(answer(
            4,
            vec![flat, wall, only, step("a1", "a2"), step("b1", "a2")],
        ));
        // Two kinds could go there: which is not known until one is chosen.
        let b2 = t.arriving("b2").expect("something would be put there");
        assert_eq!((b2.piece, b2.from), (None, None));
        assert_eq!(t.arriving("c3").unwrap().piece, Some("flat"));
        // Two pieces could step there: where from is not known, nor is it said what they are.
        assert_eq!(t.arriving("a2"), Some(Arriving::default()));
        // A click that picks a piece up puts nothing down, and nor does one nowhere.
        assert_eq!(t.arriving("a1"), None);
        assert_eq!(t.arriving("z9"), None);

        assert_eq!(t.gesture(&g("piece", "wall")), Outcome::Changed);
        assert_eq!(t.arriving("b2").unwrap().piece, Some("wall"));
        assert_eq!(t.arriving("c3"), None, "no wall may go there");

        // Once made, a selection says what it put where, whatever may follow.
        let mut t = Turn::new(1);
        t.answered(answer(4, vec![step("a1", "a2"), step("b1", "a2")]));
        assert_eq!(t.gesture(&g("pos", "a1")), Outcome::Changed);
        assert_eq!(t.arriving("a2").unwrap().from, Some("a1"));
        assert_eq!(t.gesture(&g("pos", "a2")), Outcome::Ask);
        assert_eq!(
            t.arriving("a2"),
            Some(Arriving {
                piece: None,
                from: Some("a1"),
                aimed: false,
            })
        );
    }

    #[test]
    fn with_clicks_only_nothing_is_chosen_for_the_player_and_a_whole_move_plays_itself() {
        let card = |gestures: &[&str]| {
            let gestures: Vec<(&str, &str)> = gestures.iter().map(|v| ("action", *v)).collect();
            sel("m", None, &gestures)
        };
        // One move only, as a patience may have: it is not taken by itself.
        let mut t = Turn::new(1);
        t.clicks_only = true;
        t.answered(answer(3, vec![card(&["from-t1-1", "to-f"])]));
        assert_eq!(t.stage, Stage::Choosing);
        assert!(t.gestures.is_empty(), "nothing lifted for the player");
        assert_eq!(t.gesture(&g("action", "from-t1-1")), Outcome::Changed);
        assert_eq!(t.gestures, [g("action", "from-t1-1")], "nor put down");
        assert_eq!(t.gesture(&g("action", "to-f")), Outcome::Ask);
        assert_eq!(t.answered(complete(3)), Outcome::Submit, "played at once");
        assert_eq!(t.submit(), Some(("m".into(), 3)));

        // A move that may end where it is, with more that could follow it in the same impulse
        // (one that turns no card up): played at once too, as it is.
        let mut t = Turn::new(1);
        t.clicks_only = true;
        t.answered(answer(3, vec![card(&["from-t1-1", "to-t5"])]));
        t.gesture(&g("action", "from-t1-1"));
        assert_eq!(t.gesture(&g("action", "to-t5")), Outcome::Ask);
        let more = Selections {
            made: vec!["m".into()],
            may_end: true,
            state: None,
            ..answer(3, vec![card(&["from-t4-1", "to-t1"]), card(&["draw"])])
        };
        assert_eq!(t.answered(more.clone()), Outcome::Submit);
        assert_eq!(t.submit(), Some(("m".into(), 3)), "only the move made");
        // Without clicks_only it waits for the player to play it or go on.
        let mut t = Turn::new(1);
        t.answered(answer(
            3,
            vec![card(&["from-t1-1", "to-t5"]), card(&["draw"])],
        ));
        t.gesture(&g("action", "from-t1-1"));
        t.gesture(&g("action", "to-t5"));
        assert_eq!(t.answered(more), Outcome::Changed);
        assert_eq!((t.stage, t.can_play()), (Stage::Choosing, true));

        // A whole selection at once, whatever path_to would make of its first gesture alone
        // (pin-f is reached straight away in one play and after pin-b in another).
        let mut t = Turn::new(1);
        t.clicks_only = true;
        t.answered(answer(
            3,
            vec![
                card(&["pin-b", "pin-f", "pin-i", "ball-1"]),
                card(&["pin-f", "pin-i", "ball-2"]),
            ],
        ));
        assert_eq!(
            t.gesture(&g("action", "pin-f")),
            Outcome::Ignored,
            "alone, ambiguous"
        );
        let play = [
            g("action", "pin-f"),
            g("action", "pin-i"),
            g("action", "ball-2"),
        ];
        assert_eq!(
            t.enter_all(&play[..2]),
            Outcome::Ignored,
            "not a whole selection"
        );
        assert_eq!(t.enter_all(&play), Outcome::Ask);
        assert_eq!(t.made.len(), 1);
        assert_eq!(t.answered(complete(3)), Outcome::Submit);
        assert_eq!(
            Turn::new(1).enter_all(&play),
            Outcome::Ignored,
            "not choosing"
        );

        // Without, the one way is taken, and the whole move waits for the player.
        let mut t = Turn::new(1);
        t.answered(answer(3, vec![card(&["from-t1-1", "to-f"])]));
        assert_eq!(t.stage, Stage::Asking, "taken and made");
        assert_eq!(t.answered(complete(3)), Outcome::Changed);
        assert_eq!(t.stage, Stage::Ready);

        // Started again after a refusal, it is still so.
        let mut t = Turn::new(1);
        t.clicks_only = true;
        t.auto_take = vec![g("action", "fence")];
        t.send_failed(Problem::BadAddress("x".into()));
        assert!(t.clicks_only);
        assert_eq!(t.auto_take, [g("action", "fence")]);
        assert!(t.problem.is_some());
    }

    /// A stack's move as Tak's server lists it: from a position, a count, then a position for
    /// each piece dropped.
    fn carry(from: &str, count: usize, drops: &[&str]) -> LegalSelection {
        let mut gestures = vec![("pos", from)];
        let count = count.to_string();
        gestures.push(("count", count.as_str()));
        gestures.extend(drops.iter().map(|d| ("pos", *d)));
        LegalSelection {
            from: Some(from.into()),
            ..sel("m", drops.first().copied(), &gestures)
        }
    }

    /// Tak's choices on c3 with two of the player's stones: one or both, up (c4, c5) or right
    /// (d3), and stones to place, flat or standing, with the capstone.
    fn tak() -> Vec<LegalSelection> {
        let mut out = vec![
            carry("c3", 1, &["c4"]),
            carry("c3", 2, &["c4", "c4"]),
            carry("c3", 2, &["c4", "c5"]),
            carry("c3", 1, &["d3"]),
            carry("c3", 2, &["d3", "d3"]),
        ];
        for piece in ["flat", "wall", "cap"] {
            out.push(sel("p", Some("a1"), &[("piece", piece), ("pos", "a1")]));
        }
        out
    }

    fn pools() -> HashMap<String, String> {
        [("flat", "stone"), ("wall", "stone"), ("cap", "cap")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .into()
    }

    #[test]
    fn a_stack_is_picked_up_whole_and_the_wheel_leaves_some_behind() {
        let mut t = Turn::new(1);
        t.answered(answer(8, tak()));
        assert_eq!(t.gesture(&g("pos", "c3")), Outcome::Changed);
        // All it may carry, at once: no button to press for how many.
        assert_eq!(t.gestures, [g("pos", "c3"), g("count", "2")]);
        let lifted = t.stack_moves();
        assert_eq!(
            lifted,
            [StackMove {
                from: "c3",
                count: 2,
                drops: vec![]
            }]
        );
        assert!(t.choices().is_empty());
        assert!(t.carried().is_empty(), "a stack, not one piece");
        // Down: one fewer (it goes back on the stack); not below one; up again.
        assert!(t.wheel_changes(&pools()));
        assert_eq!(t.wheel(false, &pools()), Outcome::Changed);
        assert_eq!(t.stack_moves()[0].count, 1);
        assert_eq!(t.wheel(false, &pools()), Outcome::Ignored);
        assert_eq!(t.wheel(true, &pools()), Outcome::Changed);
        assert_eq!(
            t.wheel(true, &pools()),
            Outcome::Ignored,
            "no more than it may carry"
        );
        // Each click drops the bottom piece carried; once one has, the wheel is the camera's.
        assert_eq!(t.gesture(&g("pos", "c4")), Outcome::Changed);
        assert_eq!(t.stack_moves()[0].drops, ["c4"]);
        assert!(!t.wheel_changes(&pools()));
        assert_eq!(t.targets(), ["c4", "c5"].map(String::from).into());
        assert_eq!(t.gesture(&g("pos", "c5")), Outcome::Ask);
        // Made, it is still known as a stack moved, to show.
        assert_eq!(t.stack_moves()[0].drops, ["c4", "c5"]);
        assert!(t.carried().is_empty());
    }

    #[test]
    fn a_stack_put_down_again_goes_back_whole() {
        let mut t = Turn::new(1);
        t.answered(answer(8, tak()));
        t.gesture(&g("pos", "c3"));
        t.wheel(false, &pools());
        assert_eq!(
            t.gesture(&g("pos", "c3")),
            Outcome::Changed,
            "clicked again"
        );
        assert!(t.gestures.is_empty() && t.stack_moves().is_empty());
        t.gesture(&g("pos", "c3"));
        assert!(t.back());
        assert!(t.gestures.is_empty(), "back puts it down too");
    }

    #[test]
    fn the_wheel_turns_a_flat_stone_in_hand_into_a_standing_one_and_back() {
        let mut t = Turn::new(1);
        t.answered(answer(8, tak()));
        assert!(!t.wheel_changes(&pools()), "nothing in hand: the camera's");
        t.gesture(&g("piece", "flat"));
        assert_eq!(t.wheel(true, &pools()), Outcome::Changed);
        assert_eq!(t.in_hand(), Some("wall"));
        assert_eq!(t.wheel(true, &pools()), Outcome::Changed);
        assert_eq!(t.in_hand(), Some("flat"), "round again");
        assert_eq!(t.wheel(false, &pools()), Outcome::Changed);
        assert_eq!(t.in_hand(), Some("wall"));
        assert_eq!(t.gesture(&g("pos", "a1")), Outcome::Ask);
        assert_eq!(t.made[0].piece.as_deref(), Some("wall"));
        // A capstone has no other kind in its pool.
        let mut t = Turn::new(1);
        t.answered(answer(8, tak()));
        t.gesture(&g("piece", "cap"));
        assert_eq!(t.wheel(true, &pools()), Outcome::Ignored);
        // Nor does a piece whose pool is not known.
        let mut t = Turn::new(1);
        t.answered(answer(8, tak()));
        t.gesture(&g("piece", "flat"));
        assert_eq!(t.wheel(true, &HashMap::new()), Outcome::Ignored);
    }

    /// Fendo's choices: move the piece on d1 (or leave it) and fence a side of where it ends,
    /// or add a piece.
    fn fendo() -> Vec<LegalSelection> {
        let fence = |from: &str, to: Option<&str>, side: &str| {
            let mut gestures = vec![("pos", from)];
            gestures.extend(to.map(|t| ("pos", t)));
            gestures.push(("action", side));
            LegalSelection {
                from: Some(from.into()),
                ..sel("m", Some(to.unwrap_or(from)), &gestures)
            }
        };
        vec![
            fence("d1", None, "fence-n"),
            fence("d1", None, "fence-e"),
            fence("d1", Some("d3"), "fence-e"),
            fence("d1", Some("d3"), "fence-w"),
            sel("add@a7", Some("a7"), &[("action", "add"), ("pos", "a7")]),
            sel("add@b7", Some("b7"), &[("action", "add"), ("pos", "b7")]),
        ]
    }

    fn sides() -> Vec<Gesture> {
        ["fence-e", "fence-n", "fence-s", "fence-w"]
            .map(|s| g("action", s))
            .to_vec()
    }

    #[test]
    fn a_choice_that_places_a_piece_is_in_hand_until_it_is_placed() {
        let mut t = Turn::new(1);
        t.answered(answer(2, fendo()));
        assert_eq!(t.gesture(&g("action", "add")), Outcome::Changed);
        assert_eq!(t.in_hand(), Some("add"));
        assert!(t.holding(), "the squares it may go to are lit");
        // Chosen again: put back.
        assert_eq!(t.gesture(&g("action", "add")), Outcome::Changed);
        assert_eq!(t.in_hand(), None);
        t.gesture(&g("action", "add"));
        assert_eq!(t.gesture(&g("pos", "a7")), Outcome::Ask);
        assert_eq!(t.placings().count(), 1);
        // A choice after a click on the board (a fence's side) is not in hand.
        let mut t = Turn::new(1);
        t.answered(answer(2, fendo()));
        t.gesture(&g("pos", "d1"));
        assert_eq!(t.in_hand(), None);
    }

    #[test]
    fn a_piece_whose_moves_do_not_click_it_is_picked_up_by_a_click_on_it() {
        // Courtfall's activated unit on e6: a click on where it goes, then what it does there.
        let act = |hex: &str, action: &str| LegalSelection {
            from: Some("e6".into()),
            piece: Some("queen".into()),
            ..sel(
                &format!("Q{action}{hex}"),
                Some(hex),
                &[("pos", hex), ("action", action)],
            )
        };
        let end = sel("end", None, &[("action", "end")]);
        let mut t = Turn::new(1);
        t.answered(answer(
            5,
            vec![
                act("e3", "move"),
                act("f3", "move"),
                act("d5", "strike"),
                act("d5", "shove"),
                end,
            ],
        ));
        assert_eq!(t.unclicked_from(), Some("e6"));
        assert_eq!(t.lifted(), None);
        assert!(!t.holding(), "nothing lit before it is picked up");
        // Picked up by a click on it, put down by another, or by Back.
        assert_eq!(t.gesture(&g("pos", "e6")), Outcome::Changed);
        assert_eq!(t.lifted(), Some("e6"));
        assert!(t.holding() && t.can_go_back());
        assert_eq!(t.gesture(&g("pos", "e6")), Outcome::Changed);
        assert_eq!(t.lifted(), None);
        t.gesture(&g("pos", "e6"));
        assert!(t.back());
        assert_eq!(t.lifted(), None);
        assert!(!t.can_go_back());
        // Held, a click where it may not go leaves it in hand; on where it may, it goes.
        t.gesture(&g("pos", "e6"));
        assert_eq!(t.gesture(&g("pos", "a1")), Outcome::Ignored);
        assert_eq!(t.lifted(), Some("e6"));
        assert_eq!(t.gesture(&g("pos", "f3")), Outcome::Ask);
        assert_eq!(t.made[0].notation, "Qmovef3");
        assert_eq!(t.lifted(), None);
        // A click on a unit it acts on: what it does is chosen next, and it is put down.
        let mut t = Turn::new(1);
        t.answered(answer(
            5,
            vec![act("e3", "move"), act("d5", "strike"), act("d5", "shove")],
        ));
        t.gesture(&g("pos", "e6"));
        assert_eq!(t.gesture(&g("pos", "d5")), Outcome::Changed);
        assert_eq!(t.lifted(), None);
        assert_eq!(t.choices().len(), 2);
        // A piece whose moves click it first is not this: it is picked up by its own click.
        let mut t = Turn::new(1);
        t.answered(answer(1, vec![step("a1", "a2"), step("b1", "a2")]));
        assert_eq!(t.unclicked_from(), None);
    }

    #[test]
    fn a_tile_taken_to_place_lights_where_it_may_go_and_a_click_there_lays_it() {
        // Courtfall's draft: a hex, then the tile laid on it.
        let lay = |hex: &str, tile: &str| LegalSelection {
            piece: Some(tile.into()),
            ..sel(
                &format!("tile:{tile}@{hex}"),
                Some(hex),
                &[("pos", hex), ("action", &format!("tile-{tile}"))],
            )
        };
        let green = [g("action", "tile-green")];
        let red = [g("action", "tile-red")];
        let mut t = Turn::new(1);
        t.answered(answer(
            0,
            vec![
                lay("b3", "green"),
                lay("c3", "green"),
                lay("b3", "red"),
                lay("d3", "red"),
            ],
        ));
        let all: Vec<String> = t.targets().into_iter().collect();
        assert_eq!(all, ["b3", "c3", "d3"]);
        // As a fence would be, a tile is not taken up by the rule for a choice made next.
        assert_eq!(t.take_up(&green), Outcome::Ignored);
        assert_eq!(
            t.take_to_place(&[g("action", "tile-blue")]),
            Outcome::Ignored
        );
        assert_eq!(t.take_to_place(&green), Outcome::Changed);
        assert!(t.holding(), "the hexes it may go on are lit");
        assert_eq!(t.taken(), Some(vec![&green[0]]));
        let lit: Vec<String> = t.targets().into_iter().collect();
        assert_eq!(lit, ["b3", "c3"]);
        // A hex it may not go on: nothing, and it stays in hand.
        assert_eq!(t.gesture(&g("pos", "d3")), Outcome::Ignored);
        assert!(t.taken().is_some());
        // Another kind taken instead takes its place; Back puts it down.
        assert_eq!(t.take_to_place(&red), Outcome::Changed);
        let lit: Vec<String> = t.targets().into_iter().collect();
        assert_eq!(lit, ["b3", "d3"]);
        assert!(t.back());
        assert!(t.taken().is_none());
        assert!(t.gestures.is_empty());
        // Taken again and laid with a click: the hex and the tile, the selection made.
        t.take_to_place(&green);
        assert_eq!(t.gesture(&g("pos", "b3")), Outcome::Ask);
        assert_eq!(t.made.len(), 1);
        assert_eq!(t.made[0].notation, "tile:green@b3");
        assert!(t.taken().is_none());
    }

    #[test]
    fn a_fence_taken_up_waits_for_the_side_it_goes_on_and_goes_back_first() {
        let mut t = Turn::new(1);
        t.answered(answer(2, fendo()));
        assert_eq!(
            t.take_up(&sides()),
            Outcome::Ignored,
            "no fence may be built yet"
        );
        t.gesture(&g("pos", "d1"));
        t.gesture(&g("pos", "d3"));
        assert_eq!(t.take_up(&sides()), Outcome::Changed);
        let taken: Vec<&str> = t
            .taken()
            .unwrap()
            .iter()
            .map(|g| g.value.as_str())
            .collect();
        assert_eq!(
            taken,
            ["fence-e", "fence-w"],
            "the sides it may go on from d3"
        );
        // Back puts the fence back first; the piece stays where it was moved.
        assert!(t.can_go_back());
        assert!(t.back());
        assert!(t.taken().is_none());
        assert_eq!(t.gestures.len(), 2);
        // Taken again, and again: put back.
        t.take_up(&sides());
        assert_eq!(t.take_up(&sides()), Outcome::Changed);
        assert!(t.taken().is_none());
        // Taken, then put on a side: the move is made, and nothing is taken any more.
        t.take_up(&sides());
        assert_eq!(t.gesture(&g("action", "fence-w")), Outcome::Ask);
        assert!(t.taken().is_none());
        assert_eq!(t.made[0].gestures.last(), Some(&g("action", "fence-w")));
    }

    #[test]
    fn a_fence_is_taken_up_by_itself_once_the_piece_has_moved() {
        let mut t = Turn::new(1);
        t.auto_take = sides();
        t.answered(answer(2, fendo()));
        // Picked up, the piece may still move or stay: nothing taken yet.
        t.gesture(&g("pos", "d1"));
        assert!(t.taken().is_none());
        // Moved: only a fence may follow, so it is in hand, and the wheel goes round its sides.
        t.gesture(&g("pos", "d3"));
        let taken: Vec<&str> = t
            .taken()
            .unwrap()
            .iter()
            .map(|g| g.value.as_str())
            .collect();
        assert_eq!(taken, ["fence-e", "fence-w"]);
        assert!(t.wheel_changes(&HashMap::new()));
        // Back puts it back; it is taken up again from the bowl.
        t.back();
        assert!(t.taken().is_none());
        assert_eq!(t.take_up(&sides()), Outcome::Changed);
        // A game whose look has none takes nothing up by itself.
        let mut t = Turn::new(1);
        t.answered(answer(2, fendo()));
        t.gesture(&g("pos", "d1"));
        t.gesture(&g("pos", "d3"));
        assert!(t.taken().is_none());
    }

    #[test]
    fn a_stone_is_held_by_itself_where_every_click_places_one() {
        // Go: the stone chosen for the player, then where; a pass on the panel.
        let mut options = vec![place("a1"), place("b1")];
        options.push(sel("pass", None, &[("action", "pass")]));
        let mut t = Turn::new(1);
        t.answered(answer(2, options));
        assert_eq!(t.auto_hand(), Some("stone"));
        assert!(!t.holding(), "nothing lit: nearly every point would be");
        // Placed: nothing held until the next selection.
        assert_eq!(t.gesture(&g("pos", "a1")), Outcome::Ask);
        assert_eq!(t.auto_hand(), None);
        // A choice of kind first (Blooms' colors), a piece moved (Fendo, Tak), or a move of
        // two parts: no stone held by itself.
        for options in [tak(), fendo(), vec![shoot("a1", "a2", "a3")]] {
            let mut t = Turn::new(1);
            t.answered(answer(2, options));
            assert_eq!(t.auto_hand(), None);
        }
        let mut t = Turn::new(1);
        let colors = vec![
            sel("r", Some("a1"), &[("piece", "red"), ("pos", "a1")]),
            sel("y", Some("a1"), &[("piece", "yellow"), ("pos", "a1")]),
        ];
        t.answered(answer(2, colors));
        assert_eq!(t.auto_hand(), None);
    }

    #[test]
    fn a_stack_move_is_read_from_its_gestures() {
        let gs = |v: &[(&str, &str)]| v.iter().map(|(k, x)| g(k, x)).collect::<Vec<_>>();
        let m = gs(&[("pos", "c3"), ("count", "3"), ("pos", "c4"), ("pos", "c4")]);
        assert_eq!(
            StackMove::of(&m),
            Some(StackMove {
                from: "c3",
                count: 3,
                drops: vec!["c4", "c4"]
            })
        );
        // A count with no position before it, or a choice after it: not a stack moved.
        assert_eq!(StackMove::of(&gs(&[("count", "2"), ("pos", "a1")])), None);
        assert_eq!(
            StackMove::of(&gs(&[("pos", "a1"), ("count", "2"), ("piece", "x")])),
            None
        );
        assert_eq!(StackMove::of(&gs(&[("pos", "a1"), ("pos", "a2")])), None);
    }

    /// A piece sliding from one position to another, then shooting from there (Amazons').
    fn shoot(from: &str, to: &str, aim: &str) -> LegalSelection {
        LegalSelection {
            from: Some(from.into()),
            aim: Some(aim.into()),
            ..sel(
                &format!("{from}-{to}/{aim}"),
                Some(to),
                &[("pos", from), ("pos", to), ("pos", aim)],
            )
        }
    }

    #[test]
    fn a_piece_picked_up_is_carried_put_down_and_then_aimed_from() {
        let mut t = Turn::new(1);
        t.answered(answer(
            7,
            vec![
                shoot("a1", "a2", "a3"),
                shoot("a1", "a2", "a1"),
                shoot("a1", "b2", "c3"),
                shoot("c1", "c2", "c3"),
            ],
        ));
        assert!(t.carried().is_empty(), "nothing is picked up yet");

        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Changed);
        assert_eq!(
            t.carried(),
            [Carried {
                from: "a1",
                to: None
            }],
            "in hand, not yet put anywhere"
        );
        assert_eq!(t.arriving("a2").map(|a| a.aimed), Some(false));
        assert_eq!(t.arriving("c3"), None, "c3 is only ever aimed at");

        // A click on it again puts it down.
        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Changed);
        assert!(t.gestures.is_empty() && t.carried().is_empty());

        t.gesture(&Gesture::pos("a1"));
        assert_eq!(t.gesture(&Gesture::pos("a2")), Outcome::Changed);
        assert_eq!(
            t.carried(),
            [Carried {
                from: "a1",
                to: Some("a2")
            }]
        );
        assert_eq!(
            t.targets(),
            BTreeSet::from(["a1".to_string(), "a3".to_string()]),
            "the square it left may be shot at"
        );
        let shot = t.arriving("a3").expect("an arrow would land there");
        assert!(shot.aimed);
        assert!(t.placed().next().is_none());

        assert_eq!(t.gesture(&Gesture::pos("a3")), Outcome::Ask);
        t.answered(complete(7));
        assert_eq!(t.stage, Stage::Ready);
        assert_eq!(t.placed().collect::<Vec<_>>(), ["a3"], "the arrow, shown");
        assert_eq!(
            t.carried(),
            [Carried {
                from: "a1",
                to: Some("a2")
            }],
            "the piece stays where it was put"
        );
        assert!(t.arriving("a3").unwrap().aimed);
        assert_eq!(t.arriving("a2").unwrap().from, Some("a1"));

        // Another square to shoot at replaces the shot, and a click on the shot plays it.
        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Ask);
        assert_eq!(t.made.last().unwrap().notation, "a1-a2/a1");
        t.answered(complete(7));
        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Submit);

        // Back takes off the shot first, leaving the piece where it was put.
        assert!(t.back());
        assert_eq!(t.gestures, [Gesture::pos("a1"), Gesture::pos("a2")]);
        assert!(t.back());
        assert_eq!(t.gestures, [Gesture::pos("a1")]);
    }

    #[test]
    fn a_move_that_may_end_after_one_selection_can_be_played_or_go_on() {
        let mut t = Turn::new(1);
        let red = |cell: &str| sel(cell, Some(cell), &[("piece", "red"), ("pos", cell)]);
        let yellow = |cell: &str| sel(cell, Some(cell), &[("piece", "yellow"), ("pos", cell)]);
        t.answered(answer(
            5,
            vec![red("a1"), red("b1"), yellow("a1"), yellow("b1")],
        ));
        assert!(!t.can_play(), "nothing placed yet");
        assert_eq!(t.gesture(&g("piece", "red")), Outcome::Changed);
        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Ask);
        // One stone placed: a second may follow, or the move may go as it is.
        t.answered(Selections {
            may_end: true,
            state: None,
            made: vec!["a1".into()],
            ..answer(5, vec![yellow("b1")])
        });
        assert_eq!(t.stage, Stage::Choosing);
        assert!(t.can_play());
        // Back to before the stone: not a move yet.
        let mut taken_back = t.clone();
        assert!(taken_back.back());
        assert!(!taken_back.can_play());
        // Half way to a second stone, the first alone is sent.
        t.gesture(&g("piece", "yellow"));
        assert_eq!(t.submit(), Some(("a1".to_string(), 5)));
        assert_eq!(t.stage, Stage::Sending);
        assert!(t.gestures.is_empty());
    }

    #[test]
    fn a_stone_taken_from_its_bowl_is_in_hand_until_placed_or_put_back() {
        let mut t = Turn::new(1);
        let stone =
            |piece: &str, cell: &str| sel(cell, Some(cell), &[("piece", piece), ("pos", cell)]);
        t.answered(answer(
            5,
            vec![
                stone("red", "a1"),
                stone("red", "b1"),
                stone("yellow", "a1"),
            ],
        ));
        assert_eq!(t.in_hand(), None);
        assert!(!t.holding());
        t.gesture(&g("piece", "red"));
        assert_eq!(t.in_hand(), Some("red"));
        assert!(t.holding(), "where it may go is lit");
        // The same bowl again: back in it.
        assert_eq!(t.gesture(&g("piece", "red")), Outcome::Changed);
        assert_eq!(t.in_hand(), None);
        // The other bowl instead, then placed.
        t.gesture(&g("piece", "red"));
        t.gesture(&g("piece", "yellow"));
        assert_eq!(t.in_hand(), Some("yellow"));
        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Ask);
        assert_eq!(t.in_hand(), None);
        assert_eq!(t.placements().collect::<Vec<_>>(), [("a1", Some("yellow"))]);

        // A choice taken for the player is not in hand: there was nothing to click.
        let mut t = Turn::new(1);
        t.answered(answer(5, vec![place("a1"), place("b1")]));
        assert_eq!(t.in_hand(), None);
    }

    fn swap() -> LegalSelection {
        sel("swap", None, &[("action", "swap")])
    }

    /// A position whose view asks `words` and tints `tinted` with `key`, as a preview.
    fn asking(words: &str, tinted: &[&str], key: &str) -> Position {
        let tints: serde_json::Map<String, serde_json::Value> = tinted
            .iter()
            .map(|c| (c.to_string(), serde_json::Value::from(key)))
            .collect();
        Position {
            ext: serde_json::json!({"view": {"impulse": words, "tints": tints}}),
            ..Position::default()
        }
    }

    #[test]
    fn a_move_that_goes_on_is_asked_in_the_previews_words_and_its_last_part_played_by_a_click() {
        // A ring moved to make a line, which is then removed, and a ring with it: three
        // selections of one impulse, the last taking a piece away rather than putting one down.
        let mut t = Turn::new(1);
        t.answered(answer(18, vec![step("j7", "e7"), step("c4", "c5")]));
        assert_eq!(
            t.preview(),
            None,
            "nothing made: the position is the table's"
        );
        t.gesture(&g("pos", "j7"));
        assert_eq!(t.gesture(&g("pos", "e7")), Outcome::Ask);
        let row = LegalSelection {
            from: Some("f7".into()),
            ..sel("row:f7-j7", Some("j7"), &[("pos", "f7"), ("pos", "j7")])
        };
        let words = "Choose a line to remove.";
        t.answered(Selections {
            made: vec!["j7-e7".into()],
            state: Some(asking(words, &["f7", "j7"], "row")),
            ..answer(18, vec![row])
        });
        assert_eq!(t.stage, Stage::Choosing);
        assert!(!t.can_play(), "not a whole move: Enter does nothing yet");
        let after = t.preview().expect("the server's preview");
        assert_eq!(after.impulse_words(), Some(words));
        assert_eq!(after.tints()["f7"], "row");
        assert!(t.untouched());
        t.gesture(&g("pos", "f7"));
        assert!(!t.untouched());
        assert_eq!(t.gesture(&g("pos", "j7")), Outcome::Ask);
        // Still the row's preview while the server is asked.
        assert!(t.preview().is_some());
        let ring = |at: &str| LegalSelection {
            from: Some(at.into()),
            ..sel(&format!("ring:{at}"), None, &[("pos", at)])
        };
        t.answered(Selections {
            made: vec!["j7-e7".into(), "row:f7-j7".into()],
            state: Some(asking("Choose one of your rings to remove.", &[], "")),
            ..answer(18, vec![ring("c4"), ring("e7")])
        });
        assert_eq!(
            t.preview().and_then(Position::impulse_words),
            Some("Choose one of your rings to remove.")
        );
        assert_eq!(t.gesture(&g("pos", "c4")), Outcome::Ask);
        t.answered(Selections {
            made: vec!["j7-e7".into(), "row:f7-j7".into(), "ring:c4".into()],
            state: Some(Position::default()),
            ..complete(18)
        });
        assert_eq!(t.stage, Stage::Ready);
        // A click again on the ring taken away plays the move, as one on where a piece is put.
        assert_eq!(t.gesture(&g("pos", "c4")), Outcome::Submit);
        // Back, the ring's choice goes, and the row's preview is back.
        t.back();
        assert_eq!(
            t.preview().and_then(Position::impulse_words),
            Some("Choose one of your rings to remove.")
        );
        t.back();
        t.back();
        assert_eq!(t.preview().and_then(Position::impulse_words), Some(words));
        assert_eq!(t.submit(), None);
        assert_eq!(t.notation(), "j7-e7");
    }

    #[test]
    fn a_line_chosen_by_what_it_takes_is_pointed_at_along_its_length_and_chosen_whole() {
        // Six discs in a line, a1 to a6, of which five are removed: a1–a5 or a2–a6, each named
        // by its ends and taking all five.
        let line = |a: usize, b: usize| LegalSelection {
            captures: (a..=b).map(|i| format!("a{i}")).collect(),
            ..sel(
                &format!("row:a{a}-a{b}"),
                None,
                &[("pos", &format!("a{a}")), ("pos", &format!("a{b}"))],
            )
        };
        let mut t = Turn::new(1);
        t.answered(answer(4, vec![line(1, 5), line(2, 6)]));
        let lines = t.lines();
        assert_eq!(lines.len(), 2);
        // Every disc of either is pointed at, not only their ends.
        let targets: Vec<String> = (1..=6).map(|i| format!("a{i}")).collect();
        assert_eq!(t.targets(), targets.iter().cloned().collect());
        assert!(!t.takes_at("a3"), "lit as a choice, not as a capture");
        // Along the line, the five whose middle is nearest the pointer: past the middle of a3
        // and a4, the second.
        let centre = |c: &str| Some((c[1..].parse::<f32>().ok()?, 0.0));
        let at = |cell: &str, x: f32| {
            nearest_line(&lines, cell, (x, 0.0), centre).map(|o| o.notation.clone())
        };
        assert_eq!(at("a1", 1.0).as_deref(), Some("row:a1-a5"));
        assert_eq!(at("a3", 3.2).as_deref(), Some("row:a1-a5"));
        assert_eq!(at("a3", 3.6).as_deref(), Some("row:a2-a6"));
        assert_eq!(
            at("a6", 3.0).as_deref(),
            Some("row:a2-a6"),
            "only one takes a6"
        );
        assert_eq!(at("b1", 1.0), None, "a disc no line takes");
        // A click chooses the line pointed at, by all its clicks at once.
        assert_eq!(t.enter_all(&line(2, 6).gestures), Outcome::Ask);
        assert_eq!(t.notation(), "row:a2-a6");
        // Back, it goes whole: the lines are pointed at again, nothing half clicked.
        assert!(t.back());
        assert!(t.untouched());
        assert_eq!(t.lines().len(), 2);
        // Once an end is clicked alone, the lines are chosen by clicks as before.
        t.gesture(&g("pos", "a2"));
        assert!(t.lines().is_empty());
    }

    #[test]
    fn a_move_clicked_where_it_does_not_take_is_no_line() {
        // A jump is clicked where it starts and lands, not on what it takes.
        let mut t = Turn::new(1);
        t.answered(answer(4, vec![jump(&[("c3", "capture", "b2")])]));
        assert!(t.lines().is_empty());
        // Nor a pin, clicked on the one piece it takes.
        let pin = |at: &str| LegalSelection {
            captures: vec![at.into()],
            ..sel(&format!("pin:{at}"), None, &[("pos", at)])
        };
        let mut t = Turn::new(1);
        t.answered(answer(4, vec![pin("d4"), pin("e4")]));
        assert!(t.lines().is_empty());
    }

    fn answer(ply: u64, options: Vec<LegalSelection>) -> Selections {
        Selections {
            seat: 1,
            ply,
            made: vec![],
            options,
            complete: false,
            may_end: false,
            state: None,
        }
    }

    fn complete(ply: u64) -> Selections {
        Selections {
            complete: true,
            may_end: false,
            state: None,
            ..answer(ply, vec![])
        }
    }

    /// A placement game's turn, with swapping sides offered: Hex's second move.
    fn turn() -> Turn {
        let mut t = Turn::new(1);
        assert_eq!(
            t.answered(answer(1, vec![place("a1"), place("b1"), swap()])),
            Outcome::Changed
        );
        t
    }

    #[test]
    fn a_click_on_a_cell_takes_the_forced_piece_and_makes_the_selection() {
        let mut t = turn();
        assert_eq!(
            t.targets(),
            BTreeSet::from(["a1".to_string(), "b1".to_string()])
        );
        assert_eq!(t.choices(), [g("action", "swap")]);
        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Ask);
        assert_eq!(t.made_notations(), ["a1"]);
        assert_eq!(t.stage, Stage::Asking);
        assert!(t.targets().is_empty(), "nothing to click while asking");

        assert_eq!(t.answered(complete(1)), Outcome::Changed);
        assert_eq!(t.stage, Stage::Ready);
        assert_eq!(t.placed().collect::<Vec<_>>(), ["a1"]);
        assert_eq!(t.submit(), Some(("a1".to_string(), 1)));
        assert_eq!(t.stage, Stage::Sending);
        assert_eq!(t.submit(), None, "sent once");
    }

    #[test]
    fn a_click_the_selections_do_not_allow_is_ignored() {
        let mut t = turn();
        assert_eq!(t.gesture(&Gesture::pos("z9")), Outcome::Ignored);
        assert_eq!(t.stage, Stage::Choosing);
        assert!(t.gestures.is_empty());
    }

    #[test]
    fn a_choice_the_server_leaves_no_other_to_is_taken_and_not_taken_back() {
        let mut t = Turn::new(0);
        t.answered(answer(0, vec![place("a1"), place("b1")]));
        assert_eq!(
            t.gestures,
            [g("piece", "stone")],
            "only stones can be placed"
        );
        assert!(t.choices().is_empty(), "no button for it");
        assert!(!t.can_go_back(), "nothing the player chose");
        assert!(!t.back());
        assert_eq!(t.gesture(&Gesture::pos("b1")), Outcome::Ask);
        t.answered(complete(0));
        assert!(t.back());
        assert_eq!(t.gestures, [g("piece", "stone")], "taken again");
    }

    #[test]
    fn a_choice_from_the_panel_makes_its_selection() {
        let mut t = turn();
        assert_eq!(t.gesture(&g("action", "swap")), Outcome::Ask);
        assert_eq!(t.notation(), "swap");
        assert_eq!(t.placed().count(), 0);
    }

    #[test]
    fn clicking_the_preview_again_plays_it_and_another_cell_replaces_it() {
        let mut t = turn();
        t.gesture(&Gesture::pos("a1"));
        t.answered(complete(1));
        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Submit);
        assert_eq!(t.stage, Stage::Ready, "submitting is the caller's to do");

        assert_eq!(t.gesture(&Gesture::pos("b1")), Outcome::Ask);
        assert_eq!(t.made_notations(), ["b1"]);
        // An answer for "a1" after that would be for a question no longer asked; the caller
        // checks `made`, and the turn takes the answer for "b1".
        t.answered(complete(1));
        assert_eq!(t.submit(), Some(("b1".to_string(), 1)));
    }

    #[test]
    fn a_click_that_is_not_allowed_leaves_the_preview_alone() {
        let mut t = turn();
        t.gesture(&Gesture::pos("a1"));
        t.answered(complete(1));
        assert_eq!(t.gesture(&Gesture::pos("z9")), Outcome::Ignored);
        assert_eq!(t.stage, Stage::Ready);
        assert_eq!(t.made_notations(), ["a1"]);
    }

    #[test]
    fn back_takes_off_gestures_then_selections() {
        let mut t = Turn::new(4);
        let step = |from: &str, to: &str| {
            sel(
                &format!("{from}-{to}"),
                Some(to),
                &[("pos", from), ("pos", to)],
            )
        };
        t.answered(answer(
            4,
            vec![step("a1", "a2"), step("a1", "b2"), step("c1", "c2")],
        ));
        assert_eq!(
            t.targets(),
            BTreeSet::from(["a1".to_string(), "c1".to_string()]),
            "a piece must be picked before where it goes"
        );
        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Changed);
        assert_eq!(
            t.targets(),
            BTreeSet::from(["a2".to_string(), "b2".to_string()])
        );
        // Another piece instead starts the selection again.
        assert_eq!(t.gesture(&Gesture::pos("c1")), Outcome::Changed);
        assert_eq!(t.gestures, [Gesture::pos("c1")]);
        assert!(t.can_go_back());
        assert!(t.back());
        assert!(t.gestures.is_empty());
        assert!(!t.can_go_back());

        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Changed);
        assert_eq!(t.gesture(&Gesture::pos("b2")), Outcome::Ask);
        t.answered(complete(4));
        assert!(t.back(), "off the selection made");
        assert_eq!(t.stage, Stage::Choosing);
        assert!(t.made.is_empty());
        assert_eq!(t.options.len(), 3, "the options from before it");
        assert_eq!(
            t.gestures,
            [Gesture::pos("a1")],
            "to just before its last click: the piece is in hand again"
        );
        assert!(t.back());
        assert!(t.gestures.is_empty());
        assert!(!t.back(), "nothing more to take back");
    }

    #[test]
    fn an_impulse_of_several_selections_asks_after_each() {
        let mut t = Turn::new(0);
        t.answered(answer(
            0,
            vec![sel(
                "blue@d4",
                Some("d4"),
                &[("piece", "blue"), ("pos", "d4")],
            )],
        ));
        // One piece and one cell: a click is all it takes.
        assert_eq!(t.gesture(&Gesture::pos("d4")), Outcome::Ask);
        let second = sel("black@f6", Some("f6"), &[("piece", "black"), ("pos", "f6")]);
        let other = sel("white@f6", Some("f6"), &[("piece", "white"), ("pos", "f6")]);
        assert_eq!(t.answered(answer(0, vec![second, other])), Outcome::Changed);
        assert!(
            t.targets().is_empty(),
            "two pieces could go there: the piece is chosen first"
        );
        assert_eq!(t.choices().len(), 2);
        assert_eq!(t.gesture(&Gesture::pos("f6")), Outcome::Ignored);
        assert_eq!(t.gesture(&g("piece", "black")), Outcome::Changed);
        assert_eq!(t.gesture(&Gesture::pos("f6")), Outcome::Ask);
        t.answered(complete(0));
        assert_eq!(t.submit(), Some(("blue@d4; black@f6".to_string(), 0)));
        assert_eq!(t.placed().collect::<Vec<_>>(), ["d4", "f6"]);
    }

    #[test]
    fn a_move_with_nothing_to_choose_is_made_at_once_and_waits_to_be_confirmed() {
        let mut t = Turn::new(2);
        assert_eq!(
            t.answered(answer(2, vec![sel("pass", None, &[])])),
            Outcome::Ask
        );
        assert_eq!(t.answered(complete(2)), Outcome::Changed);
        assert_eq!(t.stage, Stage::Ready);
    }

    #[test]
    fn a_refused_move_starts_the_turn_again() {
        let mut t = turn();
        t.gesture(&Gesture::pos("a1"));
        t.answered(complete(1));
        t.submit();
        t.send_failed(Problem::Refused {
            status: 409,
            message: "moved on".into(),
        });
        assert_eq!(t.stage, Stage::Asking);
        assert!(t.made.is_empty());
        assert_eq!(t.asked_at, 1);
        assert!(t.problem.is_some());
        t.answered(answer(1, vec![place("a1")]));
        assert!(t.problem.is_none(), "a new answer clears it");
    }

    #[test]
    fn nothing_is_entered_while_waiting() {
        let mut t = Turn::new(0);
        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Ignored);
        t.ask_failed(Problem::Unreachable("gone".into()));
        assert_eq!(t.stage, Stage::Failed);
        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Ignored);
        assert!(!t.back());
    }

    #[test]
    fn a_right_click_goes_back_and_a_right_drag_does_not() {
        use bevy::picking::hover::HoverMap;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<HoverMap>()
            .init_resource::<WatchStatus>()
            .init_resource::<Drag>()
            .add_systems(Update, right_click);
        let mut t = Turn::new(1);
        t.answered(answer(4, vec![step("a1", "a2"), step("a1", "b2")]));
        t.gesture(&Gesture::pos("a1"));
        app.world_mut().resource_mut::<WatchStatus>().turn = Some(t);
        let gestures = |app: &App| {
            app.world()
                .resource::<WatchStatus>()
                .turn
                .as_ref()
                .unwrap()
                .gestures
                .len()
        };
        let button = |app: &mut App, down: bool, moved: f32| {
            app.world_mut()
                .resource_mut::<AccumulatedMouseMotion>()
                .delta = Vec2::new(moved, 0.0);
            let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            buttons.clear();
            if down {
                buttons.press(MouseButton::Right);
            } else {
                buttons.release(MouseButton::Right);
            }
            app.update();
        };

        // A drag turns the camera: the piece stays in hand.
        button(&mut app, true, 0.0);
        button(&mut app, true, 25.0);
        button(&mut app, false, 0.0);
        assert_eq!(gestures(&app), 1);

        // A click, the pointer hardly moving: the piece is put down.
        button(&mut app, true, 0.0);
        button(&mut app, true, 1.5);
        button(&mut app, false, 0.0);
        assert_eq!(gestures(&app), 0);
    }

    /// Fist's chains from a1: one jump that may end there, or go on to a second.
    fn chains() -> Vec<LegalSelection> {
        let mut all = Vec::new();
        for first in ["capture", "spare"] {
            all.push(jump(&[("c3", first, "b2")]));
            for second in ["capture", "spare"] {
                all.push(jump(&[("c3", first, "b2"), ("e5", second, "d4")]));
            }
        }
        all.push(jump(&[
            ("c3", "capture", "b2"),
            ("e5", "capture", "d4"),
            ("g7", "capture", "f6"),
        ]));
        all
    }

    fn click(t: &mut Turn, cell: &str) {
        assert_eq!(t.gesture(&Gesture::pos(cell)), Outcome::Changed);
    }

    #[test]
    fn play_ends_a_chain_that_may_end_or_go_on_and_sends_it_once_whole() {
        let mut t = Turn::new(1);
        t.answered(answer(1, chains()));
        assert_eq!(t.end(), Outcome::Ignored, "nothing chosen yet");
        click(&mut t, "a1");
        click(&mut t, "c3");
        // What happens to the piece jumped: two choices, so Play waits.
        assert_eq!(t.choices().len(), 2);
        assert_eq!(t.ending_choice(), None);
        assert_eq!(t.end(), Outcome::Ignored);
        assert!(!t.can_play());
        assert_eq!(t.gesture(&g("action", "capture")), Outcome::Changed);
        // Ending is the one choice that is no click; Play stands for it.
        assert_eq!(t.ending_choice(), Some(g("action", "end")));
        assert!(t.panel_choices().is_empty());
        assert_eq!(t.choices(), [g("action", "end")]);
        assert!(t.can_play());
        assert_eq!(t.submit(), None, "Play is not the plain submit here");
        assert_eq!(t.end(), Outcome::Ask);
        assert_eq!(t.answered(complete(1)), Outcome::Submit);
        assert_eq!(t.submit(), Some(("jump".to_string(), 1)));
    }

    #[test]
    fn a_chain_goes_on_by_a_click_and_play_ends_the_last_leg() {
        let mut t = Turn::new(1);
        t.answered(answer(1, chains()));
        click(&mut t, "a1");
        click(&mut t, "c3");
        t.gesture(&g("action", "capture"));
        click(&mut t, "e5");
        assert_eq!(t.ending_choice(), None, "the second leg still asks");
        t.gesture(&g("action", "capture"));
        assert_eq!(t.ending_choice(), Some(g("action", "end")));
        assert_eq!(t.end(), Outcome::Ask);
        assert_eq!(t.answered(complete(1)), Outcome::Submit);
    }

    #[test]
    fn if_the_move_is_not_whole_after_play_s_choice_nothing_is_sent() {
        let mut t = Turn::new(1);
        t.answered(answer(1, chains()));
        click(&mut t, "a1");
        click(&mut t, "c3");
        t.gesture(&g("action", "capture"));
        assert_eq!(t.end(), Outcome::Ask);
        // The server leads somewhere else: the turn goes on, and nothing is sent.
        assert_eq!(
            t.answered(answer(1, vec![place("a1"), place("b1")])),
            Outcome::Changed
        );
        assert_eq!(t.stage, Stage::Choosing);
        // A later whole answer is not sent by an old Play.
        t.gesture(&Gesture::pos("a1"));
        assert_eq!(t.answered(complete(1)), Outcome::Changed);
        assert_eq!(t.stage, Stage::Ready);
    }

    #[test]
    fn a_step_that_may_pin_or_not_is_played_without_pinning() {
        let step = |last: (&str, &str)| LegalSelection {
            from: Some("a1".into()),
            ..sel("step", Some("b2"), &[("pos", "a1"), ("pos", "b2"), last])
        };
        let mut t = Turn::new(1);
        t.answered(answer(
            1,
            vec![step(("action", "move")), step(("pos", "c3"))],
        ));
        click(&mut t, "a1");
        assert_eq!(t.ending_choice(), None, "no choice yet");
        click(&mut t, "b2");
        assert_eq!(t.ending_choice(), Some(g("action", "move")));
        assert_eq!(t.end(), Outcome::Ask);
        assert_eq!(t.answered(complete(1)), Outcome::Submit);
    }

    #[test]
    fn a_lone_pass_or_swap_at_the_start_of_a_selection_is_not_taken_by_play() {
        let pass = sel("pass", None, &[("action", "pass")]);
        let mut t = Turn::new(1);
        t.answered(answer(1, vec![pass, place("a1")]));
        assert_eq!(t.choices(), [g("action", "pass")]);
        assert_eq!(t.ending_choice(), None);
        assert_eq!(t.end(), Outcome::Ignored);
        assert!(!t.can_play());
        let mut t = turn();
        assert_eq!(t.end(), Outcome::Ignored);
    }

    #[test]
    fn a_turn_that_plays_on_a_click_has_no_choice_for_play_to_make() {
        let mut t = Turn::new(1);
        t.clicks_only = true;
        t.answered(answer(1, chains()));
        click(&mut t, "a1");
        click(&mut t, "c3");
        t.gesture(&g("action", "capture"));
        assert_eq!(t.ending_choice(), None);
    }
}
