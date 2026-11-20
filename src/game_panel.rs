//! The panel beside the game being watched: what it is, who plays which seat, whose impulse it is
//! or how it ended, and the way back to the list. In a game this client plays, it also says what
//! the player may do on their impulse and holds the buttons for it: choices that are not a click
//! on the board (an action such as swapping sides), and playing the move shown or going back. It is
//! rebuilt whenever [`WatchStatus`] changes.
//!
//! It also starts the watch that `--watch` or `--watch-bots` asked for, once the server is there.

use std::time::Instant;

use bevy::prelude::*;

use crate::browser::{Action, button, buttons, colours, line};
use crate::clock;
use crate::fonts::UiFonts;
use crate::lobby;
use crate::look::seat_colour;
use crate::net::api::{GameInfo, Gesture};
use crate::net::{Connection, Net, Resign, ServerStatus, Target, WatchStatus, watch};
use crate::play::{Stage, Turn};
use crate::table::Table;

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
                    rebuild.run_if(resource_changed::<WatchStatus>),
                    tick_clocks,
                )
                    .chain(),
            );
    }
}

#[derive(Resource)]
struct WatchOnStart(Option<Target>);

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
    for (seat, mut text, mut colour) in &mut times {
        let Some(left) = table.time_left(seat.0, now) else {
            continue;
        };
        let words = clock::words(left);
        if text.0 != words {
            text.0 = words;
        }
        let low = if left < 10.0 {
            colours::WARN
        } else {
            colours::TEXT
        };
        if colour.0 != low {
            colour.0 = low;
        }
    }
}

#[derive(Component)]
struct GamePanel;

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
            left: px(16),
            top: px(16),
            width: px(320),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(14)),
            row_gap: px(8),
            border_radius: BorderRadius::all(px(10)),
            ..default()
        },
        BackgroundColor(colours::PANEL),
    ));
}

fn rebuild(
    mut commands: Commands,
    watching: Res<WatchStatus>,
    fonts: Res<UiFonts>,
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
                    game(p, f, table);
                    if let Some(turn) = &watching.turn {
                        your_turn(p, f, table, turn);
                    }
                    waiting_for_players(p, f, table);
                    lobby::draw(p, f, table, watching.lobby_problem.as_ref());
                    resigning(p, f, table, &watching.resign);
                }
                None => {
                    let what = match &watching.target {
                        Some(Target::Bots { .. }) => "Starting a game between two bots…",
                        Some(Target::AgainstBots { .. }) => "Starting a game against the bots…",
                        Some(Target::Host { .. } | Target::Start(_)) => "Starting a game…",
                        Some(Target::Join(_)) => "Joining the game…",
                        _ => "Opening the game…",
                    };
                    let colour = if watching.problem.is_some() {
                        colours::MUTED
                    } else {
                        colours::TEXT
                    };
                    p.spawn(line(f.semibold(16.0), colour, what));
                }
            }
            if let Some(problem) = &watching.problem {
                p.spawn(line(f.regular(13.0), colours::WARN, &problem.message()));
                if watching.retry_in.is_some() {
                    p.spawn(line(
                        f.regular(13.0),
                        colours::MUTED,
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

fn game(p: &mut ChildSpawnerCommands, f: &UiFonts, table: &Table) {
    p.spawn(line(f.semibold(20.0), colours::TEXT, &table.info.name));
    let doing = if table.yours().is_some() {
        "playing"
    } else {
        "watching"
    };
    p.spawn(line(
        f.regular(13.0),
        colours::MUTED,
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
            row.spawn((
                Node {
                    width: px(14),
                    height: px(14),
                    border_radius: BorderRadius::all(px(7)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(seat_colour(&seat.color, seat.seat)),
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
            row.spawn(line(font, colours::TEXT, &text));
            if let Some(left) = time {
                row.spawn((
                    SeatTime(seat.seat),
                    line(f.semibold(14.0), colours::TEXT, &clock::words(left)),
                ));
            }
        });
    }
    let colour = if table.finished() {
        colours::GOOD
    } else {
        colours::TEXT
    };
    let summary = if table.to_play() {
        "Your move.".to_string()
    } else {
        table.summary()
    };
    p.spawn(line(f.semibold(15.0), colour, &summary));
}

/// While a game this client is in waits for players: how somebody else joins it.
fn waiting_for_players(p: &mut ChildSpawnerCommands, f: &UiFonts, table: &Table) {
    if table.state.status != "waiting" || table.yours().is_none() || !table.game.has_open_seat() {
        return;
    }
    p.spawn(line(
        f.regular(14.0),
        colours::TEXT,
        &format!(
            "Somebody can join it with the room code {},",
            table.room_code
        ),
    ));
    if !table.game.invite_url.is_empty() {
        p.spawn(line(
            f.regular(13.0),
            colours::MUTED,
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
        p.spawn(line(f.regular(13.0), colours::WARN, &problem.message()));
    }
    p.spawn(buttons()).with_children(|row| match resign {
        Resign::Confirming => {
            row.spawn(line(f.regular(14.0), colours::TEXT, "Give up the game?"));
            row.spawn(button(f, "Resign", Action::Resign));
            row.spawn(button(f, "Keep playing", Action::KeepPlaying));
        }
        Resign::Sent => {
            row.spawn(line(f.regular(14.0), colours::MUTED, "Resigning…"));
        }
        Resign::No | Resign::Failed(_) => {
            row.spawn(button(f, "Resign", Action::Resign));
        }
    });
}

/// What the player may do on their impulse.
fn your_turn(p: &mut ChildSpawnerCommands, f: &UiFonts, table: &Table, turn: &Turn) {
    if let Some(problem) = &turn.problem {
        p.spawn(line(f.regular(13.0), colours::WARN, &problem.message()));
    }
    let hint = match turn.stage {
        Stage::Asking => String::new(),
        Stage::Choosing if !turn.targets().is_empty() => "Click a lit cell.".into(),
        Stage::Choosing if !turn.choices().is_empty() => "Choose below.".into(),
        Stage::Choosing => "There is nothing to play.".into(),
        Stage::Ready => format!(
            "Play {}? Enter plays it, Esc takes it back.",
            turn.notation()
        ),
        Stage::Sending => format!("Playing {}…", turn.notation()),
        Stage::Failed => "The server could not say what may be played.".into(),
    };
    if !hint.is_empty() {
        p.spawn(line(f.regular(14.0), colours::TEXT, &hint));
    }
    let choices = turn.choices();
    let can_go_back = turn.can_go_back();
    if choices.is_empty() && !can_go_back && turn.stage != Stage::Failed {
        return;
    }
    p.spawn(buttons()).with_children(|row| {
        for choice in choices {
            row.spawn(button(
                f,
                &choice_label(&table.info, &choice),
                Action::Choose(choice.clone()),
            ));
        }
        if turn.stage == Stage::Ready {
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
    use crate::net::api::{ActionDef, Board};
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
                regions: Default::default(),
            },
            actions: vec![ActionDef {
                id: "swap".into(),
                label: "Swap sides".into(),
            }],
            goals: vec![],
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
            api_version: "1.4.0".into(),
            defs: vec![],
            games: vec![],
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
}
