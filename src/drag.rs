//! Dragging a piece on the board: pressed, carried under the pointer while the button is held
//! down, and let go where it goes. A click still picks a piece up and leaves it lifted over its
//! space (`hand.rs`); both work side by side, and nothing chooses between them.
//!
//! A press of the primary button on a space of the board (a `Spot`: its cell, or a piece
//! standing there) is kept here ([`Drag`]) until the button is let go, with the turn as it was
//! before it and whether the pointer has left the space pressed. Let go without leaving it, it
//! is a click, made by the code every click on the board is (`play::click_at`), a lifted piece
//! put down again included. Leaving the space with the button down makes it a drag, where a
//! press there may carry a piece (`Turn::grabs`): the piece is picked up, if it was not, and the
//! board holds it under the pointer (`hand::Held`, by `board.rs`), its targets lit as for a
//! click. Let go over a space it goes to (`Turn::carries_to`), the drag is the click there: the
//! same preview, waiting for the same confirmation (a game that plays on a click plays on the
//! drop). Let go on its own space, it stays lifted there. Let go anywhere else (a space it may
//! not go to or only aims at, off the board, over a panel), or a right-click, Backspace or the
//! escape menu while the button is down, and the turn is as it was before the press. A turn
//! that ends, or a position that changes, ends a press without a click.
//!
//! The pointer is Bevy's: the press is its `PointerPress` on the space, and the button let go is
//! its `PointerInput`, so a headset's ray (`ray_pointer.rs`), whose trigger is the pointer's
//! primary button, drags as the mouse does. Where the pointer is is the space it is over
//! (`play::Hovering`): what is carried is out of picking's way meanwhile. Whether a drag is
//! under way, and what it carries, is read here and nowhere else ([`Drag::carried`]).
//!
//! A patience's cards are dragged the same way ([`CardPress`], pressed and followed by
//! `card_table.rs`, which knows the cards): a press on a card or a pile's place, the pointer
//! leaving it with the button down lifting what a click there would (or taking up what a click
//! already lifted, pressed on its own pile), and let go over a pile a click would put them on,
//! the click there; anywhere else the turn is as it was before the press ([`let_go_cards`]).

use bevy::picking::events::PointerPress;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;

use crate::escape_menu::{self, EscapeMenu};
use crate::net::{Net, WatchStatus};
use crate::play::{self, Hovering, LineAim, Outcome, SideAim, Spot, Turn};

pub struct DragPlugin;

/// The drag's system, which the board orders before it shows the turn.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Dragging;

impl Plugin for DragPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Drag>()
            .init_resource::<Hovering>()
            .init_resource::<SideAim>()
            .init_resource::<LineAim>()
            .add_message::<PointerInput>()
            .add_observer(on_press)
            .add_systems(Update, follow.in_set(Dragging));
    }
}

/// A press of the pointer's primary button on the board, while the button is down: the one
/// place that knows whether a drag is under way.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct Drag(pub Option<Press>, pub Option<CardPress>);

impl Drag {
    /// The space whose piece is carried under the pointer, while one is: the board holds the
    /// piece in hand there (`Turn::held_at`) under the pointer.
    pub fn carried(&self) -> Option<&str> {
        self.0
            .as_ref()
            .filter(|p| p.carrying)
            .map(|p| p.spot.as_str())
    }

    /// Whether the button is down after a press on the board, a drag or not yet.
    pub fn pressed(&self) -> bool {
        self.0.is_some() || self.1.is_some()
    }

    /// The press whose cards are carried under the pointer, while they are.
    pub fn carried_cards(&self) -> Option<&CardPress> {
        self.1.as_ref().filter(|p| p.carrying)
    }
}

/// A press on a patience's cards: on a card, or on a pile's place.
#[derive(Debug, Clone, PartialEq)]
pub struct CardPress {
    /// The pointer pressed.
    pub pointer: PointerId,
    /// What was pressed: a card, or a pile's place.
    pub entity: Entity,
    /// Its pile.
    pub zone: String,
    /// The card's place from the top of its pile (1 for the top), when its face is shown.
    pub from_top: Option<usize>,
    /// The turn as it was before the press, to go back to.
    pub before: Option<Turn>,
    /// Whether the pointer has left what was pressed with the button down.
    pub left: bool,
    /// Whether it carries the cards in hand under the pointer.
    pub carrying: bool,
}

