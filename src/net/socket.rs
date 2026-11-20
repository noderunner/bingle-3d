//! The server's WebSockets, blocking, on the network thread: a game's, `GET /ws/{code}`, opened
//! as a spectator or as the player of a seat (a player's socket is what shows the seat as
//! connected), and a signed-in account's own, `GET /ws-dm`, which says when its friends list
//! changes.
//!
//! The socket is read with a short timeout, so that the one network thread can still answer the
//! screens' requests between reads (see `mod.rs`). TLS is ours, the same `rustls` configuration
//! as for HTTP, so a certificate the player trusted by its fingerprint is trusted here too;
//! tungstenite only speaks the WebSocket protocol over it.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::HandshakeError;
use tungstenite::http::HeaderValue;
use tungstenite::{Message, WebSocket};

use super::server_url::{Scheme, ServerUrl, query_value};

/// How long a connection and the opening handshake may take.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// How long one read waits for a message before the thread looks at its requests again.
pub const POLL: Duration = Duration::from_millis(25);

/// The name a spectator gives the server. A spectator takes no seat; the name is only shown.
const SPECTATOR_NAME: &str = "bingle-3d";

#[derive(Debug)]
pub enum Error {
    HostNotFound,
    Io(io::Error),
    /// The server answered the handshake with an HTTP error (an unknown room, say).
    Refused {
        status: u16,
        message: String,
    },
    /// The server closed the socket.
    Closed,
    Protocol(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::HostNotFound => write!(f, "the host could not be found"),
            Error::Io(e) => write!(f, "{e}"),
            Error::Refused { status, message } => write!(f, "{status}: {message}"),
            Error::Closed => write!(f, "the server closed it"),
            Error::Protocol(m) => write!(f, "{m}"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<tungstenite::Error> for Error {
    fn from(e: tungstenite::Error) -> Self {
        match e {
            tungstenite::Error::Io(e) => Error::Io(e),
            tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
                Error::Closed
            }
            tungstenite::Error::Http(response) => {
                let status = response.status().as_u16();
                let message = response
                    .body()
                    .as_deref()
                    .map(|b| String::from_utf8_lossy(b).trim().to_string())
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| format!("the game's socket was answered with {status}"));
                Error::Refused { status, message }
            }
            other => Error::Protocol(other.to_string()),
        }
    }
}

enum Stream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Stream {
    fn tcp(&self) -> &TcpStream {
        match self {
            Stream::Plain(s) => s,
            Stream::Tls(s) => &s.sock,
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

/// Who the socket is opened for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role<'a> {
    Spectator,
    /// The player at a seat of this guest's: its name, and the guest key that owns it.
    Player {
        name: &'a str,
        key: &'a str,
    },
}

/// An open game socket.
pub struct Socket {
    ws: WebSocket<Stream>,
}

impl Socket {
    /// Opens `/ws/{room_code}`.
    pub fn open(
        url: &ServerUrl,
        room_code: &str,
        role: Role,
        tls: Arc<ClientConfig>,
    ) -> Result<Self, Error> {
        let key = match role {
            Role::Player { key, .. } => Some(key),
            Role::Spectator => None,
        };
        Self::connect(url, socket_url(url, room_code, role), key, tls)
    }

    /// Opens the game's voice socket for the player `name`, whose seat `key` holds: it carries
    /// only who joins and leaves the call, and the WebRTC messages for that seat.
    pub fn open_voice(
        url: &ServerUrl,
        room_code: &str,
        name: &str,
        key: &str,
        tls: Arc<ClientConfig>,
    ) -> Result<Self, Error> {
        let target = format!(
            "{}&only=voice",
            socket_url(url, room_code, Role::Player { name, key })
        );
        Self::connect(url, target, Some(key), tls)
    }

    /// Opens `/ws-dm`, the socket of the account whose session `token` is.
    pub fn open_account(
        url: &ServerUrl,
        token: &str,
        tls: Arc<ClientConfig>,
    ) -> Result<Self, Error> {
        Self::connect(url, account_socket_url(url), Some(token), tls)
    }

    fn connect(
        url: &ServerUrl,
        target: String,
        bearer: Option<&str>,
        tls: Arc<ClientConfig>,
    ) -> Result<Self, Error> {
        let addrs: Vec<_> = (url.host.as_str(), url.port)
            .to_socket_addrs()
            .map_err(|_| Error::HostNotFound)?
            .collect();
        let mut last = Error::HostNotFound;
        let mut tcp = None;
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, OPEN_TIMEOUT) {
                Ok(s) => {
                    tcp = Some(s);
                    break;
                }
                Err(e) => last = Error::Io(e),
            }
        }
        let tcp = tcp.ok_or(last)?;
        tcp.set_nodelay(true)?;
        tcp.set_read_timeout(Some(OPEN_TIMEOUT))?;
        tcp.set_write_timeout(Some(OPEN_TIMEOUT))?;

        let stream = match url.scheme {
            Scheme::Http => Stream::Plain(tcp),
            Scheme::Https => {
                let name = ServerName::try_from(url.host.clone())
                    .map_err(|_| Error::Protocol("the host is not a valid name for TLS".into()))?;
                let conn =
                    ClientConnection::new(tls, name).map_err(|e| Error::Protocol(e.to_string()))?;
                Stream::Tls(Box::new(StreamOwned::new(conn, tcp)))
            }
        };

        let mut request = target.into_client_request()?;
        let headers = request.headers_mut();
        headers.insert(
            "User-Agent",
            HeaderValue::from_static(concat!("bingle-3d/", env!("CARGO_PKG_VERSION"))),
        );
        if let Some(key) = bearer {
            let auth = HeaderValue::from_str(&format!("Bearer {key}"))
                .map_err(|_| Error::Protocol("the key is not a valid header".into()))?;
            headers.insert("Authorization", auth);
        }
        let (ws, _) = tungstenite::client(request, stream).map_err(|e| match e {
            HandshakeError::Failure(e) => Error::from(e),
            // Only a non-blocking stream is interrupted, and this one blocks until the timeout.
            HandshakeError::Interrupted(_) => Error::Io(io::Error::from(io::ErrorKind::TimedOut)),
        })?;
        ws.get_ref().tcp().set_read_timeout(Some(POLL))?;
        Ok(Socket { ws })
    }

