//! The first screen: which server, whether it is there, and what it has: the open games and the
//! games it can play. Stage 2 of the plan; it grows into the game browser of stage 5.
//!
//! It is a panel over the table, rebuilt whenever [`ServerStatus`] changes.

use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, ScrollArea, observe};

use crate::fonts::UiFonts;
use crate::net::api::{GameDef, OpenGame};
use crate::net::session::{Problem, Snapshot};
use crate::net::tls::Fingerprint;
use crate::net::{
    Connection, Net, Request, ServerStatus, Target, WatchStatus, reconnect, stop_watching, watch,
};

pub struct BrowserPlugin;

impl Plugin for BrowserPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_panel).add_systems(
            Update,
            (
                rebuild.run_if(resource_changed::<ServerStatus>),
                show_while_not_watching.run_if(resource_changed::<WatchStatus>),
                update_countdown,
                colour_buttons,
            )
                .chain(),
        );
    }
}

pub(crate) mod colours {
    use bevy::prelude::Color;

    pub const PANEL: Color = Color::srgba(0.07, 0.08, 0.10, 0.90);
    pub const CARD: Color = Color::srgba(1.0, 1.0, 1.0, 0.05);
    pub const TEXT: Color = Color::srgb(0.93, 0.93, 0.92);
    pub const MUTED: Color = Color::srgb(0.63, 0.65, 0.68);
    pub const GOOD: Color = Color::srgb(0.55, 0.82, 0.58);
    pub const WARN: Color = Color::srgb(0.96, 0.74, 0.38);
    pub const BUTTON: Color = Color::srgb(0.22, 0.32, 0.48);
    pub const BUTTON_HOVER: Color = Color::srgb(0.28, 0.40, 0.60);
}

/// The panel itself, hidden while a game is watched.
#[derive(Component)]
struct BrowserPanel;

/// What the panel's two rebuilt parts hang from.
#[derive(Component)]
struct StatusArea;

#[derive(Component)]
struct ListArea;

/// The "trying again in N s" line.
#[derive(Component)]
struct Countdown;

/// What a button does, on this panel and the game's.
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) enum Action {
    Retry,
    Trust(Fingerprint),
    /// Watch two of the server's bots play this game.
    WatchBots(String),
    /// Watch a game being played.
    Watch(String),
    /// Stop watching and go back to the list.
    Back,
}

fn spawn_panel(mut commands: Commands, fonts: Res<UiFonts>) {
    commands.spawn((
        BrowserPanel,
        Node {
            position_type: PositionType::Absolute,
            left: px(16),
            top: px(16),
            // Above the camera hint along the bottom.
            bottom: px(40),
            width: px(440),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(16)),
            row_gap: px(12),
            border_radius: BorderRadius::all(px(10)),
            ..default()
        },
        BackgroundColor(colours::PANEL),
        children![
            (
                Text::new("Bingle"),
                fonts.semibold(24.0),
                TextColor(colours::TEXT)
            ),
            (
                StatusArea,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    ..default()
                },
            ),
            (
                ListArea,
                ScrollArea,
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    // Lets it shrink below its content, so it scrolls instead of overflowing.
                    min_height: px(0),
                    overflow: Overflow::scroll_y(),
                    row_gap: px(8),
                    ..default()
                },
            ),
        ],
    ));
}

