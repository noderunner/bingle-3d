//! Talking to one server, blocking: it runs on the network thread, never in a Bevy system.
//!
//! [`Session::connect`] does what start-up needs: read `GET /api` and check the API version, make
//! sure there is a guest key (or that the account signed in still is), then list the games and the
//! open games. Whatever goes wrong comes back as a [`Problem`] the player can understand.
//!
//! Signed in to an account, every call and the game's socket carry the session's token in place
//! of the guest key. When the server no longer knows the token (it expired, or the account signed
//! out everywhere or changed its password), the client forgets it and goes on as a guest.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use ureq::Agent;
use ureq::unversioned::resolver::DefaultResolver;
use ureq::unversioned::transport::{ConnectProxyConnector, Connector, TcpConnector};

use rustls::ClientConfig;

use super::api::{
    self, BotInfo, BotMatchRequest, BotMatchResponse, Compatibility, CreateGameRequest,
    CreateGameResponse, ErrorBody, Game, GameDef, GameInfo, GameSeat, GameState, GuestKey, Index,
    JoinRequest, JoinResponse, MoveRequest, MoveResponse, OpenGame, OpenGames, PlayerRequest,
    Selections,
};
use super::api::{AddBotRequest, AddBotResult, GameStatus, SeatRequest};
use super::api::{LoginRequest, LogoutResponse, Me, SecondRequest, SessionTokenResponse};
use super::server_url::{Scheme, ServerUrl, query_value};
use super::socket::{self, Role, Socket};
use super::tls::{Fingerprint, TlsConnector, Trust};
use super::{GameQuery, GameSettings, LobbyCall, TimeControl};
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
    /// The account signed in, if one is.
    pub account: Option<Account>,
    /// The account that was signed in is no longer: the server ended its session.
    pub session_ended: bool,
}

/// The account this client is signed in to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub handle: String,
    pub display_name: String,
    /// The account must change its password (on the web page) before it can do anything else.
    pub must_change_password: bool,
}

impl Account {
    fn from_me(me: Me) -> Option<Self> {
        (me.kind == "account").then_some(Account {
            handle: me.handle,
            display_name: me.display_name,
            must_change_password: me.must_change_password,
        })
    }

    /// What the account is called at a table.
    pub fn name(&self) -> &str {
        if self.display_name.trim().is_empty() {
            &self.handle
        } else {
            &self.display_name
        }
    }
}

/// How a sign-in stands after a step of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    /// Signed in.
    Done(Account),
    /// The password was right; a code comes next, sent with `pending`.
    NeedsCode { pending: String },
}

