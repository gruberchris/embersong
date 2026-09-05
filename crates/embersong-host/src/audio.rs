//! Sound bus: the game thread renders synth samples and posts them; a
//! background thread owns the OS audio device. Two sinks share one stream:
//! music (one track at a time, stoppable) and SFX (fire-and-forget layering).
//! No device (CI, headless servers) degrades to silence instead of crashing.

use std::sync::mpsc::{self, Sender};

use bevy::prelude::Resource;
use rodio::Source as _;

enum AudioCmd {
    Sfx(Vec<f32>, u32),
    Music(Vec<f32>, u32),
    StopMusic,
    StopSfx,
}

#[derive(Resource)]
pub struct SoundBus {
    tx: Option<Sender<AudioCmd>>,
}

impl SoundBus {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<AudioCmd>();
        std::thread::Builder::new()
            .name("embersong-audio".into())
            .spawn(move || {
                let stream = rodio::OutputStream::try_default();
                let Ok((_stream, handle)) = stream else {
                    // No audio device: drain the channel forever, silently.
                    while rx.recv().is_ok() {}
                    return;
                };
                let (music_sink, sfx_sink) =
                    match (rodio::Sink::try_new(&handle), rodio::Sink::try_new(&handle)) {
                        (Ok(m), Ok(s)) => (m, s),
                        _ => {
                            while rx.recv().is_ok() {}
                            return;
                        }
                    };
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        AudioCmd::Sfx(samples, rate) => {
                            let src = rodio::buffer::SamplesBuffer::new(1, rate, samples);
                            sfx_sink.append(src);
                        }
                        AudioCmd::Music(samples, rate) => {
                            // Stop-then-loop: the stale track never lingers
                            // under the new one (death -> sing-again left the
                            // old combat loop queued behind menu beds), and
                            // the new track repeats gaplessly until stopped
                            // (combat exit or track change). Single-loop
                            // renders only — the sink does the repeating.
                            music_sink.stop();
                            let looped = rodio::buffer::SamplesBuffer::new(1, rate, samples)
                                .repeat_infinite();
                            music_sink.append(looped);
                        }
                        AudioCmd::StopMusic => music_sink.stop(),
                        // Death/run-end: drop the mash backlog so queued
                        // blows don't keep playing over the End screen.
                        AudioCmd::StopSfx => sfx_sink.stop(),
                    }
                }
            })
            .ok();
        Self { tx: Some(tx) }
    }

    fn send(&self, cmd: AudioCmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }

    /// Layered one-shot (SFX, stings). Never interrupts music.
    pub fn play_sfx(&self, samples: Vec<f32>, rate: u32) {
        self.send(AudioCmd::Sfx(samples, rate));
    }

    /// Music bed/track: stops whatever music is playing, then starts this.
    pub fn play_music(&self, samples: Vec<f32>, rate: u32) {
        self.send(AudioCmd::Music(samples, rate));
    }

    /// Silence music (combat exit, new run). SFX in flight are untouched.
    pub fn stop_music(&self) {
        self.send(AudioCmd::StopMusic);
    }

    /// Drop queued SFX (run end). The caller replays one clean sting after.
    pub fn stop_sfx(&self) {
        self.send(AudioCmd::StopSfx);
    }

    pub fn play_trigger(&self, trigger: embersong_core::SoundTrigger) {
        let s = embersong_synth::render_sfx(trigger);
        self.play_sfx(s.samples, s.rate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_stop_and_switch_never_block() {
        // Regression: death -> sing-again queued the stale combat loop
        // behind menu beds with no way to stop it. Exercise the full
        // lifecycle (also on machines with no audio device, via the
        // drain path).
        let bus = SoundBus::spawn();
        let bed = embersong_synth::render_song_bed(1, false);
        bus.play_music(bed.samples.clone(), bed.rate);
        bus.play_trigger(embersong_core::SoundTrigger::Strike);
        bus.stop_music();
        let track = embersong_synth::render_battle_track(0, 1);
        bus.play_music(track.samples, track.rate);
        bus.stop_music();
    }

    #[test]
    fn sfx_layering_without_device_ok() {
        let bus = SoundBus::spawn();
        for t in [
            embersong_core::SoundTrigger::Strike,
            embersong_core::SoundTrigger::BardHigh,
            embersong_core::SoundTrigger::BardLow,
        ] {
            bus.play_trigger(t);
        }
    }

    #[test]
    fn sfx_stop_clears_backlog() {
        // Regression: mashing actions queued seconds of SFX that kept
        // playing after death. Stop must be sendable mid-spam.
        let bus = SoundBus::spawn();
        for _ in 0..20 {
            bus.play_trigger(embersong_core::SoundTrigger::Strike);
        }
        bus.stop_sfx();
        bus.play_trigger(embersong_core::SoundTrigger::Defeat);
    }
}
