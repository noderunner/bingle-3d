//! A patience's cards on the table, in place of its board: each pile where `patience.rs` lays
//! it, on a cloth, its cards as the position has them (a card whose face this client may not
//! see lies face down).
//!
//! A click on a card a move starts with lifts it, and the cards on it: they hover under the
//! pointer, bobbing gently, the pointer's own arrow hidden while they take its place
//! (`hand::Held`). Each pile they may go on glows, and the one under the pointer shows them
//! see-through where they would land. A click there puts them down and plays the move at once
//! (`Turn::clicks_only`); a click on the pile they came from, on the bare cloth, a right-click
//! or Backspace puts them back. A click on the stock draws, turns the waste over or deals,
//! whichever the game offers.
//!
//! Cards go where the position puts them by the hand's own carrying (`hand::Carry::Put`): over
//! the table and down, one after another when several go at once (a row dealt, a run off the
//! table), and a card turned up on its way when it was face down where it came from.

use std::collections::{BTreeMap, HashMap, HashSet};

use bevy::light::NotShadowCaster;
use bevy::picking::Pickable;
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;

use crate::assets;
use crate::board::BoardBuilt;
use crate::hand::{BoardEdge, Carry, Hands, Held, Rest, Route, Surface};
use crate::highlight::{Lit, Outline};
use crate::look::{self, MaterialSpec};
use crate::net::api::{CardOnTable, Cards};
use crate::net::{Net, WatchStatus};
use crate::patience::{self, Click, Layout};
use crate::play::{self, Stage, Turn};
use crate::props::{self, CARD, CARD_SIZE};
use crate::sounds::Cue;
use crate::table::Table;

pub struct CardTablePlugin;

impl Plugin for CardTablePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Aimed>()
            .init_resource::<PutOn>()
            .add_message::<BoardBuilt>()
            .add_message::<Cue>()
            .add_observer(on_click)
            .add_observer(on_over)
            .add_observer(on_out)
            .add_systems(
                Update,
                (
                    sync_cards.run_if(resource_changed::<WatchStatus>),
                    show_aim.run_if(
                        resource_changed::<WatchStatus>.or_eager(resource_changed::<Aimed>),
                    ),
                    follow_hand,
                )
                    .chain()
                    .before(Hands),
            );
    }
}

/// The cloth the cards lie on: how thick, and how far it reaches beyond the piles.
const CLOTH: f32 = 0.002;
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
/// How much higher a card goes on its way than the hand carries a piece, so it clears the
/// piles it passes over (and turns over in the air).
const OVER: f32 = 0.02;
/// A card's corners, rounded as the model's are, for its glow.
const CORNER: f32 = 0.0032;

/// The pile the pointer is over, if any: its cards, or its place.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct Aimed(pub Option<String>);

/// The pile a click put the cards in hand on, while that move is sent: they are shown there.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
struct PutOn(Option<String>);

/// The cards of the game watched: where its piles lie, and the materials its cards are drawn in.
#[derive(Component)]
struct CardRoot {
    room_code: String,
    layout: Layout,
    looks: Looks,
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

/// What the cards in hand follow: `hand::Held` while there are any, under the pointer.
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

/// Each pile's cards, bottom first, by zone: the position's, for the piles the layout has.
fn piles<'t>(table: &'t Table, layout: &Layout) -> BTreeMap<&'t str, &'t [CardOnTable]> {
    table
        .state
        .state
        .cards
        .zones
        .iter()
        .filter(|(zone, _)| layout.piles.contains_key(zone.as_str()))
        .map(|(zone, z)| (zone.as_str(), z.cards.as_slice()))
        .collect()
}

/// Which way up each card of a pile lies.
fn ups(cards: &[CardOnTable]) -> Vec<bool> {
    cards.iter().map(|c| c.up).collect()
}

