//! A run started from a link (`--link`, `link.rs`), on the login screen, and whether links
//! open this program at all (`link_handler.rs`).
//!
//! A link goes through the login screen's own check of its server (`Request::Probe`). For a
//! server this client has never used, nothing is sent there until the player has seen the
//! address and said yes. Once the probe is good, a link with a code asks before making the
//! client somebody else there (`link::decide`), then trades the code and goes in; anything
//! wrong with the server stays on the login screen with its reason, and the link goes on once
//! it is put right. The room is opened as the client connects (`game_panel::WatchOnStart`).
//!
//! A link handed over by a copy started while this one runs (`link_channel.rs`) goes the same
//! way (`take_handed`), with what this copy shows left only once the player has said so: a
//! game being played is asked about first (`Stage::AskLeave`), and a server never used before
//! anything is sent there. A link for the game shown only brings the window forward. While a
//! question is open, a link for the same server and room takes the place of the one asked
//! about (its code is the newer); any other is not opened, and the question says so.
//!
//! On the main screen of a first run (not one a script limits, nor in a headset), a question
//! offers to open game links with this program; *Settings* turns it on and off after that.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::browser::{Action, button, buttons, colors, line};
use crate::fonts::UiFonts;
use crate::game_panel::WatchOnStart;
use crate::link::{self, Identity, Link, Next};
use crate::link_channel::{Forward, Inbox};
use crate::login::{Light, LoginScreen, Screen};
use crate::net::server_url::ServerUrl;
use crate::net::{Connection, Net, Request, ServerStatus, Target, WatchStatus, stop_watching};
use crate::sign_in::Signing;

pub struct LinkOpenPlugin {
    /// A run a script limits (`--frames`, `--screenshot`): it asks nothing about links and
    /// changes no registration.
    pub limited: bool,
    pub in_headset: bool,
}

impl Plugin for LinkOpenPlugin {
    fn build(&self, app: &mut App) {
        let quiet = self.limited || self.in_headset || !crate::link_handler::supported();
        let kept = crate::config::Store::load(crate::config::Store::default_dir())
            .config
            .open_links;
        // A copy moved, or a newer one unpacked elsewhere, takes the links over while they are
        // on: the copy started last is the one they open.
        let mut on = false;
        if !quiet && kept == Some(true) {
            match std::env::current_exe() {
                Ok(exe) => match crate::link_handler::keep_pointing_at(&exe) {
                    Ok(said) => {
                        on = true;
                        if let Some(said) = said {
                            info!("{said}");
                        }
                    }
                    Err(e) => warn!("game links: {e}"),
                },
                Err(e) => warn!("game links: where this program is cannot be read ({e})"),
            }
        } else if !quiet {
            on = matches!(
                (crate::link_handler::registered(), std::env::current_exe()),
                (Ok(Some(now)), Ok(exe)) if now == exe
            );
        }
        app.insert_resource(LinkSetting {
            shown: !quiet,
            on,
            offer: !quiet && kept.is_none(),
            problem: None,
        })
        .add_observer(on_answer)
        .add_systems(Startup, spawn_panel)
        .add_systems(
            Update,
            (
                take_handed.run_if(resource_exists::<Inbox>),
                advance.run_if(resource_exists::<LinkOpen>),
                rebuild.run_if(
                    resource_changed::<LinkSetting>
                        .or_eager(resource_exists_and_changed::<LinkOpen>)
                        .or_eager(resource_changed::<Screen>),
                ),
            )
                .chain(),
        );
    }
}

/// Whether game links open this program, for *Settings* and the first run's question.
#[derive(Resource, Debug)]
pub struct LinkSetting {
    /// This run may change it: a build that registers links, not limited by a script.
    pub shown: bool,
    /// Links open this program.
    pub on: bool,
    /// The first run's question is still to be answered.
    pub offer: bool,
    /// Why the last change could not be made.
    pub problem: Option<String>,
}

