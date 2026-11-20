//! A TURN server for tests, on loopback: just enough of RFC 8656 to relay a call (allocations
//! after one 401, permissions, channels, Send and Data indications, refreshes), so the relay's
//! path through the engine is tried with no server to install. It checks the user name a client
//! sends, not the message integrity: what it proves is the client's routing, not its hashing,
//! which is rtc's own and tested there.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use rtc::stun::attributes::{ATTR_NONCE, ATTR_REALM, ATTR_USERNAME};
use rtc::stun::error_code::{CODE_UNAUTHORIZED, ErrorCodeAttribute};
use rtc::stun::fingerprint::FINGERPRINT;
use rtc::stun::message::{
    CLASS_ERROR_RESPONSE, CLASS_INDICATION, CLASS_REQUEST, CLASS_SUCCESS_RESPONSE, Getter,
    METHOD_ALLOCATE, METHOD_CHANNEL_BIND, METHOD_CREATE_PERMISSION, METHOD_DATA, METHOD_REFRESH,
    METHOD_SEND, Message, MessageType, Setter, TransactionId,
};
use rtc::stun::textattrs::TextAttribute;
use rtc::stun::xoraddr::XorMappedAddress;
use rtc::turn::proto::chandata::ChannelData;
use rtc::turn::proto::channum::ChannelNumber;
use rtc::turn::proto::data::Data;
use rtc::turn::proto::lifetime::Lifetime;
use rtc::turn::proto::peeraddr::PeerAddress;
use rtc::turn::proto::relayaddr::RelayedAddress;

