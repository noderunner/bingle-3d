//! A card game's cards on the table, in place of its board: each pile where `patience.rs` lays
//! it for a patience, or where the game's appearance file says (`[cards]`), on a cloth, its cards
//! as the position has them (a card whose face this client may not see lies face down).
//!
//! A game laid by its look plays by the choices its look gives the piles: a click on a pile whose
//! choice is made together with others (`together`: a bowling rack's pins) chooses it, or puts
//! it back, and it hovers while chosen; the piles whose choice would then play exactly those
//! (the balls that knock them down) glow, and a click on one plays the move, its choices sent in
//! the order the server lists them. A click on the bare cloth, or Backspace, lets them all go.
//!
//! A click on a card a move starts with lifts it, and the cards on it: they hover over their
//! pile, bobbing gently, and the pointer stays a hand. Each pile they may go on glows, and the
//! one under the pointer shows them see-through where they would land. A click there puts them
//! down and plays the move at once (`Turn::clicks_only`); a click on the pile they came from, on
//! the bare cloth, a right-click or Backspace puts them back. A click on the stock draws, turns the waste over or deals,
//! whichever the game offers.
//!
//! They may be dragged too, as a piece on the board is (`drag.rs`): a press on a card, the
//! pointer leaving it with the button down, lifts what a click would and carries it under the
//! pointer, held near its edge (`hand::Held`); let go over a pile a click would put them on, it
//! is that click, and anywhere else they go back. A press and a release on the card is the
//! click. Every click on a patience's cards and places is such a press and release.
//!
//! A game of several seats is laid round the table by its look (`Layout::seated`), seen from
//! the seat this client plays: its own hand fanned in front of it, the others' standing in the
//! air, a trick in the middle with each card towards who played it (the zone's captions), and
//! a click on a card of the hand that may be played (they stand a little out of the fan, and
//! the one under the pointer further) makes its choice (`[cards] play`). Outside a headset the
//! player's own hand is drawn flat along the bottom of the window instead (`hand_strip.rs`, from
//! [`HandStrip`]): its cards lie unseen at the near edge of the table (`Layout::lay_strip`), where
//! a card dealt is carried to and joins the strip as it lands, and a card played sets off from.
//!
//! Cards go where the position puts them by the hand's own carrying (`hand::Carry::Put`): over
//! the table and down, one after another when several go at once (a row dealt, a run off the
//! table), and a card turned up on its way when it was face down where it came from.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use bevy::light::NotShadowCaster;
use bevy::picking::Pickable;
use bevy::picking::events::{PointerClick, PointerOut, PointerOver, PointerPress};
use bevy::picking::pointer::{PointerAction, PointerButton, PointerInput};
use bevy::prelude::*;

use crate::appearance::{Appearance, Appearances, CardsLook, DealingLook};
use crate::assets;
use crate::board::BoardBuilt;
use crate::card_layout::{self, Layout, Placed};
use crate::cursor::{Feel, Sense, TableFeel, Under};
use crate::deal::{self, Dealing, Dealt, Deck};
use crate::drag::{self, CardPress, Drag, LetGo};
use crate::escape_menu::{self, EscapeMenu};
use crate::hand::{BoardEdge, Carry, Hands, Held, Rest, Route, Surface};
use crate::hand_strip::{HandStrip, StripCard};
use crate::highlight::{Lit, Outline};
use crate::look::{self, MaterialSpec};
use crate::net::api::{CardOnTable, Cards, GameInfo, Gesture, LegalSelection, Position};
use crate::net::{Net, WatchStatus};
use crate::patience::{self, Click};
use crate::play::{self, Outcome, Stage, Turn};
use crate::props::{self, CARD, CARD_SIZE};
use crate::sounds::Cue;
use crate::table::Table;
use crate::vr_panels::InHeadset;
use crate::warm::{self, Warm};

pub struct CardTablePlugin;

impl Plugin for CardTablePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Aimed>()
            .init_resource::<PutOn>()
            .init_resource::<Together>()
            .init_resource::<Appearances>()
            .init_resource::<Drag>()
            .init_resource::<Pointed>()
            .init_resource::<HandStrip>()
            .init_resource::<InHeadset>()
            .init_resource::<Dealing>()
            .add_message::<PointerInput>()
            .add_observer(on_press)
            .add_message::<BoardBuilt>()
            .add_message::<Cue>()
            .add_observer(on_click)
            .add_observer(on_over)
            .add_observer(on_out)
            .init_resource::<Under>()
            .init_resource::<TableFeel>()
            .add_systems(Update, sense.in_set(Sense))
            .add_systems(Update, warm_cards.run_if(resource_changed::<Appearances>))
            .add_systems(
                Update,
                (
                    cut_deal,
                    let_go,
                    follow_cards,
                    hold_cards.run_if(resource_changed::<Drag>),
                    sync_cards.run_if(
                        resource_changed::<WatchStatus>
                            .or_eager(resource_changed::<Together>)
                            .or_eager(resource_changed::<InHeadset>),
                    ),
                    show_aim.run_if(
                        resource_changed::<WatchStatus>
                            .or_eager(resource_changed::<Aimed>)
                            .or_eager(resource_changed::<Together>),
                    ),
                    follow_hand,
                    stand_out,
                    fill_strip,
                )
                    .chain()
                    .before(Hands),
            )
            // After the hand's carrying, which a card being shuffled is not the hand's.
            .add_systems(Update, run_deal.after(Hands));
    }
}

/// The cloth the cards lie on: how thick, and how far it reaches beyond the piles.
pub const CLOTH: f32 = 0.002;
const CLOTH_MARGIN: f32 = 0.035;
/// A pile's place, printed on the cloth, a little larger than a card.
const PLACE: f32 = 0.0002;
const PLACE_MARGIN: f32 = 0.0015;
/// Where the cards lie: on the places.
const BASE: f32 = CLOTH + PLACE;
/// How high the cards in hand hover, and how far each lies beyond the one under it there.
const HELD_LIFT: f32 = 0.035;
const HELD_STEP: f32 = 0.021;
/// Where the hand holds a card: near its edge on the player's side, so it hovers beyond the
/// pointer and what it is aimed at is in sight.
const GRIP: f32 = CARD_SIZE.y / 2.0 - 0.008;
/// How long after one another the cards going somewhere at once set off, at the most in all.
const STAGGER: f32 = 0.045;
const STAGGER_MAX: f32 = 1.6;
/// How long after the last card of a deal lands it is over.
const END_AFTER: f32 = 0.05;
/// How much higher a card goes on its way than the hand carries a piece, so it clears the
/// piles it passes over (and turns over in the air).
const OVER: f32 = 0.02;
/// A card's corners, rounded as the model's are, for its glow.
const CORNER: f32 = 0.0032;
/// How high a card chosen with others hovers over its pile, and how much it bobs.
const CHOSEN_LIFT: f32 = 0.012;
const CHOSEN_BOB: f32 = 0.6;
/// How far a card of a hand that may be played stands out of the fan, towards its top, when
/// not every card may be; and how much further the one under the pointer.
const PLAYABLE_OUT: f32 = 0.009;
const POINTED_OUT: f32 = 0.014;
/// How much of a round table the camera frames at first, and where it looks, against the
/// cloth's width: left of the middle, so the table is clear of the panel, and towards the
/// player.
const SEATED_VIEW: f32 = 0.7;
const SEATED_LOOK: Vec2 = Vec2::new(-0.1, 0.1);
/// How many sides the outline of a round cloth has, for what is held to keep within.
const ROUND: usize = 24;

/// The place among the game's seats of the one the table is seen from: the seat this client
/// plays, else the first.
pub fn viewer(table: &Table) -> usize {
    table
        .yours()
        .and_then(|mine| table.info.seats.iter().position(|s| s.seat == mine.seat))
        .unwrap_or(0)
}

/// The seat a card's caption names, by the label the game gives it: the caption may say more
/// after it (a mark on the card that took a trick).
pub fn caption_seat(info: &GameInfo, caption: &str) -> Option<usize> {
    info.seats
        .iter()
        .filter(|s| !s.label.is_empty() && caption.starts_with(&s.label))
        // The longest label that fits: one seat's may begin another's.
        .max_by_key(|s| s.label.len())
        .and_then(|found| info.seats.iter().position(|s| s.seat == found.seat))
}

/// The choice a click on a card makes in a game whose look gives one (`play`), if it is one
/// of `choices`: the look's choice, then the card's kind.
pub fn card_choice(look: &CardsLook, card: &str, choices: &[Gesture]) -> Option<Gesture> {
    let kind = card.split('#').next().unwrap_or(card);
    let value = format!("{}{kind}", look.play.as_deref()?);
    choices
        .iter()
        .find(|g| g.kind == "action" && g.value == value)
        .cloned()
}

/// Whether a game's cards are laid on the table in place of its board: it has cards, and is a
/// patience or has a look that lays them out.
pub fn on_table(info: &GameInfo, look: Option<&Appearance>) -> bool {
    info.cards.is_some() && (patience::plays(info) || look.is_some_and(|l| l.cards.is_some()))
}

/// The choices made together so far (by their values), at the move they were made in: a move
/// played lets them go.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct Together {
    pub seq: u64,
    pub chosen: BTreeSet<String>,
}

impl Together {
    /// What is chosen at the move `seq`: nothing, if they were chosen at another.
    fn at(&self, seq: u64) -> BTreeSet<String> {
        if self.seq == seq {
            self.chosen.clone()
        } else {
            BTreeSet::new()
        }
    }
}

/// The gestures that play the move made of the choices `chosen` (made together, in any order)
/// and then `last`, in the order the server lists them; none when no move is that.
pub fn throw(
    options: &[LegalSelection],
    chosen: &BTreeSet<String>,
    last: &str,
) -> Option<Vec<Gesture>> {
    options
        .iter()
        .find(|o| {
            let Some((end, rest)) = o.gestures.split_last() else {
                return false;
            };
            let rest: BTreeSet<&str> = rest.iter().map(|g| g.value.as_str()).collect();
            end.value == last
                && rest.len() == o.gestures.len() - 1
                && rest == chosen.iter().map(String::as_str).collect()
        })
        .map(|o| o.gestures.clone())
}

/// Whether a choice made together is in any move: one that is not may not be chosen.
pub fn reachable(options: &[LegalSelection], value: &str) -> bool {
    options
        .iter()
        .any(|o| o.gestures.iter().any(|g| g.value == value))
}

/// What a click on a pile of a game laid by its look does: chooses a choice made together, or
/// lets it go; plays the move it ends; or nothing. The choices chosen after it.
pub fn look_click(
    look: &CardsLook,
    zone: &str,
    chosen: &BTreeSet<String>,
    options: &[LegalSelection],
) -> LookClick {
    let Some(value) = look.piles.get(zone).and_then(|p| p.click.as_deref()) else {
        return LookClick::Nothing;
    };
    let together = look
        .together
        .as_deref()
        .is_some_and(|t| value.starts_with(t));
    if together {
        let mut now = chosen.clone();
        if !now.remove(value) {
            if !reachable(options, value) {
                return LookClick::Nothing;
            }
            now.insert(value.to_string());
        }
        return LookClick::Choose(now);
    }
    match throw(options, chosen, value) {
        Some(gestures) => LookClick::Play(gestures),
        None => LookClick::Nothing,
    }
}

/// What a click on a pile of a game laid by its look does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookClick {
    /// What is chosen together is now this.
    Choose(BTreeSet<String>),
    /// Plays the move these gestures make.
    Play(Vec<Gesture>),
    Nothing,
}

/// The pile the pointer is over, if any: its cards, or its place.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct Aimed(pub Option<String>);

/// The card or pile's place the pointer is over, if any: what a press there leaves.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
struct Pointed(Option<Entity>);

/// The pile a click put the cards in hand on, while that move is sent: they are shown there.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
struct PutOn(Option<String>);

/// The cards of the game watched: where its piles lie, and the materials its cards are drawn in.
#[derive(Component)]
struct CardRoot {
    room_code: String,
    layout: Layout,
    looks: Looks,
    /// How the game's look lays it out, when it does (not a patience).
    by_look: Option<CardsLook>,
    /// The seat the table is seen from, its place among the game's, when it is laid round
    /// its seats.
    viewer: usize,
    /// Whether the player's own hand is drawn along the bottom of the window (not in a
    /// headset).
    strip: bool,
    /// The position last laid (its `move_seq`), and whether the game was waiting to start
    /// then: a start seen is dealt.
    seq: u64,
    waiting: bool,
}

/// A card: what the position calls it (its id, or its zone and place for one face down), where
/// it lies, and whether its face is shown.
#[derive(Component, Debug, Clone, PartialEq)]
struct CardAt {
    key: String,
    zone: String,
    /// 1 for the top card of its pile.
    from_top: usize,
    shown: bool,
    /// The client knows which card it is, whichever way up it lies: its key is its id.
    known: bool,
}

/// What the pointer over this is over: a pile, by its zone (on its cards and its place).
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct PileOf(String);

/// A pile's place on the cloth.
#[derive(Component)]
struct Place;

/// The cloth.
#[derive(Component)]
struct Cloth;

/// What the cards in hand would follow under the pointer, while it is `hand::Held`; a click
/// leaves them over their pile.
#[derive(Component)]
struct Anchor;

/// A card in hand: the how manyth from the lowest of those lifted.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct InHand(usize);

/// A card's parts, which a pointer picks.
#[derive(Component)]
struct Parts(Vec<Entity>);

/// What would land where the pointer is, see-through.
#[derive(Component)]
struct CardGhost;

/// A pile's glow, where what is in hand would land on it.
#[derive(Component)]
struct PileGlow(String);

/// The materials of a game's cards: each kind's face, and see-through, as each is wanted.
struct Looks {
    faces: HashMap<String, Handle<StandardMaterial>>,
    ghosts: HashMap<String, Handle<StandardMaterial>>,
    paper: Handle<StandardMaterial>,
    back: Handle<StandardMaterial>,
    edge: Handle<StandardMaterial>,
}

impl Looks {
    fn new(server: &AssetServer, materials: &mut Assets<StandardMaterial>) -> Self {
        Looks {
            faces: HashMap::new(),
            ghosts: HashMap::new(),
            paper: materials.add(look::paper().build(server)),
            back: materials.add(look::card_side(look::CARD_BACK).build(server)),
            edge: materials.add(look::paper().build(server)),
        }
    }

    /// The picture on the face of the card `id`: the usual deck's, by its suit's symbol and its
    /// rank, or plain paper for a card the client has no picture of.
    fn face(
        &mut self,
        cards: Option<&Cards>,
        id: &str,
        server: &AssetServer,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let Some(file) = face_file(cards, id) else {
            return self.paper.clone();
        };
        self.faces
            .entry(file.clone())
            .or_insert_with(|| materials.add(look::card_side(&file).build(server)))
            .clone()
    }