impl LinkSetting {
    /// Turns links on or off, for this program, and says so in the config (`open_links`).
    pub fn set(&mut self, net: &Net, on: bool) {
        let result = if on {
            std::env::current_exe()
                .map_err(|e| format!("where this program is cannot be read ({e})"))
                .and_then(|exe| crate::link_handler::register(&exe).map_err(|e| e.to_string()))
        } else {
            crate::link_handler::unregister().map_err(|e| e.to_string())
        };
        self.offer = false;
        match result {
            Ok(()) => {
                self.on = on;
                self.problem = None;
                net.send(Request::RememberOpenLinks(on));
            }
            Err(e) => {
                warn!("game links: {e}");
                self.problem = Some(e);
            }
        }
    }
}

/// A link this run was started from, and how far it has gone.
#[derive(Resource, Debug)]
pub struct LinkOpen {
    pub link: Link,
    /// The client has used its server before (`Store::has_used`).
    pub used: bool,
    pub stage: Stage,
    /// What the player is to be told of the trade, until they close it.
    pub said: Option<(String, String)>,
    /// Handed over while this copy ran: what it shows is left only once the questions before
    /// going are answered.
    pub running: bool,
    /// Another link came while a question was open, and was not opened.
    pub set_aside: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// A link handed over while a game is being played: leave it?
    AskLeave,
    /// The server is shown, and asked about; nothing is sent there yet.
    AskServer,
    /// The login screen checks the server.
    Probing,
    /// Who the client is there, and who the link would make it.
    AskIdentity { now: Identity, would: link::Kind },
    /// Going in, or given up: the login screen has the rest.
    Done,
}

impl LinkOpen {
    pub fn new(link: Link, used: bool) -> Self {
        let stage = if used {
            Stage::Probing
        } else {
            Stage::AskServer
        };
        LinkOpen {
            link,
            used,
            stage,
            said: None,
            running: false,
            set_aside: false,
        }
    }

    /// A link handed over while this copy runs (`link_channel.rs`): a game being played is
    /// asked about first, then a server never used; else it goes at once.
    pub fn handed(link: Link, used: bool, busy: bool) -> Self {
        let mut open = LinkOpen::new(link, used);
        if busy {
            open.stage = Stage::AskLeave;
        }
        open.running = true;
        open
    }

    /// A question about the link is open.
    fn asking(&self) -> bool {
        matches!(
            self.stage,
            Stage::AskLeave | Stage::AskServer | Stage::AskIdentity { .. }
        )
    }

    /// The game the link opens, once connected.
    pub fn target(&self) -> Target {
        match self.link.action {
            link::Action::Join => Target::Join(self.link.room.clone()),
            link::Action::Watch => Target::Room(self.link.room.clone()),
        }
    }

    /// The address the login screen checks for the link.
    pub fn address(&self) -> String {
        self.link.server.to_string()
    }
}

/// What the link's and the offer's buttons do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Leave the game being played for the link's.
    Leave,
    /// Connect to the server never used before.
    Connect,
    /// Not that server: the login screen, at the one configured.
    Cancel,
    /// Trade the code, and be who the link makes the client.
    Switch,
    /// Join as who the client already is.
    Stay,
    /// The first run's question: open game links with this program, or not.
    OpenLinks(bool),
    /// Close what was said of the trade.
    Dismiss,
}

/// Goes in at the server the login screen has checked, as its buttons do.
fn go(net: &Net, login: &mut LoginScreen, status: &mut ServerStatus) {
    status.connection = Connection::Connecting;
    status.retry_in = None;
    login.entering = true;
    login.problem = None;
    net.send(Request::Connect);
}

/// What a link handed over does, given the question open, if any, and the game shown (its
/// server's origin and room).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// The game shown already: only the window comes forward.
    Forward,
    /// The same server and room as the one asked about: it takes its place.
    Replace,
    /// Another while a question is open: not opened, and the question says so.
    SetAside,
    /// Opened as a link at start is.
    Open,
}

pub fn arrival(link: &Link, open: Option<&LinkOpen>, shown: Option<(&str, &str)>) -> Arrival {
    let origin = link.server.origin();
    if let Some(open) = open.filter(|open| open.asking()) {
        return if open.link.server.origin() == origin && open.link.room == link.room {
            Arrival::Replace
        } else {
            Arrival::SetAside
        };
    }
    if shown == Some((origin.as_str(), link.room.as_str())) {
        return Arrival::Forward;
    }
    Arrival::Open
}

