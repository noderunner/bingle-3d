//! The connection to a Bingle server.
//!
//! All network I/O happens on one thread of its own, with blocking calls, so no Bevy system ever
//! waits on the network. Systems send it [`Request`]s; it answers with what happened, which
//! [`poll_network`] turns into the [`ServerStatus`] and [`WatchStatus`] resources the screens draw
//! from.
//!
//! A game this client plays a seat of is watched too: its turn ([`crate::play::Turn`]) lives
//! beside the table in [`WatchStatus`], and asks for the legal selections and sends the move
//! through the same thread.
//!
//! While a game is watched, its WebSocket is read in the same loop, with a short timeout
//! ([`socket::POLL`]), so requests are still answered in between and there is still one thread.
//! So is the signed-in account's own socket (`/ws-dm`), which says when its friends list changes;
//! and while an account is signed in, the thread tells the server every minute that it is here,
//! so that its friends see it online.

pub mod api;
pub mod server_url;
pub mod session;
pub mod socket;
pub mod tls;

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use bevy::prelude::*;

use crate::config::Store;
use crate::play::{Outcome, Turn};
use crate::table::{Step, Table};
use api::{
    AccountEvent, BotInfo, DirectCall, DirectMessage, DirectSignal, Event, FriendSettings,
    FriendsList, GameInfo, GameState, IceServer, OpenGame, Selections, VoiceEvent, VoicePeer,
};
use std::collections::BTreeMap;

use crate::call::DirectCalling;
use crate::friends::{self, Friends};
use crate::login::{LoginScreen, Screen};
use crate::new_game::NewGame;
use crate::sign_in::Signing;
use crate::voice_chat::VoiceChat;
use session::{Code, Opened, Problem, Session, SignIn, Snapshot};
use socket::Socket;
use tls::Fingerprint;

pub struct NetPlugin {
    /// A server for this run, in place of the configured one.
    pub server: Option<String>,
    /// Skip the login screen this run (the command line asked for something on the server),
    /// or show it whatever the config says (`--sign-in`).
    pub start: Start,
}

/// Where a run starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// As the config says: the main screen if it logs in by itself, else the login screen.
    AsConfigured,
    /// On the main screen, with what is kept.
    StraightIn,
    /// On the login screen.
    Login,
}

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        let store = Store::load(Store::default_dir());
        let notices = store.problems().to_vec();
        for notice in &notices {
            warn!("{notice}");
        }
        let auto_login = store.config.auto_login;
        // Where the table stands is the scene's to build; the config is this thread's to keep.
        app.insert_resource(crate::environment::Kept(store.config.environment.clone()));
        let session = Session::new(self.server.as_deref(), store);
        let status = ServerStatus {
            server: session.server(),
            connection: Connection::Connecting,
            notices,
            retry_in: None,
        };
        let mut login = LoginScreen::new(&session.server(), auto_login);
        let net = Net::spawn(session);
        let straight_in = match self.start {
            Start::AsConfigured => auto_login,
            Start::StraightIn => true,
            Start::Login => false,
        };
        let screen = if straight_in {
            net.send(Request::Connect);
            Screen::Main
        } else {
            login.probe(&net, true);
            Screen::Login
        };
        app.insert_resource(net)
            .insert_resource(screen)
            .insert_resource(login)
            .insert_resource(status)
            .init_resource::<WatchStatus>()
            .init_resource::<GameList>()
            .init_resource::<NewGame>()
            .init_resource::<Signing>()
            .init_resource::<DirectCalling>()
            .init_resource::<VoiceChat>()
            .init_resource::<Friends>()
            .add_systems(PreUpdate, poll_network)
            .add_systems(
                Update,
                (
                    (retry_later, refresh_now, refresh_games).run_if(on_main),
                    rewatch_later,
                ),
            );
    }
}

/// What a screen can ask of the network thread.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Connect, or connect again: index, version, guest key, games.
    Connect,
    /// The player has looked at this certificate's fingerprint and says it is the server's.
    Trust(Fingerprint),
    /// Start watching, in place of any game watched before. `watch` numbers the attempt, so
    /// that what arrives for an earlier one is recognised and dropped.
    Watch { target: Target, watch: u64 },
    /// Read the watched game's state again (a move was missed).
    Reload { watch: u64 },
    /// What the seat on impulse may select after `made`. `asked_at` is the turn's, and comes
    /// back with the answer.
    Selections {
        watch: u64,
        asked_at: u64,
        made: Vec<String>,
        ply: Option<u64>,
    },
    /// Play a whole move for the seat `player_name` sits at.
    Play {
        watch: u64,
        asked_at: u64,
        player_name: String,
        notation: String,
        ply: u64,
    },
    /// Give up the watched game for the seat `player_name` sits at.
    Resign { watch: u64, player_name: String },
    /// Before the watched game starts, do this for the seat `player_name` sits at.
    Lobby {
        watch: u64,
        player_name: String,
        call: LobbyCall,
    },
    /// Close the watched game's socket.
    StopWatching,
    /// The unfinished games that match `query`. `asked` comes back with the answer.
    Games { query: GameQuery, asked: u64 },
    /// What setting up a game of this needs (its seats, the bots).
    Setup { game_def_id: String },
    /// Sign in to an account: the handle and the password.
    SignIn { handle: String, password: String },
    /// The second step of signing in, with what the first answered.
    SecondFactor { pending: String, code: Code },
    /// End the account's session.
    SignOut,
    /// The login screen's server: use this address from now on, and say whether it is there,
    /// can be used, and who this client would be there. `asked` comes back with the answer.
    Probe { address: String, asked: u64 },
    /// Go on as a guest: any account's session ended, and this guest key taken if one is given
    /// (else the one kept, or a new one); then connect.
    ContinueAsGuest { key: Option<String> },
    /// Remember the server used (if `server`) and whether to log in by itself next time.
    Remember { server: bool, auto_login: bool },
    /// Keep where the table stands (an environment's id) for the next run.
    RememberEnvironment(String),
    /// Read the account's friends list.
    Friends,
    /// Change the friends list, or answer an invitation.
    Friend(FriendCall),
    /// Read the friends settings, or change them to these.
    FriendSettings(Option<FriendSettings>),
    /// What was said with this address.
    Messages { with: String },
    /// Write to this address.
    Write { to: String, text: String },
    /// Ring a friend at this address, or answer them; with `join` false, hang up.
    Call { to: String, join: bool },
    /// The ICE servers for a call about to start.
    IceServers,
    /// One WebRTC message for the friend called.
    Signal {
        to: String,
        kind: String,
        data: serde_json::Value,
    },
    /// Join the voice chat of a game, for the seat `player_name` sits at, in place of any other.
    JoinVoice { room: String, player_name: String },
    /// Leave the game's voice chat.
    LeaveVoice,
    /// One WebRTC message for a seat in the game's voice chat.
    VoiceSignal {
        to: u32,
        kind: String,
        data: serde_json::Value,
    },
}

/// What a signed-in account can do with its friends list. The server decides whether it may (a
/// game it has a seat in, a friend who takes invitations).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FriendCall {
    /// Ask this handle to be a friend.
    Ask(String),
    /// Answer their request.
    Answer { handle: String, accept: bool },
    /// No longer friends, or take back a request.
    Remove(String),
    /// Invite a friend to a game this account has a seat in.
    Invite { handle: String, room: String },
    /// Answer an invitation to a game.
    AnswerInvite { room: String, accept: bool },
    /// Put away an invitation, or its news.
    Dismiss(String),
}

