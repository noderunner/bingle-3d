//! Talking to one server, blocking: it runs on the network thread, never in a Bevy system.
//!
//! [`Session::connect`] does what start-up needs: read `GET /api` and check the API version, make
//! sure there is a guest key, then list the games and the open games. Whatever goes wrong comes
//! back as a [`Problem`] the player can understand.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use ureq::Agent;
use ureq::unversioned::resolver::DefaultResolver;
use ureq::unversioned::transport::{ConnectProxyConnector, Connector, TcpConnector};

use rustls::ClientConfig;

use super::GameQuery;
use super::api::{
    self, BotInfo, BotMatchRequest, BotMatchResponse, Compatibility, CreateGameRequest,
    CreateGameResponse, ErrorBody, Game, GameDef, GameInfo, GameSeat, GameState, GuestKey, Index,
    JoinRequest, JoinResponse, MoveRequest, MoveResponse, OpenGame, OpenGames, PlayerRequest,
    Selections,
};
use super::server_url::{Scheme, ServerUrl, query_value};
use super::socket::{self, Role, Socket};
use super::tls::{Fingerprint, TlsConnector, Trust};
use crate::config::Store;

/// What a server has, once connected.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// The address, as the player would write it.
    pub server: String,
    pub version: String,
    pub api_version: String,
    pub defs: Vec<GameDef>,
    pub games: Vec<OpenGame>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// The configured address cannot be used (including plain HTTP beyond this machine).
    BadAddress(String),
    /// Nothing answered: not running, wrong address, or no network. Worth trying again.
    Unreachable(String),
    /// The certificate is not one an authority vouches for, nor one the player has trusted.
    Untrusted {
        fingerprint: Fingerprint,
        reason: String,
    },
    /// A secure connection could not be made for some other reason.
    Insecure(String),
    /// Something answered, but not as a Bingle server does.
    NotBingle(String),
    /// A Bingle server whose API this client cannot use.
    Incompatible(String),
    /// The server admits only signed-in accounts (accounts come in stage 6).
    NeedsAccount,
    /// The server said no, for a reason of its own.
    Refused { status: u16, message: String },
}

impl Problem {
    /// In words for the player.
    pub fn message(&self) -> String {
        match self {
            Problem::BadAddress(m)
            | Problem::Unreachable(m)
            | Problem::Insecure(m)
            | Problem::NotBingle(m)
            | Problem::Incompatible(m) => m.clone(),
            Problem::Untrusted { reason, .. } => {
                format!("The server's certificate is not trusted: {reason}.")
            }
            Problem::NeedsAccount => "This server only lets in people who have signed in to an \
                account, and this client cannot sign in yet."
                .into(),
            Problem::Refused { status, message } => {
                format!("The server turned the request down ({status}): {message}")
            }
        }
    }

    /// Whether trying again by itself, a little later, might help.
    pub fn worth_retrying(&self) -> bool {
        matches!(self, Problem::Unreachable(_))
    }
}

pub struct Session {
    url: Result<ServerUrl, String>,
    store: Store,
    trust: Arc<Trust>,
    tls: Arc<ClientConfig>,
    agent: Agent,
    /// Game definitions already read, by id: they do not change while the server runs.
    infos: HashMap<String, GameInfo>,
}

impl Session {
    /// `server` overrides the configured server for this run.
    pub fn new(server: Option<&str>, store: Store) -> Self {
        let address = server.unwrap_or(store.server()).to_string();
        let url = ServerUrl::parse(&address)
            .map_err(|e| format!("The server address \"{address}\" cannot be used: {e}."));
        let pins = match &url {
            Ok(url) => store
                .trusted(&url.origin())
                .filter_map(Fingerprint::parse)
                .collect(),
            Err(_) => Vec::new(),
        };
        let trust = Trust::new(pins);
        let tls = super::tls::client_config(trust.clone());
        let agent = agent(tls.clone());
        Session {
            url,
            store,
            trust,
            tls,
            agent,
            infos: HashMap::new(),
        }
    }

    /// The server's address as the player would write it, or as configured if it cannot be used.
    pub fn server(&self) -> String {
        match &self.url {
            Ok(url) => url.to_string(),
            Err(_) => self.store.server().to_string(),
        }
    }

