//! One connection of a call: the WebRTC peer connection to one person, the UDP socket it runs on
//! (rtc does no I/O of its own), and the Opus decoder for what they say.
//!
//! It follows what Bingle's page does (`docs/voice.md`), so a browser can be on the other end:
//! one audio track and no data channel; candidates that come before the description they belong to
//! are kept until it is set; after a failure the side that offers restarts ICE and the other asks
//! it to, by `retry`.

use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use bevy::log::{debug, info};
use bytes::{Bytes, BytesMut};
use rtc::media_stream::MediaStreamTrack;
use rtc::peer_connection::configuration::RTCConfigurationBuilder;
use rtc::peer_connection::configuration::interceptor_registry::register_default_interceptors;
use rtc::peer_connection::configuration::media_engine::{MIME_TYPE_OPUS, MediaEngine};
use rtc::peer_connection::configuration::setting_engine::SettingEngineBuilder;
use rtc::peer_connection::event::RTCPeerConnectionEvent;
use rtc::peer_connection::message::{RTCMessage, TaggedRTCMessage};
use rtc::peer_connection::sdp::RTCSessionDescription;
use rtc::peer_connection::state::RTCPeerConnectionState;
use rtc::peer_connection::transport::{
    CandidateConfig, CandidateHostConfig, CandidateServerReflexiveConfig, RTCIceCandidate,
    RTCIceCandidateInit,
};
use rtc::peer_connection::{RTCPeerConnection, RTCPeerConnectionBuilder};
use rtc::rtp_transceiver::RTCRtpSenderId;
use rtc::rtp_transceiver::rtp_sender::{
    RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters, RTCRtpEncodingParameters,
    RtpCodecKind,
};
use rtc::sansio::Protocol;
use rtc::shared::{TaggedBytesMut, TransportContext, TransportProtocol};
use rtc::stun::message::{BINDING_REQUEST, Getter, Message, TransactionId};
use rtc::stun::xoraddr::XorMappedAddress;
use rusty_opus::OpusDecoder;

use super::audio::{FRAME, RATE, SharedMixer, Speaking, rms};
use super::signal::{Candidate, Signal};
use super::{Event, PeerKey, PeerState, Recovery, Role};

/// How long after a failure the side that offers restarts ICE, and how many times in a row.
const RESTART_AFTER: Duration = Duration::from_millis(1500);
const RESTARTS: u32 = 3;
/// When a STUN server is asked again for this connection's public address, and when it is
/// given up on.
const STUN_AGAIN: Duration = Duration::from_millis(500);
const STUN_TRIES: u32 = 3;
/// The most consecutive lost packets concealed; after a longer gap the voice simply resumes.
const CONCEAL_MAX: u16 = 3;

/// Where the call's sockets are and which STUN servers tell them their public address.
#[derive(Debug, Clone)]
pub struct Network {
    pub ip: IpAddr,
    /// Only loopback: both ends on this machine (tests and `--voice-check`).
    pub loopback: bool,
    /// Each server's address and the URL it was given as.
    pub stun: Vec<(SocketAddr, String)>,
}

/// A STUN question in flight: what this socket's address looks like from outside.
struct StunQuery {
    server: SocketAddr,
    url: String,
    id: TransactionId,
    raw: Vec<u8>,
    sent: Instant,
    tries: u32,
}

pub struct Peer {
    key: PeerKey,
    role: Role,
    recovery: Recovery,
    socket: UdpSocket,
    local: SocketAddr,
    loopback: bool,
    pc: RTCPeerConnection,
    sender: RTCRtpSenderId,
    ssrc: u32,
    payload_type: Option<u8>,
    seq: u16,
    talking: bool,
    decoder: OpusDecoder,
    last_seq: Option<u16>,
    /// Candidates from the other end, waiting for its description.
    pending: Vec<Candidate>,
    /// This end's candidates, to give a new connection on the same socket.
    local_candidates: Vec<RTCIceCandidateInit>,
    /// Candidates found after the description went out, to trickle.
    trickled: Vec<Candidate>,
    stun: Vec<StunQuery>,
    state: PeerState,
    restarts: u32,
    restart_at: Option<Instant>,
    speaking: Speaking,
    pcm: Vec<f32>,
}

