//! Calls between friends (stage 8): ring, answer, hang up and mute, on the friends panel, with the
//! voice engine (`crate::voice`) carrying the sound.
//!
//! It follows Bingle's page (`js/dm-call.js`, `docs/voice.md`), so a browser can be on the other
//! end. Ringing is `POST /dm/call` with `join`, and so is answering: the call is on once both have
//! joined, the server telling each with a `dm-call` ring on the account's socket. Then the lower
//! of the two addresses offers, and the WebRTC messages go through `/dm/signal` and come back as
//! `dm-rtc` events. Hanging up is `join` false, which the other end hears as a `dm-call` end.
//!
//! [`Call`] is where a call stands, with no Bevy and no sound in it; [`DirectCalling`] holds it and
//! the engine, and turns what it decides into requests.

use std::time::Duration;

use bevy::prelude::*;

use crate::net::api::{DirectCall, DirectSignal, IceServer};
use crate::net::session::Problem;
use crate::net::{Connection, Net, Request, ServerStatus};
use crate::voice::devices::Choice;
use crate::voice::{self, Command, Engine, Event, PeerKey, PeerState, Recovery, Role, Setup};

/// How long a call rings unanswered before it is given up. The server says nothing when the
/// other end declines, so this is how the caller learns of it.
pub const RING_FOR: Duration = Duration::from_secs(60);

pub struct CallPlugin {
    /// Open no microphone and no speaker (`--no-audio-devices`): for tests and scripts.
    pub no_devices: bool,
    /// Ring this friend, by handle, once the list shows them here (`--call`).
    pub call: Option<String>,
    /// Connect only through the server's relay (`--relay-only`).
    pub relay_only: bool,
}

impl Plugin for CallPlugin {
    fn build(&self, app: &mut App) {
        let device = if self.no_devices {
            Choice::Off
        } else {
            Choice::Default
        };
        app.insert_resource(DirectCalling {
            microphone: device.clone(),
            speaker: device,
            relay_only: self.relay_only,
            ..default()
        })
        .insert_resource(CallOnStart(self.call.clone()))
        .add_systems(Update, (follow_account, call_on_start, drive).chain());
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    /// Rung, not answered yet.
    Calling,
    /// Somebody rings.
    Ringing,
    /// Both have joined: connecting, then talking.
    Live,
}

/// Where a call stands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Call {
    pub phase: Phase,
    /// The other end's address.
    pub peer: String,
    /// What they go by ("@bob").
    pub name: String,
    pub muted: bool,
    /// The connection is up: sound flows.
    pub connected: bool,
    /// They are speaking.
    pub speaking: bool,
    /// Why the last call went wrong, worded for the player.
    pub error: Option<String>,
    /// This end answered, and starts once the server has taken the answer.
    answering: bool,
    /// How long it has rung unanswered.
    rung: Duration,
}

/// What a step of a call asks to be done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// `POST /dm/call` with `join`: ring this address, or answer it.
    Ring(String),
    /// `POST /dm/call` without: leave the call.
    HangUp,
    /// Both have joined: start the connection.
    Start,
    /// Stop the connection.
    Stop,
}

