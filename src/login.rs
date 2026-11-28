//! The login screen, shown at start before the main screen: which server, with a light saying
//! whether it is there and usable, then logging in to an account or going on as a guest.
//!
//! - The server's address is checked when the player leaves its field (or presses Enter in it,
//!   and once at start): the network thread switches to it and reads its index, checking the
//!   API version as connecting does (`Request::Probe`). Green: this client can use it. Yellow:
//!   a Bingle answered, but its API and this client's do not go together (one of them is out of
//!   date). Red: nothing usable answered (or its certificate is not trusted, which can be
//!   trusted here by its fingerprint).
//! - *Log in* signs in with the username and password (and a second factor's code when the
//!   account has one), as `sign_in.rs` does. *Continue as guest* ends any account's session
//!   kept here, takes the guest key typed (optional; checked with the server first), else the
//!   one kept for that server, else asks the server for a new one.
//! - *Auto-login*: the next start skips this screen and goes in with what is kept: the
//!   account's session token, or the guest key. A password is never kept.
//!
//! Once connected, the main screen shows, and the address (if the player typed it) and the
//! auto-login choice are remembered in `config.toml`. *Logout* in the escape menu comes back
//! here.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::tab_navigation::{TabGroup, TabIndex};
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, Button};

use crate::browser::{Action, button, buttons, colors, line};
use crate::fonts::UiFonts;
use crate::net::session::{Account, Probe, Problem};
use crate::net::tls::Fingerprint;
use crate::net::{Connection, Net, Request, ServerStatus};
use crate::sign_in::{self, Signing, Step, text_field};

pub struct LoginPlugin;

impl Plugin for LoginPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_login_action)
            .add_systems(Startup, spawn_panel)
            .add_systems(Update, show_screen.run_if(resource_changed::<Screen>))
            .add_systems(
                Update,
                (
                    open_form,
                    keep_typed,
                    probe_on_leaving_the_address,
                    enter_when_connected,
                    rebuild_panel,
                    rebuild_light,
                    rebuild_messages,
                    tick_box,
                )
                    .chain()
                    .run_if(on_login),
            );
    }
}

/// Which screen is up: the login screen, or the main screen (with the game, when one is open).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Login,
    Main,
}

fn on_login(screen: Res<Screen>) -> bool {
    *screen == Screen::Login
}

/// Shown only beside the main screen: hidden on the login screen.
#[derive(Component, Default)]
pub struct MainOnly;

/// How the server at the address looks.
#[derive(Debug, Clone, PartialEq)]
pub enum Light {
    /// Being asked.
    Checking,
    /// There, and this client can use it.
    Good(Probe),
    /// A Bingle, whose API and this client's do not go together.
    Mismatch(Problem),
    /// Not there, or not usable.
    Down(Problem),
}

impl Light {
    fn from(result: Result<Probe, Problem>) -> Self {
        match result {
            Ok(probe) => Light::Good(probe),
            Err(problem @ Problem::Incompatible(_)) => Light::Mismatch(problem),
            Err(problem) => Light::Down(problem),
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Light::Checking => colors::MUTED,
            Light::Good(_) => colors::GOOD,
            Light::Mismatch(_) => colors::WARN,
            Light::Down(_) => colors::BAD,
        }
    }
}

/// The login screen's state.
#[derive(Resource, Debug)]
pub struct LoginScreen {
    /// The server's address, as typed.
    pub address: String,
    /// The player typed the address (so it is remembered), rather than it being the one kept
    /// or given on the command line.
    edited: bool,
    /// The address last asked about, and the number of that question.
    probed: String,
    asked: u64,
    pub light: Light,
    /// A guest key typed in, to go on as that guest.
    pub guest_key: String,
    pub auto_login: bool,
    /// Logging in or going on as a guest: waiting to be connected.
    pub entering: bool,
    /// Why the last attempt to go in failed.
    pub problem: Option<Problem>,
}

