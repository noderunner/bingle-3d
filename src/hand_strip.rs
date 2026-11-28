//! The player's own hand of cards, drawn flat along the bottom of the window instead of in the
//! world, in a card game laid round its seats (`card_table.rs` fills [`HandStrip`]; not in a
//! headset, where the hand stays in the world). Each card is the picture on its face; a card
//! that may be played is a button, and a click on it makes its choice (`[cards] play`), as a
//! click on the hand in the world does. While not every card may be played, those that may stand
//! a little up out of the strip, and the card under the pointer stands further, over its
//! neighbours, so it can be read.
//!
//! The strip lies between the game's panel and the rows of buttons at the bottom right
//! (`hud::BottomRight`), above the line of words along the bottom; a long hand overlaps as far as
//! it must to fit there (`place`). A window too narrow for that has it above those rows. Only
//! the cards take the pointer: beside a short hand, the table behind is clicked.

use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};
use bevy::window::PrimaryWindow;

use crate::game_panel;
use crate::hud::{BOTTOM_LINE, BottomRight};
use crate::net::api::Gesture;
use crate::net::{Net, WatchStatus};
use crate::props::CARD_SIZE;

pub struct HandStripPlugin;

impl Plugin for HandStripPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HandStrip>()
            .add_systems(Startup, spawn_strip)
            .add_systems(
                Update,
                (
                    rebuild.run_if(resource_changed::<HandStrip>),
                    lay_out,
                    raise,
                )
                    .chain(),
            );
    }
}

/// The cards of the player's own hand drawn along the bottom, left to right; empty while there
/// is no such hand.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct HandStrip(pub Vec<StripCard>);

/// A card of the strip: what the table calls it, the picture on its face (none for a card the
/// client has no picture of: plain paper), and the choice a click on it makes now, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct StripCard {
    pub key: String,
    pub picture: Option<String>,
    pub choice: Option<Gesture>,
}

/// How tall a card of the strip is, and how wide, in logical pixels.
pub const CARD_HIGH: f32 = 124.0;
pub const CARD_WIDE: f32 = CARD_HIGH * CARD_SIZE.x / CARD_SIZE.y;
/// The space between two cards of a short hand.
const SPACE: f32 = 6.0;
/// The least of a card that shows beside the next, however long the hand.
const LEAST_STEP: f32 = 8.0;
/// Between the strip and what it keeps clear of; from the window's edge.
const GAP: f32 = 16.0;
const EDGE: f32 = 12.0;
/// How far a card that may be played stands up, while not all may; and the one under the
/// pointer.
pub const PLAYABLE_RAISE: f32 = 12.0;
pub const POINTED_RAISE: f32 = 30.0;
/// The paper of a card the client has no picture of, and the line round each card.
const PAPER: Color = Color::srgb(0.95, 0.94, 0.90);
const RIM: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);

/// Where the strip lies in a window `width` wide (logical pixels), given the rows at the bottom
/// right (each its width and how far above the window's bottom its top is), for `cards` cards:
/// from the left and the right of the window, above its bottom, and how far each card is from
/// the one before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placing {
    pub left: f32,
    pub right: f32,
    pub bottom: f32,
    pub step: f32,
}

pub fn place(width: f32, rows: &[(f32, f32)], cards: usize) -> Placing {
    let left = game_panel::MARGIN + game_panel::WIDTH + GAP;
    let widest = rows.iter().map(|r| r.0).fold(0.0, f32::max);
    let beside = EDGE + widest + GAP;
    let (right, bottom) = if width - left - beside >= 2.0 * CARD_WIDE {
        (beside, BOTTOM_LINE)
    } else {
        // Too narrow beside them: above them.
        let top = rows.iter().map(|r| r.1).fold(BOTTOM_LINE, f32::max);
        (EDGE, top + GAP / 2.0)
    };
    let room = width - left - right;
    let step = if cards > 1 {
        ((room - CARD_WIDE) / (cards - 1) as f32).clamp(LEAST_STEP, CARD_WIDE + SPACE)
    } else {
        0.0
    };
    Placing {
        left,
        right,
        bottom,
        step,
    }
}

/// How far a card stands up out of the strip: one that may be played, while some may not; the
/// one under the pointer, so it is read whole.
pub fn raised(may: bool, mixed: bool, pointed: bool) -> f32 {
    if pointed {
        POINTED_RAISE
    } else if may && mixed {
        PLAYABLE_RAISE
    } else {
        0.0
    }
}