/// A new game as the player set it up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSettings {
    pub game_def_id: String,
    /// This client's seat.
    pub seat: u32,
    /// A built-in bot in each of these seats, by seat; any other seat waits for a person.
    pub bots: BTreeMap<u32, String>,
    pub time: TimeControl,
    /// It waits for this client's start, even once every seat is taken.
    pub lobby: bool,
    /// Seats kept for friends, by seat: their handles (signed in only).
    pub reserved: BTreeMap<u32, String>,
}

/// What a player at the table may do before the game starts. The server decides whether they
/// may (only the host starts the game or clears a seat), and refuses otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LobbyCall {
    /// The host starts a game that waits in its lobby.
    Start,
    /// Move to this open seat.
    Sit(u32),
    /// Put this built-in bot in this open seat.
    Bot { seat: u32, bot: String },
    /// The host clears this seat, a person's or a bot's.
    Remove(u32),
    /// Give up this client's seat.
    Leave,
}

/// How a game is timed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TimeControl {
    #[default]
    None,
    /// A game clock: each seat has `seconds` in all, and gains `increment` with every move.
    Clock { seconds: u64, increment: u64 },
    /// Each move may take `seconds`.
    PerMove { seconds: u64 },
}

/// Which of the server's unfinished games to list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameQuery {
    /// Only this game and its other layouts (a definition's `family`, or its id without one).
    pub family: Option<String>,
    /// Only games with a seat free.
    pub open_seats: bool,
}

/// What to watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A new match between two of the server's bots, in this game.
    Bots { game_def_id: String },
    /// A new game in which this client plays the first seat and the server's bots the others.
    AgainstBots { game_def_id: String },
    /// A new game in which this client plays the first seat and waits for people to join.
    Host { game_def_id: String },
    /// A new game set up by the player.
    Start(GameSettings),
    /// The game in this room, from an open seat if it is waiting for players and has one;
    /// watched otherwise.
    Join(String),
    /// A game already being played.
    Room(String),
}

enum Reply {
    Connected(Snapshot),
    Failed(Problem),
    Probed {
        asked: u64,
        result: Result<session::Probe, Problem>,
    },
    Opened {
        watch: u64,
        opened: Box<Opened>,
    },
    Event {
        watch: u64,
        event: Box<Event>,
    },
    Reloaded {
        watch: u64,
        state: Box<GameState>,
    },
    WatchFailed {
        watch: u64,
        problem: Problem,
    },
    Selections {
        watch: u64,
        asked_at: u64,
        made: Vec<String>,
        result: Result<Box<Selections>, Problem>,
    },
    Played {
        watch: u64,
        asked_at: u64,
        result: Result<(), Problem>,
    },
    Resigned {
        watch: u64,
        result: Result<(), Problem>,
    },
    Lobby {
        watch: u64,
        call: LobbyCall,
        result: Result<(), Problem>,
    },
    Games {
        asked: u64,
        result: Result<Vec<OpenGame>, Problem>,
    },
    Setup {
        game_def_id: String,
        result: Result<Box<(GameInfo, Vec<BotInfo>)>, Problem>,
    },
    SignIn(Result<SignIn, Problem>),
    SignedOut(Result<(), Problem>),
    Friends(Result<FriendsList, Problem>),
    /// A friends call was made; the list as it stands afterwards, or why it was not.
    FriendDone {
        call: FriendCall,
        result: Result<FriendsList, Problem>,
    },
    FriendSettings(Result<FriendSettings, Problem>),
    /// The account's own address, as its socket said when it opened.
    AccountAddress(String),
    Messages {
        with: String,
        result: Result<Vec<DirectMessage>, Problem>,
    },
    Written {
        to: String,
        result: Result<DirectMessage, Problem>,
    },
    /// A message arrived on the account's socket.
    Message(Box<DirectMessage>),
    /// A call was rung, answered or hung up (`join`), or why not.
    Called {
        to: String,
        join: bool,
        result: Result<(), Problem>,
    },
    IceServers(Result<Vec<IceServer>, Problem>),
    /// A call rang or ended, on the account's socket.
    CallEvent(DirectCall),
    /// A WebRTC message of a call, on the account's socket.
    CallSignal(Box<DirectSignal>),
    /// The game's voice chat was joined: the ICE servers and who is in it; or why not.
    VoiceJoined {
        room: String,
        result: Result<(Vec<IceServer>, Vec<VoicePeer>), Problem>,
    },
    /// Who is in the game's voice chat, as the heartbeat (or the socket reopened) said.
    VoicePeers {
        room: String,
        peers: Vec<VoicePeer>,
    },
    /// Somebody joined or left the game's voice chat, or signals this seat.
    Voice {
        room: String,
        event: VoiceEvent,
    },
}

#[derive(Resource)]
pub struct Net {
    requests: Sender<Request>,
    // A `Mutex` only to make the receiver `Sync`, as a resource must be; only `poll_network`
    // locks it.
    replies: Mutex<Receiver<Reply>>,
}

impl Net {
    fn spawn(session: Session) -> Self {
        let (requests, inbox) = mpsc::channel::<Request>();
        let (outbox, replies) = mpsc::channel::<Reply>();
        std::thread::Builder::new()
            .name("bingle-net".into())
            .spawn(move || run(session, inbox, outbox))
            .expect("the network thread starts");
        Net {
            requests,
            replies: Mutex::new(replies),
        }
    }

    /// A `Net` with no thread behind it, and the receiving end of its requests.
    #[cfg(test)]
    pub fn for_test() -> (Self, Receiver<Request>) {
        let (net, inbox, _) = Net::for_test_with_replies();
        (net, inbox)
    }

    /// The same, with the sending end of its replies, to answer as the thread would.
    #[cfg(test)]
    fn for_test_with_replies() -> (Self, Receiver<Request>, Sender<Reply>) {
        let (requests, inbox) = mpsc::channel();
        let (outbox, replies) = mpsc::channel();
        let net = Net {
            requests,
            replies: Mutex::new(replies),
        };
        (net, inbox, outbox)
    }

    pub fn send(&self, request: Request) {
        // Fails only if the thread has gone, which `poll_network` reports.
        let _ = self.requests.send(request);
    }
}

/// The socket of the game being watched, on the network thread.
struct Watching {
    watch: u64,
    room_code: String,
    socket: Socket,
}

