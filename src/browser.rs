//! The main screen: which server, whether it is there, and what it has, laid out as the web
//! page's home is: tabs (*Games*, and *Puzzles*, *Ratings*, *Tournaments* and *Global Chat*,
//! which are not built yet and only shown), and in *Games* a choice between *New* (a card for
//! each game to start, with its picture, name, marks and how many play it; picking one opens
//! its setup) and *Active* (the games being played or waiting for players).
//!
//! It is a panel over the table, shown while no game is open, and rebuilt whenever what it
//! shows changes.

use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, ScrollArea, observe};

use std::collections::HashMap;

use crate::appearance::Appearances;
use crate::families::{Badge, Family, families};
use crate::fonts::UiFonts;
use crate::friends::{FriendAction, Friends};
use crate::net::api::{GameDef, Gesture, OpenGame};
use crate::net::session::Problem;
use crate::net::tls::Fingerprint;
use crate::net::{
    Connection, GAMES_EVERY, GameList, GameQuery, LobbyCall, Net, Request, Resign, ServerStatus,
    Target, TimeControl, WatchStatus, find_games, lobby, reconnect, resign, stop_watching, watch,
};
use crate::new_game::{self, Fill, Form, NewGame};
use crate::play;
use crate::sign_in::{self, Signing};

pub struct BrowserPlugin {
    /// A game to open the new game form for, once connected (`--new-game`).
    pub new_game: Option<String>,
}

impl Plugin for BrowserPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NewGameOnStart(self.new_game.clone()))
            .init_resource::<ChosenMembers>()
            .init_resource::<MainScreen>()
            .add_systems(Startup, (spawn_panel, load_card_pictures))
            .add_systems(
                Update,
                (
                    new_game_on_start.run_if(resource_changed::<ServerStatus>),
                    rebuild_status.run_if(
                        resource_changed::<ServerStatus>.or_eager(resource_changed::<Signing>),
                    ),
                    rebuild_list.run_if(
                        resource_changed::<ServerStatus>
                            .or_eager(resource_changed::<GameList>)
                            .or_eager(resource_changed::<NewGame>)
                            .or_eager(resource_changed::<ChosenMembers>)
                            .or_eager(resource_changed::<Signing>)
                            .or_eager(resource_changed::<Friends>)
                            .or_eager(resource_changed::<MainScreen>),
                    ),
                    show_while_not_watching.run_if(resource_changed::<WatchStatus>),
                    update_countdown,
                    colour_buttons,
                    colour_cards,
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
    pub const BAD: Color = Color::srgb(0.90, 0.38, 0.33);
    pub const BUTTON: Color = Color::srgb(0.22, 0.32, 0.48);
    pub const BUTTON_HOVER: Color = Color::srgb(0.28, 0.40, 0.60);
    /// A choice that is made, among others that could be.
    pub const CHOSEN: Color = Color::srgb(0.27, 0.52, 0.42);
    pub const CHOSEN_HOVER: Color = Color::srgb(0.33, 0.60, 0.49);
    /// A field to type in, its edge, and its edge while it has the keyboard.
    pub const FIELD: Color = Color::srgba(0.0, 0.0, 0.0, 0.35);
    pub const FIELD_EDGE: Color = Color::srgb(0.30, 0.33, 0.38);
    pub const FIELD_FOCUS: Color = Color::srgb(0.45, 0.62, 0.90);
    /// The main screen's lines and edges, its chosen tab, and what is not built yet, as the web
    /// page's dark theme has them.
    pub const BORDER: Color = Color::srgb(0.23, 0.23, 0.26);
    pub const BORDER_HOVER: Color = Color::srgb(0.46, 0.45, 0.42);
    pub const ACCENT_SOFT: Color = Color::srgb(0.12, 0.23, 0.20);
    pub const DISABLED: Color = Color::srgb(0.40, 0.40, 0.42);
    /// Where a game has no picture. Opaque: the interface is blended in linear light, where a
    /// faint white over the dark panels shows far lighter than it would on the web page.
    pub const BLANK: Color = Color::srgb(0.15, 0.16, 0.18);
    /// The marks on a game's card: a prototype, and a beta.
    pub const PROTOTYPE: Color = Color::srgb(0.48, 0.65, 1.0);
    pub const BETA: Color = Color::srgb(0.88, 0.66, 0.29);
    pub const TUTORIAL: Color = Color::srgba(0.48, 0.65, 1.0, 0.18);
}

/// Where the main screen is: which of the *Games* tab's lists shows, and the game whose setup
/// is open in *New*, by its family's id.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub(crate) struct MainScreen {
    pub games: GamesView,
    pub family: Option<String>,
}