impl Peer {
    pub fn new(
        key: PeerKey,
        role: Role,
        recovery: Recovery,
        net: &Network,
    ) -> Result<Peer, String> {
        let socket = UdpSocket::bind(SocketAddr::new(net.ip, 0))
            .map_err(|e| format!("could not open a socket for voice: {e}"))?;
        socket
            .set_nonblocking(true)
            .map_err(|e| format!("could not open a socket for voice: {e}"))?;
        let local = socket
            .local_addr()
            .map_err(|e| format!("could not open a socket for voice: {e}"))?;
        let host = CandidateHostConfig {
            base_config: CandidateConfig {
                network: "udp".to_owned(),
                address: local.ip().to_string(),
                port: local.port(),
                component: 1,
                ..Default::default()
            },
            ..Default::default()
        }
        .new_candidate_host()
        .and_then(|c| RTCIceCandidate::from(&c).to_json())
        .map_err(|e| format!("voice: {e}"))?;
        let now = Instant::now();
        let stun = net
            .stun
            .iter()
            .map(|(server, url)| StunQuery::new(*server, url.clone(), now))
            .collect();
        let (pc, sender, ssrc) = connection(net.loopback, std::slice::from_ref(&host))?;
        let decoder = OpusDecoder::new(RATE as i32, 1).map_err(|e| format!("Opus: {e:?}"))?;
        let peer = Peer {
            key,
            role,
            recovery,
            socket,
            local,
            loopback: net.loopback,
            pc,
            sender,
            ssrc,
            payload_type: None,
            seq: 0,
            talking: false,
            decoder,
            last_seq: None,
            pending: Vec::new(),
            local_candidates: vec![host],
            trickled: Vec::new(),
            stun,
            state: PeerState::Connecting,
            restarts: 0,
            restart_at: None,
            speaking: Speaking::default(),
            pcm: vec![0.0; FRAME * 6],
        };
        for q in &peer.stun {
            peer.send_raw(&q.raw, q.server);
        }
        Ok(peer)
    }

    pub fn connected(&self) -> bool {
        self.state == PeerState::Connected
    }

    /// Begins: the side that offers makes the first offer.
    pub fn start(&mut self, out: &mut Vec<Event>) {
        if self.role == Role::Offers {
            self.offer(false, out);
        }
    }

    /// A message from the other end.
    pub fn handle(&mut self, signal: Signal, out: &mut Vec<Event>) {
        match signal {
            Signal::Offer(sdp) => self.answer(sdp, out),
            Signal::Answer(sdp) => {
                let set = RTCSessionDescription::answer(sdp)
                    .and_then(|d| self.pc.set_remote_description(Instant::now(), d));
                match set {
                    Ok(()) => self.add_pending(),
                    Err(e) => info!("voice: {}'s answer was refused: {e}", self.key),
                }
            }
            Signal::Ice(c) => {
                if self.pc.remote_description().is_some() {
                    self.add_remote(c);
                } else {
                    self.pending.push(c);
                }
            }
            Signal::Retry => {
                if self.role == Role::Offers {
                    self.renew(out);
                    self.offer(false, out);
                }
            }
        }
    }

    /// Sends one frame of Opus, `timestamp` being the frame's place in the call's clock.
    pub fn send_audio(&mut self, opus: &[u8], timestamp: u32) {
        if !self.connected() {
            return;
        }
        let Some(mut sender) = self.pc.rtp_sender(self.sender) else {
            return;
        };
        let pt = match self.payload_type {
            Some(pt) => pt,
            None => {
                let found = sender
                    .get_parameters()
                    .rtp_parameters
                    .codecs
                    .iter()
                    .find(|c| c.rtp_codec.mime_type.eq_ignore_ascii_case(MIME_TYPE_OPUS))
                    .map(|c| c.payload_type);
                let Some(pt) = found else { return };
                self.payload_type = Some(pt);
                pt
            }
        };
        let mut packet = rtc::rtp::Packet::default();
        packet.header.version = 2;
        packet.header.payload_type = pt;
        packet.header.sequence_number = self.seq;
        packet.header.timestamp = timestamp;
        packet.header.ssrc = self.ssrc;
        // The first packet after a silence starts a talkspurt.
        packet.header.marker = !self.talking;
        packet.payload = Bytes::copy_from_slice(opus);
        if let Err(e) = sender.write_rtp(Instant::now(), packet) {
            debug!("voice: could not send to {}: {e}", self.key);
            return;
        }
        self.seq = self.seq.wrapping_add(1);
        self.talking = true;
    }