/// The network thread: requests in, replies out, and the watched game's socket read in between.
/// Ends when the app drops its sender, or stops listening.
fn run(mut session: Session, inbox: Receiver<Request>, outbox: Sender<Reply>) {
    let mut watching: Option<Watching> = None;
    let mut account = AccountLink::default();
    let mut voice: Option<VoiceLink> = None;
    loop {
        // With a socket to read, the inbox is only looked at between reads; signed in without
        // one (waiting to open it again), it is waited on a little, so that the account is still
        // looked after; otherwise there is nothing to do until a request comes.
        let request = if watching.is_some() || account.socket.is_some() || voice.is_some() {
            match inbox.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        } else if session.has_account() {
            match inbox.recv_timeout(ACCOUNT_WAIT) {
                Ok(r) => Some(r),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match inbox.recv() {
                Ok(r) => Some(r),
                Err(_) => break,
            }
        };
        let replies = match request {
            Some(Request::JoinVoice { room, player_name }) => {
                if let Some(old) = voice.take() {
                    old.leave(&mut session);
                }
                let (link, reply) = VoiceLink::join(&mut session, room, player_name);
                voice = link;
                vec![reply]
            }
            Some(Request::LeaveVoice) => {
                if let Some(old) = voice.take() {
                    old.leave(&mut session);
                }
                Vec::new()
            }
            Some(Request::VoiceSignal { to, kind, data }) => {
                if let Some(link) = &voice
                    && let Err(problem) =
                        session.voice_signal(&link.room, &link.name, to, &kind, data)
                {
                    // The other seat left, or this one did: the call's list says so.
                    info!(
                        "a {kind} for seat {to} was not relayed: {}",
                        problem.message()
                    );
                }
                Vec::new()
            }
            Some(request) => handle(&mut session, &mut watching, request)
                .into_iter()
                .collect(),
            None => {
                let mut replies: Vec<Reply> =
                    read_socket(&session, &mut watching).into_iter().collect();
                replies.extend(account.tick(&mut session, Instant::now()));
                if let Some(link) = voice.as_mut() {
                    replies.extend(link.tick(&mut session, Instant::now()));
                }
                replies
            }
        };
        if replies.into_iter().any(|r| outbox.send(r).is_err()) {
            break;
        }
    }
    if let Some(w) = watching {
        w.socket.close();
    }
    if let Some(s) = account.socket {
        s.close();
    }
    if let Some(link) = voice {
        link.leave(&mut session);
    }
}

/// How often a seat in a game's voice chat says it is still there: its place lapses after 45 s.
pub const VOICE_EVERY: Duration = Duration::from_secs(15);

/// How long joining waits for the voice socket's `joined`.
const VOICE_JOINED_WAIT: Duration = Duration::from_secs(4);

/// The game's voice chat this client is in, on the network thread: its own socket, and the
/// heartbeat.
struct VoiceLink {
    room: String,
    name: String,
    socket: Option<Socket>,
    beat_at: Instant,
    retry_at: Option<Instant>,
}

impl VoiceLink {
    /// Joins, as the page does: the socket first, and the seat announced once the server says it
    /// listens, since the others offer the moment they hear of it, and a message for a seat
    /// nobody listens for is lost.
    fn join(session: &mut Session, room: String, name: String) -> (Option<VoiceLink>, Reply) {
        let failed = |room: String, problem| {
            (
                None,
                Reply::VoiceJoined {
                    room,
                    result: Err(problem),
                },
            )
        };
        let mut socket = match session.voice_socket(&room, &name) {
            Ok(s) => s,
            Err(problem) => return failed(room, problem),
        };
        let deadline = Instant::now() + VOICE_JOINED_WAIT;
        while Instant::now() < deadline {
            match socket.next() {
                Ok(Some(text)) if matches!(VoiceEvent::parse(&text), Ok(VoiceEvent::Joined)) => {
                    break;
                }
                Ok(_) => {}
                Err(e) => return failed(room, session.socket_failed(e)),
            }
        }
        let ice = session.ice_servers().unwrap_or_else(|problem| {
            warn!("the ICE servers: {}", problem.message());
            Vec::new()
        });
        match session.voice(&room, &name, true) {
            Ok(peers) => {
                info!("joined the voice chat of {room}");
                let reply = Reply::VoiceJoined {
                    room: room.clone(),
                    result: Ok((ice, peers)),
                };
                let link = VoiceLink {
                    room,
                    name,
                    socket: Some(socket),
                    beat_at: Instant::now() + VOICE_EVERY,
                    retry_at: None,
                };
                (Some(link), reply)
            }
            Err(problem) => {
                socket.close();
                failed(room, problem)
            }
        }
    }

    fn leave(self, session: &mut Session) {
        if let Some(s) = self.socket {
            s.close();
        }
        if let Err(problem) = session.voice(&self.room, &self.name, false) {
            warn!("leaving the voice chat: {}", problem.message());
        }
    }

    /// The heartbeat, and what the socket says; the socket opened again if it dropped.
    fn tick(&mut self, session: &mut Session, now: Instant) -> Vec<Reply> {
        let mut replies = Vec::new();
        let room = self.room.clone();
        if now >= self.beat_at {
            self.beat_at = now + VOICE_EVERY;
            match session.voice(&room, &self.name, true) {
                Ok(peers) => replies.push(Reply::VoicePeers { room, peers }),
                Err(problem) => warn!("the voice chat's heartbeat: {}", problem.message()),
            }
            return replies;
        }
        let Some(socket) = self.socket.as_mut() else {
            if self.retry_at.is_none_or(|t| now >= t) {
                match session.voice_socket(&room, &self.name) {
                    Ok(socket) => {
                        self.socket = Some(socket);
                        self.retry_at = None;
                        // Whatever was said while it was closed is in the list.
                        if let Ok(peers) = session.voice_peers(&room) {
                            replies.push(Reply::VoicePeers { room, peers });
                        }
                    }
                    Err(problem) => {
                        warn!("the voice chat's socket: {}", problem.message());
                        self.retry_at = Some(now + RETRY_AFTER);
                    }
                }
            }
            return replies;
        };
        match socket.next() {
            Ok(Some(text)) => match VoiceEvent::parse(&text) {
                Ok(VoiceEvent::Joined | VoiceEvent::Other(_)) => {}
                Ok(event) => replies.push(Reply::Voice { room, event }),
                Err(e) => warn!("skipped a voice event this client cannot read: {e}"),
            },
            Ok(None) => {}
            Err(e) => {
                warn!("the voice chat's socket closed: {e}");
                self.socket = None;
                self.retry_at = Some(now + Duration::from_secs(1));
            }
        }
        replies
    }
}

/// How often a signed-in account says it is here: the server forgets it after two and a half
/// minutes.
pub const HERE_EVERY: Duration = Duration::from_secs(60);

/// How long the thread waits for a request when it has no socket to read but an account to
/// look after.
const ACCOUNT_WAIT: Duration = Duration::from_millis(250);

/// The signed-in account's own socket, and when it last said it was here, on the network thread.
#[derive(Default)]
struct AccountLink {
    socket: Option<Socket>,
    /// When to open the socket again, after it failed.
    retry_at: Option<Instant>,
    /// When to say next that the account is here; `None` until it is signed in.
    here_at: Option<Instant>,
}

impl AccountLink {
    /// Keeps the socket open and the account here while it is signed in, and lets both go once
    /// it is not. Reports a friends list read again (the socket opened, or said it changed).
    fn tick(&mut self, session: &mut Session, now: Instant) -> Option<Reply> {
        if !session.has_account() {
            if let Some(s) = self.socket.take() {
                s.close();
            }
            self.retry_at = None;
            self.here_at = None;
            return None;
        }
        // The socket first: once the account says it is here, friends may ring it, and what the
        // server sends before the socket listens is lost.
        let mut opened = false;
        if self.socket.is_none() && !self.retry_at.is_some_and(|t| now < t) {
            match session.account_socket() {
                Ok(Some(socket)) => {
                    info!("the account's socket is open");
                    self.socket = Some(socket);
                    self.retry_at = None;
                    opened = true;
                }
                Ok(None) => {}
                Err(problem) => {
                    warn!("the account's socket: {}", problem.message());
                    self.retry_at = Some(now + RETRY_AFTER);
                }
            }
        }
        if self.here_at.is_none_or(|t| now >= t) {
            if let Err(problem) = session.here(true) {
                warn!("saying the account is here: {}", problem.message());
            }
            self.here_at = Some(now + HERE_EVERY);
        }
        if opened {
            // Whatever changed while it was closed.
            return Some(Reply::Friends(session.friends()));
        }
        let socket = self.socket.as_mut()?;
        match socket.next() {
            Ok(None) => None,
            Ok(Some(text)) => match AccountEvent::parse(&text) {
                Ok(AccountEvent::Friends) => {
                    info!("the friends list changed");
                    Some(Reply::Friends(session.friends()))
                }
                Ok(AccountEvent::Ready { address }) => Some(Reply::AccountAddress(address)),
                Ok(AccountEvent::Message(message)) => Some(Reply::Message(Box::new(message))),
                Ok(AccountEvent::Call(call)) => Some(Reply::CallEvent(call)),
                Ok(AccountEvent::Signal(signal)) => Some(Reply::CallSignal(Box::new(signal))),
                Ok(AccountEvent::Other(_)) => None,
                Err(e) => {
                    warn!("skipped an account event this client cannot read: {e}");
                    None
                }
            },
            Err(e) => {
                warn!("the account's socket closed: {e}");
                self.socket = None;
                self.retry_at = Some(now + RETRY_AFTER);
                None
            }
        }
    }
}

fn handle(
    session: &mut Session,
    watching: &mut Option<Watching>,
    request: Request,
) -> Option<Reply> {
    match request {
        Request::Connect => Some(connected(session.connect())),
        Request::Probe { address, asked } => {
            session.use_server(&address);
            Some(Reply::Probed {
                asked,
                result: session.probe(),
            })
        }
        Request::ContinueAsGuest { key } => {
            // A guest: no account's session is kept (nor left on the server).
            if let Err(problem) = session.sign_out() {
                warn!("signing out: {}", problem.message());
            }
            if let Some(key) = key
                && let Err(problem) = session.adopt_guest_key(&key)
            {
                return Some(Reply::Failed(problem));
            }
            Some(connected(session.connect()))
        }
        Request::Remember { server, auto_login } => {
            session.remember(server, auto_login);
            None
        }
        Request::RememberEnvironment(id) => {
            session.remember_environment(&id);
            None
        }
        Request::Trust(fp) => Some(connected(session.trust(fp))),
        Request::Watch { target, watch } => {
            if let Some(old) = watching.take() {
                old.socket.close();
            }
            let opened = match target {
                Target::Room(code) => session.open_game(&code),
                Target::Bots { game_def_id } => session
                    .start_bot_match(&game_def_id)
                    .and_then(|code| session.open_game(&code)),
                Target::AgainstBots { game_def_id } => session
                    .start_game_against_bots(&game_def_id)
                    .and_then(|code| session.open_game(&code)),
                Target::Host { game_def_id } => session
                    .host_game(&game_def_id)
                    .and_then(|code| session.open_game(&code)),
                Target::Start(settings) => session
                    .start_game(&settings)
                    .and_then(|code| session.open_game(&code)),
                Target::Join(code) => session
                    .join_game(&code)
                    .and_then(|()| session.open_game(&code)),
            };
            Some(match opened {
                Ok((opened, socket)) => {
                    *watching = Some(Watching {
                        watch,
                        room_code: opened.room_code.clone(),
                        socket,
                    });
                    Reply::Opened {
                        watch,
                        opened: Box::new(opened),
                    }
                }
                Err(problem) => Reply::WatchFailed { watch, problem },
            })
        }
        Request::Reload { watch } => {
            let w = watching.as_ref().filter(|w| w.watch == watch)?;
            let room_code = w.room_code.clone();
            Some(match session.state(&room_code) {
                Ok(state) => Reply::Reloaded {
                    watch,
                    state: Box::new(state),
                },
                Err(problem) => Reply::WatchFailed { watch, problem },
            })
        }
        Request::Selections {
            watch,
            asked_at,
            made,
            ply,
        } => {
            let w = watching.as_ref().filter(|w| w.watch == watch)?;
            let result = session.selections(&w.room_code, &made, ply).map(Box::new);
            Some(Reply::Selections {
                watch,
                asked_at,
                made,
                result,
            })
        }
        Request::Play {
            watch,
            asked_at,
            player_name,
            notation,
            ply,
        } => {
            let w = watching.as_ref().filter(|w| w.watch == watch)?;
            // The moves come on the socket, as everybody else's do; the answer only says it
            // was taken.
            let result = session
                .play(&w.room_code, &player_name, &notation, ply)
                .map(drop);
            Some(Reply::Played {
                watch,
                asked_at,
                result,
            })
        }
        Request::Resign { watch, player_name } => {
            let w = watching.as_ref().filter(|w| w.watch == watch)?;
            // The game's end comes on the socket, as a state event.
            let result = session.resign(&w.room_code, &player_name);
            Some(Reply::Resigned { watch, result })
        }
        Request::Lobby {
            watch,
            player_name,
            call,
        } => {
            let w = watching.as_ref().filter(|w| w.watch == watch)?;
            // Who sits where afterwards comes on the socket, as a state event.
            let result = session.lobby(&w.room_code, &player_name, &call);
            Some(Reply::Lobby {
                watch,
                call,
                result,
            })
        }
        Request::StopWatching => {
            if let Some(old) = watching.take() {
                old.socket.close();
            }
            None
        }
        Request::Games { query, asked } => Some(Reply::Games {
            asked,
            result: session.games(&query),
        }),
        Request::Setup { game_def_id } => Some(Reply::Setup {
            result: session.game_setup(&game_def_id).map(Box::new),
            game_def_id,
        }),
        Request::SignIn { handle, password } => {
            Some(Reply::SignIn(session.sign_in(&handle, &password)))
        }
        Request::SecondFactor { pending, code } => {
            Some(Reply::SignIn(session.second_factor(&pending, &code)))
        }
        Request::SignOut => Some(Reply::SignedOut(session.sign_out())),
        Request::Friends => Some(Reply::Friends(session.friends())),
        Request::Friend(call) => {
            let result = session.friend(&call).and_then(|()| session.friends());
            Some(Reply::FriendDone { call, result })
        }
        Request::FriendSettings(new) => Some(Reply::FriendSettings(session.friend_settings(new))),
        Request::Messages { with } => Some(Reply::Messages {
            result: session.history(&with),
            with,
        }),
        Request::Write { to, text } => Some(Reply::Written {
            result: session.write(&to, &text),
            to,
        }),
        Request::Call { to, join } => Some(Reply::Called {
            result: session.direct_call(&to, join),
            to,
            join,
        }),
        Request::IceServers => Some(Reply::IceServers(session.ice_servers())),
        // Taken by `run`, with the voice chat's link.
        Request::JoinVoice { .. } | Request::LeaveVoice | Request::VoiceSignal { .. } => None,
        Request::Signal { to, kind, data } => {
            // A message the other end does not need any more (the call ended) is refused; the
            // call itself says so.
            if let Err(problem) = session.direct_signal(&to, &kind, data) {
                info!(
                    "a {kind} for the call was not relayed: {}",
                    problem.message()
                );
            }
            None
        }
    }
}

fn connected(result: Result<Snapshot, Problem>) -> Reply {
    match result {
        Ok(snapshot) => Reply::Connected(snapshot),
        Err(problem) => Reply::Failed(problem),
    }
}

fn read_socket(session: &Session, watching: &mut Option<Watching>) -> Option<Reply> {
    let w = watching.as_mut()?;
    match w.socket.next() {
        Ok(None) => None,
        Ok(Some(text)) => match Event::parse(&text) {
            Ok(Event::Other(_)) => None,
            Ok(event) => Some(Reply::Event {
                watch: w.watch,
                event: Box::new(event),
            }),
            Err(e) => {
                warn!("skipped a game event this client cannot read: {e}");
                None
            }
        },
        Err(e) => {
            let watch = w.watch;
            let problem = session.socket_failed(e);
            *watching = None;
            Some(Reply::WatchFailed { watch, problem })
        }
    }
}

/// Where things stand with the server, for screens to draw.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct ServerStatus {
    /// The address, as the player would write it.
    pub server: String,
    pub connection: Connection,
    /// Things worth telling the player that are not about the server (a config file that could
    /// not be read).
    pub notices: Vec<String>,
    /// When an unreachable server will be tried again by itself.
    pub retry_in: Option<Timer>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Connection {
    Connecting,
    Ready(Snapshot),
    Failed(Problem),
}

/// How long to wait before trying an unreachable server again.
pub const RETRY_AFTER: Duration = Duration::from_secs(5);

// A system: each resource it fills is a parameter.
#[allow(clippy::too_many_arguments)]
fn poll_network(
    net: Res<Net>,
    mut status: ResMut<ServerStatus>,
    mut watch: ResMut<WatchStatus>,
    mut list: ResMut<GameList>,
    mut new_game: ResMut<NewGame>,
    mut signing: ResMut<Signing>,
    mut friends: ResMut<Friends>,
    mut call: ResMut<DirectCalling>,
    mut voice: ResMut<VoiceChat>,
    (screen, mut login): (Option<Res<Screen>>, Option<ResMut<LoginScreen>>),
) {
    let on_login = screen.as_deref() == Some(&Screen::Login);
    let replies = net.replies.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        match replies.try_recv() {
            Ok(Reply::Opened { watch: id, opened }) if id == watch.id => {
                // From now on a lost socket reopens this room, not another bot match.
                watch.target = Some(Target::Room(opened.room_code.clone()));
                watch.table = Some(Table::new(*opened));
                watch.problem = None;
            }
            Ok(Reply::Event { watch: id, event }) if id == watch.id => {
                let Some(table) = watch.bypass_change_detection().table.as_mut() else {
                    continue;
                };
                match table.apply(&event) {
                    Step::Changed => watch.set_changed(),
                    Step::Unchanged => {}
                    Step::Reload => net.send(Request::Reload { watch: id }),
                }
            }
            Ok(Reply::Reloaded { watch: id, state }) if id == watch.id => {
                if let Some(table) = watch.bypass_change_detection().table.as_mut()
                    && table.reloaded(*state)
                {
                    watch.set_changed();
                }
            }
            Ok(Reply::WatchFailed { watch: id, problem }) if id == watch.id => {
                let over = watch.table.as_ref().is_some_and(Table::finished);
                if over {
                    // A finished game's socket may close; there is nothing more to see.
                    continue;
                }
                warn!("watching: {}", problem.message());
                // A game that was showing is opened again by itself; one that never opened waits
                // for the player.
                watch.retry_in = (watch.table.is_some() || problem.worth_retrying())
                    .then(|| Timer::new(RETRY_AFTER, TimerMode::Once));
                watch.problem = Some(problem);
            }
            Ok(Reply::Selections {
                watch: id,
                asked_at,
                made,
                result,
            }) if id == watch.id => {
                let w = watch.bypass_change_detection();
                let Some(turn) = w.turn.as_mut().filter(|t| {
                    t.asked_at == asked_at
                        && t.stage == crate::play::Stage::Asking
                        && t.made_notations() == made
                }) else {
                    // For a question no longer asked: the player went back, or the game moved on.
                    continue;
                };
                let outcome = match result {
                    Ok(selections) => turn.answered(*selections),
                    Err(problem) => {
                        warn!("could not read the legal selections: {}", problem.message());
                        turn.ask_failed(problem);
                        Outcome::Changed
                    }
                };
                watch.set_changed();
                crate::play::follow(&net, &mut watch, outcome);
            }
            Ok(Reply::Played {
                watch: id,
                asked_at,
                result,
            }) if id == watch.id => {
                let Err(problem) = result else {
                    // Taken: the turn ends when the move arrives on the socket.
                    continue;
                };
                let Some(turn) = watch.turn.as_mut().filter(|t| t.asked_at == asked_at) else {
                    continue;
                };
                warn!("the move was refused: {}", problem.message());
                // Refused because the game moved on: read where it stands.
                let moved_on = matches!(problem, Problem::Refused { status: 409, .. });
                turn.send_failed(problem);
                ask(&net, &watch);
                if moved_on {
                    net.send(Request::Reload { watch: id });
                }
            }
            Ok(Reply::Resigned { watch: id, result }) if id == watch.id => {
                watch.resign = match result {
                    // Done when the game's end arrives on the socket.
                    Ok(()) => Resign::Sent,
                    Err(problem) => {
                        warn!("could not resign: {}", problem.message());
                        Resign::Failed(problem)
                    }
                };
            }
            Ok(Reply::Lobby {
                watch: id,
                call,
                result,
            }) if id == watch.id => match result {
                // Done when the seats arrive on the socket; having left, there is nothing more
                // to do at this table.
                Ok(()) if call == LobbyCall::Leave => {
                    stop_watching(&net, &mut watch);
                    reconnect(&net, &mut status);
                }
                Ok(()) => {}
                Err(problem) => {
                    warn!("the lobby: {}", problem.message());
                    watch.lobby_problem = Some(problem);
                }
            },
            // For a watch that has since been replaced.
            Ok(
                Reply::Opened { .. }
                | Reply::Event { .. }
                | Reply::Reloaded { .. }
                | Reply::WatchFailed { .. }
                | Reply::Selections { .. }
                | Reply::Played { .. }
                | Reply::Resigned { .. }
                | Reply::Lobby { .. },
            ) => {}
            Ok(Reply::Games { asked, result }) => {
                if asked != list.asked {
                    // For a query since replaced.
                    continue;
                }
                match result {
                    Ok(games) => {
                        if list.games != games || list.problem.is_some() {
                            list.games = games;
                            list.problem = None;
                        }
                    }
                    Err(problem) => {
                        warn!("could not list the games: {}", problem.message());
                        if list.problem.as_ref() != Some(&problem) {
                            list.problem = Some(problem);
                        }
                    }
                }
            }
            Ok(Reply::Setup {
                game_def_id,
                result,
            }) => {
                if let Some(form) = new_game.form.as_mut().filter(|f| f.def_id == game_def_id) {
                    match result {
                        Ok(setup) => {
                            let (info, bots) = *setup;
                            form.prepared(info, bots);
                        }
                        Err(problem) => {
                            warn!("could not set up a game: {}", problem.message());
                            form.problem = Some(problem);
                        }
                    }
                }
            }
            Ok(Reply::SignIn(result)) => {
                if let Some(SignIn::Done(_)) = signing.answered(result) {
                    // Who the client is has changed, and with it what it may see: read it all
                    // again, as that account.
                    reconnect(&net, &mut status);
                    // Logged in from the login screen: in once connected.
                    if let Some(login) = login.as_mut() {
                        login.entering = on_login;
                    }
                }
            }
            Ok(Reply::SignedOut(result)) => {
                if let Err(problem) = &result {
                    warn!("signing out: {}", problem.message());
                }
                signing.signed_out(result);
                // Logged out to the login screen: nothing to read until the player goes in.
                if !on_login {
                    reconnect(&net, &mut status);
                }
            }
            Ok(Reply::Friends(result)) => {
                if let Err(problem) = &result {
                    warn!("reading the friends list: {}", problem.message());
                }
                if friends.bypass_change_detection().read(result) {
                    friends.set_changed();
                }
            }
            Ok(Reply::FriendDone { call, result }) => {
                if let Err(problem) = &result {
                    warn!("friends: {}", problem.message());
                }
                if result.is_ok() {
                    friends::accepted(&net, &mut watch, &call);
                }
                friends.done(&call, result);
            }
            Ok(Reply::FriendSettings(result)) => match result {
                Ok(settings) => friends.settings = Some(settings),
                Err(problem) => {
                    warn!("friends settings: {}", problem.message());
                    friends.problem = Some(problem);
                }
            },
            Ok(Reply::AccountAddress(address)) => {
                if friends.address.as_ref() != Some(&address) {
                    friends.address = Some(address);
                }
            }
            Ok(Reply::Messages { with, result }) => match result {
                Ok(messages) => friends.history(&with, messages),
                Err(problem) => {
                    warn!("reading the messages: {}", problem.message());
                    friends.problem = Some(problem);
                }
            },
            Ok(Reply::Written { to, result }) => friends.written(&to, result),
            Ok(Reply::Message(message)) => friends.received(*message),
            Ok(Reply::Called { to, join, result }) => {
                if let Err(problem) = &result {
                    warn!("the call: {}", problem.message());
                }
                call.called(&net, &to, join, result);
            }
            Ok(Reply::IceServers(result)) => {
                call.ice_servers(&net, friends.address.as_deref(), result)
            }
            Ok(Reply::CallEvent(event)) => call.event(&net, event),
            Ok(Reply::CallSignal(signal)) => call.signal(*signal),
            Ok(Reply::VoiceJoined { room, result }) => voice.joined(&net, &room, result),
            Ok(Reply::VoicePeers { room, peers }) => voice.peers(&room, &peers),
            Ok(Reply::Voice { room, event }) => voice.event(&room, event),
            Ok(Reply::Connected(snapshot)) => {
                status.retry_in = None;
                // The connection lists every game; a narrower list is asked for again.
                if list.query == GameQuery::default() {
                    list.asked += 1;
                    if list.games != snapshot.games || list.problem.is_some() {
                        list.games = snapshot.games.clone();
                        list.problem = None;
                    }
                } else {
                    ask_games(&net, &mut list);
                }
                // Only touch the resource when something changed, so screens rebuild only then.
                if status.connection != Connection::Ready(snapshot.clone()) {
                    status.connection = Connection::Ready(snapshot);
                }
            }
            Ok(Reply::Probed { asked, result }) => {
                if let Some(login) = login.as_mut() {
                    login.probed(asked, result);
                }
            }
            Ok(Reply::Failed(problem)) => {
                warn!("{}", problem.message());
                status.retry_in = problem
                    .worth_retrying()
                    .then(|| Timer::new(RETRY_AFTER, TimerMode::Once));
                status.connection = Connection::Failed(problem);
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                let problem = Problem::Unreachable("The network thread stopped.".into());
                if status.connection != Connection::Failed(problem.clone()) {
                    error!("the network thread stopped");
                    status.connection = Connection::Failed(problem);
                    status.retry_in = None;
                }
                break;
            }
        }
    }
}

