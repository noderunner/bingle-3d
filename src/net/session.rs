//! Talking to one server, blocking: it runs on the network thread, never in a Bevy system.
//!
//! [`Session::connect`] does what start-up needs: read `GET /api` and check the API version, make
//! sure there is a guest key (or that the account signed in still is), then list the games and the
//! open games. Whatever goes wrong comes back as a [`Problem`] the player can understand.
//!
//! Signed in to an account, every call and the game's socket carry the session's token in place
//! of the guest key. When the server no longer knows the token (it expired, or the account signed
//! out everywhere or changed its password), the client forgets it and goes on as a guest.

use std::collections::{HashMap, HashSet};
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
use super::api::{CLIENT, ClientFile, ClientIndex};
use super::api::{
    CodeLoginRequest, LoginRequest, LogoutResponse, Me, SecondRequest, SessionTokenResponse,
};
use super::api::{DirectCallRequest, DirectSignalRequest, IceServer, RtcConfig};
use super::api::{DirectHistory, DirectHistoryRequest, DirectMessage, DirectMessageRequest};
use super::api::{FriendBody, FriendSettings, FriendsList, HereRequest, HereResponse, OkResponse};
use super::api::{VoicePeer, VoicePeers, VoiceRequest, VoiceSignalRequest};
use super::server_url::{Scheme, ServerUrl, query_value};
use super::socket::{self, Role, Socket};
use super::tls::{Fingerprint, TlsConnector, Trust};
use super::{FriendCall, GameQuery, GameSettings, LobbyCall, TimeControl};
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

/// What the login screen learns of a server before logging in.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub server: String,
    pub version: String,
    pub api_version: String,
    /// The account this client's kept session belongs to there, if it is still good.
    pub account: Option<Account>,
    /// Whether this client keeps a guest key for it.
    pub guest_key: bool,
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
    /// The games built from the server's private overlay (commercial games) that the private
    /// folder has no look for, by id: left out of every list this client shows (`public`).
    hidden: HashSet<String>,
    /// The games the private folder has a look for, by id (`Appearances::private_games`).
    private_looks: HashSet<String>,
}

