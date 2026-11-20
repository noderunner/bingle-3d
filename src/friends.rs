//! The signed-in account's friends: who is online, requests in and out, invitations to games,
//! the settings that go with them (Bingle's ADR 0105 and 0106), and direct messages.
//!
//! Messages arrive on the account's socket and are kept per friend, by their address, for as
//! long as the client runs; a conversation opened is read from the server, which keeps the last
//! 200 messages of a pair for two hours. The server keeps no record of what was read, so the
//! messages counted as unread are those that arrived while their conversation was not open.
//!
//! The list lives in [`Friends`], filled by the network thread: when the account's socket opens
//! or says the list changed, and after every change made here. Whether a friend is online is not
//! sent when it changes, so while the panel is open the list is read again every
//! [`REFRESH_EVERY`], as Bingle's own page does.
//!
//! The panel stands on the right of the screen while an account is signed in, during a game
//! too, since invitations come at any time. Closed, it is one button that counts what is waiting.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::tab_navigation::{TabGroup, TabIndex};
use bevy::input_focus::{AutoFocus, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Activate;

use crate::browser::{Action, button, buttons, chip, colours, line};
use crate::call::{Call, DirectCalling, Phase};
use crate::fonts::UiFonts;
use crate::net::api::{DirectMessage, FriendSettings, FriendsList};
use crate::net::session::Problem;
use crate::net::{Connection, FriendCall, Net, Request, ServerStatus, Target, WatchStatus, watch};
use crate::sign_in::text_field;

/// How often the list is read again while the panel is open, for who is online.
pub const REFRESH_EVERY: Duration = Duration::from_secs(10);

/// The longest message the server takes, in characters.
pub const MESSAGE_MAX: usize = 500;

/// How many of a conversation's latest messages the panel shows.
const SHOWN: usize = 40;

pub struct FriendsPlugin {
    /// Open the panel at start (`--friends`).
    pub open: bool,
    /// Open the conversation with this friend, by handle, once the list is read (`--talk`).
    pub talk: Option<String>,
}

impl Plugin for FriendsPlugin {
    fn build(&self, app: &mut App) {
        if self.open {
            app.add_systems(Startup, |mut friends: ResMut<Friends>| friends.open = true);
        }
        app.insert_resource(TalkOnStart(self.talk.clone()));
        app.add_systems(Startup, spawn_panel)
            .add_observer(on_friend_action)
            .add_systems(
                Update,
                (
                    follow_account,
                    follow_game,
                    refresh_while_open,
                    talk_on_start,
                    keep_typed,
                    rebuild.run_if(
                        resource_changed::<Friends>
                            .or_eager(resource_changed::<ServerStatus>)
                            .or_eager(resource_changed::<DirectCalling>),
                    ),
                )
                    .chain(),
            );
    }
}

/// The friends list and the panel's state.
#[derive(Resource, Debug)]
pub struct Friends {
    /// As the server last said; `None` until read, or while nobody is signed in.
    pub list: Option<FriendsList>,
    pub settings: Option<FriendSettings>,
    /// Why the last call or reading went wrong.
    pub problem: Option<Problem>,
    /// The panel shows the list, rather than only its button.
    pub open: bool,
    /// The handle typed to ask.
    pub asking: String,
    /// The friend *Remove* was pressed for once: it asks before it is done.
    pub removing: Option<String>,
    /// The game this client has a seat in that has not started, to invite friends to.
    pub invite_room: Option<String>,
    /// The friends invited to it from here.
    pub invited: BTreeSet<String>,
    /// This account's own address, as its socket said.
    pub address: Option<String>,
    /// What was said with each friend, by their address.
    pub talks: HashMap<String, Talk>,
    /// The friend whose conversation the panel shows, by address.
    pub talking: Option<String>,
    /// The message being written.
    pub writing: String,
    /// A message sent and not answered yet.
    pub sending: bool,
    /// The field that had the keyboard when the panel was last drawn.
    typing: Typing,
    refresh: Timer,
}

/// What was said with one friend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Talk {
    /// Oldest first.
    pub messages: Vec<DirectMessage>,
    /// Arrived from them while the conversation was not open.
    pub unread: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Typing {
    #[default]
    Nowhere,
    Ask,
    Write,
}

impl Default for Friends {
    fn default() -> Self {
        Friends {
            list: None,
            settings: None,
            problem: None,
            open: false,
            asking: String::new(),
            removing: None,
            invite_room: None,
            invited: BTreeSet::new(),
            address: None,
            talks: HashMap::new(),
            talking: None,
            writing: String::new(),
            sending: false,
            typing: Typing::Nowhere,
            refresh: Timer::new(REFRESH_EVERY, TimerMode::Repeating),
        }
    }
}

impl Friends {
    /// Requests to answer, invitations not answered yet and messages not read: what the closed
    /// panel counts.
    pub fn waiting(&self) -> usize {
        let asks = self.list.as_ref().map_or(0, |l| {
            l.incoming.len() + l.invites.iter().filter(|i| i.pending()).count()
        });
        asks + self.talks.values().map(|t| t.unread).sum::<usize>()
    }

    /// Messages from this friend not read yet.
    pub fn unread(&self, address: &str) -> usize {
        self.talks.get(address).map_or(0, |t| t.unread)
    }

    /// Opens the conversation with the friend at `address`: what they said is read.
    pub fn talk_with(&mut self, address: &str) {
        self.talking = Some(address.to_string());
        self.talks.entry(address.to_string()).or_default().unread = 0;
        self.writing.clear();
        self.problem = None;
    }

    /// The server's record of a conversation, with whatever arrived since it was asked for.
    pub fn history(&mut self, with: &str, messages: Vec<DirectMessage>) {
        let talk = self.talks.entry(with.to_string()).or_default();
        for message in messages {
            keep(&mut talk.messages, message);
        }
    }

    /// A message arrived on the account's socket: to this account, or sent by it (from here or
    /// elsewhere).
    pub fn received(&mut self, message: DirectMessage) {
        let mine = self.address.as_deref() == Some(message.from.as_str());
        let with = if mine { &message.to } else { &message.from }.clone();
        let open = self.open && self.talking.as_deref() == Some(with.as_str());
        let talk = self.talks.entry(with).or_default();
        if keep(&mut talk.messages, message) && !mine && !open {
            talk.unread += 1;
        }
    }

    /// The server answered a message sent.
    pub fn written(&mut self, to: &str, result: Result<DirectMessage, Problem>) {
        self.sending = false;
        match result {
            Ok(message) => {
                self.problem = None;
                if self.talking.as_deref() == Some(to) {
                    self.writing.clear();
                }
                keep(
                    &mut self.talks.entry(to.to_string()).or_default().messages,
                    message,
                );
            }
            Err(problem) => self.problem = Some(problem),
        }
    }

    /// The server answered a friends call.
    pub fn done(&mut self, call: &FriendCall, result: Result<FriendsList, Problem>) {
        match result {
            Ok(list) => {
                self.problem = None;
                match call {
                    FriendCall::Ask(_) => self.asking.clear(),
                    FriendCall::Invite { handle, .. } => {
                        self.invited.insert(handle.clone());
                    }
                    FriendCall::Remove(_) => self.removing = None,
                    _ => {}
                }
                self.read(Ok(list));
            }
            Err(problem) => self.problem = Some(problem),
        }
    }

    /// The list was read. Whether that changed anything, so that the panel is only drawn again
    /// when it did.
    pub fn read(&mut self, result: Result<FriendsList, Problem>) -> bool {
        match result {
            Ok(list) if self.list.as_ref() == Some(&list) => false,
            Ok(list) => {
                self.list = Some(list);
                true
            }
            Err(problem) if self.problem.as_ref() == Some(&problem) => false,
            Err(problem) => {
                self.problem = Some(problem);
                true
            }
        }
    }

    /// Nobody is signed in any more.
    fn forget(&mut self) {
        *self = Friends::default();
    }
}

/// Adds a message to a conversation, in order, unless it is there already (a message sent comes
/// back both as the answer and on the socket). Whether it was new.
fn keep(messages: &mut Vec<DirectMessage>, message: DirectMessage) -> bool {
    match messages.binary_search_by_key(&message.id, |m| m.id) {
        Ok(_) => false,
        Err(at) => {
            messages.insert(at, message);
            true
        }
    }
}

/// What a button on the friends panel does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FriendAction {
    /// Show the list, or only its button.
    Toggle,
    /// Ask the handle typed.
    Ask,
    /// A friends call, as it is.
    Call(FriendCall),
    /// *Remove* pressed: the first press asks, the second removes.
    Remove(String),
    /// Not removing after all.
    KeepFriend,
    /// Accept an invitation and join its game.
    Join(String),
    Settings(FriendSettings),
    /// Open the conversation with the friend at this address.
    Talk(String),
    /// Back to the list.
    StopTalking,
    /// Send the message written.
    Send,
    /// Ring the friend at this address (or answer them), by the name they go by.
    Ring {
        address: String,
        name: String,
    },
    /// End the call, or decline it.
    HangUp,
    /// Mute the microphone in the call, or unmute it.
    Mute,
}

