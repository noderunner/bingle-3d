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
//! What lies beside the board (a bowl, a seat's supply, the bowl of fences, the stock: a
//! [`Source`]) is pressed the same way ([`Beside`]). The pointer leaving what was pressed with
//! the button down takes up what a click there takes, unless it is in hand already (a stone
//! lifted over its bowl, pressed there or on the stone; the stone the turn holds by itself,
//! where nothing is chosen), and the board holds it under the pointer. Let go over the board,
//! it is the click there (a fence on the side aimed at); anywhere else, or where the click is
//! refused, the turn is as it was before the press. Let go without leaving, it is the click
//! `play::on_click` makes.
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
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;

use crate::escape_menu::{self, EscapeMenu};
use crate::net::api::Gesture;
use crate::net::{Net, WatchStatus};
use crate::play::{
    self, Choice, Hovering, LineAim, Outcome, SideAim, Spot, TakeToPlace, TakeUp, Turn,
};

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
            .filter(|p| p.carrying && p.beside.is_none())
            .map(|p| p.spot.as_str())
    }

    /// Whether what waits in hand beside the board (over a bowl, a supply, the stock) is
    /// carried under the pointer.
    pub fn carried_beside(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|p| p.carrying && p.beside.is_some())
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
    /// What lies beside the board, pressed instead of a space (`spot` is then empty).
    pub beside: Option<Beside>,
}

/// What lies beside the board that a press may carry something from: a bowl, a seat's supply,
/// the bowl of fences, a piece of the stock, and what waits in hand over them. The board marks
/// them; a move card's choice is not one.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Source;

/// A press on a [`Source`]: what was pressed, and what a click there takes up.
#[derive(Debug, Clone, PartialEq)]
pub struct Beside {
    pub entity: Entity,
    pub takes: Takes,
}