fn rebuild(
    mut commands: Commands,
    status: Res<ServerStatus>,
    fonts: Res<UiFonts>,
    status_area: Single<Entity, With<StatusArea>>,
    list_area: Single<(Entity, &mut ScrollPosition), With<ListArea>>,
) {
    let (list, mut scroll) = list_area.into_inner();
    let keep_scroll = matches!(status.connection, Connection::Ready(_));
    if !keep_scroll {
        scroll.0 = Vec2::ZERO;
    }
    let f = &*fonts;

    commands
        .entity(*status_area)
        .despawn_related::<Children>()
        .with_children(|area| {
            area.spawn(line(f.semibold(16.0), colours::TEXT, &status.server));
            match &status.connection {
                Connection::Connecting => {
                    area.spawn(line(f.regular(14.0), colours::MUTED, "Connecting…"));
                }
                Connection::Ready(snap) => {
                    area.spawn(line(
                        f.regular(14.0),
                        colours::GOOD,
                        &format!(
                            "Connected: Bingle {} (API {})",
                            snap.version, snap.api_version
                        ),
                    ));
                }
                Connection::Failed(problem) => problem_block(area, f, problem),
            }
            for notice in &status.notices {
                area.spawn(line(f.regular(13.0), colours::WARN, notice));
            }
        });

    commands
        .entity(list)
        .despawn_related::<Children>()
        .with_children(|area| match &status.connection {
            Connection::Ready(snap) => game_lists(area, f, snap),
            _ => {
                area.spawn(line(
                    f.regular(14.0),
                    colours::MUTED,
                    "The server's games appear here once it is connected.",
                ));
            }
        });
}

fn problem_block(area: &mut ChildSpawnerCommands, f: &UiFonts, problem: &Problem) {
    area.spawn(line(f.regular(14.0), colours::WARN, &problem.message()));
    match problem {
        Problem::Untrusted { fingerprint, .. } => {
            area.spawn(line(
                f.regular(14.0),
                colours::TEXT,
                "Its SHA-256 fingerprint is:",
            ));
            area.spawn(line(
                f.semibold(13.0),
                colours::TEXT,
                &fingerprint.two_lines(),
            ));
            area.spawn(line(
                f.regular(13.0),
                colours::MUTED,
                "A Bingle server that made its own certificate prints its fingerprint when it \
                 starts. If it is the same as this one, you can trust this certificate for this \
                 server; only this exact certificate will be trusted.",
            ));
            area.spawn(buttons()).with_children(|row| {
                row.spawn(button(
                    f,
                    "Trust this certificate",
                    Action::Trust(*fingerprint),
                ));
                row.spawn(button(f, "Try again", Action::Retry));
            });
        }
        _ => {
            if problem.worth_retrying() {
                area.spawn((Countdown, line(f.regular(13.0), colours::MUTED, "")));
            }
            area.spawn(buttons()).with_children(|row| {
                row.spawn(button(f, "Try again", Action::Retry));
            });
        }
    }
}

fn game_lists(area: &mut ChildSpawnerCommands, f: &UiFonts, snap: &Snapshot) {
    area.spawn(heading(f, &format!("Open games ({})", snap.games.len())));
    if snap.games.is_empty() {
        area.spawn(line(
            f.regular(14.0),
            colours::MUTED,
            "No open games right now.",
        ));
    }
    for game in &snap.games {
        let (title, detail) = describe_game(game);
        area.spawn(card()).with_children(|card| {
            card.spawn(line(f.semibold(15.0), colours::TEXT, &title));
            card.spawn(line(f.regular(13.0), colours::MUTED, &detail));
            if game.status == "active" {
                card.spawn(buttons()).with_children(|row| {
                    row.spawn(button(f, "Watch", Action::Watch(game.room_code.clone())));
                });
            }
        });
    }

    area.spawn(heading(f, &format!("Games ({})", snap.defs.len())));
    let mut defs: Vec<&GameDef> = snap.defs.iter().collect();
    defs.sort_by(|a, b| a.name.cmp(&b.name));
    for def in defs {
        area.spawn(card()).with_children(|card| {
            card.spawn(line(f.semibold(15.0), colours::TEXT, &describe_def(def)));
            if !def.description.is_empty() {
                card.spawn(line(f.regular(13.0), colours::MUTED, &def.description));
            }
            // A bot match is for two seats.
            if def.seats_required == 2 {
                card.spawn(buttons()).with_children(|row| {
                    row.spawn(button(
                        f,
                        "Watch two bots",
                        Action::WatchBots(def.id.clone()),
                    ));
                });
            }
        });
    }
}