/// Sends the message written, to the friend whose conversation is open.
fn send(net: &Net, friends: &mut Friends) {
    let text = friends.writing.trim();
    let Some(to) = friends.talking.clone() else {
        return;
    };
    if text.is_empty() || friends.sending {
        return;
    }
    net.send(Request::Write {
        to,
        text: text.to_string(),
    });
    friends.sending = true;
}

/// Sends what the handle field holds as a friend request.
fn ask(net: &Net, friends: &mut Friends) {
    let handle = friends
        .asking
        .trim()
        .trim_start_matches('@')
        .trim()
        .to_string();
    if !handle.is_empty() {
        net.send(Request::Friend(FriendCall::Ask(handle)));
    }
}

fn on_friend_action(
    activate: On<Activate>,
    actions: Query<&Action>,
    net: Res<Net>,
    mut friends: ResMut<Friends>,
    mut calling: ResMut<DirectCalling>,
) {
    let Ok(Action::Friends(action)) = actions.get(activate.entity) else {
        return;
    };
    match action {
        FriendAction::Toggle => {
            friends.open = !friends.open;
            if friends.open {
                net.send(Request::Friends);
                friends.refresh.reset();
            }
        }
        FriendAction::Ask => ask(&net, &mut friends),
        FriendAction::Call(call) => net.send(Request::Friend(call.clone())),
        FriendAction::Remove(handle) => {
            if friends.removing.as_ref() == Some(handle) {
                net.send(Request::Friend(FriendCall::Remove(handle.clone())));
            } else {
                friends.removing = Some(handle.clone());
            }
        }
        FriendAction::KeepFriend => friends.removing = None,
        FriendAction::Join(room) => net.send(Request::Friend(FriendCall::AnswerInvite {
            room: room.clone(),
            accept: true,
        })),
        FriendAction::Settings(settings) => net.send(Request::FriendSettings(Some(*settings))),
        FriendAction::Talk(address) => {
            friends.talk_with(address);
            net.send(Request::Messages {
                with: address.clone(),
            });
        }
        FriendAction::StopTalking => {
            friends.talking = None;
            friends.problem = None;
        }
        FriendAction::Send => send(&net, &mut friends),
        FriendAction::Ring { address, name } => calling.ring(&net, address, name),
        FriendAction::HangUp => calling.hang_up(&net),
        FriendAction::Mute => calling.toggle_mute(),
    }
}

