//! Sound bus: the game thread renders synth samples and posts them; a
//! background thread owns the OS audio device. Two sinks share one stream:
//! music (one track at a time, stoppable) and SFX (fire-and-forget layering).
//! No device (CI, headless servers) degrades to silence instead of crashing.

use std::sync::mpsc::{self, Sender};

use bevy::prelude::Resource;

enum AudioCmd {
    Sfx(Vec<f32>, u32),
    Music(Vec<f32>, u32),
    StopMusic,
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
                            // Stop-then-play: the stale track never lingers
                            // under the new one (death -> sing-again left the
                            // old combat loop queued behind menu beds).
                            music_sink.stop();
                            let src = rodio::buffer::SamplesBuffer::new(1, rate, samples);
                            music_sink.append(src);
                        }
                        AudioCmd::StopMusic => music_sink.stop(),
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
}