/// Whether the main screen shows (rather than the login screen), for what only it does: trying
/// a lost server again, reading the list of games.
fn on_main(screen: Option<Res<Screen>>) -> bool {
    screen.as_deref() != Some(&Screen::Login)
}

fn retry_later(time: Res<Time>, net: Res<Net>, mut status: ResMut<ServerStatus>) {
    let Some(timer) = status.bypass_change_detection().retry_in.as_mut() else {
        return;
    };
    if timer.tick(time.delta()).just_finished() {
        status.retry_in = None;
        status.connection = Connection::Connecting;
        net.send(Request::Connect);
    }
}

/// F5 connects again, as in a browser.
fn refresh_now(keys: Res<ButtonInput<KeyCode>>, net: Res<Net>, mut status: ResMut<ServerStatus>) {
    if keys.just_pressed(KeyCode::F5) {
        reconnect(&net, &mut status);
    }
}

/// How often the list of games is read again while it is on screen.
pub const GAMES_EVERY: Duration = Duration::from_secs(5);

/// The server's unfinished games, as the list shows them: what was asked for and what came back.
#[derive(Resource, Debug)]
pub struct GameList {
    pub query: GameQuery,
    /// The name of the game `query.family` stands for, to show.
    pub family_name: String,
    pub games: Vec<OpenGame>,
    /// Why the last reading failed; the games shown are from the one before.
    pub problem: Option<Problem>,
    /// Numbers each question, so that the answer to an older one is dropped.
    asked: u64,
    refresh: Timer,
}

