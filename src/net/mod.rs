//! The connection to a Bingle server.
//!
//! All network I/O happens on one thread of its own, with blocking calls, so no Bevy system ever
//! waits on the network. Systems send it [`Request`]s; it answers with what happened, which
//! [`poll_network`] turns into the [`ServerStatus`] and [`WatchStatus`] resources the screens draw
//! from.
//!
//! While a game is watched, its WebSocket is read in the same loop, with a short timeout
//! ([`socket::POLL`]), so requests are still answered in between and there is still one thread.

pub mod api;
pub mod server_url;
pub mod session;
pub mod socket;
pub mod tls;

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use bevy::prelude::*;

use crate::config::Store;
use crate::table::{Step, Table};
use api::{Event, GameState};
use session::{Opened, Problem, Session, Snapshot};
use socket::Socket;
use tls::Fingerprint;

pub struct NetPlugin {
    /// A server for this run, in place of the configured one.
    pub server: Option<String>,
}

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        let store = Store::load(Store::default_dir());
        let notices = store.problems().to_vec();
        for notice in &notices {
            warn!("{notice}");
        }
        let session = Session::new(self.server.as_deref(), store);
        let status = ServerStatus {
            server: session.server(),
            connection: Connection::Connecting,
            notices,
            retry_in: None,
        };
        let net = Net::spawn(session);
        net.send(Request::Connect);
        app.insert_resource(net)
            .insert_resource(status)
            .init_resource::<WatchStatus>()
            .add_systems(PreUpdate, poll_network)
            .add_systems(Update, (retry_later, refresh_now, rewatch_later));
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
    /// Close the watched game's socket.
    StopWatching,
}

/// What to watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A new match between two of the server's bots, in this game.
    Bots { game_def_id: String },
    /// A game already being played.
    Room(String),
}

enum Reply {
    Connected(Snapshot),
    Failed(Problem),
    Opened { watch: u64, opened: Box<Opened> },
    Event { watch: u64, event: Box<Event> },
    Reloaded { watch: u64, state: Box<GameState> },
    WatchFailed { watch: u64, problem: Problem },
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
        let (requests, inbox) = mpsc::channel();
        let (_, replies) = mpsc::channel();
        let net = Net {
            requests,
            replies: Mutex::new(replies),
        };
        (net, inbox)
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
    loop {
        let request = if watching.is_some() {
            match inbox.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        } else {
            match inbox.recv() {
                Ok(r) => Some(r),
                Err(_) => break,
            }
        };
        let reply = match request {
            Some(request) => handle(&mut session, &mut watching, request),
            None => read_socket(&session, &mut watching),
        };
        if let Some(reply) = reply
            && outbox.send(reply).is_err()
        {
            break;
        }
    }
    if let Some(w) = watching {
        w.socket.close();
    }
}

fn handle(
    session: &mut Session,
    watching: &mut Option<Watching>,
    request: Request,
) -> Option<Reply> {
    match request {
        Request::Connect => Some(connected(session.connect())),
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
        Request::StopWatching => {
            if let Some(old) = watching.take() {
                old.socket.close();
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

fn poll_network(net: Res<Net>, mut status: ResMut<ServerStatus>, mut watch: ResMut<WatchStatus>) {
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
            // For a watch that has since been replaced.
            Ok(
                Reply::Opened { .. }
                | Reply::Event { .. }
                | Reply::Reloaded { .. }
                | Reply::WatchFailed { .. },
            ) => {}
            Ok(Reply::Connected(snapshot)) => {
                status.retry_in = None;
                // Only touch the resource when something changed, so screens rebuild only then.
                if status.connection != Connection::Ready(snapshot.clone()) {
                    status.connection = Connection::Ready(snapshot);
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

/// Connects again now, whatever the state.
pub fn reconnect(net: &Net, status: &mut ServerStatus) {
    status.retry_in = None;
    if !matches!(status.connection, Connection::Ready(_)) {
        status.connection = Connection::Connecting;
    }
    net.send(Request::Connect);
}