/// What a click on a [`Source`] takes up, as `play::on_click` makes it.
#[derive(Debug, Clone, PartialEq)]
pub enum Takes {
    /// A bowl's or a supply's choice (`play::Choice`).
    Choice(Gesture),
    /// Something a later click puts on a side (`play::TakeUp`).
    TakeUp(Vec<Gesture>),
    /// A piece a click on the board lays (`play::TakeToPlace`).
    TakeToPlace(Vec<Gesture>),
    /// Nothing: what waits in hand already, or the bowl of the stone the turn holds by itself.
    InHand,
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

/// What letting go of a press on what lies beside the board does, with the pointer `over` a
/// space (or none). Let go without leaving it, it is `play::on_click`'s click, not this. Over
/// a space a click puts what is carried on (`Turn::targets`), or anywhere on the board with a
/// side `aimed` at for what is taken up, the click there; anywhere else, the turn as it was.
pub fn let_go_beside(press: &Press, turn: Option<&Turn>, over: Option<&str>, aimed: bool) -> LetGo {
    if !press.left || !press.carrying {
        return LetGo::Nothing;
    }
    let puts = |t: &Turn, at: &str| t.targets().contains(at) || (aimed && t.taken().is_some());
    match over {
        Some(at) if turn.is_some_and(|t| puts(t, at)) => LetGo::Drop(at.to_string()),
        _ => LetGo::Restore,
    }
}

/// Whether what `takes` names is in hand already, so leaving it with the button down takes
/// nothing more: the stone the turn holds by itself, the choice in hand, or what is taken up.
fn in_hand(turn: &Turn, takes: &Takes) -> bool {
    turn.auto_hand().is_some()
        || match takes {
            Takes::Choice(g) => turn.in_hand_choice() == Some(g),
            Takes::TakeUp(c) | Takes::TakeToPlace(c) => turn
                .taken()
                .is_some_and(|t| t.iter().all(|g| c.contains(g))),
            Takes::InHand => true,
        }
}

/// Takes up what `takes` names, as a click on it does, unless it is in hand already. Whether
/// anything waits in hand beside the board then, to carry.
fn take_beside(net: &Net, watch: &mut ResMut<WatchStatus>, takes: &Takes) -> bool {
    let Some(turn) = watch.turn.as_ref() else {
        return false;
    };
    if !in_hand(turn, takes) {
        let mut take = |f: &dyn Fn(&mut Turn) -> Outcome| {
            if let Some(turn) = watch.bypass_change_detection().turn.as_mut()
                && f(turn) != Outcome::Ignored
            {
                watch.set_changed();
            }
        };
        match takes {
            Takes::Choice(g) => {
                play::gesture(net, watch, g);
            }
            Takes::TakeUp(c) => take(&|t| t.take_up(c)),
            Takes::TakeToPlace(c) => take(&|t| t.take_to_place(c)),
            Takes::InHand => {}
        }
    }
    watch.turn.as_ref().is_some_and(|t| {
        t.auto_hand().is_some() || t.in_hand_choice().is_some() || t.taken().is_some()
    })
}

/// Whether `pointer` is still over `pressed` (or a part of it), by picking's hover map.
fn still_over(
    map: Option<&HoverMap>,
    pointer: PointerId,
    pressed: Entity,
    parents: &Query<&ChildOf>,
) -> bool {
    map.and_then(|m| m.get(&pointer)).is_some_and(|hits| {
        hits.keys()
            .any(|e| *e == pressed || parents.iter_ancestors(*e).any(|a| a == pressed))
    })
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

/// What a click on a [`Source`] makes, as `play::on_click` finds it.
type SourceClick = (
    Option<&'static Choice>,
    Option<&'static TakeUp>,
    Option<&'static TakeToPlace>,
);

/// A press of the primary button on a space of the board, or on what lies beside it (a
/// [`Source`]), begins one. Presses bubble up from a piece's parts to the piece (a bowl's heap
/// to the bowl); the first space or source they reach is the one pressed.
fn on_press(
    press: On<PointerPress>,
    spots: Query<&Spot>,
    sources: Query<SourceClick, With<Source>>,
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
    let (spot, beside) = if let Ok(spot) = spots.get(press.entity) {
        (spot.0.clone(), None)
    } else if let Ok((choice, take_up, to_place)) = sources.get(press.entity) {
        let takes = match (choice, take_up, to_place) {
            (Some(c), ..) => Takes::Choice(c.0.clone()),
            (_, Some(t), _) => Takes::TakeUp(t.0.clone()),
            (.., Some(t)) => Takes::TakeToPlace(t.0.clone()),
            _ => Takes::InHand,
        };
        let entity = press.entity;
        (String::new(), Some(Beside { entity, takes }))
    } else {
        return;
    };
    drag.0 = Some(Press {
        pointer: press.pointer.id,
        spot,
        before: watch.turn.clone(),
        left: false,
        carrying: false,
        beside,
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
    (hover, parents): (Option<Res<HoverMap>>, Query<&ChildOf>),
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
    // Off what lies beside the board with the button down: what a click there takes up is
    // taken, if it is not in hand already, and carried.
    if let Some(beside) = press.beside.as_ref().filter(|_| !press.left)
        && !still_over(hover.as_deref(), press.pointer, beside.entity, &parents)
    {
        let takes = beside.takes.clone();
        let carrying = take_beside(&net, &mut watch, &takes);
        let press = drag.0.as_mut().expect("a press");
        press.left = true;
        press.carrying = carrying;
    }
    let press = drag.0.as_ref().expect("a press");
    if press.beside.is_none() && !press.left && over != Some(press.spot.as_str()) {
        let spot = press.spot.clone();
        let grabs = watch.turn.as_ref().is_some_and(|t| t.grabs(&spot));
        if grabs && watch.turn.as_ref().and_then(Turn::in_hand_at) != Some(spot.as_str()) {
            play::click_at(&net, &mut watch, (&aim, &line), &spot);
        }
        let carrying =
            grabs && watch.turn.as_ref().and_then(Turn::in_hand_at) == Some(spot.as_str());
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
    let what = if press.beside.is_some() {
        let_go_beside(&press, watch.turn.as_ref(), over, aim.choice.is_some())
    } else {
        let_go(&press, watch.turn.as_ref(), over, occupied)
    };
    match what {
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
                .init_resource::<HoverMap>()
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

        /// Something beside the board a press may carry from, with what a click on it does.
        fn source(&mut self, click: impl Bundle) -> Entity {
            self.app.world_mut().spawn((Source, click)).id()
        }

        /// The pointer over `entity` (picking's hover map), or over nothing beside the board.
        fn hover(&mut self, entity: Option<Entity>) {
            let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
            let mut map = self.app.world_mut().resource_mut::<HoverMap>();
            map.clear();
            if let Some(e) = entity {
                map.insert(PointerId::Mouse, [(e, hit)].into_iter().collect());
            }
        }

        /// The primary button pressed over `entity`, beside the board, the pointer over it.
        fn press_on(&mut self, entity: Entity) {
            self.app.world_mut().resource_mut::<Hovering>().0 = None;
            self.hover(Some(entity));
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

        /// Off what was pressed, the button down: over nothing, then over `spot`, if any.
        fn leave(&mut self, spot: Option<&str>) {
            self.hover(None);
            self.app.update();
            self.over(spot);
        }

        /// A stone of `kind` taken from its bowl by a click.
        fn choose_piece(&mut self, kind: &str) {
            let mut watch = self.app.world_mut().resource_mut::<WatchStatus>();
            let outcome = watch.turn.as_mut().unwrap().gesture(&piece(kind));
            assert_eq!(outcome, Outcome::Changed);
            self.app.update();
        }

        fn carried_beside(&self) -> bool {
            self.app.world().resource::<Drag>().carried_beside()
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

    fn place(gestures: &[Gesture], to: &str) -> LegalSelection {
        LegalSelection {
            notation: format!("{}{to}", gestures[0].value),
            action: "place".into(),
            to: Some(to.into()),
            piece: gestures
                .iter()
                .find(|g| g.kind == "piece")
                .map(|g| g.value.clone()),
            gestures: gestures.to_vec(),
            ..Default::default()
        }
    }

    fn piece(id: &str) -> Gesture {
        Gesture {
            kind: "piece".into(),
            value: id.into(),
        }
    }

    fn action(id: &str) -> Gesture {
        Gesture {
            kind: "action".into(),
            value: id.into(),
        }
    }

    /// A seat's supply (Tak's): a flat stone or a capstone, either placed on a1 or a2.
    fn supply() -> Turn {
        let mut t = Turn::new(1);
        let mut all = Vec::new();
        for kind in ["flat", "cap"] {
            for at in ["a1", "a2"] {
                all.push(place(&[piece(kind), Gesture::pos(at)], at));
            }
        }
        t.answered(answer(all));
        t
    }

    #[test]
    fn a_bowls_choice_is_made_as_the_pointer_leaves_it_and_let_go_on_the_board_placed() {
        let mut table = Table::new(supply());
        let flats = table.source(Choice(piece("flat")));
        table.press_on(flats);
        assert!(
            table.turn().gestures.is_empty(),
            "a press alone does nothing"
        );
        // Let go on it: not a drag (the click is `play::on_click`'s).
        table.release();
        assert!(table.turn().gestures.is_empty());
        assert!(!table.app.world().resource::<Drag>().pressed());

        table.press_on(flats);
        table.leave(None);
        assert_eq!(
            table.turn().in_hand(),
            Some("flat"),
            "taken up as a click does"
        );
        assert!(table.carried_beside());
        assert_eq!(table.carried(), None, "nothing of the board's");
        table.over(Some("a2"));
        table.release();
        assert_eq!(table.turn().made_notations(), ["flata2"]);
        assert_eq!(table.asked(), 1);
        assert!(!table.carried_beside());
    }

    #[test]
    fn what_is_carried_from_beside_the_board_and_let_go_off_it_is_put_back() {
        // Let go off the board, over a space it may not go on, or the escape menu: as before.
        for elsewhere in [None, Some("e5"), Some("menu")] {
            let mut table = Table::new(supply());
            let before = table.turn().clone();
            let caps = table.source(Choice(piece("cap")));
            table.press_on(caps);
            table.leave(Some("a1"));
            assert_eq!(table.turn().in_hand(), Some("cap"));
            if elsewhere == Some("menu") {
                table.app.insert_resource(EscapeMenu {
                    open: true,
                    ..Default::default()
                });
                table.app.update();
            } else {
                table.over(elsewhere);
            }
            table.release();
            assert_eq!(*table.turn(), before, "let go over {elsewhere:?}");
            assert_eq!(table.asked(), 0);
            assert!(!table.carried_beside());
        }
    }

    #[test]
    fn a_stone_lifted_by_a_click_is_carried_from_where_it_hovers_or_its_bowl() {
        let mut table = Table::new(supply());
        table.choose_piece("flat");
        let lifted = table.turn().clone();
        let stone = table.source(());
        let flats = table.source(Choice(piece("flat")));
        for pressed in [stone, flats] {
            table.press_on(pressed);
            table.leave(None);
            assert_eq!(table.turn().in_hand(), Some("flat"), "not put back");
            assert!(table.carried_beside());
            table.release();
            assert_eq!(*table.turn(), lifted, "over its source again");
        }
        // Another kind's: it takes the place of the one in hand, as a click does.
        let caps = table.source(Choice(piece("cap")));
        table.press_on(caps);
        table.leave(Some("a1"));
        assert_eq!(table.turn().in_hand(), Some("cap"));
        table.release();
        assert_eq!(table.turn().made_notations(), ["capa1"]);
    }

    #[test]
    fn the_stone_held_by_itself_is_carried_with_nothing_chosen_and_placed_where_let_go() {
        // Go's: every click places a stone, which is chosen for the player.
        let mut t = Turn::new(1);
        t.answered(answer(
            ["a1", "a2"]
                .map(|at| place(&[piece("stone"), Gesture::pos(at)], at))
                .to_vec(),
        ));
        assert_eq!(t.auto_hand(), Some("stone"));
        let mut table = Table::new(t);
        let before = table.turn().clone();
        let bowl = table.source(());
        for pressed in [bowl, table.source(())] {
            table.press_on(pressed);
            table.leave(None);
            assert_eq!(*table.turn(), before, "nothing chosen");
            assert!(table.carried_beside());
            table.release();
            assert_eq!(*table.turn(), before);
        }
        table.press_on(bowl);
        table.leave(Some("a2"));
        table.release();
        assert_eq!(table.turn().made_notations(), ["stonea2"]);
        assert_eq!(table.asked(), 1);
    }

    #[test]
    fn a_press_on_what_gives_nothing_now_carries_nothing() {
        // Not this seat's bowl, or not its turn to choose one: no drag.
        let mut table = Table::new(chess());
        let bowl = table.source(Choice(piece("flat")));
        table.press_on(bowl);
        table.leave(Some("a2"));
        assert!(!table.carried_beside());
        table.release();
        assert!(table.turn().gestures.is_empty());
        assert_eq!(table.asked(), 0);
    }

    /// Fendo's fence: once the pawn has stood on b1, built on its north or east side.
    fn fence() -> Turn {
        let side = |id: &str| LegalSelection {
            notation: format!("b1{id}"),
            action: "build".into(),
            to: Some("b1".into()),
            gestures: vec![action(id)],
            ..Default::default()
        };
        let mut t = Turn::new(1);
        t.answered(answer(vec![side("north"), side("east")]));
        t
    }

    #[test]
    fn a_fence_taken_from_its_bowl_is_built_on_the_side_aimed_at_where_it_is_let_go() {
        let sides = vec![action("north"), action("east")];
        let mut table = Table::new(fence());
        let before = table.turn().clone();
        let bowl = table.source(TakeUp(sides.clone()));
        table.press_on(bowl);
        table.leave(Some("b1"));
        assert!(table.turn().taken().is_some(), "taken up");
        assert!(table.carried_beside());
        // No side aimed at: put back.
        table.release();
        assert_eq!(*table.turn(), before);

        table.press_on(bowl);
        table.leave(Some("b1"));
        table.app.world_mut().resource_mut::<SideAim>().choice = Some(action("east"));
        table.app.update();
        table.release();
        assert_eq!(table.turn().made_notations(), ["b1east"]);

        // Taken up by itself already (`Turn::auto_take`): pressed on, it is not put back.
        let mut table = Table::new(fence());
        table
            .app
            .world_mut()
            .resource_mut::<WatchStatus>()
            .turn
            .as_mut()
            .unwrap()
            .take_up(&sides);
        let bowl = table.source(TakeUp(sides.clone()));
        let held = table.source(());
        for pressed in [bowl, held] {
            table.press_on(pressed);
            table.leave(None);
            assert!(table.turn().taken().is_some(), "still in hand");
            assert!(table.carried_beside());
            table.release();
            assert!(table.turn().taken().is_some());
        }
    }

    #[test]
    fn a_tile_taken_from_the_stock_is_laid_where_it_is_let_go() {
        // Courtfall's draft: a hex, then the tile laid there.
        let lay = |at: &str, kind: &str| LegalSelection {
            notation: format!("{kind}@{at}"),
            action: "lay".into(),
            to: Some(at.into()),
            piece: Some(kind.into()),
            gestures: vec![Gesture::pos(at), piece(kind)],
            ..Default::default()
        };
        let mut t = Turn::new(1);
        t.answered(answer(vec![
            lay("a1", "lake"),
            lay("a2", "lake"),
            lay("a1", "hill"),
        ]));
        let mut table = Table::new(t);
        let before = table.turn().clone();
        let lakes = table.source(TakeToPlace(vec![piece("lake")]));
        table.press_on(lakes);
        table.leave(None);
        assert!(table.turn().taken_after_click(), "taken up");
        assert!(table.carried_beside());
        table.over(Some("a3"));
        table.release();
        assert_eq!(*table.turn(), before, "where none may go: put back");

        table.press_on(lakes);
        table.leave(Some("a2"));
        table.release();
        assert_eq!(table.turn().made_notations(), ["lake@a2"]);
    }

    /// A stack of two on a1 that may move up the column: one dropped on a2, both on a2, or
    /// one on a2 and one on a3.
    fn stack() -> Turn {
        let count = |n: &str| Gesture {
            kind: "count".into(),
            value: n.into(),
        };
        let mv = |n: &str, drops: &[&str]| {
            let mut gestures = vec![Gesture::pos("a1"), count(n)];
            gestures.extend(drops.iter().map(|d| Gesture::pos(d)));
            let mut path: Vec<String> = drops.iter().map(|d| d.to_string()).collect();
            path.dedup();
            LegalSelection {
                notation: format!("{n}a1-{}", drops.join("")),
                action: "move-stack".into(),
                from: Some("a1".into()),
                to: drops.last().map(|d| d.to_string()),
                path,
                gestures,
                ..Default::default()
            }
        };
        let mut t = Turn::new(1);
        t.answered(answer(vec![
            mv("1", &["a2"]),
            mv("2", &["a2", "a2"]),
            mv("2", &["a2", "a3"]),
        ]));
        t
    }

    #[test]
    fn a_stack_is_carried_whole_dropped_once_and_what_is_left_carried_on() {
        let mut table = Table::new(stack());
        table.press("a1");
        table.over(Some("b1"));
        let t = table.turn();
        assert_eq!(t.stack_at(), Some("a1"), "picked up whole");
        assert_eq!(t.stack_moves()[0].count, 2);
        assert_eq!(table.carried().as_deref(), Some("a1"));
        // The wheel, the button still down: one fewer carried, then both again.
        let wheel = |table: &mut Table, up: bool| {
            let turn = table.app.world_mut().resource_mut::<WatchStatus>();
            let mut turn = turn.map_unchanged(|w| w.turn.as_mut().unwrap());
            assert_eq!(turn.wheel(up, &Default::default()), Outcome::Changed);
            table.app.update();
        };
        wheel(&mut table, false);
        assert_eq!(table.turn().stack_moves()[0].count, 1);
        assert_eq!(table.carried().as_deref(), Some("a1"), "still carried");
        wheel(&mut table, true);
        table.over(Some("a2"));
        table.release();
        // One drop, as a click is: the rest hovers over a2.
        let t = table.turn();
        assert_eq!(t.stack_moves()[0].drops, ["a2"]);
        assert_eq!(t.stack_at(), Some("a2"));
        assert!(t.made.is_empty());
        let one_dropped = t.clone();

        // Pressed there and let go off the board: as it was.
        table.press("a2");
        table.over(Some("b1"));
        assert_eq!(table.carried().as_deref(), Some("a2"));
        table.over(None);
        table.release();
        assert_eq!(*table.turn(), one_dropped);

        // Carried on to the next space.
        table.press("a2");
        table.over(Some("a3"));
        table.release();
        assert_eq!(table.turn().made_notations(), ["2a1-a2a3"]);
        assert_eq!(table.asked(), 1);
    }
}