    /// Nothing was sent for a frame (muted, or nobody connected).
    pub fn silent(&mut self) {
        self.talking = false;
    }

    /// Reads what has arrived, runs the connection's timers, and hands on what it produced.
    pub fn drive(&mut self, now: Instant, mixer: &SharedMixer, out: &mut Vec<Event>) {
        self.read_socket(now);
        self.ask_stun_again(now);
        for c in std::mem::take(&mut self.trickled) {
            out.push(Event::Signal {
                peer: self.key.clone(),
                signal: Signal::Ice(c),
            });
        }
        if self.restart_at.is_some_and(|at| at <= now) {
            self.restart_at = None;
            info!("voice: restarting ICE with {}", self.key);
            self.pc.restart_ice();
            self.offer(true, out);
        }
        if self.pc.poll_timeout().is_some_and(|at| at <= now)
            && let Err(e) = self.pc.handle_timeout(now)
        {
            debug!("voice: {}: {e}", self.key);
        }
        self.flush();
        while let Some(event) = self.pc.poll_event() {
            if let RTCPeerConnectionEvent::OnConnectionStateChangeEvent(s) = event {
                self.changed(s, now, out);
            }
        }
        while let Some(TaggedRTCMessage { message, .. }) = self.pc.poll_read() {
            if let RTCMessage::RtpPacket(_, packet) = message {
                self.heard(packet.header.sequence_number, &packet.payload, mixer, out);
            }
        }
        self.flush();
    }

    /// When this connection next needs `drive`, if it has a timer.
    pub fn next_wake(&mut self) -> Option<Instant> {
        let stun = self.stun.iter().map(|q| q.sent + STUN_AGAIN).min();
        [self.pc.poll_timeout(), self.restart_at, stun]
            .into_iter()
            .flatten()
            .min()
    }

    pub fn close(&mut self) {
        let _ = self.pc.close();
        self.flush();
    }

    fn offer(&mut self, restart: bool, out: &mut Vec<Event>) {
        let made = self.pc.create_offer(None).and_then(|offer| {
            self.pc.set_local_description(Instant::now(), offer)?;
            Ok(())
        });
        match (made, self.pc.local_description()) {
            (Ok(()), Some(d)) => out.push(Event::Signal {
                peer: self.key.clone(),
                signal: Signal::Offer(d.sdp.clone()),
            }),
            (Err(e), _) => {
                info!(
                    "voice: could not offer to {} (restart {restart}): {e}",
                    self.key
                );
                self.set_state(PeerState::Failed, out);
            }
            _ => {}
        }
        self.flush();
    }

    fn answer(&mut self, sdp: String, out: &mut Vec<Event>) {
        // An offer on a connection of the other end's own (its certificate is new), or for one of
        // ours that has failed, is answered on a new one; otherwise it renegotiates this one.
        let theirs = self.pc.remote_description().map(|d| fingerprint_of(&d.sdp));
        let new_connection = theirs.is_some_and(|f| f != fingerprint_of(&sdp));
        if new_connection || matches!(self.state, PeerState::Failed | PeerState::Closed) {
            self.renew(out);
        }
        let set = |pc: &mut RTCPeerConnection| {
            let offer = RTCSessionDescription::offer(sdp.clone())?;
            pc.set_remote_description(Instant::now(), offer)
        };
        if let Err(e) = set(&mut self.pc) {
            debug!(
                "voice: {}'s offer on the old connection: {e}; trying a new one",
                self.key
            );
            self.renew(out);
            if let Err(e) = set(&mut self.pc) {
                info!("voice: {}'s offer was refused: {e}", self.key);
                self.set_state(PeerState::Failed, out);
                return;
            }
        }
        self.add_pending();
        let made = self.pc.create_answer(None).and_then(|answer| {
            self.pc.set_local_description(Instant::now(), answer)?;
            Ok(())
        });
        match (made, self.pc.local_description()) {
            (Ok(()), Some(d)) => out.push(Event::Signal {
                peer: self.key.clone(),
                signal: Signal::Answer(d.sdp.clone()),
            }),
            (Err(e), _) => {
                info!("voice: could not answer {}: {e}", self.key);
                self.set_state(PeerState::Failed, out);
            }
            _ => {}
        }
        self.flush();
    }