    #[cfg(test)]
    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn connect(&mut self) -> Result<Snapshot, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let index: Index = match self.get(&url, "/api", None) {
            Err(Problem::Refused { status: 404, .. }) => {
                return Err(Problem::NotBingle(format!(
                    "{url} answered, but it has no Bingle API (GET /api was not found)."
                )));
            }
            other => other?,
        };
        let api_version = match api::compatibility(index.api_version.as_deref()) {
            Compatibility::Compatible => index.api_version.clone().unwrap_or_default(),
            Compatibility::ServerTooOld(v) => {
                return Err(Problem::Incompatible(format!(
                    "This server (Bingle {}) is too old for this client: its API is {}, and this \
                     client needs {}.{} or newer. Update the server.",
                    index.version,
                    v.unwrap_or_else(|| "from before API versions".into()),
                    api::API_VERSION.major,
                    api::API_VERSION.minor,
                )));
            }
            Compatibility::ClientTooOld(v) => {
                return Err(Problem::Incompatible(format!(
                    "This client is too old for this server: the server's API is {v}, and this \
                     client speaks {}.x. Update the client.",
                    api::API_VERSION.major,
                )));
            }
            Compatibility::Unreadable(v) => {
                return Err(Problem::NotBingle(format!(
                    "{url} reports an API version this client cannot read (\"{v}\")."
                )));
            }
        };
        let defs: Vec<GameDef> = self.get_as_guest(&url, "/api/game-defs")?;
        let games: OpenGames = self.get_as_guest(&url, "/api/games")?;
        Ok(Snapshot {
            server: url.to_string(),
            version: index.version,
            api_version,
            defs,
            games: games.games,
        })
    }

    /// Remembers that the player trusts this certificate for this server, and connects again.
    pub fn trust(&mut self, fingerprint: Fingerprint) -> Result<Snapshot, Problem> {
        if let Ok(url) = &self.url {
            self.trust.pin(fingerprint);
            if let Err(e) = self.store.trust(&url.origin(), &fingerprint.to_string()) {
                bevy::log::warn!("could not save the trusted certificate: {e}");
            }
        }
        self.connect()
    }

    /// Asks the server for two of its bots to play a game, and returns the room they play in.
    pub fn start_bot_match(&mut self, game_def_id: &str) -> Result<String, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let bots: Vec<BotInfo> = self.get_as_guest(&url, "/api/bots")?;
        let bot = pick_bot(&bots).ok_or_else(|| Problem::Refused {
            status: 404,
            message: "the server has no bots to play".into(),
        })?;
        let body = serde_json::to_value(BotMatchRequest {
            game_def_id: game_def_id.to_string(),
            bot_a: bot.to_string(),
            bot_b: bot.to_string(),
        })
        .expect("a bot match request is JSON");
        let made: BotMatchResponse =
            self.as_guest(&url, "POST", "/api/bot-matches", Some(&body))?;
        Ok(made.room_code)
    }

    /// Makes a game in which this client's guest plays the first seat and the server's bots
    /// every other, and returns its room.
    pub fn start_game_against_bots(&mut self, game_def_id: &str) -> Result<String, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let bots: Vec<BotInfo> = self.get_as_guest(&url, "/api/bots")?;
        let bot = pick_bot(&bots).ok_or_else(|| Problem::Refused {
            status: 404,
            message: "the server has no bots to play".into(),
        })?;
        self.create_game(game_def_id, Some(bot.to_string()))
    }

    /// A new game with this client in the first seat and the others open, for people to join.
    pub fn host_game(&mut self, game_def_id: &str) -> Result<String, Problem> {
        self.create_game(game_def_id, None)
    }

    fn create_game(
        &mut self,
        game_def_id: &str,
        opponent: Option<String>,
    ) -> Result<String, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(CreateGameRequest {
            game_def_id: game_def_id.to_string(),
            player_name: self.store.player_name().to_string(),
            opponent,
        })
        .expect("a game request is JSON");
        let made: CreateGameResponse = self.as_guest(&url, "POST", "/api/games", Some(&body))?;
        Ok(made.room_code)
    }

    /// The unfinished games that match `query`, newest change first.
    pub fn games(&mut self, query: &GameQuery) -> Result<Vec<OpenGame>, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let list: OpenGames = self.get_as_guest(&url, &games_path(query))?;
        Ok(list.games)
    }

    /// Takes an open seat in the game in this room, if it is waiting for players and this client
    /// has no seat there yet; otherwise leaves it to be watched (or played, from the seat that is
    /// already this client's).
    pub fn join_game(&mut self, room_code: &str) -> Result<(), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        if !valid_room_code(room_code) {
            return Err(not_a_room_code(room_code));
        }
        let game: Game = self.get_as_guest(&url, &format!("/api/games/{room_code}"))?;
        if game.status != "waiting" || game.seats.iter().any(|s| s.yours) || !game.has_open_seat() {
            return Ok(());
        }
        // A seat goes by its player's name, so the name must not be one already at the table.
        let body = serde_json::to_value(JoinRequest {
            player_name: free_name(self.store.player_name(), &game.seats),
            role: "player".into(),
        })
        .expect("a join is JSON");
        let _: JoinResponse = self.as_guest(
            &url,
            "POST",
            &format!("/api/games/{room_code}/join"),
            Some(&body),
        )?;
        Ok(())
    }

    /// Gives up the game for the seat `player_name` sits at.
    pub fn resign(&mut self, room_code: &str, player_name: &str) -> Result<(), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(PlayerRequest {
            player_name: player_name.to_string(),
        })
        .expect("a resignation is JSON");
        let _: GameState = self.as_guest(
            &url,
            "POST",
            &format!("/api/games/{room_code}/resign"),
            Some(&body),
        )?;
        Ok(())
    }

    /// What the seat on impulse may select after the selections `made`, at `ply` once known.
    pub fn selections(
        &mut self,
        room_code: &str,
        made: &[String],
        ply: Option<u64>,
    ) -> Result<Selections, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let mut path = format!(
            "/api/games/{room_code}/selections?made={}",
            query_value(&made.join("; "))
        );
        if let Some(ply) = ply {
            path.push_str(&format!("&ply={ply}"));
        }
        self.get_as_guest(&url, &path)
    }

    /// Plays a whole move for the seat that `player_name` sits at, against `ply`.
    pub fn play(
        &mut self,
        room_code: &str,
        player_name: &str,
        notation: &str,
        ply: u64,
    ) -> Result<GameState, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(MoveRequest {
            player_name: player_name.to_string(),
            notation: notation.to_string(),
            ply,
        })
        .expect("a move is JSON");
        let made: MoveResponse = self.as_guest(
            &url,
            "POST",
            &format!("/api/games/{room_code}/moves"),
            Some(&body),
        )?;
        Ok(made.state)
    }

    /// Everything needed to draw a game: its definition, who sits where, and where it stands.
    /// The socket is opened first, so that no move is missed between the two: one played in
    /// between is either in the state read after it, or arrives on the socket. It is opened for
    /// the player of a seat this guest holds, if there is one, and as a spectator otherwise.
    pub fn open_game(&mut self, room_code: &str) -> Result<(Opened, Socket), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        if !valid_room_code(room_code) {
            return Err(not_a_room_code(room_code));
        }
        let game: Game = self.get_as_guest(&url, &format!("/api/games/{room_code}"))?;
        let info = match self.infos.get(&game.game_def_id) {
            Some(info) => info.clone(),
            None => {
                let info: GameInfo =
                    self.get_as_guest(&url, &format!("/api/game-defs/{}", game.game_def_id))?;
                self.infos.insert(info.id.clone(), info.clone());
                info
            }
        };
        let key = self.guest_key(&url)?;
        let role = match game.seats.iter().find(|s| s.yours) {
            Some(seat) => Role::Player {
                name: &seat.name,
                key: &key,
            },
            None => Role::Spectator,
        };
        let socket = Socket::open(&url, room_code, role, self.tls.clone())
            .map_err(|e| self.socket_problem(&url, e))?;
        let state = self.state(room_code)?;
        let opened = Opened {
            room_code: room_code.to_string(),
            info,
            game,
            state,
        };
        Ok((opened, socket))
    }

    /// Where a game stands now.
    pub fn state(&mut self, room_code: &str) -> Result<GameState, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        self.get_as_guest(&url, &format!("/api/games/{room_code}/state"))
    }

    /// What a failure on an open game socket means for the player.
    pub fn socket_failed(&self, err: socket::Error) -> Problem {
        match &self.url {
            Ok(url) => self.socket_problem(url, err),
            Err(m) => Problem::BadAddress(m.clone()),
        }
    }

    fn socket_problem(&self, url: &ServerUrl, err: socket::Error) -> Problem {
        if let Some(rejection) = self.trust.take_rejection() {
            return Problem::Untrusted {
                fingerprint: rejection.fingerprint,
                reason: rejection.reason,
            };
        }
        let at = format!("{}:{}", url.host, url.port);
        match err {
            socket::Error::HostNotFound => Problem::Unreachable(format!(
                "No computer called \"{}\" could be found.",
                url.host
            )),
            socket::Error::Io(e) if is_unreachable(&e) => Problem::Unreachable(format!(
                "Nothing is answering at {at}. Is Bingle running there?"
            )),
            socket::Error::Refused { status, message } => Problem::Refused { status, message },
            socket::Error::Closed => {
                Problem::Unreachable("The server closed the game's connection.".into())
            }
            other => {
                Problem::Unreachable(format!("The game's connection to {url} failed: {other}."))
            }
        }
    }

    /// A request with this client's guest key, made first if there is none. A key the server no
    /// longer knows (unused for months, or a new database) is replaced once.
    fn as_guest<T: DeserializeOwned>(
        &mut self,
        url: &ServerUrl,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<T, Problem> {
        let origin = url.origin();
        let had_key = self.store.guest_key(&origin).is_some();
        let key = self.guest_key(url)?;
        match self.call(url, method, path, Some(&key), body) {
            Err(Problem::Refused { status: 401, .. }) if had_key => {
                bevy::log::info!(
                    "the server no longer knows this client's guest key; making a new one"
                );
                self.forget_guest_key(&origin);
                let key = self.guest_key(url)?;
                self.call(url, method, path, Some(&key), body)
                    .map_err(unauthorized)
            }
            other => other.map_err(unauthorized),
        }
    }

    fn get_as_guest<T: DeserializeOwned>(
        &mut self,
        url: &ServerUrl,
        path: &str,
    ) -> Result<T, Problem> {
        self.as_guest(url, "GET", path, None)
    }

    fn guest_key(&mut self, url: &ServerUrl) -> Result<String, Problem> {
        let origin = url.origin();
        if let Some(key) = self.store.guest_key(&origin) {
            return Ok(key.to_string());
        }
        let made: GuestKey = self
            .call(url, "POST", "/api/guest", None, None)
            .map_err(unauthorized)?;
        if let Err(e) = self.store.set_guest_key(&origin, Some(made.key.clone())) {
            bevy::log::warn!("could not save the guest key: {e}");
        }
        Ok(made.key)
    }

    fn forget_guest_key(&mut self, origin: &str) {
        if let Err(e) = self.store.set_guest_key(origin, None) {
            bevy::log::warn!("could not forget the guest key: {e}");
        }
    }

    fn get<T: DeserializeOwned>(
        &self,
        url: &ServerUrl,
        path: &str,
        key: Option<&str>,
    ) -> Result<T, Problem> {
        self.call(url, "GET", path, key, None)
    }

    fn call<T: DeserializeOwned>(
        &self,
        url: &ServerUrl,
        method: &str,
        path: &str,
        key: Option<&str>,
        body: Option<&serde_json::Value>,
    ) -> Result<T, Problem> {
        let target = url.join(path);
        let auth = key.map(|k| format!("Bearer {k}"));
        let result = match method {
            "POST" => {
                let mut req = self.agent.post(&target);
                if let Some(a) = &auth {
                    req = req.header("Authorization", a);
                }
                match body {
                    Some(b) => req.send_json(b),
                    None => req.send_empty(),
                }
            }
            _ => {
                let mut req = self.agent.get(&target);
                if let Some(a) = &auth {
                    req = req.header("Authorization", a);
                }
                req.call()
            }
        };
        let mut response = result.map_err(|e| self.transport_problem(url, e))?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return response.body_mut().read_json::<T>().map_err(|e| {
                Problem::NotBingle(format!(
                    "{url} answered {method} {path}, but not as Bingle does ({e})."
                ))
            });
        }
        if (300..400).contains(&status) {
            let to = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("somewhere else")
                .to_string();
            return Err(Problem::BadAddress(format!(
                "{url} sends this client to {to}. Use that address for the server instead."
            )));
        }
        let body = response.body_mut().read_to_string().unwrap_or_default();
        let message = serde_json::from_str::<ErrorBody>(&body)
            .map(|b| b.error)
            .unwrap_or_else(|_| format!("{method} {path} was answered with status {status}"));
        Err(Problem::Refused { status, message })
    }

    fn transport_problem(&self, url: &ServerUrl, err: ureq::Error) -> Problem {
        if let Some(rejection) = self.trust.take_rejection() {
            return Problem::Untrusted {
                fingerprint: rejection.fingerprint,
                reason: rejection.reason,
            };
        }
        let at = format!("{}:{}", url.host, url.port);
        match err {
            ureq::Error::HostNotFound => Problem::Unreachable(format!(
                "No computer called \"{}\" could be found.",
                url.host
            )),
            ureq::Error::Timeout(_) => {
                Problem::Unreachable(format!("{at} did not answer in time."))
            }
            ureq::Error::ConnectionFailed => Problem::Unreachable(format!(
                "Nothing is answering at {at}. Is Bingle running there?"
            )),
            ureq::Error::Io(e) if is_unreachable(&e) => Problem::Unreachable(format!(
                "Nothing is answering at {at}. Is Bingle running there?"
            )),
            ureq::Error::Io(e) if url.scheme == Scheme::Https => Problem::Insecure(format!(
                "A secure connection to {url} could not be made: {e}. (Is it serving plain \
                 HTTP? Then it must be on this machine, and reached with http://.)"
            )),
            ureq::Error::Protocol(e) => Problem::NotBingle(format!(
                "{url} answered, but not in HTTP ({e}). Is it a Bingle server?"
            )),
            e => Problem::Unreachable(format!("{url} could not be reached: {e}.")),
        }
    }
}

