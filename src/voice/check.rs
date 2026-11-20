//! `--voice-check`: the microphone heard back through the speaker, over a real WebRTC connection
//! on this machine, so a player (or a maintainer) can try the devices and the whole sound path
//! without a server or anybody to call.

use std::time::{Duration, Instant};

use super::devices::{self, Choice, DeviceInfo};
use super::{Command, Engine, Event, PeerKey, PeerState, Recovery, Role, Setup};

/// How long the check lasts.
const LENGTH: Duration = Duration::from_secs(10);

pub fn run() -> Result<(), String> {
    let (microphones, speakers) = devices::list();
    println!("Microphones:{}", list(&microphones));
    println!("Speakers:{}", list(&speakers));
    println!("Using the default of each.");
    let setup = |microphone, speaker| Setup {
        ice_servers: Vec::new(),
        loopback: true,
        relay_only: false,
        microphone,
        speaker,
    };
    let talker = Engine::start(setup(Choice::Default, Choice::Off));
    let listener = Engine::start(setup(Choice::Off, Choice::Default));
    let (to_listener, to_talker) = (PeerKey("listener".into()), PeerKey("talker".into()));
    talker.send(Command::Add {
        peer: to_listener.clone(),
        role: Role::Offers,
        recovery: Recovery::OnRequest,
    });
    listener.send(Command::Add {
        peer: to_talker.clone(),
        role: Role::Answers,
        recovery: Recovery::OnRequest,
    });
    let mut until: Option<Instant> = None;
    let started = Instant::now();
    loop {
        if until.is_some_and(|t| Instant::now() >= t) {
            println!("Done.");
            return Ok(());
        }
        if until.is_none() && started.elapsed() > Duration::from_secs(10) {
            return Err("the two ends of the check did not connect".into());
        }
        for (engine, other, from) in [
            (&talker, &listener, &to_talker),
            (&listener, &talker, &to_listener),
        ] {
            for event in engine.events() {
                match event {
                    Event::Signal { signal, .. } => other.send(Command::Signal {
                        peer: from.clone(),
                        signal,
                    }),
                    Event::State {
                        state: PeerState::Connected,
                        ..
                    } if until.is_none() && std::ptr::eq(engine, &listener) => {
                        println!(
                            "Connected. Speak: you should hear yourself for {} seconds.",
                            LENGTH.as_secs()
                        );
                        until = Some(Instant::now() + LENGTH);
                    }
                    Event::State {
                        state: PeerState::Failed,
                        ..
                    } => return Err("the connection failed".into()),
                    Event::Speaking {
                        peer: None,
                        speaking,
                    } => println!("{}", if speaking { "(speaking)" } else { "(quiet)" }),
                    Event::Problem(p) => println!("{p}"),
                    _ => {}
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn list(devices: &[DeviceInfo]) -> String {
    if devices.is_empty() {
        return " none".into();
    }
    devices
        .iter()
        .map(|d| format!("\n  {}  ({})", d.name, d.id))
        .collect()
}
