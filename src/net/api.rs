//! The parts of Bingle's API this client reads, as `serde` types, and the check that a server
//! speaks the API version the client was built for.
//!
//! The types are written by hand and hold only the fields the client uses; unknown fields are
//! ignored, which is what lets the server add fields in a minor version (Bingle's ADR 0134). A
//! test checks them against the JSON Schema Bingle publishes for [`API_VERSION`], kept in
//! `schema/`.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

/// The version of Bingle's API this client is written against: 1.1 added `POST /api/bot-matches`,
/// 1.2 the regions each seat must connect (`goals`), 1.4 the lobby's calls (`start`, `seat`,
/// `leave`, `remove`, `bots`) to the schema, 1.5 the sign-in that answers with a session
/// token, for a client that is not a browser, 1.6 voice chat's calls and events, 1.8 the
/// games built from the server's private overlay marked `private`, and 1.10 the positions a
/// legal selection takes pieces from (`captures`).
pub const API_VERSION: Version = Version {
    major: 1,
    minor: 10,
    patch: 0,
};

/// `GET /api`.
#[derive(Debug, Clone, Deserialize)]
pub struct Index {
    /// The server's own version, which moves with every change to it.
    pub version: String,
    /// Absent on a server from before the API had a version of its own.
    #[serde(default)]
    pub api_version: Option<String>,
}

/// `POST /auth/login`'s body. With `token`, the session comes back as a token in the answer
/// rather than as a browser's cookie.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoginRequest {
    pub handle: String,
    pub password: String,
    pub token: bool,
}

/// `POST /auth/second`'s body: the second step of a sign-in, with a code from the account's
/// authenticator app or one of its recovery codes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SecondRequest {
    /// What `POST /auth/login` answered.
    pub pending: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub recovery_code: String,
}

/// What `POST /auth/login` and `POST /auth/second` answer with `token`: the session's token, or
/// that a code comes next.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SessionTokenResponse {
    /// `bgs_…`; empty when a code comes next.
    #[serde(default)]
    pub token: String,
    /// Who is signed in.
    #[serde(default)]
    pub me: Option<Me>,
    /// The password was right, and a code comes next, sent with `pending`.
    #[serde(default)]
    pub second_factor: bool,
    #[serde(default)]
    pub pending: String,
}

/// `GET /auth/me`: who is asking.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Me {
    /// "account", "guest" or "anonymous".
    pub kind: String,
    #[serde(default)]
    pub handle: String,
    #[serde(default)]
    pub display_name: String,
    /// The account may do nothing but change its password until it does.
    #[serde(default)]
    pub must_change_password: bool,
}

/// `POST /auth/logout`'s answer: what the request is now.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LogoutResponse {
    pub kind: String,
}

/// `GET /api/friends`: an account's friends, requests and game invitations.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct FriendsList {
    #[serde(default, deserialize_with = "null_as_default")]
    pub friends: Vec<Friend>,
    /// Asked by them, to be answered.
    #[serde(default, deserialize_with = "null_as_default")]
    pub incoming: Vec<Friend>,
    /// Asked by this account, not answered yet.
    #[serde(default, deserialize_with = "null_as_default")]
    pub outgoing: Vec<Friend>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub invites: Vec<FriendInvite>,
}

/// A friend, or somebody a request is between.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Friend {
    pub handle: String,
    /// What the list calls them ("@ana").
    pub name: String,
    /// Where messages to them go.
    #[serde(default)]
    pub address: String,
    /// They can be seen to be here (only ever true for a friend).
    #[serde(default)]
    pub online: bool,
    /// A message would reach them: they are here, or hidden but have written themselves.
    #[serde(default)]
    pub can_write: bool,
}

/// One direct message, pushed as a `dm` event, answered by `POST /api/dm`, and listed by
/// `POST /api/dm/history`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DirectMessage {
    pub id: u64,
    /// The sender's address, and `to` the receiver's.
    pub from: String,
    pub to: String,
    /// What the sender went by.
    #[serde(default)]
    pub name: String,
    pub text: String,
    /// When it was sent (RFC 3339).
    #[serde(default)]
    pub at: String,
}

/// `POST /api/dm`'s body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DirectMessageRequest {
    pub to: String,
    pub text: String,
}

/// `POST /api/dm/history`'s body: what was said with an address.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DirectHistoryRequest {
    pub with: String,
}

/// `POST /api/dm/history`'s answer, oldest first.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DirectHistory {
    #[serde(default, deserialize_with = "null_as_default")]
    pub messages: Vec<DirectMessage>,
}

/// An invitation to a game, from a friend.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FriendInvite {
    pub room: String,
    /// The game's name.
    pub game: String,
    /// Who invited ("@ana").
    pub from: String,
    /// "pending", "accepted" or "declined".
    pub status: String,
    /// News of the game, shown until dismissed.
    #[serde(default)]
    pub notice: String,
}

impl FriendInvite {
    pub fn pending(&self) -> bool {
        self.status == "pending"
    }
}

/// The body of the friends calls: who (`request`, `answer`, `remove`, `invite`), which game
/// (`invite`, `invite-answer`, `dismiss`), and the answer (`answer`, `invite-answer`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FriendBody {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub handle: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub room: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<bool>,
}

/// `GET` and `PUT /api/friends/settings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendSettings {
    /// Friend requests are mailed too.
    pub mail_requests: bool,
    /// Friends see when the account is here (Bingle's ADR 0106).
    pub show_presence: bool,
    /// Friends may invite the account to games.
    pub accept_invites: bool,
}

/// `POST /api/here`'s body: here, or (`false`) gone.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HereRequest {
    pub here: bool,
}

/// `POST /api/here`'s answer: who else is here. The client reads only that it was taken.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HereResponse {}

/// `POST /api/dm/call`'s body: ring (or answer) `to`, or, with `join` false, hang up.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DirectCallRequest {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub to: String,
    pub join: bool,
}

/// `POST /api/dm/signal`'s body: one WebRTC message for the person called, `data` in the form a
/// browser's `RTCPeerConnection` uses (`voice::Signal`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DirectSignalRequest {
    pub to: String,
    pub kind: String,
    pub data: serde_json::Value,
}

/// `GET /api/rtc/config`: the ICE servers for a call. TURN credentials expire, so it is read
/// again for each call.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RtcConfig {
    #[serde(default, deserialize_with = "null_as_default")]
    pub ice_servers: Vec<IceServer>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IceServer {
    #[serde(default, deserialize_with = "null_as_default")]
    pub urls: Vec<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub credential: Option<String>,
}

/// A `dm-call` event: somebody rings (or has answered), or a call has ended.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DirectCall {
    /// "ring" or "end".
    pub kind: String,
    /// Their address.
    pub from: String,
    #[serde(default)]
    pub name: String,
    /// It ended because they joined another call.
    #[serde(default)]
    pub dropped: bool,
}

/// A `dm-rtc` event: one WebRTC message from the person called.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DirectSignal {
    pub from: String,
    pub kind: String,
    pub data: serde_json::Value,
}

/// `POST /api/games/{code}/voice`'s body: the seat of `player_name` joins the game's voice chat,
/// or says it is still there (every 15 s; a place lapses after 45), or (`active` false) leaves.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VoiceRequest {
    pub player_name: String,
    pub active: bool,
}

/// `GET` and `POST /api/games/{code}/voice`'s answer: the seats in the call.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct VoicePeers {
    #[serde(default, deserialize_with = "null_as_default")]
    pub peers: Vec<VoicePeer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VoicePeer {
    pub seat: u32,
    pub name: String,
}

/// `POST /api/games/{code}/voice/signal`'s body: one WebRTC message for the seat `to`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VoiceSignalRequest {
    pub player_name: String,
    pub to: u32,
    pub kind: String,
    pub data: serde_json::Value,
}

/// A message on a game's voice socket (`/ws/{code}?only=voice`), of the kinds a call acts on.
#[derive(Debug, Clone, PartialEq)]
pub enum VoiceEvent {
    /// The socket listens for the seat: the call may be joined.
    Joined,
    /// A seat joined the call (`join`) or left it.
    Voice { join: bool, seat: u32, name: String },
    /// A WebRTC message from the seat `from`, for this one.
    Signal {
        from: u32,
        kind: String,
        data: serde_json::Value,
    },
    /// Anything else.
    Other(String),
}