    /// A new connection on the same socket, with the same local candidates.
    fn renew(&mut self, out: &mut Vec<Event>) {
        let _ = self.pc.close();
        match connection(self.loopback, &self.local_candidates) {
            Ok((pc, sender, ssrc)) => {
                self.pc = pc;
                self.sender = sender;
                self.ssrc = ssrc;
                self.payload_type = None;
                self.pending.clear();
                self.last_seq = None;
                self.set_state(PeerState::Connecting, out);
            }
            Err(e) => {
                info!("voice: {e}");
                self.set_state(PeerState::Failed, out);
            }
        }
    }

    fn add_pending(&mut self) {
        for c in std::mem::take(&mut self.pending) {
            self.add_remote(c);
        }
    }

    fn add_remote(&mut self, c: Candidate) {
        let init = RTCIceCandidateInit {
            candidate: c.candidate,
            sdp_mid: c.sdp_mid,
            sdp_mline_index: c.sdp_mline_index,
            username_fragment: c.username_fragment,
            url: None,
        };
        // A browser's host candidates are often hidden behind mDNS names, which this cannot
        // resolve; its checks still reach this end's candidates.
        if let Err(e) = self.pc.add_remote_candidate(init) {
            debug!("voice: a candidate from {} was not used: {e}", self.key);
        }
    }

    fn changed(&mut self, s: RTCPeerConnectionState, now: Instant, out: &mut Vec<Event>) {
        debug!("voice: the connection with {} is {s}", self.key);
        match s {
            RTCPeerConnectionState::Connected => {
                self.restarts = 0;
                self.set_state(PeerState::Connected, out);
            }
            RTCPeerConnectionState::Failed => {
                self.set_state(PeerState::Failed, out);
                if self.recovery != Recovery::Restart {
                    return;
                }
                match self.role {
                    Role::Offers if self.restarts < RESTARTS => {
                        self.restarts += 1;
                        self.restart_at = Some(now + RESTART_AFTER);
                    }
                    Role::Offers => {}
                    Role::Answers => out.push(Event::Signal {
                        peer: self.key.clone(),
                        signal: Signal::Retry,
                    }),
                }
            }
            RTCPeerConnectionState::Closed => self.set_state(PeerState::Closed, out),
            // The other end has gone quiet: it may come back, or fail.
            RTCPeerConnectionState::Disconnected => self.set_state(PeerState::Connecting, out),
            _ => {}
        }
    }

    fn set_state(&mut self, state: PeerState, out: &mut Vec<Event>) {
        if self.state != state {
            self.state = state;
            out.push(Event::State {
                peer: self.key.clone(),
                state,
            });
        }
    }

    /// One packet of Opus from the other end, concealing a few lost before it.
    fn heard(&mut self, seq: u16, payload: &[u8], mixer: &SharedMixer, out: &mut Vec<Event>) {
        let gap = match self.last_seq {
            Some(last) => seq.wrapping_sub(last) as i16,
            None => 1,
        };
        if gap <= 0 {
            return; // late, or twice
        }
        self.last_seq = Some(seq);
        let mut decoded = Vec::with_capacity(FRAME);
        let lost = (gap - 1) as u16;
        if (1..=CONCEAL_MAX).contains(&lost) {
            for i in 0..lost {
                let n = if i + 1 == lost {
                    // The frame just before this one may be carried in it (in-band FEC).
                    self.decoder.decode_fec(payload, FRAME, &mut self.pcm)
                } else {
                    self.decoder.decode(&[], FRAME, &mut self.pcm)
                };
                decoded.extend_from_slice(&self.pcm[..n.unwrap_or(0)]);
            }
        }
        match self.decoder.decode(payload, self.pcm.len(), &mut self.pcm) {
            Ok(n) => {
                if let Some(speaking) = self.speaking.hear(rms(&self.pcm[..n])) {
                    out.push(Event::Speaking {
                        peer: Some(self.key.clone()),
                        speaking,
                    });
                }
                decoded.extend_from_slice(&self.pcm[..n]);
            }
            Err(e) => debug!("voice: a packet from {} did not decode: {e:?}", self.key),
        }
        super::audio::lock(mixer).push(&self.key, &decoded);
    }