impl LoginScreen {
    pub fn new(address: &str, auto_login: bool) -> Self {
        LoginScreen {
            address: address.to_string(),
            edited: false,
            probed: String::new(),
            asked: 0,
            light: Light::Checking,
            guest_key: String::new(),
            auto_login,
            entering: false,
            problem: None,
        }
    }

    /// Asks about the address, if it changed since it was last asked about (or `again`).
    pub fn probe(&mut self, net: &Net, again: bool) {
        let address = self.address.trim().to_string();
        if !again && address == self.probed {
            return;
        }
        self.asked += 1;
        self.probed = address.clone();
        self.light = Light::Checking;
        net.send(Request::Probe {
            address,
            asked: self.asked,
        });
    }

    /// The answer about the address; one to an older question is dropped.
    pub fn probed(&mut self, asked: u64, result: Result<Probe, Problem>) {
        if asked == self.asked {
            self.light = Light::from(result);
        }
    }

    /// The address typed changed.
    fn typed_address(&mut self, address: &str) {
        if self.address != address {
            self.address = address.to_string();
            self.edited = true;
        }
    }

    /// The guest key to go on with: the one typed, if any.
    fn key(&self) -> Option<String> {
        let key = self.guest_key.trim();
        (!key.is_empty()).then(|| key.to_string())
    }

    /// The account the kept session belongs to on the server, if it is still good.
    fn kept_account(&self) -> Option<&Account> {
        match &self.light {
            Light::Good(probe) => probe.account.as_ref(),
            _ => None,
        }
    }
}

/// What the login screen's buttons do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginAction {
    /// Go on as a guest.
    Guest,
    /// Go on as the account whose session is kept.
    KeptAccount,
    /// Check the address again.
    Check,
    /// Trust the server's certificate, by its fingerprint, and check it again.
    Trust(Fingerprint),
    /// Turn auto-login on or off.
    AutoLogin,
    /// Go back to the guest a link set aside here, and go in as it.
    RestoreGuest,
}

/// Goes to the login screen: from the main screen's *Sign in*, or the escape menu's *Logout*.
pub fn go_to_login(
    net: &Net,
    screen: &mut Screen,
    login: &mut LoginScreen,
    status: &mut ServerStatus,
    signing: &mut Signing,
) {
    let handle = match &status.connection {
        Connection::Ready(snap) => snap.account.as_ref().map(|a| a.handle.clone()),
        _ => None,
    };
    *screen = Screen::Login;
    status.connection = Connection::Connecting;
    status.retry_in = None;
    login.entering = false;
    login.problem = None;
    login.guest_key.clear();
    signing.open(handle.as_deref().unwrap_or(""));
    login.probe(net, true);
}

/// Starts going in: connected, the main screen shows.
fn enter(status: &mut ServerStatus, login: &mut LoginScreen) {
    status.connection = Connection::Connecting;
    status.retry_in = None;
    login.entering = true;
    login.problem = None;
}

#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer takes what it touches as parameters"
)]
fn on_login_action(
    activate: On<Activate>,
    actions: Query<&Action>,
    net: Res<Net>,
    mut screen: ResMut<Screen>,
    mut login: ResMut<LoginScreen>,
    mut status: ResMut<ServerStatus>,
    mut signing: ResMut<Signing>,
    mut focus: ResMut<InputFocus>,
) {
    let action = match actions.get(activate.entity) {
        Ok(Action::Login(action)) => action,
        Ok(Action::OpenSignIn) => {
            focus.clear();
            go_to_login(&net, &mut screen, &mut login, &mut status, &mut signing);
            return;
        }
        _ => return,
    };
    match action {
        LoginAction::Guest => {
            enter(&mut status, &mut login);
            net.send(Request::ContinueAsGuest { key: login.key() });
        }
        LoginAction::KeptAccount => {
            enter(&mut status, &mut login);
            net.send(Request::Connect);
        }
        LoginAction::Check => login.probe(&net, true),
        LoginAction::Trust(fingerprint) => {
            net.send(Request::Trust(*fingerprint));
            login.probe(&net, true);
        }
        LoginAction::AutoLogin => login.auto_login = !login.auto_login,
        LoginAction::RestoreGuest => {
            enter(&mut status, &mut login);
            net.send(Request::RestoreGuest);
        }
    }
}