impl VoiceEvent {
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        #[derive(Deserialize)]
        struct Envelope {
            #[serde(rename = "type")]
            kind: String,
            #[serde(default)]
            data: serde_json::Value,
        }
        #[derive(Deserialize)]
        struct VoiceData {
            kind: String,
            seat: u32,
            #[serde(default)]
            name: String,
        }
        #[derive(Deserialize)]
        struct RtcData {
            from: u32,
            kind: String,
            data: serde_json::Value,
        }
        let env: Envelope = serde_json::from_str(text)?;
        Ok(match env.kind.as_str() {
            "joined" => VoiceEvent::Joined,
            "voice" => {
                let d: VoiceData = serde_json::from_value(env.data)?;
                VoiceEvent::Voice {
                    join: d.kind == "join",
                    seat: d.seat,
                    name: d.name,
                }
            }
            "rtc" => {
                let d: RtcData = serde_json::from_value(env.data)?;
                VoiceEvent::Signal {
                    from: d.from,
                    kind: d.kind,
                    data: d.data,
                }
            }
            other => VoiceEvent::Other(other.to_string()),
        })
    }
}

/// The answer of the calls that only say they were done.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OkResponse {
    pub ok: bool,
}

/// A message on `GET /ws-dm`, the account's own socket, of the kinds the client acts on.
#[derive(Debug, Clone, PartialEq)]
pub enum AccountEvent {
    /// The first message: the account's own address, the one others write to.
    Ready { address: String },
    /// The friends list changed: read it again.
    Friends,
    /// A direct message, from a friend or sent by this account (from here or elsewhere).
    Message(DirectMessage),
    /// A call rings, or ends.
    Call(DirectCall),
    /// A WebRTC message of a call.
    Signal(DirectSignal),
    /// A kind this client skips (badges, and any added later).
    Other(String),
}

impl AccountEvent {
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        #[derive(Deserialize)]
        struct Envelope {
            #[serde(rename = "type")]
            kind: String,
            #[serde(default)]
            data: serde_json::Value,
        }
        #[derive(Deserialize)]
        struct ReadyData {
            #[serde(default)]
            address: String,
        }
        let env: Envelope = serde_json::from_str(text)?;
        Ok(match env.kind.as_str() {
            "ready" => AccountEvent::Ready {
                address: serde_json::from_value::<ReadyData>(env.data)?.address,
            },
            "friends" => AccountEvent::Friends,
            "dm" => AccountEvent::Message(serde_json::from_value(env.data)?),
            "dm-call" => AccountEvent::Call(serde_json::from_value(env.data)?),
            "dm-rtc" => AccountEvent::Signal(serde_json::from_value(env.data)?),
            other => AccountEvent::Other(other.to_string()),
        })
    }
}

/// `POST /api/guest`.
#[derive(Debug, Clone, Deserialize)]
pub struct GuestKey {
    pub key: String,
}

/// One entry of `GET /api/game-defs`: a game, or a layout or variant of one.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GameDef {
    pub id: String,
    /// The name from the catalogue, such as "Hex (11×11)".
    pub name: String,
    pub seats_required: u32,
    /// The id of the game this is a layout or variant of.
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub description: String,
    /// Still being designed.
    #[serde(default)]
    pub prototype: bool,
    /// The game's own semantic version: below 1.0.0 it is a beta.
    #[serde(default)]
    pub version: String,
    /// How many tutorials the game has.
    #[serde(default)]
    pub tutorials: u32,
    /// Built from the server's private overlay (a commercial game): this build does not show it.
    #[serde(default)]
    pub private: bool,
}

/// `GET /api/games`.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenGames {
    #[serde(default, deserialize_with = "null_as_default")]
    pub games: Vec<OpenGame>,
}

/// An unfinished game this viewer may find.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OpenGame {
    pub room_code: String,
    pub game_def_id: String,
    pub name: String,
    /// "waiting" (for players, or for the host to start it) or "active".
    pub status: String,
    pub seats: u32,
    pub open_seats: u32,
    /// The seats that are taken, by people and bots.
    #[serde(default, deserialize_with = "null_as_default")]
    pub players: Vec<Player>,
    /// Whose impulse the game is waiting for, once it has started.
    #[serde(default)]
    pub impulse_seat: Option<u32>,
    /// It waits for its host to start it, even with every seat filled.
    #[serde(default)]
    pub lobby: bool,
    /// A seat is kept for this viewer, a friend of the game's creator.
    #[serde(default)]
    pub reserved_for_you: bool,
}

/// A taken seat, as `GET /api/games` lists it (these field names are capitalised on the wire).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Player {
    #[serde(rename = "Seat")]
    pub seat: u32,
    /// The seat's colour word: an identifier, not a colour to draw (Bingle's ADR 0124).
    #[serde(rename = "Color")]
    pub color: String,
    #[serde(rename = "Name")]
    pub name: String,
    /// The bot's id when a built-in bot plays the seat, empty otherwise.
    #[serde(rename = "Bot", default)]
    pub bot: String,
}

/// `GET /api/game-defs/{id}`: a game as the rules see it. Only what the client draws from.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct GameInfo {
    pub id: String,
    pub name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub seats: Vec<SeatDef>,
    pub board: Board,
    /// The game's actions, for what a button that plays one says.
    #[serde(default, deserialize_with = "null_as_default")]
    pub actions: Vec<ActionDef>,
    /// The regions of the board each seat must connect, as data (API 1.2).
    #[serde(default, deserialize_with = "null_as_default")]
    pub goals: Vec<Goal>,
    /// The kinds of piece, and the colour word of each that has one.
    #[serde(default, deserialize_with = "null_as_default")]
    pub pieces: Vec<PieceDef>,
    /// Numbers each seat keeps that hold no pieces (Blooms' captured stones), and the count a win
    /// rule wants of each.
    #[serde(default, deserialize_with = "null_as_default")]
    pub counters: Vec<Counter>,
    /// The opening, when the game has one.
    #[serde(default)]
    pub opening: Option<Opening>,
    /// What each seat starts with off the board, by pool: pieces (Tak's stones and capstones)
    /// or a counter's number.
    #[serde(default, deserialize_with = "null_as_default")]
    pub reserves: BTreeMap<String, u32>,
    /// The suits and the kinds of card of a card game, and the zones its cards lie in (the
    /// position names its cards by id only). None for a game with no cards.
    #[serde(default)]
    pub cards: Option<Cards>,
}

/// What a card game's cards are (`GameInfo.cards`). Only what the client draws from: not the
/// colours of the web page's own drawing (Bingle's ADR 0124).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Cards {
    /// Each suit by its id.
    #[serde(default, deserialize_with = "null_as_default")]
    pub suits: BTreeMap<String, Suit>,
    /// Every kind of card the game has.
    #[serde(default, deserialize_with = "null_as_default")]
    pub types: Vec<CardType>,
    /// The places cards can be.
    #[serde(default, deserialize_with = "null_as_default")]
    pub zones: Vec<ZoneDef>,
}

impl Cards {
    /// The kind of a card on the table, by its id: a kind's id, with `#` and which of its
    /// copies it is when the game has several (two decks).
    pub fn type_of(&self, card: &str) -> Option<&CardType> {
        let kind = card.split('#').next().unwrap_or(card);
        self.types.iter().find(|t| t.id == kind)
    }
}

/// A suit: how it is named and its pip, as text.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Suit {
    #[serde(default)]
    pub label: String,
    /// The text drawn as its pip (♠, or a short word).
    #[serde(default)]
    pub symbol: String,
}

/// A kind of card: its suit's id and its rank, when it has them.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct CardType {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub suit: String,
    #[serde(default)]
    pub rank: String,
}

/// A place cards can be.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct ZoneDef {
    pub id: String,
    #[serde(default)]
    pub label: String,
}

/// A number each seat keeps in a reserve pool of that `id`, holding no pieces.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Counter {
    pub id: String,
    #[serde(default)]
    pub label: String,
    /// The count a win rule wants (Blooms' captures), 0 for none.
    #[serde(default)]
    pub max: u32,
}