    fn read_socket(&mut self, now: Instant) {
        let mut buf = [0u8; 2048];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((n, from)) => {
                    let data = &buf[..n];
                    if let Some(i) = self.stun.iter().position(|q| q.answers(data)) {
                        let q = self.stun.remove(i);
                        self.reflexive(&q, data);
                        continue;
                    }
                    let read = self.pc.handle_read(TaggedBytesMut {
                        now,
                        transport: TransportContext {
                            local_addr: self.local,
                            peer_addr: from,
                            ecn: None,
                            transport_protocol: TransportProtocol::UDP,
                        },
                        message: BytesMut::from(data),
                    });
                    if let Err(e) = read {
                        debug!("voice: a packet from {from}: {e}");
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                // A port that refused an earlier packet (ICMP), on some systems: keep reading.
                Err(e) if e.kind() == ErrorKind::ConnectionReset => {}
                Err(e) => {
                    debug!("voice: reading the socket: {e}");
                    return;
                }
            }
        }
    }

    /// A STUN server said what this socket's address is from outside: a candidate.
    fn reflexive(&mut self, q: &StunQuery, data: &[u8]) {
        let mut msg = Message::new();
        msg.raw = data.to_vec();
        let mut addr = XorMappedAddress::default();
        if msg.decode().and_then(|()| addr.get_from(&msg)).is_err() {
            return;
        }
        let candidate = CandidateServerReflexiveConfig {
            base_config: CandidateConfig {
                network: "udp".to_owned(),
                address: addr.ip.to_string(),
                port: addr.port,
                component: 1,
                ..Default::default()
            },
            rel_addr: self.local.ip().to_string(),
            rel_port: self.local.port(),
            url: Some(q.url.clone()),
        }
        .new_candidate_server_reflexive()
        .and_then(|c| RTCIceCandidate::from(&c).to_json());
        let Ok(mut init) = candidate else { return };
        init.url = Some(q.url.clone());
        if self
            .local_candidates
            .iter()
            .any(|c| c.candidate == init.candidate)
        {
            return;
        }
        debug!("voice: {} told {} it is {}", q.server, self.local, addr.ip);
        if let Err(e) = self.pc.add_local_candidate(init.clone()) {
            debug!("voice: {e}");
            return;
        }
        self.local_candidates.push(init.clone());
        // Once the description is out, a new candidate is trickled; before, it goes in it.
        if let Some(d) = self.pc.local_description() {
            self.trickled.push(Candidate {
                candidate: init.candidate,
                sdp_mid: mid_of(Some(&d.sdp)),
                sdp_mline_index: Some(0),
                username_fragment: None,
            });
        }
    }

    fn ask_stun_again(&mut self, now: Instant) {
        self.stun.retain(|q| q.tries < STUN_TRIES);
        let due: Vec<(Vec<u8>, SocketAddr)> = self
            .stun
            .iter_mut()
            .filter(|q| q.sent + STUN_AGAIN <= now)
            .map(|q| {
                q.sent = now;
                q.tries += 1;
                (q.raw.clone(), q.server)
            })
            .collect();
        for (raw, server) in due {
            self.send_raw(&raw, server);
        }
    }

    fn send_raw(&self, data: &[u8], to: SocketAddr) {
        if let Err(e) = self.socket.send_to(data, to) {
            debug!("voice: sending to {to}: {e}");
        }
    }

    fn flush(&mut self) {
        while let Some(msg) = self.pc.poll_write() {
            self.send_raw(&msg.message, msg.transport.peer_addr);
        }
    }
}

impl StunQuery {
    fn new(server: SocketAddr, url: String, now: Instant) -> StunQuery {
        let id = TransactionId::new();
        let mut msg = Message::new();
        let _ = msg.build(&[Box::new(id), Box::new(BINDING_REQUEST)]);
        StunQuery {
            server,
            url,
            id,
            raw: msg.raw,
            sent: now,
            tries: 1,
        }
    }

