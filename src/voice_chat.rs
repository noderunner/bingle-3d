//! A game's voice chat (stage 8): the players at the table talk, with Bingle's page and with other
//! clients, through the voice engine (`crate::voice`).
//!
//! It follows the page (`js/voice.js`, `docs/voice.md`). Joining opens the game's voice socket,
//! announces the seat (`POST /voice`, again every 15 s, on the network thread), and connects to
//! each seat in the call: one connection per pair, **the higher seat offering**. Who joins and
//! leaves comes on the voice socket (`voice` events) and with each heartbeat; the WebRTC messages
//! go through `/voice/signal` and come back as `rtc` events. After a failure the higher seat
//! restarts ICE and the lower asks it to (`Recovery::Restart`, in the engine).
//!
//! A person talks in one call at a time: joining takes the account out of a friend's call, and
//! a friend's call takes it out of this one, which the server says with a `voice` leave for this
//! seat.
//!
//! The panel's part is on the game's panel, for a player with a seat: who is in the call, each
//! with a volume; *Mute*, *Deafen*, push to talk (hold V), the devices, *Leave*.

use std::collections::{BTreeMap, HashMap};

use bevy::input::ButtonState;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::browser::{Action, button, buttons, chip, colours, line};
use crate::call::DirectCalling;
use crate::fonts::UiFonts;
use crate::net::api::{IceServer, VoiceEvent, VoicePeer};
use crate::net::session::Problem;
use crate::net::{Net, Request, WatchStatus};
use crate::table::Table;
use crate::voice::devices::{self, Choice, DeviceInfo};
use crate::voice::{self, Command, Engine, Event, PeerKey, PeerState, Recovery, Role, Setup};

/// The key held to talk, with push to talk on.
pub const TALK_KEY: KeyCode = KeyCode::KeyV;

/// A volume's step on the panel, and its range (1.0 being as the voice comes).
const VOLUME_STEP: f32 = 0.25;
const VOLUME_MAX: f32 = 2.0;

pub struct VoiceChatPlugin {
    /// Open no microphone and no speaker (`--no-audio-devices`).
    pub no_devices: bool,
    /// Join the voice chat of the game this client sits at, once it does (`--voice`).
    pub join: bool,
    /// Connect only through the server's relay (`--relay-only`).
    pub relay_only: bool,
}

impl Plugin for VoiceChatPlugin {
    fn build(&self, app: &mut App) {
        let device = if self.no_devices {
            Choice::Off
        } else {
            Choice::Default
        };
        app.insert_resource(VoiceChat {
            microphone: device.clone(),
            speaker: device,
            relay_only: self.relay_only,
            ..default()
        })
        .insert_resource(JoinOnStart(self.join))
        .add_observer(on_voice_action)
        .add_systems(
            Update,
            (follow_game, join_on_start, push_to_talk, drive).chain(),
        );
    }
}

/// What a button of the voice chat does.
#[derive(Debug, Clone, PartialEq)]
pub enum VoiceAction {
    Join,
    Leave,
    Mute,
    Deafen,
    PushToTalk,
    /// One person's volume, up or down a step.
    Louder(String),
    Quieter(String),
    /// Show the devices, or stop showing them.
    Devices,
    Microphone(Choice),
    Speaker(Choice),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Off,
    Joining,
    In,
}

/// Somebody else in the call.
#[derive(Debug, Clone, PartialEq)]
pub struct Person {
    pub name: String,
    pub state: PeerState,
    pub speaking: bool,
}

#[derive(Resource)]
pub struct VoiceChat {
    pub phase: Phase,
    pub room: String,
    pub seat: u32,
    /// The name this client's seat plays under.
    pub name: String,
    /// The others in the call, by seat.
    pub people: BTreeMap<u32, Person>,
    pub muted: bool,
    pub deafened: bool,
    pub push_to_talk: bool,
    /// The talk key is held.
    pub holding: bool,
    /// This end is heard speaking.
    pub speaking: bool,
    /// Volumes by the name a person plays under, as the page keeps them.
    pub volumes: HashMap<String, f32>,
    pub error: Option<String>,
    /// The microphones and speakers, while the panel shows them.
    pub devices: Option<(Vec<DeviceInfo>, Vec<DeviceInfo>)>,
    pub microphone: Choice,
    pub speaker: Choice,
    relay_only: bool,
    engine: Option<Engine>,
}