/// What letting go of a press on cards does with the pointer `over` a pile (or none):
/// `puts` says whether a click on a pile would put what is in hand on it.
pub fn let_go_cards(press: &CardPress, over: Option<&str>, puts: impl Fn(&str) -> bool) -> LetGo {
    if !press.left {
        return LetGo::Click(press.zone.clone());
    }
    if !press.carrying {
        return LetGo::Nothing;
    }
    match over {
        Some(at) if at != press.zone && puts(at) => LetGo::Drop(at.to_string()),
        _ => LetGo::Restore,
    }
}

/// What was pressed, and how far the press has gone.
#[derive(Debug, Clone, PartialEq)]
pub struct Press {
    /// The pointer pressed: the mouse, or a headset's ray.
    pub pointer: PointerId,
    /// The space pressed on.
    pub spot: String,
    /// The turn as it was before the press, to go back to.
    pub before: Option<Turn>,
    /// Whether the pointer has left the space pressed with the button down: a drag, not a
    /// click.
    pub left: bool,
    /// Whether it carries the piece in hand at `spot` under the pointer.
    pub carrying: bool,
}

/// What letting the button go does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LetGo {
    /// A click on the space pressed: the pointer never left it.
    Click(String),
    /// What is carried is let go where it goes: the click there.
    Drop(String),
    /// What is carried is let go on its own space: it stays lifted there.
    Stay,
    /// The turn goes back to how it was before the press.
    Restore,
    /// Nothing: a press that left what it pressed and carried nothing.
    Nothing,
}

/// What letting go of `press` with the pointer `over` a space (or none) does, by the turn as
/// it is now. `occupied` says whether a piece stands on a space (`Turn::carries_to`).
pub fn let_go(
    press: &Press,
    turn: Option<&Turn>,
    over: Option<&str>,
    occupied: impl Fn(&str) -> bool,
) -> LetGo {
    if !press.left {
        return LetGo::Click(press.spot.clone());
    }
    if !press.carrying {
        return LetGo::Nothing;
    }
    match over {
        Some(at) if at == press.spot => LetGo::Stay,
        Some(at) if turn.is_some_and(|t| t.carries_to(at, occupied(at))) => {
            LetGo::Drop(at.to_string())
        }
        _ => LetGo::Restore,
    }
}

/// A right-click, Backspace or the escape menu, while the button is down after a press on the
/// board: the press is let go of without a click, and the turn is as it was before it. Whether
/// there was one (so it is not also a step back).
pub fn cancel(drag: &mut ResMut<Drag>, watch: &mut ResMut<WatchStatus>) -> bool {
    if let Some(press) = drag.1.take() {
        if press.carrying {
            restore(watch, press.before);
        }
        return true;
    }
    let Some(press) = drag.0.take() else {
        return false;
    };
    if press.carrying {
        restore(watch, press.before);
    }
    true
}

/// The turn put back as it was before a press, `before`.
pub fn restore(watch: &mut ResMut<WatchStatus>, before: Option<Turn>) {
    if watch.turn != before {
        watch.turn = before;
    }
}

/// A press of the primary button on a space of the board begins one. Presses bubble up from a
/// piece's parts to the piece; the first space they reach is the one pressed.
fn on_press(
    press: On<PointerPress>,
    spots: Query<&Spot>,
    watch: Res<WatchStatus>,
    menu: Option<Res<EscapeMenu>>,
    mut drag: ResMut<Drag>,
) {
    if press.button != PointerButton::Primary
        || drag.pressed()
        || !escape_menu::closed(menu.as_deref())
    {
        return;
    }
    let Ok(spot) = spots.get(press.entity) else {
        return;
    };
    drag.0 = Some(Press {
        pointer: press.pointer.id,
        spot: spot.0.clone(),
        before: watch.turn.clone(),
        left: false,
        carrying: false,
    });
}