/// An invitation accepted: join its game (a free seat if it has one, watching otherwise).
pub fn accepted(net: &Net, watching: &mut WatchStatus, call: &FriendCall) {
    if let FriendCall::AnswerInvite { room, accept: true } = call {
        watch(net, watching, Target::Join(room.clone()));
    }
}

/// Signed in, the list and the settings are read; signed out, they are forgotten.
fn follow_account(status: Res<ServerStatus>, net: Res<Net>, mut friends: ResMut<Friends>) {
    if !status.is_changed() {
        return;
    }
    let signed_in = matches!(&status.connection, Connection::Ready(s) if s.account.is_some());
    if !signed_in {
        if friends.list.is_some() || friends.settings.is_some() {
            let open = friends.open;
            friends.forget();
            // Still open for the next account, as it was asked to be.
            friends.open = open;
        }
        return;
    }
    if friends.settings.is_none() {
        net.send(Request::FriendSettings(None));
    }
    net.send(Request::Friends);
}

/// The game a friend can be invited to: one this client has a seat in, not started yet.
fn follow_game(watching: Res<WatchStatus>, mut friends: ResMut<Friends>) {
    if !watching.is_changed() {
        return;
    }
    let room = watching
        .table
        .as_ref()
        .filter(|t| t.in_lobby())
        .map(|t| t.room_code.clone());
    if friends.invite_room != room {
        friends.invite_room = room;
        friends.invited.clear();
    }
}