impl Default for GameList {
    fn default() -> Self {
        GameList {
            query: GameQuery::default(),
            family_name: String::new(),
            games: vec![],
            problem: None,
            asked: 0,
            refresh: Timer::new(GAMES_EVERY, TimerMode::Repeating),
        }
    }
}

/// Lists the games that match `query` in place of the current ones, from now on.
pub fn find_games(net: &Net, list: &mut GameList, query: GameQuery, family_name: String) {
    list.query = query;
    list.family_name = family_name;
    ask_games(net, list);
}

fn ask_games(net: &Net, list: &mut GameList) {
    list.asked += 1;
    list.refresh.reset();
    net.send(Request::Games {
        query: list.query.clone(),
        asked: list.asked,
    });
}

/// Reads the list again every few seconds while it is on screen: connected, and no game open.
fn refresh_games(
    time: Res<Time>,
    net: Res<Net>,
    status: Res<ServerStatus>,
    watch: Res<WatchStatus>,
    mut list: ResMut<GameList>,
) {
    if watch.is_watching() || !matches!(status.connection, Connection::Ready(_)) {
        return;
    }
    let list = list.bypass_change_detection();
    if list.refresh.tick(time.delta()).just_finished() {
        ask_games(&net, list);
    }
}

/// The game being watched, if any, for screens to draw.
#[derive(Resource, Debug, Default)]
pub struct WatchStatus {
    /// Numbers each watch, so that replies to an earlier one are dropped.
    id: u64,
    pub target: Option<Target>,
    /// The game, once it has been opened.
    pub table: Option<Table>,
    /// Why it could not be opened, or was lost.
    pub problem: Option<Problem>,
    /// When a lost game will be opened again by itself.
    pub retry_in: Option<Timer>,
    /// This client's seat's impulse, while the game waits for it.
    pub turn: Option<Turn>,
    /// Giving up the game, for the seat this client plays.
    pub resign: Resign,
    /// Why the server turned down the last thing asked of the lobby.
    pub lobby_problem: Option<Problem>,
}