/// What leaving what this copy shows for a link touches, beside the login screen's.
#[derive(SystemParam)]
pub struct Leaving<'w> {
    screen: ResMut<'w, Screen>,
    signing: ResMut<'w, Signing>,
    watching: ResMut<'w, WatchStatus>,
}

/// Leaves the game or the main screen for the link's server, through the login screen's check
/// as at start; the room is opened once connected.
fn leave(
    open: &LinkOpen,
    net: &Net,
    login: &mut LoginScreen,
    status: &mut ServerStatus,
    pending: &mut WatchOnStart,
    leaving: &mut Leaving,
) {
    if leaving.watching.is_watching() {
        stop_watching(net, &mut leaving.watching);
    }
    pending.set(open.target());
    login.address = open.address();
    if *leaving.screen == Screen::Main {
        crate::login::go_to_login(
            net,
            &mut leaving.screen,
            login,
            status,
            &mut leaving.signing,
        );
    } else {
        login.probe(net, true);
    }
}

/// A link another copy handed over (`link_channel.rs`), one a frame: read again by the one
/// reader, then the same questions as a link at start.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system takes what it touches as parameters"
)]
fn take_handed(
    mut commands: Commands,
    inbox: Res<Inbox>,
    net: Res<Net>,
    open: Option<ResMut<LinkOpen>>,
    mut forward: ResMut<Forward>,
    mut login: ResMut<LoginScreen>,
    mut status: ResMut<ServerStatus>,
    mut pending: ResMut<WatchOnStart>,
    mut leaving: Leaving,
) {
    let Ok(link) = inbox.0.lock().map(|links| links.try_recv()) else {
        return;
    };
    let Ok(link) = link else { return };
    forward.0 = true;
    let origin = ServerUrl::parse(&status.server)
        .ok()
        .map(|url| url.origin());
    let watching = &leaving.watching;
    let room = watching
        .table
        .as_ref()
        .map(|table| table.room_code.clone())
        .or_else(|| match &watching.target {
            Some(Target::Room(code) | Target::Join(code)) => Some(code.clone()),
            _ => None,
        });
    let shown = match (*leaving.screen, &origin, &room) {
        (Screen::Main, Some(origin), Some(room)) => Some((origin.as_str(), room.as_str())),
        _ => None,
    };
    let busy = watching
        .table
        .as_ref()
        .is_some_and(|table| table.yours().is_some() && !table.finished());
    match arrival(&link, open.as_deref(), shown) {
        Arrival::Forward => info!("game links: the link is for the game shown"),
        Arrival::SetAside => {
            info!("game links: a link was set aside: a question is open");
            if let Some(mut open) = open {
                open.set_aside = true;
            }
        }
        Arrival::Replace => {
            let Some(mut open) = open else { return };
            let code = link.code.clone();
            open.link = link;
            if let Stage::AskIdentity { would, .. } = &mut open.stage {
                match code {
                    Some(code) => *would = code.kind(),
                    // Nothing to trade: it goes on as the login screen's check says.
                    None => {
                        open.stage = Stage::Probing;
                        login.set_changed();
                    }
                }
            }
        }
        Arrival::Open => {
            let used = crate::config::Store::load(crate::config::Store::default_dir())
                .has_used(&link.server.origin());
            let open = LinkOpen::handed(link, used, busy);
            if open.stage == Stage::Probing {
                leave(
                    &open,
                    &net,
                    &mut login,
                    &mut status,
                    &mut pending,
                    &mut leaving,
                );
            }
            commands.insert_resource(open);
        }
    }
}