/// The friend `--talk` named, until the list shows their address.
#[derive(Resource)]
struct TalkOnStart(Option<String>);

fn talk_on_start(mut pending: ResMut<TalkOnStart>, net: Res<Net>, mut friends: ResMut<Friends>) {
    let Some(handle) = &pending.0 else { return };
    let Some(address) = friends.list.as_ref().and_then(|l| {
        l.friends
            .iter()
            .find(|x| {
                x.handle
                    .eq_ignore_ascii_case(handle.trim_start_matches('@'))
            })
            .map(|x| x.address.clone())
    }) else {
        return;
    };
    pending.0 = None;
    friends.talk_with(&address);
    net.send(Request::Messages { with: address });
}

fn refresh_while_open(time: Res<Time>, net: Res<Net>, mut friends: ResMut<Friends>) {
    let friends = friends.bypass_change_detection();
    if friends.open && friends.list.is_some() && friends.refresh.tick(time.delta()).just_finished()
    {
        net.send(Request::Friends);
    }
}

/// The handle field.
#[derive(Component)]
struct AskField;

/// The field a message is written in.
#[derive(Component)]
struct WriteField;

/// The panel's fields: the handle's, and the message's.
type TextFields<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static EditableText, Has<WriteField>),
    Or<(With<AskField>, With<WriteField>)>,
>;

/// What is typed in the panel's fields, kept, so that a panel drawn again shows it; and which
/// has the keyboard, so that it keeps it.
fn keep_typed(focus: Res<InputFocus>, fields: TextFields, mut friends: ResMut<Friends>) {
    let friends = friends.bypass_change_detection();
    let mut typing = Typing::Nowhere;
    for (entity, text, write) in &fields {
        let typed = text.value().to_string();
        let kept = if write {
            &mut friends.writing
        } else {
            &mut friends.asking
        };
        if *kept != typed {
            *kept = typed;
        }
        if focus.get() == Some(entity) {
            typing = if write { Typing::Write } else { Typing::Ask };
        }
    }
    friends.typing = typing;
}

#[derive(Component)]
struct FriendsPanel;

fn spawn_panel(mut commands: Commands) {
    commands.spawn((
        FriendsPanel,
        crate::login::MainOnly,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            right: px(16),
            // Below the frame rate.
            top: px(40),
            max_height: percent(80),
            width: px(300),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(12)),
            row_gap: px(6),
            border_radius: BorderRadius::all(px(10)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        BackgroundColor(colours::PANEL),
        TabGroup::new(1),
    ));
}

fn rebuild(
    mut commands: Commands,
    status: Res<ServerStatus>,
    friends: Res<Friends>,
    calling: Res<DirectCalling>,
    fonts: Res<UiFonts>,
    panel: Single<(Entity, &mut Node), With<FriendsPanel>>,
) {
    let (panel, mut node) = panel.into_inner();
    let signed_in = matches!(&status.connection, Connection::Ready(s) if s.account.is_some());
    node.display = if signed_in {
        Display::Flex
    } else {
        Display::None
    };
    let f = &*fonts;
    commands
        .entity(panel)
        .despawn_related::<Children>()
        .with_children(|p| {
            if signed_in {
                draw(p, f, &friends, &calling.call);
            }
        });
}