/// Connected after logging in or going on as a guest: the main screen, and what to remember.
/// Refused: say why, here.
fn enter_when_connected(
    net: Res<Net>,
    status: Res<ServerStatus>,
    mut login: ResMut<LoginScreen>,
    mut screen: ResMut<Screen>,
    mut signing: ResMut<Signing>,
) {
    if !login.entering || !status.is_changed() {
        return;
    }
    match &status.connection {
        Connection::Ready(_) => {
            login.entering = false;
            net.send(Request::Remember {
                server: login.edited,
                auto_login: login.auto_login,
            });
            signing.form = None;
            *screen = Screen::Main;
        }
        Connection::Failed(problem) => {
            login.entering = false;
            login.problem = Some(problem.clone());
        }
        Connection::Connecting => {}
    }
}

/// The sign-in form stands on the login screen whenever it shows.
fn open_form(mut signing: ResMut<Signing>) {
    if signing.form.is_none() {
        signing.open("");
    }
}

#[derive(Component)]
struct LoginPanel;

/// What the panel's rebuilt parts hang from.
#[derive(Component)]
struct FieldsArea;

#[derive(Component)]
struct LightArea;

#[derive(Component)]
struct MessagesArea;

/// The server address's field, and the guest key's.
#[derive(Component)]
struct AddressField;

#[derive(Component)]
struct GuestKeyField;

/// The auto-login box's tick.
#[derive(Component)]
struct Tick;

