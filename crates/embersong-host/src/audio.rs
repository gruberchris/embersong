//! Fire-and-forget sound bus: the game thread renders synth samples and posts
//! them; a background thread owns the OS audio device. No device (CI,
//! headless servers) degrades to silence instead of crashing.

use std::sync::mpsc::{self, Sender};

use bevy::prelude::Resource;

#[derive(Resource)]
pub struct SoundBus {
    tx: Option<Sender<(Vec<f32>, u32)>>,
}

impl SoundBus {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<(Vec<f32>, u32)>();
        std::thread::Builder::new()
            .name("embersong-audio".into())
            .spawn(move || {
                let stream = rodio::OutputStream::try_default();
                let Ok((_stream, handle)) = stream else {
                    // No audio device: drain the channel forever, silently.
                    while rx.recv().is_ok() {}
                    return;
                };
                let sink = match rodio::Sink::try_new(&handle) {
                    Ok(s) => s,
                    Err(_) => {
                        while rx.recv().is_ok() {}
                        return;
                    }
                };
                while let Ok((samples, rate)) = rx.recv() {
                    let src = rodio::buffer::SamplesBuffer::new(1, rate, samples);
                    sink.append(src);
                }
            })
            .ok();
        Self { tx: Some(tx) }
    }

    /// Post rendered samples; never blocks the game loop.
    pub fn play(&self, samples: Vec<f32>, rate: u32) {
        if let Some(tx) = &self.tx {
            let _ = tx.send((samples, rate));
        }
    }

    pub fn play_trigger(&self, trigger: embersong_core::SoundTrigger) {
        let s = embersong_synth::render_sfx(trigger);
        self.play(s.samples, s.rate);
    }
}
