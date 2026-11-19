//! The parts of Bingle's API this client reads, as `serde` types, and the check that a server
//! speaks the API version the client was built for.
//!
//! The types are written by hand and hold only the fields the client uses; unknown fields are
//! ignored, which is what lets the server add fields in a minor version (Bingle's ADR 0134). A
//! test checks them against the JSON Schema Bingle publishes for [`API_VERSION`], kept in
//! `schema/`.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

/// The version of Bingle's API this client is written against: 1.1 added `POST /api/bot-matches`.
pub const API_VERSION: Version = Version {
    major: 1,
    minor: 1,
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GameInfo {
    pub id: String,
    pub name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub seats: Vec<SeatDef>,
    pub board: Board,
}

/// A seat of a game definition.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SeatDef {
    pub seat: u32,
    /// An identifier, not a colour to draw (Bingle's ADR 0124).
    pub color: String,
    /// What the seat is called, in words ("Red (top – bottom)").
    pub label: String,
}

/// The board as the rules see it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Board {
    /// "grid", "hex" or "explicit".
    pub kind: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub positions: Vec<BoardPosition>,
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
    #[serde(default, deserialize_with = "null_as_default")]
    pub seats: Vec<GameSeat>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GameSeat {
    pub seat: u32,
    pub color: String,
    /// Empty while the seat is free.
    #[serde(default)]
    pub name: String,
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
}

/// The full position document's cells: each position's stack, bottom first.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Position {
    #[serde(default, deserialize_with = "null_as_default")]
    pub cells: BTreeMap<String, Vec<PieceOnBoard>>,
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
    State { reason: String, state: GameState },
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
    fn a_server_must_share_the_major_and_have_at_least_the_minor() {
        use Compatibility::*;
        assert_eq!(compatibility(Some("1.1.0")), Compatible);
        assert_eq!(compatibility(Some("1.7.2")), Compatible);
        assert_eq!(
            compatibility(Some("1.0.9")),
            ServerTooOld(Some("1.0.9".into())),
            "1.0 has no bot matches"
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
        assert_eq!(
            Event::parse(r#"{"type":"chat","data":{"text":"hi"}}"#).unwrap(),
            Event::Other("chat".into())
        );
        assert!(Event::parse("not json").is_err());
    }

    const SCHEMA: &str = include_str!("../../schema/bingle-api-1.1.0.schema.json");

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
                Some(items) if full => json!([instance(root, items, full, depth + 1)]),
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

    #[test]
    fn a_bot_match_request_has_what_the_schema_requires() {
        let root = schema();
        let node = &root["x-endpoints"]["POST /api/bot-matches"]["request"];
        let node = match node.get("$ref").and_then(Value::as_str) {
            Some(r) => &root["$defs"][r.strip_prefix("#/$defs/").unwrap()],
            None => node,
        };
        let sent = serde_json::to_value(BotMatchRequest {
            game_def_id: "g".into(),
            bot_a: "a".into(),
            bot_b: "b".into(),
        })
        .unwrap();
        let props = node["properties"].as_object().unwrap();
        for name in node["required"].as_array().unwrap() {
            assert!(sent.get(name.as_str().unwrap()).is_some(), "{name} missing");
        }
        for name in sent.as_object().unwrap().keys() {
            assert!(props.contains_key(name), "{name} is not in the schema");
        }
    }
}
