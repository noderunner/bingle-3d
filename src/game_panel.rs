//! The panel beside the game being watched: what it is, who plays which seat, whose impulse it is
//! or how it ended, and the way back to the list. In a game this client plays, it also says what
//! the player may do on their impulse and holds the buttons for it: choices that are not a click
//! on the board (an action such as swapping sides), and playing the move shown or going back. It is
//! rebuilt whenever [`WatchStatus`] changes.
//!
//! It also starts the watch that `--watch` or `--watch-bots` asked for, once the server is there.

use std::time::Instant;

use bevy::prelude::*;

use crate::appearance::Appearances;
use crate::browser::{Action, button, buttons, colors, dotted_button, line, said, seat_dots};
use crate::clock;
use crate::downloads::Downloads;
use crate::fonts::UiFonts;
use crate::lobby;
use crate::net::api::{GameInfo, Gesture};
use crate::net::{Connection, Net, Resign, ServerStatus, Target, WatchStatus, watch};
use crate::play::{Stage, Turn};
use crate::table::{Said, Table};
use crate::voice_chat::{self, VoiceChat};

pub struct GamePanelPlugin {
    /// What to watch as soon as the server is connected.
    pub on_start: Option<Target>,
}

impl Plugin for GamePanelPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WatchOnStart(self.on_start.clone()))
            .add_systems(Startup, spawn_panel)
            .add_systems(
                Update,
                (
                    watch_on_start.run_if(resource_changed::<ServerStatus>),
                    rebuild.run_if(
                        resource_changed::<WatchStatus>
                            .or_eager(resource_changed::<VoiceChat>)
                            .or_eager(resource_changed::<Downloads>),
                    ),
                    tick_clocks,
                )
                    .chain(),
            );
    }
}

#[derive(Resource)]
struct WatchOnStart(Option<Target>);

/// A game waiting for the look the server offers: how far it has been fetched, and the way to
/// play on without it.
fn looking(p: &mut ChildSpawnerCommands, f: &UiFonts, (done, total): (u64, u64)) {
    p.spawn(line(
        f.semibold(16.0),
        colors::TEXT,
        "Fetching the game's look…",
    ));
    if total > 0 {
        let mb = |bytes: u64| bytes as f64 / 1e6;
        p.spawn(line(
            f.regular(13.0),
            colors::MUTED,
            &format!("{:.1} of {:.1} MB", mb(done), mb(total)),
        ));
    }
    p.spawn(buttons()).with_children(|row| {
        row.spawn(button(f, "Play without it", Action::SkipLook));
    });
}

/// A seat's time on the panel, counted down between the server's reports.
#[derive(Component)]
struct SeatTime(u32);

fn tick_clocks(
    watching: Res<WatchStatus>,
    mut times: Query<(&SeatTime, &mut Text, &mut TextColor)>,
) {
    let Some(table) = &watching.table else {
        return;
    };
    let now = Instant::now();
    for (seat, mut text, mut color) in &mut times {
        let Some(left) = table.time_left(seat.0, now) else {
            continue;
        };
        let words = clock::words(left);
        if text.0 != words {
            text.0 = words;
        }
        let low = if left < 10.0 {
            colors::WARN
        } else {
            colors::TEXT
        };
        if color.0 != low {
            color.0 = low;
        }
    }
}

/// The panel, which a headset hangs in the world (`vr_panels.rs`).
#[derive(Component)]
pub struct GamePanel;

/// How wide the panel is, and how far from the window's top left corner.
pub const WIDTH: f32 = 320.0;
pub const MARGIN: f32 = 16.0;

fn watch_on_start(
    mut pending: ResMut<WatchOnStart>,
    status: Res<ServerStatus>,
    net: Res<Net>,
    mut watching: ResMut<WatchStatus>,
) {
    if matches!(status.connection, Connection::Ready(_))
        && let Some(target) = pending.0.take()
    {
        watch(&net, &mut watching, target);
    }
}

