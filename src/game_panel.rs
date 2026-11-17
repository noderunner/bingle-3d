//! The panel beside the game being watched: what it is, who plays which seat, whose impulse it is
//! or how it ended, and the way back to the list. It is rebuilt whenever [`WatchStatus`] changes.
//!
//! It also starts the watch that `--watch` or `--watch-bots` asked for, once the server is there.

use bevy::prelude::*;

use crate::browser::{Action, button, buttons, colours, line};
use crate::fonts::UiFonts;
use crate::look::seat_colour;
use crate::net::{Connection, Net, ServerStatus, Target, WatchStatus, watch};
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
                ),
            );
    }
}

#[derive(Resource)]
struct WatchOnStart(Option<Target>);

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
                Some(table) => game(p, f, table),
                None => {
                    let what = match &watching.target {
                        Some(Target::Bots { .. }) => "Starting a game between two bots…",
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
                    row.spawn(button(
                        f,
                        "Watch another",
                        Action::WatchBots(table.info.id.clone()),
                    ));
                }
                row.spawn(button(f, "Back to the games", Action::Back));
            });
        });
}

fn game(p: &mut ChildSpawnerCommands, f: &UiFonts, table: &Table) {
    p.spawn(line(f.semibold(20.0), colours::TEXT, &table.info.name));
    p.spawn(line(
        f.regular(13.0),
        colours::MUTED,
        &format!("Room {}  ·  watching", table.room_code),
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
            let mut text = format!("{}  —  {}", table.player(seat.seat), seat.label);
            if won {
                text.push_str("  · won");
            }
            row.spawn(line(font, colours::TEXT, &text));
        });
    }
    let colour = if table.finished() {
        colours::GOOD
    } else {
        colours::TEXT
    };
    p.spawn(line(f.semibold(15.0), colour, &table.summary()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Request;
    use crate::net::session::Snapshot;

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
            api_version: "1.1.0".into(),
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
