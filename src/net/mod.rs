//! The connection to a Bingle server.
//!
//! All network I/O happens on one thread of its own, with blocking calls, so no Bevy system ever
//! waits on the network. Systems send it [`Request`]s; it answers with what happened, which
//! [`poll_network`] turns into the [`ServerStatus`] resource the screens draw from.

pub mod api;
pub mod server_url;
pub mod session;
pub mod tls;

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use bevy::prelude::*;

use crate::config::Store;
use session::{Problem, Session, Snapshot};
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
            .add_systems(PreUpdate, poll_network)
            .add_systems(Update, (retry_later, refresh_now));
    }
}

/// What a screen can ask of the network thread.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Connect, or connect again: index, version, guest key, games.
    Connect,
    /// The player has looked at this certificate's fingerprint and says it is the server's.
    Trust(Fingerprint),
}

enum Reply {
    Connected(Snapshot),
    Failed(Problem),
}

#[derive(Resource)]
pub struct Net {
    requests: Sender<Request>,
    // A `Mutex` only to make the receiver `Sync`, as a resource must be; only `poll_network`
    // locks it.
    replies: Mutex<Receiver<Reply>>,
}

impl Net {
    fn spawn(mut session: Session) -> Self {
        let (requests, inbox) = mpsc::channel::<Request>();
        let (outbox, replies) = mpsc::channel::<Reply>();
        std::thread::Builder::new()
            .name("bingle-net".into())
            .spawn(move || {
                // Ends when the app drops its sender.
                while let Ok(request) = inbox.recv() {
                    let result = match request {
                        Request::Connect => session.connect(),
                        Request::Trust(fp) => session.trust(fp),
                    };
                    let reply = match result {
                        Ok(snapshot) => Reply::Connected(snapshot),
                        Err(problem) => Reply::Failed(problem),
                    };
                    if outbox.send(reply).is_err() {
                        break;
                    }
                }
            })
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

fn poll_network(net: Res<Net>, mut status: ResMut<ServerStatus>) {
    let replies = net.replies.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        match replies.try_recv() {
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

/// Connects again now, whatever the state.
pub fn reconnect(net: &Net, status: &mut ServerStatus) {
    status.retry_in = None;
    if !matches!(status.connection, Connection::Ready(_)) {
        status.connection = Connection::Connecting;
    }
    net.send(Request::Connect);
}