fn act(f: &UiFonts, label: &str, action: FriendAction) -> impl Bundle {
    button(f, label, Action::Friends(action))
}

/// The call, above everything, the panel open or not: a ring must be seen.
fn call_bar(p: &mut ChildSpawnerCommands, f: &UiFonts, call: &Call) {
    if let Some(error) = &call.error {
        p.spawn(line(f.regular(13.0), colours::WARN, error));
    }
    let text = match call.phase {
        Phase::Idle => return,
        Phase::Ringing => format!("{} is calling", call.name),
        Phase::Calling => format!("Calling {}…", call.name),
        Phase::Live if !call.connected => format!("Connecting to {}…", call.name),
        Phase::Live if call.speaking => format!("In a call with {}  ·  speaking", call.name),
        Phase::Live => format!("In a call with {}", call.name),
    };
    p.spawn(line(f.semibold(14.0), colours::TEXT, &text));
    p.spawn(buttons()).with_children(|row| match call.phase {
        Phase::Ringing => {
            row.spawn(act(
                f,
                "Answer",
                FriendAction::Ring {
                    address: call.peer.clone(),
                    name: call.name.clone(),
                },
            ));
            row.spawn(act(f, "Decline", FriendAction::HangUp));
        }
        Phase::Live => {
            row.spawn(chip(
                f,
                "Mute",
                Action::Friends(FriendAction::Mute),
                call.muted,
            ));
            row.spawn(act(f, "Hang up", FriendAction::HangUp));
        }
        _ => {
            row.spawn(act(f, "Hang up", FriendAction::HangUp));
        }
    });
}