impl Default for VoiceChat {
    /// No devices: only the plugin chooses them, so no test opens a real microphone.
    fn default() -> Self {
        VoiceChat {
            phase: Phase::Off,
            room: String::new(),
            seat: 0,
            name: String::new(),
            people: BTreeMap::new(),
            muted: false,
            deafened: false,
            push_to_talk: false,
            holding: false,
            speaking: false,
            volumes: HashMap::new(),
            error: None,
            devices: None,
            microphone: Choice::Off,
            speaker: Choice::Off,
            relay_only: false,
            engine: None,
        }
    }
}

/// Which end of a pair offers: the higher seat.
pub fn role(me: u32, seat: u32) -> Role {
    if me > seat {
        Role::Offers
    } else {
        Role::Answers
    }
}

/// What the server's list of the call changes: the seats to connect to (with their names), and
/// those to let go.
pub fn changes(
    me: u32,
    people: &BTreeMap<u32, Person>,
    list: &[VoicePeer],
) -> (Vec<(u32, String)>, Vec<u32>) {
    let add = list
        .iter()
        .filter(|p| p.seat != me && !people.contains_key(&p.seat))
        .map(|p| (p.seat, p.name.clone()))
        .collect();
    let remove = people
        .keys()
        .filter(|seat| !list.iter().any(|p| p.seat == **seat))
        .copied()
        .collect();
    (add, remove)
}

fn key(seat: u32) -> PeerKey {
    PeerKey(seat.to_string())
}

impl VoiceChat {
    /// Joins the call of the game `table` shows, at this client's seat.
    pub fn join(&mut self, net: &Net, table: &Table, calling: &mut DirectCalling) {
        let Some(seat) = table.yours() else { return };
        if self.phase != Phase::Off {
            return;
        }
        // One call at a time: the server takes the account out of a friend's call as it joins.
        calling.hang_up(net);
        self.phase = Phase::Joining;
        self.room = table.room_code.clone();
        self.seat = seat.seat;
        self.name = seat.name.clone();
        self.error = None;
        net.send(Request::JoinVoice {
            room: self.room.clone(),
            player_name: self.name.clone(),
        });
    }

    pub fn leave(&mut self, net: &Net) {
        if self.phase == Phase::Off {
            return;
        }
        net.send(Request::LeaveVoice);
        self.teardown();
    }

    fn teardown(&mut self) {
        self.engine = None;
        self.phase = Phase::Off;
        self.people.clear();
        self.speaking = false;
        self.holding = false;
    }

    /// The network thread joined, or could not.
    pub fn joined(
        &mut self,
        net: &Net,
        room: &str,
        result: Result<(Vec<IceServer>, Vec<VoicePeer>), Problem>,
    ) {
        if self.phase != Phase::Joining || room != self.room {
            // Left before it was in.
            if result.is_ok() {
                net.send(Request::LeaveVoice);
            }
            return;
        }
        let (ice, peers) = match result {
            Ok(r) => r,
            Err(problem) => {
                self.teardown();
                self.error = Some(problem.message());
                return;
            }
        };
        let engine = Engine::start(Setup {
            ice_servers: ice
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
        self.engine = Some(engine);
        self.phase = Phase::In;
        self.apply_mic();
        self.peers(room, &peers);
    }

    /// The server's list of the call.
    pub fn peers(&mut self, room: &str, list: &[VoicePeer]) {
        if self.phase != Phase::In || room != self.room {
            return;
        }
        let (add, remove) = changes(self.seat, &self.people, list);
        for seat in remove {
            self.remove(seat);
        }
        for (seat, name) in add {
            self.add(seat, name);
        }
    }

    /// Something said on the voice socket.
    pub fn event(&mut self, room: &str, event: VoiceEvent) {
        if self.phase != Phase::In || room != self.room {
            return;
        }
        match event {
            VoiceEvent::Voice {
                join: true,
                seat,
                name,
            } if seat != self.seat => {
                // A seat that comes back replaces its old connection, if this end offers to it.
                if self.people.contains_key(&seat) && role(self.seat, seat) == Role::Offers {
                    self.remove(seat);
                }
                if !self.people.contains_key(&seat) {
                    self.add(seat, name);
                }
            }
            VoiceEvent::Voice {
                join: false, seat, ..
            } => {
                if seat == self.seat {
                    // The server took this seat out: the account joined another call.
                    self.teardown();
                    self.error = Some(
                        "You left the voice chat of this game: you are in another call.".into(),
                    );
                } else {
                    self.remove(seat);
                }
            }
            VoiceEvent::Signal { from, kind, data } => {
                let signal = match voice::Signal::read(&kind, &data) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("the voice chat: {e}");
                        return;
                    }
                };
                if !self.people.contains_key(&from) {
                    self.add(from, format!("Seat {from}"));
                }
                if let Some(engine) = &self.engine {
                    engine.send(Command::Signal {
                        peer: key(from),
                        signal,
                    });
                }
            }
            _ => {}
        }
    }