/// A game's title line and what is happening in it, in words.
pub fn describe_game(game: &OpenGame) -> (String, String) {
    let title = format!("{}  ·  {}", game.name, game.room_code);
    let names: Vec<&str> = game.players.iter().map(|p| p.name.as_str()).collect();
    let players = if names.is_empty() {
        "nobody yet".to_string()
    } else {
        names.join(" vs ")
    };
    let state = match game.status.as_str() {
        "waiting" if game.open_seats > 0 => format!(
            "waiting for players: {} of {} {} free",
            game.open_seats,
            game.seats,
            if game.seats == 1 { "seat" } else { "seats" }
        ),
        "waiting" if game.lobby => "waiting for its host to start it".to_string(),
        "waiting" => "about to start".to_string(),
        "active" => match game
            .impulse_seat
            .and_then(|s| game.players.iter().find(|p| p.seat == s))
        {
            Some(p) => format!("{} to move", p.name),
            None => "playing".to_string(),
        },
        other => other.to_string(),
    };
    (title, format!("{players}  —  {state}"))
}

fn describe_def(def: &GameDef) -> String {
    let seats = match def.seats_required {
        1 => "1 player".to_string(),
        n => format!("{n} players"),
    };
    let mut out = format!("{}  —  {seats}", def.name);
    if def.prototype {
        out.push_str("  (prototype)");
    }
    out
}

pub(crate) fn line(font: TextFont, colour: Color, text: &str) -> impl Bundle {
    (Text::new(text), font, TextColor(colour))
}

fn heading(f: &UiFonts, text: &str) -> impl Bundle {
    (
        Text::new(text),
        f.semibold(17.0),
        TextColor(colours::TEXT),
        Node {
            margin: UiRect::top(px(6)),
            ..default()
        },
    )
}

fn card() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
            padding: UiRect::axes(px(10), px(8)),
            border_radius: BorderRadius::all(px(6)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(colours::CARD),
    )
}

pub(crate) fn buttons() -> impl Bundle {
    Node {
        column_gap: px(8),
        margin: UiRect::top(px(4)),
        ..default()
    }
}