pub struct FakeTurn {
    pub addr: SocketAddr,
    /// Packets carried between a client and a peer, either way.
    pub relayed: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct Allocation {
    relay: UdpSocket,
    permitted: HashSet<IpAddr>,
    channels: HashMap<u16, SocketAddr>,
}

impl FakeTurn {
    /// A server that lets `user` allocate.
    pub fn start(user: &str) -> FakeTurn {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let addr = socket.local_addr().unwrap();
        let (stop, relayed) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicUsize::new(0)),
        );
        let (s, r, user) = (stop.clone(), relayed.clone(), user.to_owned());
        let thread = std::thread::spawn(move || serve(socket, &user, &s, &r));
        FakeTurn {
            addr,
            relayed,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for FakeTurn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn serve(socket: UdpSocket, user: &str, stop: &AtomicBool, relayed: &AtomicUsize) {
    let mut allocations: HashMap<SocketAddr, Allocation> = HashMap::new();
    let mut buf = [0u8; 2048];
    while !stop.load(Ordering::Relaxed) {
        let mut busy = false;
        while let Ok((n, client)) = socket.recv_from(&mut buf) {
            busy = true;
            let data = &buf[..n];
            if ChannelData::is_channel_data(data) {
                let mut cd = ChannelData {
                    raw: data.to_vec(),
                    ..Default::default()
                };
                if cd.decode().is_ok()
                    && let Some(a) = allocations.get(&client)
                    && let Some(peer) = a.channels.get(&cd.number.0)
                {
                    let _ = a.relay.send_to(&cd.data, peer);
                    relayed.fetch_add(1, Ordering::Relaxed);
                }
                continue;
            }
            let mut msg = Message::new();
            msg.raw = data.to_vec();
            if msg.decode().is_err() {
                continue;
            }
            if let Some(reply) = answer(&msg, client, user, &mut allocations, relayed) {
                let _ = socket.send_to(&reply, client);
            }
        }
        for (client, a) in &allocations {
            while let Ok((n, from)) = a.relay.recv_from(&mut buf) {
                busy = true;
                if !a.permitted.contains(&from.ip()) {
                    continue;
                }
                relayed.fetch_add(1, Ordering::Relaxed);
                let channel = a
                    .channels
                    .iter()
                    .find(|(_, p)| **p == from)
                    .map(|(n, _)| *n);
                let out = match channel {
                    Some(number) => {
                        let mut cd = ChannelData {
                            data: buf[..n].to_vec(),
                            number: ChannelNumber(number),
                            raw: Vec::new(),
                        };
                        cd.encode();
                        cd.raw
                    }
                    None => build(
                        MessageType::new(METHOD_DATA, CLASS_INDICATION),
                        None,
                        vec![
                            Box::new(PeerAddress {
                                ip: from.ip(),
                                port: from.port(),
                            }),
                            Box::new(Data(buf[..n].to_vec())),
                        ],
                    ),
                };
                let _ = socket.send_to(&out, client);
            }
        }
        if !busy {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// The reply to a request, if it needs one.
fn answer(
    msg: &Message,
    client: SocketAddr,
    user: &str,
    allocations: &mut HashMap<SocketAddr, Allocation>,
    relayed: &AtomicUsize,
) -> Option<Vec<u8>> {
    let success = |method| MessageType::new(method, CLASS_SUCCESS_RESPONSE);
    let id = Some(msg.transaction_id);
    let peer = || {
        let mut p = PeerAddress::default();
        p.get_from(msg).ok().map(|()| SocketAddr::new(p.ip, p.port))
    };
    match (msg.typ.method, msg.typ.class) {
        (METHOD_ALLOCATE, CLASS_REQUEST) => {
            let named =
                TextAttribute::get_from_as(msg, ATTR_USERNAME).is_ok_and(|n| n.text == user);
            if !named {
                return Some(build(
                    MessageType::new(METHOD_ALLOCATE, CLASS_ERROR_RESPONSE),
                    id,
                    vec![
                        Box::new(ErrorCodeAttribute {
                            code: CODE_UNAUTHORIZED,
                            reason: b"Unauthorized".to_vec(),
                        }),
                        Box::new(TextAttribute::new(ATTR_REALM, "bingle.test".into())),
                        Box::new(TextAttribute::new(ATTR_NONCE, "fake-nonce".into())),
                    ],
                ));
            }
            let relay = UdpSocket::bind("127.0.0.1:0").ok()?;
            relay.set_nonblocking(true).ok()?;
            let at = relay.local_addr().ok()?;
            allocations.insert(
                client,
                Allocation {
                    relay,
                    permitted: HashSet::new(),
                    channels: HashMap::new(),
                },
            );
            Some(build(
                success(METHOD_ALLOCATE),
                id,
                vec![
                    Box::new(RelayedAddress {
                        ip: at.ip(),
                        port: at.port(),
                    }),
                    Box::new(XorMappedAddress {
                        ip: client.ip(),
                        port: client.port(),
                    }),
                    Box::new(Lifetime(Duration::from_secs(600))),
                ],
            ))
        }
        (METHOD_REFRESH, CLASS_REQUEST) => Some(build(
            success(METHOD_REFRESH),
            id,
            vec![Box::new(Lifetime(Duration::from_secs(600)))],
        )),
        (METHOD_CREATE_PERMISSION, CLASS_REQUEST) => {
            let a = allocations.get_mut(&client)?;
            // Every XOR-PEER-ADDRESS it carries; the first is enough for a test.
            a.permitted.insert(peer()?.ip());
            Some(build(success(METHOD_CREATE_PERMISSION), id, vec![]))
        }
        (METHOD_CHANNEL_BIND, CLASS_REQUEST) => {
            let a = allocations.get_mut(&client)?;
            let mut number = ChannelNumber::default();
            number.get_from(msg).ok()?;
            let peer = peer()?;
            a.permitted.insert(peer.ip());
            a.channels.insert(number.0, peer);
            Some(build(success(METHOD_CHANNEL_BIND), id, vec![]))
        }
        (METHOD_SEND, CLASS_INDICATION) => {
            let a = allocations.get(&client)?;
            let peer = peer()?;
            let mut data = Data::default();
            data.get_from(msg).ok()?;
            if a.permitted.contains(&peer.ip()) {
                let _ = a.relay.send_to(&data.0, peer);
                relayed.fetch_add(1, Ordering::Relaxed);
            }
            None
        }
        _ => None,
    }
}

fn build(typ: MessageType, id: Option<TransactionId>, mut attrs: Vec<Box<dyn Setter>>) -> Vec<u8> {
    let mut setters: Vec<Box<dyn Setter>> = vec![Box::new(typ)];
    setters.push(Box::new(id.unwrap_or_default()));
    setters.append(&mut attrs);
    setters.push(Box::new(FINGERPRINT));
    let mut msg = Message::new();
    let _ = msg.build(&setters);
    msg.raw
}