    fn add(&mut self, seat: u32, name: String) {
        let Some(engine) = &self.engine else { return };
        engine.send(Command::Add {
            peer: key(seat),
            role: role(self.seat, seat),
            recovery: Recovery::Restart,
        });
        if let Some(volume) = self.volumes.get(&name) {
            engine.send(Command::Volume {
                peer: key(seat),
                volume: *volume,
            });
        }
        self.people.insert(
            seat,
            Person {
                name,
                state: PeerState::Connecting,
                speaking: false,
            },
        );
    }

    fn remove(&mut self, seat: u32) {
        self.people.remove(&seat);
        if let Some(engine) = &self.engine {
            engine.send(Command::Remove { peer: key(seat) });
        }
    }

    /// The microphone is open unless muted, deafened, or waiting for the talk key.
    pub fn mic_open(&self) -> bool {
        !self.muted && !self.deafened && (!self.push_to_talk || self.holding)
    }

    fn apply_mic(&self) {
        if let Some(engine) = &self.engine {
            engine.send(Command::Mute(!self.mic_open()));
            engine.send(Command::Deafen(self.deafened));
        }
    }

    /// One person's volume, by the name they play under.
    pub fn volume(&self, name: &str) -> f32 {
        self.volumes.get(name).copied().unwrap_or(1.0)
    }

    fn set_volume(&mut self, name: &str, volume: f32) {
        let volume = volume.clamp(0.0, VOLUME_MAX);
        self.volumes.insert(name.to_string(), volume);
        if let Some(engine) = &self.engine {
            for (seat, person) in &self.people {
                if person.name == name {
                    engine.send(Command::Volume {
                        peer: key(*seat),
                        volume,
                    });
                }
            }
        }
    }

    fn act(
        &mut self,
        net: &Net,
        action: &VoiceAction,
        table: Option<&Table>,
        calling: &mut DirectCalling,
    ) {
        match action {
            VoiceAction::Join => {
                if let Some(table) = table {
                    self.join(net, table, calling);
                }
            }
            VoiceAction::Leave => self.leave(net),
            VoiceAction::Mute => {
                self.muted = !self.muted;
                self.apply_mic();
            }
            VoiceAction::Deafen => {
                self.deafened = !self.deafened;
                self.apply_mic();
            }
            VoiceAction::PushToTalk => {
                self.push_to_talk = !self.push_to_talk;
                self.holding = false;
                self.apply_mic();
            }
            VoiceAction::Louder(name) => self.set_volume(name, self.volume(name) + VOLUME_STEP),
            VoiceAction::Quieter(name) => self.set_volume(name, self.volume(name) - VOLUME_STEP),
            VoiceAction::Devices => {
                self.devices = match self.devices {
                    Some(_) => None,
                    None => Some(devices::list()),
                };
            }
            VoiceAction::Microphone(choice) => {
                self.microphone = choice.clone();
                self.reopen_devices();
            }
            VoiceAction::Speaker(choice) => {
                self.speaker = choice.clone();
                self.reopen_devices();
            }
        }
    }

