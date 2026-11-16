//! A Bingle server's address, and the rule about which addresses may be used at all.
//!
//! The connection must be secure (Bingle's ADR 0065): plain `http://` only to this machine
//! (loopback), and `https://` everywhere else. An address without a scheme gets `http://` when it
//! names this machine and `https://` otherwise, so `localhost:8080` and `games.example.com` both
//! do what is meant.

use std::fmt;
use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Http,
    Https,
}

/// Where a server is: a scheme, a host and a port, and nothing else. Bingle is served at the root
/// of its host, so a path is not part of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerUrl {
    pub scheme: Scheme,
    /// The host as written, lower-cased, without the brackets of an IPv6 address.
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlError {
    Empty,
    /// Something other than `http` or `https`.
    Scheme(String),
    NoHost,
    Port(String),
    /// A path, a query or a fragment: Bingle lives at the root of its host.
    Path(String),
    /// Plain HTTP to a host that is not this machine.
    PlainHttpNotLoopback(String),
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UrlError::Empty => write!(f, "no server address is set"),
            UrlError::Scheme(s) => write!(f, "\"{s}\" is not http or https"),
            UrlError::NoHost => write!(f, "the address names no host"),
            UrlError::Port(p) => write!(f, "\"{p}\" is not a port number"),
            UrlError::Path(p) => write!(
                f,
                "a server address is a host and a port, without a path (\"{p}\")"
            ),
            UrlError::PlainHttpNotLoopback(host) => write!(
                f,
                "plain http:// is only allowed to this machine; use https://{host} instead"
            ),
        }
    }
}

impl std::error::Error for UrlError {}

impl ServerUrl {
    pub fn parse(input: &str) -> Result<Self, UrlError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(UrlError::Empty);
        }
        let (scheme, rest) = match input.split_once("://") {
            Some((s, rest)) => match s.to_ascii_lowercase().as_str() {
                "http" => (Some(Scheme::Http), rest),
                "https" => (Some(Scheme::Https), rest),
                _ => return Err(UrlError::Scheme(s.to_string())),
            },
            None => (None, input),
        };
        // Allow one trailing slash; anything after the authority is a path.
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        if let Some(i) = rest.find(['/', '?', '#']) {
            return Err(UrlError::Path(rest[i..].to_string()));
        }
        if rest.contains('@') {
            // user:password@host would put a secret in the config file.
            return Err(UrlError::NoHost);
        }
        let (host, port) = split_host_port(rest)?;
        if host.is_empty() {
            return Err(UrlError::NoHost);
        }
        let host = host.to_ascii_lowercase();
        let scheme = scheme.unwrap_or(if is_loopback(&host) {
            Scheme::Http
        } else {
            Scheme::Https
        });
        if scheme == Scheme::Http && !is_loopback(&host) {
            return Err(UrlError::PlainHttpNotLoopback(rest.to_string()));
        }
        let port = match port {
            Some(p) => p,
            None => match scheme {
                Scheme::Http => 80,
                Scheme::Https => 443,
            },
        };
        Ok(ServerUrl { scheme, host, port })
    }

    /// The canonical form, used as the key for what is remembered about a server (its guest key,
    /// a certificate the player trusted): `https://host:port`, the port always written.
    pub fn origin(&self) -> String {
        let scheme = match self.scheme {
            Scheme::Http => "http",
            Scheme::Https => "https",
        };
        format!("{scheme}://{}:{}", self.bracketed_host(), self.port)
    }

    /// A full URL for a path on this server (`/api/games`).
    pub fn join(&self, path: &str) -> String {
        format!("{}{path}", self.origin())
    }

    #[cfg(test)]
    pub fn is_loopback(&self) -> bool {
        is_loopback(&self.host)
    }

    fn bracketed_host(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        }
    }
}

impl fmt::Display for ServerUrl {
    /// As a person would write it: the default port left out.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let default = matches!(
            (self.scheme, self.port),
            (Scheme::Http, 80) | (Scheme::Https, 443)
        );
        let scheme = match self.scheme {
            Scheme::Http => "http",
            Scheme::Https => "https",
        };
        if default {
            write!(f, "{scheme}://{}", self.bracketed_host())
        } else {
            write!(f, "{scheme}://{}:{}", self.bracketed_host(), self.port)
        }
    }
}