fn spawn_panel(mut commands: Commands) {
    commands.spawn((
        GamePanel,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: px(MARGIN),
            top: px(MARGIN),
            width: px(WIDTH),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(14)),
            row_gap: px(8),
            border_radius: BorderRadius::all(px(10)),
            ..default()
        },
        BackgroundColor(colors::PANEL),
    ));
}

fn rebuild(
    mut commands: Commands,
    watching: Res<WatchStatus>,
    voice: Res<VoiceChat>,
    fonts: Res<UiFonts>,
    appearances: Res<Appearances>,
    downloads: Res<Downloads>,
    panel: Single<(Entity, &mut Node), With<GamePanel>>,
) {
    let (panel, mut node) = panel.into_inner();
    node.display = if watching.is_watching() {
        Display::Flex
    } else {
        Display::None
    };
    let f = &*fonts;
    commands
        .entity(panel)
        .despawn_related::<Children>()
        .with_children(|p| {
            match &watching.table {
                Some(table) => {
                    game(p, f, table, &appearances);
                    score_sheet(p, f, table, &appearances);
                    if let Some(turn) = &watching.turn {
                        your_turn(p, f, table, turn, &appearances);
                    }
                    waiting_for_players(p, f, table);
                    lobby::draw(p, f, table, &appearances, watching.lobby_problem.as_ref());
                    resigning(p, f, table, &watching.resign);
                    voice_chat::draw(p, f, &voice, table);
                }
                None if downloads.holds(watching.id()) => {
                    looking(p, f, downloads.waiting(watching.id()).unwrap_or_default());
                }
                None => {
                    let what = match &watching.target {
                        Some(Target::Bots { .. }) => "Starting a game between two bots…",
                        Some(Target::AgainstBots { .. }) => "Starting a game against the bots…",
                        Some(Target::Host { .. } | Target::Start(_)) => "Starting a game…",
                        Some(Target::Join(_)) => "Joining the game…",
                        _ => "Opening the game…",
                    };
                    let color = if watching.problem.is_some() {
                        colors::MUTED
                    } else {
                        colors::TEXT
                    };
                    p.spawn(line(f.semibold(16.0), color, what));
                }
            }
            if let Some(problem) = &watching.problem {
                p.spawn(line(f.regular(13.0), colors::WARN, &problem.message()));
                if watching.retry_in.is_some() {
                    p.spawn(line(
                        f.regular(13.0),
                        colors::MUTED,
                        "Trying again shortly.",
                    ));
                }
            }
            p.spawn(buttons()).with_children(|row| {
                if let Some(table) = watching.table.as_ref().filter(|t| t.finished()) {
                    if table.against_bots() {
                        row.spawn(button(
                            f,
                            "Play again",
                            Action::PlayBots(table.info.id.clone()),
                        ));
                    } else if table.yours().is_some() {
                        row.spawn(button(f, "New game", Action::Host(table.info.id.clone())));
                    } else {
                        row.spawn(button(
                            f,
                            "Watch another",
                            Action::WatchBots(table.info.id.clone()),
                        ));
                    }
                }
                row.spawn(button(f, "Back to the games", Action::Back));
            });
        });
}