fn draw(p: &mut ChildSpawnerCommands, f: &UiFonts, friends: &Friends, call: &Call) {
    let waiting = friends.waiting();
    let title = match (friends.open, waiting) {
        (true, _) => "Friends".to_string(),
        (false, 0) => "Friends".to_string(),
        (false, n) => format!("Friends  ·  {n} waiting"),
    };
    p.spawn(buttons()).with_children(|row| {
        row.spawn(chip(
            f,
            &title,
            Action::Friends(FriendAction::Toggle),
            friends.open,
        ));
    });
    call_bar(p, f, call);
    if !friends.open {
        return;
    }
    if let Some(problem) = &friends.problem {
        p.spawn(line(f.regular(13.0), colours::WARN, &problem.message()));
    }
    let Some(list) = &friends.list else {
        p.spawn(line(f.regular(13.0), colours::MUTED, "Reading the list…"));
        return;
    };
    if let Some(address) = &friends.talking {
        talk(p, f, friends, list, address);
        return;
    }

    for invite in &list.invites {
        let text = format!("{} invites you to {}", invite.from, invite.game);
        p.spawn(line(f.semibold(14.0), colours::TEXT, &text));
        if !invite.notice.is_empty() {
            p.spawn(line(f.regular(13.0), colours::MUTED, &invite.notice));
        }
        p.spawn(buttons()).with_children(|row| {
            if invite.pending() {
                row.spawn(act(f, "Join", FriendAction::Join(invite.room.clone())));
                row.spawn(act(
                    f,
                    "Decline",
                    FriendAction::Call(FriendCall::AnswerInvite {
                        room: invite.room.clone(),
                        accept: false,
                    }),
                ));
            } else {
                row.spawn(act(
                    f,
                    "Dismiss",
                    FriendAction::Call(FriendCall::Dismiss(invite.room.clone())),
                ));
            }
        });
    }

    for asker in &list.incoming {
        p.spawn(line(
            f.regular(14.0),
            colours::TEXT,
            &format!("{} asks to be your friend", asker.name),
        ));
        p.spawn(buttons()).with_children(|row| {
            for (label, accept) in [("Accept", true), ("Decline", false)] {
                row.spawn(act(
                    f,
                    label,
                    FriendAction::Call(FriendCall::Answer {
                        handle: asker.handle.clone(),
                        accept,
                    }),
                ));
            }
        });
    }

    if list.friends.is_empty() {
        p.spawn(line(
            f.regular(13.0),
            colours::MUTED,
            "No friends yet: ask somebody by their handle below.",
        ));
    }
    let mut friends_sorted: Vec<_> = list.friends.iter().collect();
    friends_sorted.sort_by_key(|x| (!x.online, x.handle.to_lowercase()));
    for friend in friends_sorted {
        p.spawn(Node {
            column_gap: px(8),
            align_items: AlignItems::Center,
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(4),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(10),
                    height: px(10),
                    border_radius: BorderRadius::all(px(5)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(if friend.online {
                    colours::GOOD
                } else {
                    colours::FIELD_EDGE
                }),
            ));
            let (font, colour) = if friend.online {
                (f.semibold(14.0), colours::TEXT)
            } else {
                (f.regular(14.0), colours::MUTED)
            };
            row.spawn(line(font, colour, &friend.name));
            if let Some(room) = &friends.invite_room {
                if friends.invited.contains(&friend.handle) {
                    row.spawn(line(f.regular(13.0), colours::MUTED, "invited"));
                } else {
                    row.spawn(act(
                        f,
                        "Invite",
                        FriendAction::Call(FriendCall::Invite {
                            handle: friend.handle.clone(),
                            room: room.clone(),
                        }),
                    ));
                }
            }
            let write = match friends.unread(&friend.address) {
                0 => "Write".to_string(),
                n => format!("Write  ·  {n}"),
            };
            row.spawn(act(f, &write, FriendAction::Talk(friend.address.clone())));
            // The server rings only a friend who is here.
            if friend.online && call.phase == Phase::Idle {
                row.spawn(act(
                    f,
                    "Call",
                    FriendAction::Ring {
                        address: friend.address.clone(),
                        name: friend.name.clone(),
                    },
                ));
            }
            if friends.removing.as_ref() == Some(&friend.handle) {
                row.spawn(act(
                    f,
                    "Remove",
                    FriendAction::Remove(friend.handle.clone()),
                ));
                row.spawn(act(f, "Keep", FriendAction::KeepFriend));
            } else {
                row.spawn(act(
                    f,
                    "Remove",
                    FriendAction::Remove(friend.handle.clone()),
                ));
            }
        });
        if friends.removing.as_ref() == Some(&friend.handle) {
            p.spawn(line(
                f.regular(13.0),
                colours::WARN,
                &format!("No longer friends with {}?", friend.name),
            ));
        }
    }

    for asked in &list.outgoing {
        p.spawn(buttons()).with_children(|row| {
            row.spawn(line(
                f.regular(13.0),
                colours::MUTED,
                &format!("Asked {}", asked.name),
            ));
            row.spawn(act(
                f,
                "Cancel",
                FriendAction::Call(FriendCall::Remove(asked.handle.clone())),
            ));
        });
    }

    p.spawn(line(f.semibold(14.0), colours::TEXT, "Ask a friend"));
    let mut field = p.spawn((AskField, text_field(f, &friends.asking), TabIndex(0)));
    field.observe(on_ask_key);
    if friends.typing == Typing::Ask {
        field.insert(AutoFocus);
    }
    p.spawn(buttons()).with_children(|row| {
        row.spawn(act(f, "Ask", FriendAction::Ask));
    });

    if let Some(settings) = friends.settings {
        p.spawn(line(f.semibold(14.0), colours::TEXT, "Settings"));
        let toggles = [
            (
                "Friends see when I'm here",
                settings.show_presence,
                FriendSettings {
                    show_presence: !settings.show_presence,
                    ..settings
                },
            ),
            (
                "Friends may invite me to games",
                settings.accept_invites,
                FriendSettings {
                    accept_invites: !settings.accept_invites,
                    ..settings
                },
            ),
            (
                "Mail me friend requests",
                settings.mail_requests,
                FriendSettings {
                    mail_requests: !settings.mail_requests,
                    ..settings
                },
            ),
        ];
        p.spawn(buttons()).with_children(|row| {
            for (label, on, then) in toggles {
                row.spawn(chip(
                    f,
                    label,
                    Action::Friends(FriendAction::Settings(then)),
                    on,
                ));
            }
        });
    }
}