    /// The same, see-through, and the back and edge to go with it.
    fn ghost(
        &mut self,
        cards: Option<&Cards>,
        id: &str,
        server: &AssetServer,
        materials: &mut Assets<StandardMaterial>,
    ) -> [Handle<StandardMaterial>; 3] {
        let face = self.face(cards, id, server, materials);
        let mut see_through = |key: String, of: &Handle<StandardMaterial>| {
            self.ghosts
                .entry(key)
                .or_insert_with(|| {
                    let mut m = materials.get(of).cloned().unwrap_or_default();
                    m.base_color = m.base_color.with_alpha(0.5);
                    m.alpha_mode = AlphaMode::Blend;
                    materials.add(m)
                })
                .clone()
        };
        let key = face_file(cards, id).unwrap_or_default();
        let (back, edge) = (self.back.clone(), self.edge.clone());
        [
            see_through(key, &face),
            see_through("back".into(), &back),
            see_through("edge".into(), &edge),
        ]
    }
}

/// The picture for the face of card `id`, if the client has one.
fn face_file(cards: Option<&Cards>, id: &str) -> Option<String> {
    let cards = cards?;
    let kind = cards.type_of(id)?;
    let symbol = &cards.suits.get(&kind.suit)?.symbol;
    let rank = if kind.rank.is_empty() {
        &kind.label
    } else {
        &kind.rank
    };
    props::standard_face(symbol, rank)
}

/// A card's corners round its middle (x, z), wound as a cell's are, for a glow to trace.
fn card_corners() -> Vec<Vec2> {
    let half = CARD_SIZE.xy() / 2.0 - Vec2::splat(CORNER);
    // From the near left corner's arc round, the way a cell's corners go.
    let centres = [
        Vec2::new(-half.x, -half.y),
        Vec2::new(-half.x, half.y),
        Vec2::new(half.x, half.y),
        Vec2::new(half.x, -half.y),
    ];
    let starts = [1.5, 1.0, 0.5, 0.0].map(|t: f32| t * std::f32::consts::PI);
    let mut out = Vec::new();
    for (centre, start) in centres.into_iter().zip(starts) {
        for i in 0..=3 {
            let a = start - i as f32 / 3.0 * std::f32::consts::FRAC_PI_2;
            out.push(centre + Vec2::from_angle(a) * CORNER);
        }
    }
    out
}

/// Each pile's cards, bottom first, by zone: the position's, for the piles the layout has. A
/// pile its look shows only while another is empty has none while that one has cards, and a
/// card it does show is left out of where else the position has it (a trick taken stays where
/// it was played until the next is led).
fn piles(
    position: &Position,
    layout: &Layout,
    look: Option<&CardsLook>,
) -> BTreeMap<String, Vec<CardOnTable>> {
    let mut all = position.card_zones();
    all.retain(|zone, _| layout.piles.contains_key(zone));
    let Some(look) = look else {
        return all;
    };
    let waits = |zone: &str| look.pile(zone).and_then(|p| p.unless.clone());
    let hidden: Vec<String> = all
        .keys()
        .filter(|zone| waits(zone).is_some_and(|o| all.get(&o).is_some_and(|c| !c.is_empty())))
        .cloned()
        .collect();
    for zone in hidden {
        all.insert(zone, Vec::new());
    }
    let kept: HashSet<String> = all
        .iter()
        .filter(|(zone, _)| waits(zone).is_some())
        .flat_map(|(_, cards)| cards.iter().map(|c| c.id.clone()))
        .filter(|id| !id.is_empty())
        .collect();
    if !kept.is_empty() {
        for (zone, cards) in all.iter_mut() {
            if waits(zone).is_none() {
                cards.retain(|c| !kept.contains(&c.id));
            }
        }
    }
    all
}