/// A game opened to watch: what [`Session::open_game`] read.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    pub room_code: String,
    pub info: GameInfo,
    pub game: Game,
    pub state: GameState,
}

/// The bots to watch, best first among those this client knows something about: ones that look
/// ahead make a game worth watching, and any bot will do on a server with others.
const PREFERRED_BOTS: [&str; 3] = ["mcts", "minimax", "tactical"];

pub fn pick_bot(bots: &[BotInfo]) -> Option<&str> {
    PREFERRED_BOTS
        .iter()
        .find_map(|id| bots.iter().find(|b| b.id == *id))
        .or_else(|| bots.first())
        .map(|b| b.id.as_str())
}

/// Room codes are letters and digits, which keeps them safe to put in a path.
/// `GET /api/games` for `query`, with as many games as the server lists at once.
pub fn games_path(query: &GameQuery) -> String {
    let mut path = "/api/games?limit=60".to_string();
    if let Some(family) = &query.family {
        path.push_str("&family=");
        path.push_str(&query_value(family));
    }
    if query.open_seats {
        path.push_str("&open_seats=1");
    }
    path
}

/// `name`, or the first of "name 2", "name 3"… that nobody at the table goes by.
pub fn free_name(name: &str, seats: &[GameSeat]) -> String {
    let taken = |n: &str| seats.iter().any(|s| s.name.eq_ignore_ascii_case(n));
    if !taken(name) {
        return name.to_string();
    }
    (2..)
        .map(|i| format!("{name} {i}"))
        .find(|n| !taken(n))
        .expect("some number is free")
}