/// A kind of piece of a game definition.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct PieceDef {
    /// Its id, as a selection's `piece` names it.
    #[serde(rename = "type")]
    pub id: String,
    /// The colour word it is played in, when it has one (Blooms' stones each have their own).
    #[serde(default)]
    pub color: String,
    /// The reserve pool it is taken from (Tak's flat and standing stones share one).
    #[serde(default)]
    pub pool: String,
    /// Whether another piece may go on it (a standing stone and a capstone may not).
    #[serde(default = "yes")]
    pub coverable: bool,
}

fn yes() -> bool {
    true
}

/// The opening, when the game has one: its first `plies`, and whose pieces are placed in it
/// (`opponent`: each seat places a piece of another's, as Tak's first stones are).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Opening {
    pub plies: u64,
    #[serde(default)]
    pub owner: String,
}

/// A goal of the game: for `connect`, a group of the seat's pieces that joins regions of the
/// board. A client skips a type it does not know.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Goal {
    #[serde(rename = "type")]
    pub kind: String,
    /// The seat whose goal it is; every seat's when absent.
    #[serde(default)]
    pub seat: Option<u32>,
    /// Pairs of regions to connect, any one pair winning.
    #[serde(default, deserialize_with = "null_as_default")]
    pub regions: Vec<Vec<String>>,
    /// Regions of which a group must touch `touch`, instead of pairs.
    #[serde(default, deserialize_with = "null_as_default")]
    pub of: Vec<String>,
}

impl Goal {
    /// Every region the goal names.
    pub fn named_regions(&self) -> impl Iterator<Item = &str> {
        self.regions
            .iter()
            .flatten()
            .chain(&self.of)
            .map(String::as_str)
    }
}

/// One of a game's actions (placing, stepping, swapping sides), as the rules name it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ActionDef {
    pub id: String,
    /// In words, such as "Swap sides".
    pub label: String,
    /// The kind of action it is: what the rules do with it ("patience", a move of cards
    /// between piles).
    #[serde(rename = "type", default)]
    pub kind: String,
}

/// A seat of a game definition.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SeatDef {
    pub seat: u32,
    /// An identifier, not a colour to draw (Bingle's ADR 0124).
    pub color: String,
    /// What the seat is called, in words ("Red (top – bottom)").
    pub label: String,
    /// Every colour word the seat plays, when it plays several (Bingle's ADR 0117).
    #[serde(default, deserialize_with = "null_as_default")]
    pub colors: Vec<String>,
}

/// The board as the rules see it.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Board {
    /// "grid", "hex" or "explicit".
    pub kind: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub positions: Vec<BoardPosition>,
    /// Named sets of positions, such as a board's edges.
    #[serde(default, deserialize_with = "null_as_default")]
    pub regions: BTreeMap<String, Vec<String>>,
}

/// One position, at its layout coordinates: one unit between neighbours, y growing downwards.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BoardPosition {
    pub id: String,
    pub x: f32,
    pub y: f32,
}

/// One entry of `GET /api/bots`: a built-in bot.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BotInfo {
    pub id: String,
    pub label: String,
}

/// `POST /api/bot-matches`' body. `delay_ms` is left to the server's default.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BotMatchRequest {
    pub game_def_id: String,
    pub bot_a: String,
    pub bot_b: String,
}

/// `POST /api/bot-matches`' answer.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BotMatchResponse {
    pub room_code: String,
}

/// `GET /api/games/{code}`: who sits where.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Game {
    pub game_def_id: String,
    /// "waiting" until every seat is filled, then "active", then "finished".
    pub status: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub seats: Vec<GameSeat>,
    /// A link to the game's page, to send to somebody who should join it.
    #[serde(default)]
    pub invite_url: String,
    /// Each seat's time, in a game played with a clock.
    #[serde(default)]
    pub clock: Option<Clock>,
    /// When the seat on impulse runs out of time (RFC 3339), in a timed game, and the server's
    /// time when it said so.
    #[serde(default)]
    pub impulse_deadline: String,
    #[serde(default)]
    pub server_time: String,
    /// The time each move may take, in a game timed by the move.
    #[serde(default)]
    pub move_time_seconds: Option<u64>,
    /// It waits for its host's start, even with every seat taken.
    #[serde(default)]
    pub lobby: bool,
    /// The host's seat: the person who may start the game and clear other seats before it
    /// starts. Absent when no person sits.
    #[serde(default)]
    pub host_seat: Option<u32>,
    /// The seats kept for friends of the creator (Bingle's ADR 0107).
    #[serde(default, deserialize_with = "null_as_default")]
    pub reservations: Vec<Reservation>,
}

/// A seat kept for a friend.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Reservation {
    pub seat: u32,
    pub handle: String,
    /// It is kept for this viewer.
    #[serde(default)]
    pub yours: bool,
}

/// A game clock: what each seat has left, and whose time is running.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Clock {
    #[serde(default, deserialize_with = "null_as_default")]
    pub seats: Vec<SeatClock>,
    #[serde(default)]
    pub running: Option<u32>,
    #[serde(default)]
    pub increment_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SeatClock {
    pub seat: u32,
    pub initial_seconds: u64,
    pub seconds_left: f64,
}

impl Game {
    /// The friend `seat` is kept for, if it is kept for somebody.
    pub fn kept_for(&self, seat: u32) -> Option<&Reservation> {
        self.reservations.iter().find(|r| r.seat == seat)
    }

    /// A seat this viewer may take: open, and not kept for somebody else.
    pub fn has_seat_for_me(&self) -> bool {
        self.seats.iter().any(|s| {
            s.name.is_empty() && s.bot.is_empty() && self.kept_for(s.seat).is_none_or(|r| r.yours)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GameSeat {
    pub seat: u32,
    pub color: String,
    /// Empty while the seat is free.
    #[serde(default)]
    pub name: String,
    /// The bot's id when a built-in bot plays the seat.
    #[serde(default)]
    pub bot: String,
    /// The seat is this client's guest's own.
    #[serde(default)]
    pub yours: bool,
}

/// `POST /api/games`' body. With `opponent`, that bot fills every seat but the creator's and the
/// game starts at once; without it, the game waits for people to join.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreateGameRequest {
    pub game_def_id: String,
    pub player_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opponent: Option<String>,
    /// The creator's seat (the first, when left out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seat: Option<u32>,
    /// A built-in bot in each of these seats, by seat number; the others stay open.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub seat_bots: BTreeMap<String, String>,
    /// A game clock: each seat's time, and what a seat gains each time it hands the turn on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_increment_seconds: Option<u64>,
    /// The time each move may take.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_time_seconds: Option<u64>,
    /// Wait for the creator's start, even with every seat taken.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub lobby: bool,
    /// Seats kept for friends of the creator, by seat number: their handles. The server then
    /// keeps the game waiting for its creator's start, and invites them.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub reserved_seats: BTreeMap<String, String>,
}

impl CreateGameRequest {
    /// The creator in the first seat, the others open or filled by `opponent`, and no clock.
    pub fn new(game_def_id: &str, player_name: &str, opponent: Option<String>) -> Self {
        CreateGameRequest {
            game_def_id: game_def_id.to_string(),
            player_name: player_name.to_string(),
            opponent,
            seat: None,
            seat_bots: BTreeMap::new(),
            clock_seconds: None,
            clock_increment_seconds: None,
            move_time_seconds: None,
            lobby: false,
            reserved_seats: BTreeMap::new(),
        }
    }
}

/// `POST /api/games/{code}/join`'s body: take an open seat.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JoinRequest {
    pub player_name: String,
    pub role: String,
}

/// `POST /api/games/{code}/join`'s answer.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct JoinResponse {
    pub role: String,
    #[serde(default)]
    pub seat: Option<u32>,
}

/// The body of the calls a seated player makes about the game as a whole (`resign`, and in a
/// game not yet started `start` and `leave`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayerRequest {
    pub player_name: String,
}

/// The body of the lobby's calls on one seat: move to it (`seat`), or clear it (`remove`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeatRequest {
    pub player_name: String,
    pub seat: u32,
}