impl Session {
    /// `server` overrides the configured server for this run.
    pub fn new(server: Option<&str>, store: Store) -> Self {
        let address = server.unwrap_or(store.server()).to_string();
        let (url, trust, tls, agent) = Self::reach(&address, &store);
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
            hidden: HashSet::new(),
            private_looks: HashSet::new(),
        }
    }

    /// The games the private folder has a look for: shown, though the server marks them private.
    pub fn with_private_looks(mut self, games: HashSet<String>) -> Self {
        self.private_looks = games;
        self
    }

    /// The address read, and what reaches it: the certificates trusted there, and an agent.
    fn reach(
        address: &str,
        store: &Store,
    ) -> (
        Result<ServerUrl, String>,
        Arc<Trust>,
        Arc<ClientConfig>,
        Agent,
    ) {
        let url = ServerUrl::parse(address)
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
        (url, trust, tls, agent)
    }

    /// From now on, talk to the server at this address (the login screen's). What was read from
    /// another server is forgotten.
    pub fn use_server(&mut self, address: &str) {
        let (url, trust, tls, agent) = Self::reach(address.trim(), &self.store);
        if url.is_ok() && url == self.url {
            return;
        }
        self.url = url;
        self.trust = trust;
        self.tls = tls;
        self.agent = agent;
        self.infos.clear();
        self.bots = None;
        self.account = None;
        self.session_ended = false;
    }

    /// Whether the server is there and this client can use its API, and who this client would be
    /// there: the account its kept session belongs to, or a guest (with a key kept, or not).
    pub fn probe(&mut self) -> Result<Probe, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let (index, api_version) = self.index(&url)?;
        self.check_account(&url)?;
        Ok(Probe {
            server: url.to_string(),
            version: index.version,
            api_version,
            account: self.account.clone(),
            guest_key: self.store.guest_key(&url.origin()).is_some(),
        })
    }

    /// Takes a guest key the player typed in, once the server says it knows it: from then on
    /// this client is that guest here. One it does not know is refused, and the key kept before
    /// stays.
    pub fn adopt_guest_key(&mut self, key: &str) -> Result<(), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let key = key.trim();
        match self.call::<OpenGames>(&url, "GET", "/api/games", Some(key), None) {
            Ok(_) => {}
            Err(Problem::Refused { status: 401, .. }) => {
                return Err(Problem::Refused {
                    status: 401,
                    message: "the server does not know that guest key".into(),
                });
            }
            Err(other) => return Err(other),
        }
        if let Err(e) = self
            .store
            .set_guest_key(&url.origin(), Some(key.to_string()))
        {
            bevy::log::warn!("could not save the guest key: {e}");
        }
        Ok(())
    }

    /// Remembers the server now used (when `server` is true) and whether to log in by itself
    /// at the next start.
    pub fn remember(&mut self, server: bool, auto_login: bool) {
        let address = match (&self.url, server) {
            (Ok(url), true) => Some(url.to_string()),
            _ => None,
        };
        if let Err(e) = self.store.remember(address.as_deref(), auto_login) {
            bevy::log::warn!("could not save the config: {e}");
        }
    }

    /// Remembers where the table stands (an environment's id).
    pub fn remember_environment(&mut self, id: &str) {
        if let Err(e) = self.store.remember_environment(id) {
            bevy::log::warn!("could not save the config: {e}");
        }
    }

    pub fn remember_display(&mut self, fullscreen: bool, resolution: Option<(u32, u32)>) {
        let resolution = resolution.map(crate::display::format);
        if let Err(e) = self.store.remember_display(fullscreen, resolution) {
            bevy::log::warn!("could not save the config: {e}");
        }
    }

    pub fn remember_graphics(&mut self, graphics: &crate::graphics::Graphics, keep_bloom: bool) {
        if let Err(e) = self.store.remember_graphics(graphics, keep_bloom) {
            bevy::log::warn!("could not save the config: {e}");
        }
    }

    pub fn remember_max_fps(&mut self, fps: u32) {
        if let Err(e) = self.store.remember_max_fps(fps) {
            bevy::log::warn!("could not save the config: {e}");
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
        let (index, api_version) = self.index(&url)?;
        self.check_account(&url)?;
        let (defs, hidden) = public(self.get_as_me(&url, "/api/game-defs")?, &self.private_looks);
        self.hidden = hidden;
        let games: OpenGames = self.get_as_me(&url, "/api/games")?;
        Ok(Snapshot {
            server: url.to_string(),
            version: index.version,
            api_version,
            defs,
            games: self.shown(games.games),
            account: self.account.clone(),
            session_ended: std::mem::take(&mut self.session_ended),
        })
    }

    /// The server's index, and its API version once this client is known to be able to use it.
    fn index(&mut self, url: &ServerUrl) -> Result<(Index, String), Problem> {
        let index: Index = match self.get(url, "/api", None) {
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
        Ok((index, api_version))
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

    /// Signs in with the code of a server's setup file (`setup.rs`), which works once. The
    /// guest key goes with it as with any sign-in; a key the server no longer knows is refused
    /// before the code is looked at, so the code is then tried by itself.
    pub fn sign_in_with_code(&mut self, code: &str) -> Result<SignIn, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(CodeLoginRequest {
            code: code.to_string(),
        })
        .expect("a sign-in is JSON");
        let guest = self.store.guest_key(&url.origin()).map(str::to_string);
        let mut answer: Result<SessionTokenResponse, Problem> =
            self.call(&url, "POST", "/auth/login", guest.as_deref(), Some(&body));
        if guest.is_some() && matches!(answer, Err(Problem::Refused { status: 401, .. })) {
            answer = self.call(&url, "POST", "/auth/login", None, Some(&body));
        }
        match self.signed_in(&url, answer?)? {
            SignIn::Done(account) => Ok(SignIn::Done(account)),
            SignIn::NeedsCode { .. } => Err(Problem::NotBingle(format!(
                "{url} asked for a second factor after a setup file's sign-in code, which Bingle \
                 does not."
            ))),
        }
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
        // Gone from friends' lists at once, rather than when the server forgets the account.
        let gone = serde_json::to_value(HereRequest { here: false }).expect("here is JSON");
        let _ = self.call::<HereResponse>(&url, "POST", "/api/here", Some(&token), Some(&gone));
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

    /// An account is signed in, as far as this client knows.
    pub fn has_account(&self) -> bool {
        self.account.is_some()
    }

    /// The account's friends, requests and game invitations.
    pub fn friends(&mut self) -> Result<FriendsList, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        self.get_as_me(&url, "/api/friends")
    }

    /// Does `call` on the friends list.
    pub fn friend(&mut self, call: &FriendCall) -> Result<(), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let (path, body) = friend_request(call);
        let body = serde_json::to_value(body).expect("a friends call is JSON");
        let _: OkResponse =
            self.as_me(&url, "POST", &format!("/api/friends/{path}"), Some(&body))?;
        Ok(())
    }

    /// The friends settings, changed to `new` first if given.
    pub fn friend_settings(
        &mut self,
        new: Option<FriendSettings>,
    ) -> Result<FriendSettings, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        match new {
            Some(new) => {
                let body = serde_json::to_value(new).expect("settings are JSON");
                self.as_me(&url, "PUT", "/api/friends/settings", Some(&body))
            }
            None => self.get_as_me(&url, "/api/friends/settings"),
        }
    }

    /// Says that the account is here (looking at Bingle), or, with `false`, that it has gone.
    pub fn here(&mut self, here: bool) -> Result<(), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(HereRequest { here }).expect("here is JSON");
        let _: HereResponse = self.as_me(&url, "POST", "/api/here", Some(&body))?;
        Ok(())
    }

    /// Writes to a friend, at their address.
    pub fn write(&mut self, to: &str, text: &str) -> Result<DirectMessage, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(DirectMessageRequest {
            to: to.to_string(),
            text: text.trim().to_string(),
        })
        .expect("a message is JSON");
        self.as_me(&url, "POST", "/api/dm", Some(&body))
    }

    /// What was said with an address (the server keeps the last 200 of a pair, for two hours).
    pub fn history(&mut self, with: &str) -> Result<Vec<DirectMessage>, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(DirectHistoryRequest {
            with: with.to_string(),
        })
        .expect("a history request is JSON");
        let history: DirectHistory = self.as_me(&url, "POST", "/api/dm/history", Some(&body))?;
        Ok(history.messages)
    }

    /// Rings a friend at their address (or answers them, when they rang first); with `join`
    /// false, hangs up the call this account is in.
    pub fn direct_call(&mut self, to: &str, join: bool) -> Result<(), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(DirectCallRequest {
            to: if join { to.to_string() } else { String::new() },
            join,
        })
        .expect("a call is JSON");
        let _: OkResponse = self.as_me(&url, "POST", "/api/dm/call", Some(&body))?;
        Ok(())
    }

    /// Relays one WebRTC message to the person this account is calling.
    pub fn direct_signal(
        &mut self,
        to: &str,
        kind: &str,
        data: serde_json::Value,
    ) -> Result<(), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(DirectSignalRequest {
            to: to.to_string(),
            kind: kind.to_string(),
            data,
        })
        .expect("a signal is JSON");
        let _: OkResponse = self.as_me(&url, "POST", "/api/dm/signal", Some(&body))?;
        Ok(())
    }

    /// The ICE servers for a call.
    pub fn ice_servers(&mut self) -> Result<Vec<IceServer>, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let config: RtcConfig = self.get_as_me(&url, "/api/rtc/config")?;
        Ok(config.ice_servers)
    }

    /// Opens a game's voice socket for the player `name` (a seat this client holds).
    pub fn voice_socket(&mut self, room_code: &str, name: &str) -> Result<Socket, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        if !valid_room_code(room_code) {
            return Err(not_a_room_code(room_code));
        }
        let key = self.bearer(&url)?;
        Socket::open_voice(&url, room_code, name, &key, self.tls.clone())
            .map_err(|e| self.socket_problem(&url, e))
    }

    /// Joins a game's voice chat for the seat of `name`, or says it is still there; with
    /// `active` false, leaves it. Who is in the call.
    pub fn voice(
        &mut self,
        room_code: &str,
        name: &str,
        active: bool,
    ) -> Result<Vec<VoicePeer>, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(VoiceRequest {
            player_name: name.to_string(),
            active,
        })
        .expect("a voice request is JSON");
        let peers: VoicePeers = self.as_me(
            &url,
            "POST",
            &format!("/api/games/{room_code}/voice"),
            Some(&body),
        )?;
        Ok(peers.peers)
    }

    /// Who is in a game's voice chat.
    pub fn voice_peers(&mut self, room_code: &str) -> Result<Vec<VoicePeer>, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let peers: VoicePeers = self.get_as_me(&url, &format!("/api/games/{room_code}/voice"))?;
        Ok(peers.peers)
    }

    /// Relays one WebRTC message from the seat of `name` to the seat `to`.
    pub fn voice_signal(
        &mut self,
        room_code: &str,
        name: &str,
        to: u32,
        kind: &str,
        data: serde_json::Value,
    ) -> Result<(), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let body = serde_json::to_value(VoiceSignalRequest {
            player_name: name.to_string(),
            to,
            kind: kind.to_string(),
            data,
        })
        .expect("a signal is JSON");
        let _: OkResponse = self.as_me(
            &url,
            "POST",
            &format!("/api/games/{room_code}/voice/signal"),
            Some(&body),
        )?;
        Ok(())
    }

    /// Opens the signed-in account's own socket, `/ws-dm`. `None` when nobody is signed in.
    pub fn account_socket(&mut self) -> Result<Option<Socket>, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let Some(token) = self.store.session_token(&url.origin()).map(str::to_string) else {
            return Ok(None);
        };
        Socket::open_account(&url, &token, self.tls.clone())
            .map(Some)
            .map_err(|e| self.socket_problem(&url, e))
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

    /// The origin of the server in use (`https://host:port`): what a downloaded look's index is
    /// kept under, since another server's game of the same id is another game.
    pub fn origin(&self) -> Option<String> {
        self.url.as_ref().ok().map(ServerUrl::origin)
    }

    /// `GET /api/game-defs/{id}/clients/bingle-3d`: the files of the look the server offers for
    /// this game (API 1.17). Anyone who can reach the server may read them, so nobody is named.
    pub fn client_index(&self, game_def_id: &str) -> Result<ClientIndex, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        self.get(&url, &client_path(game_def_id, None), None)
    }

    /// One file of that look, from byte `from` to `to` (exclusive), as it comes. It is asked for
    /// only if it is still the one whose hash the index gave (`If-Range`): a server whose file
    /// has changed sends the whole of the new one, from its start, which a caller tells by
    /// [`Received::Whole`].
    pub fn client_file(
        &self,
        game_def_id: &str,
        file: &ClientFile,
        from: u64,
        to: u64,
    ) -> Result<(Received, ureq::BodyReader<'static>), Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let target = url.join(&client_path(game_def_id, Some(&file.path)));
        let response = self
            .agent
            .get(&target)
            .header("Range", format!("bytes={from}-{}", to.max(from + 1) - 1))
            .header("If-Range", format!("\"{}\"", file.sha256))
            .call()
            .map_err(|e| self.transport_problem(&url, e))?;
        let status = response.status().as_u16();
        let starts_at = |range: &str| {
            range
                .strip_prefix("bytes ")
                .and_then(|r| r.split_once('-'))
                .and_then(|(start, _)| start.parse::<u64>().ok())
        };
        let received = match status {
            200 => Received::Whole,
            206 => {
                let range = response
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(starts_at);
                if range != Some(from) {
                    return Err(Problem::NotBingle(format!(
                        "{url} sent another part of {} than the one asked for.",
                        file.path
                    )));
                }
                Received::Part
            }
            _ => {
                let mut response = response;
                let body = response.body_mut().read_to_string().unwrap_or_default();
                let message = serde_json::from_str::<ErrorBody>(&body)
                    .map(|b| b.error)
                    .unwrap_or_else(|_| format!("{} was answered with status {status}", file.path));
                return Err(Problem::Refused { status, message });
            }
        };
        Ok((received, response.into_body().into_reader()))
    }

    /// The unfinished games that match `query`, newest change first.
    pub fn games(&mut self, query: &GameQuery) -> Result<Vec<OpenGame>, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let list: OpenGames = self.get_as_me(&url, &games_path(query))?;
        Ok(self.shown(list.games))
    }

    /// The games of these that this build shows: none of a private game.
    fn shown(&self, games: Vec<OpenGame>) -> Vec<OpenGame> {
        games
            .into_iter()
            .filter(|g| !self.hidden.contains(&g.game_def_id))
            .collect()
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
        // A seat kept for somebody else cannot be taken: such a game is only watched.
        if game.status != "waiting" || game.seats.iter().any(|s| s.yours) || !game.has_seat_for_me()
        {
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
        let held = held(&info, &game.seats);
        let state = self.state_seen(room_code, &held)?;
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

    /// Where a game stands now: as everybody sees it, or, with `viewer`, as the player of that
    /// name does (the seat's own hand, in a game that keeps secrets; anybody who does not hold
    /// that seat gets the public view).
    pub fn state(&mut self, room_code: &str, viewer: Option<&str>) -> Result<GameState, Problem> {
        let url = self.url.clone().map_err(Problem::BadAddress)?;
        let mut path = format!("/api/games/{room_code}/state");
        if let Some(name) = viewer {
            path.push_str(&format!("?player_name={}", query_value(name)));
        }
        self.get_as_me(&url, &path)
    }

    /// Where a game stands now, as this client sees it from the seats it holds (`held`): as
    /// the one on impulse, when it holds several, else the first; as everybody, with none.
    pub fn state_seen(
        &mut self,
        room_code: &str,
        held: &[(u32, String)],
    ) -> Result<GameState, Problem> {
        let first = held.first().map(|(_, name)| name.as_str());
        let state = self.state(room_code, first)?;
        match viewer(held, state.impulse_seat) {
            Some(name) if Some(name) != first => self.state(room_code, Some(name)),
            _ => Ok(state),
        }
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
            "PUT" => {
                let mut req = self.agent.put(&target);
                if let Some(a) = &auth {
                    req = req.header("Authorization", a);
                }
                match body {
                    Some(b) => req.send_json(b),
                    None => req.send_empty(),
                }
            }
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

/// The seats this client holds in a game that may keep secrets from a seat, each with its
/// player's name: what it reads the position as. None in any other game, whose position is
/// the same for everybody.
pub fn held(info: &GameInfo, seats: &[GameSeat]) -> Vec<(u32, String)> {
    if !info.may_keep_secrets() {
        return Vec::new();
    }
    seats
        .iter()
        .filter(|s| s.yours && s.bot.is_empty() && !s.name.is_empty())
        .map(|s| (s.seat, s.name.clone()))
        .collect()
}

/// The seats of `held` as they sit now (`seats`, as an event lists them): each player's by
/// its name, which only one person at the table goes by.
pub fn held_now(held: &[(u32, String)], seats: &[GameSeat]) -> Vec<(u32, String)> {
    seats
        .iter()
        .filter(|s| s.bot.is_empty())
        .filter_map(|s| {
            let (_, name) = held.iter().find(|(_, n)| n.eq_ignore_ascii_case(&s.name))?;
            Some((s.seat, name.clone()))
        })
        .collect()
}

/// The player a position is read as: of the seats held, the one on impulse, else the first.
pub fn viewer(held: &[(u32, String)], impulse: Option<u32>) -> Option<&str> {
    held.iter()
        .find(|(seat, _)| impulse == Some(*seat))
        .or(held.first())
        .map(|(_, name)| name.as_str())
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

/// The friends call's path under `/api/friends`, and its body.
pub fn friend_request(call: &FriendCall) -> (&'static str, FriendBody) {
    let who = |handle: &str| handle.trim().trim_start_matches('@').to_string();
    match call {
        FriendCall::Ask(handle) => (
            "request",
            FriendBody {
                handle: who(handle),
                ..Default::default()
            },
        ),
        FriendCall::Answer { handle, accept } => (
            "answer",
            FriendBody {
                handle: who(handle),
                accept: Some(*accept),
                ..Default::default()
            },
        ),
        FriendCall::Remove(handle) => (
            "remove",
            FriendBody {
                handle: who(handle),
                ..Default::default()
            },
        ),
        FriendCall::Invite { handle, room } => (
            "invite",
            FriendBody {
                handle: who(handle),
                room: room.clone(),
                accept: None,
            },
        ),
        FriendCall::AnswerInvite { room, accept } => (
            "invite-answer",
            FriendBody {
                room: room.clone(),
                accept: Some(*accept),
                ..Default::default()
            },
        ),
        FriendCall::Dismiss(room) => (
            "dismiss",
            FriendBody {
                room: room.clone(),
                ..Default::default()
            },
        ),
    }
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
        reserved_seats: settings
            .reserved
            .iter()
            .map(|(seat, handle)| (seat.to_string(), handle.clone()))
            .collect(),
        ..CreateGameRequest::new(&settings.game_def_id, player_name, None)
    }
}

/// What [`Session::client_file`] is sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Received {
    /// The bytes asked for.
    Part,
    /// The whole file, from its start.
    Whole,
}

/// The path of a game's files for this client: the index, or one file of it.
pub fn client_path(game_def_id: &str, file: Option<&str>) -> String {
    let index = format!(
        "/api/game-defs/{}/clients/{CLIENT}",
        query_value(game_def_id)
    );
    match file {
        // Each part of a path is checked before it is asked for (`downloads::check::path`),
        // and keeps to what a URL may hold as it is.
        Some(f) => format!(
            "{index}/files/{}",
            f.split('/').map(query_value).collect::<Vec<_>>().join("/")
        ),
        None => index,
    }
}

/// `GET /api/games` for `query`, with as many games as the server lists at once.
/// The games this client shows, and the ids of those it leaves out: the server's private
/// overlay's (commercial games, whose looks this repository may not hold: Bingle's ADR 0127),
/// but those the private folder has a look for (`looks`; plan, decision 2) and those the server
/// offers a look for (`GameDef::offers_look`; *Downloaded looks*). A public server marks none.
pub fn public(defs: Vec<GameDef>, looks: &HashSet<String>) -> (Vec<GameDef>, HashSet<String>) {
    let (hidden, shown): (Vec<GameDef>, Vec<GameDef>) = defs
        .into_iter()
        .partition(|d| d.private && !looks.contains(&d.id) && !d.offers_look());
    (shown, hidden.into_iter().map(|d| d.id).collect())
}

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
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    #[derive(Debug, Clone)]
    pub(crate) struct Seen {
        pub(crate) method: String,
        pub(crate) path: String,
        pub(crate) auth: Option<String>,
        pub(crate) body: String,
    }

    pub(crate) type Handler = dyn Fn(&Seen) -> (u16, String) + Send + Sync;

    /// A plain HTTP server on loopback that answers with `handler` and records what it was asked.
    pub(crate) fn serve(handler: Box<Handler>) -> (String, Arc<Mutex<Vec<Seen>>>) {
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

    #[test]
    fn a_private_game_and_its_tables_are_left_out_of_the_lists() {
        let def = |id: &str, private: bool| GameDef {
            id: id.into(),
            name: id.into(),
            seats_required: 2,
            family: None,
            description: String::new(),
            prototype: false,
            version: "1.0.0".into(),
            tutorials: 0,
            private,
            clients: Vec::new(),
        };
        let none = HashSet::new();
        let defs = || vec![def("hex", false), def("p", true), def("go", false)];
        let (shown, hidden) = public(defs(), &none);
        let ids: Vec<&str> = shown.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["hex", "go"]);
        assert_eq!(hidden, HashSet::from(["p".to_string()]));
        // A public server marks none.
        assert!(public(vec![def("hex", false)], &none).1.is_empty());
        // The private folder has a look for it: shown.
        let looks = HashSet::from(["p".to_string()]);
        let (shown, hidden) = public(defs(), &looks);
        assert_eq!(shown.len(), 3);
        assert!(hidden.is_empty());
        // The server offers a look for it: shown. One for another client is not enough.
        let offered = |clients: &[&str]| {
            let mut defs = defs();
            defs[1].clients = clients.iter().map(|c| c.to_string()).collect();
            public(defs, &none).1
        };
        assert!(offered(&["made-up", "bingle-3d"]).is_empty());
        assert_eq!(offered(&["made-up"]), HashSet::from(["p".to_string()]));
        // Read from the server: absent is public.
        let read: Vec<GameDef> = serde_json::from_str(
            r#"[{"id":"a","name":"A","seats_required":2},
                {"id":"b","name":"B","seats_required":2,"private":true}]"#,
        )
        .unwrap();
        assert_eq!(public(read, &none).1, HashSet::from(["b".to_string()]));
    }

    const INDEX: &str = r#"{"version":"0.115.0","api_version":"1.20.0","docs":"/api/docs"}"#;
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

    pub(crate) fn session(addr: &str) -> Session {
        Session::new(Some(addr), Store::load(None))
    }

    #[test]
    fn a_probe_switches_server_and_says_what_it_is_without_making_a_guest() {
        let (good, seen) = serve(bingle(INDEX, &["bgg_new"]));
        let mut s = session("http://127.0.0.1:1");
        assert!(matches!(s.probe(), Err(Problem::Unreachable(_))));
        s.use_server(&good);
        let probe = s.probe().expect("there");
        assert_eq!(probe.api_version, "1.20.0");
        assert_eq!(probe.account, None);
        assert!(!probe.guest_key);
        let paths: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.path.clone())
            .collect();
        assert_eq!(
            paths,
            ["/api"],
            "only the index: no guest key is made to look"
        );
        let (old, _) = serve(bingle(r#"{"version":"9.0.0","api_version":"2.0.0"}"#, &[]));
        s.use_server(&old);
        assert!(matches!(s.probe(), Err(Problem::Incompatible(_))));
    }

    #[test]
    fn a_guest_key_typed_is_kept_only_if_the_server_knows_it() {
        let (addr, _) = serve(bingle(INDEX, &["bgg_old", "bgg_mine"]));
        let mut s = session(&addr);
        let origin = ServerUrl::parse(&addr).unwrap().origin();
        s.store
            .set_guest_key(&origin, Some("bgg_old".into()))
            .unwrap();
        let refused = s.adopt_guest_key("bgg_typo").unwrap_err();
        assert!(matches!(refused, Problem::Refused { status: 401, .. }));
        assert_eq!(
            s.store.guest_key(&origin),
            Some("bgg_old"),
            "the kept one stays"
        );
        s.adopt_guest_key(" bgg_mine ").expect("known");
        assert_eq!(s.store.guest_key(&origin), Some("bgg_mine"));
        s.connect().expect("connects as that guest");
        assert_eq!(s.store.guest_key(&origin), Some("bgg_mine"));
    }

    #[test]
    fn connecting_reads_the_index_makes_a_guest_once_and_lists_the_games() {
        let (addr, seen) = serve(bingle(INDEX, &["bgg_new"]));
        let mut s = session(&addr);
        let snap = s.connect().expect("connects");
        assert_eq!(snap.api_version, "1.20.0");
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
            let kept = |yours: bool| {
                format!(
                    r#"{{"game_def_id":"hex-7","status":"waiting","seats":[{ana},{open}],"reservations":[{{"seat":2,"handle":"bob","yours":{yours}}}]}}"#
                )
            };
            match (req.method.as_str(), req.path.as_str()) {
                ("POST", "/api/guest") => (201, r#"{"key":"bgg_new"}"#.into()),
                ("GET", "/api/games/KEPT01") => (200, kept(false)),
                ("GET", "/api/games/KEPT02") => (200, kept(true)),
                ("POST", "/api/games/KEPT02/join") => (
                    200,
                    r#"{"role":"player","seat":2,"color":"blue","status":"waiting"}"#.into(),
                ),
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
        for code in ["WAIT01", "BUSY01", "MINE01", "KEPT01", "KEPT02"] {
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
            [
                "/api/games/WAIT01/join",
                "/api/games/KEPT02/join",
                "/api/games/BUSY01/resign"
            ],
            "a game already playing, with a seat of this guest's, or whose open seat is kept for \
             somebody else, is only watched; a seat kept for this viewer is taken"
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
    pub(crate) fn accounts(second: bool, ended: Arc<Mutex<bool>>) -> Box<Handler> {
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

    /// A server that takes a setup file's sign-in code once, and knows no guest key.
    pub(crate) fn takes_a_code_once() -> Box<Handler> {
        let inner = accounts(false, Arc::new(Mutex::new(false)));
        let spent = Mutex::new(false);
        Box::new(move |req: &Seen| {
            let body: serde_json::Value = serde_json::from_str(&req.body).unwrap_or_default();
            if req.path != "/auth/login" || body.get("code").is_none() {
                return inner(req);
            }
            let mut spent = spent.lock().unwrap();
            if req.auth.is_some() {
                // Refused before the sign-in is looked at: the code is not spent by it.
                let error = "auth: that guest key is not one this server knows";
                (401, format!(r#"{{"error":"{error}"}}"#))
            } else if body["code"] == "bgc_good" && !*spent {
                *spent = true;
                let me = r#"{"kind":"account","handle":"ana","display_name":"Ana","accounts":true,"signup":false}"#;
                (
                    200,
                    format!(r#"{{"token":"bgs_ana","expires":"x","me":{me}}}"#),
                )
            } else {
                let error =
                    "auth: that sign-in code does not work: it was used already, or it is too old";
                (401, format!(r#"{{"error":"{error}"}}"#))
            }
        })
    }

    #[test]
    fn a_setup_files_code_signs_in_once_and_a_guest_key_the_server_forgot_does_not_cost_it() {
        let (addr, seen) = serve(takes_a_code_once());
        let origin = ServerUrl::parse(&addr).unwrap().origin();
        let mut store = Store::load(None);
        store
            .set_guest_key(&origin, Some("bgg_forgotten".into()))
            .unwrap();
        let mut s = Session::new(Some(&addr), store);

        let signed = s.sign_in_with_code("bgc_good").unwrap();
        let SignIn::Done(account) = signed else {
            panic!("{signed:?}")
        };
        assert_eq!(account.handle, "ana");
        assert_eq!(s.store().session_token(&origin), Some("bgs_ana"));
        {
            let seen = seen.lock().unwrap();
            let logins: Vec<&Seen> = seen.iter().filter(|r| r.path == "/auth/login").collect();
            assert_eq!(logins.len(), 2);
            assert_eq!(
                logins[0].auth.as_deref(),
                Some("Bearer bgg_forgotten"),
                "the guest goes along, as with any sign-in"
            );
            assert_eq!(logins[1].auth, None, "then the code by itself");
            let sent: serde_json::Value = serde_json::from_str(&logins[1].body).unwrap();
            assert_eq!(sent, serde_json::json!({"code": "bgc_good"}));
        }
        // It worked once.
        let again = s.sign_in_with_code("bgc_good");
        let Err(Problem::Refused {
            status: 401,
            message,
        }) = again
        else {
            panic!("{again:?}")
        };
        assert!(message.contains("used already"), "{message}");
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
    fn the_friends_calls_go_to_their_paths_with_the_session() {
        let ended = Arc::new(Mutex::new(false));
        let inner = accounts(false, ended);
        let (addr, seen) = serve(Box::new(move |req: &Seen| {
            let signed_in = req.auth.as_deref() == Some("Bearer bgs_ana");
            match (req.method.as_str(), req.path.as_str()) {
                ("POST", p) if signed_in && p.starts_with("/api/friends/") => {
                    (200, r#"{"ok":true}"#.into())
                }
                ("GET", "/api/friends") if signed_in => (
                    200,
                    r#"{"friends":[{"name":"@bo","handle":"bo","address":"a","online":true,"can_write":true}],
                    "incoming":null,"outgoing":[],"invites":[{"room":"AB12CD","game":"Hex (7×7)","def":"hex-7","from":"@bo","status":"pending"}]}"#
                        .into(),
                ),
                ("PUT" | "GET", "/api/friends/settings") if signed_in => (
                    200,
                    r#"{"mail_requests":false,"show_presence":false,"accept_invites":true}"#.into(),
                ),
                ("POST", "/api/here") if signed_in => (200, r#"{"people":[]}"#.into()),
                ("POST", "/api/dm") if signed_in => (
                    200,
                    r#"{"id":7,"from":"me","name":"ana","to":"a","text":"hello","at":"2026-11-20T03:00:00Z"}"#.into(),
                ),
                ("POST", "/api/dm/history") if signed_in => (
                    200,
                    r#"{"messages":[{"id":6,"from":"a","name":"bo","to":"me","text":"hi","at":"x"}]}"#.into(),
                ),
                _ => inner(req),
            }
        }));
        let mut s = session(&addr);
        s.sign_in("ana", "pw").unwrap();
        assert!(s.has_account());
        for call in [
            FriendCall::Ask(" @Bo ".into()),
            FriendCall::Answer {
                handle: "bo".into(),
                accept: false,
            },
            FriendCall::Remove("bo".into()),
            FriendCall::Invite {
                handle: "bo".into(),
                room: "AB12CD".into(),
            },
            FriendCall::AnswerInvite {
                room: "AB12CD".into(),
                accept: true,
            },
            FriendCall::Dismiss("AB12CD".into()),
        ] {
            s.friend(&call)
                .unwrap_or_else(|e| panic!("{call:?}: {e:?}"));
        }
        let list = s.friends().unwrap();
        assert!(list.friends[0].online);
        assert!(list.incoming.is_empty(), "null reads as none");
        assert!(list.invites[0].pending());
        let off = FriendSettings {
            mail_requests: false,
            show_presence: false,
            accept_invites: true,
        };
        assert_eq!(s.friend_settings(Some(off)).unwrap(), off);
        s.here(true).unwrap();
        let sent_message = s.write("a", " hello ").unwrap();
        assert_eq!((sent_message.id, sent_message.text.as_str()), (7, "hello"));
        let history = s.history("a").unwrap();
        assert_eq!(history[0].name, "bo");

        let seen = seen.lock().unwrap();
        let sent: Vec<(String, String, serde_json::Value)> = seen
            .iter()
            .filter(|r| r.path.starts_with("/api/friends") || r.path == "/api/here")
            .filter(|r| r.method != "GET")
            .map(|r| {
                (
                    r.method.clone(),
                    r.path.clone(),
                    serde_json::from_str(&r.body).unwrap(),
                )
            })
            .collect();
        use serde_json::json;
        let post = |p: &str, b| ("POST".to_string(), p.to_string(), b);
        assert_eq!(
            sent,
            [
                post("/api/friends/request", json!({"handle": "Bo"})),
                post(
                    "/api/friends/answer",
                    json!({"handle": "bo", "accept": false})
                ),
                post("/api/friends/remove", json!({"handle": "bo"})),
                post(
                    "/api/friends/invite",
                    json!({"handle": "bo", "room": "AB12CD"})
                ),
                post(
                    "/api/friends/invite-answer",
                    json!({"room": "AB12CD", "accept": true})
                ),
                post("/api/friends/dismiss", json!({"room": "AB12CD"})),
                (
                    "PUT".to_string(),
                    "/api/friends/settings".to_string(),
                    json!({"mail_requests": false, "show_presence": false, "accept_invites": true})
                ),
                post("/api/here", json!({"here": true})),
            ],
            "a handle is sent without its @ and spaces"
        );
        assert!(
            seen.iter()
                .filter(|r| r.path.starts_with("/api/friends") || r.path == "/api/here")
                .all(|r| r.auth.as_deref() == Some("Bearer bgs_ana")),
            "as the account"
        );
        let dm: Vec<(String, serde_json::Value)> = seen
            .iter()
            .filter(|r| r.path.starts_with("/api/dm"))
            .map(|r| (r.path.clone(), serde_json::from_str(&r.body).unwrap()))
            .collect();
        assert_eq!(
            dm,
            [
                ("/api/dm".to_string(), json!({"to": "a", "text": "hello"})),
                ("/api/dm/history".to_string(), json!({"with": "a"})),
            ],
            "a message is sent trimmed, to the friend's address"
        );
    }

    #[test]
    fn a_call_rings_signals_and_hangs_up_as_the_account() {
        let ended = Arc::new(Mutex::new(false));
        let inner = accounts(false, ended);
        let (addr, seen) = serve(Box::new(move |req: &Seen| {
            let signed_in = req.auth.as_deref() == Some("Bearer bgs_ana");
            match (req.method.as_str(), req.path.as_str()) {
                ("POST", "/api/dm/call" | "/api/dm/signal") if signed_in => {
                    (200, r#"{"ok":true}"#.into())
                }
                ("GET", "/api/rtc/config") if signed_in => (
                    200,
                    r#"{"ice_servers":[{"urls":["stun:stun.example.com:3478"]},
                    {"urls":["turn:turn.example.com"],"username":"u","credential":"c"}]}"#
                        .into(),
                ),
                _ => inner(req),
            }
        }));
        let mut s = session(&addr);
        s.sign_in("ana", "pw").unwrap();
        let servers = s.ice_servers().unwrap();
        assert_eq!(servers[0].urls, ["stun:stun.example.com:3478"]);
        assert_eq!(servers[1].credential.as_deref(), Some("c"));
        s.direct_call("a", true).unwrap();
        s.direct_signal("a", "retry", serde_json::json!({}))
            .unwrap();
        s.direct_call("a", false).unwrap();
        let seen = seen.lock().unwrap();
        let sent: Vec<(String, serde_json::Value)> = seen
            .iter()
            .filter(|r| r.path.starts_with("/api/dm"))
            .map(|r| (r.path.clone(), serde_json::from_str(&r.body).unwrap()))
            .collect();
        use serde_json::json;
        assert_eq!(
            sent,
            [
                ("/api/dm/call".into(), json!({"to": "a", "join": true})),
                (
                    "/api/dm/signal".into(),
                    json!({"to": "a", "kind": "retry", "data": {}})
                ),
                ("/api/dm/call".into(), json!({"join": false})),
            ]
        );
        assert!(
            seen.iter()
                .filter(|r| r.path.starts_with("/api/dm") || r.path == "/api/rtc/config")
                .all(|r| r.auth.as_deref() == Some("Bearer bgs_ana"))
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
            reserved: [(3, "bob".to_string())].into(),
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
                "reserved_seats": {"3": "bob"},
            })
        );
        let plain = GameSettings {
            seat: 1,
            bots: BTreeMap::new(),
            time: TimeControl::PerMove { seconds: 60 },
            lobby: false,
            reserved: BTreeMap::new(),
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

    #[test]
    fn a_game_that_may_keep_secrets_is_read_as_the_seat_this_client_holds() {
        let seat = |seat: u32, name: &str, bot: &str, yours: bool| GameSeat {
            seat,
            color: String::new(),
            name: name.into(),
            bot: bot.into(),
            yours,
        };
        let seats = [
            seat(1, "Ana", "", true),
            seat(2, "Ana · Blue", "", true),
            seat(3, "Bot", "mcts", false),
            seat(4, "Ben", "", false),
        ];
        let open = GameInfo::default();
        assert!(held(&open, &seats).is_empty(), "the same for everybody");
        let secret = GameInfo {
            engine: "ruleset".into(),
            ..GameInfo::default()
        };
        let mine = held(&secret, &seats);
        assert_eq!(
            mine,
            [(1, "Ana".to_string()), (2, "Ana · Blue".to_string())]
        );
        // As the one on impulse, of several; else as the first.
        assert_eq!(viewer(&mine, Some(2)), Some("Ana · Blue"));
        assert_eq!(viewer(&mine, Some(4)), Some("Ana"));
        assert_eq!(viewer(&mine, None), Some("Ana"));
        assert_eq!(viewer(&[], Some(1)), None, "a spectator");
        // Seats changed in the lobby: each is found again by its player's name.
        let moved = [seat(3, "ana", "", false), seat(1, "Ben", "", false)];
        assert_eq!(held_now(&mine, &moved), [(3, "Ana".to_string())]);
    }

    #[test]
    fn a_seats_own_view_is_asked_for_by_its_players_name() {
        let state = r#"{"state":{"cells":{}},"status":"active","move_seq":3,"impulse_seat":2}"#;
        let (addr, seen) = serve(Box::new(move |r: &Seen| {
            match (r.method.as_str(), r.path.as_str()) {
                ("POST", "/api/guest") => (201, r#"{"key":"bgg_new"}"#.into()),
                (_, p) if p.starts_with("/api/games/AB12CD/state") => (200, state.into()),
                _ => (404, "{}".into()),
            }
        }));
        let mut s = session(&addr);
        let held = [(1, "Ana".to_string()), (2, "Ana · Blue".to_string())];
        let read = s.state_seen("AB12CD", &held).unwrap();
        assert_eq!(read.move_seq, 3);
        assert_eq!(s.state("AB12CD", None).unwrap().move_seq, 3);
        let paths: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path.contains("/state"))
            .map(|r| r.path.clone())
            .collect();
        assert_eq!(
            paths,
            [
                "/api/games/AB12CD/state?player_name=Ana",
                // The seat on impulse is another of this client's: read again as that one.
                "/api/games/AB12CD/state?player_name=Ana%20%C2%B7%20Blue",
                "/api/games/AB12CD/state",
            ]
        );
    }
}