/// The conversation with the friend at `address`, in place of the list.
fn talk(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    friends: &Friends,
    list: &FriendsList,
    address: &str,
) {
    let friend = list.friends.iter().find(|x| x.address == address);
    let name = friend.map_or("A friend who has gone", |x| x.name.as_str());
    p.spawn(buttons()).with_children(|row| {
        row.spawn(act(f, "Back to friends", FriendAction::StopTalking));
    });
    p.spawn(line(f.semibold(15.0), colours::TEXT, name));
    let messages = friends
        .talks
        .get(address)
        .map_or(&[][..], |t| &t.messages[..]);
    if messages.is_empty() {
        p.spawn(line(
            f.regular(13.0),
            colours::MUTED,
            "Nothing said yet. Messages are kept for two hours.",
        ));
    }
    for message in &messages[messages.len().saturating_sub(SHOWN)..] {
        let mine = friends.address.as_deref() == Some(message.from.as_str());
        let who = if mine { "You" } else { name };
        p.spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|block| {
            block.spawn(line(
                f.semibold(12.0),
                if mine { colours::MUTED } else { colours::GOOD },
                who,
            ));
            block.spawn(line(f.regular(14.0), colours::TEXT, &message.text));
        });
    }
    if friend.is_some_and(|x| !x.can_write) {
        p.spawn(line(
            f.regular(13.0),
            colours::WARN,
            &format!("{name} is not here, so a message would not reach them."),
        ));
    }
    let mut field = p.spawn((WriteField, text_field(f, &friends.writing), TabIndex(0)));
    field.insert(EditableText {
        max_characters: Some(MESSAGE_MAX),
        ..EditableText::new(&friends.writing)
    });
    field.observe(on_write_key);
    if friends.typing != Typing::Ask {
        field.insert(AutoFocus);
    }
    p.spawn(buttons()).with_children(|row| {
        if friends.sending {
            row.spawn(line(f.regular(13.0), colours::MUTED, "Sending…"));
        } else {
            row.spawn(act(f, "Send", FriendAction::Send));
        }
    });
}

/// Enter in the message field sends it.
fn on_write_key(
    mut key: On<FocusedInput<KeyboardInput>>,
    net: Res<Net>,
    mut friends: ResMut<Friends>,
) {
    if key.input.state == ButtonState::Pressed && key.input.logical_key == Key::Enter {
        key.propagate(false);
        send(&net, &mut friends);
    }
}