/// Follows a press while the button is down: the pointer leaving the space pressed (a drag,
/// picking up what a press there may carry), and the button let go (a click, a drop, or the
/// turn as it was). A turn that ends or a position that changes ends it; the escape menu
/// cancels it.
fn follow(
    mut drag: ResMut<Drag>,
    mut inputs: MessageReader<PointerInput>,
    hovering: Res<Hovering>,
    menu: Option<Res<EscapeMenu>>,
    (net, aim, line): (Res<Net>, Res<SideAim>, Res<LineAim>),
    mut watch: ResMut<WatchStatus>,
) {
    let Some(pointer) = drag.0.as_ref().map(|p| p.pointer) else {
        inputs.clear();
        return;
    };
    // Every input is read, so none is left for the next press.
    let released = inputs
        .read()
        .filter(|i| {
            i.pointer_id == pointer
                && matches!(i.action, PointerAction::Release(PointerButton::Primary))
        })
        .count()
        > 0;
    // The turn it was pressed in is over (played, or the position read again): nothing to
    // carry on or to go back to.
    let asked_at = |t: &Option<Turn>| t.as_ref().map(|t| t.asked_at);
    if drag
        .0
        .as_ref()
        .is_some_and(|p| p.before.is_some() && asked_at(&p.before) != asked_at(&watch.turn))
    {
        drag.0 = None;
        return;
    }
    if !escape_menu::closed(menu.as_deref()) {
        cancel(&mut drag, &mut watch);
        return;
    }
    let over = hovering.0.as_deref();
    // Out of the space pressed with the button down: a drag. What a press there may carry is
    // picked up, if it is not in hand already, and carried under the pointer.
    let press = drag.0.as_ref().expect("a press");
    if !press.left && over != Some(press.spot.as_str()) {
        let spot = press.spot.clone();
        let grabs = watch.turn.as_ref().is_some_and(|t| t.grabs(&spot));
        if grabs && watch.turn.as_ref().and_then(Turn::held_at) != Some(spot.as_str()) {
            play::click_at(&net, &mut watch, (&aim, &line), &spot);
        }
        let carrying = grabs && watch.turn.as_ref().and_then(Turn::held_at) == Some(spot.as_str());
        let press = drag.0.as_mut().expect("a press");
        press.left = true;
        press.carrying = carrying;
    }
    if !released {
        return;
    }
    let press = drag.0.take().expect("a press");
    let occupied = |at: &str| {
        watch
            .table
            .as_ref()
            .and_then(|t| t.state.state.cells.get(at))
            .is_some_and(|s| !s.is_empty())
    };
    match let_go(&press, watch.turn.as_ref(), over, occupied) {
        LetGo::Click(at) => {
            play::click_at(&net, &mut watch, (&aim, &line), &at);
        }
        LetGo::Drop(at) => {
            if play::click_at(&net, &mut watch, (&aim, &line), &at) == Outcome::Ignored {
                restore(&mut watch, press.before);
            }
        }
        LetGo::Restore => restore(&mut watch, press.before),
        LetGo::Stay | LetGo::Nothing => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Request;
    use crate::net::api::{Gesture, LegalSelection, Selections};
    use crate::play::Stage;
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::Pointer;
    use bevy::picking::pointer::Location;
    use std::sync::mpsc::Receiver;

    fn step(from: &str, to: &str) -> LegalSelection {
        LegalSelection {
            notation: format!("{from}-{to}"),
            action: "step".into(),
            from: Some(from.into()),
            to: Some(to.into()),
            gestures: vec![Gesture::pos(from), Gesture::pos(to)],
            ..Default::default()
        }
    }

    fn answer(options: Vec<LegalSelection>) -> Selections {
        Selections {
            seat: 1,
            ply: 3,
            made: vec![],
            options,
            complete: false,
            may_end: false,
            state: None,
        }
    }

    /// A rook on a1 that may go to a2 or a3, and a knight on b1 that may go to c3.
    fn chess() -> Turn {
        let mut t = Turn::new(1);
        t.answered(answer(vec![
            step("a1", "a2"),
            step("a1", "a3"),
            step("b1", "c3"),
        ]));
        t
    }

    struct Table {
        app: App,
        requests: Receiver<Request>,
        spots: Vec<(String, Entity)>,
    }

    impl Table {
        fn new(turn: Turn) -> Self {
            let (net, requests) = Net::for_test();
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, DragPlugin))
                .insert_resource(net)
                .init_resource::<WatchStatus>();
            app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
            let spots = ["a1", "a2", "a3", "b1", "c3", "e5"]
                .iter()
                .map(|s| {
                    (
                        s.to_string(),
                        app.world_mut().spawn(Spot(s.to_string())).id(),
                    )
                })
                .collect();
            app.update();
            Table {
                app,
                requests,
                spots,
            }
        }

        fn turn(&self) -> &Turn {
            self.app
                .world()
                .resource::<WatchStatus>()
                .turn
                .as_ref()
                .expect("a turn")
        }

        fn over(&mut self, spot: Option<&str>) {
            self.app.world_mut().resource_mut::<Hovering>().0 = spot.map(String::from);
            self.app.update();
        }

        /// The primary button pressed over `spot`, the pointer over it.
        fn press(&mut self, spot: &str) {
            self.app.world_mut().resource_mut::<Hovering>().0 = Some(spot.into());
            let entity = self.spots.iter().find(|(s, _)| s == spot).unwrap().1;
            let event = PointerPress {
                entity,
                pointer: Pointer::new(PointerId::Mouse, nowhere()),
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                count: 1,
            };
            self.app.world_mut().trigger(event);
            self.app.update();
        }

        fn release(&mut self) {
            self.app.world_mut().write_message(PointerInput::new(
                PointerId::Mouse,
                nowhere(),
                PointerAction::Release(PointerButton::Primary),
            ));
            self.app.update();
        }

        fn carried(&self) -> Option<String> {
            self.app
                .world()
                .resource::<Drag>()
                .carried()
                .map(String::from)
        }

        fn asked(&self) -> usize {
            self.requests.try_iter().count()
        }
    }

    fn nowhere() -> Location {
        Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        }
    }

    #[test]
    fn a_press_a_move_off_the_space_and_a_release_on_a_target_make_the_two_gestures() {
        let mut table = Table::new(chess());
        table.press("a1");
        assert!(
            table.turn().gestures.is_empty(),
            "a press alone does nothing"
        );
        assert_eq!(table.carried(), None);
        // Off its space with the button down: picked up and carried under the pointer.
        table.over(Some("a2"));
        assert_eq!(table.turn().gestures, [Gesture::pos("a1")]);
        assert_eq!(table.turn().held_at(), Some("a1"));
        assert_eq!(table.carried().as_deref(), Some("a1"));
        table.over(None);
        table.over(Some("a3"));
        table.release();
        // The click on a3, after the click on a1: the selection is made, and asked about.
        assert_eq!(table.turn().made_notations(), ["a1-a3"]);
        assert_eq!(table.turn().stage, Stage::Asking);
        assert_eq!(table.asked(), 1);
        assert!(!table.app.world().resource::<Drag>().pressed());
    }

    #[test]
    fn a_drop_leaves_the_move_waiting_to_be_confirmed() {
        let mut table = Table::new(chess());
        table.press("b1");
        table.over(Some("c3"));
        table.release();
        assert_eq!(table.asked(), 1);
        // The server says the move is whole: it waits, as after a click, for the second click.
        let whole = Selections {
            made: vec!["b1-c3".into()],
            complete: true,
            ..answer(vec![])
        };
        let answered = table
            .app
            .world_mut()
            .resource_mut::<WatchStatus>()
            .turn
            .as_mut()
            .unwrap()
            .answered(whole);
        assert_eq!(answered, Outcome::Changed, "not sent");
        table.app.update();
        assert_eq!(table.asked(), 0, "nothing played");
        assert_eq!(table.turn().stage, Stage::Ready);
    }

    #[test]
    fn a_release_anywhere_else_restores_the_turn() {
        for elsewhere in [Some("e5"), Some("c3"), None] {
            let mut table = Table::new(chess());
            let before = table.turn().clone();
            table.press("a1");
            table.over(Some("a2"));
            assert_eq!(table.carried().as_deref(), Some("a1"));
            table.over(elsewhere);
            table.release();
            assert_eq!(*table.turn(), before, "let go over {elsewhere:?}");
            assert_eq!(table.carried(), None);
            assert_eq!(table.asked(), 0);
        }
    }

    #[test]
    fn a_press_and_release_on_the_space_is_still_a_click() {
        let mut table = Table::new(chess());
        // On a piece: picked up, and it stays lifted.
        table.press("a1");
        table.release();
        assert_eq!(table.turn().held_at(), Some("a1"));
        assert_eq!(table.carried(), None, "nothing under the pointer");
        // On it again: put down, as a click does.
        table.press("a1");
        table.release();
        assert!(table.turn().gestures.is_empty());
        // Picked up, then a click on where it goes.
        table.press("a1");
        table.release();
        table.press("a2");
        table.release();
        assert_eq!(table.turn().made_notations(), ["a1-a2"]);
    }

    #[test]
    fn a_piece_let_go_on_its_own_space_stays_lifted_there() {
        let mut table = Table::new(chess());
        table.press("a1");
        table.over(Some("a2"));
        table.over(Some("a1"));
        table.release();
        assert_eq!(table.turn().gestures, [Gesture::pos("a1")], "still in hand");
        assert_eq!(table.carried(), None);
    }

    #[test]
    fn a_piece_lifted_by_a_click_is_dragged_on_from_where_it_hovers() {
        let mut table = Table::new(chess());
        table.press("a1");
        table.release();
        let lifted = table.turn().clone();
        table.press("a1");
        table.over(Some("a2"));
        assert_eq!(table.turn().gestures, [Gesture::pos("a1")], "not put down");
        assert_eq!(table.carried().as_deref(), Some("a1"));
        // Let go off the board: lifted, as it was before the press.
        table.over(None);
        table.release();
        assert_eq!(*table.turn(), lifted);
        // Again, onto where it goes.
        table.press("a1");
        table.over(Some("a2"));
        table.release();
        assert_eq!(table.turn().made_notations(), ["a1-a2"]);
    }

    #[test]
    fn a_right_click_or_the_escape_menu_cancels_a_drag() {
        let mut table = Table::new(chess());
        let before = table.turn().clone();
        table.press("a1");
        table.over(Some("a2"));
        let cancelled = table
            .app
            .world_mut()
            .run_system_cached(|mut drag: ResMut<Drag>, mut watch: ResMut<WatchStatus>| {
                cancel(&mut drag, &mut watch)
            })
            .unwrap();
        assert!(cancelled);
        assert_eq!(*table.turn(), before);
        table.release();
        assert_eq!(
            *table.turn(),
            before,
            "the release that follows does nothing"
        );

        table.app.insert_resource(EscapeMenu {
            open: false,
            ..Default::default()
        });
        table.press("a1");
        table.over(Some("a2"));
        table.app.world_mut().resource_mut::<EscapeMenu>().open = true;
        table.app.update();
        assert_eq!(*table.turn(), before);
        assert!(!table.app.world().resource::<Drag>().pressed());
    }

    #[test]
    fn a_press_ends_with_its_turn() {
        let mut table = Table::new(chess());
        table.press("a1");
        table.over(Some("a2"));
        // The position changed: a new turn.
        table.app.world_mut().resource_mut::<WatchStatus>().turn = Some(Turn::new(2));
        table.app.update();
        assert!(!table.app.world().resource::<Drag>().pressed());
        table.release();
        assert_eq!(table.turn().asked_at, 2);
    }

    #[test]
    fn a_press_off_a_target_that_carries_nothing_is_no_click() {
        let mut table = Table::new(chess());
        table.press("a1");
        table.release();
        // Pressed on a2, where it goes, then away and let go: no click, as on any button.
        table.press("a2");
        table.over(Some("a3"));
        table.release();
        assert_eq!(table.turn().gestures, [Gesture::pos("a1")]);
    }

    /// Fist's chains from a1 as the server lists them: a jump over b2 to c3, captured or
    /// spared, that may end there or jump on over d4 to e5.
    fn chains() -> Turn {
        let jump = |legs: &[(&str, &str)]| {
            let mut gestures = vec![Gesture::pos("a1")];
            for (land, choice) in legs {
                gestures.push(Gesture::pos(land));
                gestures.push(Gesture {
                    kind: "action".into(),
                    value: choice.to_string(),
                });
            }
            gestures.push(Gesture {
                kind: "action".into(),
                value: "end".into(),
            });
            LegalSelection {
                notation: "jump".into(),
                action: "jump".into(),
                from: Some("a1".into()),
                to: legs.last().map(|(l, _)| l.to_string()),
                path: legs.iter().map(|(l, _)| l.to_string()).collect(),
                gestures,
                ..Default::default()
            }
        };
        let mut t = Turn::new(1);
        let mut all = Vec::new();
        for first in ["capture", "spare"] {
            all.push(jump(&[("c3", first)]));
            for second in ["capture", "spare"] {
                all.push(jump(&[("c3", first), ("e5", second)]));
            }
        }
        t.answered(answer(all));
        t
    }

    fn choose(table: &mut Table, choice: &str) {
        let g = Gesture {
            kind: "action".into(),
            value: choice.into(),
        };
        let outcome = table
            .app
            .world_mut()
            .resource_mut::<WatchStatus>()
            .turn
            .as_mut()
            .unwrap()
            .gesture(&g);
        assert_ne!(outcome, Outcome::Ignored, "{choice}");
    }

    #[test]
    fn a_jumper_dropped_on_its_first_landing_stands_there_and_is_dragged_on_from_there() {
        let mut table = Table::new(chains());
        table.press("a1");
        table.over(Some("b1"));
        table.over(Some("c3"));
        table.release();
        let t = table.turn();
        assert_eq!(t.legs(), ["c3"]);
        assert_eq!(t.stands_at(), Some("c3"));
        let jumper = t.carried().into_iter().last().unwrap();
        assert_eq!((jumper.from, jumper.to), ("a1", Some("c3")), "stands on c3");
        // What happens to the piece jumped is asked: nothing is in hand meanwhile.
        assert_eq!(t.held_at(), None);
        assert!(!t.grabs("c3"));
        choose(&mut table, "capture");
        // It may end here or jump on: it hovers over c3, and is dragged on from there.
        assert_eq!(table.turn().held_at(), Some("c3"));
        table.press("c3");
        table.over(Some("d4"));
        assert_eq!(table.carried().as_deref(), Some("c3"));
        table.over(Some("e5"));
        table.release();
        let t = table.turn();
        assert_eq!(t.legs(), ["c3", "e5"]);
        assert_eq!(t.carried().last().unwrap().to, Some("e5"));
        assert_eq!(t.held_at(), None, "the second jump's piece is asked about");
    }

    #[test]
    fn a_jumper_clicked_on_its_first_landing_stands_there_too() {
        let mut t = chains();
        assert_eq!(t.gesture(&Gesture::pos("a1")), Outcome::Changed);
        assert_eq!(t.held_at(), Some("a1"));
        assert!(t.legs().is_empty());
        assert_eq!(t.gesture(&Gesture::pos("c3")), Outcome::Changed);
        assert_eq!(t.stands_at(), Some("c3"));
        assert_eq!(t.carried()[0].to, Some("c3"));
    }

    #[test]
    fn a_target_the_piece_only_aims_at_is_no_drop() {
        // A step to b2 that may pin the piece on c3 (Fist's), or an arrow shot (Amazons').
        let pin = LegalSelection {
            notation: "a1-b2xc3".into(),
            action: "move".into(),
            from: Some("a1".into()),
            to: Some("b2".into()),
            aim: Some("c3".into()),
            gestures: vec![Gesture::pos("a1"), Gesture::pos("b2"), Gesture::pos("c3")],
            ..Default::default()
        };
        let mut t = Turn::new(1);
        t.answered(answer(vec![pin, step("a1", "a2")]));
        t.gesture(&Gesture::pos("a1"));
        assert!(t.carries_to("b2", false) && t.carries_to("a2", false));
        assert!(!t.carries_to("c3", false), "not a target yet");
        t.gesture(&Gesture::pos("b2"));
        assert_eq!(t.held_at(), None, "put down on b2: the pin is a click");
        assert!(!t.carries_to("c3", true));
        // The piece every click moves acts on a piece where one stands, and does not go there.
        let strike = LegalSelection {
            notation: "x".into(),
            action: "strike".into(),
            from: Some("a1".into()),
            to: Some("b2".into()),
            gestures: vec![Gesture::pos("b2")],
            ..Default::default()
        };
        let walk = LegalSelection {
            notation: "w".into(),
            action: "move".into(),
            from: Some("a1".into()),
            to: Some("a2".into()),
            gestures: vec![Gesture::pos("a2")],
            ..Default::default()
        };
        let mut t = Turn::new(1);
        t.answered(answer(vec![strike, walk]));
        assert!(t.grabs("a1"));
        t.gesture(&Gesture::pos("a1"));
        assert_eq!(t.held_at(), Some("a1"));
        assert!(t.carries_to("a2", false));
        assert!(!t.carries_to("b2", true));
    }
}