/// The *Games* tab's two lists.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GamesView {
    /// The games to start.
    #[default]
    New,
    /// The games being played or waiting for players.
    Active,
}

/// The main screen's tabs, as the web page's. Only *Games* is built.
const TABS: [(&str, bool); 5] = [
    ("Games", true),
    ("Puzzles", false),
    ("Ratings", false),
    ("Tournaments", false),
    ("Global Chat", false),
];

/// The pictures on the games' cards, by game id, loaded once.
#[derive(Resource, Debug, Default)]
struct CardPictures(HashMap<String, Handle<Image>>);

impl CardPictures {
    /// The picture of the family's chosen member, or else of any of its members.
    fn for_family(&self, family: &Family, chosen: &str) -> Option<Handle<Image>> {
        self.0.get(chosen).cloned().or_else(|| {
            family
                .members
                .iter()
                .find_map(|m| self.0.get(&m.def.id).cloned())
        })
    }
}

fn load_card_pictures(
    mut commands: Commands,
    appearances: Res<Appearances>,
    server: Res<AssetServer>,
) {
    let mut out = HashMap::new();
    for id in appearances.0.iter().flat_map(|look| &look.games) {
        if let Some(file) = appearances.card_picture(id) {
            out.insert(id.clone(), server.load(crate::assets::path(file)));
        }
    }
    commands.insert_resource(CardPictures(out));
}

/// The member of each family chosen on the list (a size, a layout), by the family's id. A family
/// not in it shows its default.
#[derive(Resource, Debug, Default)]
pub(crate) struct ChosenMembers(HashMap<String, String>);

/// The panel itself, hidden while a game is watched.
#[derive(Component)]
struct BrowserPanel;

/// What the panel's rebuilt parts hang from: the server's status, the tabs (and the *Games*
/// tab's choice of list) above the list, and the list.
#[derive(Component)]
struct StatusArea;

#[derive(Component)]
struct HeadArea;

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
    /// Play this game against the server's bots.
    PlayBots(String),
    /// Start a game of this and wait for people to join it.
    Host(String),
    /// Take an open seat in the game in this room.
    Join(String),
    /// Before the watched game starts: change the table, or start it.
    Lobby(LobbyCall),
    /// Give up the game (asked twice: the first press asks whether the player is sure).
    Resign,
    /// Not resigning after all.
    KeepPlaying,
    /// Open the form that sets up a new game of this.
    NewGame {
        def_id: String,
        name: String,
    },
    /// On the form: who plays a seat.
    SetSeat {
        seat: u32,
        fill: Fill,
    },
    /// On the form: how the game is timed.
    SetTime(TimeControl),
    /// On the form: whether the game waits for this client's start.
    SetLobby(bool),
    /// Make the game the form set up.
    StartGame,
    /// Close the form.
    CancelNewGame,
    /// On the list: which of a family's members (a size, a layout) its buttons are for.
    ChooseMember {
        family: String,
        def_id: String,
    },
    /// List only the games with a free seat (or every one again).
    FreeSeats(bool),
    /// On the main screen: show the *Games* tab's new games or its active ones.
    ShowGames(GamesView),
    /// On the main screen: open the setup of the game of this family.
    OpenFamily(String),
    /// On the main screen: close the game's setup, back to every game.
    AllGames,
    /// The escape menu's (`escape_menu.rs` acts on it).
    Menu(crate::escape_menu::MenuAction),
    /// List only this game's family (or every game), named as `name`.
    FindGames {
        family: Option<String>,
        name: String,
    },
    /// A choice for the move being built that is not a click on the board.
    Choose(Gesture),
    /// Play the move shown.
    Submit,
    /// Take back the last gesture or selection of the move being built.
    Undo,
    /// Ask the server again what may be played.
    AskAgain,
    /// Stop watching and go back to the list.
    Back,
    /// Go to the login screen, to log in to an account (`login.rs` acts on it).
    OpenSignIn,
    /// Send the sign-in form's step.
    SubmitSignIn,
    /// On the login screen (`login.rs` acts on it).
    Login(crate::login::LoginAction),
    /// On the sign-in form: ask for a recovery code (or the app's code again).
    UseRecoveryCode(bool),
    /// End the account's session.
    SignOut,
    /// On the friends panel (`friends.rs` acts on it).
    Friends(FriendAction),
    /// The game's voice chat (`voice_chat.rs` acts on it).
    Voice(crate::voice_chat::VoiceAction),
}

