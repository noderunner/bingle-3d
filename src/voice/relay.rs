//! A TURN relay for one connection, for players whose network lets no direct connection through
//! (a strict NAT, or a firewall that only lets out what it has seen go out): the relay server
//! gives this socket an address of its own (an allocation), which becomes a relay candidate, and
//! carries the connection's packets to and from the other end. Bingle hands out the server and
//! time-limited credentials with `GET /api/rtc/config`.
//!
//! The relay shares the connection's UDP socket: what comes from the server is the relay's; what
//! the connection sends from the relayed address goes through it. rtc's TURN client does the
//! protocol (allocating, refreshing, permissions, channels), with no I/O of its own.

use std::net::SocketAddr;
use std::time::Instant;

use bevy::log::{debug, info};
use bytes::BytesMut;
use rtc::sansio::Protocol;
use rtc::shared::error::Error as TurnError;
use rtc::shared::{TaggedBytesMut, TransportContext, TransportProtocol};
use rtc::stun::message::TransactionId;
use rtc::turn::client::{Client, ClientConfig, Event as TurnEvent};

/// A relay server, as `GET /api/rtc/config` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnServer {
    pub addr: SocketAddr,
    /// The URL it was given as, which the relay candidate names.
    pub url: String,
    pub username: String,
    pub credential: String,
}

/// What the relay has to say to the connection.
#[derive(Debug, PartialEq, Eq)]
pub enum Relayed {
    /// The server gave this socket an address: a candidate.
    Allocated(SocketAddr),
    /// The server would not; the call goes on without a relay.
    Refused(String),
    /// A packet from `from`, through the relay.
    Data { from: SocketAddr, data: BytesMut },
}

pub struct Relay {
    client: Client,
    server: TurnServer,
    local: SocketAddr,
    allocation: Option<TransactionId>,
    relayed: Option<SocketAddr>,
}

impl Relay {
    /// Asks `server` for an allocation for the socket at `local`.
    pub fn new(server: &TurnServer, local: SocketAddr, now: Instant) -> Result<Relay, String> {
        let config = ClientConfig {
            stun_serv_addr: String::new(),
            turn_serv_addr: server.addr.to_string(),
            local_addr: local,
            transport_protocol: TransportProtocol::UDP,
            username: server.username.clone(),
            password: server.credential.clone(),
            // Learnt from the server's first answer.
            realm: String::new(),
            software: String::new(),
            rto_in_ms: 0,
            allocation_refresh_interval_cap: None,
        };
        let fail = |e: String| format!("voice: the relay {}: {e}", server.url);
        let crypto = rtc::crypto::default_provider().map_err(|e| fail(e.to_string()))?;
        let mut client = Client::new(config, crypto).map_err(|e| fail(e.to_string()))?;
        let allocation = client.allocate(now).map_err(|e| fail(e.to_string()))?;
        Ok(Relay {
            client,
            server: server.clone(),
            local,
            allocation: Some(allocation),
            relayed: None,
        })
    }

    /// The address the server gave, once it has.
    pub fn relayed(&self) -> Option<SocketAddr> {
        self.relayed
    }

    pub fn url(&self) -> &str {
        &self.server.url
    }

    /// Whether a datagram came from the relay server (and so is the relay's to read).
    pub fn is_from_server(&self, from: SocketAddr) -> bool {
        from == self.server.addr
    }

    /// A datagram from the server; what it meant is in `happened`.
    pub fn read(&mut self, now: Instant, data: &[u8], happened: &mut Vec<Relayed>) {
        let read = self.client.handle_read(TaggedBytesMut {
            now,
            transport: TransportContext {
                local_addr: self.local,
                peer_addr: self.server.addr,
                ecn: None,
                transport_protocol: TransportProtocol::UDP,
            },
            message: BytesMut::from(data),
        });
        if let Err(e) = read {
            debug!("voice: from the relay: {e}");
        }
        self.events(happened);
    }

    /// Sends `data` to `to` through the relay. The first packets to somebody new are dropped
    /// while the server is asked to let them through (a permission): ICE sends its checks again.
    pub fn send(&mut self, now: Instant, data: &[u8], to: SocketAddr) {
        let Some(relayed) = self.relayed else { return };
        let Ok(mut relay) = self.client.relay(relayed) else {
            return;
        };
        match relay.send_to(now, data, to) {
            Ok(()) => {}
            Err(TurnError::ErrNoPermission) => {
                if let Ok(Some(_)) = relay.create_permission(now, to) {
                    debug!("voice: asking the relay to let {to} through");
                }
            }
            Err(e) => debug!("voice: through the relay to {to}: {e}"),
        }
    }

    /// What is to go to the server.
    pub fn poll_write(&mut self) -> Option<(BytesMut, SocketAddr)> {
        self.client
            .poll_write()
            .map(|m| (m.message, m.transport.peer_addr))
    }

    pub fn next_wake(&mut self) -> Option<Instant> {
        self.client.poll_timeout()
    }

    /// Runs the client's timers (retransmissions, refreshing the allocation and permissions).
    pub fn tick(&mut self, now: Instant, happened: &mut Vec<Relayed>) {
        if self.client.poll_timeout().is_some_and(|at| at <= now)
            && let Err(e) = self.client.handle_timeout(now)
        {
            debug!("voice: the relay's timers: {e}");
        }
        self.events(happened);
    }

    /// Gives the allocation back, so the server need not wait for it to expire.
    pub fn close(&mut self, now: Instant) {
        if let Some(relayed) = self.relayed.take()
            && let Ok(mut relay) = self.client.relay(relayed)
        {
            let _ = relay.close(now);
        }
    }

    fn events(&mut self, happened: &mut Vec<Relayed>) {
        while let Some(event) = self.client.poll_event() {
            match event {
                TurnEvent::AllocateResponse(id, addr) if self.allocation == Some(id) => {
                    self.allocation = None;
                    self.relayed = Some(addr);
                    info!("voice: the relay {} gave {addr}", self.server.url);
                    happened.push(Relayed::Allocated(addr));
                }
                // Until the allocation is made, it is the only request: an error or a timeout
                // is its (the first, unauthenticated attempt has a transaction of its own).
                TurnEvent::AllocateError(_, e) if self.allocation.is_some() => {
                    self.allocation = None;
                    happened.push(Relayed::Refused(e.to_string()));
                }
                TurnEvent::TransactionTimeout(_) if self.allocation.is_some() => {
                    self.allocation = None;
                    happened.push(Relayed::Refused("it did not answer".into()));
                }
                TurnEvent::DataIndicationOrChannelData(_, from, data) => {
                    happened.push(Relayed::Data { from, data });
                }
                TurnEvent::CreatePermissionError(_, e) => {
                    debug!("voice: the relay refused a permission: {e}");
                }
                _ => {}
            }
        }
    }
}