/// Where resigning stands. It takes two presses of the button, as it cannot be taken back.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Resign {
    #[default]
    No,
    /// Asked once: the panel asks whether the player is sure.
    Confirming,
    /// Sent and taken; the game's end is on its way.
    Sent,
    Failed(Problem),
}

impl WatchStatus {
    pub fn is_watching(&self) -> bool {
        self.target.is_some()
    }
}

/// Starts watching, in place of whatever was watched before.
pub fn watch(net: &Net, status: &mut WatchStatus, target: Target) {
    status.id += 1;
    status.target = Some(target.clone());
    status.table = None;
    status.problem = None;
    status.retry_in = None;
    status.turn = None;
    status.resign = Resign::No;
    status.lobby_problem = None;
    net.send(Request::Watch {
        target,
        watch: status.id,
    });
}

pub fn stop_watching(net: &Net, status: &mut WatchStatus) {
    status.id += 1;
    status.target = None;
    status.table = None;
    status.problem = None;
    status.retry_in = None;
    status.turn = None;
    status.resign = Resign::No;
    status.lobby_problem = None;
    net.send(Request::StopWatching);
}

fn rewatch_later(time: Res<Time>, net: Res<Net>, mut status: ResMut<WatchStatus>) {
    let Some(timer) = status.bypass_change_detection().retry_in.as_mut() else {
        return;
    };
    if timer.tick(time.delta()).just_finished()
        && let Some(target) = status.target.clone()
    {
        // Keep the table showing while it opens again.
        status.id += 1;
        status.retry_in = None;
        status.problem = None;
        net.send(Request::Watch {
            target,
            watch: status.id,
        });
    }
}

