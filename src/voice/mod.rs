//! Voice: a call with the people of a game, or with a friend, that a browser on Bingle's page can
//! be part of (stage 8 of docs/plan.md).
//!
//! The call runs on **a thread of its own**, the one exception to all network I/O being on the
//! network thread: sound must leave every 20 ms, and the network thread's blocking calls would
//! hold it up. That thread owns one UDP socket per person, rtc's peer connections (WebRTC with no
//! I/O of its own), and Opus. The microphone and the speaker are cpal's, on their own audio
//! threads, and meet the voice thread through two buffers ([`audio::Capture`] and
//! [`audio::Mixer`]).
//!
//! The engine knows nothing of Bingle: it is told who is in the call and what the other ends
//! signal ([`Command`]), and says what to signal to them ([`Event`]). Who carries those messages,
//! and who offers to whom, is the caller's: a game's call or a call between friends.

pub mod audio;
pub mod check;
pub mod devices;
mod echo;
mod peer;
pub mod signal;

use std::collections::HashMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bevy::log::{info, warn};
use rusty_opus::{Application, OpusEncoder};

use audio::{Capture, FRAME, Mixer, RATE, SharedMixer, Speaking, rms};
use devices::Choice;
use echo::Cleaner;
use peer::{Network, Peer};
pub use signal::Signal;

/// The longest the voice thread sleeps: how late a packet that has arrived may be read.
const TICK: Duration = Duration::from_millis(5);

/// Who a connection is with: a seat, or a friend's address, as the caller names them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerKey(pub String);

impl fmt::Display for PeerKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which end of a connection makes the offer. The caller decides, by the page's rules: in a game
/// the higher seat offers, between friends the lower address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Offers,
    Answers,
}

/// What happens after a connection fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// A game's call: the side that offers restarts ICE, a few times; the other asks it to.
    Restart,
    /// A call between friends: nothing by itself, but a `retry` from the other end is answered.
    OnRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    Connecting,
    Connected,
    Failed,
    Closed,
}

/// One of `GET /api/rtc/config`'s servers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

/// How a call is set up.
#[derive(Debug, Clone)]
pub struct Setup {
    pub ice_servers: Vec<IceServer>,
    /// Both ends on this machine: sockets on loopback (tests and `--voice-check`).
    pub loopback: bool,
    pub microphone: Choice,
    pub speaker: Choice,
}

/// What the voice thread is told.
#[derive(Debug, Clone)]
pub enum Command {
    /// Somebody is in the call: connect to them.
    Add {
        peer: PeerKey,
        role: Role,
        recovery: Recovery,
    },
    /// What the other end of a connection sent.
    Signal { peer: PeerKey, signal: Signal },
    /// Somebody left the call.
    Remove { peer: PeerKey },
    /// Muted, nothing is sent.
    Mute(bool),
    /// Deafened, nothing is heard and nothing is sent.
    Deafen(bool),
    /// One person's volume, 1.0 being as they come.
    Volume { peer: PeerKey, volume: f32 },
    /// Opens other devices.
    Devices { microphone: Choice, speaker: Choice },
}

/// What the voice thread says.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// To send to the other end of a connection.
    Signal {
        peer: PeerKey,
        signal: Signal,
    },
    State {
        peer: PeerKey,
        state: PeerState,
    },
    /// Somebody started or stopped speaking: a peer, or this end (`None`).
    Speaking {
        peer: Option<PeerKey>,
        speaking: bool,
    },
    /// Something the player should know, worded for them (a microphone that would not open).
    Problem(String),
}

/// A call in progress: a handle on its thread, which ends when this is dropped. It can be shared
/// (a Bevy resource), so its receiver is behind a lock.
pub struct Engine {
    commands: Option<Sender<Command>>,
    events: Mutex<Receiver<Event>>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Starts a call with the devices `setup` chooses.
    pub fn start(setup: Setup) -> Engine {
        Engine::start_with(setup, Capture::default(), Mixer::shared())
    }

    /// Starts a call whose sound goes through these buffers; with both devices off, a test feeds
    /// one and drains the other.
    pub fn start_with(setup: Setup, capture: Capture, mixer: SharedMixer) -> Engine {
        let (commands, commanded) = mpsc::channel();
        let (said, events) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("voice".into())
            .spawn(move || run(setup, capture, mixer, commanded, said))
            .expect("could not start the voice thread");
        Engine {
            commands: Some(commands),
            events: Mutex::new(events),
            thread: Some(thread),
        }
    }

    pub fn send(&self, command: Command) {
        if let Some(c) = &self.commands {
            let _ = c.send(command);
        }
    }

    /// What the thread has said since last asked.
    pub fn events(&self) -> Vec<Event> {
        audio::lock(&self.events).try_iter().collect()
    }

