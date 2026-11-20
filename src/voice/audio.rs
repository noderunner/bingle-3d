//! Sound on its way to and from the call, with no devices and no network in it: the microphone's
//! samples waiting to be sent, the mix of everyone heard, a resampler between a device's rate and
//! the call's, and whether somebody is speaking.
//!
//! Inside the call everything is mono at 48 kHz in frames of 20 ms, which is what Opus sends.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use super::PeerKey;

/// The call's sample rate.
pub const RATE: u32 = 48_000;
/// One frame: 20 ms at 48 kHz.
pub const FRAME: usize = 960;

/// The most the microphone may get ahead of the voice thread before its oldest samples are
/// dropped, so a stall never turns into lasting delay.
const CAPTURE_MAX: usize = FRAME * 10;
/// What a voice needs queued before it starts to play, to ride out uneven arrivals.
const PREBUFFER: usize = FRAME * 2;
/// The most of a voice queued; beyond it the oldest is dropped down to `TRIM_TO`.
const QUEUE_MAX: usize = FRAME * 10;
const TRIM_TO: usize = FRAME * 4;

/// The RMS of a frame above which somebody counts as speaking (the web page's level).
const SPEAK_LEVEL: f32 = 0.035;
/// Frames of quiet before somebody stops counting as speaking.
const SPEAK_HANGOVER: u32 = 15;

/// Locks a mutex even if a thread panicked holding it: these hold samples, never invariants.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The microphone's samples, mono at 48 kHz, waiting for the voice thread to send them.
#[derive(Clone, Default)]
pub struct Capture(Arc<Mutex<VecDeque<f32>>>);

impl Capture {
    pub fn push(&self, samples: &[f32]) {
        let mut q = lock(&self.0);
        q.extend(samples);
        let over = q.len().saturating_sub(CAPTURE_MAX);
        q.drain(..over);
    }

    /// Takes the next whole frame, if there is one.
    pub fn take_frame(&self, out: &mut [f32; FRAME]) -> bool {
        let mut q = lock(&self.0);
        if q.len() < FRAME {
            return false;
        }
        for (o, s) in out.iter_mut().zip(q.drain(..FRAME)) {
            *o = s;
        }
        true
    }
}

/// The voices heard, each queued as it arrives and mixed as the speaker asks for sound.
#[derive(Default)]
pub struct Mixer {
    voices: HashMap<PeerKey, Voice>,
    volumes: HashMap<PeerKey, f32>,
    deafened: bool,
}

#[derive(Default)]
struct Voice {
    queue: VecDeque<f32>,
    playing: bool,
}

pub type SharedMixer = Arc<Mutex<Mixer>>;

impl Mixer {
    pub fn shared() -> SharedMixer {
        Arc::new(Mutex::new(Mixer::default()))
    }

    /// Queues what was just heard from `peer`.
    pub fn push(&mut self, peer: &PeerKey, samples: &[f32]) {
        let voice = self.voices.entry(peer.clone()).or_default();
        voice.queue.extend(samples);
        if voice.queue.len() > QUEUE_MAX {
            let over = voice.queue.len() - TRIM_TO;
            voice.queue.drain(..over);
        }
    }

    pub fn remove(&mut self, peer: &PeerKey) {
        self.voices.remove(peer);
        self.volumes.remove(peer);
    }

    /// One voice's volume, 1.0 being as it came; kept until the peer is removed.
    pub fn set_volume(&mut self, peer: &PeerKey, volume: f32) {
        self.volumes.insert(peer.clone(), volume.clamp(0.0, 2.0));
    }

    /// Deafened, nothing is played and what arrives is dropped.
    pub fn set_deafened(&mut self, deafened: bool) {
        self.deafened = deafened;
        if deafened {
            self.voices.values_mut().for_each(|v| {
                v.queue.clear();
                v.playing = false;
            });
        }
    }