/// Once the login screen's check of the link's server is good: ask, trade or go in.
fn advance(
    net: Res<Net>,
    mut open: ResMut<LinkOpen>,
    mut login: ResMut<LoginScreen>,
    mut status: ResMut<ServerStatus>,
    mut pending: ResMut<WatchOnStart>,
) {
    if open.stage != Stage::Probing || !login.is_changed() {
        return;
    }
    // The player typed another server: the link is not for it.
    if login.address.trim() != open.address() {
        info!("the link is set aside: another server was chosen");
        open.stage = Stage::Done;
        pending.clear();
        return;
    }
    let Light::Good(probe) = &login.light else {
        return;
    };
    if probe.server != open.address() {
        return;
    }
    let identity = match (&probe.account, probe.guest_key) {
        (Some(account), _) => Some(Identity::Account(account.handle.clone())),
        (None, true) => Some(Identity::Guest),
        (None, false) => None,
    };
    match link::decide(&open.link, open.used, true, identity.as_ref(), false) {
        Next::AskIdentity { now, would } => open.stage = Stage::AskIdentity { now, would },
        Next::Trade => {
            if let Some(code) = open.link.code.take() {
                net.send(Request::TradeLinkCode { code });
            }
            open.stage = Stage::Done;
            go(&net, &mut login, &mut status);
        }
        // Asked already, before the probe was sent.
        Next::Join | Next::AskServer => {
            open.link.code = None;
            open.stage = Stage::Done;
            go(&net, &mut login, &mut status);
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer takes what it touches as parameters"
)]
fn on_answer(
    activate: On<Activate>,
    actions: Query<&Action>,
    net: Res<Net>,
    open: Option<ResMut<LinkOpen>>,
    mut setting: ResMut<LinkSetting>,
    mut login: ResMut<LoginScreen>,
    mut status: ResMut<ServerStatus>,
    mut pending: ResMut<WatchOnStart>,
    mut leaving: Leaving,
) {
    let Ok(Action::Link(answer)) = actions.get(activate.entity) else {
        return;
    };
    if let Answer::OpenLinks(on) = answer {
        setting.set(&net, *on);
        return;
    }
    let Some(mut open) = open else { return };
    match answer {
        Answer::Leave if open.stage == Stage::AskLeave => {
            if open.used {
                open.stage = Stage::Probing;
                leave(
                    &open,
                    &net,
                    &mut login,
                    &mut status,
                    &mut pending,
                    &mut leaving,
                );
            } else {
                open.stage = Stage::AskServer;
            }
        }
        // Handed over while running, and nothing left yet: stay as it was.
        Answer::Cancel
            if open.running && matches!(open.stage, Stage::AskLeave | Stage::AskServer) =>
        {
            open.stage = Stage::Done;
            open.link.code = None;
        }
        Answer::Connect if open.stage == Stage::AskServer && open.running => {
            open.stage = Stage::Probing;
            leave(
                &open,
                &net,
                &mut login,
                &mut status,
                &mut pending,
                &mut leaving,
            );
        }
        Answer::Connect if open.stage == Stage::AskServer => {
            login.address = open.address();
            open.stage = Stage::Probing;
            login.probe(&net, true);
        }
        Answer::Cancel => {
            open.stage = Stage::Done;
            open.link.code = None;
            pending.clear();
            login.probe(&net, true);
        }
        Answer::Switch if matches!(open.stage, Stage::AskIdentity { .. }) => {
            if let Some(code) = open.link.code.take() {
                net.send(Request::TradeLinkCode { code });
            }
            open.stage = Stage::Done;
            go(&net, &mut login, &mut status);
        }
        Answer::Dismiss => open.said = None,
        Answer::Stay if matches!(open.stage, Stage::AskIdentity { .. }) => {
            open.link.code = None;
            open.stage = Stage::Done;
            go(&net, &mut login, &mut status);
        }
        _ => {}
    }
}

#[derive(Component)]
struct LinkPanel;

#[derive(Component)]
struct LinkArea;

fn spawn_panel(mut commands: Commands) {
    commands.spawn((
        LinkPanel,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        // Over the login screen.
        GlobalZIndex(60),
        children![(
            LinkArea,
            Node {
                width: px(460),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(10),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(12)),
                ..default()
            },
            // Opaque: the interface is blended in linear light (see `browser.rs`).
            BackgroundColor(Color::srgb(0.07, 0.08, 0.10)),
            BorderColor::all(colors::BORDER),
        )],
    ));
}