fn game(p: &mut ChildSpawnerCommands, f: &UiFonts, table: &Table, appearances: &Appearances) {
    p.spawn(line(f.semibold(20.0), colors::TEXT, &table.info.name));
    let doing = if table.yours().is_some() {
        "playing"
    } else {
        "watching"
    };
    p.spawn(line(
        f.regular(13.0),
        colors::MUTED,
        &format!("Room {}  ·  {doing}", table.room_code),
    ));
    for seat in &table.info.seats {
        let to_move = !table.finished() && table.state.impulse_seat == Some(seat.seat);
        let won = table.state.winner_seat == Some(seat.seat);
        p.spawn(Node {
            column_gap: px(8),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn(seat_dots(
                appearances.seat_paints(&table.info.id, seat),
                14.0,
            ));
            let font = if to_move || won {
                f.semibold(14.0)
            } else {
                f.regular(14.0)
            };
            let time = table.time_left(seat.seat, Instant::now());
            let mut text = if table.is_open(seat.seat) {
                format!("{}  ·  open", seat.label)
            } else {
                format!("{}  —  {}", table.player(seat.seat), seat.label)
            };
            if table.yours().is_some_and(|s| s.seat == seat.seat) {
                text.push_str("  · you");
            }
            if won {
                text.push_str("  · won");
            }
            row.spawn(line(font, colors::TEXT, &text));
            if let Some(left) = time {
                row.spawn((
                    SeatTime(seat.seat),
                    line(f.semibold(14.0), colors::TEXT, &clock::words(left)),
                ));
            }
        });
        // What the look has the panel count for each seat, as the view counts it.
        let counted = counts(table, appearances, seat.seat);
        if !counted.is_empty() {
            p.spawn((
                Node {
                    margin: UiRect::left(px(22)),
                    ..default()
                },
                children![line(f.regular(13.0), colors::MUTED, &counted)],
            ));
        }
    }
    let color = if table.finished() {
        colors::GOOD
    } else {
        colors::TEXT
    };
    let summary = match table.state.impulse_seat {
        Some(seat) if table.to_play() => vec![Said::Seat(seat), Said::words("Your move.")],
        _ => table.told(),
    };
    let dots = |seat: u32| seat_paints(table, appearances, seat);
    p.spawn(said(f.semibold(15.0), color, &summary, &dots, 12.0));
    // What the seat on impulse is asked, as the server words it, for everyone watching; the
    // player's own is the hint under their move (`your_turn`).
    if let Some(words) = asked(table) {
        p.spawn(line(f.regular(14.0), colors::TEXT, words));
    }
}

/// The paint of each color a seat of the table plays.
fn seat_paints(table: &Table, appearances: &Appearances, seat: u32) -> Vec<Color> {
    table
        .info
        .seats
        .iter()
        .find(|d| d.seat == seat)
        .map(|d| appearances.seat_paints(&table.info.id, d))
        .unwrap_or_default()
}

/// The paint of a choice that names a color some seat of the game plays (a stone of one of a
/// seat's colors), shown beside its name; none for any other choice.
fn choice_paint(table: &Table, appearances: &Appearances, choice: &Gesture) -> Option<Color> {
    if choice.kind != "piece" {
        return None;
    }
    let seat = table
        .info
        .seats
        .iter()
        .find(|d| d.played().any(|c| c == choice.value))?;
    Some(appearances.seat_paint(&table.info.id, &choice.value, seat.seat))
}

/// A seat's counters of the view that the game's look has the panel show (`[panel]
/// counters`), each with the view's label, in the look's order: "Points 12  ·  Tricks 3".
fn counts(table: &Table, appearances: &Appearances, seat: u32) -> String {
    let Some(look) = appearances.for_game(&table.info.id) else {
        return String::new();
    };
    look.panel
        .counters
        .iter()
        .filter_map(|id| table.state.state.view_count(seat, id))
        .map(|(label, value)| format!("{label} {value}"))
        .collect::<Vec<_>>()
        .join("  ·  ")
}

/// One frame of a score sheet: its number, the marks of its balls, its total once scored, and
/// whether it is the one being played.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Frame {
    number: usize,
    marks: Vec<String>,
    total: Option<i64>,
    current: bool,
}

/// How many frames a score sheet has, the last with room for three marks.
const FRAMES: usize = 10;