    /// Fills `out` with the mix of every voice.
    pub fn pull(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        if self.deafened {
            self.voices.clear();
            return;
        }
        for (peer, voice) in &mut self.voices {
            if !voice.playing && voice.queue.len() >= PREBUFFER {
                voice.playing = true;
            }
            if !voice.playing {
                continue;
            }
            let volume = self.volumes.get(peer).copied().unwrap_or(1.0);
            let n = out.len().min(voice.queue.len());
            for (o, s) in out.iter_mut().zip(voice.queue.drain(..n)) {
                *o += s * volume;
            }
            if voice.queue.is_empty() {
                // Ran dry: wait for a little to build up again rather than stutter.
                voice.playing = false;
            }
        }
        for o in out {
            *o = o.clamp(-1.0, 1.0);
        }
    }
}

/// Converts a stream of samples from one rate to another, by linear interpolation (enough for
/// speech, and cheap enough for an audio callback once its buffers have grown).
pub struct Resampler {
    step: f64,
    pos: f64,
    buf: Vec<f32>,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Resampler {
        Resampler {
            step: f64::from(from) / f64::from(to),
            pos: 0.0,
            buf: Vec::new(),
        }
    }

    /// Feeds samples at the rate converted from.
    pub fn feed(&mut self, input: &[f32]) {
        self.buf.extend_from_slice(input);
    }

    /// How many more samples it must be fed to produce `n`.
    pub fn wants(&self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        let last = self.pos + (n - 1) as f64 * self.step;
        (last.floor() as usize + 2).saturating_sub(self.buf.len())
    }

    /// Appends to `out` what it can produce, at most `max` samples.
    pub fn produce(&mut self, out: &mut Vec<f32>, max: usize) {
        let mut made = 0;
        while made < max {
            let i = self.pos.floor() as usize;
            if i + 1 >= self.buf.len() {
                break;
            }
            let f = (self.pos - i as f64) as f32;
            out.push(self.buf[i] + (self.buf[i + 1] - self.buf[i]) * f);
            self.pos += self.step;
            made += 1;
        }
        let used = (self.pos.floor() as usize).min(self.buf.len());
        self.buf.drain(..used);
        self.pos -= used as f64;
    }
}

/// The root mean square of some samples: how loud they are.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Whether somebody is speaking, from the loudness of each frame, holding on through the short
/// gaps between words.
#[derive(Default)]
pub struct Speaking {
    speaking: bool,
    quiet: u32,
}