/// A question: its title, its words, and its buttons.
type Asked = (String, String, Vec<(String, Answer)>);

/// What the panel asks now, if anything: a title, words, and the buttons.
fn asked(open: Option<&LinkOpen>, setting: &LinkSetting, screen: Screen) -> Option<Asked> {
    if let Some(open) = open {
        if let Some((title, body)) = &open.said {
            return Some((
                title.clone(),
                body.clone(),
                vec![("OK".into(), Answer::Dismiss)],
            ));
        }
        // A second link while this is asked: said under the question.
        let aside = |body: String| {
            if open.set_aside {
                format!("{body}\n\nAnother link was not opened: open it again after this.")
            } else {
                body
            }
        };
        match &open.stage {
            Stage::AskLeave => {
                return Some((
                    "Leave this game?".into(),
                    aside(format!(
                        "A link opens game {} on {}.",
                        open.link.room, open.link.server
                    )),
                    vec![
                        ("Leave".into(), Answer::Leave),
                        ("Stay".into(), Answer::Cancel),
                    ],
                ));
            }
            Stage::AskServer => {
                let (title, body) = link::server_question(&open.link);
                let body = aside(body);
                return Some((
                    title,
                    body,
                    vec![
                        ("Connect".into(), Answer::Connect),
                        ("Cancel".into(), Answer::Cancel),
                    ],
                ));
            }
            Stage::AskIdentity { now, would } => {
                let (title, body) = link::identity_question(&open.link.server, now, *would);
                let body = aside(body);
                return Some((
                    title,
                    body,
                    vec![
                        ("Switch".into(), Answer::Switch),
                        (link::stay_label(now), Answer::Stay),
                    ],
                ));
            }
            Stage::Probing | Stage::Done => {}
        }
    }
    (setting.offer && screen == Screen::Main).then(|| {
        (
            "Open game links here?".to_string(),
            "A server's Join with Bingle3D would then open this program.".to_string(),
            vec![
                ("Open them here".into(), Answer::OpenLinks(true)),
                ("Not now".into(), Answer::OpenLinks(false)),
            ],
        )
    })
}