fn split_host_port(s: &str) -> Result<(&str, Option<u16>), UrlError> {
    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        // [::1]:8080
        let Some((host, after)) = rest.split_once(']') else {
            return Err(UrlError::NoHost);
        };
        match after {
            "" => (host, None),
            p if p.starts_with(':') => (host, Some(&p[1..])),
            _ => return Err(UrlError::NoHost),
        }
    } else if s.matches(':').count() > 1 {
        // A bare IPv6 address, which cannot carry a port without brackets.
        (s, None)
    } else {
        match s.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (s, None),
        }
    };
    let port = match port {
        Some(p) => Some(
            p.parse::<u16>()
                .ok()
                .filter(|&p| p != 0)
                .ok_or_else(|| UrlError::Port(p.to_string()))?,
        ),
        None => None,
    };
    Ok((host, port))
}

/// Whether a host is this machine: `localhost` (and names under it, which resolvers keep local),
/// `127.0.0.0/8` or `::1`.
fn is_loopback(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> ServerUrl {
        ServerUrl::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn loopback_gets_http_and_anything_else_https() {
        assert_eq!(parse("localhost:8080").origin(), "http://localhost:8080");
        assert_eq!(parse("127.0.0.1:8080").origin(), "http://127.0.0.1:8080");
        assert_eq!(parse("[::1]:8080").origin(), "http://[::1]:8080");
        assert_eq!(
            parse("games.example.com").origin(),
            "https://games.example.com:443"
        );
        assert_eq!(
            parse("192.168.1.20:8443").origin(),
            "https://192.168.1.20:8443"
        );
    }

    #[test]
    fn plain_http_is_refused_beyond_this_machine() {
        assert!(matches!(
            ServerUrl::parse("http://games.example.com"),
            Err(UrlError::PlainHttpNotLoopback(_))
        ));
        assert!(matches!(
            ServerUrl::parse("http://192.168.1.20:8080"),
            Err(UrlError::PlainHttpNotLoopback(_))
        ));
        // Looks local, is not.
        assert!(matches!(
            ServerUrl::parse("http://localhost.example.com"),
            Err(UrlError::PlainHttpNotLoopback(_))
        ));
        assert!(parse("http://127.8.9.10:8080").is_loopback());
        assert!(parse("http://app.localhost:8080").is_loopback());
    }

    #[test]
    fn https_to_this_machine_is_fine() {
        let url = parse("https://localhost:8443/");
        assert_eq!(url.scheme, Scheme::Https);
        assert_eq!(url.port, 8443);
    }

    #[test]
    fn the_origin_always_has_a_port_and_display_leaves_the_default_out() {
        let url = parse("HTTPS://Games.Example.com/");
        assert_eq!(url.origin(), "https://games.example.com:443");
        assert_eq!(url.to_string(), "https://games.example.com");
        assert_eq!(url.join("/api"), "https://games.example.com:443/api");
        assert_eq!(parse("localhost:8080").to_string(), "http://localhost:8080");
    }

    #[test]
    fn bad_addresses_say_what_is_wrong() {
        assert_eq!(ServerUrl::parse("  "), Err(UrlError::Empty));
        assert!(matches!(
            ServerUrl::parse("ftp://x"),
            Err(UrlError::Scheme(_))
        ));
        assert!(matches!(
            ServerUrl::parse("localhost:http"),
            Err(UrlError::Port(_))
        ));
        assert!(matches!(
            ServerUrl::parse("localhost:0"),
            Err(UrlError::Port(_))
        ));
        assert!(matches!(
            ServerUrl::parse("https://example.com/bingle"),
            Err(UrlError::Path(_))
        ));
        assert!(matches!(
            ServerUrl::parse("https://me:secret@example.com"),
            Err(UrlError::NoHost)
        ));
        assert!(matches!(
            ServerUrl::parse("https://:8080"),
            Err(UrlError::NoHost)
        ));
    }
}
