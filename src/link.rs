//! A link that opens the client at a game (Bingle's ADR 0163): what a server's page hands the
//! system beside *Join* and *Watch*, which this client registers (`link_handler.rs`).
//!
//! ```text
//! bingle-3d://join?server=https%3A%2F%2Fgames.example.com&room=K7MQ2X&code=bgc_…
//! bingle-3d://watch?server=http%3A%2F%2F127.0.0.1%3A8080&room=K7MQ2X
//! ```
//!
//! **A link is hostile input**: any page can offer one, to any server, with anything in it.
//! [`Link::parse`] accepts exactly the contract's form and refuses the whole link for anything
//! else; nothing in a link ever becomes a command-line option or reaches a shell. What to do
//! with a link that parses, given what this client holds for its server, is [`decide`]. No Bevy
//! in it. **Never log a link or its code**: [`Link`]'s `Debug` leaves the code out.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

use crate::net::server_url::ServerUrl;

/// The scheme the client registers: its own name, which no other program's is.
pub const SCHEME: &str = "bingle-3d";

/// The longest link taken, in bytes (every character of one that parses is ASCII).
pub const MAX_LEN: usize = 1024;

/// What a link asks for, its host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// An open seat, or the one the player already holds, else watching: `--join`.
    Join,
    /// Watching: `--watch`.
    Watch,
}

/// A one-time code a link carries (`bgc_…` an account's, `bgj_…` a guest's): good once, for a
/// minute, with the link's server only. It is never shown, logged or written down.
#[derive(Clone, PartialEq, Eq)]
pub struct Code(String);

impl Code {
    /// The code itself, for the one request that spends it.
    pub fn secret(&self) -> &str {
        &self.0
    }

    /// Whom it would make this client: an account (`bgc_`) or a guest (`bgj_`).
    pub fn kind(&self) -> Kind {
        if self.0.starts_with("bgc_") {
            Kind::Account
        } else {
            Kind::Guest
        }
    }
}

impl fmt::Debug for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Code(…)")
    }
}

/// Whom a code would make this client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Account,
    Guest,
}

/// A link that parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub action: Action,
    /// The server the link names, by the client's own address rule too.
    pub server: ServerUrl,
    /// The room's code.
    pub room: String,
    pub code: Option<Code>,
}

/// Why a link was refused, in words for the player. None of them repeats what the link says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    TooLong,
    /// A character no link of the contract's has: a space, a control, a NUL, anything not ASCII.
    Character,
    Scheme,
    Action,
    UserInfo,
    Port,
    Path,
    Fragment,
    /// No query, an empty parameter or one without `=`.
    Query,
    Unknown,
    Repeated,
    MissingServer,
    MissingRoom,
    /// A `%` not followed by two hex digits, or bytes that are not UTF-8.
    Encoding,
    Server,
    Room,
    Code,
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            LinkError::TooLong => "it is longer than a link can be",
            LinkError::Character => "it has a character no link has",
            LinkError::Scheme => "it is not a bingle-3d link",
            LinkError::Action => "it asks for something other than join or watch",
            LinkError::UserInfo => "it has a user's name in it",
            LinkError::Port => "it has a port after its action",
            LinkError::Path => "it has a path after its action",
            LinkError::Fragment => "it has a fragment",
            LinkError::Query => "its parameters cannot be read",
            LinkError::Unknown => "it has a parameter a link does not have",
            LinkError::Repeated => "it gives a parameter twice",
            LinkError::MissingServer => "it names no server",
            LinkError::MissingRoom => "it names no room",
            LinkError::Encoding => "a value in it is not encoded as a link's are",
            LinkError::Server => "its server is not an address a link may name",
            LinkError::Room => "its room is not a room's code",
            LinkError::Code => "its code is not one",
        };
        f.write_str(why)
    }
}

impl std::error::Error for LinkError {}