/// `POST /api/games/{code}/bots`' body: a built-in bot in an open seat of a game not yet started.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AddBotRequest {
    pub player_name: String,
    pub bot: String,
    /// The open seat it takes (the lowest, when left out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seat: Option<u32>,
}

/// `POST /api/games/{code}/bots`' answer. Who sits where arrives on the socket.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AddBotResult {
    pub seat: u32,
    pub status: String,
}

/// The answer of the lobby's other calls: where the game stands after one.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GameStatus {
    pub status: String,
    pub lobby: bool,
}

/// `POST /api/games`' answer.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CreateGameResponse {
    pub room_code: String,
    pub seat: u32,
}

/// `GET /api/games/{code}/selections`: what the seat on impulse may select next, after the
/// selections `made` (Bingle's ADR 0114).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Selections {
    pub seat: u32,
    /// The impulses played so far; a move sent with it is refused if the game has moved on.
    pub ply: u64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub made: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub options: Vec<LegalSelection>,
    /// The selections made are a whole move, to send as one.
    pub complete: bool,
    /// The selections made may be sent as they are, though more may follow (Blooms' second
    /// stone).
    #[serde(default)]
    pub may_end: bool,
    /// The position after the selections made, as this seat sees it: what the view then asks
    /// (a line to remove, after the move that made it). Not when nothing is made.
    #[serde(default)]
    pub state: Option<Position>,
}

/// One selection the seat may make, and the gestures that enter it.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct LegalSelection {
    pub notation: String,
    pub action: String,
    /// The position it places on or moves to, when it has one.
    #[serde(default)]
    pub to: Option<String>,
    /// The position it moves from, and the kind of piece it places, when it says.
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub piece: Option<String>,
    /// Where what follows the move goes, in a move of two parts (an arrow shot from where a
    /// piece landed).
    #[serde(default)]
    pub aim: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub gestures: Vec<Gesture>,
    /// The positions whose pieces it takes off the board (API 1.10): absent for none, and in a
    /// game with secrets or for a selection that rolls or draws.
    #[serde(default, deserialize_with = "null_as_default")]
    pub captures: Vec<String>,
}

/// One thing a player does to enter a selection: a click on a position (`pos`), a choice of
/// piece (`piece`), of a count (`count`) or of an action (`action`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub struct Gesture {
    pub kind: String,
    pub value: String,
}

impl Gesture {
    pub fn pos(id: &str) -> Self {
        Gesture {
            kind: "pos".into(),
            value: id.into(),
        }
    }

    /// A choice of the kind of piece `id`.
    pub fn piece(id: &str) -> Self {
        Gesture {
            kind: "piece".into(),
            value: id.into(),
        }
    }

    /// A click on the board, rather than a choice from a list.
    pub fn is_pos(&self) -> bool {
        self.kind == "pos"
    }
}

/// `POST /api/games/{code}/moves`' body: the whole move, its selections' notations joined by
/// `; `, made against `ply`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MoveRequest {
    pub player_name: String,
    pub notation: String,
    pub ply: u64,
}

/// `POST /api/games/{code}/moves`' answer. The moves themselves come on the game's socket.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MoveResponse {
    pub state: GameState,
}

/// Where a game stands: the position and whose impulse it is, or how it ended. Read from
/// `GET /api/games/{code}/state`, and the same fields come with every `move` and `state` event.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct GameState {
    /// The position. Each game has its own document; the client reads only its cells.
    #[serde(default, deserialize_with = "null_as_default")]
    pub state: Position,
    pub status: String,
    #[serde(default)]
    pub impulse_seat: Option<u32>,
    /// Selections played so far. A `move` event's `seq` is this after it.
    #[serde(default)]
    pub move_seq: u64,
    /// Impulses played so far.
    #[serde(default)]
    pub ply: u64,
    #[serde(default)]
    pub winner_seat: Option<u32>,
    /// Seats that share a draw.
    #[serde(default, deserialize_with = "null_as_default")]
    pub drawn_seats: Vec<u32>,
    /// Every seat lost.
    #[serde(default)]
    pub all_lost: bool,
    /// Why it ended, as a word ("connection", "resign").
    #[serde(default)]
    pub end_reason: String,
    /// The clock after the change, in a game played with one (events carry it; the state
    /// endpoint does not).
    #[serde(default)]
    pub clock: Option<Clock>,
    /// As in [`Game`]: when the seat on impulse runs out of time, and the server's time then.
    #[serde(default)]
    pub impulse_deadline: String,
    #[serde(default)]
    pub server_time: String,
}

/// The full position document's cells: each position's stack, bottom first.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Position {
    #[serde(default, deserialize_with = "null_as_default")]
    pub cells: BTreeMap<String, Vec<PieceOnBoard>>,
    /// What each seat holds off the board, by the seat's colour word: a number for each pool.
    /// Kept as it came, since each game has its own; read with [`Position::count`].
    #[serde(default)]
    pub reserves: serde_json::Value,
    /// A game whose rules are written in Go keeps more here, with a view of it to draw from
    /// (`ext.view`: each seat's counters, the walls between positions). Kept as it came.
    #[serde(default)]
    pub ext: serde_json::Value,
    /// A card game's cards, zone by zone, as this client may see them.
    #[serde(default)]
    pub cards: CardTable,
}

/// The cards of a game in progress: each zone's, by its name (a zone's id; `id:seat` for a
/// seat's own).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CardTable {
    #[serde(default, deserialize_with = "null_as_default")]
    pub zones: BTreeMap<String, CardZone>,
}

/// A zone's cards in order, the bottom one first.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CardZone {
    #[serde(default, deserialize_with = "null_as_default")]
    pub cards: Vec<CardOnTable>,
}

/// One card where it lies: its id, empty for one whose face this client may not see, and
/// whether it lies face up.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct CardOnTable {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub up: bool,
}

impl CardOnTable {
    /// Whether its face is shown: it lies face up and this client may see it.
    pub fn shown(&self) -> bool {
        self.up && !self.id.is_empty()
    }
}

impl Position {
    /// How many the seat of colour word `seat` has in its pool `pool`; 0 when it has none.
    pub fn count(&self, seat: &str, pool: &str) -> u32 {
        self.reserves
            .get(seat)
            .and_then(|r| r.get(pool))
            .and_then(serde_json::Value::as_u64)
            .map_or(0, |n| n.min(u32::MAX as u64) as u32)
    }

    /// Seat `seat`'s counter `id` in the view, if it has one (Fendo's pieces left in stock).
    pub fn view_counter(&self, seat: u32, id: &str) -> Option<u32> {
        self.ext["view"]["seats"][seat.to_string()]["counters"]
            .as_array()?
            .iter()
            .find(|c| c["id"] == id)?["value"]
            .as_u64()
            .map(|n| n.min(u32::MAX as u64) as u32)
    }

    /// The most seat `seat`'s counter `id` in the view may reach, if it says (the rings a win
    /// wants removed).
    pub fn view_counter_max(&self, seat: u32, id: &str) -> Option<u32> {
        self.ext["view"]["seats"][seat.to_string()]["counters"]
            .as_array()?
            .iter()
            .find(|c| c["id"] == id)?["max"]
            .as_u64()
            .map(|n| n.min(u32::MAX as u64) as u32)
    }

    /// What the view says the seat on impulse is asked to choose, in words ("Choose a Court."):
    /// its `impulse`, else its `prompt` (the same, with the seat's name), as Bingle's page shows
    /// it. Any game whose rules are written in Go may say; others do not.
    pub fn impulse_words(&self) -> Option<&str> {
        let view = &self.ext["view"];
        ["impulse", "prompt"]
            .into_iter()
            .filter_map(|key| view[key].as_str())
            .map(str::trim)
            .find(|words| !words.is_empty())
    }

