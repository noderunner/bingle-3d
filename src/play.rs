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

use std::collections::{BTreeSet, HashMap};

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::input_focus::InputFocus;
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;

use crate::camera::{PIXELS_PER_LINE, PointerOverUi, WheelTaken};
use crate::escape_menu::{self, EscapeMenu};
use crate::net::api::{Gesture, LegalSelection, Selections};
use crate::net::session::Problem;
use crate::net::{self, Net, WatchStatus};

pub struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hovering>()
            .init_resource::<WheelTaken>()
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
    /// What could be selected before each of those, and whether the move could end there, to go
    /// back to.
    before: Vec<(Vec<LegalSelection>, bool)>,
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
        if self.stage != Stage::Choosing || StackMove::of(&self.gestures).is_some() {
            return out;
        }
        let clicked = |at: &str| self.gestures.contains(&Gesture::pos(at));
        let agreed = |of: fn(&LegalSelection) -> Option<&String>| {
            let mut all = self.matching().map(of);
            let first = all.next()??;
            all.all(|o| o == Some(first))
                .then_some(first.as_str())
                .filter(|at| clicked(at))
        };
        if let Some(from) = agreed(|s| s.from.as_ref()) {
            out.push(Carried {
                from,
                to: agreed(|s| s.to.as_ref()),
            });
        }
        out
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

    /// Whether the wheel would change anything now (so the camera leaves it alone).
    pub fn wheel_changes(&self, pools: &HashMap<String, String>) -> bool {
        [true, false]
            .iter()
            .any(|up| self.clone().wheel(*up, pools) != Outcome::Ignored)
    }

    /// Whether the player has clicked towards the selection being entered: a piece picked up,
    /// say, whose moves the board then lights.
    pub fn holding(&self) -> bool {
        self.stage == Stage::Choosing && (self.last_click().is_some() || self.in_hand().is_some())
    }

    /// The kind of piece the player has taken to place (from its bowl, say), while it waits for
    /// where it goes: a choice of piece the player made, not one taken for them.
    pub fn in_hand(&self) -> Option<&str> {
        if self.stage != Stage::Choosing {
            return None;
        }
        let i = self.gestures.iter().rposition(|g| g.kind == "piece")?;
        let after = &self.gestures[i + 1..];
        (i >= self.forced && !after.iter().any(Gesture::is_pos))
            .then(|| self.gestures[i].value.as_str())
    }

    /// What the selections made place (not move): where, and the kind of piece when they say.
    pub fn placements(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.made
            .iter()
            .filter(|s| s.from.is_none())
            .filter_map(|s| Some((s.to.as_deref()?, s.piece.as_deref())))
    }

    /// Whether the next click is where what follows a move goes (an arrow shot from where the
    /// piece landed), for the board to put it in the player's hand.
    pub fn aiming(&self) -> bool {
        if self.stage != Stage::Choosing {
            return false;
        }
        let n = self.gestures.len();
        let mut options = self.matching().peekable();
        options.peek().is_some()
            && options.all(|o| {
                let next = o.gestures[n..].iter().find(|g| g.is_pos());
                next.is_some() && next.map(|g| &g.value) == o.aim.as_ref()
            })
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

    /// The server's answer to the last question.
    pub fn answered(&mut self, selections: Selections) -> Outcome {
        if self.stage != Stage::Asking {
            return Outcome::Ignored;
        }
        self.ply = Some(selections.ply);
        self.problem = None;
        self.may_end = selections.may_end && !selections.made.is_empty();
        if selections.complete {
            self.stage = Stage::Ready;
            return Outcome::Changed;
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
            ..Turn::new(self.asked_at)
        };
    }

    /// A gesture from the player.
    pub fn gesture(&mut self, gesture: &Gesture) -> Outcome {
        match self.stage {
            Stage::Choosing => self.enter(gesture),
            Stage::Ready => {
                // A click on the preview plays it, as a second click does on the web page.
                if gesture.is_pos()
                    && self
                        .made
                        .last()
                        .and_then(|s| s.aim.as_ref().or(s.to.as_ref()))
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

    /// Back, as Backspace or the panel's button: first the gestures towards a selection, then the
    /// selections made, one at a time, each to just before its last click (a piece moved and
    /// its arrow not yet shot). Nothing is sent, so nothing needs undoing on the server.
    pub fn back(&mut self) -> bool {
        match self.stage {
            Stage::Choosing if self.gestures.len() > self.forced => {
                self.gestures
                    .truncate(self.last_click().unwrap_or(self.forced));
                true
            }
            Stage::Choosing | Stage::Ready | Stage::Asking => match self.before.pop() {
                Some((options, may_end)) => {
                    self.may_end = may_end;
                    let undone = self.made.pop();
                    self.options = options;
                    self.stage = Stage::Choosing;
                    self.start_selection();
                    if let Some(s) = undone
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
            Stage::Choosing => self.gestures.len() > self.forced || !self.made.is_empty(),
            Stage::Ready => true,
            _ => false,
        }
    }

    /// Whether the move may be sent now: it is whole, or it may end where it is (what is half
    /// entered towards another selection is let go).
    pub fn can_play(&self) -> bool {
        match self.stage {
            Stage::Ready => true,
            Stage::Choosing => self.may_end && !self.made.is_empty(),
            _ => false,
        }
    }

    /// The player confirms the move: what to send, and the ply it was made at.
    pub fn submit(&mut self) -> Option<(String, u64)> {
        let ply = self.ply?;
        if !self.can_play() {
            return None;
        }
        self.gestures.clear();
        self.stage = Stage::Sending;
        Some((self.notation(), ply))
    }

    /// The positions a click on would be taken, for the board to show.
    pub fn targets(&self) -> BTreeSet<String> {
        if self.stage != Stage::Choosing {
            return BTreeSet::new();
        }
        let n = self.gestures.len();
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
        // A click again on the position last clicked, where it means nothing more, takes that
        // click back: a piece picked up is put down again.
        if let Some(i) = self.last_click()
            && self.gestures[i] == *gesture
            && self.path_to(gesture).is_none()
        {
            self.gestures.truncate(i);
            return Outcome::Changed;
        }
        // So does the same choice again (a stone's bowl clicked twice): the stone goes back.
        if gesture.kind == "piece"
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
        self.settle()
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
        self.gestures.clear();
        self.take_forced();
        self.forced = self.gestures.len();
    }

    /// Takes a choice from a list for the player when every selection still possible makes it.
    fn take_forced(&mut self) {
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
        self.before
            .push((std::mem::take(&mut self.options), self.may_end));
        self.made.push(chosen);
        self.gestures.clear();
        self.stage = Stage::Asking;
        Outcome::Ask
    }
}

/// The position the pointer is over, if it is one on the board.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct Hovering(pub Option<String>);

/// A choice a click on something other than the board makes (a stone's bowl: a stone of its
/// colour).
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Choice(pub Gesture);

/// A position of the board, on its cell and on every piece that stands there: what a click on
/// either means.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Spot(pub String);

/// When this client's seat is on impulse, a turn begins and the server is asked what it may
/// select; when it is not, there is no turn.
pub(crate) fn start_turn(net: Res<Net>, mut watch: ResMut<WatchStatus>) {
    let seq = watch.table.as_ref().map(|t| t.state.move_seq);
    let ours = watch.table.as_ref().is_some_and(|t| t.to_play());
    match (&watch.turn, ours) {
        (Some(_), false) => watch.turn = None,
        (Some(turn), true) if Some(turn.asked_at) == seq => {}
        (_, true) => {
            watch.turn = Some(Turn::new(seq.unwrap_or_default()));
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

/// A gesture, from the board or the panel.
pub fn gesture(net: &Net, watch: &mut ResMut<WatchStatus>, gesture: &Gesture) {
    let Some(turn) = watch.bypass_change_detection().turn.as_mut() else {
        return;
    };
    let outcome = turn.gesture(gesture);
    if outcome != Outcome::Ignored {
        watch.set_changed();
    }
    follow(net, watch, outcome);
}

/// The player confirms the move.
pub fn submit(net: &Net, watch: &mut ResMut<WatchStatus>) {
    let Some(turn) = watch.bypass_change_detection().turn.as_mut() else {
        return;
    };
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
        let asked_at = turn.asked_at;
        *turn = Turn::new(asked_at);
        net::ask(net, watch);
    }
}

fn on_click(
    click: On<PointerClick>,
    spots: Query<&Spot>,
    choices: Query<&Choice>,
    net: Res<Net>,
    mut watch: ResMut<WatchStatus>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    // Clicks bubble up to the board; only the cell or piece itself is a position. A bowl's
    // choice is the bowl's, whether the bowl or the stones in it were clicked.
    if let Ok(spot) = spots.get(click.entity) {
        gesture(&net, &mut watch, &Gesture::pos(&spot.0));
    } else if let Ok(choice) = choices.get(click.entity) {
        gesture(&net, &mut watch, &choice.0);
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
/// field has the keyboard, nor while the escape menu is open.
fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    menu: Option<Res<EscapeMenu>>,
    net: Res<Net>,
    mut watch: ResMut<WatchStatus>,
) {
    if focus.is_some_and(|f| f.get().is_some()) || !escape_menu::closed(menu.as_deref()) {
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
/// escape menu is open. `pressed` is how far the pointer has moved since the button went down
/// over the table.
fn right_click(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    pointer: PointerOverUi,
    menu: Option<Res<EscapeMenu>>,
    mut pressed: Local<Option<f32>>,
    mut watch: ResMut<WatchStatus>,
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
    mut watch: ResMut<WatchStatus>,
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
        assert!(!t.aiming());

        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Changed);
        assert_eq!(
            t.carried(),
            [Carried {
                from: "a1",
                to: None
            }],
            "in hand, not yet put anywhere"
        );
        assert!(!t.aiming(), "where it goes comes first");
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
        assert!(t.aiming());
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
        assert!(t.aiming());
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

    fn answer(ply: u64, options: Vec<LegalSelection>) -> Selections {
        Selections {
            seat: 1,
            ply,
            made: vec![],
            options,
            complete: false,
            may_end: false,
        }
    }

    fn complete(ply: u64) -> Selections {
        Selections {
            complete: true,
            may_end: false,
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
}
