//! The signed-in account's friends: who is online, requests in and out, invitations to games,
//! and the settings that go with them (Bingle's ADR 0105 and 0106).
//!
//! The list lives in [`Friends`], filled by the network thread: when the account's socket opens
//! or says the list changed, and after every change made here. Whether a friend is online is not
//! sent when it changes, so while the panel is open the list is read again every
//! [`REFRESH_EVERY`], as Bingle's own page does.
//!
//! The panel stands on the right of the screen while an account is signed in, during a game
//! too, since invitations come at any time. Closed, it is one button that counts what is waiting.

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::tab_navigation::{TabGroup, TabIndex};
use bevy::input_focus::{AutoFocus, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Activate;

use crate::browser::{Action, button, buttons, chip, colours, line};
use crate::fonts::UiFonts;
use crate::net::api::{FriendSettings, FriendsList};
use crate::net::session::Problem;
use crate::net::{Connection, FriendCall, Net, Request, ServerStatus, Target, WatchStatus, watch};
use crate::sign_in::text_field;

/// How often the list is read again while the panel is open, for who is online.
pub const REFRESH_EVERY: Duration = Duration::from_secs(10);

pub struct FriendsPlugin {
    /// Open the panel at start (`--friends`).
    pub open: bool,
}

impl Plugin for FriendsPlugin {
    fn build(&self, app: &mut App) {
        if self.open {
            app.add_systems(Startup, |mut friends: ResMut<Friends>| friends.open = true);
        }
        app.add_systems(Startup, spawn_panel)
            .add_observer(on_friend_action)
            .add_systems(
                Update,
                (
                    follow_account,
                    follow_game,
                    refresh_while_open,
                    keep_typed,
                    rebuild.run_if(
                        resource_changed::<Friends>.or_eager(resource_changed::<ServerStatus>),
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
    /// The handle field had the keyboard when the panel was last drawn.
    typing: bool,
    refresh: Timer,
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
            typing: false,
            refresh: Timer::new(REFRESH_EVERY, TimerMode::Repeating),
        }
    }
}

impl Friends {
    /// Requests to answer and invitations not answered yet: what the closed panel counts.
    pub fn waiting(&self) -> usize {
        self.list.as_ref().map_or(0, |l| {
            l.incoming.len() + l.invites.iter().filter(|i| i.pending()).count()
        })
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

/// What is typed in the handle field, kept, so that a panel drawn again shows it; and whether it
/// has the keyboard, so that it keeps it.
fn keep_typed(
    focus: Res<InputFocus>,
    fields: Query<(Entity, &EditableText), With<AskField>>,
    mut friends: ResMut<Friends>,
) {
    let friends = friends.bypass_change_detection();
    for (entity, text) in &fields {
        let typed = text.value().to_string();
        if friends.asking != typed {
            friends.asking = typed;
        }
        friends.typing = focus.get() == Some(entity);
    }
}

#[derive(Component)]
struct FriendsPanel;

fn spawn_panel(mut commands: Commands) {
    commands.spawn((
        FriendsPanel,
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
                draw(p, f, &friends);
            }
        });
}

fn act(f: &UiFonts, label: &str, action: FriendAction) -> impl Bundle {
    button(f, label, Action::Friends(action))
}

fn draw(p: &mut ChildSpawnerCommands, f: &UiFonts, friends: &Friends) {
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
    if friends.typing {
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
            online,
        }
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