/// Gives up the watched game, for the seat this client plays.
pub fn resign(net: &Net, status: &mut WatchStatus) {
    let Some(player_name) = status
        .table
        .as_ref()
        .filter(|t| !t.finished())
        .and_then(|t| t.yours())
        .map(|s| s.name.clone())
    else {
        return;
    };
    status.resign = Resign::Sent;
    net.send(Request::Resign {
        watch: status.id,
        player_name,
    });
}

/// Before the watched game starts, does `call` for the seat this client plays.
pub fn lobby(net: &Net, status: &mut WatchStatus, call: LobbyCall) {
    let Some(player_name) = status
        .table
        .as_ref()
        .filter(|t| t.state.status == "waiting")
        .and_then(|t| t.yours())
        .map(|s| s.name.clone())
    else {
        return;
    };
    status.lobby_problem = None;
    net.send(Request::Lobby {
        watch: status.id,
        player_name,
        call,
    });
}

/// Asks what the turn may select next, after the selections it has made.
pub fn ask(net: &Net, status: &WatchStatus) {
    if let Some(turn) = &status.turn {
        net.send(Request::Selections {
            watch: status.id,
            asked_at: turn.asked_at,
            made: turn.made_notations(),
            ply: turn.ply,
        });
    }
}

/// Sends the turn's move, for the seat this client plays.
pub fn play(net: &Net, status: &WatchStatus, notation: String, ply: u64) {
    let (Some(turn), Some(table)) = (&status.turn, &status.table) else {
        return;
    };
    let Some(player_name) = table.yours().map(|s| s.name.clone()) else {
        return;
    };
    net.send(Request::Play {
        watch: status.id,
        asked_at: turn.asked_at,
        player_name,
        notation,
        ply,
    });
}