fn spawn_panel(mut commands: Commands, fonts: Res<UiFonts>) {
    commands.spawn((
        BrowserPanel,
        crate::login::MainOnly,
        Node {
            position_type: PositionType::Absolute,
            left: px(16),
            top: px(16),
            // Above the camera hint along the bottom, and clear of the friends panel.
            bottom: px(40),
            right: px(332),
            max_width: px(1000),
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
                HeadArea,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(10),
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

fn rebuild_status(
    mut commands: Commands,
    status: Res<ServerStatus>,
    signing: Res<Signing>,
    fonts: Res<UiFonts>,
    status_area: Single<Entity, With<StatusArea>>,
) {
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
                    sign_in::status(area, f, snap.account.as_ref(), snap.session_ended, &signing);
                }
                Connection::Failed(problem) => {
                    problem_block(area, f, problem);
                    if *problem == Problem::NeedsAccount && signing.form.is_none() {
                        area.spawn(buttons()).with_children(|row| {
                            row.spawn(button(f, "Sign in", Action::OpenSignIn));
                        });
                    }
                }
            }
            for notice in &status.notices {
                area.spawn(line(f.regular(13.0), colours::WARN, notice));
            }
        });
}

#[expect(
    clippy::too_many_arguments,
    reason = "the list area is drawn from everything that can stand in it"
)]
fn rebuild_list(
    mut commands: Commands,
    status: Res<ServerStatus>,
    list: Res<GameList>,
    new_game: Res<NewGame>,
    screen: (Res<MainScreen>, Res<ChosenMembers>, Res<CardPictures>),
    friends: Res<Friends>,
    fonts: Res<UiFonts>,
    appearances: Res<Appearances>,
    head_area: Single<Entity, With<HeadArea>>,
    list_area: Single<(Entity, &mut ScrollPosition), With<ListArea>>,
) {
    let (screen, chosen, pictures) = screen;
    let (area_id, mut scroll) = list_area.into_inner();
    // A list read again keeps its place; a new connection starts at the top.
    if !matches!(status.connection, Connection::Ready(_)) {
        scroll.0 = Vec2::ZERO;
    }
    let f = &*fonts;
    // The tabs, over the lists only: a form stands alone.
    let tabs = matches!(status.connection, Connection::Ready(_)) && new_game.form.is_none();
    commands
        .entity(*head_area)
        .despawn_related::<Children>()
        .with_children(|head| {
            if tabs {
                tab_bar(head, f);
                games_toggle(head, f, screen.games, list.games.len());
            }
        });
    commands
        .entity(area_id)
        .despawn_related::<Children>()
        .with_children(|area| match &status.connection {
            Connection::Ready(snap) => match &new_game.form {
                Some(form) => {
                    // A seat can be kept for a friend only by an account that has them.
                    let handles: Vec<String> = match (&snap.account, &friends.list) {
                        (Some(_), Some(list)) => {
                            list.friends.iter().map(|x| x.handle.clone()).collect()
                        }
                        _ => vec![],
                    };
                    new_game::draw(area, f, form, &handles, &appearances)
                }
                None => match screen.games {
                    GamesView::Active => games_being_played(area, f, &list),
                    GamesView::New => {
                        let families = families(&snap.defs);
                        match screen
                            .family
                            .as_ref()
                            .and_then(|id| families.iter().find(|x| x.id == *id))
                        {
                            Some(family) => setup(area, f, family, &chosen, &pictures),
                            None => gallery(area, f, &families, &chosen, &pictures),
                        }
                    }
                },
            },
            _ => {
                area.spawn(line(
                    f.regular(14.0),
                    colours::MUTED,
                    "The server's games appear here once it is connected.",
                ));
            }
        });
}