    /// Whether a datagram is this question's answer: STUN's magic cookie and its transaction.
    fn answers(&self, data: &[u8]) -> bool {
        data.len() >= 20 && data[4..8] == [0x21, 0x12, 0xa4, 0x42] && data[8..20] == self.id.0
    }
}

/// A new peer connection for one Opus track, with this end's candidates.
fn connection(
    loopback: bool,
    candidates: &[RTCIceCandidateInit],
) -> Result<(RTCPeerConnection, RTCRtpSenderId, u32), String> {
    let fail = |e: rtc::shared::error::Error| format!("voice: {e}");
    let mut media = MediaEngine::default();
    media
        .register_codec(
            RTCRtpCodecParameters {
                rtp_codec: opus(),
                payload_type: 111,
            },
            RtpCodecKind::Audio,
        )
        .map_err(fail)?;
    let registry = register_default_interceptors(rtc::interceptor::Registry::new(), &mut media)
        .map_err(fail)?;
    let settings = SettingEngineBuilder::new()
        .with_include_loopback_candidate(loopback)
        .build();
    let mut pc = RTCPeerConnectionBuilder::new()
        .with_configuration(RTCConfigurationBuilder::new().build())
        .with_media_engine(media)
        .with_setting_engine(settings)
        .with_interceptor_registry(registry)
        .build(Instant::now())
        .map_err(fail)?;
    let ssrc = new_ssrc();
    let sender = pc
        .add_track(MediaStreamTrack::new(
            format!("bingle-{ssrc:08x}"),
            "voice".into(),
            "voice".into(),
            RtpCodecKind::Audio,
            vec![RTCRtpEncodingParameters {
                rtp_coding_parameters: RTCRtpCodingParameters {
                    ssrc: Some(ssrc),
                    ..Default::default()
                },
                codec: opus(),
                ..Default::default()
            }],
        ))
        .map_err(fail)?;
    for c in candidates {
        pc.add_local_candidate(c.clone()).map_err(fail)?;
    }
    Ok((pc, sender, ssrc))
}

/// Opus as browsers offer it: two channels in the SDP (always, by RFC 7587), sent as mono.
fn opus() -> RTCRtpCodec {
    RTCRtpCodec {
        mime_type: MIME_TYPE_OPUS.to_owned(),
        clock_rate: RATE,
        channels: 2,
        sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
        rtcp_feedback: vec![],
    }
}

/// A synchronisation source for a new track: random, as RFC 3550 asks.
fn new_ssrc() -> u32 {
    let mut b = [0u8; 4];
    let _ = ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut b);
    u32::from_be_bytes(b).max(1)
}

/// The certificate fingerprint a description gives, which is new with every connection.
fn fingerprint_of(sdp: &str) -> Option<String> {
    sdp.lines()
        .find_map(|l| l.trim().strip_prefix("a=fingerprint:"))
        .map(str::to_owned)
}

/// The `mid` of the first media section of a description.
fn mid_of(sdp: Option<&str>) -> Option<String> {
    sdp?.lines()
        .find_map(|l| l.trim().strip_prefix("a=mid:"))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mid_is_read_from_the_description() {
        let sdp = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:0\r\n";
        assert_eq!(mid_of(Some(sdp)).as_deref(), Some("0"));
        assert_eq!(mid_of(Some("v=0\r\n")), None);
    }

    #[test]
    fn the_fingerprint_is_read_from_the_description() {
        let sdp = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=fingerprint:sha-256 AB:CD\r\n";
        assert_eq!(fingerprint_of(sdp).as_deref(), Some("sha-256 AB:CD"));
        assert_eq!(fingerprint_of("v=0\r\n"), None);
    }

    #[test]
    fn a_stun_answer_is_known_by_its_transaction() {
        let now = Instant::now();
        let q = StunQuery::new("192.0.2.1:3478".parse().unwrap(), "stun:x".into(), now);
        assert!(
            q.answers(&q.raw),
            "the request carries the same transaction"
        );
        let other = StunQuery::new("192.0.2.1:3478".parse().unwrap(), "stun:x".into(), now);
        assert!(!q.answers(&other.raw));
        assert!(!q.answers(&[0; 10]));
    }
}