pub(crate) fn button(f: &UiFonts, label: &str, action: Action) -> impl Bundle {
    (
        Button,
        action,
        Hovered::default(),
        Node {
            padding: UiRect::axes(px(12), px(6)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(colours::BUTTON),
        observe(on_activate),
        children![(
            Text::new(label),
            f.semibold(14.0),
            TextColor(colours::TEXT),
            // Clicks land on the button, not its label.
            Pickable::IGNORE,
        )],
    )
}

fn on_activate(
    activate: On<Activate>,
    actions: Query<&Action>,
    net: Res<Net>,
    mut status: ResMut<ServerStatus>,
    mut watching: ResMut<WatchStatus>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    match action {
        Action::Retry => reconnect(&net, &mut status),
        Action::Trust(fingerprint) => {
            status.retry_in = None;
            status.connection = Connection::Connecting;
            net.send(Request::Trust(*fingerprint));
        }
        Action::WatchBots(game_def_id) => watch(
            &net,
            &mut watching,
            Target::Bots {
                game_def_id: game_def_id.clone(),
            },
        ),
        Action::Watch(room_code) => watch(&net, &mut watching, Target::Room(room_code.clone())),
        Action::Back => {
            stop_watching(&net, &mut watching);
            // The list is as old as the last connection: read it again.
            reconnect(&net, &mut status);
        }
    }
}

fn show_while_not_watching(
    watching: Res<WatchStatus>,
    mut panel: Single<&mut Node, With<BrowserPanel>>,
) {
    let display = if watching.is_watching() {
        Display::None
    } else {
        Display::Flex
    };
    if panel.display != display {
        panel.display = display;
    }
}

type HoverChanged = (With<Action>, Changed<Hovered>);

fn colour_buttons(mut buttons: Query<(&Hovered, &mut BackgroundColor), HoverChanged>) {
    for (hovered, mut bg) in &mut buttons {
        bg.0 = if hovered.0 {
            colours::BUTTON_HOVER
        } else {
            colours::BUTTON
        };
    }
}

fn update_countdown(status: Res<ServerStatus>, mut lines: Query<&mut Text, With<Countdown>>) {
    let text = match &status.retry_in {
        Some(timer) => format!(
            "Trying again in {} s.",
            timer.remaining().as_secs_f32().ceil() as u32
        ),
        None => String::new(),
    };
    for mut line in &mut lines {
        if line.0 != text {
            line.0 = text.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::api::Player;

    fn app_with(connection: Connection) -> (App, std::sync::mpsc::Receiver<Request>) {
        let (net, requests) = Net::for_test();
        let mut app = App::new();
        app.insert_resource(net)
            .init_resource::<WatchStatus>()
            .insert_resource(ServerStatus {
                server: "https://box:8443".into(),
                connection,
                notices: vec![],
                retry_in: None,
            });
        (app, requests)
    }

    #[test]
    fn trust_sends_the_fingerprint_that_was_shown() {
        let fp = Fingerprint([7; 32]);
        let (mut app, requests) = app_with(Connection::Failed(Problem::Untrusted {
            fingerprint: fp,
            reason: "self-signed".into(),
        }));
        let button = app
            .world_mut()
            .spawn(Action::Trust(fp))
            .observe(on_activate)
            .id();
        app.world_mut().trigger(Activate { entity: button });
        assert_eq!(requests.try_recv(), Ok(Request::Trust(fp)));
        assert_eq!(
            app.world().resource::<ServerStatus>().connection,
            Connection::Connecting
        );
    }

    #[test]
    fn try_again_connects_again() {
        let (mut app, requests) = app_with(Connection::Failed(Problem::Unreachable(
            "Nothing is answering".into(),
        )));
        let button = app
            .world_mut()
            .spawn(Action::Retry)
            .observe(on_activate)
            .id();
        app.world_mut().trigger(Activate { entity: button });
        assert_eq!(requests.try_recv(), Ok(Request::Connect));
        assert_eq!(
            app.world().resource::<ServerStatus>().connection,
            Connection::Connecting
        );
    }

    fn game(status: &str, seats: u32, players: &[(&str, &str)]) -> OpenGame {
        OpenGame {
            room_code: "N5WUVW".into(),
            game_def_id: "hex-7".into(),
            name: "Hex (7×7)".into(),
            status: status.into(),
            seats,
            open_seats: seats - players.len() as u32,
            players: players
                .iter()
                .enumerate()
                .map(|(i, (name, bot))| Player {
                    seat: i as u32 + 1,
                    color: "red".into(),
                    name: (*name).into(),
                    bot: (*bot).into(),
                })
                .collect(),
            impulse_seat: None,
            lobby: false,
        }
    }

    #[test]
    fn a_game_in_play_says_who_is_to_move() {
        let mut g = game("active", 2, &[("Ana", ""), ("Bot (Random)", "random")]);
        g.impulse_seat = Some(2);
        let (title, detail) = describe_game(&g);
        assert_eq!(title, "Hex (7×7)  ·  N5WUVW");
        assert_eq!(detail, "Ana vs Bot (Random)  —  Bot (Random) to move");
    }

    #[test]
    fn a_waiting_game_says_how_many_seats_are_free() {
        let (_, detail) = describe_game(&game("waiting", 2, &[("Ana", "")]));
        assert_eq!(detail, "Ana  —  waiting for players: 1 of 2 seats free");
        let (_, detail) = describe_game(&game("waiting", 4, &[]));
        assert_eq!(
            detail,
            "nobody yet  —  waiting for players: 4 of 4 seats free"
        );
        let mut full = game("waiting", 2, &[("Ana", ""), ("Bo", "")]);
        full.lobby = true;
        assert_eq!(
            describe_game(&full).1,
            "Ana vs Bo  —  waiting for its host to start it"
        );
    }

    #[test]
    fn a_status_this_client_does_not_know_is_shown_as_it_is() {
        assert_eq!(
            describe_game(&game("paused", 2, &[])).1,
            "nobody yet  —  paused"
        );
    }

    #[test]
    fn a_game_definition_says_how_many_play() {
        let def = GameDef {
            id: "x".into(),
            name: "Solitaire".into(),
            seats_required: 1,
            family: None,
            description: String::new(),
            prototype: true,
        };
        assert_eq!(describe_def(&def), "Solitaire  —  1 player  (prototype)");
    }
}