/// The main screen's tabs, the web page's: only *Games* can be chosen yet; the others are shown
/// greyed, for what is to come.
fn tab_bar(head: &mut ChildSpawnerCommands, f: &UiFonts) {
    head.spawn((
        Node {
            column_gap: px(4),
            border: UiRect::bottom(px(1)),
            ..default()
        },
        BorderColor::all(colours::BORDER),
    ))
    .with_children(|bar| {
        for (name, built) in TABS {
            let (background, edge, colour) = if built {
                (colours::ACCENT_SOFT, colours::BORDER, colours::TEXT)
            } else {
                (Color::NONE, Color::NONE, colours::DISABLED)
            };
            bar.spawn((
                Node {
                    padding: UiRect::axes(px(18), px(8)),
                    border: UiRect {
                        left: px(1),
                        right: px(1),
                        top: px(1),
                        bottom: px(0),
                    },
                    border_radius: BorderRadius::top(px(8)),
                    ..default()
                },
                BackgroundColor(background),
                BorderColor::all(edge),
                children![(
                    Text::new(name),
                    f.semibold(15.0),
                    TextColor(colour),
                    Pickable::IGNORE,
                )],
            ));
        }
    });
}

/// The *Games* tab's choice between its two lists.
fn games_toggle(head: &mut ChildSpawnerCommands, f: &UiFonts, view: GamesView, active: usize) {
    head.spawn(Node {
        column_gap: px(2),
        ..default()
    })
    .with_children(|row| {
        row.spawn(chip(
            f,
            "New",
            Action::ShowGames(GamesView::New),
            view == GamesView::New,
        ));
        row.spawn(chip(
            f,
            &format!("Active ({active})"),
            Action::ShowGames(GamesView::Active),
            view == GamesView::Active,
        ));
    });
}

/// A card on the list of games to start.
#[derive(Component)]
struct GameCard;

/// A card for each game to start, as the web page's gallery: its picture, its name, and its
/// marks and how many play it.
fn gallery(
    area: &mut ChildSpawnerCommands,
    f: &UiFonts,
    families: &[Family],
    chosen: &ChosenMembers,
    pictures: &CardPictures,
) {
    area.spawn(line(
        f.regular(13.0),
        colours::MUTED,
        "Pick a game to set up a match.",
    ));
    area.spawn(Node {
        flex_wrap: FlexWrap::Wrap,
        column_gap: px(10),
        row_gap: px(10),
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|grid| {
        for family in families {
            let def = family
                .member(chosen.0.get(&family.id).map(String::as_str))
                .def;
            grid.spawn((
                GameCard,
                Button,
                Action::OpenFamily(family.id.clone()),
                Hovered::default(),
                Node {
                    width: px(156),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    padding: UiRect::all(px(8)),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(10)),
                    ..default()
                },
                BackgroundColor(colours::CARD),
                BorderColor::all(colours::BORDER),
                observe(on_activate),
            ))
            .with_children(|card| {
                picture(card, pictures.for_family(family, &def.id), 136.0);
                card.spawn((
                    Text::new(family.title.as_str()),
                    f.semibold(15.0),
                    TextColor(colours::TEXT),
                    Pickable::IGNORE,
                ));
                card.spawn(card_meta(f, family));
            });
        }
    });
}