impl Link {
    /// Reads a link, refusing the whole of it for anything the contract does not have.
    pub fn parse(input: &str) -> Result<Self, LinkError> {
        if input.len() > MAX_LEN {
            return Err(LinkError::TooLong);
        }
        // Printable ASCII only: no space, control, NUL or anything a second reader might take
        // otherwise. A value's other characters are percent-encoded.
        if !input.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
            return Err(LinkError::Character);
        }
        if input.contains('#') {
            return Err(LinkError::Fragment);
        }
        let rest = input
            .split_once("://")
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case(SCHEME))
            .map(|(_, rest)| rest)
            .ok_or(LinkError::Scheme)?;
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let (authority, rest) = rest.split_at(end);
        if authority.contains('@') {
            return Err(LinkError::UserInfo);
        }
        if authority.contains(':') {
            return Err(LinkError::Port);
        }
        let action = if authority.eq_ignore_ascii_case("join") {
            Action::Join
        } else if authority.eq_ignore_ascii_case("watch") {
            Action::Watch
        } else {
            return Err(LinkError::Action);
        };
        if rest.starts_with('/') {
            return Err(LinkError::Path);
        }
        let query = rest.strip_prefix('?').ok_or(LinkError::MissingServer)?;
        // What `encodeURIComponent` leaves as it is, the escapes, and the query's own marks.
        if !query
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.!~*'()%&=".contains(&b))
        {
            return Err(LinkError::Encoding);
        }
        let (mut server, mut room, mut code) = (None, None, None);
        for pair in query.split('&') {
            let (name, value) = pair.split_once('=').ok_or(LinkError::Query)?;
            let slot = match name {
                "server" => &mut server,
                "room" => &mut room,
                "code" => &mut code,
                "" => return Err(LinkError::Query),
                _ => return Err(LinkError::Unknown),
            };
            if slot.is_some() {
                return Err(LinkError::Repeated);
            }
            *slot = Some(decode(value)?);
        }
        let server = server_of(&server.ok_or(LinkError::MissingServer)?)?;
        let room = room.ok_or(LinkError::MissingRoom)?;
        if !(4..=16).contains(&room.len())
            || !room
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return Err(LinkError::Room);
        }
        let code = code.map(|c| code_of(&c)).transpose()?;
        Ok(Link {
            action,
            server,
            room,
            code,
        })
    }
}

/// Percent-decodes one value, strictly: every `%` begins two hex digits, and what they make is
/// UTF-8. It is decoded once; a value encoded twice keeps a `%`, which no value may hold.
fn decode(value: &str) -> Result<String, LinkError> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3).ok_or(LinkError::Encoding)?;
            let hex = std::str::from_utf8(hex).map_err(|_| LinkError::Encoding)?;
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(LinkError::Encoding);
            }
            out.push(u8::from_str_radix(hex, 16).map_err(|_| LinkError::Encoding)?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| LinkError::Encoding)
}

/// A link's code: `^bg[cj]_[A-Za-z0-9_-]{43}$`.
fn code_of(code: &str) -> Result<Code, LinkError> {
    let body = code
        .strip_prefix("bgc_")
        .or_else(|| code.strip_prefix("bgj_"))
        .ok_or(LinkError::Code)?;
    if body.len() != 43
        || !body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(LinkError::Code);
    }
    Ok(Code(code.to_string()))
}

/// A link's server: `https://host[:port]`, or `http://` to this machine (`localhost`,
/// `127.0.0.0/8`, `[::1]`), and nothing else: no path (not even `/`), query, fragment or user
/// info. Read by a grammar narrower than the client's own address rule, and then by that rule
/// too, which must come to the same origin: two readers never disagree about where a link goes.
fn server_of(server: &str) -> Result<ServerUrl, LinkError> {
    let bad = LinkError::Server;
    let (plain, rest) = if let Some(rest) = server.strip_prefix("https://") {
        (false, rest)
    } else if let Some(rest) = server.strip_prefix("http://") {
        (true, rest)
    } else {
        return Err(bad);
    };
    // Host and port only: printable ASCII with nothing that ends or escapes an authority.
    if rest.is_empty()
        || !rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b))
    {
        return Err(bad);
    }
    let (host, port) = if let Some(inner) = rest.strip_prefix('[') {
        let (ip, after) = inner.split_once(']').ok_or(bad)?;
        let ip: Ipv6Addr = ip.parse().map_err(|_| bad)?;
        (Host::V6(ip), after)
    } else {
        let end = rest.find(':').unwrap_or(rest.len());
        let (name, after) = rest.split_at(end);
        (host_of(name)?, after)
    };
    let port = match port {
        "" => None,
        p => {
            let digits = p.strip_prefix(':').ok_or(bad)?;
            if digits.is_empty()
                || digits.len() > 5
                || digits.starts_with('0')
                || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(bad);
            }
            Some(digits.parse::<u16>().map_err(|_| bad)?)
        }
    };
    let loopback = match &host {
        Host::Name(name) => name == "localhost",
        Host::V4(ip) => ip.is_loopback(),
        Host::V6(ip) => ip.is_loopback(),
    };
    if plain && !loopback {
        return Err(bad);
    }
    let url = ServerUrl::parse(server).map_err(|_| bad)?;
    let (scheme, default_port) = if plain { ("http", 80) } else { ("https", 443) };
    let origin = format!(
        "{scheme}://{}:{}",
        host.written(),
        port.unwrap_or(default_port)
    );
    if url.origin() != origin {
        return Err(bad);
    }
    Ok(url)
}