impl Speaking {
    /// Hears one frame's RMS; returns the new state when it changes.
    pub fn hear(&mut self, rms: f32) -> Option<bool> {
        if rms >= SPEAK_LEVEL {
            self.quiet = 0;
            if !self.speaking {
                self.speaking = true;
                return Some(true);
            }
        } else if self.speaking {
            self.quiet += 1;
            if self.quiet >= SPEAK_HANGOVER {
                self.speaking = false;
                return Some(false);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> PeerKey {
        PeerKey(s.into())
    }

    #[test]
    fn capture_hands_out_whole_frames_and_drops_the_oldest_when_behind() {
        let c = Capture::default();
        let mut frame = [0.0; FRAME];
        c.push(&[0.5; FRAME - 1]);
        assert!(!c.take_frame(&mut frame));
        c.push(&[0.5]);
        assert!(c.take_frame(&mut frame));
        assert!(!c.take_frame(&mut frame));
        c.push(&vec![0.1; CAPTURE_MAX]);
        c.push(&[0.9; FRAME]);
        let mut frames = 0;
        while c.take_frame(&mut frame) {
            frames += 1;
        }
        assert_eq!(frames, CAPTURE_MAX / FRAME);
        assert_eq!(frame[0], 0.9, "the newest samples are kept");
    }

    #[test]
    fn a_voice_waits_for_a_little_to_be_queued_then_plays() {
        let mut m = Mixer::default();
        let mut out = [0.0; FRAME];
        m.push(&key("a"), &[0.25; FRAME]);
        m.pull(&mut out);
        assert_eq!(out[0], 0.0, "not yet: less than the prebuffer");
        m.push(&key("a"), &[0.25; FRAME]);
        m.pull(&mut out);
        assert_eq!(out[0], 0.25);
        m.pull(&mut out);
        assert_eq!(out[0], 0.25);
        m.pull(&mut out);
        assert_eq!(out[0], 0.0, "ran dry");
    }

    #[test]
    fn voices_mix_at_their_volumes_and_clip() {
        let mut m = Mixer::default();
        m.push(&key("a"), &[0.25; PREBUFFER]);
        m.push(&key("b"), &[0.5; PREBUFFER]);
        m.set_volume(&key("b"), 0.5);
        let mut out = [0.0; 4];
        m.pull(&mut out);
        assert_eq!(out, [0.5; 4]);
        m.set_volume(&key("a"), 2.0);
        m.set_volume(&key("b"), 2.0);
        m.pull(&mut out);
        assert_eq!(out, [1.0; 4], "clipped");
    }

    #[test]
    fn deafened_nothing_plays() {
        let mut m = Mixer::default();
        m.push(&key("a"), &[0.25; PREBUFFER]);
        m.set_deafened(true);
        let mut out = [0.0; 8];
        m.pull(&mut out);
        assert_eq!(out, [0.0; 8]);
        m.set_deafened(false);
        m.pull(&mut out);
        assert_eq!(
            out, [0.0; 8],
            "what came while deafened is not played after"
        );
    }

    #[test]
    fn a_voice_far_behind_is_trimmed() {
        let mut m = Mixer::default();
        m.push(&key("a"), &vec![0.1; QUEUE_MAX]);
        m.push(&key("a"), &[0.2]);
        assert_eq!(m.voices[&key("a")].queue.len(), TRIM_TO);
        assert_eq!(m.voices[&key("a")].queue.back(), Some(&0.2));
    }

    #[test]
    fn the_resampler_keeps_the_rate_and_the_shape() {
        // 44.1 kHz to 48 kHz and back: a second in is a second out, give or take a sample.
        for (from, to) in [(44_100, 48_000), (48_000, 44_100), (48_000, 48_000)] {
            let mut r = Resampler::new(from, to);
            let mut out = Vec::new();
            for _ in 0..100 {
                r.feed(&vec![0.5; from as usize / 100]);
                r.produce(&mut out, usize::MAX);
            }
            assert!(
                out.len().abs_diff(to as usize) <= 2,
                "{from}->{to}: {}",
                out.len()
            );
            assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-6));
        }
        // A ramp stays a ramp.
        let mut r = Resampler::new(1, 2);
        r.feed(&[0.0, 1.0, 2.0]);
        let mut out = Vec::new();
        r.produce(&mut out, usize::MAX);
        assert_eq!(out, [0.0, 0.5, 1.0, 1.5]);
    }

    #[test]
    fn the_resampler_says_what_it_wants() {
        let mut r = Resampler::new(48_000, 44_100);
        let mut out = Vec::new();
        for n in [512, 441, 1, 1024] {
            let want = r.wants(n);
            r.feed(&vec![0.0; want]);
            out.clear();
            r.produce(&mut out, n);
            assert_eq!(out.len(), n);
        }
    }

    #[test]
    fn speaking_holds_through_short_gaps() {
        let mut s = Speaking::default();
        assert_eq!(s.hear(0.0), None);
        assert_eq!(s.hear(0.2), Some(true));
        for _ in 0..SPEAK_HANGOVER - 1 {
            assert_eq!(s.hear(0.0), None);
        }
        assert_eq!(s.hear(0.2), None, "still speaking");
        for _ in 0..SPEAK_HANGOVER - 1 {
            assert_eq!(s.hear(0.0), None);
        }
        assert_eq!(s.hear(0.0), Some(false));
        assert!((rms(&[0.5, -0.5]) - 0.5).abs() < 1e-6);
    }
}