    /// The positions the view tints, each with the key of the game's legend it is tinted by
    /// (Courtfall's terrain).
    pub fn tints(&self) -> BTreeMap<String, String> {
        self.ext["view"]["tints"]
            .as_object()
            .map(|tints| {
                tints
                    .iter()
                    .filter_map(|(at, key)| Some((at.clone(), key.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A count the view keeps of each of several things, by its name in the view (a draft's
    /// tiles left, by kind); empty when it has none.
    pub fn view_counts(&self, name: &str) -> BTreeMap<String, u32> {
        self.ext["view"][name]
            .as_object()
            .map(|counts| {
                counts
                    .iter()
                    .filter_map(|(kind, n)| {
                        Some((kind.clone(), n.as_u64()?.min(u32::MAX as u64) as u32))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Each zone's cards, bottom first, by the zone's name: the position's own (a declared
    /// game's), else the view's table of them (a game whose rules are written in Go), where a
    /// zone of which nothing may be seen gives only its count, as that many cards face down.
    pub fn card_zones(&self) -> BTreeMap<String, Vec<CardOnTable>> {
        if !self.cards.zones.is_empty() {
            return self
                .cards
                .zones
                .iter()
                .map(|(zone, z)| (zone.clone(), z.cards.clone()))
                .collect();
        }
        let Some(table) = self.ext["view"]["table"].as_array() else {
            return BTreeMap::new();
        };
        table
            .iter()
            .filter_map(|z| {
                let id = z["id"].as_str()?;
                let name = match z["owner"].as_u64().filter(|o| *o > 0) {
                    Some(owner) => format!("{id}:{owner}"),
                    None => id.to_string(),
                };
                let cards: Vec<CardOnTable> = match z["cards"].as_array() {
                    Some(cards) => cards
                        .iter()
                        .map(|c| serde_json::from_value(c.clone()).unwrap_or_default())
                        .collect(),
                    None => {
                        let count = z["count"].as_u64().unwrap_or(0).min(1000) as usize;
                        vec![CardOnTable::default(); count]
                    }
                };
                Some((name, cards))
            })
            .collect()
    }

    /// A list of words the view keeps by its name (a patience's suits, each foundation's in
    /// turn); empty when it has none.
    pub fn view_words(&self, name: &str) -> Vec<&str> {
        self.ext["view"][name]
            .as_array()
            .map(|all| all.iter().filter_map(serde_json::Value::as_str).collect())
            .unwrap_or_default()
    }

    /// The choices the view says are drawn on the table rather than as buttons, as the start of
    /// their values (a patience's `from-` and `to-`, and its stock's `draw`).
    pub fn panel_actions(&self) -> Vec<&str> {
        self.view_words("panel_actions")
    }

    /// The walls the view says stand between neighbouring positions (Fendo's fences), each by
    /// its two positions.
    pub fn walls(&self) -> Vec<(String, String)> {
        self.ext["view"]["walls"]
            .as_array()
            .map(|walls| {
                walls
                    .iter()
                    .filter_map(|w| Some((w[0].as_str()?.to_string(), w[1].as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PieceOnBoard {
    /// The piece's type.
    pub piece: String,
    /// The colour word of the seat it belongs to.
    pub color: String,
}

/// A message on `GET /ws/{code}`, of the kinds the client acts on.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The first message: the selections played when the socket subscribed.
    Joined { move_seq: u64 },
    /// One selection was played; the game now stands as `state` says.
    Move { seq: u64, state: GameState },
    /// Something other than a move changed the game (a seat taken, an undo, a resignation).
    /// `seats` are who sits where now; they never say which is this client's.
    State {
        reason: String,
        state: GameState,
        seats: Vec<GameSeat>,
        /// As in [`Game`]: it waits for its host's start, and the host's seat.
        lobby: bool,
        host_seat: Option<u32>,
    },
    /// A kind this client skips (chat, connections, voice, and any added later).
    Other(String),
}

impl Event {
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        #[derive(Deserialize)]
        struct Envelope {
            #[serde(rename = "type")]
            kind: String,
            #[serde(default)]
            data: serde_json::Value,
        }
        #[derive(Deserialize)]
        struct JoinedData {
            move_seq: u64,
        }
        #[derive(Deserialize)]
        struct MoveData {
            seq: u64,
            #[serde(flatten)]
            state: GameState,
        }
        #[derive(Deserialize)]
        struct StateData {
            reason: String,
            #[serde(default, deserialize_with = "null_as_default")]
            seats: Vec<GameSeat>,
            #[serde(default)]
            lobby: bool,
            #[serde(default)]
            host_seat: Option<u32>,
            #[serde(flatten)]
            state: GameState,
        }
        let env: Envelope = serde_json::from_str(text)?;
        Ok(match env.kind.as_str() {
            "joined" => Event::Joined {
                move_seq: serde_json::from_value::<JoinedData>(env.data)?.move_seq,
            },
            "move" => {
                let d: MoveData = serde_json::from_value(env.data)?;
                // A move event says where the game stands after it, but not as `move_seq`.
                let mut state = d.state;
                state.move_seq = d.seq;
                Event::Move { seq: d.seq, state }
            }
            "state" => {
                let d: StateData = serde_json::from_value(env.data)?;
                Event::State {
                    reason: d.reason,
                    state: d.state,
                    seats: d.seats,
                    lobby: d.lobby,
                    host_seat: d.host_seat,
                }
            }
            other => Event::Other(other.to_string()),
        })
    }
}

/// The body of every error answer.
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}

fn null_as_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(de)?.unwrap_or_default())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.trim().split('.');
        let mut next = || parts.next()?.parse::<u32>().ok();
        let v = Version {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        parts.next().is_none().then_some(v)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Whether a server's API can be used by this client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compatibility {
    Compatible,
    /// The server is older: an earlier major version, or the same major without the additions this
    /// client uses. `None` when it reports no API version at all.
    ServerTooOld(Option<String>),
    /// The server's API has moved on to a major version this client does not know.
    ClientTooOld(String),
    /// What it reports is not a version.
    Unreadable(String),
}

/// Same major, and at least the minor this client was built for (it may use what that minor
/// added). A newer minor or patch only adds what a tolerant client can ignore.
pub fn compatibility(server: Option<&str>) -> Compatibility {
    let Some(reported) = server else {
        return Compatibility::ServerTooOld(None);
    };
    let Some(v) = Version::parse(reported) else {
        return Compatibility::Unreadable(reported.to_string());
    };
    if v.major > API_VERSION.major {
        Compatibility::ClientTooOld(reported.to_string())
    } else if (v.major, v.minor) < (API_VERSION.major, API_VERSION.minor) {
        Compatibility::ServerTooOld(Some(reported.to_string()))
    } else {
        Compatibility::Compatible
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::{Map, Value, json};

    #[test]
    fn versions_parse_strictly() {
        assert_eq!(
            Version::parse("1.2.3"),
            Some(Version {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("v1.2.3"), None);
    }

    #[test]
    fn a_view_says_in_words_what_the_seat_on_impulse_is_asked() {
        let read = |json: &str| serde_json::from_str::<Position>(json).unwrap();
        let both = read(
            r#"{"cells":{},"ext":{"view":{"prompt":"Dark: Choose a Court.","impulse":"Choose a Court."}}}"#,
        );
        assert_eq!(both.impulse_words(), Some("Choose a Court."));
        let prompt =
            read(r#"{"cells":{},"ext":{"view":{"prompt":"Dark: Choose a Court.","impulse":" "}}}"#);
        assert_eq!(prompt.impulse_words(), Some("Dark: Choose a Court."));
        assert_eq!(read(r#"{"cells":{}}"#).impulse_words(), None);
        assert_eq!(
            read(r#"{"cells":{},"ext":{"view":{"impulse":3}}}"#).impulse_words(),
            None
        );
    }

    #[test]
    fn a_view_says_which_positions_it_tints_and_counts_what_is_left() {
        let position: Position = serde_json::from_str(r#"{"cells":{}}"#).unwrap();
        assert!(position.tints().is_empty());
        assert!(position.view_counts("tiles_left").is_empty());
        let position: Position = serde_json::from_str(
            r#"{"cells":{},"ext":{"view":{"tints":{"d4":"eye","c3":"green","x":1},
            "tiles_left":{"green":3,"blue":0,"red":"many"}}}}"#,
        )
        .unwrap();
        let tints = position.tints();
        assert_eq!(tints.len(), 2);
        assert_eq!(tints["d4"], "eye");
        assert_eq!(tints["c3"], "green");
        let left = position.view_counts("tiles_left");
        assert_eq!(left.len(), 2);
        assert_eq!((left["green"], left["blue"]), (3, 0));
    }

    #[test]
    fn a_view_says_each_seats_counters_and_the_walls_between_positions() {
        let position: Position = serde_json::from_str(
            r#"{"cells":{},"ext":{"view":{"walls":[["d3","e3"],["a1"]],
            "seats":{"1":{"counters":[{"id":"territory","value":0},{"id":"stock","value":6}]},
            "2":{"counters":[{"id":"stock","value":5}]}}}}}"#,
        )
        .unwrap();
        assert_eq!(position.view_counter(1, "stock"), Some(6));
        assert_eq!(position.view_counter(2, "stock"), Some(5));
        assert_eq!(position.view_counter(3, "stock"), None);
        assert_eq!(position.view_counter(2, "territory"), None);
        assert_eq!(position.walls(), [("d3".to_string(), "e3".to_string())]);
        assert!(Position::default().walls().is_empty());
    }

    #[test]
    fn a_server_must_share_the_major_and_have_at_least_the_minor() {
        use Compatibility::*;
        assert_eq!(compatibility(Some("1.10.0")), Compatible);
        assert_eq!(compatibility(Some("1.11.2")), Compatible);
        assert_eq!(
            compatibility(Some("1.9.2")),
            ServerTooOld(Some("1.9.2".into())),
            "1.9 does not say what a selection captures"
        );
        assert_eq!(
            compatibility(Some("1.7.3")),
            ServerTooOld(Some("1.7.3".into())),
            "1.7 does not mark private games"
        );
        assert_eq!(
            compatibility(Some("1.0.9")),
            ServerTooOld(Some("1.0.9".into())),
            "1.0 has no bot matches"
        );
        assert_eq!(
            compatibility(Some("1.1.4")),
            ServerTooOld(Some("1.1.4".into())),
            "1.1 has no goals"
        );
        assert_eq!(
            compatibility(Some("1.3.2")),
            ServerTooOld(Some("1.3.2".into())),
            "1.3's schema has no lobby"
        );
        assert_eq!(
            compatibility(Some("1.4.1")),
            ServerTooOld(Some("1.4.1".into())),
            "1.4 has no sign-in for a client that is not a browser"
        );
        assert_eq!(
            compatibility(Some("1.5.0")),
            ServerTooOld(Some("1.5.0".into())),
            "1.5's schema has no calls"
        );
        assert_eq!(compatibility(Some("2.0.0")), ClientTooOld("2.0.0".into()));
        assert_eq!(
            compatibility(Some("0.9.0")),
            ServerTooOld(Some("0.9.0".into()))
        );
        assert_eq!(compatibility(None), ServerTooOld(None));
        assert_eq!(compatibility(Some("soon")), Unreadable("soon".into()));
    }

    #[test]
    fn unknown_fields_and_nulls_are_tolerated() {
        let games: OpenGames = serde_json::from_value(json!({
            "games": [{
                "room_code": "N5WUVW", "game_def_id": "hex-7", "name": "Hex (7×7)",
                "status": "active", "seats": 2, "open_seats": 0, "players": null,
                "impulse_seat": null, "move_seq": 0, "something_new": {"a": 1}
            }]
        }))
        .unwrap();
        assert_eq!(games.games[0].players, vec![]);
        assert_eq!(games.games[0].impulse_seat, None);
        let none: OpenGames = serde_json::from_value(json!({"games": null})).unwrap();
        assert!(none.games.is_empty());
    }

    // --- Against the published schema ---

    #[test]
    fn events_are_read_by_their_type_and_unknown_ones_skipped() {
        let joined = Event::parse(r#"{"type":"joined","data":{"role":"spectator","status":"active","connected":null,"move_seq":4}}"#).unwrap();
        assert_eq!(joined, Event::Joined { move_seq: 4 });
        let mv = Event::parse(
            r#"{"type":"move","data":{"seq":5,"seat":1,"color":"red","from":"","to":"c3","captured":null,
            "impulse_seat":2,"game_ended":false,"winner_seat":null,"status":"active","board_state":{},"ply":5,
            "state":{"v":2,"cells":{"c3":[{"piece":"stone","color":"red"}]},"ply":5}}}"#,
        )
        .unwrap();
        let Event::Move { seq, state } = mv else {
            panic!("{mv:?}")
        };
        assert_eq!(seq, 5);
        assert_eq!(state.move_seq, 5, "a move's seq is where the game stands");
        assert_eq!(state.impulse_seat, Some(2));
        assert_eq!(state.state.cells["c3"][0].color, "red");
        let seated = Event::parse(
            r#"{"type":"state","data":{"reason":"seat","status":"waiting","impulse_seat":null,
            "move_seq":0,"ply":0,"winner_seat":null,"board_state":"","state":null,"draw_offer_seat":null,
            "lobby":true,"host_seat":2,"seats":[{"seat":2,"color":"blue","name":"Ana"}]}}"#,
        )
        .unwrap();
        let Event::State {
            lobby,
            host_seat,
            seats,
            ..
        } = seated
        else {
            panic!("{seated:?}")
        };
        assert!(lobby);
        assert_eq!(host_seat, Some(2));
        assert_eq!(seats[0].name, "Ana");
        assert_eq!(
            Event::parse(r#"{"type":"chat","data":{"text":"hi"}}"#).unwrap(),
            Event::Other("chat".into())
        );
        assert!(Event::parse("not json").is_err());
    }

    const SCHEMA: &str = include_str!("../../schema/bingle-api-1.10.0.schema.json");

    fn schema() -> Value {
        serde_json::from_str(SCHEMA).expect("the schema is JSON")
    }

    /// A value that satisfies `node`: with `full`, every property the schema lists is present and
    /// not null where it may be something else; without, only the required ones, and null wherever
    /// null is allowed. A type that reads both is one whose required and optional fields, and
    /// their types, agree with the schema.
    fn instance(root: &Value, node: &Value, full: bool, depth: usize) -> Value {
        assert!(depth < 20, "the schema nests too deep to follow");
        if let Some(r) = node.get("$ref").and_then(Value::as_str) {
            let name = r.strip_prefix("#/$defs/").expect("a local $ref");
            return instance(root, &root["$defs"][name], full, depth + 1);
        }
        for key in ["allOf", "anyOf", "oneOf"] {
            if let Some(options) = node.get(key).and_then(Value::as_array) {
                let pick = options
                    .iter()
                    .find(|o| o.get("type") != Some(&json!("null")))
                    .unwrap_or(&options[0]);
                return instance(root, pick, full, depth + 1);
            }
        }
        if let Some(c) = node.get("const") {
            return c.clone();
        }
        let types: Vec<&str> = match node.get("type") {
            Some(Value::String(t)) => vec![t.as_str()],
            Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !full && types.contains(&"null") {
            return Value::Null;
        }
        match types.iter().find(|t| **t != "null").copied() {
            Some("string") => json!("x"),
            Some("integer") => json!(1),
            Some("number") => json!(1.5),
            Some("boolean") => json!(true),
            Some("array") => match node.get("items") {
                // A type that lists its own kind (a move's selections are moves) ends somewhere.
                Some(items) if full && depth < 10 => {
                    json!([instance(root, items, full, depth + 1)])
                }
                _ => json!([]),
            },
            Some("object") | None if node.get("properties").is_some() => {
                let required: Vec<&str> = node["required"]
                    .as_array()
                    .map(|r| r.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                let mut out = Map::new();
                for (name, prop) in node["properties"].as_object().unwrap() {
                    if full || required.contains(&name.as_str()) {
                        out.insert(name.clone(), instance(root, prop, full, depth + 1));
                    }
                }
                Value::Object(out)
            }
            Some("object") => json!({}),
            _ => Value::Null,
        }
    }

    fn response(root: &Value, endpoint: &str) -> Value {
        root["x-endpoints"]
            .get(endpoint)
            .unwrap_or_else(|| panic!("the schema has no {endpoint}"))["response"]
            .clone()
    }

    fn reads<T: DeserializeOwned>(endpoint: &str) {
        let root = schema();
        let node = response(&root, endpoint);
        for full in [false, true] {
            let value = instance(&root, &node, full, 0);
            if let Err(e) = serde_json::from_value::<T>(value.clone()) {
                panic!(
                    "{} cannot read {endpoint} ({}): {e}\n{value:#}",
                    std::any::type_name::<T>(),
                    if full {
                        "every field"
                    } else {
                        "required fields only"
                    },
                );
            }
        }
    }

    #[test]
    fn the_view_says_what_is_drawn_on_the_table_and_lists_words_by_name() {
        let read = |json: &str| -> Position { serde_json::from_str(json).unwrap() };
        let p = read(
            r#"{"cells":{},"ext":{"view":{"panel_actions":["from-","draw"],
            "suits":["hearts","spades",3]}}}"#,
        );
        assert_eq!(p.panel_actions(), ["from-", "draw"]);
        assert_eq!(p.view_words("suits"), ["hearts", "spades"], "words only");
        assert!(read(r#"{"cells":{}}"#).panel_actions().is_empty());
        assert!(p.view_words("prompt").is_empty());
    }

    fn read_empty() -> Position {
        serde_json::from_str(r#"{"cells":{}}"#).unwrap()
    }

    #[test]
    fn a_card_game_reads_its_kinds_of_card_and_the_cards_where_they_lie() {
        let cards: Cards = serde_json::from_str(
            r##"{"suits":{"hearts":{"label":"Hearts","symbol":"♥","color":"#c00"}},
            "suit_order":["hearts"],"back":{},"zones":null,
            "types":[{"id":"hearts-A","suit":"hearts","rank":"A","label":"A","value":1}]}"##,
        )
        .unwrap();
        assert_eq!(cards.type_of("hearts-A").unwrap().rank, "A");
        assert_eq!(
            cards.type_of("hearts-A#1").unwrap().id,
            "hearts-A",
            "a second deck's copy"
        );
        assert!(cards.type_of("spades-A").is_none());
        assert!(cards.zones.is_empty());
        let p: Position = serde_json::from_str(
            r#"{"cells":{},"cards":{"zones":{"t1":{"cards":[{},{"id":"hearts-A","up":true}]},
            "f1":{}}}}"#,
        )
        .unwrap();
        assert_eq!(
            p.card_zones()["t1"],
            p.cards.zones["t1"].cards,
            "the position's own"
        );
        let t1 = &p.cards.zones["t1"].cards;
        assert!(!t1[0].shown() && t1[1].shown());
        assert!(p.cards.zones["f1"].cards.is_empty());
        assert!(
            !CardOnTable {
                id: "x".into(),
                up: false
            }
            .shown(),
            "face down"
        );

        // A game whose rules are written in Go: the view's table, a hidden zone by its count.
        let p: Position = serde_json::from_str(
            r#"{"cells":{},"ext":{"view":{"table":[
            {"id":"ball1","count":3,"cards":[{},{},{"id":"spades-7","up":true}]},
            {"id":"stock","count":2},{"id":"hand","owner":2,"count":1,"cards":[{}]},
            {"count":4}]}}}"#,
        )
        .unwrap();
        let zones = p.card_zones();
        assert_eq!(zones.len(), 3, "one with no id is left out");
        assert!(zones["ball1"][2].shown() && !zones["ball1"][0].shown());
        assert_eq!(zones["stock"], vec![CardOnTable::default(); 2]);
        assert_eq!(zones["hand:2"].len(), 1, "a seat's own zone, by its owner");
        assert!(read_empty().card_zones().is_empty());
    }

    #[test]
    fn the_schema_is_the_one_for_this_api_version() {
        assert_eq!(
            schema()["x-api-version"].as_str(),
            Some(API_VERSION.to_string().as_str())
        );
    }

    #[test]
    fn the_types_read_what_the_schema_describes() {
        reads::<Index>("GET /api");
        reads::<GuestKey>("POST /api/guest");
        reads::<Vec<GameDef>>("GET /api/game-defs");
        reads::<OpenGames>("GET /api/games");
        reads::<GameInfo>("GET /api/game-defs/{id}");
        reads::<Vec<BotInfo>>("GET /api/bots");
        reads::<BotMatchResponse>("POST /api/bot-matches");
        reads::<Game>("GET /api/games/{code}");
        reads::<GameState>("GET /api/games/{code}/state");
        reads::<CreateGameResponse>("POST /api/games");
        reads::<Selections>("GET /api/games/{code}/selections");
        reads::<MoveResponse>("POST /api/games/{code}/moves");
        reads::<JoinResponse>("POST /api/games/{code}/join");
        reads::<GameState>("POST /api/games/{code}/resign");
        reads::<AddBotResult>("POST /api/games/{code}/bots");
        reads::<SessionTokenResponse>("POST /auth/login");
        reads::<SessionTokenResponse>("POST /auth/second");
        reads::<Me>("GET /auth/me");
        reads::<LogoutResponse>("POST /auth/logout");
        reads::<FriendsList>("GET /api/friends");
        reads::<FriendSettings>("GET /api/friends/settings");
        reads::<FriendSettings>("PUT /api/friends/settings");
        reads::<HereResponse>("POST /api/here");
        reads::<DirectMessage>("POST /api/dm");
        reads::<DirectHistory>("POST /api/dm/history");
        reads::<OkResponse>("POST /api/dm/call");
        reads::<OkResponse>("POST /api/dm/signal");
        reads::<RtcConfig>("GET /api/rtc/config");
        reads::<VoicePeers>("GET /api/games/{code}/voice");
        reads::<VoicePeers>("POST /api/games/{code}/voice");
        reads::<OkResponse>("POST /api/games/{code}/voice/signal");
        for call in [
            "request",
            "answer",
            "remove",
            "invite",
            "invite-answer",
            "dismiss",
        ] {
            reads::<OkResponse>(&format!("POST /api/friends/{call}"));
        }
        for call in ["start", "seat", "leave", "remove"] {
            reads::<GameStatus>(&format!("POST /api/games/{{code}}/{call}"));
        }
        let root = schema();
        for full in [false, true] {
            serde_json::from_value::<ErrorBody>(instance(&root, &root["x-error"], full, 0))
                .expect("the error body");
        }
    }

    #[test]
    fn the_events_the_client_acts_on_read_as_the_schema_describes_them() {
        let root = schema();
        let arms = root["$defs"]["WebSocketEvent"]["oneOf"]
            .as_array()
            .expect("the events are a oneOf");
        let mut acted_on = Vec::new();
        for arm in arms {
            let kind = arm["properties"]["type"]["const"].as_str().unwrap();
            for full in [false, true] {
                let text = instance(&root, arm, full, 0).to_string();
                let event = Event::parse(&text)
                    .unwrap_or_else(|e| panic!("cannot read a {kind} event ({e}): {text}"));
                if !matches!(event, Event::Other(_)) {
                    acted_on.push(kind);
                }
            }
        }
        acted_on.dedup();
        assert_eq!(acted_on, ["joined", "move", "state"]);
    }

    /// Every field `sent` has is in the schema of `endpoint`'s request, and every field the schema
    /// requires is sent.
    fn sends(endpoint: &str, sent: Value) {
        let root = schema();
        let node = &root["x-endpoints"][endpoint]["request"];
        let node = match node.get("$ref").and_then(Value::as_str) {
            Some(r) => &root["$defs"][r.strip_prefix("#/$defs/").unwrap()],
            None => node,
        };
        let props = node["properties"].as_object().unwrap();
        for name in node["required"].as_array().into_iter().flatten() {
            assert!(
                sent.get(name.as_str().unwrap()).is_some(),
                "{endpoint}: {name} missing"
            );
        }
        for name in sent.as_object().unwrap().keys() {
            assert!(
                props.contains_key(name),
                "{endpoint}: {name} is not in the schema"
            );
        }
    }

    #[test]
    fn the_requests_have_what_the_schema_requires() {
        fn json(v: impl Serialize) -> Value {
            serde_json::to_value(v).unwrap()
        }
        sends(
            "POST /api/bot-matches",
            json(BotMatchRequest {
                game_def_id: "g".into(),
                bot_a: "a".into(),
                bot_b: "b".into(),
            }),
        );
        sends(
            "POST /api/games",
            json(CreateGameRequest::new("g", "Ana", Some("mcts".into()))),
        );
        sends(
            "POST /api/games",
            json(CreateGameRequest::new("g", "Ana", None)),
        );
        sends(
            "POST /api/games",
            json(CreateGameRequest {
                seat: Some(2),
                seat_bots: [("1".to_string(), "random".to_string())].into(),
                clock_seconds: Some(300),
                clock_increment_seconds: Some(3),
                move_time_seconds: Some(60),
                ..CreateGameRequest::new("g", "Ana", None)
            }),
        );
        sends(
            "POST /api/games/{code}/join",
            json(JoinRequest {
                player_name: "Ana".into(),
                role: "player".into(),
            }),
        );
        sends(
            "POST /api/games/{code}/resign",
            json(PlayerRequest {
                player_name: "Ana".into(),
            }),
        );
        for call in ["start", "leave"] {
            sends(
                &format!("POST /api/games/{{code}}/{call}"),
                json(PlayerRequest {
                    player_name: "Ana".into(),
                }),
            );
        }
        for call in ["seat", "remove"] {
            sends(
                &format!("POST /api/games/{{code}}/{call}"),
                json(SeatRequest {
                    player_name: "Ana".into(),
                    seat: 2,
                }),
            );
        }
        for seat in [None, Some(2)] {
            sends(
                "POST /api/games/{code}/bots",
                json(AddBotRequest {
                    player_name: "Ana".into(),
                    bot: "mcts".into(),
                    seat,
                }),
            );
        }
        sends(
            "POST /api/games",
            json(CreateGameRequest {
                lobby: true,
                reserved_seats: [("2".to_string(), "bob".to_string())].into(),
                ..CreateGameRequest::new("g", "Ana", None)
            }),
        );
        for body in [
            FriendBody {
                handle: "bo".into(),
                ..Default::default()
            },
            FriendBody {
                handle: "bo".into(),
                accept: Some(true),
                ..Default::default()
            },
            FriendBody {
                handle: "bo".into(),
                room: "AB12CD".into(),
                ..Default::default()
            },
            FriendBody {
                room: "AB12CD".into(),
                accept: Some(false),
                ..Default::default()
            },
        ] {
            sends("POST /api/friends/request", json(body));
        }
        sends(
            "PUT /api/friends/settings",
            json(FriendSettings {
                mail_requests: false,
                show_presence: true,
                accept_invites: true,
            }),
        );
        sends("POST /api/here", json(HereRequest { here: false }));
        sends(
            "POST /api/dm",
            json(DirectMessageRequest {
                to: "addr".into(),
                text: "hello".into(),
            }),
        );
        for body in [
            DirectCallRequest {
                to: "addr".into(),
                join: true,
            },
            DirectCallRequest {
                to: String::new(),
                join: false,
            },
        ] {
            sends("POST /api/dm/call", json(body));
        }
        sends(
            "POST /api/dm/signal",
            json(DirectSignalRequest {
                to: "addr".into(),
                kind: "retry".into(),
                data: serde_json::json!({}),
            }),
        );
        for active in [true, false] {
            sends(
                "POST /api/games/{code}/voice",
                json(VoiceRequest {
                    player_name: "Ana".into(),
                    active,
                }),
            );
        }
        sends(
            "POST /api/games/{code}/voice/signal",
            json(VoiceSignalRequest {
                player_name: "Ana".into(),
                to: 2,
                kind: "offer".into(),
                data: serde_json::json!({"type": "offer", "sdp": "v=0"}),
            }),
        );
        sends(
            "POST /api/dm/history",
            json(DirectHistoryRequest {
                with: "addr".into(),
            }),
        );
        sends(
            "POST /auth/login",
            json(LoginRequest {
                handle: "ana".into(),
                password: "secret".into(),
                token: true,
            }),
        );
        for (code, recovery_code) in [("123456", ""), ("", "abcd-efgh")] {
            sends(
                "POST /auth/second",
                json(SecondRequest {
                    pending: "p".into(),
                    code: code.into(),
                    recovery_code: recovery_code.into(),
                }),
            );
        }
        sends(
            "POST /api/games/{code}/moves",
            json(MoveRequest {
                player_name: "Ana".into(),
                notation: "d4".into(),
                ply: 3,
            }),
        );
    }

    #[test]
    fn the_voice_events_read_as_the_schema_describes_them() {
        let root = schema();
        let arms = root["$defs"]["WebSocketEvent"]["oneOf"].as_array().unwrap();
        let mut acted_on = Vec::new();
        for arm in arms {
            let kind = arm["properties"]["type"]["const"].as_str().unwrap();
            for full in [false, true] {
                let text = instance(&root, arm, full, 0).to_string();
                let event = VoiceEvent::parse(&text)
                    .unwrap_or_else(|e| panic!("cannot read a {kind} event ({e}): {text}"));
                if !matches!(event, VoiceEvent::Other(_)) {
                    acted_on.push(kind);
                }
            }
        }
        acted_on.dedup();
        assert_eq!(acted_on, ["joined", "voice", "rtc"]);
        assert_eq!(
            VoiceEvent::parse(r#"{"type":"voice","data":{"kind":"leave","seat":2,"name":"Bo"}}"#)
                .unwrap(),
            VoiceEvent::Voice {
                join: false,
                seat: 2,
                name: "Bo".into()
            }
        );
    }

    #[test]
    fn the_account_events_the_client_acts_on_read_as_the_schema_describes_them() {
        let root = schema();
        let arms = root["$defs"]["DirectWebSocketEvent"]["oneOf"]
            .as_array()
            .expect("the events are a oneOf");
        let mut acted_on = Vec::new();
        for arm in arms {
            let kind = arm["properties"]["type"]["const"].as_str().unwrap();
            for full in [false, true] {
                let text = instance(&root, arm, full, 0).to_string();
                let event = AccountEvent::parse(&text)
                    .unwrap_or_else(|e| panic!("cannot read a {kind} event ({e}): {text}"));
                if !matches!(event, AccountEvent::Other(_)) {
                    acted_on.push(kind);
                }
            }
        }
        acted_on.dedup();
        assert_eq!(acted_on, ["ready", "dm", "dm-call", "dm-rtc", "friends"]);
    }

    #[test]
    fn selections_read_as_the_server_sends_them() {
        let s: Selections = serde_json::from_str(
            r#"{"seat":2,"ply":1,"made":[],"options":[
            {"notation":"a1","action":"place","piece":"stone","to":"a1",
             "gestures":[{"kind":"piece","value":"stone"},{"kind":"pos","value":"a1"}]},
            {"notation":"swap","action":"swap","gestures":[{"kind":"action","value":"swap"}]},
            {"notation":"c3^e5x","action":"jump","from":"c3","to":"e5","captures":["d4"],
             "gestures":[{"kind":"pos","value":"c3"},{"kind":"pos","value":"e5"}]}],
            "complete":false}"#,
        )
        .unwrap();
        assert_eq!(s.ply, 1);
        assert_eq!(s.options[0].to.as_deref(), Some("a1"));
        assert!(s.options[0].captures.is_empty(), "absent: it takes nothing");
        assert_eq!(s.options[2].captures, ["d4"]);
        assert!(s.options[0].gestures[1].is_pos());
        assert_eq!(s.options[1].to, None);
        let done: Selections = serde_json::from_str(
            r#"{"seat":2,"ply":1,"made":["a1"],"options":[],"complete":true}"#,
        )
        .unwrap();
        assert!(done.complete);
        assert_eq!(s.state, None);
        // With the position after them, and what its view asks next.
        let more: Selections = serde_json::from_str(
            r#"{"seat":1,"ply":18,"made":["j7-e7"],"options":[],"complete":false,
            "state":{"v":2,"cells":{"e7":[{"piece":"ring","color":"white"}]},"ply":19,
            "ext":{"view":{"impulse":"Choose a line to remove.","tints":{"f7":"row"}}}}}"#,
        )
        .unwrap();
        let after = more.state.unwrap();
        assert_eq!(after.cells["e7"][0].piece, "ring");
        assert_eq!(after.impulse_words(), Some("Choose a line to remove."));
        assert_eq!(after.tints()["f7"], "row");
    }
}