    fn reopen_devices(&self) {
        if let Some(engine) = &self.engine {
            engine.send(Command::Devices {
                microphone: self.microphone.clone(),
                speaker: self.speaker.clone(),
            });
        }
    }
}

fn on_voice_action(
    activate: On<Activate>,
    actions: Query<&Action>,
    net: Res<Net>,
    watching: Res<WatchStatus>,
    mut voice: ResMut<VoiceChat>,
    mut calling: ResMut<DirectCalling>,
) {
    let Ok(Action::Voice(action)) = actions.get(activate.entity) else {
        return;
    };
    voice.act(&net, action, watching.table.as_ref(), &mut calling);
}

/// The call is the game's: it ends when the client stops watching that game, or loses its seat.
fn follow_game(net: Res<Net>, watching: Res<WatchStatus>, mut voice: ResMut<VoiceChat>) {
    if !watching.is_changed() || voice.phase == Phase::Off {
        return;
    }
    let still = watching.table.as_ref().is_some_and(|t| {
        t.room_code == voice.room && t.yours().is_some_and(|s| s.seat == voice.seat)
    });
    // While the table is opened again (a dropped socket), the call goes on.
    if !still && (watching.table.is_some() || !watching.is_watching()) {
        voice.leave(&net);
    }
}

/// `--voice`: join once seated.
#[derive(Resource)]
struct JoinOnStart(bool);

fn join_on_start(
    mut pending: ResMut<JoinOnStart>,
    net: Res<Net>,
    watching: Res<WatchStatus>,
    mut voice: ResMut<VoiceChat>,
    mut calling: ResMut<DirectCalling>,
) {
    if !pending.0 {
        return;
    }
    if let Some(table) = watching.table.as_ref().filter(|t| t.yours().is_some()) {
        pending.0 = false;
        voice.join(&net, table, &mut calling);
    }
}

/// Hold the talk key to talk, with push to talk on (not while typing in a field).
fn push_to_talk(
    mut keys: MessageReader<KeyboardInput>,
    focus: Res<InputFocus>,
    mut voice: ResMut<VoiceChat>,
) {
    for key in keys.read() {
        if key.key_code != TALK_KEY || voice.phase != Phase::In || !voice.push_to_talk {
            continue;
        }
        let holding = key.state == ButtonState::Pressed && focus.get().is_none();
        if voice.holding != holding {
            voice.holding = holding;
            voice.apply_mic();
        }
    }
}

/// What the connections say: messages for the other seats, who is connected, who speaks. The
/// resource is marked changed only when something shown changed.
fn drive(net: Res<Net>, mut voice: ResMut<VoiceChat>) {
    let Some(events) = voice.engine.as_ref().map(Engine::events) else {
        return;
    };
    if events.is_empty() {
        return;
    }
    let v = voice.bypass_change_detection();
    let mut shown = false;
    for event in events {
        match event {
            Event::Signal { peer, signal } => {
                if let Ok(to) = peer.0.parse() {
                    net.send(Request::VoiceSignal {
                        to,
                        kind: signal.kind().to_string(),
                        data: signal.data(),
                    });
                }
            }
            Event::State { peer, state } => {
                if let Some(p) = peer.0.parse().ok().and_then(|s: u32| v.people.get_mut(&s)) {
                    info!("the voice chat with {}: {state:?}", p.name);
                    p.state = state;
                    shown = true;
                }
            }
            Event::Speaking {
                peer: Some(peer),
                speaking,
            } => {
                if let Some(p) = peer.0.parse().ok().and_then(|s: u32| v.people.get_mut(&s)) {
                    p.speaking = speaking;
                    shown = true;
                }
            }
            Event::Speaking {
                peer: None,
                speaking,
            } => {
                v.speaking = speaking;
                shown = true;
            }
            Event::Problem(problem) => {
                warn!("the voice chat: {problem}");
                v.error = Some(problem);
                shown = true;
            }
        }
    }
    if shown {
        voice.set_changed();
    }
}

fn act(f: &UiFonts, label: &str, action: VoiceAction) -> impl Bundle {
    button(f, label, Action::Voice(action))
}