impl Call {
    /// The player rings a friend, or answers them when they are ringing.
    pub fn ring(&mut self, address: &str, name: &str) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.phase != Phase::Idle && self.peer != address {
            // The server takes this account out of the old call as it joins the new one.
            effects.push(Effect::Stop);
            self.reset();
        }
        if self.phase == Phase::Live || self.phase == Phase::Calling {
            return effects;
        }
        let answering = self.phase == Phase::Ringing;
        self.peer = address.to_string();
        if !name.is_empty() {
            self.name = name.to_string();
        }
        self.error = None;
        self.phase = if answering {
            Phase::Live
        } else {
            Phase::Calling
        };
        self.answering = answering;
        self.rung = Duration::ZERO;
        effects.push(Effect::Ring(address.to_string()));
        effects
    }

    /// The server took a ring or an answer, or refused it.
    pub fn rang(&mut self, to: &str, result: Result<(), String>) -> Vec<Effect> {
        if to != self.peer || self.phase == Phase::Idle {
            return Vec::new();
        }
        match result {
            Ok(()) if self.answering && self.phase == Phase::Live => {
                self.answering = false;
                vec![Effect::Start]
            }
            Ok(()) => Vec::new(),
            Err(why) => {
                self.reset();
                self.error = Some(why);
                vec![Effect::Stop]
            }
        }
    }

    /// The account's socket says somebody rings, or has answered.
    pub fn heard_ring(&mut self, from: &str, name: &str) -> Vec<Effect> {
        if self.phase != Phase::Idle && self.peer != from {
            // Busy with somebody else: they ring until they give up.
            return Vec::new();
        }
        if !name.is_empty() {
            self.name = name.to_string();
        }
        match self.phase {
            Phase::Idle => {
                self.peer = from.to_string();
                self.phase = Phase::Ringing;
                self.error = None;
                Vec::new()
            }
            Phase::Calling => {
                self.phase = Phase::Live;
                vec![Effect::Start]
            }
            Phase::Ringing | Phase::Live => Vec::new(),
        }
    }

    /// The player hangs up (or gives up ringing).
    pub fn hang_up(&mut self) -> Vec<Effect> {
        if self.phase == Phase::Idle {
            return Vec::new();
        }
        let joined = self.phase != Phase::Ringing;
        self.reset();
        self.error = None;
        if joined {
            vec![Effect::Stop, Effect::HangUp]
        } else {
            // Declined: this end never joined, so there is nothing to leave.
            Vec::new()
        }
    }

    /// The other end hung up, or the server ended the call.
    pub fn ended(&mut self, from: &str, dropped: bool) -> Vec<Effect> {
        if self.phase == Phase::Idle || from != self.peer {
            return Vec::new();
        }
        self.reset();
        self.error = dropped.then(|| "The call ended: they joined another one.".to_string());
        vec![Effect::Stop]
    }

    /// Time passes: a call not answered in [`RING_FOR`] is given up.
    pub fn tick(&mut self, delta: Duration) -> Vec<Effect> {
        if self.phase != Phase::Calling {
            return Vec::new();
        }
        self.rung += delta;
        if self.rung < RING_FOR {
            return Vec::new();
        }
        let effects = self.hang_up();
        self.error = Some("No answer.".to_string());
        effects
    }

    fn reset(&mut self) {
        *self = Call {
            error: self.error.take(),
            ..Call::default()
        };
    }
}

/// Which end offers: the lower of the two addresses, compared as strings.
pub fn role(me: &str, peer: &str) -> Role {
    if me < peer {
        Role::Offers
    } else {
        Role::Answers
    }
}

/// The friend call, and the connection carrying it. By default it opens no devices: only the
/// plugin chooses them, so no test ever opens a real microphone.
#[derive(Resource)]
pub struct DirectCalling {
    pub call: Call,
    engine: Option<Engine>,
    /// Asked for the ICE servers, to start once they come.
    starting: bool,
    /// Messages from the other end that came before the connection started.
    pending: Vec<voice::Signal>,
    microphone: Choice,
    speaker: Choice,
    relay_only: bool,
}

impl Default for DirectCalling {
    fn default() -> Self {
        DirectCalling {
            call: Call::default(),
            engine: None,
            starting: false,
            pending: Vec::new(),
            microphone: Choice::Off,
            speaker: Choice::Off,
            relay_only: false,
        }
    }
}

impl DirectCalling {
    /// Does what a step of the call asked.
    fn apply(&mut self, net: &Net, effects: Vec<Effect>, peer: &str) {
        for effect in effects {
            match effect {
                Effect::Ring(to) => net.send(Request::Call { to, join: true }),
                Effect::HangUp => net.send(Request::Call {
                    to: peer.to_string(),
                    join: false,
                }),
                Effect::Start => {
                    // TURN credentials expire, so the ICE servers are read for each call.
                    self.starting = true;
                    net.send(Request::IceServers);
                }
                Effect::Stop => {
                    self.engine = None;
                    self.starting = false;
                    self.pending.clear();
                }
            }
        }
    }