fn rebuild(
    mut commands: Commands,
    open: Option<Res<LinkOpen>>,
    setting: Res<LinkSetting>,
    screen: Res<Screen>,
    fonts: Res<UiFonts>,
    mut panel: Single<&mut Node, With<LinkPanel>>,
    area: Single<Entity, With<LinkArea>>,
) {
    let shown = asked(open.as_deref(), &setting, *screen);
    panel.display = if shown.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    let f = &*fonts;
    commands
        .entity(*area)
        .despawn_related::<Children>()
        .with_children(|area| {
            let Some((title, body, answers)) = shown else {
                return;
            };
            area.spawn(line(f.semibold(20.0), colors::TEXT, &title));
            area.spawn(line(f.regular(14.0), colors::MUTED, &body));
            area.spawn(buttons()).with_children(|row| {
                for (label, answer) in answers {
                    row.spawn(button(f, &label, Action::Link(answer)));
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(code: bool) -> Link {
        let code = if code {
            "&code=bgj_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
        } else {
            ""
        };
        Link::parse(&format!(
            "bingle-3d://join?server=https%3A%2F%2Fgames.example.com&room=K7MQ2X{code}"
        ))
        .unwrap()
    }

    fn setting(offer: bool) -> LinkSetting {
        LinkSetting {
            shown: true,
            on: false,
            offer,
            problem: None,
        }
    }

    #[test]
    fn a_server_never_used_is_shown_before_anything() {
        let open = LinkOpen::new(link(true), false);
        assert_eq!(open.stage, Stage::AskServer);
        let (title, _, answers) = asked(Some(&open), &setting(false), Screen::Login).unwrap();
        assert!(title.contains("https://games.example.com"));
        assert_eq!(answers[0].1, Answer::Connect);
        assert_eq!(LinkOpen::new(link(false), true).stage, Stage::Probing);
    }

    #[test]
    fn the_identity_question_offers_to_stay() {
        let mut open = LinkOpen::new(link(true), true);
        open.stage = Stage::AskIdentity {
            now: Identity::Account("ana".into()),
            would: link::Kind::Guest,
        };
        let (title, _, answers) = asked(Some(&open), &setting(true), Screen::Main).unwrap();
        assert!(title.contains("ana"), "the link's question comes first");
        assert_eq!(answers[1], ("Stay as ana".into(), Answer::Stay));
    }

    #[test]
    fn what_the_trade_came_to_is_said_until_closed() {
        let mut open = LinkOpen::new(link(true), true);
        open.stage = Stage::Done;
        open.said = Some(("The link did not change who you are".into(), "x".into()));
        let (title, _, answers) = asked(Some(&open), &setting(false), Screen::Main).unwrap();
        assert_eq!(title, "The link did not change who you are");
        assert_eq!(answers, [("OK".to_string(), Answer::Dismiss)]);
    }

    #[test]
    fn a_link_handed_over_asks_before_leaving_a_game() {
        let open = LinkOpen::handed(link(false), true, true);
        assert_eq!(open.stage, Stage::AskLeave);
        let (title, body, answers) = asked(Some(&open), &setting(false), Screen::Main).unwrap();
        assert_eq!(title, "Leave this game?");
        assert!(body.contains("K7MQ2X") && body.contains("https://games.example.com"));
        assert_eq!(answers[0].1, Answer::Leave);
        assert_eq!(answers[1], ("Stay".to_string(), Answer::Cancel));
        // Not busy: a server never used is asked about, one used goes at once.
        assert_eq!(
            LinkOpen::handed(link(true), false, false).stage,
            Stage::AskServer
        );
        assert_eq!(
            LinkOpen::handed(link(true), true, false).stage,
            Stage::Probing
        );
        assert!(LinkOpen::handed(link(true), true, false).running);
        assert!(!LinkOpen::new(link(true), true).running);
    }

    #[test]
    fn what_a_link_handed_over_does() {
        let room_elsewhere =
            Link::parse("bingle-3d://watch?server=https%3A%2F%2Fgames.example.com&room=ZZZZ")
                .unwrap();
        let shown = Some(("https://games.example.com:443", "K7MQ2X"));
        // The game shown: only the window comes forward.
        assert_eq!(arrival(&link(true), None, shown), Arrival::Forward);
        assert_eq!(arrival(&room_elsewhere, None, shown), Arrival::Open);
        assert_eq!(arrival(&link(false), None, None), Arrival::Open);
        // A question open: the same server and room takes its place; another is set aside.
        let asking = LinkOpen::handed(link(false), true, true);
        assert_eq!(arrival(&link(true), Some(&asking), None), Arrival::Replace);
        assert_eq!(
            arrival(&room_elsewhere, Some(&asking), shown),
            Arrival::SetAside
        );
        // A link gone on already asks nothing.
        let mut done = LinkOpen::handed(link(false), true, false);
        done.stage = Stage::Done;
        assert_eq!(arrival(&room_elsewhere, Some(&done), None), Arrival::Open);
    }

    #[test]
    fn a_link_set_aside_is_said_under_the_question() {
        let mut open = LinkOpen::handed(link(false), false, false);
        open.set_aside = true;
        let (_, body, _) = asked(Some(&open), &setting(false), Screen::Main).unwrap();
        assert!(body.ends_with("Another link was not opened: open it again after this."));
    }

    #[test]
    fn the_offer_is_made_on_the_main_screen_until_answered() {
        assert!(asked(None, &setting(true), Screen::Login).is_none());
        let (_, _, answers) = asked(None, &setting(true), Screen::Main).unwrap();
        assert_eq!(answers[0].1, Answer::OpenLinks(true));
        assert!(asked(None, &setting(false), Screen::Main).is_none());
        let mut done = LinkOpen::new(link(false), true);
        done.stage = Stage::Done;
        assert!(asked(Some(&done), &setting(false), Screen::Login).is_none());
    }
}
