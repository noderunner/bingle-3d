//! What the microphone hears, cleaned as a browser cleans it before it is sent: the voices the
//! speaker plays taken out of it (echo cancellation), the background noise lowered, and the level
//! evened out. Bingle's page asks the browser for all three (`echoCancellation`,
//! `noiseSuppression`, `autoGainControl`); here they are `sonora`, WebRTC's own audio processing
//! in Rust, on the voice thread.
//!
//! The canceller needs to know what was played: the mixer keeps a copy of everything the speaker
//! pulled ([`super::audio::Mixer::take_played`]), and it is fed here before each frame from the
//! microphone. Where the echo sits in time is the canceller's to find.

use sonora::config::{AdaptiveDigital, EchoCanceller, GainController2, NoiseSuppression};
use sonora::{AudioProcessing, Config, StreamConfig};

use super::audio::{FRAME, RATE};

/// The canceller works on 10 ms at a time.
const CHUNK: usize = RATE as usize / 100;

/// A guess at the time between a sound being mixed and the microphone hearing it back (the
/// speaker's and the microphone's buffers): only where the canceller starts to look, since it
/// finds the echo's delay itself.
const DELAY_MS: i32 = 60;

/// The microphone's sound on its way out: echo cancellation, noise suppression and gain control.
pub struct Cleaner {
    apm: AudioProcessing,
    /// What was played, waiting for a whole chunk.
    played: Vec<f32>,
    out: [f32; CHUNK],
}

impl Default for Cleaner {
    fn default() -> Self {
        Cleaner::new()
    }
}

impl Cleaner {
    pub fn new() -> Cleaner {
        let config = Config {
            echo_canceller: Some(EchoCanceller::default()),
            noise_suppression: Some(NoiseSuppression::default()),
            gain_controller2: Some(GainController2 {
                adaptive_digital: Some(AdaptiveDigital::default()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let stream = StreamConfig::new(RATE, 1);
        Cleaner {
            apm: AudioProcessing::builder()
                .config(config)
                .capture_config(stream)
                .render_config(stream)
                .build(),
            played: Vec::with_capacity(FRAME * 2),
            out: [0.0; CHUNK],
        }
    }

    /// What the speaker played since the last call, mono at the call's rate.
    pub fn played(&mut self, samples: &[f32]) {
        self.played.extend_from_slice(samples);
        let (chunks, _) = self.played.as_chunks::<CHUNK>();
        let whole = chunks.len() * CHUNK;
        for chunk in chunks {
            // Render is only analysed; what it would write back is not used.
            let _ = self
                .apm
                .process_render_f32(&[chunk], &mut [&mut self.out[..]]);
        }
        self.played.drain(..whole);
    }

    /// Cleans one frame from the microphone, in place. Should the canceller refuse it, the
    /// frame goes out as it was.
    pub fn clean(&mut self, frame: &mut [f32; FRAME]) {
        for chunk in frame.as_chunks_mut::<CHUNK>().0 {
            let _ = self.apm.set_stream_delay_ms(DELAY_MS);
            if self
                .apm
                .process_capture_f32(&[chunk], &mut [&mut self.out[..]])
                .is_ok()
            {
                chunk.copy_from_slice(&self.out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::audio::rms;

    /// Something like a voice, for the canceller: a pitch that glides as speech does, with its
    /// harmonics and some breath, in syllables. (A steady pure tone is no test: the canceller
    /// cannot tell where in time a sine's echo sits, and gives up on it.)
    struct Talk {
        seed: u32,
        pitch: f32,
        syllables: f32,
        phase: f32,
        n: usize,
    }

    impl Talk {
        /// A voice around `pitch` Hz, saying `syllables` a second.
        fn new(seed: u32, pitch: f32, syllables: f32) -> Talk {
            Talk {
                seed,
                pitch,
                syllables,
                phase: 0.0,
                n: 0,
            }
        }

        fn frame(&mut self) -> [f32; FRAME] {
            use std::f32::consts::{PI, TAU};
            let mut f = [0.0; FRAME];
            for s in &mut f {
                self.seed = self
                    .seed
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
                let breath = (self.seed >> 8) as f32 / (1 << 24) as f32 - 0.5;
                let t = self.n as f32 / RATE as f32;
                let pitch = self.pitch * (1.0 + 0.3 * (t * 0.7 * TAU).sin());
                self.phase = (self.phase + pitch / RATE as f32) % 1.0;
                let voiced: f32 = (1..=5)
                    .map(|h| (self.phase * h as f32 * TAU).sin() / h as f32)
                    .sum();
                let envelope = (t * self.syllables * PI).sin().abs();
                *s = 0.2 * envelope * (0.6 * voiced + 0.6 * breath);
                self.n += 1;
            }
            f
        }
    }

    /// Plays `far` and hears it back `delay` samples later at `gain`, plus `near` (what the
    /// player says), for `frames` frames; the level of what is sent over the last second.
    fn run(frames: usize, delay: usize, gain: f32, far: bool, near: bool) -> (f32, f32) {
        let mut cleaner = Cleaner::new();
        let (mut far_talk, mut near_talk) = (Talk::new(1, 120.0, 3.0), Talk::new(2, 220.0, 5.0));
        let mut line: Vec<f32> = vec![0.0; delay];
        let (mut sent, mut heard) = (Vec::new(), Vec::new());
        for i in 0..frames {
            let played = if far { far_talk.frame() } else { [0.0; FRAME] };
            cleaner.played(&played);
            line.extend_from_slice(&played);
            let said = if near {
                near_talk.frame()
            } else {
                [0.0; FRAME]
            };
            let mut frame = [0.0; FRAME];
            for (j, s) in frame.iter_mut().enumerate() {
                *s = line[j] * gain + said[j];
            }
            line.drain(..FRAME);
            let before = frame;
            cleaner.clean(&mut frame);
            if i >= frames - 50 {
                heard.push(rms(&before));
                sent.push(rms(&frame));
            }
        }
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        (mean(&heard), mean(&sent))
    }

    #[test]
    fn the_voices_played_are_taken_out_of_the_microphone() {
        // Ten seconds of the far end heard back 50 ms later at half its level, nobody speaking
        // here: what goes out should be far quieter than what came in.
        let (heard, sent) = run(500, 2400, 0.5, true, false);
        let erle = 20.0 * (heard / sent.max(1e-6)).log10();
        assert!(heard > 0.02, "the echo was there to take out ({heard})");
        assert!(erle > 20.0, "the echo was lowered by {erle:.1} dB only");
    }

    #[test]
    fn the_player_is_still_heard_when_nothing_plays() {
        let (heard, sent) = run(250, 0, 0.0, false, true);
        assert!(
            sent > heard * 0.3,
            "the voice was lost: {heard} in, {sent} out"
        );
    }
}
