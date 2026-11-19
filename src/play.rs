//! Playing a seat: the move being built, from the legal selections the server lists, as the
//! player makes gestures (Bingle's ADR 0114). The client decides nothing about legality: it asks
//! `GET …/selections` what may be selected, narrows that list by the gestures made, and sends the
//! move the list allowed, by its notation.
//!
//! A [`Turn`] is plain data with no Bevy in it, so the whole of entering a move is tested without
//! a window. The systems at the end turn clicks on the board, keys and the panel's buttons into
//! gestures and commands on it, and send what it asks for to the network thread.
//!
//! A click on a position stands for the choices that must come before it when only one way to
//! get there is legal: in a game where a stone is placed by choosing the stone and then the cell,
//! a click on the cell is enough (the web page takes such forced choices by itself too). The
//! selection that ends the impulse is previewed, and waits for the player to confirm it.

use std::collections::BTreeSet;

use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;

use crate::net::api::{Gesture, LegalSelection, Selections};
use crate::net::session::Problem;
use crate::net::{self, Net, WatchStatus};

pub struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hovering>()
            .add_observer(on_click)
            .add_observer(on_over)
            .add_observer(on_out)
            .add_systems(
                Update,
                (start_turn.run_if(resource_changed::<WatchStatus>), keys),
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

/// The impulse this client's seat is on, and the move being built for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    /// The table's `move_seq` when the turn began, so an answer for an earlier one is known.
    pub asked_at: u64,
    /// The ply the server says the game is at, once it has answered.
    pub ply: Option<u64>,
    /// The selections made so far, in order.
    pub made: Vec<LegalSelection>,
    /// What could be selected before each of those, to go back to.
    before: Vec<Vec<LegalSelection>>,
    /// What may be selected now.
    pub options: Vec<LegalSelection>,
    /// The gestures made towards the next selection.
    pub gestures: Vec<Gesture>,
    /// How many of those were taken for the player because nothing else was possible: going
    /// back does not take them off.
    forced: usize,
    pub stage: Stage,
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

    /// Where the selections made put something, to show it there before it is sent.
    pub fn placed(&self) -> impl Iterator<Item = &str> {
        self.made.iter().filter_map(|s| s.to.as_deref())
    }

    /// The server's answer to the last question.
    pub fn answered(&mut self, selections: Selections) -> Outcome {
        if self.stage != Stage::Asking {
            return Outcome::Ignored;
        }
        self.ply = Some(selections.ply);
        self.problem = None;
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
                    && self.made.last().and_then(|s| s.to.as_ref()) == Some(&gesture.value)
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

    /// Back, as Esc or the panel's button: first the gestures towards a selection, then the
    /// selections made, one at a time. Nothing is sent, so nothing needs undoing on the server.
    pub fn back(&mut self) -> bool {
        match self.stage {
            Stage::Choosing if self.gestures.len() > self.forced => {
                self.gestures.truncate(self.forced);
                true
            }
            Stage::Choosing | Stage::Ready | Stage::Asking => match self.before.pop() {
                Some(options) => {
                    self.made.pop();
                    self.options = options;
                    self.stage = Stage::Choosing;
                    self.start_selection();
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

    /// The player confirms the move: what to send, and the ply it was made at.
    pub fn submit(&mut self) -> Option<(String, u64)> {
        let ply = self.ply?;
        if self.stage != Stage::Ready {
            return None;
        }
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

    fn enter(&mut self, gesture: &Gesture) -> Outcome {
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
        self.settle()
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
        self.before.push(std::mem::take(&mut self.options));
        self.made.push(chosen);
        self.gestures.clear();
        self.stage = Stage::Asking;
        Outcome::Ask
    }
}

/// The position the pointer is over, if it is one on the board.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct Hovering(pub Option<String>);

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
    net: Res<Net>,
    mut watch: ResMut<WatchStatus>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    // Clicks bubble up to the board; only the cell or piece itself is a position.
    if let Ok(spot) = spots.get(click.entity) {
        gesture(&net, &mut watch, &Gesture::pos(&spot.0));
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

/// Enter plays the move shown; Esc and Backspace go back.
fn keys(keys: Res<ButtonInput<KeyCode>>, net: Res<Net>, mut watch: ResMut<WatchStatus>) {
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
        submit(&net, &mut watch);
    }
    if keys.any_just_pressed([KeyCode::Escape, KeyCode::Backspace]) {
        back(&mut watch);
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
            gestures: gestures.iter().map(|(k, v)| g(k, v)).collect(),
        }
    }

    fn place(cell: &str) -> LegalSelection {
        sel(cell, Some(cell), &[("piece", "stone"), ("pos", cell)])
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
        }
    }

    fn complete(ply: u64) -> Selections {
        Selections {
            complete: true,
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
}