fn spawn_panel(mut commands: Commands) {
    commands.spawn((
        LoginPanel,
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
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        GlobalZIndex(50),
        TabGroup::new(0),
        children![(
            FieldsArea,
            Node {
                width: px(400),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(8),
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

/// The login screen shows, or the main screen does.
fn show_screen(
    screen: Res<Screen>,
    mut panel: Single<&mut Node, With<LoginPanel>>,
    mut main_only: Query<&mut Visibility, With<MainOnly>>,
) {
    let login = *screen == Screen::Login;
    panel.display = if login { Display::Flex } else { Display::None };
    for mut visibility in &mut main_only {
        *visibility = if login {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}

/// Which form step the panel was built for, so that it is built again only when that changes
/// (building it again takes the keyboard from the field being typed in).
#[derive(Default, PartialEq)]
struct Built(Option<Step>);

fn rebuild_panel(
    mut commands: Commands,
    screen: Res<Screen>,
    signing: Res<Signing>,
    login: Res<LoginScreen>,
    fonts: Res<UiFonts>,
    area: Single<Entity, With<FieldsArea>>,
    mut built: Local<Built>,
) {
    let Some(form) = &signing.form else { return };
    let step = Some(form.step.clone());
    if built.0 == step && !screen.is_changed() {
        return;
    }
    built.0 = step;
    let f = &*fonts;
    commands
        .entity(*area)
        .despawn_related::<Children>()
        .with_children(|panel| {
            panel.spawn((
                Text::new("Bingle"),
                f.semibold(34.0),
                TextColor(colors::TEXT),
                Node {
                    align_self: AlignSelf::Center,
                    margin: UiRect::bottom(px(8)),
                    ..default()
                },
            ));
            panel.spawn(line(f.regular(14.0), colors::TEXT, "Server"));
            panel
                .spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((AddressField, text_field(f, &login.address), TabIndex(0)))
                        // Across the panel, but for the light.
                        .insert(Node {
                            flex_grow: 1.0,
                            ..sign_in_field_node()
                        })
                        .observe(on_address_key);
                    row.spawn((
                        LightArea,
                        Node {
                            width: px(14),
                            height: px(14),
                            border_radius: BorderRadius::all(px(7)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(login.light.color()),
                    ));
                });
            panel.spawn((
                ServerLine,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                },
            ));
            sign_in::fields(panel, f, form, 1);
            panel.spawn((
                MessagesArea,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                },
            ));
            panel.spawn(buttons()).with_children(|row| {
                row.spawn(button(f, "Log in", Action::SubmitSignIn));
                if matches!(form.step, Step::Password) {
                    row.spawn(button(
                        f,
                        "Continue as guest",
                        Action::Login(LoginAction::Guest),
                    ));
                }
                sign_in::code_choice(row, f, form);
            });
            if matches!(form.step, Step::Password) {
                panel.spawn(line(f.regular(14.0), colors::TEXT, "Guest key (optional)"));
                panel
                    .spawn((GuestKeyField, text_field(f, &login.guest_key), TabIndex(3)))
                    .observe(on_guest_key_key);
                panel.spawn((GuestKeyHint, line(f.regular(12.0), colors::MUTED, "")));
            }
            panel.spawn(auto_login_box(f, login.auto_login));
            panel.spawn(line(f.regular(12.0), colors::MUTED, AUTO_LOGIN_HINT));
        });
}

fn sign_in_field_node() -> Node {
    Node {
        height: px(32),
        padding: UiRect::axes(px(8), px(5)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(6)),
        align_items: AlignItems::Center,
        overflow: Overflow::clip(),
        ..default()
    }
}

/// The line under the address: what the server is, or what is wrong with it.
#[derive(Component)]
struct ServerLine;

/// What *Continue as guest* without a key will do.
#[derive(Component)]
struct GuestKeyHint;

fn auto_login_box(f: &UiFonts, on: bool) -> impl Bundle {
    (
        Button,
        Action::Login(LoginAction::AutoLogin),
        Hovered::default(),
        Node {
            align_items: AlignItems::Center,
            column_gap: px(8),
            margin: UiRect::top(px(6)),
            ..default()
        },
        children![
            (
                Node {
                    width: px(18),
                    height: px(18),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(colors::FIELD),
                BorderColor::all(colors::FIELD_EDGE),
                Pickable::IGNORE,
                children![(
                    Tick,
                    Text::new(if on { "✓" } else { "" }),
                    f.semibold(14.0),
                    TextColor(colors::GOOD),
                    Pickable::IGNORE,
                )],
            ),
            (
                Text::new("Auto-login"),
                f.regular(14.0),
                TextColor(colors::TEXT),
                Pickable::IGNORE,
            ),
        ],
    )
}

/// What auto-login uses.
const AUTO_LOGIN_HINT: &str = "Next time, straight to the main screen with what is kept here: your session, or your \
     guest key. Never a password.";

fn tick_box(login: Res<LoginScreen>, mut ticks: Query<&mut Text, With<Tick>>) {
    let shown = if login.auto_login { "✓" } else { "" };
    for mut tick in &mut ticks {
        if tick.0 != shown {
            tick.0 = shown.into();
        }
    }
}

/// The light, the line under the address, and the guest key's hint, when what is known of the
/// server changes.
fn rebuild_light(
    mut commands: Commands,
    login: Res<LoginScreen>,
    fonts: Res<UiFonts>,
    mut lights: Query<&mut BackgroundColor, With<LightArea>>,
    lines: Query<Entity, With<ServerLine>>,
    mut hints: Query<&mut Text, With<GuestKeyHint>>,
    added: Query<(), Added<ServerLine>>,
) {
    if !login.is_changed() && added.is_empty() {
        return;
    }
    for mut light in &mut lights {
        if light.0 != login.light.color() {
            light.0 = login.light.color();
        }
    }
    let hint = match &login.light {
        Light::Good(probe) if probe.guest_key => {
            "Left empty, this computer's guest key for the server is used."
        }
        Light::Good(_) => "Left empty, the server gives you a new one.",
        _ => "To go on as a guest you have been before.",
    };
    for mut text in &mut hints {
        if text.0 != hint {
            text.0 = hint.into();
        }
    }
    let f = &*fonts;
    for area in &lines {
        commands
            .entity(area)
            .despawn_related::<Children>()
            .with_children(|area| server_line(area, f, &login.light));
    }
}

fn server_line(area: &mut ChildSpawnerCommands, f: &UiFonts, light: &Light) {
    match light {
        Light::Checking => {
            area.spawn(line(f.regular(12.0), colors::MUTED, "Checking…"));
        }
        Light::Good(probe) => {
            area.spawn(line(
                f.regular(12.0),
                colors::GOOD,
                &format!("Bingle {} (API {})", probe.version, probe.api_version),
            ));
            if let Some(account) = &probe.account {
                area.spawn(line(
                    f.regular(12.0),
                    colors::MUTED,
                    &format!("Still signed in here as {}.", account.handle),
                ));
            }
        }
        Light::Mismatch(problem) | Light::Down(problem) => {
            area.spawn(line(f.regular(12.0), light.color(), &problem.message()));
            area.spawn(buttons()).with_children(|row| {
                if let Problem::Untrusted { fingerprint, .. } = problem {
                    area_fingerprint(row, f, fingerprint);
                }
                row.spawn(button(f, "Check again", Action::Login(LoginAction::Check)));
            });
        }
    }
}

fn area_fingerprint(row: &mut ChildSpawnerCommands, f: &UiFonts, fingerprint: &Fingerprint) {
    row.spawn(line(
        f.regular(12.0),
        colors::TEXT,
        &format!("SHA-256: {}", fingerprint.two_lines()),
    ));
    row.spawn(button(
        f,
        "Trust this certificate",
        Action::Login(LoginAction::Trust(*fingerprint)),
    ));
}

/// What going in is doing, and why it failed.
fn rebuild_messages(
    mut commands: Commands,
    login: Res<LoginScreen>,
    signing: Res<Signing>,
    status: Res<ServerStatus>,
    fonts: Res<UiFonts>,
    areas: Query<Entity, With<MessagesArea>>,
    added: Query<(), Added<MessagesArea>>,
) {
    if !login.is_changed() && !signing.is_changed() && !status.is_changed() && added.is_empty() {
        return;
    }
    let f = &*fonts;
    let form = signing.form.as_ref();
    for area in &areas {
        commands
            .entity(area)
            .despawn_related::<Children>()
            .with_children(|area| {
                if let Some(problem) = form.and_then(|form| form.problem.as_ref()) {
                    area.spawn(line(f.regular(13.0), colors::WARN, &problem.message()));
                }
                if let Some(problem) = &login.problem {
                    area.spawn(line(f.regular(13.0), colors::WARN, &problem.message()));
                }
                // What the client could not read or keep, a setup file among it: said here
                // too, since this is the screen a file that was not taken leaves the player on.
                for notice in &status.notices {
                    area.spawn(line(f.regular(13.0), colors::WARN, notice));
                }
                if form.is_some_and(|form| form.busy) {
                    area.spawn(line(f.regular(13.0), colors::MUTED, "Logging in…"));
                } else if login.entering {
                    area.spawn(line(f.regular(13.0), colors::MUTED, "Connecting…"));
                }
                if let Some(account) = login.kept_account()
                    && !login.entering
                {
                    area.spawn(buttons()).with_children(|row| {
                        row.spawn(button(
                            f,
                            &format!("Continue as {}", account.handle),
                            Action::Login(LoginAction::KeptAccount),
                        ));
                    });
                }
                // The guest a link's code took the place of: never lost, and taken back here.
                if matches!(&login.light, Light::Good(probe) if probe.set_aside) && !login.entering
                {
                    area.spawn(buttons()).with_children(|row| {
                        row.spawn(button(
                            f,
                            "Back to your earlier guest",
                            Action::Login(LoginAction::RestoreGuest),
                        ));
                    });
                }
            });
    }
}

type TypedFields<'w, 's> = Query<
    'w,
    's,
    (&'static EditableText, Has<AddressField>),
    (
        Changed<EditableText>,
        Or<(With<AddressField>, With<GuestKeyField>)>,
    ),
>;

/// What is typed in the address's and the guest key's fields, kept, so that the panel built
/// again shows it.
fn keep_typed(mut login: ResMut<LoginScreen>, fields: TypedFields) {
    for (text, address) in &fields {
        let login = login.bypass_change_detection();
        let typed = text.value().to_string();
        if address {
            login.typed_address(&typed);
        } else if login.guest_key != typed {
            login.guest_key = typed;
        }
    }
}

/// The address is checked once the player leaves its field for another (or for nothing).
fn probe_on_leaving_the_address(
    focus: Res<InputFocus>,
    net: Res<Net>,
    mut login: ResMut<LoginScreen>,
    fields: Query<Entity, With<AddressField>>,
    mut was_in: Local<bool>,
) {
    let in_it = focus.get().is_some_and(|e| fields.contains(e));
    if *was_in && !in_it {
        login.probe(&net, false);
    }
    *was_in = in_it;
}

/// Enter in the address checks it.
fn on_address_key(
    mut key: On<FocusedInput<KeyboardInput>>,
    net: Res<Net>,
    mut login: ResMut<LoginScreen>,
) {
    if key.input.state == ButtonState::Pressed && key.input.logical_key == Key::Enter {
        key.propagate(false);
        login.probe(&net, false);
    }
}

/// Enter in the guest key goes on as that guest.
fn on_guest_key_key(
    mut key: On<FocusedInput<KeyboardInput>>,
    net: Res<Net>,
    mut login: ResMut<LoginScreen>,
    mut status: ResMut<ServerStatus>,
) {
    if key.input.state == ButtonState::Pressed && key.input.logical_key == Key::Enter {
        key.propagate(false);
        enter(&mut status, &mut login);
        net.send(Request::ContinueAsGuest { key: login.key() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(account: Option<&str>) -> Probe {
        Probe {
            server: "http://127.0.0.1:8080".into(),
            version: "0.122.2".into(),
            api_version: "1.7.0".into(),
            account: account.map(|h| Account {
                handle: h.into(),
                display_name: String::new(),
                must_change_password: false,
            }),
            guest_key: false,
            set_aside: false,
        }
    }

    #[test]
    fn the_light_is_green_yellow_or_red() {
        assert_eq!(Light::from(Ok(probe(None))).color(), colors::GOOD);
        let old = Problem::Incompatible("too old".into());
        assert_eq!(Light::from(Err(old)).color(), colors::WARN);
        for down in [
            Problem::Unreachable("nothing".into()),
            Problem::NotBingle("a web page".into()),
            Problem::BadAddress("ftp://x".into()),
        ] {
            assert_eq!(Light::from(Err(down)).color(), colors::BAD);
        }
        assert_eq!(Light::Checking.color(), colors::MUTED);
    }

    #[test]
    fn an_address_is_asked_about_once_and_an_old_answer_dropped() {
        let (net, requests) = Net::for_test();
        let mut login = LoginScreen::new("localhost:8080", false);
        login.probe(&net, false);
        login.probe(&net, false);
        let asked: Vec<Request> = requests.try_iter().collect();
        assert_eq!(
            asked,
            [Request::Probe {
                address: "localhost:8080".into(),
                asked: 1
            }],
            "the same address is not asked about twice"
        );
        login.typed_address("https://games.example.com ");
        login.probe(&net, false);
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::Probe { asked: 2, ref address }) if address == "https://games.example.com"
        ));
        assert_eq!(login.light, Light::Checking);
        login.probed(1, Ok(probe(None)));
        assert_eq!(
            login.light,
            Light::Checking,
            "the answer about the old address"
        );
        login.probed(2, Ok(probe(Some("ana"))));
        assert_eq!(login.kept_account().map(|a| a.handle.as_str()), Some("ana"));
        assert!(login.edited, "an address typed is remembered");
    }

    #[test]
    fn the_guest_key_typed_is_optional() {
        let mut login = LoginScreen::new("localhost:8080", false);
        assert_eq!(login.key(), None, "none: the kept one, or a new one");
        login.guest_key = "  bgg_abc ".into();
        assert_eq!(login.key().as_deref(), Some("bgg_abc"));
    }

    fn app() -> (App, std::sync::mpsc::Receiver<Request>) {
        let (net, requests) = Net::for_test();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(net)
            .insert_resource(Screen::Login)
            .insert_resource(LoginScreen::new("localhost:8080", true))
            .init_resource::<Signing>()
            .init_resource::<InputFocus>()
            .insert_resource(ServerStatus {
                server: "http://localhost:8080".into(),
                connection: Connection::Connecting,
                notices: vec![],
                retry_in: None,
            })
            .add_observer(on_login_action)
            .add_systems(Update, enter_when_connected);
        (app, requests)
    }

    fn press(app: &mut App, action: Action) {
        let button = app.world_mut().spawn(action).id();
        app.world_mut().trigger(Activate { entity: button });
    }

    fn snapshot() -> crate::net::session::Snapshot {
        crate::net::session::Snapshot {
            server: "http://localhost:8080".into(),
            version: "1".into(),
            api_version: "1.7.0".into(),
            defs: vec![],
            games: vec![],
            own: vec![],
            account: None,
            session_ended: false,
        }
    }

    #[test]
    fn a_guest_goes_in_once_connected_and_what_was_chosen_is_remembered() {
        let (mut app, requests) = app();
        app.world_mut().resource_mut::<LoginScreen>().guest_key = "bgg_abc".into();
        press(&mut app, Action::Login(LoginAction::Guest));
        assert_eq!(
            requests.try_recv(),
            Ok(Request::ContinueAsGuest {
                key: Some("bgg_abc".into())
            })
        );
        app.update();
        assert_eq!(*app.world().resource::<Screen>(), Screen::Login, "not yet");
        app.world_mut().resource_mut::<ServerStatus>().connection = Connection::Ready(snapshot());
        app.update();
        assert_eq!(*app.world().resource::<Screen>(), Screen::Main);
        assert_eq!(
            requests.try_recv(),
            Ok(Request::Remember {
                server: false,
                auto_login: true
            })
        );
    }

    #[test]
    fn a_refusal_stays_on_the_login_screen_and_says_why() {
        let (mut app, _requests) = app();
        press(&mut app, Action::Login(LoginAction::Guest));
        app.update();
        let refused = Problem::Refused {
            status: 401,
            message: "the server does not know that guest key".into(),
        };
        app.world_mut().resource_mut::<ServerStatus>().connection =
            Connection::Failed(refused.clone());
        app.update();
        assert_eq!(*app.world().resource::<Screen>(), Screen::Login);
        let login = app.world().resource::<LoginScreen>();
        assert!(!login.entering);
        assert_eq!(login.problem, Some(refused));
    }

    #[test]
    fn sign_in_on_the_main_screen_comes_back_here_and_checks_the_server() {
        let (mut app, requests) = app();
        *app.world_mut().resource_mut::<Screen>() = Screen::Main;
        app.world_mut().resource_mut::<ServerStatus>().connection = Connection::Ready(snapshot());
        press(&mut app, Action::OpenSignIn);
        assert_eq!(*app.world().resource::<Screen>(), Screen::Login);
        assert!(app.world().resource::<Signing>().form.is_some());
        assert!(matches!(requests.try_recv(), Ok(Request::Probe { .. })));
        assert_eq!(
            app.world().resource::<ServerStatus>().connection,
            Connection::Connecting,
            "nothing of the main screen stands while logged out"
        );
    }

    #[test]
    fn auto_login_is_a_toggle() {
        let (mut app, _requests) = app();
        press(&mut app, Action::Login(LoginAction::AutoLogin));
        assert!(!app.world().resource::<LoginScreen>().auto_login);
        press(&mut app, Action::Login(LoginAction::AutoLogin));
        assert!(app.world().resource::<LoginScreen>().auto_login);
    }
}