enum Host {
    Name(String),
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

impl Host {
    fn written(&self) -> String {
        match self {
            Host::Name(name) => name.clone(),
            Host::V4(ip) => ip.to_string(),
            Host::V6(ip) => format!("[{ip}]"),
        }
    }
}

/// A host's name or an IPv4 address. A name whose last label is a number, or a hex number, is
/// an address to a URL reader and to the system's resolver (`2130706433`, `0x7f.1`), so it must
/// be one written as four decimals or it is refused. An international name is refused as
/// written; its ASCII form (`xn--…`) is a name like any other, and is shown as it is.
fn host_of(name: &str) -> Result<Host, LinkError> {
    let bad = LinkError::Server;
    let name = name.to_ascii_lowercase();
    if name.is_empty() || name.len() > 253 {
        return Err(bad);
    }
    let labels: Vec<&str> = name.split('.').collect();
    let last = labels.last().copied().unwrap_or_default();
    let numeric =
        !last.is_empty() && (last.bytes().all(|b| b.is_ascii_digit()) || last.starts_with("0x"));
    if numeric {
        return name.parse::<Ipv4Addr>().map(Host::V4).map_err(|_| bad);
    }
    for label in &labels {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(bad);
        }
    }
    Ok(Host::Name(name))
}

/// Who this client is on a server, as its probe says: the account its session belongs to, or a
/// guest whose key it keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    Account(String),
    Guest,
}

/// What to do next with a link, given what the client holds for its server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Show the server and ask before connecting to it at all: one never used.
    AskServer,
    /// The client is somebody there already, and the link's code would make it another: say
    /// who it is and who the link would make it, and ask before trading.
    AskIdentity { now: Identity, would: Kind },
    /// Trade the code, then join.
    Trade,
    /// Join as whoever the client is there.
    Join,
}

/// What to do with `link`. `used`: the client has used its server before (it keeps a key, a
/// session or a trusted certificate for it, or it is the configured one). `agreed`: the player
/// has said yes to the server this run. `identity`: who the client is there, once the probe has
/// said (`None` before it has, and for nobody yet). `stay`: the player chose to stay who they
/// are rather than trade.
pub fn decide(
    link: &Link,
    used: bool,
    agreed: bool,
    identity: Option<&Identity>,
    stay: bool,
) -> Next {
    if !used && !agreed {
        return Next::AskServer;
    }
    let Some(code) = &link.code else {
        return Next::Join;
    };
    if stay {
        return Next::Join;
    }
    match identity {
        Some(now) => Next::AskIdentity {
            now: now.clone(),
            would: code.kind(),
        },
        None => Next::Trade,
    }
}

/// The question about a server never used.
pub fn server_question(link: &Link) -> (String, String) {
    let what = match link.action {
        Action::Join => "join",
        Action::Watch => "watch",
    };
    (
        format!("Connect to {}?", link.server),
        format!(
            "A link asks this client to {what} game {} there. You have not used this server \
             before: connect only if you trust it.",
            link.room
        ),
    )
}