/// How the side of the seat that played each card of `zone` is turned, as far as the view's
/// captions say who did, for a table seen from `viewer`.
fn towards(
    table: &Table,
    captions: &BTreeMap<String, Vec<String>>,
    viewer: usize,
    zone: &str,
) -> Vec<Option<f32>> {
    let seats = table.info.seats.len();
    captions
        .get(zone)
        .map(|all| {
            all.iter()
                .map(|caption| {
                    let i = caption_seat(&table.info, caption)?;
                    Some(card_layout::seat_turn(
                        (i + seats - viewer % seats) % seats,
                        seats,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Which way up each card of a pile lies.
fn ups(cards: &[CardOnTable]) -> Vec<bool> {
    cards.iter().map(|c| c.up).collect()
}

/// What a card is called in the scene: its id, or for one the client may not know, its zone
/// and place (which changes when it is turned up, so it is drawn again with its face).
fn key(zone: &str, index: usize, card: &CardOnTable) -> String {
    if !card.id.is_empty() {
        card.id.clone()
    } else {
        format!("{zone}#{index}")
    }
}

/// What the move being built has lifted, while it is being entered.
fn in_hand(turn: Option<&Turn>) -> Option<(String, usize)> {
    let turn = turn.filter(|t| t.stage == Stage::Choosing)?;
    patience::lifted(&turn.gestures)
}

/// What the move made puts down, while it is sent: lifted from where, how many.
fn put_down(turn: Option<&Turn>) -> Option<(String, usize)> {
    let turn = turn.filter(|t| matches!(t.stage, Stage::Asking | Stage::Ready | Stage::Sending))?;
    let made = turn.made.last()?;
    patience::lifted(made.gestures.get(..1)?)
}

/// The foundation what is in hand (a zone, how many) goes home to, when the view says which
/// suit each is built in: its lowest card's suit's.
fn home_of(table: &Table, (zone, count): &(String, usize)) -> Option<String> {
    let zones = table.state.state.card_zones();
    let pile = zones.get(zone)?;
    let lowest = pile.get(pile.len().checked_sub(*count)?)?;
    let suit = &table.info.cards.as_ref()?.type_of(&lowest.id)?.suit;
    patience::home(suit, &table.state.state.view_words("suits"))
}

/// A pile's cards that are lifted: the top `count`, by their places in it.
fn lifted_from(len: usize, count: usize) -> std::ops::Range<usize> {
    len.saturating_sub(count)..len
}

type Existing<'a> = (
    Entity,
    &'a CardAt,
    &'a Transform,
    &'a Rest,
    Option<&'a Carry>,
    Option<&'a InHand>,
    &'a Parts,
    Has<Dealt>,
);

/// A card set off for where the position puts it: from which pile, to which, and its place
/// there.
struct Going {
    card: Entity,
    from: String,
    to: String,
    index: usize,
}

/// Of the cards set off, those one pile deals: [`deal::DEAL_LEAST`] or more from one pile to
/// two others or more. The pile they are dealt from.
fn dealt_from(going: &[Going]) -> Option<String> {
    let mut by: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for g in going.iter().filter(|g| g.from != g.to) {
        by.entry(g.from.as_str()).or_default().insert(g.to.as_str());
    }
    by.into_iter()
        .filter(|(_, to)| to.len() >= 2)
        .map(|(from, _)| from)
        .find(|from| {
            going
                .iter()
                .filter(|g| g.from == *from && g.to != *from)
                .count()
                >= deal::DEAL_LEAST
        })
        .map(str::to_string)
}

/// The deck's place: the pile its look names, else the stock, else the first pile the
/// position leaves empty that lies squared or played to (a free cell, the trick), else the
/// table's middle. The zone, when it is a pile.
fn deck_of(
    layout: &Layout,
    piles: &BTreeMap<String, Vec<CardOnTable>>,
    named: Option<&str>,
) -> (Deck, Option<String>) {
    let empty = |zone: &str| piles.get(zone).is_none_or(|c| c.is_empty());
    let found = named
        .filter(|z| layout.piles.contains_key(*z))
        .or_else(|| layout.piles.contains_key("stock").then_some("stock"))
        .or_else(|| {
            layout
                .piles
                .iter()
                .find(|(zone, p)| {
                    empty(zone)
                        && matches!(
                            p.spread,
                            card_layout::Spread::Squared | card_layout::Spread::Played
                        )
                })
                .map(|(zone, _)| zone.as_str())
        });
    match found.and_then(|z| Some((z, layout.piles.get(z)?))) {
        Some((zone, pile)) => (
            Deck {
                at: pile.at,
                turn: pile.turn,
            },
            Some(zone.to_string()),
        ),
        None => (Deck::default(), None),
    }
}

/// How a card of a deal goes: after its turn, over the piles, silently.
fn deal_route(delay: f32) -> Route {
    Route {
        delay,
        over: OVER,
        lands: None,
        ..Route::default()
    }
}

/// Puts warm cards (`warm.rs`) in the scene from the first frame a game has cards, if none are
/// yet: one of each pipeline a card is drawn with (a face with a picture or plain paper, the back and the edge,
/// and each see-through).
fn warm_cards(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    appearances: Res<Appearances>,
    warm: Query<&Warm>,
) {
    if warm::has(&warm, Warm::Cards) || !appearances.0.iter().any(|a| a.cards.is_some()) {
        return;
    }
    let mut looks = Looks::new(&server, &mut materials);
    let picture = props::standard_face("♠", "A")
        .map(|file| materials.add(look::card_side(&file).build(&server)));
    let [ghost_face, ghost_back, ghost_edge] = looks.ghost(None, "", &server, &mut materials);
    let (back, edge, paper) = (looks.back.clone(), looks.edge.clone(), looks.paper.clone());
    let mut sets = vec![
        [("face", paper), ("back", back), ("edge", edge)],
        [
            ("face", ghost_face),
            ("back", ghost_back),
            ("edge", ghost_edge),
        ],
    ];
    if let Some(picture) = picture {
        sets.push([
            ("face", picture),
            ("back", looks.back.clone()),
            ("edge", looks.edge.clone()),
        ]);
    }
    for set in sets {
        let whole = warm::whole(&mut commands, Warm::Cards);
        parts(&mut commands, &server, &set, whole, Pickable::IGNORE);
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_cards(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    server: Res<AssetServer>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut roots: Query<(Entity, &mut CardRoot)>,
    cards: Query<Existing>,
    (put_on, together, appearances): (Res<PutOn>, Res<Together>, Res<Appearances>),
    (mut built, mut cues): (MessageWriter<BoardBuilt>, MessageWriter<Cue>),
    in_headset: Res<InHeadset>,
    (time, mut dealing): (Res<Time>, ResMut<Dealing>),
) {
    let look = |t: &Table| appearances.for_game(&t.info.id);
    let Some(table) = watch.table.as_ref().filter(|t| on_table(&t.info, look(t))) else {
        for (root, _) in &roots {
            commands.entity(root).despawn();
        }
        return;
    };
    let by_look = look(table).and_then(|l| l.cards.as_ref());
    let seen_from = by_look.filter(|l| l.seated()).map_or(0, |_| viewer(table));
    let strip = !in_headset.0;
    let dealing_look = look(table).map(|l| l.dealing.clone()).unwrap_or_default();
    // A game that was waiting to start is laid again once it has: its start is dealt.
    let started = |r: &CardRoot| r.waiting && table.state.status == "active";
    let current = roots.iter_mut().find(|(_, r)| {
        r.room_code == table.room_code && r.viewer == seen_from && r.strip == strip && !started(r)
    });
    let Some((root, mut shown)) = current else {
        for (old, _) in &roots {
            commands.entity(old).despawn();
        }
        let fresh = build(
            &mut commands,
            table,
            look(table).and_then(|l| l.cards.clone()),
            strip,
            &server,
            &mut meshes,
            &mut materials,
            &mut built,
        );
        let now = time.elapsed_secs();
        let deals = (&dealing_look, &mut *dealing, now);
        return place_fresh(&mut commands, table, fresh, &server, &mut materials, deals);
    };
    let root_data = &mut *shown;
    let layout = root_data.layout.clone();
    let info_cards = table.info.cards.as_ref();
    let position = watch.shown().unwrap_or(&table.state.state);
    let piles = piles(position, &layout, root_data.by_look.as_ref());
    let captions = position.card_captions();
    let turn = watch.turn.as_ref();
    let held = in_hand(turn);
    let put = put_down(turn).zip(put_on.0.clone());
    // The piles whose choice is chosen together with others: their top card hovers.
    let chosen = together.at(table.state.move_seq);
    let lifted_piles: HashSet<&str> = root_data
        .by_look
        .iter()
        .flat_map(|l| &l.piles)
        .filter(|(_, p)| p.click.as_ref().is_some_and(|c| chosen.contains(c)))
        .map(|(zone, _)| zone.as_str())
        .collect();

    let existing: HashMap<&str, _> = cards.iter().map(|c| (c.1.key.as_str(), c)).collect();
    let mut wanted: HashSet<String> = HashSet::new();
    let mut order: Vec<(&str, &[CardOnTable])> = piles
        .iter()
        .map(|(z, c)| (z.as_str(), c.as_slice()))
        .collect();
    // Dealt in the piles' order: the stock first, the columns from the left.
    order.sort_by_key(|(zone, _)| patience::kind(zone));
    for (zone, pile) in &order {
        for (i, c) in pile.iter().enumerate() {
            wanted.insert(key(zone, i, c));
        }
    }

    // What is no longer there: where it was is where what replaces it comes from (a card turned
    // up in its place, a card drawn from the stock).
    let mut gone_at: HashMap<(String, usize), Transform> = HashMap::new();
    let mut gone: Vec<(usize, Transform, String)> = Vec::new();
    for (entity, at, t, ..) in existing.values() {
        if wanted.contains(&at.key) {
            continue;
        }
        let index = piles
            .get(&at.zone)
            .map_or(0, |p| p.len())
            .saturating_add(at.from_top)
            .saturating_sub(1);
        gone_at.insert((at.zone.clone(), index), **t);
        gone.push((usize::MAX - index, **t, at.zone.clone()));
        commands.entity(*entity).despawn();
    }
    gone.sort_by_key(|(order, ..)| *order);
    let mut gone = gone.into_iter().map(|(_, t, zone)| (t, zone));

    // The cards set off, in the order they set off unless one pile deals them.
    let mut going: Vec<Going> = Vec::new();
    let mut lifting = false;
    for (zone, pile) in &order {
        let up = ups(pile);
        let rests =
            layout.lie_towards(zone, &up, &towards(table, &captions, seen_from, zone), BASE);
        let lifted = held
            .as_ref()
            .filter(|(z, _)| z == zone)
            .map(|(_, n)| lifted_from(pile.len(), *n));
        let putting = put
            .as_ref()
            .filter(|((z, _), _)| z == zone)
            .map(|((_, n), onto)| {
                let onto_up = piles.get(onto).map(|p| ups(p)).unwrap_or_default();
                (
                    lifted_from(pile.len(), *n),
                    layout.landing(onto, &onto_up, *n, BASE),
                )
            });
        for (i, card) in pile.iter().enumerate() {
            let k = key(zone, i, card);
            let rest = rests[i];
            let at = CardAt {
                key: k.clone(),
                zone: zone.to_string(),
                from_top: pile.len() - i,
                shown: card.shown(),
                known: !card.id.is_empty(),
            };
            let in_hand = lifted
                .as_ref()
                .filter(|r| r.contains(&i))
                .map(|r| i - r.start);
            let put_at = putting
                .as_ref()
                .filter(|(r, _)| r.contains(&i))
                .and_then(|(r, landing)| landing.get(i - r.start).copied());
            match existing.get(k.as_str()) {
                // Still on its way in a deal of this position: it goes on.
                Some((.., true)) if dealing.running => {}
                Some((entity, was, t, old_rest, carry, hand, parts, _)) => {
                    let mut e = commands.entity(*entity);
                    if *was != &at {
                        e.insert((at, PileOf(zone.to_string())));
                    }
                    if old_rest.0 != rest {
                        e.insert(Rest(rest));
                    }
                    if let Some(j) = in_hand {
                        if hand.is_none() {
                            lifting = true;
                            e.insert((
                                InHand(j),
                                Carry::Over {
                                    at: **t,
                                    scale: 0.0,
                                },
                            ));
                            pickable(&mut commands, parts, false);
                        }
                        continue;
                    }
                    if hand.is_some() {
                        e.remove::<InHand>();
                        pickable(&mut commands, parts, true);
                    }
                    // Chosen with others: it hovers over its pile until the move is played.
                    let hovering = matches!(carry, Some(Carry::Held { .. }));
                    if i + 1 == pile.len() && lifted_piles.contains(zone) {
                        if !hovering {
                            lifting = true;
                            commands.entity(*entity).insert(Carry::Held {
                                lift: CHOSEN_LIFT,
                                scale: CHOSEN_BOB,
                            });
                        }
                        continue;
                    }
                    let goal = put_at.unwrap_or(rest);
                    // Let go where it lies: it settles back by itself.
                    if hovering && goal == rest && !moved(old_rest.0, rest) {
                        commands.entity(*entity).remove::<Carry>();
                        continue;
                    }
                    let on_way = matches!(carry, Some(Carry::Put(to)) if *to == goal);
                    if on_way {
                        continue;
                    }
                    let near = t.translation.distance(goal.translation) < 1e-4
                        && t.rotation.angle_between(goal.rotation) < 1e-3;
                    if near && hand.is_none() {
                        if carry.is_some() {
                            commands.entity(*entity).remove::<Carry>();
                        }
                        continue;
                    }
                    commands.entity(*entity).insert(Carry::Put(goal));
                    going.push(Going {
                        card: *entity,
                        from: was.zone.clone(),
                        to: zone.to_string(),
                        index: i,
                    });
                }
                None => {
                    // From where a card went, turned as it was there; else where it lies.
                    let from = gone_at
                        .remove(&(zone.to_string(), i))
                        .map(|t| (t, zone.to_string()))
                        .or_else(|| gone.next());
                    let whole = spawn_card(
                        &mut commands,
                        root,
                        &server,
                        &mut materials,
                        &mut root_data.looks,
                        info_cards,
                        card,
                        from.as_ref().map_or(rest, |(t, _)| *t),
                    );
                    commands
                        .entity(whole)
                        .insert((at, PileOf(zone.to_string()), Rest(rest)));
                    if let Some((_, was)) = from {
                        commands.entity(whole).insert(Carry::Put(rest));
                        going.push(Going {
                            card: whole,
                            from: was,
                            to: zone.to_string(),
                            index: i,
                        });
                    }
                }
            }
        }
    }
    if lifting {
        cues.write(Cue::Lift);
    }

    // One move on, several cards from one pile to several others: dealt, round the piles.
    let next = table.state.move_seq == root_data.seq + 1;
    let deals = next && dealing_look.deal;
    let from = deals.then(|| dealt_from(&going)).flatten();
    let seats: Vec<u32> = table.info.seats.iter().map(|s| s.seat).collect();
    let dealt: Vec<&Going> = going
        .iter()
        .filter(|g| {
            from.as_ref()
                .is_some_and(|f| *f == g.from && g.to != g.from)
        })
        .collect();
    let first: HashMap<&str, usize> = dealt.iter().fold(HashMap::new(), |mut m, g| {
        let low = m.entry(g.to.as_str()).or_insert(g.index);
        *low = (*low).min(g.index);
        m
    });
    let nth = deal::order(
        &dealt
            .iter()
            .map(|g| (deal::rank(&g.to, &seats), g.index - first[g.to.as_str()]))
            .collect::<Vec<_>>(),
    );
    let step = deal::step(dealt.len(), deal_route(0.0).takes());
    for (g, k) in dealt.iter().zip(&nth) {
        commands.entity(g.card).insert((
            deal_route(*k as f32 * step),
            Dealt {
                slot: 0,
                nth: Some(*k),
            },
        ));
    }
    if !dealt.is_empty() {
        *dealing = Dealing {
            room: table.room_code.clone(),
            seq: table.state.move_seq,
            began: time.elapsed_secs(),
            step,
            ends: (dealt.len() - 1) as f32 * step + deal_route(0.0).takes() + END_AFTER,
            running: true,
            cards: dealt.len(),
            ..default()
        };
    }
    let others = going
        .iter()
        .filter(|g| !dealt.iter().any(|d| d.card == g.card));
    for (setting_off, g) in others.enumerate() {
        let delay = (setting_off as f32 * STAGGER).min(STAGGER_MAX);
        commands.entity(g.card).insert(Route {
            delay,
            over: OVER,
            ..Route::default()
        });
    }
    root_data.seq = table.state.move_seq;
}

/// Lays the table for a game just opened: the cloth, each pile's place and glow, and the cards
/// as they lie; or, for a game seen at its start (`deals`), the cards in the deck, to be
/// shuffled and dealt where they lie (`run_deal`).
fn place_fresh(
    commands: &mut Commands,
    table: &Table,
    (root, mut data): (Entity, CardRoot),
    server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    (look, dealing, now): (&DealingLook, &mut Dealing, f32),
) {
    let layout = data.layout.clone();
    let captions = table.state.state.card_captions();
    let piles = piles(&table.state.state, &layout, data.by_look.as_ref());
    let mut laid: Vec<(Entity, &str, usize, Transform)> = Vec::new();
    for (zone, pile) in &piles {
        let rests = layout.lie_towards(
            zone,
            &ups(pile),
            &towards(table, &captions, data.viewer, zone),
            BASE,
        );
        for (i, card) in pile.iter().enumerate() {
            let whole = spawn_card(
                commands,
                root,
                server,
                materials,
                &mut data.looks,
                table.info.cards.as_ref(),
                card,
                rests[i],
            );
            commands.entity(whole).insert((
                CardAt {
                    key: key(zone, i, card),
                    zone: zone.clone(),
                    from_top: pile.len() - i,
                    shown: card.shown(),
                    known: !card.id.is_empty(),
                },
                PileOf(zone.clone()),
                Rest(rests[i]),
            ));
            laid.push((whole, zone, i, rests[i]));
        }
    }
    let start = table.state.status == "active" && table.state.move_seq == 0;
    if start && look.deal && !laid.is_empty() && !dealing.shown_start(&table.room_code) {
        deal_start(commands, table, &layout, &piles, &laid, look, dealing, now);
    }
    commands.entity(root).insert(data);
}

/// The cards just laid (each its entity, pile, place in it and rest) gathered into the deck,
/// to be shuffled and dealt round the piles; those of the deck's own pile stay in it, at its
/// bottom.
#[allow(clippy::too_many_arguments)]
fn deal_start(
    commands: &mut Commands,
    table: &Table,
    layout: &Layout,
    piles: &BTreeMap<String, Vec<CardOnTable>>,
    laid: &[(Entity, &str, usize, Transform)],
    look: &DealingLook,
    dealing: &mut Dealing,
    now: f32,
) {
    let (deck, deck_zone) = deck_of(layout, piles, look.deck.as_deref());
    let stays = |zone: &str| deck_zone.as_deref() == Some(zone);
    let seats: Vec<u32> = table.info.seats.iter().map(|s| s.seat).collect();
    let dealt: Vec<_> = laid.iter().filter(|(_, z, ..)| !stays(z)).collect();
    let nth = deal::order(
        &dealt
            .iter()
            .map(|(_, zone, i, _)| (deal::rank(zone, &seats), *i))
            .collect::<Vec<_>>(),
    );
    let n = laid.len();
    let shuffle = if look.shuffle { deal::SHUFFLE } else { 0.0 };
    let start = |slot: usize| {
        if look.shuffle {
            deck.riffle(slot, n, 0.0, BASE)
        } else {
            deck.slot(slot, BASE)
        }
    };
    // The deck's own cards at its bottom, as they lie; then the rest, the first dealt on top.
    let mut bottom = 0;
    for (entity, zone, _, _) in laid {
        if stays(zone) {
            let slot = bottom;
            bottom += 1;
            commands
                .entity(*entity)
                .insert((Dealt { slot, nth: None }, start(slot)));
        }
    }
    for ((entity, ..), k) in dealt.iter().zip(&nth) {
        let slot = n - 1 - k;
        commands.entity(*entity).insert((
            Dealt {
                slot,
                nth: Some(*k),
            },
            start(slot),
        ));
    }
    let step = deal::step(dealt.len(), deal_route(0.0).takes());
    *dealing = Dealing {
        room: table.room_code.clone(),
        seq: table.state.move_seq,
        deck,
        began: now,
        shuffle,
        step,
        ends: shuffle
            + dealt.len().saturating_sub(1) as f32 * step
            + deal_route(0.0).takes()
            + END_AFTER,
        running: true,
        swallow: false,
        cards: n,
    };
}

/// Moves the cards of a shuffle under way, and sets each off on its turn when it is over; once
/// every card has arrived, the deal is over.
fn run_deal(
    mut commands: Commands,
    time: Res<Time>,
    mut dealing: ResMut<Dealing>,
    mut cards: Query<(Entity, &Dealt, &Rest, &mut Transform, Has<Carry>)>,
) {
    if !dealing.running {
        return;
    }
    let secs = time.elapsed_secs() - dealing.began;
    if secs >= dealing.ends {
        dealing.running = false;
        for (entity, ..) in &cards {
            commands.entity(entity).remove::<Dealt>();
        }
        return;
    }
    for (entity, dealt, rest, mut t, carried) in &mut cards {
        if secs < dealing.shuffle {
            *t = dealing.deck.riffle(dealt.slot, dealing.cards, secs, BASE);
            continue;
        }
        if carried {
            continue;
        }
        match dealt.nth {
            Some(k) => {
                *t = dealing.deck.slot(dealt.slot, BASE);
                let due = dealing.shuffle + k as f32 * dealing.step - secs;
                commands
                    .entity(entity)
                    .insert((Carry::Put(rest.0), deal_route(due.max(0.0))));
            }
            // Staying in the deck: it settles where it lies there.
            None => {
                commands.entity(entity).remove::<Dealt>();
            }
        }
    }
}

/// A shuffle or a deal under way ends at once, every card where it rests, at a press of the
/// primary button (which does nothing else: its click is swallowed) or when the position is
/// another.
fn cut_deal(
    mut commands: Commands,
    mut dealing: ResMut<Dealing>,
    watch: Res<WatchStatus>,
    mut inputs: MessageReader<PointerInput>,
    mut cards: Query<(Entity, &Rest, &mut Transform), With<Dealt>>,
) {
    let mut cut = false;
    for input in inputs.read() {
        match input.action {
            PointerAction::Press(PointerButton::Primary) if dealing.running => {
                cut = true;
                dealing.swallow = true;
            }
            PointerAction::Release(PointerButton::Primary) if dealing.swallow => {
                dealing.swallow = false;
            }
            _ => {}
        }
    }
    let other = watch
        .table
        .as_ref()
        .is_none_or(|t| t.room_code != dealing.room || t.state.move_seq != dealing.seq);
    if !dealing.running || !(cut || other) {
        return;
    }
    dealing.running = false;
    for (entity, rest, mut t) in &mut cards {
        *t = rest.0;
        commands.entity(entity).remove::<(Dealt, Carry)>();
    }
}

/// The cloth, the piles' places and glows, and what the cards in hand follow, for the camera to
/// frame.
#[allow(clippy::too_many_arguments)]
fn build(
    commands: &mut Commands,
    table: &Table,
    by_look: Option<CardsLook>,
    strip: bool,
    server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    built: &mut MessageWriter<BoardBuilt>,
) -> (Entity, CardRoot) {
    let seen_from = by_look
        .as_ref()
        .filter(|l| l.seated())
        .map_or(0, |_| viewer(table));
    let layout = match &by_look {
        Some(look) if look.seated() => {
            let placed: Vec<Placed> = look
                .piles
                .iter()
                .map(|(zone, p)| Placed {
                    zone: zone.clone(),
                    at: Vec2::from_array(p.seat.unwrap_or(p.at)),
                    spread: p.spread,
                    each_seat: p.seat.is_some(),
                })
                .collect();
            let seats: Vec<u32> = table.info.seats.iter().map(|s| s.seat).collect();
            let mut layout = Layout::seated(&placed, &seats, seen_from, table.yours().is_some());
            if let Some(radius) = look.radius {
                layout.size = layout.size.max(Vec2::splat(2.0 * radius));
            }
            if strip {
                layout.lay_strip();
            }
            layout
        }
        Some(look) => Layout::placed(
            look.piles
                .iter()
                .map(|(zone, p)| (zone.clone(), Vec2::new(p.at[0], p.at[1]), p.spread)),
        ),
        None => {
            let zones = table.state.state.card_zones();
            patience::layout(zones.iter().map(|(zone, z)| (zone.as_str(), z.len())))
        }
    };
    let root = commands
        .spawn((
            Name::new("Cards"),
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    let cloth_size = layout.size + Vec2::splat(2.0 * CLOTH_MARGIN);
    let cloth_mesh = if layout.round {
        meshes.add(
            Cylinder::new(cloth_size.x / 2.0, CLOTH)
                .mesh()
                .resolution(96),
        )
    } else {
        meshes.add(Cuboid::new(cloth_size.x, CLOTH, cloth_size.y))
    };
    let cloth = materials.add(
        MaterialSpec {
            tint: Some([0.045, 0.11, 0.08]),
            roughness: Some(0.95),
            ..default()
        }
        .build(server),
    );
    commands.spawn((
        Name::new("Cloth"),
        Cloth,
        Surface,
        Pickable::default(),
        Mesh3d(cloth_mesh),
        MeshMaterial3d(cloth),
        Transform::from_xyz(0.0, CLOTH / 2.0, 0.0),
        ChildOf(root),
    ));
    let place = materials.add(
        MaterialSpec {
            tint: Some([0.075, 0.17, 0.125]),
            roughness: Some(0.95),
            ..default()
        }
        .build(server),
    );
    let place_mesh = meshes.add(Cuboid::new(
        CARD_SIZE.x + 2.0 * PLACE_MARGIN,
        PLACE,
        CARD_SIZE.y + 2.0 * PLACE_MARGIN,
    ));
    for (zone, pile) in layout.piles.iter().filter(|(_, p)| p.place) {
        commands.spawn((
            Name::new(format!("Place {zone}")),
            Place,
            PileOf(zone.clone()),
            Surface,
            Pickable::default(),
            Mesh3d(place_mesh.clone()),
            MeshMaterial3d(place.clone()),
            Transform::from_xyz(pile.at.x, CLOTH + PLACE / 2.0, pile.at.y),
            ChildOf(root),
        ));
        commands.spawn((
            Name::new(format!("Glow {zone}")),
            PileGlow(zone.clone()),
            Outline::space(card_corners(), 0.0),
            Transform::from_xyz(pile.at.x, BASE, pile.at.y),
            Visibility::default(),
            ChildOf(root),
        ));
    }
    commands.spawn((
        Name::new("Hand"),
        Anchor,
        Transform::from_xyz(0.0, BASE + HELD_LIFT, 0.0),
        Visibility::Hidden,
        ChildOf(root),
    ));
    let half = cloth_size / 2.0;
    let edge = if layout.round {
        (0..ROUND)
            .map(|i| Vec2::from_angle(std::f32::consts::TAU * i as f32 / ROUND as f32) * half.x)
            .collect()
    } else {
        vec![
            Vec2::new(-half.x, -half.y),
            Vec2::new(-half.x, half.y),
            Vec2::new(half.x, half.y),
            Vec2::new(half.x, -half.y),
        ]
    };
    commands.insert_resource(BoardEdge(edge));
    // Framed as dealt, with room for the columns to grow by half their reach: a long one
    // reaches beyond the picture rather than every deal being seen from afar.
    let unused = layout.room / 2.0;
    built.write(if layout.round {
        // Seen from the player's own seat: nearer their hand than the far side, and clear of
        // the game's panel at the left.
        BoardBuilt {
            size: cloth_size * SEATED_VIEW,
            middle: cloth_size * SEATED_LOOK,
        }
    } else {
        BoardBuilt {
            size: cloth_size - Vec2::Y * unused,
            middle: Vec2::Y * -unused / 2.0,
        }
    });
    let looks = Looks::new(server, materials);
    (
        root,
        CardRoot {
            room_code: table.room_code.clone(),
            layout,
            looks,
            by_look,
            viewer: seen_from,
            strip,
            seq: table.state.move_seq,
            waiting: table.state.status == "waiting",
        },
    )
}

/// A card as it lies at `at`, its parts picked by a pointer.
#[allow(clippy::too_many_arguments)]
fn spawn_card(
    commands: &mut Commands,
    root: Entity,
    server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    looks: &mut Looks,
    cards: Option<&Cards>,
    card: &CardOnTable,
    at: Transform,
) -> Entity {
    // A card the client may not know shows its back from both sides: a hand standing in the
    // air is seen from its seat's side too.
    let face = if !card.id.is_empty() {
        looks.face(cards, &card.id, server, materials)
    } else {
        looks.back.clone()
    };
    let by_part = [
        ("face", face),
        ("back", looks.back.clone()),
        ("edge", looks.edge.clone()),
    ];
    let whole = commands
        .spawn((
            Name::new("Card"),
            Surface,
            at,
            Visibility::default(),
            ChildOf(root),
        ))
        .id();
    let parts = parts(commands, server, &by_part, whole, Pickable::default());
    commands.entity(whole).insert(Parts(parts));
    whole
}

/// A card's parts under `whole`, each in the material given for its name.
fn parts(
    commands: &mut Commands,
    server: &AssetServer,
    by_part: &[(&str, Handle<StandardMaterial>)],
    whole: Entity,
    pick: Pickable,
) -> Vec<Entity> {
    assets::model_parts(CARD)
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            let (_, material) = by_part.iter().find(|(part, _)| part == name)?;
            Some(
                commands
                    .spawn((
                        Mesh3d(props::part(server, CARD, index)),
                        MeshMaterial3d(material.clone()),
                        pick,
                        ChildOf(whole),
                    ))
                    .id(),
            )
        })
        .collect()
}

/// Whether a card's parts are picked: not while it is in hand, so the pointer finds what is
/// under it.
fn pickable(commands: &mut Commands, parts: &Parts, on: bool) {
    for part in &parts.0 {
        let pick = if on {
            Pickable::default()
        } else {
            Pickable::IGNORE
        };
        commands.entity(*part).insert(pick);
    }
}

/// The piles what is in hand may go on glow, and on the one under the pointer it is shown
/// see-through where it would land.
#[allow(clippy::too_many_arguments)]
fn show_aim(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    aimed: Res<Aimed>,
    server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut roots: Query<(Entity, &mut CardRoot)>,
    mut glows: Query<(Entity, &PileGlow, &mut Transform, Has<Lit>)>,
    ghosts: Query<Entity, With<CardGhost>>,
    together: Res<Together>,
) {
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
    let Some(table) = watch.table.as_ref() else {
        return;
    };
    let Some((root, mut shown)) = roots
        .iter_mut()
        .find(|(_, r)| r.room_code == table.room_code)
    else {
        return;
    };
    let turn = watch.turn.as_ref();
    let held = in_hand(turn);
    let next = match (&held, turn) {
        (Some(_), Some(turn)) => turn.choices(),
        _ => Vec::new(),
    };
    let position = watch.shown().unwrap_or(&table.state.state);
    let piles = piles(position, &shown.layout, shown.by_look.as_ref());
    let home = held.as_ref().and_then(|h| home_of(table, h));
    let targets: Vec<&str> = match (&shown.by_look, turn) {
        // Laid by its look: the piles whose choice plays what is chosen, once something is.
        (Some(look), Some(turn)) if turn.stage == Stage::Choosing => {
            let chosen = together.at(table.state.move_seq);
            look.piles
                .iter()
                .filter(|_| !chosen.is_empty())
                .filter(|(_, p)| {
                    p.click
                        .as_deref()
                        .is_some_and(|c| throw(&turn.options, &chosen, c).is_some())
                })
                .map(|(zone, _)| zone.as_str())
                .collect()
        }
        (Some(_), _) => Vec::new(),
        (None, _) => patience::targets(
            shown.layout.piles.keys().map(String::as_str),
            &next,
            home.as_deref(),
        ),
    };
    let landing = |zone: &str, n: usize| {
        let up = piles.get(zone).map(|p| ups(p)).unwrap_or_default();
        let at = shown.layout.landing(zone, &up, n, BASE);
        // A pile laid by its look glows where its top card lies: what it plays is already there.
        match (&shown.by_look, shown.layout.lie(zone, &up, BASE).last()) {
            (Some(_), Some(top)) => vec![*top],
            _ => at,
        }
    };
    for (entity, glow, mut t, lit) in &mut glows {
        let on = targets.contains(&glow.0.as_str());
        if on {
            if let Some(first) = landing(&glow.0, 1).first() {
                t.translation = first
                    .translation
                    .with_y(first.translation.y + CARD_SIZE.z / 2.0);
            }
            if !lit {
                commands.entity(entity).insert(Lit);
            }
        } else if lit {
            commands.entity(entity).remove::<Lit>();
        }
    }
    let (Some((from, count)), Some(over)) = (&held, &aimed.0) else {
        return;
    };
    if !targets.contains(&over.as_str()) {
        return;
    }
    let Some(lifted) = piles.get(from) else {
        return;
    };
    let cards = table.info.cards.as_ref();
    let ids = &lifted[lifted_from(lifted.len(), *count)];
    for (card, at) in ids.iter().zip(landing(over, *count)) {
        let [face, back, edge] = shown.looks.ghost(cards, &card.id, &server, &mut materials);
        let whole = commands
            .spawn((
                Name::new("Card ghost"),
                CardGhost,
                at,
                Visibility::default(),
                NotShadowCaster,
                ChildOf(root),
            ))
            .id();
        let by_part = [("face", face), ("back", back), ("edge", edge)];
        for part in parts(&mut commands, &server, &by_part, whole, Pickable::IGNORE) {
            commands.entity(part).insert(NotShadowCaster);
        }
    }
}

/// What the cards in hand follow, while there are any.
type HandHeld = (With<Anchor>, With<Held>);

/// The cards in hand follow it while it is held under the pointer, each beyond the one under
/// it; else (as after a click) they are lifted over where they lie.
fn follow_hand(
    anchors: Query<(&Transform, &Visibility), HandHeld>,
    mut cards: Query<(&InHand, &Rest, &mut Carry)>,
) {
    let hand = anchors
        .iter()
        .find(|(_, v)| **v != Visibility::Hidden)
        .map(|(t, _)| t.translation);
    for (in_hand, rest, mut carry) in &mut cards {
        let step = Vec3::new(0.0, 4.0 * CARD_SIZE.z, HELD_STEP) * in_hand.0 as f32;
        let wanted = match hand {
            Some(at) => Carry::Over {
                at: Transform::from_translation(at + step - Vec3::Z * GRIP),
                scale: 0.0,
            },
            None => Carry::Over {
                at: rest
                    .0
                    .with_translation(rest.0.translation + Vec3::Y * HELD_LIFT),
                scale: 1.0,
            },
        };
        carry.set_if_neq(wanted);
    }
}

/// What a click on the table may be on: a card, a pile's place, the bare cloth.
type Clicked<'w, 's> = (
    Query<'w, 's, &'static CardAt>,
    Query<'w, 's, &'static PileOf, With<Place>>,
    Query<'w, 's, (), With<Cloth>>,
);

fn on_click(
    click: On<PointerClick>,
    (cards, places, cloths): Clicked,
    roots: Query<&CardRoot>,
    net: Res<Net>,
    mut watch: ResMut<WatchStatus>,
    mut together: ResMut<Together>,
    dealing: Res<Dealing>,
) {
    // A click that ends a deal is the deal's.
    if click.button != PointerButton::Primary || dealing.holds_clicks() {
        return;
    }
    let (zone, known) = if let Ok(card) = cards.get(click.entity) {
        (card.zone.clone(), card.known.then(|| card.key.clone()))
    } else if let Ok(pile) = places.get(click.entity) {
        (pile.0.clone(), None)
    } else if cloths.contains(click.entity) {
        // The bare cloth: what is in hand goes back, and what is chosen is let go.
        if in_hand(watch.turn.as_ref()).is_some() {
            play::back(&mut watch);
        }
        if !together.chosen.is_empty() {
            together.chosen.clear();
        }
        return;
    } else {
        return;
    };
    let Some(turn) = watch.turn.as_ref().filter(|t| t.stage == Stage::Choosing) else {
        return;
    };
    let seq = watch.table.as_ref().map_or(0, |t| t.state.move_seq);
    // A patience's cards and places are clicked by the press and the release (`follow_cards`).
    let Some(look) = watch
        .table
        .as_ref()
        .and_then(|t| roots.iter().find(|r| r.room_code == t.room_code))
        .and_then(|r| r.by_look.as_ref())
    else {
        return;
    };
    // A card of the hand, played by the click.
    let played = known
        .as_deref()
        .and_then(|card| card_choice(look, card, &turn.choices()));
    if let Some(choice) = played {
        play::gesture(&net, &mut watch, &choice);
        return;
    }
    match look_click(look, &zone, &together.at(seq), &turn.options) {
        LookClick::Choose(chosen) => *together = Together { seq, chosen },
        LookClick::Play(gestures) => {
            together.chosen.clear();
            play::enter_all(&net, &mut watch, &gestures);
        }
        LookClick::Nothing => {}
    }
}

/// What a click on `zone` (on its card `from_top` from the top, when its face is shown, or on
/// the pile) does in a patience, whether it is a press and a release there or cards dragged and
/// let go there: lift cards, put down what is in hand (the move played), put it back, draw.
fn click_pile(
    net: &Net,
    watch: &mut ResMut<WatchStatus>,
    put_on: &mut PutOn,
    zone: &str,
    from_top: Option<usize>,
) -> Outcome {
    let Some(turn) = watch.turn.as_ref().filter(|t| t.stage == Stage::Choosing) else {
        return Outcome::Ignored;
    };
    let lifted = patience::lifted(&turn.gestures);
    let next = turn.choices();
    let held = lifted.as_ref().map(|(z, n)| (z.as_str(), *n));
    match patience::click(zone, from_top, held, &next, &turn.options) {
        Click::Gesture(g) => {
            if g.value.starts_with("to-") {
                // Sent home, it is shown going to its suit's foundation, when that is known.
                let home = (g.value == "to-f")
                    .then(|| {
                        let table = watch.table.as_ref()?;
                        home_of(table, lifted.as_ref()?)
                    })
                    .flatten();
                put_on.0 = Some(home.unwrap_or_else(|| zone.to_string()));
            }
            play::gesture(net, watch, &g)
        }
        Click::PutBack => {
            play::back(watch);
            Outcome::Changed
        }
        Click::Nothing => Outcome::Ignored,
    }
}

/// Whether a click on `zone` would put what is in hand on it.
fn puts_on(turn: Option<&Turn>, zone: &str) -> bool {
    let Some(turn) = turn.filter(|t| t.stage == Stage::Choosing) else {
        return false;
    };
    let lifted = patience::lifted(&turn.gestures);
    let held = lifted.as_ref().map(|(z, n)| (z.as_str(), *n));
    matches!(
        patience::click(zone, None, held, &turn.choices(), &turn.options),
        Click::Gesture(g) if g.value.starts_with("to-")
    )
}

/// Whether the watched game is a patience on the table (not one laid by its look).
fn patience_shown(watch: &WatchStatus, roots: &Query<&CardRoot>) -> bool {
    watch.table.as_ref().is_some_and(|t| {
        roots
            .iter()
            .any(|r| r.room_code == t.room_code && r.by_look.is_none())
    })
}

/// A press of the primary button on a patience's card or pile's place begins one (`drag.rs`).
/// Presses bubble up from a card's parts to the card.
fn on_press(
    press: On<PointerPress>,
    piles: Query<(&PileOf, Option<&CardAt>)>,
    roots: Query<&CardRoot>,
    watch: Res<WatchStatus>,
    menu: Option<Res<EscapeMenu>>,
    mut drag: ResMut<Drag>,
    dealing: Res<Dealing>,
) {
    if press.button != PointerButton::Primary
        || dealing.holds_clicks()
        || drag.pressed()
        || !escape_menu::closed(menu.as_deref())
        || !patience_shown(&watch, &roots)
    {
        return;
    }
    let Ok((pile, card)) = piles.get(press.entity) else {
        return;
    };
    drag.1 = Some(CardPress {
        pointer: press.pointer.id,
        entity: press.entity,
        zone: pile.0.clone(),
        from_top: card.filter(|c| c.shown).map(|c| c.from_top),
        before: watch.turn.clone(),
        left: false,
        carrying: false,
    });
}

/// Follows a press on the cards while the button is down: the pointer leaving what was
/// pressed (a drag, lifting what a click there would), and the button let go (the click, the
/// click on the pile let go over, or the turn as it was). A turn that ends ends it; the escape
/// menu cancels it.
fn follow_cards(
    mut drag: ResMut<Drag>,
    mut inputs: MessageReader<PointerInput>,
    (pointed, aimed): (Res<Pointed>, Res<Aimed>),
    menu: Option<Res<EscapeMenu>>,
    net: Res<Net>,
    mut watch: ResMut<WatchStatus>,
    mut put_on: ResMut<PutOn>,
) {
    let Some(pointer) = drag.1.as_ref().map(|p| p.pointer) else {
        inputs.clear();
        return;
    };
    let released = inputs
        .read()
        .filter(|i| {
            i.pointer_id == pointer
                && matches!(i.action, PointerAction::Release(PointerButton::Primary))
        })
        .count()
        > 0;
    let asked_at = |t: &Option<Turn>| t.as_ref().map(|t| t.asked_at);
    if drag
        .1
        .as_ref()
        .is_some_and(|p| p.before.is_some() && asked_at(&p.before) != asked_at(&watch.turn))
    {
        drag.1 = None;
        return;
    }
    if !escape_menu::closed(menu.as_deref()) {
        drag::cancel(&mut drag, &mut watch);
        return;
    }
    let press = drag.1.as_ref().expect("a press");
    if !press.left && pointed.0 != Some(press.entity) {
        let (zone, from_top) = (press.zone.clone(), press.from_top);
        let held = |w: &WatchStatus| in_hand(w.turn.as_ref()).map(|(z, _)| z);
        // Lifted already (by a click, pressed on its own pile), they are taken up as they are.
        if held(&watch).as_deref() != Some(zone.as_str()) {
            let lifts = watch
                .turn
                .as_ref()
                .filter(|t| t.stage == Stage::Choosing)
                .is_some_and(|t| {
                    let lifted = patience::lifted(&t.gestures);
                    let held = lifted.as_ref().map(|(z, n)| (z.as_str(), *n));
                    matches!(
                        patience::click(&zone, from_top, held, &t.choices(), &t.options),
                        Click::Gesture(g) if patience::lifted(std::slice::from_ref(&g)).is_some()
                    )
                });
            if lifts {
                click_pile(&net, &mut watch, &mut put_on, &zone, from_top);
            }
        }
        let carrying = held(&watch).as_deref() == Some(zone.as_str());
        let press = drag.1.as_mut().expect("a press");
        press.left = true;
        press.carrying = carrying;
    }
    if !released {
        return;
    }
    let press = drag.1.take().expect("a press");
    let turn = watch.turn.clone();
    match drag::let_go_cards(&press, aimed.0.as_deref(), |at| puts_on(turn.as_ref(), at)) {
        LetGo::Click(zone) => {
            click_pile(&net, &mut watch, &mut put_on, &zone, press.from_top);
        }
        LetGo::Drop(at) => {
            if click_pile(&net, &mut watch, &mut put_on, &at, None) == Outcome::Ignored {
                drag::restore(&mut watch, press.before);
            }
        }
        LetGo::Restore => drag::restore(&mut watch, press.before),
        LetGo::Stay | LetGo::Nothing => {}
    }
}

/// While a drag carries the cards in hand, what they follow is held under the pointer.
fn hold_cards(
    mut commands: Commands,
    drag: Res<Drag>,
    anchors: Query<(Entity, Has<Held>), With<Anchor>>,
) {
    let carrying = drag.carried_cards().is_some();
    for (anchor, holding) in &anchors {
        match (carrying, holding) {
            (true, false) => {
                commands.entity(anchor).insert((
                    Held {
                        board: BASE,
                        lift: HELD_LIFT,
                        scale: 1.0,
                    },
                    // Where the pointer is, not sliding there from where it was last.
                    Visibility::Hidden,
                ));
            }
            (false, true) => {
                commands.entity(anchor).remove::<Held>();
            }
            _ => {}
        }
    }
}

/// What a click on a card or a pile of `zone` would do, for the pointer's hand: lift cards
/// to move them, put down what is in hand or choose a pile (a pin, a ball), or nothing. As
/// `on_click` decides: `from_top` is the card clicked, when its face is shown.
fn feel(
    zone: &str,
    from_top: Option<usize>,
    turn: Option<&Turn>,
    by_look: Option<&CardsLook>,
    chosen: &BTreeSet<String>,
    known: Option<&str>,
) -> Feel {
    let Some(turn) = turn.filter(|t| t.stage == Stage::Choosing) else {
        return Feel::Open;
    };
    if let Some(look) = by_look {
        if known.is_some_and(|card| card_choice(look, card, &turn.choices()).is_some()) {
            return Feel::Point;
        }
        return match look_click(look, zone, chosen, &turn.options) {
            LookClick::Nothing => Feel::Open,
            LookClick::Choose(_) | LookClick::Play(_) => Feel::Point,
        };
    }
    let lifted = patience::lifted(&turn.gestures);
    let held = lifted.as_ref().map(|(z, n)| (z.as_str(), *n));
    match patience::click(zone, from_top, held, &turn.choices(), &turn.options) {
        Click::Gesture(g) if patience::lifted(std::slice::from_ref(&g)).is_some() => Feel::Grab,
        Click::Gesture(_) | Click::PutBack => Feel::Point,
        Click::Nothing => Feel::Open,
    }
}

/// Says what a click on the cards under the pointer would do (`cursor.rs`).
fn sense(
    under: Res<Under>,
    (cards, places): (Query<&CardAt>, Query<&PileOf, With<Place>>),
    roots: Query<&CardRoot>,
    (watch, together): (Res<WatchStatus>, Res<Together>),
    mut table: ResMut<TableFeel>,
) {
    let Some((zone, from_top, known)) = under.chain.iter().find_map(|e| {
        if let Ok(card) = cards.get(*e) {
            Some((
                card.zone.as_str(),
                card.shown.then_some(card.from_top),
                card.known.then_some(card.key.as_str()),
            ))
        } else {
            places.get(*e).ok().map(|p| (p.0.as_str(), None, None))
        }
    }) else {
        return;
    };
    let seq = watch.table.as_ref().map_or(0, |t| t.state.move_seq);
    let by_look = watch
        .table
        .as_ref()
        .and_then(|t| roots.iter().find(|r| r.room_code == t.room_code))
        .and_then(|r| r.by_look.as_ref());
    table.set_if_neq(TableFeel(feel(
        zone,
        from_top,
        watch.turn.as_ref(),
        by_look,
        &together.at(seq),
        known,
    )));
}

/// In a game whose look has a click on a card play it: the cards that may be played stand a
/// little out of their fan while not all may be, and the one under the pointer further. A
/// card's parts are moved, not the card, which stays where it rests.
fn stand_out(
    time: Res<Time>,
    under: Res<Under>,
    watch: Res<WatchStatus>,
    roots: Query<&CardRoot>,
    cards: Query<(Entity, &CardAt, &Parts)>,
    mut parts: Query<&mut Transform, Without<CardAt>>,
) {
    let Some(look) = watch
        .table
        .as_ref()
        .and_then(|t| roots.iter().find(|r| r.room_code == t.room_code))
        .and_then(|r| r.by_look.as_ref())
        .filter(|l| l.play.is_some())
    else {
        return;
    };
    let choices = watch
        .turn
        .as_ref()
        .filter(|t| t.stage == Stage::Choosing)
        .map(Turn::choices)
        .unwrap_or_default();
    let may = |at: &CardAt| at.known && card_choice(look, &at.key, &choices).is_some();
    // The zones where some card may be played and another may not.
    let mixed: HashSet<&str> = cards
        .iter()
        .filter(|(_, at, _)| at.known && !may(at))
        .map(|(_, at, _)| at.zone.as_str())
        .collect();
    let pointed = under.chain.iter().find(|e| cards.contains(**e)).copied();
    let ease = 1.0 - (-18.0 * time.delta_secs()).exp();
    for (entity, at, card_parts) in &cards {
        let mut out = 0.0;
        if may(at) {
            if mixed.contains(at.zone.as_str()) {
                out += PLAYABLE_OUT;
            }
            if pointed == Some(entity) {
                out += POINTED_OUT;
            }
        }
        for part in &card_parts.0 {
            if let Ok(mut t) = parts.get_mut(*part) {
                // A card's top is its far side as it lies: towards -z.
                let wanted = -out;
                if (t.translation.z - wanted).abs() > 1e-5 {
                    t.translation.z += (wanted - t.translation.z) * ease;
                }
            }
        }
    }
}

/// The player's own hand along the bottom of the window: its cards that have come to rest at
/// the near edge of the table, where they are unseen, left to right, each with its picture and
/// the choice a click on it makes now. One on its way there or from there is seen on the table.
fn fill_strip(
    watch: Res<WatchStatus>,
    roots: Query<&CardRoot>,
    mut cards: Query<(&CardAt, &Transform, &Rest, &mut Visibility)>,
    mut strip: ResMut<HandStrip>,
) {
    let root = watch
        .table
        .as_ref()
        .and_then(|t| roots.iter().find(|r| r.room_code == t.room_code));
    let Some((table, root, zone)) = watch
        .table
        .as_ref()
        .zip(root)
        .and_then(|(t, r)| Some((t, r, r.layout.strip.as_deref()?)))
    else {
        if !strip.0.is_empty() {
            strip.0.clear();
        }
        return;
    };
    let choices = watch
        .turn
        .as_ref()
        .filter(|t| t.stage == Stage::Choosing)
        .map(Turn::choices)
        .unwrap_or_default();
    let mut held: Vec<(usize, StripCard)> = Vec::new();
    for (at, t, rest, mut visibility) in &mut cards {
        let resting = at.zone == zone && t.translation.distance(rest.0.translation) < 1e-4;
        let wanted = if resting {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        visibility.set_if_neq(wanted);
        if !resting {
            continue;
        }
        let picture = if at.known {
            face_file(table.info.cards.as_ref(), &at.key)
        } else {
            Some(look::CARD_BACK.to_string())
        };
        let choice = root
            .by_look
            .as_ref()
            .filter(|_| at.known)
            .and_then(|l| card_choice(l, &at.key, &choices));
        held.push((
            at.from_top,
            StripCard {
                key: at.key.clone(),
                picture,
                choice,
            },
        ));
    }
    // The bottom card of the pile at the left.
    held.sort_by_key(|(from_top, _)| std::cmp::Reverse(*from_top));
    let now: Vec<StripCard> = held.into_iter().map(|(_, c)| c).collect();
    if strip.0 != now {
        strip.0 = now;
    }
}

/// Backspace lets go of what is chosen together, as it takes back what is in hand.
fn let_go(keys: Option<Res<ButtonInput<KeyCode>>>, mut together: ResMut<Together>) {
    let pressed = keys.is_some_and(|k| k.just_pressed(KeyCode::Backspace));
    if pressed && !together.chosen.is_empty() {
        together.chosen.clear();
    }
}

/// Whether a pile's card rests somewhere else now.
fn moved(was: Transform, now: Transform) -> bool {
    was.translation.distance(now.translation) > 1e-5
}

fn on_over(
    over: On<PointerOver>,
    piles: Query<&PileOf>,
    mut aimed: ResMut<Aimed>,
    mut pointed: ResMut<Pointed>,
) {
    let Ok(pile) = piles.get(over.entity) else {
        return;
    };
    if aimed.0.as_ref() != Some(&pile.0) {
        aimed.0 = Some(pile.0.clone());
    }
    if pointed.0 != Some(over.entity) {
        pointed.0 = Some(over.entity);
    }
}

fn on_out(
    out: On<PointerOut>,
    piles: Query<&PileOf>,
    mut aimed: ResMut<Aimed>,
    mut pointed: ResMut<Pointed>,
) {
    let Ok(pile) = piles.get(out.entity) else {
        return;
    };
    if aimed.0.as_ref() == Some(&pile.0) {
        aimed.0 = None;
    }
    if pointed.0 == Some(out.entity) {
        pointed.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::session::Opened;
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::Pointer;
    use bevy::picking::pointer::{Location, PointerId};
    use serde_json::json;

    /// A small patience as the server describes it: four cards, a stock, a waste, a
    /// foundation and two columns, with `zones` as the position has them.
    fn table(seq: u64, zones: serde_json::Value) -> Table {
        let info = json!({
            "id": "g", "name": "A patience",
            "seats": [{"seat": 1, "color": "red", "label": "Player"}],
            "board": {"kind": "grid", "positions": [{"id": "0,0", "x": 0, "y": 0}]},
            "actions": [{"id": "patience", "type": "patience", "label": "Move"}],
            "cards": {
                "suits": {"hearts": {"label": "Hearts", "symbol": "♥"},
                          "spades": {"label": "Spades", "symbol": "♠"}},
                "suit_order": ["hearts", "spades"],
                "types": [
                    {"id": "hearts-A", "suit": "hearts", "rank": "A", "label": "A", "value": 1},
                    {"id": "spades-K", "suit": "spades", "rank": "K", "label": "K", "value": 13},
                    {"id": "spades-2", "suit": "spades", "rank": "2", "label": "2", "value": 2}
                ],
                "back": {}, "zones": [{"id": "stock", "scope": "table"}]
            }
        });
        let state = json!({
            "state": {"cells": {}, "cards": {"zones": zones}},
            "status": "active", "move_seq": seq, "impulse_seat": 1
        });
        let game = json!({"game_def_id": "g", "status": "active", "seats": []});
        Table::new(Opened {
            room_code: "AB12CD".into(),
            info: serde_json::from_value(info).unwrap(),
            game: serde_json::from_value(game).unwrap(),
            state: serde_json::from_value(state).unwrap(),
            bots: vec![],
        })
    }

    fn dealt(seq: u64) -> Table {
        table(
            seq,
            json!({
                "stock": {"cards": [{}, {}, {}]},
                "waste": {},
                "f1": {},
                "t1": {"cards": [{"id": "hearts-A", "up": true}]},
                "t2": {"cards": [{}, {"id": "spades-K", "up": true}]}
            }),
        )
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), CardTablePlugin))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .insert_resource(Net::for_test().0)
            .init_resource::<WatchStatus>();
        app
    }

    fn watch(app: &mut App, table: Table, turn: Option<Turn>) {
        let mut watch = app.world_mut().resource_mut::<WatchStatus>();
        watch.table = Some(table);
        watch.turn = turn;
    }

    fn card(app: &mut App, key: &str) -> Entity {
        let world = app.world_mut();
        let mut cards = world.query::<(Entity, &CardAt)>();
        cards
            .iter(world)
            .find(|(_, at)| at.key == key)
            .unwrap_or_else(|| panic!("no card {key}"))
            .0
    }

    fn act(value: &str) -> crate::net::api::Gesture {
        crate::net::api::Gesture {
            kind: "action".into(),
            value: value.into(),
        }
    }

    #[test]
    fn a_game_with_cards_has_cards_in_the_scene_before_any_table_is_laid() {
        let mut app = app();
        app.update();
        let mut warm = app.world_mut().query_filtered::<(), With<Warm>>();
        assert_eq!(warm.iter(app.world()).count(), 0, "no game has cards yet");

        app.world_mut()
            .resource_mut::<Appearances>()
            .0
            .push(crate::test_hand::look());
        app.update();
        app.update();
        let world = app.world_mut();
        let mut warm = world.query_filtered::<&Transform, With<Warm>>();
        let sets = warm.iter(world).count();
        assert!(sets >= 2, "plain and see-through, at least: {sets}");
        assert!(
            warm.iter(world).all(|t| t.scale.max_element() < 1.0e-3),
            "too small to be seen"
        );

        // Not again for each change of the looks.
        world
            .resource_mut::<Appearances>()
            .0
            .push(crate::test_hand::look());
        app.update();
        app.update();
        let world = app.world_mut();
        let mut warm = world.query_filtered::<(), With<Warm>>();
        assert_eq!(warm.iter(world).count(), sets);
    }

    #[test]
    fn the_hand_grabs_a_card_a_click_lifts_and_points_at_where_it_may_go() {
        let mut turn = Turn::new(1);
        turn.clicks_only = true;
        turn.options = vec![LegalSelection {
            notation: "t1-f".into(),
            action: "patience".into(),
            gestures: vec![act("from-t1-1"), act("to-f")],
            ..Default::default()
        }];
        turn.stage = Stage::Choosing;
        let none = BTreeSet::new();
        assert_eq!(
            feel("t1", Some(1), Some(&turn), None, &none, None),
            Feel::Grab
        );
        assert_eq!(
            feel("t3", Some(1), Some(&turn), None, &none, None),
            Feel::Open
        );
        assert_eq!(
            feel("t1", Some(1), None, None, &none, None),
            Feel::Open,
            "not ours"
        );
        // Lifted: the foundation it goes to is pointed at, and so is where it came from (a
        // click there puts it back).
        turn.gestures = vec![act("from-t1-1")];
        assert_eq!(
            feel("f1", None, Some(&turn), None, &none, None),
            Feel::Point
        );
        assert_eq!(
            feel("t1", Some(1), Some(&turn), None, &none, None),
            Feel::Point
        );
    }

    #[test]
    fn a_patience_is_laid_as_it_lies_its_cards_lifted_over_their_pile_and_carried_where_they_go() {
        let mut app = app();
        watch(&mut app, dealt(1), None);
        app.update();
        app.update();
        let world = app.world_mut();
        let cards: Vec<CardAt> = world.query::<&CardAt>().iter(world).cloned().collect();
        assert_eq!(cards.len(), 6, "every card of every pile");
        assert_eq!(world.query::<&Place>().iter(world).count(), 5);
        let ace = card(&mut app, "hearts-A");
        let at = app.world().get::<CardAt>(ace).unwrap().clone();
        assert_eq!((at.zone.as_str(), at.from_top, at.shown), ("t1", 1, true));
        let down = card(&mut app, "t2#0");
        assert_eq!(app.world().get::<CardAt>(down).unwrap().from_top, 2);
        assert!(
            app.world().get::<Carry>(ace).is_none(),
            "a table just laid deals nothing"
        );

        // The ace lifted: it is in hand, over its pile, and nothing follows the pointer.
        let mut turn = Turn::new(1);
        turn.clicks_only = true;
        turn.options = vec![crate::net::api::LegalSelection {
            notation: "t1-f".into(),
            action: "patience".into(),
            gestures: vec![act("from-t1-1"), act("to-f")],
            ..Default::default()
        }];
        turn.stage = Stage::Choosing;
        turn.gestures = vec![act("from-t1-1")];
        watch(&mut app, dealt(1), Some(turn));
        app.update();
        app.update();
        assert_eq!(app.world().get::<InHand>(ace), Some(&InHand(0)));
        let rest = app.world().get::<Rest>(ace).unwrap().0;
        assert_eq!(
            app.world().get::<Carry>(ace),
            Some(&Carry::Over {
                at: rest.with_translation(rest.translation + Vec3::Y * HELD_LIFT),
                scale: 1.0,
            }),
            "lifted over where it lies"
        );
        let world = app.world_mut();
        let held = world.query_filtered::<(), With<Held>>().iter(world).count();
        assert_eq!(held, 0, "nothing under the pointer");
        let lit: Vec<String> = world
            .query_filtered::<&PileGlow, With<Lit>>()
            .iter(world)
            .map(|g| g.0.clone())
            .collect();
        assert_eq!(lit, ["f1"], "where it may go glows");

        // Played: the ace is on the foundation, and goes there.
        watch(
            &mut app,
            table(
                2,
                json!({
                    "stock": {"cards": [{}, {}, {}]},
                    "f1": {"cards": [{"id": "hearts-A", "up": true}]},
                    "t2": {"cards": [{}, {"id": "spades-K", "up": true}]}
                }),
            ),
            None,
        );
        app.update();
        let world = app.world();
        assert_eq!(world.get::<InHand>(ace), None);
        let rest = world.get::<Rest>(ace).unwrap().0;
        assert_eq!(world.get::<Carry>(ace), Some(&Carry::Put(rest)));
        let layout =
            patience::layout([("stock", 3), ("waste", 0), ("f1", 0), ("t1", 0), ("t2", 0)]);
        assert_eq!(rest, layout.lie("f1", &[true], BASE)[0]);
        let world = app.world_mut();
        let held = world
            .query_filtered::<(), (With<Anchor>, With<Held>)>()
            .iter(world)
            .count();
        assert_eq!(held, 0, "nothing in hand");
    }

    /// A column with a run of two on a card face down, an empty column and a foundation: the
    /// ace may go home, or the run to the empty column.
    fn run_table() -> Table {
        table(
            1,
            json!({
                "stock": {"cards": [{}]},
                "waste": {},
                "f1": {},
                "t1": {"cards": [{}, {"id": "spades-2", "up": true}, {"id": "hearts-A", "up": true}]},
                "t2": {}
            }),
        )
    }

    fn run_turn() -> Turn {
        let mut turn = Turn::new(1);
        turn.clicks_only = true;
        let option = |notation: &str, gestures: &[&str]| LegalSelection {
            notation: notation.into(),
            action: "patience".into(),
            gestures: gestures.iter().map(|g| act(g)).collect(),
            ..Default::default()
        };
        turn.options = vec![
            option("t1-f", &["from-t1-1", "to-f"]),
            option("t1-t2", &["from-t1-2", "to-t2"]),
        ];
        turn.stage = Stage::Choosing;
        turn
    }

    /// A patience laid and ours to play, and what it asks of the server.
    struct Patience {
        app: App,
        requests: std::sync::mpsc::Receiver<crate::net::Request>,
    }

    impl Patience {
        fn new() -> Self {
            let mut app = app();
            let (net, requests) = Net::for_test();
            app.insert_resource(net);
            watch(&mut app, run_table(), Some(run_turn()));
            app.update();
            app.update();
            Patience { app, requests }
        }

        fn place(&mut self, zone: &str) -> Entity {
            let world = self.app.world_mut();
            let mut places = world.query_filtered::<(Entity, &PileOf), With<Place>>();
            places.iter(world).find(|(_, p)| p.0 == zone).unwrap().0
        }

        /// The pointer over `what` (a card or a place) and its pile, or over the bare cloth.
        fn over(&mut self, what: Option<Entity>) {
            let zone = what.and_then(|e| self.app.world().get::<PileOf>(e).map(|p| p.0.clone()));
            self.app.world_mut().resource_mut::<Pointed>().0 = what;
            self.app.world_mut().resource_mut::<Aimed>().0 = zone;
            self.app.update();
        }

        fn press(&mut self, what: Entity) {
            self.over(Some(what));
            self.app.world_mut().trigger(PointerPress {
                entity: what,
                pointer: Pointer::new(PointerId::Mouse, nowhere()),
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                count: 1,
            });
            self.app.update();
        }

        fn release(&mut self) {
            self.app.world_mut().write_message(PointerInput::new(
                PointerId::Mouse,
                nowhere(),
                PointerAction::Release(PointerButton::Primary),
            ));
            self.app.update();
            self.app.update();
        }

        fn turn(&self) -> &Turn {
            let watch = self.app.world().resource::<WatchStatus>();
            watch.turn.as_ref().expect("a turn")
        }

        fn gestures(&self) -> Vec<String> {
            self.turn()
                .gestures
                .iter()
                .map(|g| g.value.clone())
                .collect()
        }

        fn held(&mut self) -> bool {
            let world = self.app.world_mut();
            let mut q = world.query_filtered::<(), (With<Anchor>, With<Held>)>();
            q.iter(world).count() == 1
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
    fn a_run_dragged_to_a_column_plays_the_move() {
        let mut p = Patience::new();
        let two = card(&mut p.app, "spades-2");
        let ace = card(&mut p.app, "hearts-A");
        p.press(two);
        assert!(p.gestures().is_empty(), "a press alone does nothing");
        // Off the card with the button down: the run is lifted and carried under the pointer.
        p.over(None);
        assert_eq!(p.gestures(), ["from-t1-2"]);
        assert!(p.held(), "under the pointer");
        p.app.update();
        assert_eq!(p.app.world().get::<InHand>(ace), Some(&InHand(1)));
        let to = p.place("t2");
        p.over(Some(to));
        p.release();
        assert_eq!(p.turn().made_notations(), ["t1-t2"], "the click on t2");
        assert_ne!(p.turn().stage, Stage::Choosing);
        assert!(p.asked() > 0, "played at once");
        assert!(!p.held());
        assert!(!p.app.world().resource::<Drag>().pressed());
    }

    #[test]
    fn a_card_let_go_over_the_bare_cloth_goes_back() {
        let mut p = Patience::new();
        let ace = card(&mut p.app, "hearts-A");
        p.press(ace);
        p.over(None);
        assert_eq!(p.gestures(), ["from-t1-1"]);
        assert!(p.held());
        p.release();
        assert!(p.gestures().is_empty(), "back where it was");
        assert!(!p.held());
        assert_eq!(p.asked(), 0);
        // Let go over a pile it may not go on, or its own: back too.
        for elsewhere in ["t2", "t1"] {
            p.press(ace);
            p.over(None);
            let at = p.place(elsewhere);
            p.over(Some(at));
            p.release();
            assert!(p.gestures().is_empty(), "let go over {elsewhere}");
            assert_eq!(p.asked(), 0);
        }
    }

    #[test]
    fn a_press_and_release_on_a_card_is_still_a_click() {
        let mut p = Patience::new();
        let ace = card(&mut p.app, "hearts-A");
        p.press(ace);
        p.release();
        assert_eq!(p.gestures(), ["from-t1-1"], "lifted, over its pile");
        assert!(!p.held(), "nothing under the pointer");
        // A press and release on the foundation: put there, and played.
        let home = p.place("f1");
        p.press(home);
        p.release();
        assert_eq!(p.turn().made_notations(), ["t1-f"]);
        assert!(p.asked() > 0);
    }

    #[test]
    fn cards_lifted_by_a_click_are_dragged_on_from_where_they_wait() {
        let mut p = Patience::new();
        let ace = card(&mut p.app, "hearts-A");
        let two = card(&mut p.app, "spades-2");
        p.press(ace);
        p.release();
        let lifted = p.turn().clone();
        // Pressed where it waits, over its own pile (the card under it), and carried off.
        p.press(two);
        p.over(None);
        assert_eq!(p.gestures(), ["from-t1-1"], "not put back");
        assert!(p.held());
        // A right-click: lifted, as it was before the press.
        p.app
            .world_mut()
            .run_system_cached(|mut drag: ResMut<Drag>, mut watch: ResMut<WatchStatus>| {
                drag::cancel(&mut drag, &mut watch)
            })
            .unwrap();
        p.app.update();
        assert_eq!(*p.turn(), lifted);
        assert!(!p.held());
        p.release();
        assert_eq!(*p.turn(), lifted, "the release that follows does nothing");
        // Again, onto the foundation.
        p.press(two);
        p.over(None);
        let home = p.place("f1");
        p.over(Some(home));
        p.release();
        assert_eq!(p.turn().made_notations(), ["t1-f"]);
    }

    #[test]
    fn a_card_drawn_comes_from_the_stock_where_it_lay_face_down() {
        let mut app = app();
        watch(&mut app, dealt(1), None);
        app.update();
        app.update();
        let top = card(&mut app, "stock#2");
        let was = *app.world().get::<Transform>(top).unwrap();
        watch(
            &mut app,
            table(
                2,
                json!({
                    "stock": {"cards": [{}, {}]},
                    "waste": {"cards": [{"id": "spades-2", "up": true}]},
                    "f1": {},
                    "t1": {"cards": [{"id": "hearts-A", "up": true}]},
                    "t2": {"cards": [{}, {"id": "spades-K", "up": true}]}
                }),
            ),
            None,
        );
        app.update();
        app.update();
        let drawn = card(&mut app, "spades-2");
        let world = app.world();
        assert!(world.get_entity(top).is_err(), "the face-down card is gone");
        let rest = world.get::<Rest>(drawn).unwrap().0;
        assert_eq!(world.get::<Carry>(drawn), Some(&Carry::Put(rest)));
        let start = world.get::<Transform>(drawn).unwrap();
        assert_eq!(start.translation, was.translation, "from the stock");
        assert!((start.rotation * Vec3::Y).y < 0.0, "face down as it was");
        assert!(
            (rest.rotation * Vec3::Y).y > 0.0,
            "and turned up where it goes"
        );
    }

    /// A game laid by its look: two pins chosen together, a ball that knocks them down, and a
    /// pile of what is out of play.
    const LOOK: &str = r#"
        games = ["rack"]
        [cards]
        together = "pin-"
        sheet = "score"
        [cards.piles.pina]
        at = [0.0, 0.0]
        click = "pin-a"
        [cards.piles.pinb]
        at = [0.08, 0.0]
        click = "pin-b"
        [cards.piles.ball1]
        at = [0.2, 0.1]
        click = "ball-1"
        [cards.piles.gone]
        at = [0.2, -0.1]
        spread = "fanned"
    "#;

    fn look() -> Appearance {
        Appearance::parse(LOOK).unwrap()
    }

    /// A move as the server lists it, its gestures in its own order.
    fn play_of(values: &[&str]) -> LegalSelection {
        LegalSelection {
            notation: values.join(" "),
            action: "move".into(),
            gestures: values.iter().map(|v| act(v)).collect(),
            ..Default::default()
        }
    }

    fn set(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn choices_made_together_are_chosen_in_any_order_and_played_in_the_servers() {
        let options = [
            play_of(&["pin-a", "pin-b", "ball-1"]),
            play_of(&["pin-b", "ball-1"]),
            play_of(&["end"]),
        ];
        assert_eq!(
            throw(&options, &set(&["pin-b", "pin-a"]), "ball-1"),
            Some(options[0].gestures.clone()),
            "chosen b then a, sent a then b"
        );
        assert_eq!(
            throw(&options, &set(&["pin-b"]), "ball-1"),
            Some(options[1].gestures.clone())
        );
        assert_eq!(
            throw(&options, &set(&["pin-a"]), "ball-1"),
            None,
            "no such play"
        );
        assert_eq!(
            throw(&options, &set(&[]), "end"),
            Some(options[2].gestures.clone())
        );
        assert!(reachable(&options, "pin-a"));
        assert!(!reachable(&options, "pin-c"));

        let cards = look().cards.unwrap();
        let none = set(&[]);
        assert_eq!(
            look_click(&cards, "pinb", &none, &options),
            LookClick::Choose(set(&["pin-b"]))
        );
        assert_eq!(
            look_click(&cards, "pinb", &set(&["pin-a", "pin-b"]), &options),
            LookClick::Choose(set(&["pin-a"])),
            "chosen again, let go"
        );
        assert_eq!(
            look_click(&cards, "ball1", &set(&["pin-a", "pin-b"]), &options),
            LookClick::Play(options[0].gestures.clone())
        );
        assert_eq!(
            look_click(&cards, "ball1", &set(&["pin-a"]), &options),
            LookClick::Nothing,
            "a ball that knocks down no such set"
        );
        assert_eq!(
            look_click(&cards, "gone", &none, &options),
            LookClick::Nothing
        );
        let only_b = [play_of(&["pin-b", "ball-1"])];
        assert_eq!(
            look_click(&cards, "pina", &none, &only_b),
            LookClick::Nothing,
            "a pin no play takes may not be chosen"
        );
    }

    /// The game laid by `LOOK`, its cards in the view's table only, as a game whose rules are
    /// written in Go has them.
    fn rack(seq: u64, pins: &[(&str, &str)], gone: &[&str]) -> Table {
        let mut table = table(seq, json!({}));
        table.info.id = "rack".into();
        table.info.actions[0].kind = "move".into();
        let mut zones: Vec<serde_json::Value> = pins
            .iter()
            .map(|(zone, id)| json!({"id": zone, "count": 1, "cards": [{"id": id, "up": true}]}))
            .collect();
        zones.push(json!({"id": "ball1", "count": 2,
            "cards": [{}, {"id": "spades-K", "up": true}]}));
        zones.push(json!({"id": "gone", "count": gone.len(),
            "cards": gone.iter().map(|id| json!({"id": id, "up": true})).collect::<Vec<_>>()}));
        table.state.state.ext = json!({"view": {"table": zones, "score": {"frame": 1}}});
        table
    }

    #[test]
    fn a_game_laid_by_its_look_lifts_what_is_chosen_and_lights_the_ball_that_plays_it() {
        let mut app = app();
        app.insert_resource(Appearances(vec![look()]));
        let pins = [("pina", "hearts-A"), ("pinb", "spades-2")];
        watch(&mut app, rack(1, &pins, &[]), None);
        app.update();
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&CardAt>().iter(world).count(), 4);
        assert_eq!(world.query::<&Place>().iter(world).count(), 4);

        let mut turn = Turn::new(1);
        turn.clicks_only = true;
        turn.options = vec![play_of(&["pin-a", "pin-b", "ball-1"])];
        turn.stage = Stage::Choosing;
        watch(&mut app, rack(1, &pins, &[]), Some(turn));
        app.world_mut().insert_resource(Together {
            seq: 1,
            chosen: set(&["pin-b"]),
        });
        app.update();
        let b = card(&mut app, "spades-2");
        let a = card(&mut app, "hearts-A");
        assert!(matches!(
            app.world().get::<Carry>(b),
            Some(Carry::Held { .. })
        ));
        assert_eq!(app.world().get::<Carry>(a), None);
        let lit = |app: &mut App| -> Vec<String> {
            let world = app.world_mut();
            world
                .query_filtered::<&PileGlow, With<Lit>>()
                .iter(world)
                .map(|g| g.0.clone())
                .collect()
        };
        assert!(lit(&mut app).is_empty(), "no ball knocks down b alone");

        app.world_mut().resource_mut::<Together>().chosen = set(&["pin-a", "pin-b"]);
        app.update();
        assert_eq!(lit(&mut app), ["ball1"]);

        // Thrown: the pins are out of play, and what was chosen is let go.
        watch(&mut app, rack(2, &[], &["hearts-A", "spades-2"]), None);
        app.update();
        let rest = app.world().get::<Rest>(b).unwrap().0;
        assert_eq!(app.world().get::<Carry>(b), Some(&Carry::Put(rest)));
        assert!(lit(&mut app).is_empty());
    }

    #[test]
    fn a_cards_glow_goes_round_its_rounded_corners_the_way_a_cells_does() {
        let corners = card_corners();
        assert_eq!(corners.len(), 16);
        let half = CARD_SIZE.xy() / 2.0;
        for c in &corners {
            assert!(
                c.x.abs() <= half.x + 1e-6 && c.y.abs() <= half.y + 1e-6,
                "{c}"
            );
        }
        // Its extremes are the card's sides.
        let widest = corners.iter().map(|c| c.x).fold(f32::MIN, f32::max);
        let deepest = corners.iter().map(|c| c.y).fold(f32::MIN, f32::max);
        assert!((widest - half.x).abs() < 1e-6 && (deepest - half.y).abs() < 1e-6);
        // Wound as a square cell's: from the near left, along the left side first.
        let cell = crate::slab::cell_corners(crate::board::CellShape::Square, 1.0);
        let turn = |p: &[Vec2]| (p[1] - p[0]).perp_dot(p[2] - p[1]).signum();
        assert_eq!(turn(&corners[2..5]), turn(&cell[..3]));
    }

    #[test]
    fn the_cards_lifted_are_the_top_of_their_pile() {
        assert_eq!(lifted_from(5, 2), 3..5);
        assert_eq!(lifted_from(1, 1), 0..1);
        assert_eq!(lifted_from(1, 3), 0..1);
    }

    #[test]
    fn a_card_is_known_by_its_id_once_its_face_is_seen() {
        let up = CardOnTable {
            id: "hearts-A".into(),
            up: true,
        };
        let down = CardOnTable::default();
        assert_eq!(key("t3", 4, &up), "hearts-A");
        assert_eq!(key("t3", 2, &down), "t3#2");
    }

    /// A game of three seats laid round the table by its look: a hand each, a trick, the
    /// trick before it, and what each seat captured.
    const SEATED: &str = r#"
        games = ["tricks"]
        [cards]
        play = "card-"
        [cards.piles.hand]
        seat = [0.0, 0.4]
        spread = "held"
        [cards.piles.taken]
        seat = [0.2, 0.3]
        spread = "row"
        [cards.piles.trick]
        spread = "played"
        [cards.piles.before]
        spread = "played"
        unless = "trick"
    "#;

    fn seated_look() -> Appearance {
        Appearance::parse(SEATED).unwrap()
    }

    /// That game as the server describes it to whoever holds its second seat, with the view's
    /// table `zones`.
    fn tricks(seq: u64, zones: serde_json::Value) -> Table {
        let mut table = table(seq, json!({}));
        table.info.id = "tricks".into();
        table.info.actions[0].kind = "move".into();
        table.info.seats = serde_json::from_value(json!([
            {"seat": 1, "color": "red", "label": "Fox"},
            {"seat": 2, "color": "blue", "label": "Owl"},
            {"seat": 3, "color": "green", "label": "Owlet"}
        ]))
        .unwrap();
        table.game.seats = serde_json::from_value(json!([
            {"seat": 1, "color": "red", "name": "Ana"},
            {"seat": 2, "color": "blue", "name": "Ben", "yours": true},
            {"seat": 3, "color": "green", "name": "Cy"}
        ]))
        .unwrap();
        table.state.state.ext = json!({"view": {"table": zones}});
        table
    }

    fn mid_trick() -> serde_json::Value {
        json!([
            {"id": "hand", "owner": 1, "count": 2, "cards": [{}, {}]},
            {"id": "hand", "owner": 2, "count": 2,
             "cards": [{"id": "hearts-A"}, {"id": "spades-2"}]},
            {"id": "hand", "owner": 3, "count": 1, "cards": [{}]},
            {"id": "trick", "count": 1, "cards": [{"id": "spades-K", "up": true}],
             "captions": ["Owlet"]},
            {"id": "before", "count": 1, "cards": [{"id": "spades-K#1", "up": true}],
             "captions": ["Fox ★"]}
        ])
    }

    #[test]
    fn a_caption_names_its_seat_by_the_seats_label_whatever_follows_it() {
        let table = tricks(1, json!([]));
        assert_eq!(caption_seat(&table.info, "Fox"), Some(0));
        assert_eq!(caption_seat(&table.info, "Owl ★"), Some(1));
        assert_eq!(
            caption_seat(&table.info, "Owlet"),
            Some(2),
            "the longest that fits"
        );
        assert_eq!(caption_seat(&table.info, "Hare"), None);
        assert_eq!(viewer(&table), 1, "seen from the seat this client plays");
    }

    #[test]
    fn a_click_on_a_card_makes_the_looks_choice_for_its_kind_when_the_move_may() {
        let cards = seated_look().cards.unwrap();
        let choices = [act("card-hearts-A"), act("side-2")];
        assert_eq!(
            card_choice(&cards, "hearts-A#2", &choices),
            Some(act("card-hearts-A")),
            "whichever of its copies it is"
        );
        assert_eq!(card_choice(&cards, "spades-2", &choices), None);
        assert_eq!(
            card_choice(&look().cards.unwrap(), "hearts-A", &choices),
            None
        );
    }

    #[test]
    fn the_trick_before_is_shown_until_the_next_is_led_and_its_cards_nowhere_else() {
        let look = seated_look().cards.unwrap();
        let layout = Layout::seated(
            &[
                ("hand", true),
                ("taken", true),
                ("trick", false),
                ("before", false),
            ]
            .map(|(zone, each_seat)| Placed {
                zone: zone.into(),
                at: Vec2::ZERO,
                spread: card_layout::Spread::Squared,
                each_seat,
            }),
            &[1, 2, 3],
            1,
            true,
        );
        // A trick just taken: it lies where it was played, the card captured with it.
        let taken = tricks(
            2,
            json!([
                {"id": "trick", "count": 0},
                {"id": "before", "count": 2,
                 "cards": [{"id": "spades-K", "up": true}, {"id": "hearts-A", "up": true}]},
                {"id": "taken", "owner": 3, "count": 1, "cards": [{"id": "hearts-A", "up": true}]}
            ]),
        );
        let lying = piles(&taken.state.state, &layout, Some(&look));
        assert_eq!(lying["before"].len(), 2);
        assert!(lying["taken:3"].is_empty(), "not drawn twice");
        // The next is led: the one before is gone, and the captured card is its captor's.
        let led = tricks(
            3,
            json!([
                {"id": "trick", "count": 1, "cards": [{"id": "spades-2", "up": true}]},
                {"id": "before", "count": 2,
                 "cards": [{"id": "spades-K", "up": true}, {"id": "hearts-A", "up": true}]},
                {"id": "taken", "owner": 3, "count": 1, "cards": [{"id": "hearts-A", "up": true}]}
            ]),
        );
        let lying = piles(&led.state.state, &layout, Some(&look));
        assert!(lying["before"].is_empty());
        assert_eq!(lying["taken:3"].len(), 1);
        assert_eq!(lying["trick"].len(), 1);
    }

    #[test]
    fn a_game_of_several_seats_is_laid_round_the_table_and_a_card_of_the_hand_is_played_by_a_click()
    {
        let mut app = app();
        // In a headset, where the hand stays in the world.
        app.insert_resource(Appearances(vec![seated_look()]))
            .insert_resource(InHeadset(true));
        let mut turn = Turn::new(1);
        turn.clicks_only = true;
        turn.options = vec![play_of(&["card-spades-2"])];
        turn.stage = Stage::Choosing;
        watch(&mut app, tricks(1, mid_trick()), Some(turn));
        app.update();
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&CardAt>().iter(world).count(), 6);
        assert_eq!(world.query::<&Place>().iter(world).count(), 0, "no places");
        // The player's own cards are known though they do not lie face up, and are theirs to
        // read: tilted up to the player's side.
        let ace = card(&mut app, "hearts-A");
        let two = card(&mut app, "spades-2");
        let at = app.world().get::<CardAt>(ace).unwrap().clone();
        assert!(at.known && !at.shown && at.zone == "hand:2");
        let face = app.world().get::<Transform>(ace).unwrap().rotation * Vec3::Y;
        assert!(face.y > 0.5 && face.z > 0.3, "{face}");
        // Another seat's stand in the air, and the trick's card lies towards who played it:
        // the third seat, on the player's left.
        let theirs = card(&mut app, "hand:1#0");
        let t = app.world().get::<Transform>(theirs).unwrap();
        assert!((t.rotation * Vec3::NEG_Z).y > 0.99, "upright");
        assert!(t.translation.y > 0.05);
        let king = card(&mut app, "spades-K");
        let k = app.world().get::<Transform>(king).unwrap().translation;
        assert!(k.x < -0.03 && k.x.abs() < 0.1, "towards the left: {k}");
        // The trick before it waits for the trick to be empty.
        let world = app.world_mut();
        let before = world
            .query::<&CardAt>()
            .iter(world)
            .filter(|c| c.zone == "before")
            .count();
        assert_eq!(before, 0);

        // The card that may be played stands out of the fan, the other does not.
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.update();
        }
        let out = |app: &App, card: Entity| {
            let part = app.world().get::<Parts>(card).unwrap().0[0];
            -app.world().get::<Transform>(part).unwrap().translation.z
        };
        assert!(out(&app, two) > PLAYABLE_OUT * 0.5, "{}", out(&app, two));
        assert!(out(&app, ace).abs() < 1e-6);
        let none = BTreeSet::new();
        let turn = app.world().resource::<WatchStatus>().turn.clone();
        let cards = seated_look().cards.unwrap();
        let feel_of = |key: &str| {
            feel(
                "hand:2",
                None,
                turn.as_ref(),
                Some(&cards),
                &none,
                Some(key),
            )
        };
        assert_eq!(feel_of("spades-2"), Feel::Point);
        assert_eq!(feel_of("hearts-A"), Feel::Open);
    }

    #[test]
    fn outside_a_headset_the_players_hand_is_the_strips_and_its_cards_come_and_go_by_the_near_edge()
    {
        let mut app = app();
        app.insert_resource(Appearances(vec![seated_look()]));
        let mut turn = Turn::new(1);
        turn.clicks_only = true;
        turn.options = vec![play_of(&["card-spades-2"])];
        turn.stage = Stage::Choosing;
        watch(&mut app, tricks(1, mid_trick()), Some(turn));
        app.update();
        app.update();
        // The hand, left to right as the position has it, the card that may be played with its
        // choice; its cards unseen at the near edge of the table.
        let strip = app.world().resource::<HandStrip>().0.clone();
        let keys: Vec<&str> = strip.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["hearts-A", "spades-2"]);
        assert_eq!(strip[0].choice, None);
        assert_eq!(strip[1].choice, Some(act("card-spades-2")));
        assert_eq!(
            strip[1].picture.as_deref(),
            Some("textures/cards/faces/spades_2_albedo.png")
        );
        let two = card(&mut app, "spades-2");
        let near = *app.world().get::<Transform>(two).unwrap();
        assert_eq!(
            app.world().get::<Visibility>(two),
            Some(&Visibility::Hidden)
        );
        assert!(
            near.translation.z > 0.3 && near.translation.x.abs() < 1e-6,
            "{near:?}"
        );
        assert!((near.rotation * Vec3::Y).y > 0.99, "face up");
        // Another seat's hand still stands in the air, and is seen.
        let theirs = card(&mut app, "hand:1#0");
        assert_eq!(
            app.world().get::<Visibility>(theirs),
            Some(&Visibility::Inherited)
        );

        // The two played: it leaves the strip and is carried from the near edge to the trick,
        // seen; a card dealt to the hand is carried in and joins the strip only once there.
        // (Passed from the first seat's hand: it comes from there.)
        let played = json!([
            {"id": "hand", "owner": 1, "count": 1, "cards": [{}]},
            {"id": "hand", "owner": 2, "count": 2,
             "cards": [{"id": "hearts-A"}, {"id": "clubs-5"}]},
            {"id": "hand", "owner": 3, "count": 1, "cards": [{}]},
            {"id": "trick", "count": 2,
             "cards": [{"id": "spades-K", "up": true}, {"id": "spades-2", "up": true}],
             "captions": ["Owlet", "Owl"]}
        ]);
        watch(&mut app, tricks(2, played), None);
        app.update();
        app.update();
        let strip = app.world().resource::<HandStrip>().0.clone();
        let keys: Vec<&str> = strip.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["hearts-A"], "the card dealt is still on its way");
        let world = app.world();
        assert_eq!(
            world.get::<Transform>(two),
            Some(&near),
            "sets off from there"
        );
        assert!(matches!(world.get::<Carry>(two), Some(Carry::Put(_))));
        assert_eq!(world.get::<Visibility>(two), Some(&Visibility::Inherited));
        let dealt = card(&mut app, "clubs-5");
        let world = app.world();
        let rest = world.get::<Rest>(dealt).unwrap().0;
        assert_eq!(world.get::<Carry>(dealt), Some(&Carry::Put(rest)));
        assert_eq!(world.get::<Visibility>(dealt), Some(&Visibility::Inherited));
        // Landed: it is the strip's.
        app.world_mut()
            .entity_mut(dealt)
            .insert(rest)
            .remove::<Carry>();
        app.update();
        let strip = app.world().resource::<HandStrip>().0.clone();
        assert_eq!(strip.len(), 2);
        assert_eq!(
            app.world().get::<Visibility>(dealt),
            Some(&Visibility::Hidden)
        );
    }

    /// A table that carries its cards and keeps time by a thirtieth of a second a frame.
    fn dealing_app() -> App {
        let mut app = app();
        app.add_plugins(crate::hand::HandPlugin).insert_resource(
            bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_nanos(
                1_000_000_000 / 30,
            )),
        );
        app
    }

    fn frames(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
    }

    /// Every card: what it is, where it is, where it rests, and whether it is still dealt.
    fn all_cards(app: &mut App) -> Vec<(CardAt, Transform, Transform, Option<Dealt>)> {
        let world = app.world_mut();
        world
            .query::<(&CardAt, &Transform, &Rest, Option<&Dealt>)>()
            .iter(world)
            .map(|(at, t, rest, dealt)| (at.clone(), *t, rest.0, dealt.copied()))
            .collect()
    }

    /// No card's face is up that the client may not know.
    fn nothing_shown_unknown(app: &mut App) {
        for (at, t, ..) in all_cards(app) {
            let up = (t.rotation * Vec3::Y).y > 0.2;
            assert!(!up || at.known, "{} shows its face", at.key);
        }
    }

    fn the_made_up_start(app: &mut App, cards: usize, in_headset: bool) {
        app.insert_resource(Appearances(vec![crate::test_hand::look()]))
            .insert_resource(InHeadset(in_headset));
        let table = Table::new(crate::test_hand::started(cards));
        watch(app, table, None);
        app.update();
        app.update();
    }

    #[test]
    fn a_game_seen_at_its_start_is_shuffled_face_down_then_dealt_round_the_seats() {
        let mut app = dealing_app();
        the_made_up_start(&mut app, 5, false);
        let dealing = app.world().resource::<Dealing>().clone();
        assert!(dealing.running);
        assert_eq!((dealing.cards, dealing.shuffle), (15, deal::SHUFFLE));
        // Every card in the deck, in the middle where the trick will be, face down.
        let cards = all_cards(&mut app);
        assert_eq!(cards.len(), 15);
        for (at, t, _, dealt) in &cards {
            assert!(dealt.is_some(), "{}", at.key);
            assert!(
                t.translation.xz().length() < CARD_SIZE.y,
                "{}: {t:?}",
                at.key
            );
            assert!((t.rotation * Vec3::Y).y < -0.9, "face down");
        }
        // Dealt round the seats one at a time from the game's first, and none stays.
        let nth = |zone: &str, from_top: usize| {
            cards
                .iter()
                .find(|(at, ..)| at.zone == zone && at.from_top == from_top)
                .and_then(|(.., d)| d.and_then(|d| d.nth))
        };
        assert_eq!(nth("hand:1", 5), Some(0));
        assert_eq!(nth("hand:2", 5), Some(1));
        assert_eq!(nth("hand:3", 5), Some(2));
        assert_eq!(nth("hand:1", 4), Some(3));
        assert_eq!(nth("hand:3", 1), Some(14));

        // Shuffled for a second: still in the deck, nothing set off.
        for _ in 0..28 {
            app.update();
            nothing_shown_unknown(&mut app);
        }
        assert!(all_cards(&mut app).iter().all(|(.., d)| d.is_some()));
        let world = app.world_mut();
        assert_eq!(world.query::<&Carry>().iter(world).count(), 0);
        // Then dealt, and over within two seconds more, every card where it rests.
        for _ in 0..62 {
            app.update();
            nothing_shown_unknown(&mut app);
        }
        assert!(!app.world().resource::<Dealing>().running);
        for (at, t, rest, dealt) in all_cards(&mut app) {
            assert_eq!(dealt, None);
            assert!(
                t.translation.distance(rest.translation) < 1e-5,
                "{}",
                at.key
            );
        }
        // The player's own joined the strip as they landed.
        assert_eq!(app.world().resource::<HandStrip>().0.len(), 5);

        // Laid again (into a headset and out), the start is not dealt twice.
        app.insert_resource(InHeadset(true));
        frames(&mut app, 2);
        assert!(!app.world().resource::<Dealing>().running);
        assert!(all_cards(&mut app).iter().all(|(.., d)| d.is_none()));
    }

    #[test]
    fn in_a_headset_the_players_hand_is_dealt_into_its_fan_in_the_world() {
        let mut app = dealing_app();
        the_made_up_start(&mut app, 4, true);
        assert!(app.world().resource::<Dealing>().running);
        frames(&mut app, 100);
        assert!(app.world().resource::<HandStrip>().0.is_empty());
        for (at, t, rest, _) in all_cards(&mut app) {
            assert!(
                t.translation.distance(rest.translation) < 1e-5,
                "{}",
                at.key
            );
        }
        // The player's fan is in front of them, face up to them, and seen.
        let own = card(
            &mut app,
            &crate::test_hand::started(4).state.state.card_zones()["hand:2"][0].id,
        );
        let t = app.world().get::<Transform>(own).unwrap();
        assert!(
            t.translation.z > 0.1 && (t.rotation * Vec3::Y).z > 0.2,
            "{t:?}"
        );
        assert_eq!(
            app.world().get::<Visibility>(own),
            Some(&Visibility::Inherited)
        );
    }

    #[test]
    fn a_press_ends_the_deal_at_once_and_does_nothing_else() {
        let mut app = dealing_app();
        let mut turn = Turn::new(1);
        turn.clicks_only = true;
        turn.options = vec![LegalSelection {
            notation: "t1-f".into(),
            action: "patience".into(),
            gestures: vec![act("from-t1-1"), act("to-f")],
            ..Default::default()
        }];
        turn.stage = Stage::Choosing;
        watch(&mut app, dealt(0), Some(turn));
        frames(&mut app, 10);
        assert!(app.world().resource::<Dealing>().running, "shuffling");
        // The stock is the deck: its cards stay, and the columns are dealt from it, the first
        // of each, then the second's second.
        let cards = all_cards(&mut app);
        let nth = |key: &str| {
            cards
                .iter()
                .find(|(at, ..)| at.key == key)
                .and_then(|(.., d)| d.unwrap().nth)
        };
        assert_eq!(nth("stock#0"), None);
        assert_eq!(nth("hearts-A"), Some(0));
        assert_eq!(nth("t2#0"), Some(1));
        assert_eq!(nth("spades-K"), Some(2));

        // Pressed on the ace while it shuffles: no drag begins, and every card is where it rests.
        let ace = card(&mut app, "hearts-A");
        app.world_mut().trigger(PointerPress {
            entity: ace,
            pointer: Pointer::new(PointerId::Mouse, nowhere()),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            count: 1,
        });
        app.world_mut().write_message(PointerInput::new(
            PointerId::Mouse,
            nowhere(),
            PointerAction::Press(PointerButton::Primary),
        ));
        app.update();
        assert!(!app.world().resource::<Drag>().pressed(), "no drag");
        let dealing = app.world().resource::<Dealing>().clone();
        assert!(!dealing.running && dealing.holds_clicks());
        for (at, t, rest, dealt) in all_cards(&mut app) {
            assert_eq!(dealt, None);
            assert_eq!(t, rest, "{}", at.key);
        }
        let world = app.world_mut();
        assert_eq!(world.query::<&Carry>().iter(world).count(), 0);
        // Let go: the click was the deal's, and the next is the game's.
        app.world_mut().write_message(PointerInput::new(
            PointerId::Mouse,
            nowhere(),
            PointerAction::Release(PointerButton::Primary),
        ));
        app.update();
        assert!(!app.world().resource::<Dealing>().holds_clicks());
        let turn = app.world().resource::<WatchStatus>().turn.clone().unwrap();
        assert!(turn.gestures.is_empty(), "nothing lifted");
    }

    #[test]
    fn the_next_position_ends_a_deal_and_a_game_opened_part_way_is_not_dealt() {
        let mut app = dealing_app();
        watch(&mut app, dealt(0), None);
        frames(&mut app, 40);
        assert!(app.world().resource::<Dealing>().running, "dealing");
        watch(&mut app, dealt(1), None);
        app.update();
        assert!(!app.world().resource::<Dealing>().running);
        for (at, t, rest, dealt) in all_cards(&mut app) {
            assert_eq!(dealt, None);
            assert_eq!(t, rest, "{}", at.key);
        }

        let mut app = dealing_app();
        watch(&mut app, dealt(3), None);
        frames(&mut app, 2);
        assert!(!app.world().resource::<Dealing>().running);
        assert!(all_cards(&mut app).iter().all(|(.., d)| d.is_none()));
    }

    #[test]
    fn a_row_dealt_from_the_stock_by_a_move_goes_round_the_columns_silently() {
        let mut app = dealing_app();
        let before = json!({
            "stock": {"cards": [{}, {}, {}, {}]},
            "t1": {"cards": [{"id": "hearts-A", "up": true}]},
            "t2": {"cards": [{}, {"id": "spades-K", "up": true}]}
        });
        watch(&mut app, table(4, before), None);
        frames(&mut app, 2);
        let after = json!({
            "stock": {"cards": [{}]},
            "t1": {"cards": [{"id": "hearts-A", "up": true}, {"id": "spades-2", "up": true}, {}]},
            "t2": {"cards": [{}, {"id": "spades-K", "up": true}, {}]}
        });
        watch(&mut app, table(5, after), None);
        app.update();
        let dealing = app.world().resource::<Dealing>().clone();
        assert!(dealing.running && dealing.shuffle == 0.0);
        assert_eq!(dealing.cards, 3);
        let world = app.world_mut();
        let mut dealt: Vec<(String, usize, Route)> = world
            .query::<(&CardAt, &Dealt, &Route)>()
            .iter(world)
            .map(|(at, d, r)| (at.zone.clone(), d.nth.unwrap(), *r))
            .collect();
        dealt.sort_by_key(|(_, k, _)| *k);
        let zones: Vec<&str> = dealt.iter().map(|(z, ..)| z.as_str()).collect();
        assert_eq!(
            zones,
            ["t1", "t2", "t1"],
            "round the columns, one at a time"
        );
        assert!(dealt.iter().all(|(.., r)| r.lands.is_none()), "silent");
        assert!(dealt[1].2.delay > dealt[0].2.delay);
        frames(&mut app, 40);
        assert!(!app.world().resource::<Dealing>().running);

        // One card drawn is no deal.
        let drawn = json!({
            "stock": {},
            "waste": {"cards": [{"id": "hearts-A", "up": true}]},
            "t1": {"cards": [{"id": "spades-2", "up": true}]}
        });
        watch(&mut app, table(6, drawn), None);
        app.update();
        assert!(!app.world().resource::<Dealing>().running);
    }

    #[test]
    fn the_deck_is_the_looks_pile_else_the_stock_else_an_empty_pile_else_the_middle() {
        let layout = patience::layout([("stock", 3), ("waste", 0), ("t1", 1), ("c1", 0)]);
        let piles: BTreeMap<String, Vec<CardOnTable>> =
            [("t1".to_string(), vec![CardOnTable::default()])].into();
        let at = |z: &str| layout.piles[z].at;
        assert_eq!(deck_of(&layout, &piles, None).1.as_deref(), Some("stock"));
        assert_eq!(deck_of(&layout, &piles, None).0.at, at("stock"));
        assert_eq!(
            deck_of(&layout, &piles, Some("c1")).1.as_deref(),
            Some("c1")
        );
        let no_stock = patience::layout([("t1", 1), ("c1", 0), ("c2", 0)]);
        assert_eq!(deck_of(&no_stock, &piles, None).1.as_deref(), Some("c1"));
        let full = Layout::placed([(
            "t1".to_string(),
            Vec2::new(0.2, 0.0),
            card_layout::Spread::Down,
        )]);
        let (deck, zone) = deck_of(&full, &piles, Some("nowhere"));
        assert_eq!((deck, zone), (Deck::default(), None));
    }
}