/// A game's picture, or a blank square where it has none.
fn picture(parent: &mut ChildSpawnerCommands, image: Option<Handle<Image>>, size: f32) {
    let mut square = parent.spawn((
        Node {
            width: px(size),
            height: px(size),
            flex_shrink: 0.0,
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        Pickable::IGNORE,
    ));
    match image {
        Some(image) => square.insert(ImageNode::new(image)),
        None => square.insert(BackgroundColor(colours::BLANK)),
    };
}

/// The line under a game's name: its marks, and how many play it.
fn card_meta(f: &UiFonts, family: &Family) -> impl Bundle + use<> {
    let badges: Vec<Badge> = family.badges();
    let players = family.players();
    (
        Node {
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            column_gap: px(6),
            row_gap: px(4),
            ..default()
        },
        Pickable::IGNORE,
        Children::spawn((
            SpawnIter(
                badges
                    .into_iter()
                    .map(|b| badge(f, b))
                    .collect::<Vec<_>>()
                    .into_iter(),
            ),
            Spawn((
                Text::new(players),
                f.regular(12.0),
                TextColor(colours::MUTED),
                Pickable::IGNORE,
            )),
        )),
    )
}

/// A mark on a card, as the web page draws it: a rounded tag, a prototype's blue and a beta's
/// amber.
fn badge(f: &UiFonts, badge: Badge) -> impl Bundle + use<> {
    let (colour, edge, background) = match badge {
        Badge::Prototype => (colours::PROTOTYPE, colours::PROTOTYPE, Color::NONE),
        Badge::Beta => (colours::BETA, colours::BETA, Color::NONE),
        Badge::Tutorial => (colours::TEXT, colours::BORDER, colours::TUTORIAL),
    };
    (
        Node {
            padding: UiRect::axes(px(7), px(1)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(999)),
            ..default()
        },
        BackgroundColor(background),
        BorderColor::all(edge),
        Pickable::IGNORE,
        children![(
            Text::new(badge.label()),
            f.semibold(10.0),
            TextColor(colour),
            Pickable::IGNORE,
        )],
    )
}

/// A game picked from the cards: its picture and description, a choice of its members when it
/// has several, and what can be done with the one chosen.
fn setup(
    area: &mut ChildSpawnerCommands,
    f: &UiFonts,
    family: &Family,
    chosen: &ChosenMembers,
    pictures: &CardPictures,
) {
    let def = family
        .member(chosen.0.get(&family.id).map(String::as_str))
        .def;
    area.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: px(12),
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|row| {
        row.spawn(button(f, "← All games", Action::AllGames));
        row.spawn(line(f.semibold(22.0), colours::TEXT, &family.title));
    });
    area.spawn(Node {
        column_gap: px(16),
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|row| {
        picture(row, pictures.for_family(family, &def.id), 220.0);
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            flex_shrink: 1.0,
            min_width: px(0),
            row_gap: px(6),
            ..default()
        })
        .with_children(|side| {
            side.spawn(card_meta(f, family));
            side.spawn(line(
                f.semibold(15.0),
                colours::TEXT,
                &describe_def(&family.title, def),
            ));
            if family.members.len() > 1 {
                side.spawn(buttons()).with_children(|row| {
                    for member in &family.members {
                        row.spawn(chip(
                            f,
                            &member.label,
                            Action::ChooseMember {
                                family: family.id.clone(),
                                def_id: member.def.id.clone(),
                            },
                            member.def.id == def.id,
                        ));
                    }
                });
            }
            if !def.description.is_empty() {
                side.spawn(line(f.regular(14.0), colours::MUTED, &def.description));
            }
            setup_buttons(side, f, family, def);
        });
    });
}