/// What a card is called in the scene: its id, or for one whose face is not seen, its zone and
/// place (which changes when it is turned up, so it is drawn again with its face).
fn key(zone: &str, index: usize, card: &CardOnTable) -> String {
    if card.shown() {
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
    let pile = &table.state.state.cards.zones.get(zone)?.cards;
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
);

#[allow(clippy::too_many_arguments)]
fn sync_cards(
    mut commands: Commands,
    watch: Res<WatchStatus>,
    server: Res<AssetServer>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut roots: Query<(Entity, &mut CardRoot)>,
    cards: Query<Existing>,
    anchors: Query<(Entity, Has<Held>), With<Anchor>>,
    put_on: Res<PutOn>,
    (mut built, mut cues): (MessageWriter<BoardBuilt>, MessageWriter<Cue>),
) {
    let Some(table) = watch.table.as_ref().filter(|t| patience::plays(&t.info)) else {
        for (root, _) in &roots {
            commands.entity(root).despawn();
        }
        return;
    };
    let current = roots
        .iter_mut()
        .find(|(_, r)| r.room_code == table.room_code);
    let Some((root, mut shown)) = current else {
        for (old, _) in &roots {
            commands.entity(old).despawn();
        }
        let fresh = build(
            &mut commands,
            table,
            &server,
            &mut meshes,
            &mut materials,
            &mut built,
        );
        return place_fresh(&mut commands, table, fresh, &server, &mut materials);
    };
    let root_data = &mut *shown;
    let layout = root_data.layout.clone();
    let info_cards = table.info.cards.as_ref();
    let piles = piles(table, &layout);
    let turn = watch.turn.as_ref();
    let held = in_hand(turn);
    let put = put_down(turn).zip(put_on.0.clone());

    let existing: HashMap<&str, Existing> = cards.iter().map(|c| (c.1.key.as_str(), c)).collect();
    let mut wanted: HashSet<String> = HashSet::new();
    let mut order: Vec<(&str, &[CardOnTable])> = piles.iter().map(|(z, c)| (*z, *c)).collect();
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
    let mut gone: Vec<(usize, Transform)> = Vec::new();
    for (entity, at, t, ..) in existing.values() {
        if wanted.contains(&at.key) {
            continue;
        }
        let index = piles
            .get(at.zone.as_str())
            .map_or(0, |p| p.len())
            .saturating_add(at.from_top)
            .saturating_sub(1);
        gone_at.insert((at.zone.clone(), index), **t);
        gone.push((usize::MAX - index, **t));
        commands.entity(*entity).despawn();
    }
    gone.sort_by_key(|(order, _)| *order);
    let mut gone = gone.into_iter().map(|(_, t)| t);

    let mut setting_off = 0usize;
    let mut route = || {
        let delay = (setting_off as f32 * STAGGER).min(STAGGER_MAX);
        setting_off += 1;
        Route {
            delay,
            over: OVER,
            ..Route::default()
        }
    };
    let mut lifting = false;
    for (zone, pile) in &order {
        let up = ups(pile);
        let rests = layout.lie(zone, &up, BASE);
        let lifted = held
            .as_ref()
            .filter(|(z, _)| z == zone)
            .map(|(_, n)| lifted_from(pile.len(), *n));
        let putting = put
            .as_ref()
            .filter(|((z, _), _)| z == zone)
            .map(|((_, n), onto)| {
                let onto_up = piles.get(onto.as_str()).map(|p| ups(p)).unwrap_or_default();
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
                Some((entity, was, t, old_rest, carry, hand, parts)) => {
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
                    let goal = put_at.unwrap_or(rest);
                    let going = matches!(carry, Some(Carry::Put(to)) if *to == goal);
                    if going {
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
                    commands.entity(*entity).insert((Carry::Put(goal), route()));
                }
                None => {
                    // From where a card went, turned as it was there; else where it lies.
                    let from = gone_at
                        .remove(&(zone.to_string(), i))
                        .or_else(|| gone.next());
                    let whole = spawn_card(
                        &mut commands,
                        root,
                        &server,
                        &mut materials,
                        &mut root_data.looks,
                        info_cards,
                        card,
                        from.unwrap_or(rest),
                    );
                    commands
                        .entity(whole)
                        .insert((at, PileOf(zone.to_string()), Rest(rest)));
                    if from.is_some() {
                        commands.entity(whole).insert((Carry::Put(rest), route()));
                    }
                }
            }
        }
    }
    if lifting {
        cues.write(Cue::Lift);
    }
    for (anchor, holding) in &anchors {
        match (held.is_some(), holding) {
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

/// Lays the table for a game just opened: the cloth, each pile's place and glow, and the cards
/// as they lie, none of them dealt.
fn place_fresh(
    commands: &mut Commands,
    table: &Table,
    (root, mut data): (Entity, CardRoot),
    server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
) {
    let layout = data.layout.clone();
    for (zone, pile) in piles(table, &layout) {
        let rests = layout.lie(zone, &ups(pile), BASE);
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
                    zone: zone.to_string(),
                    from_top: pile.len() - i,
                    shown: card.shown(),
                },
                PileOf(zone.to_string()),
                Rest(rests[i]),
            ));
        }
    }
    commands.entity(root).insert(data);
}

/// The cloth, the piles' places and glows, and what the cards in hand follow, for the camera to
/// frame.
fn build(
    commands: &mut Commands,
    table: &Table,
    server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    built: &mut MessageWriter<BoardBuilt>,
) -> (Entity, CardRoot) {
    let zones = table
        .state
        .state
        .cards
        .zones
        .iter()
        .map(|(zone, z)| (zone.as_str(), z.cards.len()));
    let layout = Layout::new(zones);
    let root = commands
        .spawn((
            Name::new("Cards"),
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    let cloth_size = layout.size + Vec2::splat(2.0 * CLOTH_MARGIN);
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
        Mesh3d(meshes.add(Cuboid::new(cloth_size.x, CLOTH, cloth_size.y))),
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
    for (zone, pile) in &layout.piles {
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
    commands.insert_resource(BoardEdge(vec![
        Vec2::new(-half.x, -half.y),
        Vec2::new(-half.x, half.y),
        Vec2::new(half.x, half.y),
        Vec2::new(half.x, -half.y),
    ]));
    // Framed as dealt, with room for the columns to grow by half their reach: a long one
    // reaches beyond the picture rather than every deal being seen from afar.
    let unused = patience::REACH / 2.0;
    built.write(BoardBuilt {
        size: cloth_size - Vec2::Y * unused,
        middle: Vec2::Y * -unused / 2.0,
    });
    let looks = Looks::new(server, materials);
    (
        root,
        CardRoot {
            room_code: table.room_code.clone(),
            layout,
            looks,
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
    let face = if card.shown() {
        looks.face(cards, &card.id, server, materials)
    } else {
        looks.paper.clone()
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
    let piles = piles(table, &shown.layout);
    let home = held.as_ref().and_then(|h| home_of(table, h));
    let targets: Vec<&str> = patience::targets(
        shown.layout.piles.keys().map(String::as_str),
        &next,
        home.as_deref(),
    );
    let landing = |zone: &str, n: usize| {
        let up = piles.get(zone).map(|p| ups(p)).unwrap_or_default();
        shown.layout.landing(zone, &up, n, BASE)
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
    let Some(lifted) = piles.get(from.as_str()) else {
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

/// The cards in hand follow it: under the pointer, each beyond the one under it, or lifted
/// over where they lie until the pointer is known.
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
    net: Res<Net>,
    mut watch: ResMut<WatchStatus>,
    mut put_on: ResMut<PutOn>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let (zone, from_top) = if let Ok(card) = cards.get(click.entity) {
        (card.zone.clone(), card.shown.then_some(card.from_top))
    } else if let Ok(pile) = places.get(click.entity) {
        (pile.0.clone(), None)
    } else if cloths.contains(click.entity) {
        // The bare cloth: what is in hand goes back.
        if in_hand(watch.turn.as_ref()).is_some() {
            play::back(&mut watch);
        }
        return;
    } else {
        return;
    };
    let Some(turn) = watch.turn.as_ref().filter(|t| t.stage == Stage::Choosing) else {
        return;
    };
    let lifted = patience::lifted(&turn.gestures);
    let next = turn.choices();
    let held = lifted.as_ref().map(|(z, n)| (z.as_str(), *n));
    match patience::click(&zone, from_top, held, &next, &turn.options) {
        Click::Gesture(g) => {
            if g.value.starts_with("to-") {
                // Sent home, it is shown going to its suit's foundation, when that is known.
                let home = (g.value == "to-f")
                    .then(|| {
                        let table = watch.table.as_ref()?;
                        home_of(table, lifted.as_ref()?)
                    })
                    .flatten();
                put_on.0 = Some(home.unwrap_or(zone));
            }
            play::gesture(&net, &mut watch, &g);
        }
        Click::PutBack => play::back(&mut watch),
        Click::Nothing => {}
    }
}

fn on_over(over: On<PointerOver>, piles: Query<&PileOf>, mut aimed: ResMut<Aimed>) {
    if let Ok(pile) = piles.get(over.entity)
        && aimed.0.as_ref() != Some(&pile.0)
    {
        aimed.0 = Some(pile.0.clone());
    }
}

fn on_out(out: On<PointerOut>, piles: Query<&PileOf>, mut aimed: ResMut<Aimed>) {
    if let Ok(pile) = piles.get(out.entity)
        && aimed.0.as_ref() == Some(&pile.0)
    {
        aimed.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::session::Opened;
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
    fn a_patience_is_laid_as_it_lies_its_cards_lifted_into_the_hand_and_carried_where_they_go() {
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

        // The ace lifted: it is in hand, and the hand follows the pointer.
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
        assert_eq!(app.world().get::<InHand>(ace), Some(&InHand(0)));
        let world = app.world_mut();
        let held = world
            .query_filtered::<(), (With<Anchor>, With<Held>)>()
            .iter(world)
            .count();
        assert_eq!(held, 1);
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
        let layout = Layout::new([("stock", 3), ("waste", 0), ("f1", 0), ("t1", 0), ("t2", 0)]);
        assert_eq!(rest, layout.lie("f1", &[true], BASE)[0]);
        let world = app.world_mut();
        let held = world
            .query_filtered::<(), (With<Anchor>, With<Held>)>()
            .iter(world)
            .count();
        assert_eq!(held, 0, "nothing in hand");
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
}