/// Enter in the handle field asks.
fn on_ask_key(
    mut key: On<FocusedInput<KeyboardInput>>,
    net: Res<Net>,
    mut friends: ResMut<Friends>,
) {
    if key.input.state == ButtonState::Pressed && key.input.logical_key == Key::Enter {
        key.propagate(false);
        ask(&net, &mut friends);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::api::{Friend, FriendInvite};

    fn friend(handle: &str, online: bool) -> Friend {
        Friend {
            handle: handle.into(),
            name: format!("@{handle}"),
            address: format!("addr-{handle}"),
            online,
            can_write: online,
        }
    }

    fn message(id: u64, from: &str, to: &str, text: &str) -> DirectMessage {
        DirectMessage {
            id,
            from: from.into(),
            to: to.into(),
            name: from.into(),
            text: text.into(),
            at: String::new(),
        }
    }

    #[test]
    fn messages_are_kept_by_friend_in_order_once_and_counted_until_read() {
        let mut friends = Friends {
            address: Some("me".into()),
            open: true,
            ..Default::default()
        };
        friends.received(message(2, "bo", "me", "hi"));
        friends.received(message(3, "cy", "me", "hey"));
        friends.received(message(2, "bo", "me", "hi"));
        assert_eq!(friends.unread("bo"), 1, "the same message twice is one");
        assert_eq!(friends.waiting(), 2);

        // Opening the conversation reads it; the history fills in what came before.
        friends.talk_with("bo");
        assert_eq!(friends.unread("bo"), 0);
        friends.history(
            "bo",
            vec![
                message(1, "me", "bo", "hello"),
                message(2, "bo", "me", "hi"),
            ],
        );
        let ids: Vec<u64> = friends.talks["bo"].messages.iter().map(|m| m.id).collect();
        assert_eq!(ids, [1, 2]);

        // While it is open, theirs are read as they come; this account's own never count.
        friends.received(message(4, "bo", "me", "still there?"));
        friends.received(message(5, "me", "cy", "from the web page"));
        assert_eq!(friends.unread("bo"), 0);
        assert_eq!(friends.unread("cy"), 1);
        assert_eq!(friends.talks["cy"].messages.len(), 2);

        // Sent from here: the answer and the socket bring the same message.
        friends.writing = "yes".into();
        friends.sending = true;
        friends.written("bo", Ok(message(6, "me", "bo", "yes")));
        friends.received(message(6, "me", "bo", "yes"));
        assert!(!friends.sending && friends.writing.is_empty());
        assert_eq!(friends.talks["bo"].messages.len(), 4);

        // Refused: what was written stays, to send again.
        friends.writing = "again".into();
        friends.sending = true;
        let gone = Problem::Refused {
            status: 404,
            message: "session: that friend is not here".into(),
        };
        friends.written("bo", Err(gone.clone()));
        assert_eq!(friends.writing, "again");
        assert_eq!(friends.problem, Some(gone));
    }

    #[test]
    fn a_message_for_a_closed_panel_counts_even_in_the_open_conversation() {
        let mut friends = Friends {
            address: Some("me".into()),
            talking: Some("bo".into()),
            open: false,
            ..Default::default()
        };
        friends.received(message(1, "bo", "me", "hi"));
        assert_eq!(friends.waiting(), 1);
    }

    fn invite(room: &str, status: &str) -> FriendInvite {
        FriendInvite {
            room: room.into(),
            game: "Hex (7×7)".into(),
            from: "@bo".into(),
            status: status.into(),
            notice: String::new(),
        }
    }

    #[test]
    fn requests_and_pending_invitations_are_what_waits() {
        let mut friends = Friends::default();
        assert_eq!(friends.waiting(), 0);
        friends.read(Ok(FriendsList {
            friends: vec![friend("bo", true)],
            incoming: vec![friend("cy", false)],
            outgoing: vec![friend("di", false)],
            invites: vec![invite("AB12CD", "pending"), invite("EF34GH", "accepted")],
        }));
        assert_eq!(friends.waiting(), 2);
    }

    #[test]
    fn a_call_done_clears_what_it_was_about_and_a_refusal_is_kept() {
        let mut friends = Friends {
            asking: "bo".into(),
            removing: Some("cy".into()),
            ..Default::default()
        };
        let list = FriendsList {
            outgoing: vec![friend("bo", false)],
            ..Default::default()
        };
        friends.done(&FriendCall::Ask("bo".into()), Ok(list.clone()));
        assert_eq!(friends.asking, "");
        assert_eq!(friends.list, Some(list));

        friends.done(
            &FriendCall::Invite {
                handle: "bo".into(),
                room: "AB12CD".into(),
            },
            Ok(FriendsList::default()),
        );
        assert!(friends.invited.contains("bo"));

        let nobody = Problem::Refused {
            status: 404,
            message: "session: there is no account with that handle".into(),
        };
        friends.done(&FriendCall::Ask("zz".into()), Err(nobody.clone()));
        assert_eq!(friends.problem, Some(nobody));
        assert_eq!(friends.removing.as_deref(), Some("cy"), "untouched");
    }

    #[test]
    fn the_same_list_read_again_changes_nothing() {
        let mut friends = Friends::default();
        let list = FriendsList {
            friends: vec![friend("bo", false)],
            ..Default::default()
        };
        assert!(friends.read(Ok(list.clone())));
        assert!(
            !friends.read(Ok(list)),
            "so the panel is not drawn again every ten seconds for nothing"
        );
        assert!(
            friends.read(Ok(FriendsList {
                friends: vec![friend("bo", true)],
                ..Default::default()
            })),
            "bo came online"
        );
    }
}