    /// Waits up to `timeout` for the thread's next event.
    #[cfg(test)]
    pub fn next_event(&self, timeout: Duration) -> Option<Event> {
        audio::lock(&self.events).recv_timeout(timeout).ok()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Closing the channel ends the thread's loop; it closes its connections on the way out.
        self.commands = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Call {
    net: Network,
    peers: HashMap<PeerKey, Peer>,
    mixer: SharedMixer,
    muted: bool,
    deafened: bool,
    /// What the microphone hears is cleaned while there is a microphone (`echo`); what is pushed
    /// into the capture by other means (the tests) goes as it is.
    cleaner: Option<Cleaner>,
    out: Vec<Event>,
}

fn run(
    setup: Setup,
    capture: Capture,
    mixer: SharedMixer,
    commands: Receiver<Command>,
    events: Sender<Event>,
) {
    let mut call = Call {
        net: network(&setup),
        peers: HashMap::new(),
        mixer,
        muted: false,
        deafened: false,
        cleaner: cleaner_for(&setup.microphone),
        out: Vec::new(),
    };
    let mut streams = devices::open(
        &setup.microphone,
        &setup.speaker,
        &capture,
        &call.mixer,
        &events,
    );
    let mut encoder = match OpusEncoder::new(RATE as i32, 1, Application::Voip) {
        Ok(e) => e,
        Err(e) => {
            let _ = events.send(Event::Problem(format!("Voice chat could not start: {e:?}")));
            return;
        }
    };
    let mut frame = [0.0; FRAME];
    let mut played = Vec::new();
    let mut packet = [0u8; 1500];
    let mut frames: u32 = 0;
    let mut me = Speaking::default();
    loop {
        let now = Instant::now();
        let wake = call
            .peers
            .values_mut()
            .filter_map(Peer::next_wake)
            .min()
            .map_or(TICK, |at| at.saturating_duration_since(now).min(TICK));
        match commands.recv_timeout(wake) {
            Ok(c) => {
                if let Some(s) = call.command(c, &capture, &events) {
                    streams = s;
                }
                while let Ok(c) = commands.try_recv() {
                    if let Some(s) = call.command(c, &capture, &events) {
                        streams = s;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        for peer in call.peers.values_mut() {
            peer.drive(now, &call.mixer, &mut call.out);
        }
        // What the speaker played first, so the canceller knows it before it hears it back.
        audio::lock(&call.mixer).take_played(&mut played);
        if let Some(c) = &mut call.cleaner {
            c.played(&played);
        }
        // Whatever the microphone has caught, a frame at a time, paced by its clock.
        while capture.take_frame(&mut frame) {
            frames = frames.wrapping_add(1);
            // Cleaned even when muted, so the canceller keeps up with the room.
            if let Some(c) = &mut call.cleaner {
                c.clean(&mut frame);
            }
            let silent = call.muted || call.deafened;
            if let Some(speaking) = me.hear(if silent { 0.0 } else { rms(&frame) }) {
                call.out.push(Event::Speaking {
                    peer: None,
                    speaking,
                });
            }
            let anybody = call.peers.values().any(Peer::connected);
            let encoded = if silent || !anybody {
                None
            } else {
                encoder.encode(&frame, FRAME, &mut packet).ok()
            };
            for peer in call.peers.values_mut() {
                match encoded {
                    Some(n) => peer.send_audio(&packet[..n], frames.wrapping_mul(FRAME as u32)),
                    None => peer.silent(),
                }
            }
        }
        for peer in call.peers.values_mut() {
            peer.drive(now, &call.mixer, &mut call.out);
        }
        for e in call.out.drain(..) {
            let _ = events.send(e);
        }
    }
    for peer in call.peers.values_mut() {
        peer.close();
    }
    drop(streams);
}

impl Call {
    /// Carries out a command; new devices, when it opened them.
    fn command(
        &mut self,
        command: Command,
        capture: &Capture,
        events: &Sender<Event>,
    ) -> Option<devices::Streams> {
        match command {
            Command::Add {
                peer,
                role,
                recovery,
            } => {
                if self.peers.contains_key(&peer) {
                    return None;
                }
                match Peer::new(peer.clone(), role, recovery, &self.net) {
                    Ok(mut p) => {
                        p.start(&mut self.out);
                        self.peers.insert(peer, p);
                    }
                    Err(e) => self.out.push(Event::Problem(e)),
                }
            }
            Command::Signal { peer, signal } => match self.peers.get_mut(&peer) {
                Some(p) => p.handle(signal, &mut self.out),
                None => info!(
                    "voice: a {} from {peer}, who is not in the call",
                    signal.kind()
                ),
            },
            Command::Remove { peer } => {
                if let Some(mut p) = self.peers.remove(&peer) {
                    p.close();
                }
                audio::lock(&self.mixer).remove(&peer);
            }
            Command::Mute(m) => self.muted = m,
            Command::Deafen(d) => {
                self.deafened = d;
                audio::lock(&self.mixer).set_deafened(d);
            }
            Command::Volume { peer, volume } => {
                audio::lock(&self.mixer).set_volume(&peer, volume);
            }
            Command::Devices {
                microphone,
                speaker,
            } => {
                self.cleaner = cleaner_for(&microphone);
                return Some(devices::open(
                    &microphone,
                    &speaker,
                    capture,
                    &self.mixer,
                    events,
                ));
            }
        }
        None
    }
}

/// The sockets' address and the STUN servers, looked up once at the start of a call. TURN relays
/// are not used yet.
fn cleaner_for(microphone: &Choice) -> Option<Cleaner> {
    (*microphone != Choice::Off).then(Cleaner::new)
}

fn network(setup: &Setup) -> Network {
    if setup.loopback {
        return Network {
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            loopback: true,
            stun: Vec::new(),
        };
    }
    let stun = stun_servers(&setup.ice_servers)
        .into_iter()
        .filter_map(|(host, url)| {
            let addr = host
                .to_socket_addrs()
                .ok()?
                .find(SocketAddr::is_ipv4)
                .or_else(|| {
                    warn!("voice: the STUN server {host} has no IPv4 address");
                    None
                })?;
            Some((addr, url))
        })
        .collect();
    Network {
        ip: host_ip().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        loopback: false,
        stun,
    }
}

/// The address this machine reaches the internet from: the one other machines can be told.
/// Connecting a UDP socket sends nothing; it only picks the route.
fn host_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    Some(s.local_addr().ok()?.ip())
}

/// The STUN servers among the ICE servers, each as `host:port` and the URL it came as.
fn stun_servers(servers: &[IceServer]) -> Vec<(String, String)> {
    servers
        .iter()
        .flat_map(|s| &s.urls)
        .filter_map(|url| {
            let rest = url.strip_prefix("stun:")?;
            let rest = rest.split('?').next().unwrap_or(rest);
            let host = if rest.starts_with('[') || rest.rsplit_once(':').is_some() {
                rest.to_owned()
            } else {
                format!("{rest}:3478")
            };
            Some((host, url.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loopback() -> Setup {
        Setup {
            ice_servers: Vec::new(),
            loopback: true,
            microphone: Choice::Off,
            speaker: Choice::Off,
        }
    }

    #[test]
    fn stun_servers_are_found_among_the_ice_servers() {
        let servers = [
            IceServer {
                urls: vec![
                    "stun:stun.example.com".into(),
                    "stun:192.0.2.1:19302?transport=udp".into(),
                ],
                ..Default::default()
            },
            IceServer {
                urls: vec!["turn:turn.example.com:3478".into()],
                username: Some("u".into()),
                credential: Some("c".into()),
            },
        ];
        let found = stun_servers(&servers);
        assert_eq!(
            found,
            [
                (
                    "stun.example.com:3478".into(),
                    "stun:stun.example.com".into()
                ),
                (
                    "192.0.2.1:19302".into(),
                    "stun:192.0.2.1:19302?transport=udp".into()
                ),
            ]
        );
    }

    /// Two engines, `a` offering, joined through this test the way the server would carry their
    /// messages; until both say they are connected.
    fn connect(a: &Engine, b: &Engine, recovery: Recovery) {
        a.send(Command::Add {
            peer: PeerKey("b".into()),
            role: Role::Offers,
            recovery,
        });
        b.send(Command::Add {
            peer: PeerKey("a".into()),
            role: Role::Answers,
            recovery,
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut a_up, mut b_up) = (false, false);
        while !(a_up && b_up) {
            assert!(Instant::now() < deadline, "the two did not connect");
            relay(a, b, &mut a_up, &mut b_up);
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn relay(a: &Engine, b: &Engine, a_up: &mut bool, b_up: &mut bool) {
        for e in a.events() {
            match e {
                Event::Signal { signal, .. } => b.send(Command::Signal {
                    peer: PeerKey("a".into()),
                    signal: through_json(signal),
                }),
                Event::State { state, .. } => *a_up = state == PeerState::Connected,
                _ => {}
            }
        }
        for e in b.events() {
            match e {
                Event::Signal { signal, .. } => a.send(Command::Signal {
                    peer: PeerKey("b".into()),
                    signal: through_json(signal),
                }),
                Event::State { state, .. } => *b_up = state == PeerState::Connected,
                _ => {}
            }
        }
    }

    /// As the server carries it: `kind` and `data`, as JSON.
    fn through_json(signal: Signal) -> Signal {
        let data = serde_json::to_string(&signal.data()).unwrap();
        Signal::read(signal.kind(), &serde_json::from_str(&data).unwrap()).unwrap()
    }

    /// Speaks a tone into `capture` for `frames` frames, in real time.
    fn speak(capture: &Capture, frames: usize) {
        let mut phase = 0.0f32;
        for _ in 0..frames {
            let tone: Vec<f32> = (0..FRAME)
                .map(|_| {
                    phase += 440.0 * std::f32::consts::TAU / RATE as f32;
                    0.3 * phase.sin()
                })
                .collect();
            capture.push(&tone);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn heard(mixer: &SharedMixer) -> f32 {
        let mut out = vec![0.0; RATE as usize];
        audio::lock(mixer).pull(&mut out);
        rms(&out)
    }

    #[test]
    fn two_engines_talk_over_loopback() {
        let (a_capture, a_mixer) = (Capture::default(), Mixer::shared());
        let (b_capture, b_mixer) = (Capture::default(), Mixer::shared());
        let a = Engine::start_with(loopback(), a_capture.clone(), a_mixer.clone());
        let b = Engine::start_with(loopback(), b_capture.clone(), b_mixer.clone());
        connect(&a, &b, Recovery::Restart);

        speak(&a_capture, 25);
        std::thread::sleep(Duration::from_millis(100));
        let level = heard(&b_mixer);
        assert!(level > 0.05, "b heard a's tone at RMS {level}");
        let spoke = b.events();
        assert!(
            spoke.contains(&Event::Speaking {
                peer: Some(PeerKey("a".into())),
                speaking: true
            }),
            "b saw a speak: {spoke:?}"
        );
        assert!(
            a.events().into_iter().any(|e| e
                == Event::Speaking {
                    peer: None,
                    speaking: true
                }),
            "a saw itself speak"
        );

        // And the other way, at half volume.
        b.send(Command::Volume {
            peer: PeerKey("a".into()),
            volume: 0.5,
        });
        speak(&b_capture, 25);
        std::thread::sleep(Duration::from_millis(100));
        assert!(heard(&a_mixer) > 0.05, "a heard b");

        // Muted, a sends nothing.
        a.send(Command::Mute(true));
        std::thread::sleep(Duration::from_millis(20));
        speak(&a_capture, 10);
        std::thread::sleep(Duration::from_millis(100));
        let level = heard(&b_mixer);
        assert!(level < 0.001, "b heard a muted at RMS {level}");

        // Removed, the connection closes, and b soon sees a has gone quiet.
        a.send(Command::Remove {
            peer: PeerKey("b".into()),
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(Instant::now() < deadline, "b did not see the call end");
            if let Some(Event::State { state, .. }) = b.next_event(Duration::from_millis(50))
                && state != PeerState::Connected
            {
                break;
            }
        }
    }

    #[test]
    fn deafened_nothing_is_heard_or_sent() {
        let (a_capture, a_mixer) = (Capture::default(), Mixer::shared());
        let (b_capture, b_mixer) = (Capture::default(), Mixer::shared());
        let a = Engine::start_with(loopback(), a_capture.clone(), a_mixer.clone());
        let b = Engine::start_with(loopback(), b_capture.clone(), b_mixer.clone());
        connect(&a, &b, Recovery::OnRequest);
        b.send(Command::Deafen(true));
        std::thread::sleep(Duration::from_millis(20));
        speak(&a_capture, 15);
        speak(&b_capture, 15);
        std::thread::sleep(Duration::from_millis(100));
        assert!(heard(&b_mixer) < 0.001, "b, deafened, heard a");
        assert!(heard(&a_mixer) < 0.001, "b, deafened, was heard");
    }

    #[test]
    fn a_retry_gets_a_new_connection() {
        let (a_capture, b_mixer) = (Capture::default(), Mixer::shared());
        let a = Engine::start_with(loopback(), a_capture.clone(), Mixer::shared());
        let b = Engine::start_with(loopback(), Capture::default(), b_mixer.clone());
        connect(&a, &b, Recovery::OnRequest);
        // b asks a to call again: a offers on a new connection, and b answers it.
        a.send(Command::Signal {
            peer: PeerKey("b".into()),
            signal: Signal::Retry,
        });
        let (mut a_up, mut b_up) = (true, true);
        let deadline = Instant::now() + Duration::from_secs(10);
        while a_up {
            assert!(Instant::now() < deadline, "a did not start again");
            relay(&a, &b, &mut a_up, &mut b_up);
            std::thread::sleep(Duration::from_millis(2));
        }
        while !a_up {
            assert!(
                Instant::now() < deadline,
                "the new connection did not come up"
            );
            relay(&a, &b, &mut a_up, &mut b_up);
            std::thread::sleep(Duration::from_millis(2));
        }
        speak(&a_capture, 25);
        std::thread::sleep(Duration::from_millis(100));
        assert!(heard(&b_mixer) > 0.05, "b hears a on the new connection");
    }
}