    /// The next text message, waiting at most [`POLL`]: `None` when nothing came in time, or
    /// when what came was not text (a ping, which tungstenite answers by itself).
    pub fn next(&mut self) -> Result<Option<String>, Error> {
        match self.ws.read() {
            Ok(Message::Text(text)) => Ok(Some(text.to_string())),
            Ok(_) => Ok(None),
            // Linux reports a read timeout as WouldBlock, Windows as TimedOut. Either leaves the
            // socket as it was, partial frames included, to be read again.
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Says goodbye to the server, as far as it can without waiting.
    pub fn close(mut self) {
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }
}

/// The server's origin with `ws://` or `wss://` for its scheme.
fn ws_origin(url: &ServerUrl) -> String {
    let origin = url.origin();
    origin
        .strip_prefix("https")
        .map(|r| format!("wss{r}"))
        .or_else(|| origin.strip_prefix("http").map(|r| format!("ws{r}")))
        .expect("an origin starts with its scheme")
}

/// The account's socket.
pub fn account_socket_url(url: &ServerUrl) -> String {
    format!("{}/ws-dm", ws_origin(url))
}

/// `ws://` or `wss://`, to the room's socket as a spectator or a seat's player.
pub fn socket_url(url: &ServerUrl, room_code: &str, role: Role) -> String {
    let rest = ws_origin(url);
    match role {
        Role::Spectator => format!("{rest}/ws/{room_code}?name={SPECTATOR_NAME}&role=spectator"),
        Role::Player { name, .. } => format!(
            "{rest}/ws/{room_code}?name={}&role=player",
            query_value(name)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn the_socket_url_follows_the_server_scheme() {
        let local = ServerUrl::parse("localhost:8080").unwrap();
        assert_eq!(
            socket_url(&local, "AB12CD", Role::Spectator),
            "ws://localhost:8080/ws/AB12CD?name=bingle-3d&role=spectator"
        );
        let remote = ServerUrl::parse("games.example.com").unwrap();
        assert_eq!(
            socket_url(&remote, "AB12CD", Role::Spectator),
            "wss://games.example.com:443/ws/AB12CD?name=bingle-3d&role=spectator"
        );
        let player = Role::Player {
            name: "Ana Bé",
            key: "bgg_x",
        };
        assert_eq!(
            socket_url(&local, "AB12CD", player),
            "ws://localhost:8080/ws/AB12CD?name=Ana%20B%C3%A9&role=player",
            "the name is a player's, and the key is not in the address"
        );
        assert_eq!(account_socket_url(&local), "ws://localhost:8080/ws-dm");
        assert_eq!(
            account_socket_url(&remote),
            "wss://games.example.com:443/ws-dm"
        );
    }

    fn tls() -> Arc<ClientConfig> {
        super::super::tls::client_config(super::super::tls::Trust::new(vec![]))
    }

    /// A WebSocket server on loopback that sends `messages`, one every `gap`, then closes.
    fn serve(messages: Vec<&'static str>, gap: Duration) -> ServerUrl {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            for m in messages {
                std::thread::sleep(gap);
                ws.send(Message::text(m)).unwrap();
            }
            let _ = ws.close(None);
            // Answer the client's close, so the socket ends cleanly.
            while ws.read().is_ok() {}
        });
        ServerUrl::parse(&format!("127.0.0.1:{port}")).unwrap()
    }

    #[test]
    fn messages_arrive_between_timeouts_and_a_close_ends_it() {
        // Slower than POLL, so reads time out in between and must not lose anything.
        let url = serve(vec!["one", "two"], POLL * 3);
        let mut socket = Socket::open(&url, "AB12CD", Role::Spectator, tls()).unwrap();
        let mut got = Vec::new();
        let mut empty = 0;
        let err = loop {
            match socket.next() {
                Ok(Some(m)) => got.push(m),
                Ok(None) => empty += 1,
                Err(e) => break e,
            }
            assert!(empty < 400, "the socket never closed");
        };
        assert_eq!(got, ["one", "two"]);
        assert!(empty > 0, "some reads timed out");
        assert!(matches!(err, Error::Closed), "{err:?}");
    }

    #[test]
    fn an_http_error_at_the_handshake_is_a_refusal() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0; 2048];
            let _ = stream.read(&mut buf);
            let body = "session: no such room\n";
            // In one write, as Go's http.Error sends it: tungstenite keeps only the part of the
            // body that arrived with the headers.
            let answer = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(answer.as_bytes());
        });
        let url = ServerUrl::parse(&format!("127.0.0.1:{port}")).unwrap();
        match Socket::open(&url, "NOPE00", Role::Spectator, tls()) {
            Err(Error::Refused { status, message }) => {
                assert_eq!(status, 404);
                assert_eq!(message, "session: no such room");
            }
            other => panic!("{:?}", other.err()),
        }
    }
}