/// What can be done with a game: play it against bots, set up a new game, watch two bots, or
/// find the games of it being played.
fn setup_buttons(side: &mut ChildSpawnerCommands, f: &UiFonts, family: &Family, def: &GameDef) {
    side.spawn(buttons()).with_children(|row| {
        if def.seats_required >= 2 {
            let play = if def.seats_required == 2 {
                "Play a bot"
            } else {
                "Play bots"
            };
            row.spawn(button(f, play, Action::PlayBots(def.id.clone())));
            row.spawn(button(
                f,
                "New game…",
                Action::NewGame {
                    def_id: def.id.clone(),
                    name: def.name.clone(),
                },
            ));
            // A bot match is for two seats.
            if def.seats_required == 2 {
                row.spawn(button(
                    f,
                    "Watch two bots",
                    Action::WatchBots(def.id.clone()),
                ));
            }
        }
        row.spawn(button(
            f,
            "Find games",
            Action::FindGames {
                family: Some(family.id.clone()),
                name: family.title.clone(),
            },
        ));
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

fn games_being_played(area: &mut ChildSpawnerCommands, f: &UiFonts, list: &GameList) {
    area.spawn(line(f.regular(13.0), colours::MUTED, &describe_query(list)));
    area.spawn(buttons()).with_children(|row| {
        let (label, open_seats) = if list.query.open_seats {
            ("Show full games too", false)
        } else {
            ("Only with a free seat", true)
        };
        row.spawn(button(f, label, Action::FreeSeats(open_seats)));
        if list.query.family.is_some() {
            row.spawn(button(
                f,
                "Every game",
                Action::FindGames {
                    family: None,
                    name: String::new(),
                },
            ));
        }
    });
    if let Some(problem) = &list.problem {
        area.spawn(line(f.regular(13.0), colours::WARN, &problem.message()));
    }
    if list.games.is_empty() {
        area.spawn(line(f.regular(14.0), colours::MUTED, "None right now."));
    }
    for game in &list.games {
        let (title, detail) = describe_game(game);
        area.spawn(card()).with_children(|card| {
            card.spawn(line(f.semibold(15.0), colours::TEXT, &title));
            card.spawn(line(f.regular(13.0), colours::MUTED, &detail));
            if game.status == "active" {
                card.spawn(buttons()).with_children(|row| {
                    row.spawn(button(f, "Watch", Action::Watch(game.room_code.clone())));
                });
            } else if game.status == "waiting" && game.open_seats > 0 {
                card.spawn(buttons()).with_children(|row| {
                    row.spawn(button(f, "Join", Action::Join(game.room_code.clone())));
                });
            }
        });
    }
}

/// Which games the list shows, in words.
pub fn describe_query(list: &GameList) -> String {
    let which = match &list.query.family {
        Some(_) => format!("{} (any size or layout)", list.family_name),
        None => "Every game".to_string(),
    };
    let seats = if list.query.open_seats {
        "with a free seat"
    } else {
        "being played or waiting for players"
    };
    format!(
        "{which}, {seats}; read again every {} s.",
        GAMES_EVERY.as_secs()
    )
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
        "waiting" if game.reserved_for_you => "a seat is kept for you".to_string(),
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

/// A game's title line: its family's title, and how many play the member chosen.
fn describe_def(title: &str, def: &GameDef) -> String {
    let seats = match def.seats_required {
        1 => "1 player".to_string(),
        n => format!("{n} players"),
    };
    let mut out = format!("{title}  —  {seats}");
    if def.prototype {
        out.push_str("  (prototype)");
    }
    out
}

pub(crate) fn line(font: TextFont, colour: Color, text: &str) -> impl Bundle {
    (Text::new(text), font, TextColor(colour))
}

/// A seat's dot, in its colour, ringed so that a dark one still shows on a dark panel.
pub(crate) fn seat_dot(colour: Color, size: f32) -> impl Bundle {
    (
        Node {
            width: px(size),
            height: px(size),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(size / 2.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(colour),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.35)),
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
        row_gap: px(6),
        flex_wrap: FlexWrap::Wrap,
        margin: UiRect::top(px(4)),
        ..default()
    }
}

pub(crate) fn button(f: &UiFonts, label: &str, action: Action) -> impl Bundle {
    button_in(f, label, action, false)
}

/// Whether a button is the choice made (see [`chip`]).
#[derive(Component)]
pub(crate) struct Chosen(bool);

fn button_in(f: &UiFonts, label: &str, action: Action, chosen: bool) -> impl Bundle {
    let colour = if chosen {
        colours::CHOSEN
    } else {
        colours::BUTTON
    };
    (
        Button,
        action,
        Chosen(chosen),
        Hovered::default(),
        Node {
            padding: UiRect::axes(px(12), px(6)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(colour),
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

#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer takes what it touches as parameters: every panel's buttons land here"
)]
fn on_activate(
    activate: On<Activate>,
    actions: Query<&Action>,
    net: Res<Net>,
    mut status: ResMut<ServerStatus>,
    mut watching: ResMut<WatchStatus>,
    mut list: ResMut<GameList>,
    mut new_game: ResMut<NewGame>,
    mut chosen: ResMut<ChosenMembers>,
    mut signing: ResMut<Signing>,
    mut screen: ResMut<MainScreen>,
    mut list_scroll: Query<&mut ScrollPosition, With<ListArea>>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    let mut to_top = || {
        for mut scroll in &mut list_scroll {
            scroll.0 = Vec2::ZERO;
        }
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
        Action::PlayBots(game_def_id) => watch(
            &net,
            &mut watching,
            Target::AgainstBots {
                game_def_id: game_def_id.clone(),
            },
        ),
        Action::Host(game_def_id) => watch(
            &net,
            &mut watching,
            Target::Host {
                game_def_id: game_def_id.clone(),
            },
        ),
        Action::Join(room_code) => watch(&net, &mut watching, Target::Join(room_code.clone())),
        Action::Resign => match watching.resign {
            Resign::Confirming => resign(&net, &mut watching),
            _ => watching.resign = Resign::Confirming,
        },
        Action::KeepPlaying => watching.resign = Resign::No,
        Action::NewGame { def_id, name } => {
            signing.form = None;
            new_game.form = Some(Form::new(def_id, name));
            net.send(Request::Setup {
                game_def_id: def_id.clone(),
            });
            to_top();
        }
        Action::SetSeat { seat, fill } => {
            if let Some(form) = new_game.form.as_mut() {
                form.set(*seat, fill.clone());
            }
        }
        Action::SetTime(time) => {
            if let Some(form) = new_game.form.as_mut() {
                form.time = *time;
            }
        }
        Action::SetLobby(wait) => {
            if let Some(form) = new_game.form.as_mut() {
                form.lobby = *wait;
            }
        }
        Action::Lobby(call) => lobby(&net, &mut watching, call.clone()),
        Action::StartGame => {
            if let Some(settings) = new_game.form.as_ref().and_then(Form::settings) {
                new_game.form = None;
                watch(&net, &mut watching, Target::Start(settings));
            }
        }
        Action::CancelNewGame => new_game.form = None,
        Action::ChooseMember { family, def_id } => {
            chosen.0.insert(family.clone(), def_id.clone());
        }
        Action::FreeSeats(open_seats) => {
            let query = GameQuery {
                open_seats: *open_seats,
                ..list.query.clone()
            };
            let name = list.family_name.clone();
            find_games(&net, &mut list, query, name);
        }
        Action::FindGames { family, name } => {
            let query = GameQuery {
                family: family.clone(),
                ..list.query.clone()
            };
            find_games(&net, &mut list, query, name.clone());
            screen.games = GamesView::Active;
            to_top();
        }
        Action::ShowGames(view) => {
            if screen.games != *view {
                screen.games = *view;
                to_top();
            }
        }
        Action::OpenFamily(family) => {
            screen.family = Some(family.clone());
            to_top();
        }
        Action::AllGames => {
            screen.family = None;
            to_top();
        }
        Action::Choose(g) => play::gesture(&net, &mut watching, g),
        Action::Submit => play::submit(&net, &mut watching),
        Action::Undo => play::back(&mut watching),
        Action::AskAgain => play::ask_again(&net, &mut watching),
        Action::OpenSignIn => new_game.form = None,
        Action::SubmitSignIn => sign_in::submit(&net, &mut signing),
        Action::UseRecoveryCode(recovery) => {
            if let Some(form) = signing.form.as_mut() {
                form.recovery = *recovery;
                form.code.clear();
                form.problem = None;
            }
        }
        Action::SignOut => net.send(Request::SignOut),
        Action::Friends(_) | Action::Voice(_) | Action::Menu(_) | Action::Login(_) => {}
        Action::Back => {
            stop_watching(&net, &mut watching);
            // The list is as old as the last connection: read it again.
            reconnect(&net, &mut status);
        }
    }
}

#[derive(Resource)]
struct NewGameOnStart(Option<String>);

fn new_game_on_start(
    mut pending: ResMut<NewGameOnStart>,
    status: Res<ServerStatus>,
    net: Res<Net>,
    mut new_game: ResMut<NewGame>,
) {
    let Connection::Ready(snap) = &status.connection else {
        return;
    };
    let Some(def_id) = pending.0.take() else {
        return;
    };
    let name = snap
        .defs
        .iter()
        .find(|d| d.id == def_id)
        .map_or_else(|| def_id.clone(), |d| d.name.clone());
    new_game.form = Some(Form::new(&def_id, &name));
    net.send(Request::Setup {
        game_def_id: def_id,
    });
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

fn colour_buttons(mut buttons: Query<(&Hovered, &mut BackgroundColor, &Chosen), HoverChanged>) {
    for (hovered, mut bg, chosen) in &mut buttons {
        bg.0 = match (chosen.0, hovered.0) {
            (true, true) => colours::CHOSEN_HOVER,
            (true, false) => colours::CHOSEN,
            (false, true) => colours::BUTTON_HOVER,
            (false, false) => colours::BUTTON,
        };
    }
}

type CardHoverChanged = (With<GameCard>, Changed<Hovered>);

/// A card's edge lightens under the pointer, as on the web page.
fn colour_cards(mut cards: Query<(&Hovered, &mut BorderColor), CardHoverChanged>) {
    for (hovered, mut edge) in &mut cards {
        *edge = BorderColor::all(if hovered.0 {
            colours::BORDER_HOVER
        } else {
            colours::BORDER
        });
    }
}

/// A button for one choice among several, coloured when it is the one made.
pub(crate) fn chip(f: &UiFonts, label: &str, action: Action, chosen: bool) -> impl Bundle {
    button_in(f, label, action, chosen)
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
            .init_resource::<GameList>()
            .init_resource::<NewGame>()
            .init_resource::<ChosenMembers>()
            .init_resource::<MainScreen>()
            .init_resource::<Signing>()
            .init_resource::<crate::call::DirectCalling>()
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
    fn the_filters_ask_for_the_narrower_list_and_keep_each_other() {
        let (mut app, requests) = app_with(Connection::Connecting);
        let press = |app: &mut App, action: Action| {
            let button = app.world_mut().spawn(action).observe(on_activate).id();
            app.world_mut().trigger(Activate { entity: button });
        };
        press(&mut app, Action::FreeSeats(true));
        press(
            &mut app,
            Action::FindGames {
                family: Some("hex-11".into()),
                name: "Hex (7×7)".into(),
            },
        );
        let asked: Vec<Request> = requests.try_iter().collect();
        assert!(
            matches!(
                &asked[..],
                [
                    Request::Games { query: first, asked: 1 },
                    Request::Games { query: second, asked: 2 },
                ] if *first == GameQuery { family: None, open_seats: true }
                    && *second == GameQuery { family: Some("hex-11".into()), open_seats: true }
            ),
            "{asked:?}"
        );
        let list = app.world().resource::<GameList>();
        assert_eq!(
            describe_query(list),
            "Hex (7×7) (any size or layout), with a free seat; read again every 5 s."
        );
    }

    #[test]
    fn the_main_screen_opens_a_games_setup_and_switches_lists() {
        let (mut app, requests) = app_with(Connection::Connecting);
        let press = |app: &mut App, action: Action| {
            let button = app.world_mut().spawn(action).observe(on_activate).id();
            app.world_mut().trigger(Activate { entity: button });
        };
        assert_eq!(*app.world().resource::<MainScreen>(), MainScreen::default());
        press(&mut app, Action::OpenFamily("go".into()));
        assert_eq!(
            app.world().resource::<MainScreen>().family.as_deref(),
            Some("go")
        );
        press(&mut app, Action::AllGames);
        assert_eq!(app.world().resource::<MainScreen>().family, None);
        press(&mut app, Action::ShowGames(GamesView::Active));
        assert_eq!(
            app.world().resource::<MainScreen>().games,
            GamesView::Active
        );
        press(&mut app, Action::ShowGames(GamesView::New));
        assert!(
            requests.try_recv().is_err(),
            "nothing is asked of the server"
        );
        // Finding a game's games shows them, in the active list.
        press(
            &mut app,
            Action::FindGames {
                family: Some("go".into()),
                name: "Go".into(),
            },
        );
        assert_eq!(
            app.world().resource::<MainScreen>().games,
            GamesView::Active
        );
        assert!(matches!(requests.try_recv(), Ok(Request::Games { .. })));
    }

    #[test]
    fn only_the_games_tab_is_built() {
        let built: Vec<&str> = TABS.iter().filter(|(_, b)| *b).map(|(n, _)| *n).collect();
        assert_eq!(built, ["Games"]);
        assert_eq!(TABS.len(), 5);
    }

    #[test]
    fn a_card_shows_its_chosen_members_picture_or_any_members() {
        let def = |id: &str| GameDef {
            id: id.into(),
            name: format!("Sq ({id})"),
            seats_required: 2,
            family: Some("a".into()),
            description: String::new(),
            prototype: false,
            version: "1.0.0".into(),
            tutorials: 0,
            private: false,
        };
        let defs = [def("a"), def("b")];
        let family = &families(&defs)[0];
        let mut pictures = CardPictures::default();
        assert_eq!(pictures.for_family(family, "a"), None);
        let b = Handle::<Image>::default();
        pictures.0.insert("b".into(), b.clone());
        assert_eq!(pictures.for_family(family, "a"), Some(b));
    }

    #[test]
    fn a_members_chip_is_remembered_for_its_family() {
        let (mut app, requests) = app_with(Connection::Connecting);
        let button = app
            .world_mut()
            .spawn(Action::ChooseMember {
                family: "hex-11".into(),
                def_id: "hex-7".into(),
            })
            .observe(on_activate)
            .id();
        app.world_mut().trigger(Activate { entity: button });
        let chosen = app.world().resource::<ChosenMembers>();
        assert_eq!(chosen.0.get("hex-11").map(String::as_str), Some("hex-7"));
        assert!(
            requests.try_recv().is_err(),
            "nothing is asked of the server"
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
            reserved_for_you: false,
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
    fn a_seat_kept_for_this_viewer_is_said() {
        let mut kept = game("waiting", 2, &[("Ana", "")]);
        kept.reserved_for_you = true;
        assert_eq!(describe_game(&kept).1, "Ana  —  a seat is kept for you");
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
            version: "1.0.0".into(),
            tutorials: 0,
            private: false,
        };
        assert_eq!(
            describe_def("Solitaire", &def),
            "Solitaire  —  1 player  (prototype)"
        );
    }
}