/// Connects again now, whatever the state.
pub fn reconnect(net: &Net, status: &mut ServerStatus) {
    status.retry_in = None;
    if !matches!(status.connection, Connection::Ready(_)) {
        status.connection = Connection::Connecting;
    }
    net.send(Request::Connect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::api::{Board, Game, GameInfo, GameSeat, Gesture, LegalSelection};
    use crate::play::{self, Stage};
    use bevy::ecs::system::RunSystemOnce;
    use std::sync::Arc;

    fn opened() -> Opened {
        Opened {
            room_code: "AB12CD".into(),
            info: GameInfo {
                id: "g".into(),
                name: "A game".into(),
                seats: vec![],
                board: Board {
                    kind: "hex".into(),
                    positions: vec![],
                    regions: Default::default(),
                },
                actions: vec![],
                goals: vec![],
            },
            game: Game {
                game_def_id: "g".into(),
                status: "active".into(),
                invite_url: String::new(),
                clock: None,
                impulse_deadline: String::new(),
                server_time: String::new(),
                move_time_seconds: None,
                lobby: false,
                host_seat: Some(1),
                reservations: vec![],
                seats: vec![
                    GameSeat {
                        seat: 1,
                        color: "red".into(),
                        name: "Ana".into(),
                        bot: String::new(),
                        yours: true,
                    },
                    GameSeat {
                        seat: 2,
                        color: "blue".into(),
                        name: "Bot".into(),
                        bot: "mcts".into(),
                        yours: false,
                    },
                ],
            },
            state: GameState {
                status: "active".into(),
                impulse_seat: Some(1),
                move_seq: 2,
                ..Default::default()
            },
            bots: vec![],
        }
    }

    fn place(cell: &str) -> LegalSelection {
        LegalSelection {
            notation: cell.into(),
            action: "place".into(),
            to: Some(cell.into()),
            gestures: vec![
                Gesture {
                    kind: "piece".into(),
                    value: "stone".into(),
                },
                Gesture::pos(cell),
            ],
        }
    }

    fn selections(made: &[&str], options: Vec<LegalSelection>) -> Selections {
        Selections {
            seat: 1,
            ply: 2,
            made: made.iter().map(|m| m.to_string()).collect(),
            complete: options.is_empty(),
            options,
        }
    }

    fn stage(app: &App) -> Option<Stage> {
        app.world()
            .resource::<WatchStatus>()
            .turn
            .as_ref()
            .map(|t| t.stage)
    }

    #[test]
    fn the_list_takes_the_answer_to_the_last_question_only() {
        let (net, requests, replies) = Net::for_test_with_replies();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(ServerStatus {
                server: "http://127.0.0.1:8080".into(),
                connection: Connection::Connecting,
                notices: vec![],
                retry_in: None,
            })
            .init_resource::<WatchStatus>()
            .init_resource::<GameList>()
            .init_resource::<NewGame>()
            .init_resource::<Signing>()
            .init_resource::<DirectCalling>()
            .init_resource::<VoiceChat>()
            .init_resource::<Friends>()
            .add_systems(PreUpdate, poll_network);
        let open = |code: &str| OpenGame {
            room_code: code.into(),
            game_def_id: "hex-7".into(),
            name: "Hex (7×7)".into(),
            status: "waiting".into(),
            seats: 2,
            open_seats: 1,
            players: vec![],
            impulse_seat: None,
            lobby: false,
            reserved_for_you: false,
        };
        {
            let mut list = app.world_mut().resource_mut::<GameList>();
            let query = GameQuery {
                family: None,
                open_seats: true,
            };
            find_games(&net, &mut list, query.clone(), String::new());
            find_games(&net, &mut list, query, String::new());
        }
        assert_eq!(requests.try_iter().count(), 2);
        for (asked, code) in [(2, "NEWER1"), (1, "OLDER1")] {
            replies
                .send(Reply::Games {
                    asked,
                    result: Ok(vec![open(code)]),
                })
                .unwrap();
        }
        // A new connection while the list is narrowed asks for the narrow list again, rather
        // than show every game.
        replies
            .send(Reply::Connected(Snapshot {
                server: "http://127.0.0.1:8080".into(),
                version: "1".into(),
                api_version: "1.6.0".into(),
                defs: vec![],
                games: vec![open("EVERY1")],
                account: None,
                session_ended: false,
            }))
            .unwrap();
        app.insert_resource(net);
        app.update();
        let list = app.world().resource::<GameList>();
        let codes: Vec<&str> = list.games.iter().map(|g| g.room_code.as_str()).collect();
        assert_eq!(codes, ["NEWER1"]);
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::Games { asked: 3, .. })
        ));
    }

    #[test]
    fn resigning_is_sent_for_this_clients_seat_and_a_refusal_is_shown() {
        let (net, requests, replies) = Net::for_test_with_replies();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(ServerStatus {
                server: "http://127.0.0.1:8080".into(),
                connection: Connection::Connecting,
                notices: vec![],
                retry_in: None,
            })
            .init_resource::<WatchStatus>()
            .init_resource::<GameList>()
            .init_resource::<NewGame>()
            .init_resource::<Signing>()
            .init_resource::<DirectCalling>()
            .init_resource::<VoiceChat>()
            .init_resource::<Friends>()
            .add_systems(PreUpdate, poll_network);
        let mut status = WatchStatus {
            id: 3,
            table: Some(Table::new(opened())),
            ..Default::default()
        };
        resign(&net, &mut status);
        assert_eq!(status.resign, Resign::Sent);
        assert_eq!(
            requests.try_recv(),
            Ok(Request::Resign {
                watch: 3,
                player_name: "Ana".into()
            })
        );

        let refused = Problem::Refused {
            status: 409,
            message: "the game is over".into(),
        };
        replies
            .send(Reply::Resigned {
                watch: 3,
                result: Err(refused.clone()),
            })
            .unwrap();
        app.insert_resource(net).insert_resource(status);
        app.update();
        let status = app.world().resource::<WatchStatus>();
        assert_eq!(status.resign, Resign::Failed(refused));

        // A finished game cannot be given up.
        let (net, requests) = Net::for_test();
        let mut over = WatchStatus {
            table: Some(Table::new(opened())),
            ..Default::default()
        };
        over.table.as_mut().unwrap().state.status = "finished".into();
        resign(&net, &mut over);
        assert_eq!(over.resign, Resign::No);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn the_lobby_acts_for_this_clients_seat_shows_a_refusal_and_leaving_goes_back() {
        let (net, requests, replies) = Net::for_test_with_replies();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(ServerStatus {
                server: "http://127.0.0.1:8080".into(),
                connection: Connection::Connecting,
                notices: vec![],
                retry_in: None,
            })
            .init_resource::<GameList>()
            .init_resource::<NewGame>()
            .init_resource::<Signing>()
            .init_resource::<DirectCalling>()
            .init_resource::<VoiceChat>()
            .init_resource::<Friends>()
            .add_systems(PreUpdate, poll_network);
        let mut waiting = Table::new(opened());
        waiting.state.status = "waiting".into();
        let mut status = WatchStatus {
            id: 4,
            target: Some(Target::Room("AB12CD".into())),
            table: Some(waiting),
            ..Default::default()
        };
        lobby(&net, &mut status, LobbyCall::Remove(2));
        assert_eq!(
            requests.try_recv(),
            Ok(Request::Lobby {
                watch: 4,
                player_name: "Ana".into(),
                call: LobbyCall::Remove(2),
            })
        );
        let refused = Problem::Refused {
            status: 403,
            message: "only the host".into(),
        };
        replies
            .send(Reply::Lobby {
                watch: 4,
                call: LobbyCall::Remove(2),
                result: Err(refused.clone()),
            })
            .unwrap();
        app.insert_resource(net).insert_resource(status);
        app.update();
        let status = app.world().resource::<WatchStatus>();
        assert_eq!(status.lobby_problem, Some(refused));
        assert!(status.is_watching());

        replies
            .send(Reply::Lobby {
                watch: 4,
                call: LobbyCall::Leave,
                result: Ok(()),
            })
            .unwrap();
        app.update();
        let status = app.world().resource::<WatchStatus>();
        assert!(!status.is_watching(), "having left, back to the list");
        assert_eq!(status.lobby_problem, None);
        let sent: Vec<Request> = requests.try_iter().collect();
        assert_eq!(sent, [Request::StopWatching, Request::Connect]);

        // Once the game has started there is no lobby to act on.
        let (net, requests) = Net::for_test();
        let mut playing = WatchStatus {
            table: Some(Table::new(opened())),
            ..Default::default()
        };
        lobby(&net, &mut playing, LobbyCall::Leave);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn a_turn_asks_the_server_enters_a_move_and_sends_it_once_confirmed() {
        let (net, requests, replies) = Net::for_test_with_replies();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(net)
            .insert_resource(ServerStatus {
                server: "http://127.0.0.1:8080".into(),
                connection: Connection::Connecting,
                notices: vec![],
                retry_in: None,
            })
            .init_resource::<WatchStatus>()
            .init_resource::<GameList>()
            .init_resource::<NewGame>()
            .init_resource::<Signing>()
            .init_resource::<DirectCalling>()
            .init_resource::<VoiceChat>()
            .init_resource::<Friends>()
            .add_systems(PreUpdate, poll_network)
            .add_systems(Update, play::start_turn);
        let id = {
            let mut w = app.world_mut().resource_mut::<WatchStatus>();
            w.id = 7;
            w.table = Some(Table::new(opened()));
            w.id
        };
        app.update();
        assert_eq!(stage(&app), Some(Stage::Asking), "our seat is on impulse");
        assert_eq!(
            requests.try_recv(),
            Ok(Request::Selections {
                watch: id,
                asked_at: 2,
                made: vec![],
                ply: None,
            })
        );

        // An answer for an earlier turn is dropped.
        let answer = |asked_at, made: &[&str], options| Reply::Selections {
            watch: id,
            asked_at,
            made: made.iter().map(|m| m.to_string()).collect(),
            result: Ok(Box::new(selections(made, options))),
        };
        replies.send(answer(1, &[], vec![place("a1")])).unwrap();
        app.update();
        assert_eq!(stage(&app), Some(Stage::Asking));

        replies
            .send(answer(2, &[], vec![place("a1"), place("b1")]))
            .unwrap();
        app.update();
        assert_eq!(stage(&app), Some(Stage::Choosing));

        // A click on a cell, as the board's observer makes it.
        app.world_mut()
            .run_system_once(|net: Res<Net>, mut watch: ResMut<WatchStatus>| {
                play::gesture(&net, &mut watch, &Gesture::pos("b1"));
            })
            .unwrap();
        assert_eq!(
            requests.try_recv(),
            Ok(Request::Selections {
                watch: id,
                asked_at: 2,
                made: vec!["b1".into()],
                ply: Some(2),
            })
        );
        replies.send(answer(2, &["b1"], vec![])).unwrap();
        app.update();
        assert_eq!(stage(&app), Some(Stage::Ready), "shown, not yet sent");
        assert!(requests.try_recv().is_err());

        app.world_mut()
            .run_system_once(|net: Res<Net>, mut watch: ResMut<WatchStatus>| {
                play::submit(&net, &mut watch);
            })
            .unwrap();
        assert_eq!(
            requests.try_recv(),
            Ok(Request::Play {
                watch: id,
                asked_at: 2,
                player_name: "Ana".into(),
                notation: "b1".into(),
                ply: 2,
            })
        );
        assert_eq!(stage(&app), Some(Stage::Sending));

        // Refused because the game moved on: the turn starts again, and the state is read.
        replies
            .send(Reply::Played {
                watch: id,
                asked_at: 2,
                result: Err(Problem::Refused {
                    status: 409,
                    message: "the game has moved on".into(),
                }),
            })
            .unwrap();
        app.update();
        assert_eq!(stage(&app), Some(Stage::Asking));
        let sent: Vec<Request> = requests.try_iter().collect();
        assert_eq!(
            sent,
            [
                Request::Selections {
                    watch: id,
                    asked_at: 2,
                    made: vec![],
                    ply: None,
                },
                Request::Reload { watch: id },
            ]
        );

        // The bot's impulse: no turn.
        app.world_mut()
            .resource_mut::<WatchStatus>()
            .table
            .as_mut()
            .unwrap()
            .state
            .impulse_seat = Some(2);
        app.update();
        assert_eq!(stage(&app), None);
    }

    #[test]
    fn the_account_listens_before_it_says_it_is_here() {
        use session::tests::{Seen, accounts, serve, session};
        let inner = accounts(false, Arc::new(Mutex::new(false)));
        let (addr, seen) = serve(Box::new(move |req: &Seen| match req.path.as_str() {
            "/api/here" => (200, r#"{"people":[]}"#.into()),
            "/api/friends" => (
                200,
                r#"{"friends":[],"incoming":[],"outgoing":[],"invites":[]}"#.into(),
            ),
            _ => inner(req),
        }));
        let mut s = session(&addr);
        s.sign_in("ana", "pw").unwrap();
        AccountLink::default().tick(&mut s, Instant::now());
        let paths: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.path.clone())
            .filter(|p| p == "/ws-dm" || p == "/api/here")
            .collect();
        // A friend who sees the account here may ring it at once: the socket must be listening.
        assert_eq!(paths, ["/ws-dm", "/api/here"]);
    }
}