    pub fn ring(&mut self, net: &Net, address: &str, name: &str) {
        let peer = self.call.peer.clone();
        let effects = self.call.ring(address, name);
        self.apply(net, effects, &peer);
    }

    pub fn hang_up(&mut self, net: &Net) {
        let peer = self.call.peer.clone();
        let effects = self.call.hang_up();
        self.apply(net, effects, &peer);
    }

    pub fn toggle_mute(&mut self) {
        self.call.muted = !self.call.muted;
        if let Some(engine) = &self.engine {
            engine.send(Command::Mute(self.call.muted));
        }
    }

    /// The server answered a ring, an answer or a hang-up.
    pub fn called(&mut self, net: &Net, to: &str, join: bool, result: Result<(), Problem>) {
        if !join {
            return;
        }
        let peer = self.call.peer.clone();
        let effects = self.call.rang(to, result.map_err(|p| p.message()));
        self.apply(net, effects, &peer);
    }

    /// A `dm-call` on the account's socket.
    pub fn event(&mut self, net: &Net, event: DirectCall) {
        let peer = self.call.peer.clone();
        let effects = match event.kind.as_str() {
            "ring" => self.call.heard_ring(&event.from, &event.name),
            "end" => self.call.ended(&event.from, event.dropped),
            _ => Vec::new(),
        };
        self.apply(net, effects, &peer);
    }

    /// A `dm-rtc` on the account's socket.
    pub fn signal(&mut self, signal: DirectSignal) {
        if self.call.phase != Phase::Live || signal.from != self.call.peer {
            return;
        }
        let signal = match voice::Signal::read(&signal.kind, &signal.data) {
            Ok(s) => s,
            Err(e) => {
                warn!("the call: {e}");
                return;
            }
        };
        match &self.engine {
            Some(engine) => engine.send(Command::Signal {
                peer: PeerKey(self.call.peer.clone()),
                signal,
            }),
            None => self.pending.push(signal),
        }
    }

    /// The ICE servers came: the connection starts.
    pub fn ice_servers(
        &mut self,
        net: &Net,
        me: Option<&str>,
        result: Result<Vec<IceServer>, Problem>,
    ) {
        if !self.starting || self.call.phase != Phase::Live {
            return;
        }
        self.starting = false;
        let servers = result.unwrap_or_else(|problem| {
            // Host candidates may still do, on one network.
            warn!("the ICE servers: {}", problem.message());
            Vec::new()
        });
        let Some(me) = me else {
            // The account's socket says the address as it opens; without it, who offers is
            // not known.
            self.call.error = Some("The call could not start: try again in a moment.".into());
            self.hang_up(net);
            return;
        };
        let engine = Engine::start(Setup {
            ice_servers: servers
                .into_iter()
                .map(|s| voice::IceServer {
                    urls: s.urls,
                    username: s.username,
                    credential: s.credential,
                })
                .collect(),
            loopback: false,
            relay_only: self.relay_only,
            microphone: self.microphone.clone(),
            speaker: self.speaker.clone(),
        });
        let peer = PeerKey(self.call.peer.clone());
        if self.call.muted {
            engine.send(Command::Mute(true));
        }
        engine.send(Command::Add {
            peer: peer.clone(),
            role: role(me, &self.call.peer),
            recovery: Recovery::OnRequest,
        });
        for signal in self.pending.drain(..) {
            engine.send(Command::Signal {
                peer: peer.clone(),
                signal,
            });
        }
        info!("the call with {} starts", self.call.name);
        self.engine = Some(engine);
    }
}

