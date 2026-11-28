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
//! On the main screen of a first run (not one a script limits, nor in a headset), a question
//! offers to open game links with this program; *Settings* turns it on and off after that.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::browser::{Action, button, buttons, colors, line};
use crate::fonts::UiFonts;
use crate::game_panel::WatchOnStart;
use crate::link::{self, Identity, Link, Next};
use crate::login::{Light, LoginScreen, Screen};
use crate::net::{Connection, Net, Request, ServerStatus};

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
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
        match &open.stage {
            Stage::AskServer => {
                let (title, body) = link::server_question(&open.link);
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