/// Labels for devices whose names repeat (as ALSA's do): numbered.
fn labels(devices: &[DeviceInfo]) -> Vec<String> {
    devices
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let same = devices.iter().filter(|e| e.name == d.name).count();
            if same > 1 {
                let n = devices[..=i].iter().filter(|e| e.name == d.name).count();
                format!("{} ({n})", d.name)
            } else {
                d.name.clone()
            }
        })
        .collect()
}

/// The voice chat's part of the game panel, for a player with a seat at `table`.
pub fn draw(p: &mut ChildSpawnerCommands, f: &UiFonts, voice: &VoiceChat, table: &Table) {
    if table.yours().is_none() {
        return;
    }
    let here = voice.phase != Phase::Off && voice.room == table.room_code;
    p.spawn(line(f.semibold(15.0), colours::TEXT, "Voice chat"));
    if let Some(error) = &voice.error {
        p.spawn(line(f.regular(13.0), colours::WARN, error));
    }
    if !here {
        p.spawn(buttons()).with_children(|row| {
            row.spawn(act(f, "Join the voice chat", VoiceAction::Join));
        });
        return;
    }
    if voice.phase == Phase::Joining {
        p.spawn(line(f.regular(13.0), colours::MUTED, "Joining…"));
        return;
    }
    let me = match (voice.mic_open(), voice.speaking) {
        (false, _) if voice.push_to_talk && !voice.muted && !voice.deafened => {
            "You  ·  hold V to talk".to_string()
        }
        (false, _) => "You  ·  muted".to_string(),
        (true, true) => "You  ·  speaking".to_string(),
        (true, false) => "You".to_string(),
    };
    p.spawn(line(f.regular(14.0), colours::TEXT, &me));
    if voice.people.is_empty() {
        p.spawn(line(
            f.regular(13.0),
            colours::MUTED,
            "Nobody else is in the call yet.",
        ));
    }
    for person in voice.people.values() {
        p.spawn(Node {
            column_gap: px(8),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(10),
                    height: px(10),
                    border_radius: BorderRadius::all(px(5)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(match person.state {
                    PeerState::Connected => colours::GOOD,
                    PeerState::Failed => colours::WARN,
                    _ => colours::FIELD_EDGE,
                }),
            ));
            let text = match person.state {
                PeerState::Connected if person.speaking => format!("{}  ·  speaking", person.name),
                PeerState::Connected => person.name.clone(),
                PeerState::Failed => format!("{}  ·  no connection", person.name),
                _ => format!("{}  ·  connecting…", person.name),
            };
            row.spawn(line(f.regular(14.0), colours::TEXT, &text));
            row.spawn(act(f, "−", VoiceAction::Quieter(person.name.clone())));
            let volume = format!("{:.0}%", voice.volume(&person.name) * 100.0);
            row.spawn(line(f.regular(13.0), colours::MUTED, &volume));
            row.spawn(act(f, "+", VoiceAction::Louder(person.name.clone())));
        });
    }
    p.spawn(buttons()).with_children(|row| {
        let toggles = [
            ("Mute", voice.muted, VoiceAction::Mute),
            ("Deafen", voice.deafened, VoiceAction::Deafen),
            (
                "Push to talk (V)",
                voice.push_to_talk,
                VoiceAction::PushToTalk,
            ),
            ("Devices", voice.devices.is_some(), VoiceAction::Devices),
        ];
        for (label, on, action) in toggles {
            row.spawn(chip(f, label, Action::Voice(action), on));
        }
        row.spawn(act(f, "Leave", VoiceAction::Leave));
    });
    if let Some((microphones, speakers)) = &voice.devices {
        let lists = [
            ("Microphone", microphones, &voice.microphone, true),
            ("Speaker", speakers, &voice.speaker, false),
        ];
        for (title, list, chosen, input) in lists {
            p.spawn(line(f.semibold(13.0), colours::TEXT, title));
            p.spawn(buttons()).with_children(|row| {
                let wrap = |c: Choice| {
                    Action::Voice(if input {
                        VoiceAction::Microphone(c)
                    } else {
                        VoiceAction::Speaker(c)
                    })
                };
                let default = matches!(chosen, Choice::Default);
                row.spawn(chip(f, "Default", wrap(Choice::Default), default));
                for (device, label) in list.iter().zip(labels(list)) {
                    let on = *chosen == Choice::Device(device.id.clone());
                    row.spawn(chip(f, &label, wrap(Choice::Device(device.id.clone())), on));
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(seat: u32, name: &str) -> VoicePeer {
        VoicePeer {
            seat,
            name: name.into(),
        }
    }

    fn person(name: &str) -> Person {
        Person {
            name: name.into(),
            state: PeerState::Connecting,
            speaking: false,
        }
    }

    #[test]
    fn the_higher_seat_offers() {
        assert_eq!(role(2, 1), Role::Offers);
        assert_eq!(role(1, 2), Role::Answers);
    }

    #[test]
    fn the_list_says_who_to_connect_to_and_who_to_let_go() {
        let people = BTreeMap::from([(1, person("Ana")), (3, person("Cy"))]);
        let (add, remove) = changes(2, &people, &[peer(1, "Ana"), peer(2, "Me"), peer(4, "Di")]);
        assert_eq!(add, [(4, "Di".to_string())], "never this seat");
        assert_eq!(remove, [3]);
    }

    #[test]
    fn the_microphone_follows_mute_deafen_and_the_talk_key() {
        let mut v = VoiceChat::default();
        assert!(v.mic_open());
        v.push_to_talk = true;
        assert!(!v.mic_open());
        v.holding = true;
        assert!(v.mic_open());
        v.deafened = true;
        assert!(!v.mic_open(), "deafened mutes too");
        v.deafened = false;
        v.muted = true;
        assert!(!v.mic_open());
    }

    #[test]
    fn repeated_device_names_are_numbered() {
        let d = |id: &str, name: &str| DeviceInfo {
            id: id.into(),
            name: name.into(),
        };
        let list = [d("a", "PCH"), d("b", "Headset"), d("c", "PCH")];
        assert_eq!(labels(&list), ["PCH (1)", "Headset", "PCH (2)"]);
    }

    #[test]
    fn being_taken_out_of_the_call_says_why() {
        let mut v = VoiceChat {
            phase: Phase::In,
            room: "AB12CD".into(),
            seat: 2,
            ..default()
        };
        v.event(
            "AB12CD",
            VoiceEvent::Voice {
                join: false,
                seat: 2,
                name: "Me".into(),
            },
        );
        assert_eq!(v.phase, Phase::Off);
        assert!(v.error.as_deref().unwrap().contains("another call"));
    }

    #[test]
    fn a_call_joined_connects_to_the_others_and_offers_to_the_lower_seats() {
        let (net, inbox) = Net::for_test();
        let mut v = VoiceChat {
            phase: Phase::Joining,
            room: "AB12CD".into(),
            seat: 2,
            name: "Me".into(),
            ..default()
        };
        v.joined(
            &net,
            "AB12CD",
            Ok((
                Vec::new(),
                vec![peer(1, "Ana"), peer(2, "Me"), peer(3, "Cy")],
            )),
        );
        assert_eq!(v.phase, Phase::In);
        assert_eq!(v.people.keys().copied().collect::<Vec<_>>(), [1, 3]);
        // Seat 2 offers to seat 1 (lower), and waits for seat 3's offer.
        let engine = v.engine.as_ref().unwrap();
        let mut offered_to = Vec::new();
        for _ in 0..40 {
            if let Some(Event::Signal {
                peer,
                signal: voice::Signal::Offer(_),
            }) = engine.next_event(std::time::Duration::from_millis(50))
            {
                offered_to.push(peer.0);
            }
        }
        assert_eq!(offered_to, ["1"]);
        // Somebody leaves; then this end leaves.
        v.event(
            "AB12CD",
            VoiceEvent::Voice {
                join: false,
                seat: 3,
                name: "Cy".into(),
            },
        );
        assert_eq!(v.people.keys().copied().collect::<Vec<_>>(), [1]);
        v.leave(&net);
        assert!(v.engine.is_none());
        assert!(matches!(inbox.try_recv(), Ok(Request::LeaveVoice)));
    }
}
