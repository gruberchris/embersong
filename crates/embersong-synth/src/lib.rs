//! Procedural audio for Embersong: every SFX and bard-song is synthesized at
//! runtime from oscillators + noise. No audio files, no downloads, MIT-clean.
//!
//! The synth is backend-free: it renders mono `f32` samples (and 16-bit WAV
//! bytes) that the desktop host hands to its audio output.

use std::f32::consts::PI;

use embersong_core::SoundTrigger;

/// Sample rate for all rendered audio.
pub const SAMPLE_RATE: u32 = 22_050;

/// A rendered mono sound: samples plus the rate they were rendered at.
#[derive(Debug, Clone)]
pub struct Sound {
    pub samples: Vec<f32>,
    pub rate: u32,
}

impl Sound {
    /// Encode as 16-bit PCM mono WAV bytes (for file export / audio sources).
    pub fn to_wav_bytes(&self) -> Vec<u8> {
        let n = self.samples.len() as u32;
        let mut out = Vec::with_capacity(44 + n as usize * 2);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + n * 2).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&self.rate.to_le_bytes());
        out.extend_from_slice(&(self.rate * 2).to_le_bytes()); // byte rate
        out.extend_from_slice(&2u16.to_le_bytes()); // block align
        out.extend_from_slice(&16u16.to_le_bytes()); // bits
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(n * 2).to_le_bytes());
        for s in &self.samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }
}

// --- tiny deterministic noise (no RNG dependency in synth) ------------------

struct Noise {
    state: u32,
}

impl Noise {
    fn new(seed: u32) -> Self {
        Self { state: seed.max(1) }
    }
    fn next(&mut self) -> f32 {
        // xorshift32 -> [-1, 1]
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        (self.state as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

// --- primitives ---------------------------------------------------------------

fn sine(freq: f32, t: f32) -> f32 {
    (2.0 * PI * freq * t).sin()
}

/// ADSR-ish pluck: attack 5ms, exponential decay.
fn pluck(freq: f32, dur: f32, rate: u32, brightness: f32) -> Vec<f32> {
    let n = (dur * rate as f32) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let env = (-4.5 * t / dur).exp() * (1.0 - (-t * 400.0).exp());
            sine(freq, t) * env + brightness * sine(freq * 2.0, t) * env * 0.4
        })
        .collect()
}

fn thump(dur: f32, rate: u32, seed: u32) -> Vec<f32> {
    let n = (dur * rate as f32) as usize;
    let mut noise = Noise::new(seed);
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let env = (-9.0 * t / dur).exp();
            (sine(90.0 - 50.0 * t / dur, t) * 0.8 + noise.next() * 0.35) * env
        })
        .collect()
}

fn shimmer(freqs: &[f32], dur: f32, rate: u32) -> Vec<f32> {
    let n = (dur * rate as f32) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let env = (1.0 - (-t * 30.0).exp()) * (-2.5 * t / dur).exp();
            freqs.iter().map(|f| sine(*f, t)).sum::<f32>() / freqs.len() as f32 * env
        })
        .collect()
}

fn mix(a: Vec<f32>, b: Vec<f32>) -> Vec<f32> {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| a.get(i).copied().unwrap_or(0.0) * 0.7 + b.get(i).copied().unwrap_or(0.0) * 0.7)
        .collect()
}

// --- public API -----------------------------------------------------------------

/// Render a one-shot effect for a combat event.
pub fn render_sfx(trigger: SoundTrigger) -> Sound {
    let rate = SAMPLE_RATE;
    let samples = match trigger {
        SoundTrigger::Strike => mix(pluck(196.0, 0.18, rate, 0.5), thump(0.12, rate, 11)),
        SoundTrigger::HeroHurt => mix(pluck(110.0, 0.25, rate, 0.7), thump(0.2, rate, 77)),
        SoundTrigger::MonsterDie => shimmer(&[330.0, 415.0, 494.0, 660.0], 0.6, rate),
        SoundTrigger::Heal => shimmer(&[523.0, 659.0, 784.0], 0.5, rate),
        SoundTrigger::Miss => pluck(880.0, 0.09, rate, 0.2),
        SoundTrigger::Tame => shimmer(&[392.0, 494.0, 587.0, 784.0, 988.0], 0.9, rate),
        SoundTrigger::Song => shimmer(&[262.0, 330.0, 392.0, 523.0], 0.7, rate),
        SoundTrigger::Enrage => mix(pluck(65.0, 0.5, rate, 0.9), thump(0.4, rate, 5)),
        SoundTrigger::Flee => shimmer(&[523.0, 392.0, 262.0], 0.5, rate),
        SoundTrigger::Victory => shimmer(&[523.0, 659.0, 784.0, 1047.0, 1319.0], 1.2, rate),
        SoundTrigger::Defeat => shimmer(&[392.0, 330.0, 262.0, 196.0], 1.2, rate),
    };
    Sound { samples, rate }
}

/// Render a looping bard-song bed (2 bars) for title / tavern / combat.
pub fn render_song_bed(bars: usize, combat: bool) -> Sound {
    let rate = SAMPLE_RATE;
    // D-minor-ish pentatonic wander, brighter + faster in combat.
    let seq = if combat {
        [294.0, 349.0, 392.0, 440.0, 523.0, 440.0, 392.0, 349.0]
    } else {
        [262.0, 294.0, 330.0, 392.0, 440.0, 392.0, 330.0, 294.0]
    };
    let note_dur = if combat { 0.22 } else { 0.34 };
    let mut samples = Vec::new();
    for _ in 0..bars {
        for f in seq {
            samples.extend(pluck(f, note_dur, rate, 0.35));
        }
    }
    // Gentle fade on the tail so loops don't click.
    let fade = (0.1 * rate as f32) as usize;
    for (i, s) in samples.iter_mut().rev().take(fade).enumerate() {
        *s *= 1.0 - i as f32 / fade as f32;
    }
    Sound { samples, rate }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_trigger_renders_non_silent_audio() {
        let triggers = [
            SoundTrigger::Strike,
            SoundTrigger::HeroHurt,
            SoundTrigger::MonsterDie,
            SoundTrigger::Heal,
            SoundTrigger::Miss,
            SoundTrigger::Tame,
            SoundTrigger::Song,
            SoundTrigger::Enrage,
            SoundTrigger::Flee,
            SoundTrigger::Victory,
            SoundTrigger::Defeat,
        ];
        for t in triggers {
            let s = render_sfx(t);
            assert!(!s.samples.is_empty(), "{t:?} empty");
            assert!(s.samples.iter().any(|v| v.abs() > 0.01), "{t:?} silent");
            assert!(s.samples.iter().all(|v| v.is_finite()), "{t:?} non-finite");
            // WAV header sanity: RIFF....WAVEfmt
            let wav = s.to_wav_bytes();
            assert_eq!(&wav[0..4], b"RIFF");
            assert_eq!(&wav[8..12], b"WAVE");
        }
    }

    #[test]
    fn song_beds_loop_cleanly() {
        let calm = render_song_bed(2, false);
        let battle = render_song_bed(2, true);
        assert!(calm.samples.len() < battle.samples.len() || calm.samples.len() > 1000);
        assert!(battle.samples.iter().any(|v| v.abs() > 0.01));
        // Tail fades to near-silence for click-free loops.
        assert!(calm.samples.last().copied().unwrap_or(1.0).abs() < 0.05);
    }
}