/// The question before trading a code: who the client is, and who the link would make it.
pub fn identity_question(server: &ServerUrl, now: &Identity, would: Kind) -> (String, String) {
    let who = match now {
        Identity::Account(handle) => format!("You are signed in as {handle} on {server}."),
        Identity::Guest => format!("You are a guest on {server}."),
    };
    let then = match (now, would) {
        (Identity::Account(_), Kind::Account) => {
            "The link would sign you in as the account that opened it, in place of this one."
        }
        (Identity::Guest, Kind::Account) => {
            "The link would sign you in as the account that opened it. Your guest stays here, \
             for when you sign out."
        }
        (Identity::Account(_), Kind::Guest) => {
            "The link would make you the guest that opened it, and sign you out here."
        }
        (Identity::Guest, Kind::Guest) => {
            "The link would make you the guest that opened it, in place of this one."
        }
    };
    (who, then.to_string())
}

/// What the button to stay reads.
pub fn stay_label(now: &Identity) -> String {
    match now {
        Identity::Account(handle) => format!("Stay as {handle}"),
        Identity::Guest => "Stay as this guest".into(),
    }
}

/// What the player is told once a link's code was traded: nothing when it made them who the
/// link said; that they joined as who they already were when it was refused (spent, too old,
/// too many) or could not be traded.
pub fn traded_notice(
    result: &Result<crate::net::session::Traded, crate::net::session::Problem>,
) -> Option<String> {
    use crate::net::session::Problem;
    let why = match result {
        Ok(_) => return None,
        Err(Problem::Refused { status: 401, .. }) => {
            "its code was used already or is too old".to_string()
        }
        Err(Problem::Refused { status: 429, .. }) => {
            "too many links were opened from this address".to_string()
        }
        Err(problem) => format!("its code could not be traded ({})", problem.message()),
    };
    Some(format!(
        "The link did not change who you are: {why}. You joined as who you already were."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "bgc_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
    const GUEST_CODE: &str = "bgj_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";

    fn parse(s: &str) -> Result<Link, LinkError> {
        Link::parse(s)
    }

    fn refused(s: &str) -> LinkError {
        match Link::parse(s) {
            Ok(link) => panic!("{s} was taken: {link:?}"),
            Err(e) => e,
        }
    }

    /// The contract's own examples.
    #[test]
    fn the_contracts_links_are_taken() {
        let link = parse(&format!(
            "bingle-3d://join?server=https%3A%2F%2Fgames.example.com&room=K7MQ2X&code={CODE}"
        ))
        .unwrap();
        assert_eq!(link.action, Action::Join);
        assert_eq!(link.server.origin(), "https://games.example.com:443");
        assert_eq!(link.room, "K7MQ2X");
        assert_eq!(link.code.as_ref().map(Code::secret), Some(CODE));
        assert_eq!(link.code.as_ref().map(Code::kind), Some(Kind::Account));

        let link =
            parse("bingle-3d://watch?server=http%3A%2F%2F127.0.0.1%3A8080&room=K7MQ2X").unwrap();
        assert_eq!(link.action, Action::Watch);
        assert_eq!(link.server.origin(), "http://127.0.0.1:8080");
        assert_eq!(link.code, None, "a link without a code works");
    }

    #[test]
    fn order_does_not_matter_and_each_value_is_decoded() {
        let link = parse(&format!(
            "bingle-3d://join?code={GUEST_CODE}&room=AB23&server=https%3a%2f%2fbox.example%3a8443"
        ))
        .unwrap();
        assert_eq!(link.server.origin(), "https://box.example:8443");
        assert_eq!(link.code.unwrap().kind(), Kind::Guest);
        for server in [
            "http%3A%2F%2Flocalhost%3A18077",
            "http%3A%2F%2F127.8.9.10",
            "http%3A%2F%2F%5B%3A%3A1%5D%3A8080",
            "https%3A%2F%2F%5B2001%3Adb8%3A%3A1%5D",
            "https%3A%2F%2F192.168.1.20%3A65535",
            "https%3A%2F%2Fxn--bcher-kva.example",
        ] {
            let link = format!("bingle-3d://watch?server={server}&room=ABCD");
            assert!(parse(&link).is_ok(), "{link}");
        }
    }

    #[test]
    fn the_whole_link_is_refused_for_what_the_contract_lists() {
        let ok = "server=https%3A%2F%2Fgames.example.com&room=K7MQ2X";
        let cases: Vec<(String, LinkError)> = vec![
            // Another scheme or action.
            (format!("https://join?{ok}"), LinkError::Scheme),
            (format!("bingle3d://join?{ok}"), LinkError::Scheme),
            (format!("bingle-3d:join?{ok}"), LinkError::Scheme),
            (format!("bingle-3d://open?{ok}"), LinkError::Action),
            (format!("bingle-3d://?{ok}"), LinkError::Action),
            (format!("bingle-3d://join.example?{ok}"), LinkError::Action),
            // User info, a port or a path after the action.
            (format!("bingle-3d://ana@join?{ok}"), LinkError::UserInfo),
            (format!("bingle-3d://join:80?{ok}"), LinkError::Port),
            (format!("bingle-3d://join/?{ok}"), LinkError::Path),
            (format!("bingle-3d://join/x?{ok}"), LinkError::Path),
            // A fragment, even an empty one.
            (format!("bingle-3d://join?{ok}#"), LinkError::Fragment),
            (format!("bingle-3d://join?{ok}#x"), LinkError::Fragment),
            // A parameter not listed, given twice, or malformed.
            (format!("bingle-3d://join?{ok}&seat=1"), LinkError::Unknown),
            (
                format!("bingle-3d://join?{ok}&Server=x"),
                LinkError::Unknown,
            ),
            (
                format!("bingle-3d://join?{ok}&%73erver=x"),
                LinkError::Unknown,
            ),
            (
                format!("bingle-3d://join?{ok}&room=ABCD"),
                LinkError::Repeated,
            ),
            (
                format!("bingle-3d://join?{ok}&code={CODE}&code={CODE}"),
                LinkError::Repeated,
            ),
            (format!("bingle-3d://join?{ok}&"), LinkError::Query),
            (format!("bingle-3d://join?&{ok}"), LinkError::Query),
            (format!("bingle-3d://join?{ok}&code"), LinkError::Query),
            (format!("bingle-3d://join?{ok}&=x"), LinkError::Query),
            // A missing server or room.
            ("bingle-3d://join".into(), LinkError::MissingServer),
            (
                "bingle-3d://join?room=K7MQ2X".into(),
                LinkError::MissingServer,
            ),
            (
                "bingle-3d://join?server=https%3A%2F%2Fgames.example.com".into(),
                LinkError::MissingRoom,
            ),
            // Over 1024 characters, though every part is good.
            (
                format!("bingle-3d://join?{ok}&code={}", "A".repeat(1024)),
                LinkError::TooLong,
            ),
        ];
        for (link, why) in cases {
            assert_eq!(refused(&link), why, "{link}");
        }
    }

    #[test]
    fn a_value_that_fails_its_pattern_refuses_the_link() {
        let server = "server=https%3A%2F%2Fgames.example.com";
        for room in [
            "ABC",
            "ABCDEFGHJKMNPQRST",
            "k7mq2x",
            "K7MQ-X",
            "K7MQ2X%20",
            "",
            "-K7MQ2X",
        ] {
            let link = format!("bingle-3d://join?{server}&room={room}");
            assert_eq!(refused(&link), LinkError::Room, "{link}");
        }
        for code in [
            "",
            "bgs_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
            "bgg_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
            "bgc_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh",
            "bgc_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8A",
            "bgc_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdH.8",
            "BGC_AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
            "--frames",
        ] {
            let link = format!("bingle-3d://join?{server}&room=K7MQ2X&code={code}");
            assert_eq!(refused(&link), LinkError::Code, "{link}");
        }
    }

    #[test]
    fn a_server_a_link_may_not_name_is_refused() {
        for server in [
            // Plain http beyond this machine, even what looks local.
            "http://games.example.com",
            "http://192.168.1.20:8080",
            "http://localhost.example.com",
            "http://app.localhost",
            "http://[::2]",
            "http://0x7f.0.0.1",
            "http://2130706433",
            "http://127.1",
            "http://0177.0.0.1",
            // A path (not even /), a query, a fragment, user info.
            "https://games.example.com/",
            "https://games.example.com/bingle",
            "https://games.example.com?x",
            "https://games.example.com#x",
            "https://ana@games.example.com",
            "https://ana:pw@games.example.com",
            // Two readers that could disagree about where it goes.
            "https://games.example.com\\@evil.example",
            "https://evil.example\\games.example.com",
            "https:\\\\games.example.com",
            "https:/games.example.com",
            "https:games.example.com",
            "https://games.example.com@evil.example",
            "https://bücher.example",
            "https://games.example.com.",
            "https://games..example.com",
            "https://-games.example.com",
            "https://games-.example.com",
            "https://games_x.example.com",
            "https://games.example.com:",
            "https://games.example.com:0",
            "https://games.example.com:08443",
            "https://games.example.com:65536",
            "https://games.example.com:8443:1",
            "https://[::1",
            "https://[::1]x",
            "https://[v1.x]",
            "https://1.2.3.4.5",
            "https://999.0.0.1",
            "https://0x7f000001",
            // Another scheme, none, or something on either side.
            "ftp://games.example.com",
            "HTTPS://games.example.com",
            "games.example.com",
            "//games.example.com",
            " https://games.example.com",
            "https://games.example.com ",
            "https://games.example.com\n",
            "https://games.example.com\0",
            "https://",
            "",
            "--setup",
            "-https://games.example.com",
        ] {
            let encoded = crate::net::server_url::query_value(server);
            let link = format!("bingle-3d://join?server={encoded}&room=K7MQ2X");
            assert!(
                matches!(refused(&link), LinkError::Server | LinkError::Encoding),
                "{server:?}"
            );
        }
    }

    #[test]
    fn odd_encodings_are_refused() {
        let room = "room=K7MQ2X";
        for query in [
            // Encoded twice: the second layer is left, with a % no value may hold.
            "server=https%253A%252F%252Fgames.example.com",
            // A % not followed by two hex digits.
            "server=https%3A%2F%2Fgames.example.com%",
            "server=https%3A%2F%2Fgames.example.com%2",
            "server=https%3A%2F%2Fgames.example.com%zz",
            "server=https%3A%2F%2Fgames.example.com%+1",
            // Not UTF-8 once decoded.
            "server=https%3A%2F%2Fgames.example.com%FF",
            "server=https%3A%2F%2Fg%C3%28.example.com",
            // A NUL, a newline or a carriage return, encoded.
            "server=https%3A%2F%2Fgames.example.com%00",
            "server=https%3A%2F%2Fgames.example.com%0A",
            "server=https%3A%2F%2Fgames.example.com%0D%0A--setup",
            // Characters the page always encodes, left as they are.
            "server=https://games.example.com",
            "server=https%3A%2F%2Fgames.example.com:8443",
            "server=https%3A%2F%2Fgames.example.com+",
            "server=https%3A%2F%2Fgames.example.com;x",
            "server=https%3A%2F%2F[::1]",
        ] {
            let link = format!("bingle-3d://join?{query}&{room}");
            assert!(
                matches!(refused(&link), LinkError::Server | LinkError::Encoding),
                "{link}"
            );
        }
        // Raw characters anywhere in the link: a space, a quote, a newline, a NUL, a tab,
        // anything not ASCII.
        let ok = "bingle-3d://join?server=https%3A%2F%2Fgames.example.com&room=K7MQ2X";
        for bad in [" ", "\"", "\n", "\r\n", "\0", "\t", "é", "\u{202e}"] {
            let link = format!("{ok}{bad}");
            assert_eq!(
                refused(&link),
                if bad == "\"" {
                    LinkError::Encoding
                } else {
                    LinkError::Character
                },
                "{link:?}"
            );
            let link = format!("bingle-3d://join{bad}?server=x&room=K7MQ2X");
            assert!(Link::parse(&link).is_err(), "{link:?}");
        }
        assert_eq!(
            refused(&format!("{ok}\" --setup \"x")),
            LinkError::Character
        );
        assert_eq!(refused(&format!("-{ok}")), LinkError::Scheme);
        assert_eq!(refused(&format!("--link={ok}")), LinkError::Scheme);
    }

    #[test]
    fn the_scheme_and_action_are_read_without_regard_to_case() {
        let link = "BINGLE-3D://Join?server=https%3A%2F%2Fgames.example.com&room=K7MQ2X";
        assert_eq!(parse(link).unwrap().action, Action::Join);
        // The host of the server is a name either way, written lower-case.
        let link = "bingle-3d://watch?server=https%3A%2F%2FGames.Example.COM&room=K7MQ2X";
        assert_eq!(
            parse(link).unwrap().server.origin(),
            "https://games.example.com:443"
        );
    }

    #[test]
    fn the_code_is_never_in_what_is_printed() {
        let link = parse(&format!(
            "bingle-3d://join?server=https%3A%2F%2Fgames.example.com&room=K7MQ2X&code={CODE}"
        ))
        .unwrap();
        let printed = format!("{link:?}");
        assert!(!printed.contains(&CODE[4..]), "{printed}");
        let (title, body) = server_question(&link);
        assert!(!title.contains(&CODE[4..]) && !body.contains(&CODE[4..]));
    }

    fn link(code: Option<&str>) -> Link {
        let code = code.map(|c| format!("&code={c}")).unwrap_or_default();
        parse(&format!(
            "bingle-3d://join?server=https%3A%2F%2Fgames.example.com&room=K7MQ2X{code}"
        ))
        .unwrap()
    }

    #[test]
    fn a_server_never_used_is_asked_about_before_anything() {
        for l in [link(None), link(Some(CODE))] {
            assert_eq!(decide(&l, false, false, None, false), Next::AskServer);
            let guest = Identity::Guest;
            assert_eq!(
                decide(&l, false, false, Some(&guest), false),
                Next::AskServer
            );
        }
        // Agreed to, with nobody held there: the code is traded, or the client joins.
        assert_eq!(
            decide(&link(Some(CODE)), false, true, None, false),
            Next::Trade
        );
        assert_eq!(decide(&link(None), false, true, None, false), Next::Join);
    }

    #[test]
    fn a_code_that_would_make_the_client_somebody_else_is_asked_about() {
        let ana = Identity::Account("ana".into());
        assert_eq!(
            decide(&link(Some(CODE)), true, false, Some(&ana), false),
            Next::AskIdentity {
                now: ana.clone(),
                would: Kind::Account
            }
        );
        assert_eq!(
            decide(
                &link(Some(GUEST_CODE)),
                true,
                false,
                Some(&Identity::Guest),
                false
            ),
            Next::AskIdentity {
                now: Identity::Guest,
                would: Kind::Guest
            }
        );
        // Staying who one is joins without trading.
        assert_eq!(
            decide(&link(Some(CODE)), true, false, Some(&ana), true),
            Next::Join
        );
        // Nobody held there yet: nobody to be asked about.
        assert_eq!(
            decide(&link(Some(CODE)), true, false, None, false),
            Next::Trade
        );
        // No code: who the client is stays.
        assert_eq!(
            decide(&link(None), true, false, Some(&ana), false),
            Next::Join
        );
    }

    #[test]
    fn a_refused_code_joins_as_who_one_is_and_says_so() {
        use crate::net::session::{Problem, Traded};
        assert_eq!(traded_notice(&Ok(Traded::Guest)), None);
        let refused = |status| {
            traded_notice(&Err(Problem::Refused {
                status,
                message: "x".into(),
            }))
            .unwrap()
        };
        assert!(refused(401).contains("used already or is too old"));
        assert!(refused(401).contains("joined as who you already were"));
        assert!(refused(429).contains("too many"));
    }

    #[test]
    fn the_questions_say_who_and_where() {
        let l = link(Some(CODE));
        let (title, body) = server_question(&l);
        assert_eq!(title, "Connect to https://games.example.com?");
        assert!(body.contains("K7MQ2X") && body.contains("not used this server"));
        let (who, then) =
            identity_question(&l.server, &Identity::Account("ana".into()), Kind::Account);
        assert!(who.contains("ana") && who.contains("games.example.com"));
        assert!(then.contains("account that opened it"));
        let (who, then) = identity_question(&l.server, &Identity::Guest, Kind::Guest);
        assert!(who.contains("guest"));
        assert!(then.contains("in place of this one"));
        assert_eq!(stay_label(&Identity::Account("ana".into())), "Stay as ana");
    }
}