/// The strip's row.
#[derive(Component)]
struct Strip;

/// A card of the strip: its place in it, and the choice a click on it makes.
#[derive(Component, Debug, Clone)]
struct Slot {
    index: usize,
    choice: Option<Gesture>,
}

/// The picture of a card of the strip, which stands up out of it.
#[derive(Component)]
struct Face;

fn spawn_strip(mut commands: Commands) {
    commands.spawn((
        Name::new("Hand strip"),
        Strip,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            height: px(CARD_HIGH),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::FlexEnd,
            ..default()
        },
        // Only its cards: between and beside them the table is clicked.
        Pickable::IGNORE,
    ));
}

/// The strip's cards as the hand now is.
fn rebuild(
    mut commands: Commands,
    hand: Res<HandStrip>,
    server: Res<AssetServer>,
    mut strips: Query<(Entity, &mut Node), With<Strip>>,
) {
    for (strip, mut node) in &mut strips {
        commands.entity(strip).despawn_children();
        let display = if hand.0.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
        for (index, card) in hand.0.iter().enumerate() {
            let face = match &card.picture {
                Some(file) => (
                    ImageNode::new(crate::textures::load(&server, file)),
                    BackgroundColor(Color::NONE),
                ),
                None => (ImageNode::default(), BackgroundColor(PAPER)),
            };
            let mut slot = commands.spawn((
                Name::new(format!("Hand strip {}", card.key)),
                Slot {
                    index,
                    choice: card.choice.clone(),
                },
                Hovered::default(),
                Node {
                    width: px(CARD_WIDE),
                    height: px(CARD_HIGH),
                    flex_shrink: 0.0,
                    ..default()
                },
                ChildOf(strip),
                children![(
                    Face,
                    face,
                    Node {
                        width: percent(100),
                        height: percent(100),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    Outline::new(px(1), px(0), RIM),
                    // Its slot, which does not move, takes the pointer.
                    Pickable::IGNORE,
                )],
            ));
            if card.choice.is_some() {
                slot.insert((Button, observe(play_card)));
            }
        }
    }
}

/// The rows at the bottom right, which the strip keeps clear of.
type Rows = (With<BottomRight>, Without<Strip>, Without<Slot>);

/// Lays the strip between the game's panel and the rows at the bottom right, each card as far
/// from the one before it as there is room for.
fn lay_out(
    windows: Query<&Window, With<PrimaryWindow>>,
    rows: Query<(&Node, &ComputedNode), Rows>,
    mut strips: Query<&mut Node, (With<Strip>, Without<Slot>)>,
    mut slots: Query<(&Slot, &mut Node), Without<Strip>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let rows: Vec<(f32, f32)> = rows
        .iter()
        .filter(|(node, _)| node.display != Display::None)
        .map(|(node, computed)| {
            let size = computed.size() * computed.inverse_scale_factor();
            let bottom = match node.bottom {
                Val::Px(b) => b,
                _ => 0.0,
            };
            (size.x, bottom + size.y)
        })
        .collect();
    let count = slots.iter().count();
    let at = place(window.width(), &rows, count);
    for mut node in &mut strips {
        let wanted = (px(at.left), px(at.right), px(at.bottom));
        if (node.left, node.right, node.bottom) != wanted {
            (node.left, node.right, node.bottom) = wanted;
        }
    }
    for (slot, mut node) in &mut slots {
        let margin = if slot.index == 0 {
            px(0)
        } else {
            px(at.step - CARD_WIDE)
        };
        if node.margin.left != margin {
            node.margin.left = margin;
        }
    }
}

/// Each card stands up as far as it should, the one under the pointer over its neighbours.
fn raise(
    mut commands: Commands,
    slots: Query<(&Slot, &Hovered, &Children)>,
    mut faces: Query<(&mut Node, Has<GlobalZIndex>), With<Face>>,
) {
    let may: Vec<bool> = slots.iter().map(|(s, ..)| s.choice.is_some()).collect();
    let mixed = may.contains(&true) && may.contains(&false);
    for (slot, hovered, children) in &slots {
        let up = raised(slot.choice.is_some(), mixed, hovered.0);
        for child in children {
            let Ok((mut node, on_top)) = faces.get_mut(*child) else {
                continue;
            };
            if node.top != px(-up) {
                node.top = px(-up);
            }
            match (hovered.0, on_top) {
                (true, false) => {
                    commands.entity(*child).insert(GlobalZIndex(1));
                }
                (false, true) => {
                    commands.entity(*child).remove::<GlobalZIndex>();
                }
                _ => {}
            }
        }
    }
}

/// A click on a card that may be played makes its choice.
fn play_card(
    activate: On<Activate>,
    slots: Query<&Slot>,
    net: Res<Net>,
    mut watch: ResMut<WatchStatus>,
) {
    if let Some(choice) = slots
        .get(activate.entity)
        .ok()
        .and_then(|s| s.choice.as_ref())
    {
        crate::play::gesture(&net, &mut watch, choice);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::{Stage, Turn};

    #[test]
    fn a_long_hand_overlaps_as_far_as_it_must_between_the_panel_and_the_buttons() {
        let rows = [(380.0, 60.0), (300.0, 92.0), (240.0, 126.0)];
        let short = place(1280.0, &rows, 5);
        let left = game_panel::MARGIN + game_panel::WIDTH;
        assert!(short.left > left, "clear of the game's panel");
        assert!(short.right > 380.0 + EDGE, "clear of the buttons");
        assert_eq!(short.bottom, BOTTOM_LINE, "above the line of words");
        assert!(short.step > CARD_WIDE, "a short hand side by side");
        let long = place(1280.0, &rows, 13);
        let room = 1280.0 - long.left - long.right;
        assert!(long.step < CARD_WIDE, "overlapping");
        assert!(
            (CARD_WIDE + 12.0 * long.step - room).abs() < 1e-3,
            "as wide as the room there is"
        );
        // Each card's corner still shows, however long the hand.
        assert_eq!(place(1280.0, &rows, 80).step, LEAST_STEP);
        // Too narrow beside the buttons: above them, the whole width beyond the panel.
        let narrow = place(800.0, &rows, 13);
        assert_eq!(narrow.right, EDGE);
        assert!(narrow.bottom > 126.0);
        assert_eq!(place(1280.0, &rows, 1).step, 0.0);
    }

    #[test]
    fn the_card_under_the_pointer_stands_up_furthest_and_those_that_may_be_played_while_some_may_not()
     {
        assert_eq!(raised(false, true, true), POINTED_RAISE);
        assert_eq!(raised(true, true, false), PLAYABLE_RAISE);
        assert_eq!(raised(true, false, false), 0.0, "all may: none stands out");
        assert_eq!(raised(false, true, false), 0.0);
    }

    fn act(value: &str) -> Gesture {
        Gesture {
            kind: "action".into(),
            value: value.into(),
        }
    }

    #[test]
    fn a_card_is_drawn_for_each_of_the_hand_and_a_click_on_one_that_may_be_played_plays_it() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), HandStripPlugin))
            .init_asset::<Image>()
            .insert_resource(Net::for_test().0)
            .init_resource::<WatchStatus>();
        let mut turn = Turn::new(1);
        turn.clicks_only = true;
        turn.options = vec![crate::net::api::LegalSelection {
            notation: "2".into(),
            action: "move".into(),
            gestures: vec![act("card-spades-2")],
            ..Default::default()
        }];
        turn.stage = Stage::Choosing;
        app.world_mut().resource_mut::<WatchStatus>().turn = Some(turn);
        app.update();
        let strip = |app: &mut App| {
            let world = app.world_mut();
            world
                .query_filtered::<&Node, With<Strip>>()
                .single(world)
                .unwrap()
                .display
        };
        assert_eq!(strip(&mut app), Display::None, "no hand, no strip");
        app.insert_resource(HandStrip(vec![
            StripCard {
                key: "hearts-A".into(),
                picture: None,
                choice: None,
            },
            StripCard {
                key: "spades-2".into(),
                picture: Some("textures/cards/faces/spades_2_albedo.png".into()),
                choice: Some(act("card-spades-2")),
            },
        ]));
        app.update();
        assert_eq!(strip(&mut app), Display::Flex);
        let world = app.world_mut();
        let mut slots = world.query::<(Entity, &Slot, Has<Button>)>();
        let mut all: Vec<_> = slots.iter(world).map(|(e, s, b)| (e, s.index, b)).collect();
        all.sort_by_key(|s| s.1);
        assert_eq!(all.len(), 2);
        assert!(!all[0].2, "one that may not be played is no button");
        assert!(all[1].2);
        // The click: the choice is made.
        app.world_mut().trigger(Activate { entity: all[1].0 });
        app.update();
        let turn = app.world().resource::<WatchStatus>().turn.clone().unwrap();
        assert_ne!(turn.stage, Stage::Choosing, "played");
    }
}