/// Signed out, a call ends with the session (the server forgets the account's call with it).
fn follow_account(status: Res<ServerStatus>, mut calling: ResMut<DirectCalling>) {
    if !status.is_changed() {
        return;
    }
    let signed_in = matches!(&status.connection, Connection::Ready(s) if s.account.is_some());
    if !signed_in && calling.call.phase != Phase::Idle {
        calling.engine = None;
        calling.starting = false;
        calling.pending.clear();
        calling.call = Call::default();
    }
}

/// The friend `--call` named, until the list shows them here (the server rings only a friend
/// who is).
#[derive(Resource)]
struct CallOnStart(Option<String>);

fn call_on_start(
    mut pending: ResMut<CallOnStart>,
    net: Res<Net>,
    friends: Res<crate::friends::Friends>,
    mut calling: ResMut<DirectCalling>,
) {
    let Some(handle) = &pending.0 else { return };
    // Who offers depends on this account's address, which its socket gives.
    if friends.address.is_none() {
        return;
    }
    let Some(friend) = friends.list.as_ref().and_then(|l| {
        l.friends.iter().find(|x| {
            x.online
                && x.handle
                    .eq_ignore_ascii_case(handle.trim_start_matches('@'))
        })
    }) else {
        return;
    };
    let (address, name) = (friend.address.clone(), friend.name.clone());
    pending.0 = None;
    calling.ring(&net, &address, &name);
}