/// The second step of a sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Code {
    /// From the account's authenticator app.
    App(String),
    /// One of the account's recovery codes.
    Recovery(String),
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
    /// The server admits only signed-in accounts.
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
            Problem::NeedsAccount => {
                "This server only lets in people who have signed in to an account.".into()
            }
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
    /// The account signed in, as the server last said.
    account: Option<Account>,
    /// The server ended the account's session since the last connection.
    session_ended: bool,
    /// The server's bots, once read; nor do they.
    bots: Option<Vec<BotInfo>>,
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
            bots: None,
            account: None,
            session_ended: false,
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
        self.check_account(&url)?;
        let defs: Vec<GameDef> = self.get_as_me(&url, "/api/game-defs")?;
        let games: OpenGames = self.get_as_me(&url, "/api/games")?;
        Ok(Snapshot {
            server: url.to_string(),
            version: index.version,
            api_version,
            defs,
            games: games.games,
            account: self.account.clone(),
            session_ended: std::mem::take(&mut self.session_ended),
        })
    }

    /// Who the session's token is, if this client holds one for this server.
    fn check_account(&mut self, url: &ServerUrl) -> Result<(), Problem> {
        let origin = url.origin();
        let Some(token) = self.store.session_token(&origin).map(str::to_string) else {
            self.account = None;
            return Ok(());
        };
        match self.call::<Me>(url, "GET", "/auth/me", Some(&token), None) {
            Ok(me) => match Account::from_me(me) {
                Some(account) => self.account = Some(account),
                None => self.session_over(&origin),
            },
            Err(Problem::Refused { status: 401, .. }) => self.session_over(&origin),
            Err(other) => return Err(other),
        }
        Ok(())
    }

    /// The server no longer knows this client's session: forget it, and go on as a guest.
    fn session_over(&mut self, origin: &str) {
        bevy::log::info!("the server ended this client's session; going on as a guest");
        if let Err(e) = self.store.set_session_token(origin, None) {
            bevy::log::warn!("could not forget the session: {e}");
        }
        self.account = None;
        self.session_ended = true;
    }

    /// The first step of signing in: the handle and the password. This client's guest key goes
    /// with it, so that the guest's seats become the account's.
    pub fn sign_in(&mut self, handle: &str, password: &str) -> Result<SignIn, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(LoginRequest {
            handle: handle.trim().to_string(),
            password: password.to_string(),
            token: true,
        })
        .expect("a sign-in is JSON");
        let guest = self.store.guest_key(&url.origin()).map(str::to_string);
        let answer: SessionTokenResponse =
            self.call(&url, "POST", "/auth/login", guest.as_deref(), Some(&body))?;
        self.signed_in(&url, answer)
    }

    /// The second step: a code, after the password was right.
    pub fn second_factor(&mut self, pending: &str, code: &Code) -> Result<SignIn, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let (code, recovery_code) = match code {
            Code::App(c) => (c.trim().to_string(), String::new()),
            Code::Recovery(c) => (String::new(), c.trim().to_string()),
        };
        let body = serde_json::to_value(SecondRequest {
            pending: pending.to_string(),
            code,
            recovery_code,
        })
        .expect("a code is JSON");
        let guest = self.store.guest_key(&url.origin()).map(str::to_string);
        let answer: SessionTokenResponse =
            self.call(&url, "POST", "/auth/second", guest.as_deref(), Some(&body))?;
        self.signed_in(&url, answer)
    }

    fn signed_in(
        &mut self,
        url: &ServerUrl,
        answer: SessionTokenResponse,
    ) -> Result<SignIn, Problem> {
        if answer.second_factor {
            return Ok(SignIn::NeedsCode {
                pending: answer.pending,
            });
        }
        let not_bingle = || {
            Problem::NotBingle(format!(
                "{url} answered the sign-in, but not with a session as Bingle does."
            ))
        };
        if answer.token.is_empty() {
            return Err(not_bingle());
        }
        let origin = url.origin();
        let account = match answer.me {
            Some(me) => Account::from_me(me),
            None => self
                .call::<Me>(url, "GET", "/auth/me", Some(&answer.token), None)
                .ok()
                .and_then(Account::from_me),
        }
        .ok_or_else(not_bingle)?;
        if let Err(e) = self.store.set_session_token(&origin, Some(answer.token)) {
            bevy::log::warn!("could not save the session: {e}");
        }
        // The guest became the account's, and its key no longer works.
        self.forget_guest_key(&origin);
        self.account = Some(account.clone());
        self.session_ended = false;
        Ok(SignIn::Done(account))
    }

    /// Ends the session, on the server and here. It is forgotten here even if the server could
    /// not be told, so that the player is signed out either way.
    pub fn sign_out(&mut self) -> Result<(), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let origin = url.origin();
        let Some(token) = self.store.session_token(&origin).map(str::to_string) else {
            self.account = None;
            return Ok(());
        };
        let told = self.call::<LogoutResponse>(&url, "POST", "/auth/logout", Some(&token), None);
        if let Err(e) = self.store.set_session_token(&origin, None) {
            bevy::log::warn!("could not forget the session: {e}");
        }
        self.account = None;
        match told {
            // Already over on the server.
            Ok(_) | Err(Problem::Refused { status: 401, .. }) => Ok(()),
            Err(other) => Err(other),
        }
    }

    /// The name to sit down under: the account's, signed in, or the configured one.
    fn player_name(&self) -> String {
        match &self.account {
            Some(account) => account.name().to_string(),
            None => self.store.player_name().to_string(),
        }
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
        let bots = self.bots(&url)?;
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
        let made: BotMatchResponse = self.as_me(&url, "POST", "/api/bot-matches", Some(&body))?;
        Ok(made.room_code)
    }

    /// Makes a game in which this client's guest plays the first seat and the server's bots
    /// every other, and returns its room.
    pub fn start_game_against_bots(&mut self, game_def_id: &str) -> Result<String, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let bots = self.bots(&url)?;
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

    /// A new game as the player set it up: their seat, a bot or nobody in each other one, and
    /// how it is timed.
    pub fn start_game(&mut self, settings: &GameSettings) -> Result<String, Problem> {
        let request = new_game_request(settings, &self.player_name());
        self.send_new_game(request)
    }

    fn create_game(
        &mut self,
        game_def_id: &str,
        opponent: Option<String>,
    ) -> Result<String, Problem> {
        let request = CreateGameRequest::new(game_def_id, &self.player_name(), opponent);
        self.send_new_game(request)
    }

    fn send_new_game(&mut self, request: CreateGameRequest) -> Result<String, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(request).expect("a game request is JSON");
        let made: CreateGameResponse = self.as_me(&url, "POST", "/api/games", Some(&body))?;
        Ok(made.room_code)
    }

    /// What setting up a game of this needs: its seats, and the bots that can fill them.
    pub fn game_setup(&mut self, game_def_id: &str) -> Result<(GameInfo, Vec<BotInfo>), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let info = self.game_info(&url, game_def_id)?;
        let bots = self.bots(&url)?;
        Ok((info, bots))
    }

    fn game_info(&mut self, url: &ServerUrl, game_def_id: &str) -> Result<GameInfo, Problem> {
        if let Some(info) = self.infos.get(game_def_id) {
            return Ok(info.clone());
        }
        let info: GameInfo =
            self.get_as_me(url, &format!("/api/game-defs/{}", query_value(game_def_id)))?;
        self.infos.insert(game_def_id.to_string(), info.clone());
        Ok(info)
    }

    /// The unfinished games that match `query`, newest change first.
    pub fn games(&mut self, query: &GameQuery) -> Result<Vec<OpenGame>, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let list: OpenGames = self.get_as_me(&url, &games_path(query))?;
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
        let game: Game = self.get_as_me(&url, &format!("/api/games/{room_code}"))?;
        if game.status != "waiting" || game.seats.iter().any(|s| s.yours) || !game.has_open_seat() {
            return Ok(());
        }
        // A seat goes by its player's name, so the name must not be one already at the table.
        let body = serde_json::to_value(JoinRequest {
            player_name: free_name(&self.player_name(), &game.seats),
            role: "player".into(),
        })
        .expect("a join is JSON");
        let _: JoinResponse = self.as_me(
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
        let _: GameState = self.as_me(
            &url,
            "POST",
            &format!("/api/games/{room_code}/resign"),
            Some(&body),
        )?;
        Ok(())
    }

    /// Before the game starts, does `call` for the seat `player_name` sits at.
    pub fn lobby(
        &mut self,
        room_code: &str,
        player_name: &str,
        call: &LobbyCall,
    ) -> Result<(), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let player_name = player_name.to_string();
        let (path, body) = lobby_request(call, player_name);
        let path = format!("/api/games/{room_code}/{path}");
        if matches!(call, LobbyCall::Bot { .. }) {
            let _: AddBotResult = self.as_me(&url, "POST", &path, Some(&body))?;
        } else {
            let _: GameStatus = self.as_me(&url, "POST", &path, Some(&body))?;
        }
        Ok(())
    }

    /// The server's bots, read once.
    fn bots(&mut self, url: &ServerUrl) -> Result<Vec<BotInfo>, Problem> {
        if let Some(bots) = &self.bots {
            return Ok(bots.clone());
        }
        let bots: Vec<BotInfo> = self.get_as_me(url, "/api/bots")?;
        self.bots = Some(bots.clone());
        Ok(bots)
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
        self.get_as_me(&url, &path)
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
        let made: MoveResponse = self.as_me(
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
        let game: Game = self.get_as_me(&url, &format!("/api/games/{room_code}"))?;
        let info = self.game_info(&url, &game.game_def_id)?;
        let key = self.bearer(&url)?;
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
        // The bots a seat can be given before the game starts, by a player at the table.
        let bots = if game.status == "waiting" && game.seats.iter().any(|s| s.yours) {
            self.bots(&url)?
        } else {
            Vec::new()
        };
        let opened = Opened {
            room_code: room_code.to_string(),
            info,
            game,
            state,
            bots,
        };
        Ok((opened, socket))
    }

    /// Where a game stands now.
    pub fn state(&mut self, room_code: &str) -> Result<GameState, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        self.get_as_me(&url, &format!("/api/games/{room_code}/state"))
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

    /// A request as this client: with the session's token when an account is signed in, and
    /// otherwise with the guest key, made first if there is none. A key the server no longer knows
    /// (unused for months, or a new database) is replaced once; a session it no longer knows is
    /// forgotten, and the request made again as a guest.
    fn as_me<T: DeserializeOwned>(
        &mut self,
        url: &ServerUrl,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<T, Problem> {
        let origin = url.origin();
        if let Some(token) = self.store.session_token(&origin).map(str::to_string) {
            match self.call(url, method, path, Some(&token), body) {
                Err(Problem::Refused { status: 401, .. }) => self.session_over(&origin),
                other => return other.map_err(unauthorized),
            }
        }
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

    fn get_as_me<T: DeserializeOwned>(
        &mut self,
        url: &ServerUrl,
        path: &str,
    ) -> Result<T, Problem> {
        self.as_me(url, "GET", path, None)
    }

    /// What this client authenticates with: the session's token, or the guest key.
    fn bearer(&mut self, url: &ServerUrl) -> Result<String, Problem> {
        match self.store.session_token(&url.origin()) {
            Some(token) => Ok(token.to_string()),
            None => self.guest_key(url),
        }
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
    /// The server's bots, in a game not yet started that this client has a seat in.
    pub bots: Vec<BotInfo>,
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

/// The lobby call's path under the game, and its body.
pub fn lobby_request(call: &LobbyCall, player_name: String) -> (&'static str, serde_json::Value) {
    fn json(v: impl serde::Serialize) -> serde_json::Value {
        serde_json::to_value(v).expect("a lobby call is JSON")
    }
    match call {
        LobbyCall::Start => ("start", json(PlayerRequest { player_name })),
        LobbyCall::Leave => ("leave", json(PlayerRequest { player_name })),
        LobbyCall::Sit(seat) => (
            "seat",
            json(SeatRequest {
                player_name,
                seat: *seat,
            }),
        ),
        LobbyCall::Remove(seat) => (
            "remove",
            json(SeatRequest {
                player_name,
                seat: *seat,
            }),
        ),
        LobbyCall::Bot { seat, bot } => (
            "bots",
            json(AddBotRequest {
                player_name,
                bot: bot.clone(),
                seat: Some(*seat),
            }),
        ),
    }
}

/// `POST /api/games`' body for a game set up as `settings`.
pub fn new_game_request(settings: &GameSettings, player_name: &str) -> CreateGameRequest {
    let (clock_seconds, clock_increment_seconds, move_time_seconds) = match settings.time {
        TimeControl::None => (None, None, None),
        TimeControl::Clock { seconds, increment } => {
            (Some(seconds), (increment > 0).then_some(increment), None)
        }
        TimeControl::PerMove { seconds } => (None, None, Some(seconds)),
    };
    CreateGameRequest {
        seat: (settings.seat != 1).then_some(settings.seat),
        seat_bots: settings
            .bots
            .iter()
            .map(|(seat, bot)| (seat.to_string(), bot.clone()))
            .collect(),
        clock_seconds,
        clock_increment_seconds,
        move_time_seconds,
        lobby: settings.lobby,
        ..CreateGameRequest::new(&settings.game_def_id, player_name, None)
    }
}

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

/// Room codes are letters and digits, which keeps them safe to put in a path.
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
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    #[derive(Debug, Clone)]
    struct Seen {
        method: String,
        path: String,
        auth: Option<String>,
        body: String,
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
                let req = Seen {
                    method,
                    path,
                    auth,
                    body: String::from_utf8_lossy(&body).into_owned(),
                };
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

    const INDEX: &str = r#"{"version":"0.115.0","api_version":"1.5.0","docs":"/api/docs"}"#;
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
        assert_eq!(snap.api_version, "1.5.0");
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
    fn the_lobby_calls_go_to_their_paths_for_the_seat_named() {
        let (addr, seen) = serve(Box::new(|req: &Seen| {
            let status = r#"{"status":"waiting","lobby":true,"host_seat":1,"move_seq":0,"impulse_seat":null}"#;
            match (req.method.as_str(), req.path.as_str()) {
                ("POST", "/api/guest") => (201, r#"{"key":"bgg_new"}"#.into()),
                ("POST", "/api/games/AB12CD/bots") => (
                    200,
                    r#"{"seat":3,"name":"Green (MCTS)","color":"green","bot":"mcts","status":"waiting"}"#
                        .into(),
                ),
                ("POST", "/api/games/AB12CD/remove") => (
                    403,
                    r#"{"error":"session: only the host of this game can do that"}"#.into(),
                ),
                ("POST", _) => (200, status.into()),
                _ => (404, r#"{"error":"api: no such endpoint"}"#.into()),
            }
        }));
        let mut s = session(&addr);
        for call in [
            LobbyCall::Sit(2),
            LobbyCall::Bot {
                seat: 3,
                bot: "mcts".into(),
            },
            LobbyCall::Start,
            LobbyCall::Leave,
        ] {
            s.lobby("AB12CD", "Ana", &call)
                .unwrap_or_else(|e| panic!("{call:?}: {e:?}"));
        }
        assert_eq!(
            s.lobby("AB12CD", "Ana", &LobbyCall::Remove(2)),
            Err(Problem::Refused {
                status: 403,
                message: "session: only the host of this game can do that".into()
            })
        );
        let seen = seen.lock().unwrap();
        let sent: Vec<(&str, serde_json::Value)> = seen
            .iter()
            .filter(|r| r.path != "/api/guest")
            .map(|r| (r.path.as_str(), serde_json::from_str(&r.body).unwrap()))
            .collect();
        use serde_json::json;
        assert_eq!(
            sent,
            [
                (
                    "/api/games/AB12CD/seat",
                    json!({"player_name": "Ana", "seat": 2})
                ),
                (
                    "/api/games/AB12CD/bots",
                    json!({"player_name": "Ana", "bot": "mcts", "seat": 3})
                ),
                ("/api/games/AB12CD/start", json!({"player_name": "Ana"})),
                ("/api/games/AB12CD/leave", json!({"player_name": "Ana"})),
                (
                    "/api/games/AB12CD/remove",
                    json!({"player_name": "Ana", "seat": 2})
                ),
            ]
        );
    }

    /// A Bingle with accounts: "ana" signs in with her password ("pw") and, when `second` is set,
    /// a code ("123456"); her session is `bgs_ana` until it is ended.
    fn accounts(second: bool, ended: Arc<Mutex<bool>>) -> Box<Handler> {
        Box::new(move |req: &Seen| {
            let auth = req.auth.as_deref().unwrap_or_default();
            let body: serde_json::Value = serde_json::from_str(&req.body).unwrap_or_default();
            let session_ok = auth == "Bearer bgs_ana" && !*ended.lock().unwrap();
            let me = r#"{"kind":"account","handle":"ana","display_name":"Ana","accounts":true,"signup":false}"#;
            let signed_in = format!(r#"{{"token":"bgs_ana","expires":"x","me":{me}}}"#);
            match (req.method.as_str(), req.path.as_str()) {
                ("GET", "/api") => (200, INDEX.into()),
                ("POST", "/api/guest") => (201, r#"{"key":"bgg_new"}"#.into()),
                ("POST", "/auth/login") if body["password"] == "pw" => {
                    if second {
                        (200, r#"{"second_factor":true,"pending":"p1"}"#.into())
                    } else {
                        (200, signed_in)
                    }
                }
                ("POST", "/auth/login") => (
                    401,
                    r#"{"error":"auth: the handle or the password is wrong"}"#.into(),
                ),
                ("POST", "/auth/second") if body["code"] == "123456" => (200, signed_in),
                ("POST", "/auth/second") => (401, r#"{"error":"auth: that code is wrong"}"#.into()),
                ("GET", "/auth/me") if session_ok => (200, me.into()),
                ("POST", "/auth/logout") if session_ok => {
                    *ended.lock().unwrap() = true;
                    (200, r#"{"kind":"anonymous"}"#.into())
                }
                (_, "/api/game-defs") if session_ok || auth == "Bearer bgg_new" => {
                    (200, DEFS.into())
                }
                (_, "/api/games") if session_ok || auth == "Bearer bgg_new" => (200, GAMES.into()),
                _ => (401, r#"{"error":"auth: who is this?"}"#.into()),
            }
        })
    }

    #[test]
    fn signing_in_brings_the_guest_along_and_the_token_is_used_from_then_on() {
        let ended = Arc::new(Mutex::new(false));
        let (addr, seen) = serve(accounts(false, ended.clone()));
        let mut s = session(&addr);
        let origin = ServerUrl::parse(&addr).unwrap().origin();
        assert_eq!(s.connect().unwrap().account, None);
        assert!(matches!(
            s.sign_in("ana", "wrong"),
            Err(Problem::Refused { status: 401, .. })
        ));
        let signed = s.sign_in(" ana ", "pw").unwrap();
        let SignIn::Done(account) = signed else {
            panic!("{signed:?}")
        };
        assert_eq!(account.name(), "Ana");
        assert_eq!(s.store().session_token(&origin), Some("bgs_ana"));
        assert_eq!(
            s.store().guest_key(&origin),
            None,
            "merged into the account"
        );
        assert_eq!(s.player_name(), "Ana", "sits down under the account's name");

        let snap = s.connect().unwrap();
        assert_eq!(snap.account.map(|a| a.handle), Some("ana".into()));
        assert!(!snap.session_ended);
        {
            let seen = seen.lock().unwrap();
            let login = seen.iter().rfind(|r| r.path == "/auth/login").unwrap();
            assert_eq!(
                login.auth.as_deref(),
                Some("Bearer bgg_new"),
                "the guest goes along"
            );
            let sent: serde_json::Value = serde_json::from_str(&login.body).unwrap();
            assert_eq!(
                sent,
                serde_json::json!({"handle": "ana", "password": "pw", "token": true}),
                "the handle trimmed, and a token asked for"
            );
            let last_games = seen.iter().rfind(|r| r.path == "/api/games").unwrap();
            assert_eq!(last_games.auth.as_deref(), Some("Bearer bgs_ana"));
        }

        // Signed out elsewhere: the next connection finds the session over, and goes on as a guest.
        *ended.lock().unwrap() = true;
        let snap = s.connect().unwrap();
        assert_eq!(snap.account, None);
        assert!(snap.session_ended, "the player is told");
        assert_eq!(s.store().session_token(&origin), None);
        assert!(!s.connect().unwrap().session_ended, "told once");
    }

    #[test]
    fn a_second_factor_takes_a_code_and_signing_out_ends_the_session() {
        let ended = Arc::new(Mutex::new(false));
        let (addr, seen) = serve(accounts(true, ended.clone()));
        let mut s = session(&addr);
        let origin = ServerUrl::parse(&addr).unwrap().origin();
        assert_eq!(
            s.sign_in("ana", "pw").unwrap(),
            SignIn::NeedsCode {
                pending: "p1".into()
            }
        );
        assert_eq!(s.store().session_token(&origin), None, "not yet");
        assert!(s.second_factor("p1", &Code::App("000000".into())).is_err());
        assert!(matches!(
            s.second_factor("p1", &Code::App(" 123456 ".into())),
            Ok(SignIn::Done(_))
        ));
        assert_eq!(s.store().session_token(&origin), Some("bgs_ana"));

        s.sign_out().unwrap();
        assert!(*ended.lock().unwrap(), "the server was told");
        assert_eq!(s.store().session_token(&origin), None);
        s.sign_out().expect("signing out twice is no problem");

        let seen = seen.lock().unwrap();
        let codes: Vec<serde_json::Value> = seen
            .iter()
            .filter(|r| r.path == "/auth/second")
            .map(|r| serde_json::from_str(&r.body).unwrap())
            .collect();
        assert_eq!(
            codes,
            [
                serde_json::json!({"code": "000000", "pending": "p1"}),
                serde_json::json!({"code": "123456", "pending": "p1"}),
            ]
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
    fn a_game_set_up_by_the_player_asks_for_its_seats_bots_and_time() {
        let settings = GameSettings {
            game_def_id: "hex-7".into(),
            seat: 2,
            bots: [(1, "mcts".to_string())].into(),
            time: TimeControl::Clock {
                seconds: 600,
                increment: 5,
            },
            lobby: true,
        };
        let sent = serde_json::to_value(new_game_request(&settings, "Ana")).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({
                "game_def_id": "hex-7",
                "player_name": "Ana",
                "seat": 2,
                "seat_bots": {"1": "mcts"},
                "clock_seconds": 600,
                "clock_increment_seconds": 5,
                "lobby": true,
            })
        );
        let plain = GameSettings {
            seat: 1,
            bots: BTreeMap::new(),
            time: TimeControl::PerMove { seconds: 60 },
            lobby: false,
            ..settings
        };
        assert_eq!(
            serde_json::to_value(new_game_request(&plain, "Ana")).unwrap(),
            serde_json::json!({"game_def_id": "hex-7", "player_name": "Ana", "move_time_seconds": 60})
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