/// The frames of the view's score sheet `sheet` (`frames`, each with its `marks` and its
/// `total` once scored, and the `frame` being played, from 1), all ten, those not yet played
/// blank.
fn frames(sheet: &serde_json::Value) -> Vec<Frame> {
    let played = sheet["frames"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let now = sheet["frame"].as_u64().unwrap_or(0) as usize;
    (0..FRAMES.max(played.len()))
        .map(|i| {
            let frame = played.get(i);
            Frame {
                number: i + 1,
                marks: frame
                    .and_then(|f| f["marks"].as_array())
                    .map(|m| {
                        m.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                total: frame.and_then(|f| f["total"].as_i64()),
                current: i + 1 == now && i < played.len(),
            }
        })
        .collect()
}

/// The game's score sheet, when its look names one in the view (`[cards] sheet`): a box for
/// each frame, with its number, its marks and its total.
fn score_sheet(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    table: &Table,
    appearances: &Appearances,
) {
    let Some(name) = appearances
        .for_game(&table.info.id)
        .and_then(|l| l.cards.as_ref())
        .and_then(|c| c.sheet.as_deref())
    else {
        return;
    };
    let sheet = &table.state.state.ext["view"][name];
    if sheet.is_null() {
        return;
    }
    p.spawn(Node {
        column_gap: px(2),
        flex_wrap: FlexWrap::NoWrap,
        ..default()
    })
    .with_children(|row| {
        for frame in frames(sheet) {
            let edge = if frame.current {
                colors::FIELD_FOCUS
            } else {
                colors::BORDER
            };
            row.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    min_width: px(27),
                    padding: UiRect::axes(px(1), px(2)),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(colors::CARD),
                BorderColor::all(edge),
            ))
            .with_children(|cell| {
                cell.spawn(line(
                    f.regular(10.0),
                    colors::MUTED,
                    &frame.number.to_string(),
                ));
                cell.spawn(line(f.regular(12.0), colors::TEXT, &frame.marks.join(" ")));
                let total = frame.total.map(|t| t.to_string()).unwrap_or_default();
                cell.spawn(line(f.semibold(12.0), colors::TEXT, &total));
            });
        }
    });
    if let Some(total) = sheet["total"].as_i64() {
        p.spawn(line(
            f.semibold(14.0),
            colors::TEXT,
            &format!("Score: {total}"),
        ));
    }
}

/// What the server says the seat on impulse is asked, while the game waits on someone who is
/// not this client.
fn asked(table: &Table) -> Option<&str> {
    let waiting = !table.finished() && table.state.status == "active" && !table.to_play();
    waiting.then(|| table.state.state.impulse_words()).flatten()
}

/// While a game this client is in waits for players: how somebody else joins it.
fn waiting_for_players(p: &mut ChildSpawnerCommands, f: &UiFonts, table: &Table) {
    if table.state.status != "waiting" || table.yours().is_none() || !table.open_to_anybody() {
        return;
    }
    p.spawn(line(
        f.regular(14.0),
        colors::TEXT,
        &format!(
            "Somebody can join it with the room code {},",
            table.room_code
        ),
    ));
    if !table.game.invite_url.is_empty() {
        p.spawn(line(
            f.regular(13.0),
            colors::MUTED,
            &format!("or on the web at {}", table.game.invite_url),
        ));
    }
}

/// The button that gives up the game, and the question it asks first.
fn resigning(p: &mut ChildSpawnerCommands, f: &UiFonts, table: &Table, resign: &Resign) {
    if table.finished() || table.yours().is_none() || table.state.status != "active" {
        return;
    }
    if let Resign::Failed(problem) = resign {
        p.spawn(line(f.regular(13.0), colors::WARN, &problem.message()));
    }
    p.spawn(buttons()).with_children(|row| match resign {
        Resign::Confirming => {
            row.spawn(line(f.regular(14.0), colors::TEXT, "Give up the game?"));
            row.spawn(button(f, "Resign", Action::Resign));
            row.spawn(button(f, "Keep playing", Action::KeepPlaying));
        }
        Resign::Sent => {
            row.spawn(line(f.regular(14.0), colors::MUTED, "Resigning…"));
        }
        Resign::No | Resign::Failed(_) => {
            row.spawn(button(f, "Resign", Action::Resign));
        }
    });
}

/// What the player may do on their impulse.
fn your_turn(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    table: &Table,
    turn: &Turn,
    appearances: &Appearances,
) {
    if let Some(problem) = &turn.problem {
        p.spawn(line(f.regular(13.0), colors::WARN, &problem.message()));
    }
    let drawn = table.state.state.panel_actions();
    // After a selection, what the position it leaves asks next (a line to remove, once
    // the move that made it is chosen), as the server previews it.
    let said = match turn.preview() {
        Some(after) => after.impulse_words(),
        None if turn.made.is_empty() => table.state.state.impulse_words(),
        None => None,
    };
    let hint = hint(turn, said, &drawn);
    if !hint.is_empty() {
        p.spawn(line(f.regular(14.0), colors::TEXT, &hint));
    }
    let (choices, _) = on_panel(turn.panel_choices(), &drawn);
    let can_go_back = turn.can_go_back();
    if choices.is_empty() && !can_go_back && !turn.can_play() && turn.stage != Stage::Failed {
        return;
    }
    p.spawn(buttons()).with_children(|row| {
        for choice in choices {
            // In the view's words where it has them (a team, by its members' names).
            let label = match table.state.state.choice_words(&choice.value) {
                Some(words) => words.to_string(),
                None => choice_label(&table.info, &choice),
            };
            let action = Action::Choose(choice.clone());
            match choice_paint(table, appearances, &choice) {
                Some(paint) => row.spawn(dotted_button(f, &label, action, paint)),
                None => row.spawn(button(f, &label, action)),
            };
        }
        if turn.can_play() {
            row.spawn(button(f, "Play", Action::Submit));
        }
        if can_go_back {
            row.spawn(button(f, "Back", Action::Undo));
        }
        if turn.stage == Stage::Failed {
            row.spawn(button(f, "Try again", Action::AskAgain));
        }
    });
}

/// The choices the panel offers as buttons: those the view does not say are drawn on the table
/// (`drawn`, the start of their values: a patience's cards). And whether any are drawn there.
fn on_panel(choices: Vec<Gesture>, drawn: &[&str]) -> (Vec<Gesture>, bool) {
    let (on_table, buttons): (Vec<Gesture>, Vec<Gesture>) = choices
        .into_iter()
        .partition(|c| drawn.iter().any(|d| c.value.starts_with(d)));
    (buttons, !on_table.is_empty())
}

/// What the player is told of their move: before they have chosen anything towards a
/// selection, what the server says they are asked (`said`: for the first, the position's view;
/// for one after it, the preview's), if it says and the move may not be played as it is; after
/// that, or without it, how to go on from here. `drawn` is what the view says is chosen on the
/// table rather than on the panel.
fn hint(turn: &Turn, said: Option<&str>, drawn: &[&str]) -> String {
    let (buttons, on_table) = on_panel(turn.panel_choices(), drawn);
    match turn.stage {
        Stage::Asking => String::new(),
        Stage::Choosing if turn.untouched() && !turn.can_play() && said.is_some() => {
            said.unwrap_or_default().to_string()
        }
        // Something in hand, to be put where it goes on the table.
        Stage::Choosing if on_table && turn.can_go_back() => {
            "Click where it goes; a right-click puts it back.".into()
        }
        Stage::Choosing if on_table && buttons.is_empty() => "Click what to move.".into(),
        Stage::Choosing if turn.ending_choice().is_some() => {
            if turn.targets().is_empty() {
                "Play (Enter) ends the move.".into()
            } else if turn.held_at().is_some() {
                "Click or drag it where it goes on, or Play (Enter) to end the move.".into()
            } else {
                "Click where to go on, or Play (Enter) to end the move.".into()
            }
        }
        Stage::Choosing if turn.can_play() => {
            format!("Play {} as it is (Enter), or go on.", turn.notation())
        }
        // A piece in hand (a stack, or one taken from beside the board too), or pieces to pick
        // up: a click or a drag moves them (`drag.rs`).
        Stage::Choosing
            if turn.in_hand_at().is_some()
                || turn.in_hand().is_some()
                || turn.taken().is_some() =>
        {
            "Click or drag it where it goes.".into()
        }
        Stage::Choosing if turn.unclicked_from().is_some() || !turn.moves_only().is_empty() => {
            "Click or drag a piece to move it.".into()
        }
        Stage::Choosing if !turn.targets().is_empty() => "Click where to play.".into(),
        Stage::Choosing if !buttons.is_empty() => "Choose below.".into(),
        Stage::Choosing => "There is nothing to play.".into(),
        Stage::Ready => format!(
            "Play {}? Enter plays it; a right-click or Backspace takes it back.",
            turn.notation()
        ),
        Stage::Sending => format!("Playing {}…", turn.notation()),
        Stage::Failed => "The server could not say what may be played.".into(),
    }
}

/// What a button for a choice says: an action by the label the rules give it, anything else by
/// its value.
pub fn choice_label(info: &GameInfo, choice: &Gesture) -> String {
    let named = (choice.kind == "action")
        .then(|| info.actions.iter().find(|a| a.id == choice.value))
        .flatten();
    match named {
        Some(action) => action.label.clone(),
        None => {
            let mut c = choice.value.chars();
            match c.next() {
                Some(first) => first.to_uppercase().chain(c).collect(),
                None => choice.kind.clone(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Request;
    use crate::net::api::{ActionDef, Board, LegalSelection, Selections};
    use crate::net::session::Snapshot;

    #[test]
    fn a_choice_is_labelled_by_its_action_or_its_value() {
        let info = GameInfo {
            id: "g".into(),
            name: "A game".into(),
            seats: vec![],
            board: Board {
                kind: "hex".into(),
                positions: vec![],
                boards: vec![],
                regions: Default::default(),
            },
            actions: vec![ActionDef {
                id: "swap".into(),
                label: "Swap sides".into(),
                kind: "swap".into(),
            }],
            goals: vec![],
            pieces: vec![],
            counters: vec![],
            opening: None,
            reserves: Default::default(),
            cards: None,
            engine: String::new(),
            clients: Vec::new(),
            endings: Default::default(),
        };
        let g = |kind: &str, value: &str| Gesture {
            kind: kind.into(),
            value: value.into(),
        };
        assert_eq!(choice_label(&info, &g("action", "swap")), "Swap sides");
        assert_eq!(choice_label(&info, &g("action", "pass")), "Pass");
        assert_eq!(choice_label(&info, &g("piece", "stone")), "Stone");
        assert_eq!(choice_label(&info, &g("count", "3")), "3");
    }

    #[test]
    fn the_player_is_told_what_the_server_asks_until_they_choose_something() {
        let option = |hex: &str, tile: &str| LegalSelection {
            notation: format!("tile:{tile}@{hex}"),
            action: "tile".into(),
            to: Some(hex.into()),
            from: None,
            piece: Some(tile.into()),
            aim: None,
            gestures: vec![
                Gesture::pos(hex),
                Gesture {
                    kind: "action".into(),
                    value: format!("tile-{tile}"),
                },
            ],
            captures: Vec::new(),
            converts_to: Default::default(),
            path: Vec::new(),
        };
        let mut turn = Turn::new(1);
        let asking = hint(&turn, Some("Choose a Court."), &[]);
        assert_eq!(
            asking, "",
            "nothing while the server is asked what may be played"
        );
        turn.answered(Selections {
            seat: 1,
            ply: 0,
            made: vec![],
            options: vec![
                option("b3", "green"),
                option("b3", "red"),
                option("c3", "green"),
            ],
            complete: false,
            may_end: false,
            state: None,
        });
        let said = Some("Choose where to lay a tile.");
        assert_eq!(hint(&turn, said, &[]), "Choose where to lay a tile.");
        // A game that says nothing gets the generic words.
        assert_eq!(hint(&turn, None, &[]), "Click where to play.");
        // Once the player has chosen something, how to go on from there: the server's words were
        // for the position, not for the half-made move.
        turn.gesture(&Gesture::pos("b3"));
        assert_eq!(hint(&turn, said, &[]), "Choose below.");
    }

    #[test]
    fn the_player_is_told_a_piece_may_be_clicked_or_dragged() {
        let step = |from: &str, to: &str| LegalSelection {
            notation: format!("{from}-{to}"),
            action: "step".into(),
            from: Some(from.into()),
            to: Some(to.into()),
            gestures: vec![Gesture::pos(from), Gesture::pos(to)],
            ..Default::default()
        };
        let mut turn = Turn::new(1);
        turn.answered(Selections {
            seat: 1,
            ply: 0,
            made: vec![],
            options: vec![step("a1", "a2"), step("a1", "a3")],
            complete: false,
            may_end: false,
            state: None,
        });
        assert_eq!(hint(&turn, None, &[]), "Click or drag a piece to move it.");
        turn.gesture(&Gesture::pos("a1"));
        assert_eq!(hint(&turn, None, &[]), "Click or drag it where it goes.");
    }

    #[test]
    fn a_score_sheet_has_ten_frames_with_the_marks_and_totals_the_view_gives() {
        let sheet = serde_json::json!({
            "frame": 2, "ball": 1, "total": 9,
            "frames": [{"marks": ["7", "2"], "total": 9}, {"marks": []}]
        });
        let all = frames(&sheet);
        assert_eq!(all.len(), 10);
        assert_eq!(all[0].marks, ["7", "2"]);
        assert_eq!(all[0].total, Some(9));
        assert!(!all[0].current && all[1].current, "the frame being played");
        assert_eq!((all[1].total, all[9].number), (None, 10));
        assert!(all[2..].iter().all(|f| f.marks.is_empty() && !f.current));
        assert_eq!(frames(&serde_json::Value::Null).len(), 10, "nothing played");
    }

    #[test]
    fn what_the_view_draws_on_the_table_is_no_button_and_the_hint_says_where_to_click() {
        let act = |v: &str| Gesture {
            kind: "action".into(),
            value: v.into(),
        };
        let option = |gestures: &[&str]| LegalSelection {
            notation: gestures.join(" "),
            action: "patience".into(),
            gestures: gestures.iter().map(|g| act(g)).collect(),
            ..LegalSelection::default()
        };
        let drawn = ["from-", "to-", "draw", "recycle", "deal"];
        let (buttons, on_table) =
            on_panel(vec![act("from-t1-1"), act("draw"), act("finish")], &drawn);
        assert_eq!(buttons, [act("finish")], "only what is not drawn");
        assert!(on_table);
        assert_eq!(on_panel(vec![act("swap")], &[]), (vec![act("swap")], false));

        let mut turn = Turn::new(1);
        turn.clicks_only = true;
        turn.answered(Selections {
            seat: 1,
            ply: 0,
            made: vec![],
            options: vec![option(&["from-t1-1", "to-t2"]), option(&["draw"])],
            complete: false,
            may_end: false,
            state: None,
        });
        assert_eq!(hint(&turn, None, &drawn), "Click what to move.");
        assert_eq!(hint(&turn, Some("Click a card."), &drawn), "Click a card.");
        turn.gesture(&act("from-t1-1"));
        assert_eq!(
            hint(&turn, Some("Click a card."), &drawn),
            "Click where it goes; a right-click puts it back."
        );
    }

    #[test]
    fn the_watch_asked_for_starts_once_the_server_is_there() {
        let (net, requests) = Net::for_test();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(net)
            .init_resource::<WatchStatus>()
            .insert_resource(ServerStatus {
                server: "http://127.0.0.1:8080".into(),
                connection: Connection::Connecting,
                notices: vec![],
                retry_in: None,
            })
            .insert_resource(WatchOnStart(Some(Target::Room("AB12CD".into()))))
            .add_systems(
                Update,
                watch_on_start.run_if(resource_changed::<ServerStatus>),
            );
        app.update();
        assert!(requests.try_recv().is_err(), "not before it is connected");

        app.world_mut().resource_mut::<ServerStatus>().connection = Connection::Ready(Snapshot {
            server: "http://127.0.0.1:8080".into(),
            version: "x".into(),
            api_version: "1.8.0".into(),
            defs: vec![],
            games: vec![],
            own: vec![],
            account: None,
            session_ended: false,
        });
        app.update();
        match requests.try_recv() {
            Ok(Request::Watch { target, .. }) => assert_eq!(target, Target::Room("AB12CD".into())),
            other => panic!("{other:?}"),
        }
        // A reconnection later does not start it again.
        app.world_mut().resource_mut::<ServerStatus>().set_changed();
        app.update();
        assert!(requests.try_recv().is_err());
        assert!(app.world().resource::<WatchStatus>().is_watching());
    }

    #[test]
    fn the_panel_counts_for_each_seat_what_the_games_look_names_in_the_looks_order() {
        let appearances = Appearances(vec![
            crate::appearance::Appearance::parse(
                "games = [\"g\"]\n[panel]\ncounters = [\"tricks\", \"score\", \"none\"]",
            )
            .unwrap(),
        ]);
        let mut table = crate::table::Table::new(crate::net::session::Opened {
            room_code: "AB12CD".into(),
            info: GameInfo {
                id: "g".into(),
                ..GameInfo::default()
            },
            game: serde_json::from_str(r#"{"game_def_id":"g","status":"active","seats":[]}"#)
                .unwrap(),
            state: serde_json::from_str(
                r#"{"state":{"cells":{},"ext":{"view":{"seats":{"2":{"counters":[
                    {"id":"score","label":"Points","value":-6},
                    {"id":"tricks","label":"Tricks","value":2}]}}}}},
                    "status":"active","move_seq":4}"#,
            )
            .unwrap(),
            bots: vec![],
        });
        assert_eq!(counts(&table, &appearances, 2), "Tricks 2  ·  Points -6");
        assert_eq!(
            counts(&table, &appearances, 1),
            "",
            "a seat the view counts nothing for"
        );
        table.info.id = "other".into();
        assert_eq!(counts(&table, &appearances, 2), "", "a game with no look");
    }

    #[test]
    fn a_seat_of_two_colors_has_two_dots_and_a_choice_of_one_has_its_dot() {
        let appearances = Appearances(vec![
            crate::appearance::Appearance::parse(
                "games = [\"g\"]\n[seats.red]\npaint = [0.6, 0.2, 0.1]\n\
                 [seats.yellow]\npaint = [0.7, 0.5, 0.2]",
            )
            .unwrap(),
        ]);
        let seat = |seat: u32, color: &str, colors: &[&str]| crate::net::api::SeatDef {
            seat,
            color: color.into(),
            label: format!("Seat {seat}"),
            colors: colors.iter().map(|c| c.to_string()).collect(),
        };
        let table = crate::table::Table::new(crate::net::session::Opened {
            room_code: "AB12CD".into(),
            info: GameInfo {
                id: "g".into(),
                seats: vec![seat(1, "red", &["red", "yellow"]), seat(2, "blue", &[])],
                ..GameInfo::default()
            },
            game: serde_json::from_str(r#"{"game_def_id":"g","status":"active","seats":[]}"#)
                .unwrap(),
            state: serde_json::from_str(r#"{"state":{"cells":{}},"status":"active","move_seq":0}"#)
                .unwrap(),
            bots: vec![],
        });
        let red = Color::srgb(0.6, 0.2, 0.1);
        let yellow = Color::srgb(0.7, 0.5, 0.2);
        assert_eq!(seat_paints(&table, &appearances, 1), [red, yellow]);
        assert_eq!(
            seat_paints(&table, &appearances, 2),
            [crate::look::seat_paint("blue", 2)],
            "a color the look does not paint: the standard look's"
        );
        assert!(seat_paints(&table, &appearances, 3).is_empty());
        let g = |kind: &str, value: &str| Gesture {
            kind: kind.into(),
            value: value.into(),
        };
        assert_eq!(
            choice_paint(&table, &appearances, &g("piece", "yellow")),
            Some(yellow)
        );
        assert_eq!(
            choice_paint(&table, &appearances, &g("piece", "queen")),
            None
        );
        assert_eq!(
            choice_paint(&table, &appearances, &g("action", "red")),
            None
        );
    }
}