/// What the connection says: messages for the other end, whether it is up, who speaks. The
/// resource is only marked changed when something shown changed.
fn drive(time: Res<Time>, net: Res<Net>, mut calling: ResMut<DirectCalling>) {
    let before = calling.call.clone();
    let c = calling.bypass_change_detection();
    let peer = c.call.peer.clone();
    let effects = c.call.tick(time.delta());
    c.apply(&net, effects, &peer);
    let events = c.engine.as_ref().map(Engine::events).unwrap_or_default();
    for event in events {
        match event {
            Event::Signal { signal, .. } => net.send(Request::Signal {
                to: c.call.peer.clone(),
                kind: signal.kind().to_string(),
                data: signal.data(),
            }),
            Event::State { state, .. } => {
                info!("the call with {}: {state:?}", c.call.name);
                c.call.connected = state == PeerState::Connected;
                if state == PeerState::Failed {
                    c.call.error = Some("The call could not be connected.".into());
                } else if state == PeerState::Connected {
                    c.call.error = None;
                }
            }
            Event::Speaking {
                peer: Some(_),
                speaking,
            } => c.call.speaking = speaking,
            Event::Speaking { peer: None, .. } => {}
            Event::Problem(problem) => {
                warn!("the call: {problem}");
                c.call.error = Some(problem);
            }
        }
    }
    // Ringing time counts in `Call`, but is not something shown.
    let mut now = c.call.clone();
    now.rung = before.rung;
    if now != before {
        calling.set_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_rung_starts_when_they_answer() {
        let mut c = Call::default();
        assert_eq!(c.ring("b", "@bob"), [Effect::Ring("b".into())]);
        assert_eq!(c.phase, Phase::Calling);
        assert_eq!(c.rang("b", Ok(())), []);
        // Their answer is a ring on the account's socket.
        assert_eq!(c.heard_ring("b", "@bob"), [Effect::Start]);
        assert_eq!(c.phase, Phase::Live);
        assert_eq!(c.hang_up(), [Effect::Stop, Effect::HangUp]);
        assert_eq!(c.phase, Phase::Idle);
    }

    #[test]
    fn an_answer_starts_once_the_server_has_taken_it() {
        let mut c = Call::default();
        assert_eq!(c.heard_ring("a", "@ana"), []);
        assert_eq!((c.phase, c.name.as_str()), (Phase::Ringing, "@ana"));
        assert_eq!(c.ring("a", ""), [Effect::Ring("a".into())]);
        assert_eq!(c.phase, Phase::Live);
        // Signals before both have joined are refused, so not before.
        assert_eq!(c.rang("a", Ok(())), [Effect::Start]);
        assert_eq!(c.rang("a", Ok(())), [], "once");
    }

    #[test]
    fn a_ring_refused_says_why() {
        let mut c = Call::default();
        c.ring("b", "@bob");
        assert_eq!(c.rang("b", Err("@bob is not here.".into())), [Effect::Stop]);
        assert_eq!(c.phase, Phase::Idle);
        assert_eq!(c.error.as_deref(), Some("@bob is not here."));
    }

    #[test]
    fn declining_tells_nobody_and_a_call_unanswered_is_given_up() {
        let mut c = Call::default();
        c.heard_ring("a", "@ana");
        assert_eq!(c.hang_up(), [], "never joined, nothing to leave");
        assert_eq!(c.phase, Phase::Idle);

        c.ring("b", "@bob");
        assert_eq!(c.tick(RING_FOR / 2), []);
        assert_eq!(c.tick(RING_FOR / 2), [Effect::Stop, Effect::HangUp]);
        assert_eq!(c.error.as_deref(), Some("No answer."));
    }

    #[test]
    fn while_busy_another_ring_waits_and_an_end_from_elsewhere_is_ignored() {
        let mut c = Call::default();
        c.ring("b", "@bob");
        c.heard_ring("b", "@bob");
        assert_eq!(c.heard_ring("x", "@xi"), []);
        assert_eq!(c.peer, "b");
        assert_eq!(c.ended("x", false), []);
        assert_eq!(c.ended("b", true), [Effect::Stop]);
        assert!(c.error.as_deref().unwrap().contains("another"));
    }

    #[test]
    fn ringing_somebody_else_leaves_the_call() {
        let mut c = Call::default();
        c.ring("b", "@bob");
        c.heard_ring("b", "@bob");
        assert_eq!(c.ring("x", "@xi"), [Effect::Stop, Effect::Ring("x".into())]);
        assert_eq!((c.phase, c.peer.as_str()), (Phase::Calling, "x"));
    }

    #[test]
    fn a_call_asks_the_server_and_starts_a_connection_that_offers() {
        let (net, inbox) = Net::for_test();
        let mut calling = DirectCalling::default();
        calling.ring(&net, "b", "@bob");
        assert!(matches!(
            inbox.try_recv(),
            Ok(Request::Call { to, join: true }) if to == "b"
        ));
        calling.called(&net, "b", true, Ok(()));
        // An offer that comes early (it cannot, before both joined, but a candidate may) waits.
        calling.event(
            &net,
            DirectCall {
                kind: "ring".into(),
                from: "b".into(),
                name: "@bob".into(),
                dropped: false,
            },
        );
        assert!(matches!(inbox.try_recv(), Ok(Request::IceServers)));
        calling.signal(DirectSignal {
            from: "b".into(),
            kind: "ice".into(),
            data: serde_json::json!({"candidate": "", "sdpMid": "0", "sdpMLineIndex": 0}),
        });
        assert_eq!(calling.pending.len(), 1);
        calling.ice_servers(&net, Some("a"), Ok(Vec::new()));
        assert!(calling.pending.is_empty());
        let engine = calling.engine.as_ref().expect("the connection started");
        // "a" is the lower address: it offers.
        let offered = (0..100).any(|_| {
            matches!(
                engine.next_event(Duration::from_millis(50)),
                Some(Event::Signal {
                    signal: voice::Signal::Offer(_),
                    ..
                })
            )
        });
        assert!(offered, "the connection offers");
        calling.hang_up(&net);
        assert!(calling.engine.is_none());
        assert!(matches!(
            inbox.try_recv(),
            Ok(Request::Call { join: false, .. })
        ));
    }

    #[test]
    fn the_lower_address_offers() {
        assert_eq!(role("00aa", "00ab"), Role::Offers);
        assert_eq!(role("00ab", "00aa"), Role::Answers);
    }
}