fn not_a_room_code(code: &str) -> Problem {
    Problem::BadAddress(format!("\"{code}\" is not a room code."))
}

pub fn valid_room_code(code: &str) -> bool {
    !code.is_empty() && code.len() <= 32 && code.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn is_unreachable(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    matches!(
        e.kind(),
        ConnectionRefused
            | ConnectionReset
            | ConnectionAborted
            | NotConnected
            | TimedOut
            | HostUnreachable
            | NetworkUnreachable
            | AddrNotAvailable
    )
}

/// A 401 outside the key's own replacement means the server wants an account. A 403 is about
/// what was asked (a seat that is somebody else's), and keeps the server's reason.
fn unauthorized(p: Problem) -> Problem {
    match p {
        Problem::Refused { status: 401, .. } => Problem::NeedsAccount,
        other => other,
    }
}

fn agent(tls: Arc<ClientConfig>) -> Agent {
    let config = Agent::config_builder()
        .http_status_as_error(false)
        // A redirect is reported rather than followed, so the address in use is the one shown.
        .max_redirects(0)
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(concat!("bingle-3d/", env!("CARGO_PKG_VERSION")))
        .build();
    let connector =
        ().chain(ConnectProxyConnector::default())
            .chain(TcpConnector::default())
            .chain(TlsConnector::new(tls));
    Agent::with_parts(config, connector, DefaultResolver::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    #[derive(Debug, Clone)]
    struct Seen {
        method: String,
        path: String,
        auth: Option<String>,
    }

    type Handler = dyn Fn(&Seen) -> (u16, String) + Send + Sync;

    /// A plain HTTP server on loopback that answers with `handler` and records what it was asked.
    fn serve(handler: Box<Handler>) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                let mut auth = None;
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        if name.eq_ignore_ascii_case("authorization") {
                            auth = Some(value.trim().to_string());
                        }
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                }
                // Read the body before answering, as a server does: closing with some of it
                // unread resets the connection, and the client may see that instead.
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let req = Seen { method, path, auth };
                let (status, body) = handler(&req);
                log.lock().unwrap().push(req);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (addr, seen)
    }

    const INDEX: &str = r#"{"version":"0.115.0","api_version":"1.2.0","docs":"/api/docs"}"#;
    const DEFS: &str =
        r#"[{"id":"hex-7","name":"Hex (7×7)","seats_required":2,"family":"hex-11"}]"#;
    const GAMES: &str = r#"{"games":[{"room_code":"N5WUVW","game_def_id":"hex-7","name":"Hex (7×7)","status":"active","seats":2,"open_seats":0,"players":[{"Seat":1,"Color":"red","Label":"","Name":"Ana","Bot":""}],"impulse_seat":1,"move_seq":0,"updated_at":"x"}]}"#;

    /// A Bingle that knows the keys in `valid` and hands out `bgg_new` when asked.
    fn bingle(index: &'static str, valid: &'static [&'static str]) -> Box<Handler> {
        Box::new(move |req: &Seen| {
            let key_ok = req
                .auth
                .as_deref()
                .and_then(|a| a.strip_prefix("Bearer "))
                .is_some_and(|k| valid.contains(&k));
            match (req.method.as_str(), req.path.as_str()) {
                ("GET", "/api") => (200, index.into()),
                ("POST", "/api/guest") => (201, r#"{"key":"bgg_new"}"#.into()),
                (_, "/api/game-defs") if key_ok => (200, DEFS.into()),
                (_, "/api/games") if key_ok => (200, GAMES.into()),
                (_, "/api/game-defs" | "/api/games") => (
                    401,
                    r#"{"error":"auth: that guest key is not one this server knows"}"#.into(),
                ),
                _ => (404, r#"{"error":"api: no such endpoint"}"#.into()),
            }
        })
    }

    fn session(addr: &str) -> Session {
        Session::new(Some(addr), Store::load(None))
    }

    #[test]
    fn connecting_reads_the_index_makes_a_guest_once_and_lists_the_games() {
        let (addr, seen) = serve(bingle(INDEX, &["bgg_new"]));
        let mut s = session(&addr);
        let snap = s.connect().expect("connects");
        assert_eq!(snap.api_version, "1.2.0");
        assert_eq!(snap.version, "0.115.0");
        assert_eq!(snap.defs[0].name, "Hex (7×7)");
        assert_eq!(snap.games[0].players[0].name, "Ana");

        s.connect().expect("again");
        let seen = seen.lock().unwrap();
        let guests = seen.iter().filter(|r| r.path == "/api/guest").count();
        assert_eq!(guests, 1, "the key is kept: {seen:?}");
        assert!(
            seen.iter()
                .filter(|r| r.path == "/api/games")
                .all(|r| r.auth.as_deref() == Some("Bearer bgg_new"))
        );
        assert!(
            seen.iter()
                .find(|r| r.path == "/api")
                .unwrap()
                .auth
                .is_none()
        );
    }

    #[test]
    fn a_key_the_server_has_forgotten_is_replaced() {
        let (addr, seen) = serve(bingle(INDEX, &["bgg_new"]));
        let mut store = Store::load(None);
        let origin = ServerUrl::parse(&addr).unwrap().origin();
        store
            .set_guest_key(&origin, Some("bgg_old".into()))
            .unwrap();
        let mut s = Session::new(Some(&addr), store);
        s.connect().expect("connects with a new key");
        assert_eq!(s.store().guest_key(&origin), Some("bgg_new"));
        assert_eq!(
            seen.lock()
                .unwrap()
                .iter()
                .filter(|r| r.path == "/api/guest")
                .count(),
            1
        );
    }

    #[test]
    fn a_server_that_will_not_take_a_guest_needs_an_account() {
        let (addr, _) = serve(bingle(INDEX, &[]));
        assert_eq!(session(&addr).connect(), Err(Problem::NeedsAccount));
    }

    #[test]
    fn the_api_version_is_checked_before_anything_else() {
        let (addr, seen) = serve(bingle(
            r#"{"version":"9.0.0","api_version":"2.0.0"}"#,
            &["bgg_new"],
        ));
        match session(&addr).connect() {
            Err(Problem::Incompatible(m)) => assert!(m.contains("client is too old"), "{m}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(seen.lock().unwrap().len(), 1, "nothing after the index");

        let (addr, _) = serve(bingle(r#"{"version":"0.110.0"}"#, &["bgg_new"]));
        match session(&addr).connect() {
            Err(Problem::Incompatible(m)) => {
                assert!(m.contains("too old for this client"), "{m}");
                assert!(m.contains("from before API versions"), "{m}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn something_that_is_not_bingle_says_so() {
        let (addr, _) = serve(Box::new(|_: &Seen| (404, "<html>nope</html>".into())));
        assert!(matches!(
            session(&addr).connect(),
            Err(Problem::NotBingle(_))
        ));
        let (addr, _) = serve(Box::new(|_: &Seen| (200, "<html>hello</html>".into())));
        assert!(matches!(
            session(&addr).connect(),
            Err(Problem::NotBingle(_))
        ));
    }

    #[test]
    fn nothing_listening_is_unreachable_and_worth_retrying() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let err = session(&format!("http://127.0.0.1:{port}"))
            .connect()
            .unwrap_err();
        assert!(matches!(err, Problem::Unreachable(_)), "{err:?}");
        assert!(err.worth_retrying());
        assert!(
            err.message().contains("Is Bingle running"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn a_game_against_the_bots_is_made_asked_about_and_played_as_the_guest() {
        let (addr, seen) = serve(Box::new(|req: &Seen| {
            match (req.method.as_str(), req.path.as_str()) {
                ("POST", "/api/guest") => (201, r#"{"key":"bgg_new"}"#.into()),
                ("GET", "/api/bots") => (
                    200,
                    r#"[{"id":"random","label":"Random"},{"id":"mcts","label":"MCTS"}]"#.into(),
                ),
                ("POST", "/api/games") => (
                    200,
                    r#"{"room_code":"AB12CD","seat":1,"color":"red","status":"active"}"#.into(),
                ),
                ("GET", p) if p.starts_with("/api/games/AB12CD/selections?") => (
                    200,
                    r#"{"seat":1,"ply":3,"made":["blue@d4"],"options":[],"complete":true}"#.into(),
                ),
                ("POST", "/api/games/AB12CD/moves") => (
                    200,
                    r#"{"move":{"seq":4},"state":{"status":"active","move_seq":4,"impulse_seat":2}}"#
                        .into(),
                ),
                _ => (404, r#"{"error":"api: no such endpoint"}"#.into()),
            }
        }));
        let mut s = session(&addr);
        assert_eq!(s.start_game_against_bots("hex-7").unwrap(), "AB12CD");
        let answer = s
            .selections("AB12CD", &["blue@d4".into(), "black@f6".into()], Some(3))
            .unwrap();
        assert!(answer.complete);
        assert_eq!(s.selections("AB12CD", &[], None).unwrap().ply, 3);
        let state = s.play("AB12CD", "Player", "d4", 3).unwrap();
        assert_eq!(state.move_seq, 4);

        let seen = seen.lock().unwrap();
        let paths: Vec<&str> = seen.iter().map(|r| r.path.as_str()).collect();
        assert!(
            paths.contains(&"/api/games/AB12CD/selections?made=blue%40d4%3B%20black%40f6&ply=3"),
            "the selections made are joined and encoded: {paths:?}"
        );
        assert!(paths.contains(&"/api/games/AB12CD/selections?made="));
        assert!(
            seen.iter()
                .filter(|r| r.path != "/api/guest")
                .all(|r| r.auth.as_deref() == Some("Bearer bgg_new")),
            "every call is the guest's: {seen:?}"
        );
    }

    #[test]
    fn joining_takes_an_open_seat_only_in_a_game_waiting_for_players() {
        let (addr, seen) = serve(Box::new(|req: &Seen| {
            let open = r#"{"seat":2,"color":"blue","name":""}"#;
            let ana = r#"{"seat":1,"color":"red","name":"Ana"}"#;
            let game = |status: &str, seats: &str| {
                format!(r#"{{"game_def_id":"hex-7","status":"{status}","seats":[{seats}]}}"#)
            };
            match (req.method.as_str(), req.path.as_str()) {
                ("POST", "/api/guest") => (201, r#"{"key":"bgg_new"}"#.into()),
                ("GET", "/api/games/WAIT01") => (200, game("waiting", &format!("{ana},{open}"))),
                ("GET", "/api/games/BUSY01") => (
                    200,
                    game("active", &format!(r#"{ana},{{"seat":2,"color":"blue","name":"Ben"}}"#)),
                ),
                ("GET", "/api/games/MINE01") => (
                    200,
                    game(
                        "waiting",
                        &format!(r#"{{"seat":1,"color":"red","name":"Ana","yours":true}},{open}"#),
                    ),
                ),
                ("POST", "/api/games/WAIT01/join") => (
                    200,
                    r#"{"role":"player","seat":2,"color":"blue","status":"active"}"#.into(),
                ),
                ("POST", "/api/games/BUSY01/resign") => (
                    200,
                    r#"{"status":"finished","impulse_seat":null,"move_seq":5,"ply":5,"winner_seat":2,"board_state":"","draw_offer_seat":null}"#.into(),
                ),
                _ => (404, r#"{"error":"api: no such endpoint"}"#.into()),
            }
        }));
        let mut s = session(&addr);
        for code in ["WAIT01", "BUSY01", "MINE01"] {
            s.join_game(code).unwrap();
        }
        assert!(matches!(
            s.join_game("not a code"),
            Err(Problem::BadAddress(_))
        ));
        s.resign("BUSY01", "Ana").unwrap();

        let seen = seen.lock().unwrap();
        let posts: Vec<&str> = seen
            .iter()
            .filter(|r| r.method == "POST" && r.path != "/api/guest")
            .map(|r| r.path.as_str())
            .collect();
        assert_eq!(
            posts,
            ["/api/games/WAIT01/join", "/api/games/BUSY01/resign"],
            "a game already playing, or with a seat of this guest's, is only watched"
        );
    }

    #[test]
    fn the_games_asked_for_are_narrowed_in_the_query() {
        assert_eq!(games_path(&GameQuery::default()), "/api/games?limit=60");
        assert_eq!(
            games_path(&GameQuery {
                family: Some("hex 11".into()),
                open_seats: true,
            }),
            "/api/games?limit=60&family=hex%2011&open_seats=1"
        );
    }

    #[test]
    fn a_name_already_at_the_table_is_numbered() {
        let seat = |name: &str| GameSeat {
            seat: 1,
            color: "red".into(),
            name: name.into(),
            bot: String::new(),
            yours: false,
        };
        assert_eq!(free_name("Player", &[seat(""), seat("Ana")]), "Player");
        assert_eq!(free_name("Player", &[seat("player")]), "Player 2");
        assert_eq!(
            free_name("Player", &[seat("Player"), seat("Player 2")]),
            "Player 3"
        );
    }

    #[test]
    fn plain_http_beyond_this_machine_is_refused_without_connecting() {
        let err = session("http://games.example.com").connect().unwrap_err();
        match &err {
            Problem::BadAddress(m) => assert!(m.contains("https://"), "{m}"),
            other => panic!("{other:?}"),
        }
        assert!(!err.worth_retrying());
    }
}
