//! WASM guest ABI over [`embersong_core`].
//!
//! The desktop host runs this module under `wasmtime` (target
//! `wasm32-unknown-unknown`, zero imports — not even WASI — so the empty
//! linker accepts it). The ABI is deliberately tiny — JSON in, JSON out — so
//! the host needs no bindgen and mods can implement the same three exports.
//!
//! ```text
//! embersong_alloc(len) -> ptr          reserve `len` bytes in guest memory
//! embersong_free(ptr, len)             release them
//! embersong_command(state_ptr, state_len, cmd_ptr, cmd_len, out_len_ptr) -> ptr
//!     state: JSON `embersong_core::Game`
//!     cmd:   JSON `Command` (see below)
//!     out:   JSON `CommandResult` written to guest memory, length via out_len_ptr
//! ```

use embersong_core::{Action, Event, Game};
use serde::{Deserialize, Serialize};

/// A host request: either drive one hero turn or (de)serialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum Command {
    /// Run one hero turn: `{ "op": "act", "action": <Action> }`.
    /// `beat_time` is the wall-clock seconds since the fight started;
    /// `None`/absent falls back to the turn-stepped bard clock.
    Act {
        action: Action,
        #[serde(default)]
        beat_time: Option<f32>,
    },
    /// Fresh run: `{ "op": "new", "seed": 123 }`.
    New { seed: u64 },
}

/// Guest reply: the new state plus UI/audio events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub state: Game,
    pub events: Vec<Event>,
}

/// Native helper (also used by host tests): apply a command without WASM.
pub fn dispatch(state: &Game, cmd: &Command) -> CommandResult {
    match cmd {
        Command::New { seed } => CommandResult {
            state: Game::new(*seed),
            events: vec![],
        },
        Command::Act { action, beat_time } => {
            let mut state = state.clone();
            let events = state.act_at(*action, *beat_time);
            CommandResult { state, events }
        }
    }
}

// --- raw exports (wasm32 only) -------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod guest {
    use super::*;

    use std::ptr::addr_of_mut;

    /// Bump buffer for the current reply. Single-threaded guest: fine.
    static mut REPLY: Vec<u8> = Vec::new();

    /// Replace the reply buffer and return (ptr, len). Uses raw addressing so
    /// no shared reference to the mutable static is ever created.
    unsafe fn set_reply(bytes: Vec<u8>, out_len_ptr: *mut usize) -> *const u8 {
        *addr_of_mut!(REPLY) = bytes;
        let reply = addr_of_mut!(REPLY);
        if !out_len_ptr.is_null() {
            *out_len_ptr = (*reply).len();
        }
        (*reply).as_ptr()
    }

    #[no_mangle]
    pub extern "C" fn embersong_alloc(len: usize) -> *mut u8 {
        let mut buf = Vec::<u8>::with_capacity(len);
        let ptr = buf.as_mut_ptr();
        std::mem::forget(buf);
        ptr
    }

    #[no_mangle]
    pub extern "C" fn embersong_free(ptr: *mut u8, len: usize) {
        if ptr.is_null() || len == 0 {
            return;
        }
        unsafe {
            let _ = Vec::from_raw_parts(ptr, 0, len);
        }
    }

    #[no_mangle]
    pub extern "C" fn embersong_command(
        state_ptr: *const u8,
        state_len: usize,
        cmd_ptr: *const u8,
        cmd_len: usize,
        out_len_ptr: *mut usize,
    ) -> *const u8 {
        let fail = |msg: &str| {
            let bytes = format!(r#"{{"error":{msg:?}}}"#).into_bytes();
            unsafe { set_reply(bytes, out_len_ptr) }
        };
        if state_ptr.is_null() || cmd_ptr.is_null() || out_len_ptr.is_null() {
            return fail("null pointer");
        }
        let state_bytes = unsafe { std::slice::from_raw_parts(state_ptr, state_len) };
        let cmd_bytes = unsafe { std::slice::from_raw_parts(cmd_ptr, cmd_len) };
        let state: Game = match serde_json::from_slice(state_bytes) {
            Ok(g) => g,
            Err(e) => return fail(&format!("bad state: {e}")),
        };
        let cmd: Command = match serde_json::from_slice(cmd_bytes) {
            Ok(c) => c,
            Err(e) => return fail(&format!("bad command: {e}")),
        };
        let result = dispatch(&state, &cmd);
        match serde_json::to_vec(&result) {
            Ok(bytes) => unsafe { set_reply(bytes, out_len_ptr) },
            Err(e) => fail(&format!("encode: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embersong_core::Song;

    #[test]
    fn dispatch_new_and_act_roundtrip() {
        let fresh = Game::new(5);
        let state_json = serde_json::to_vec(&fresh).unwrap();

        // New game command.
        let r = dispatch(&fresh, &Command::New { seed: 5 });
        assert_eq!(r.state.seed, 5);
        assert!(r.state.current.is_some());

        // Strike turn via JSON (the exact bytes the host sends).
        let cmd = serde_json::to_vec(&Command::Act {
            action: Action::Strike,
            beat_time: Some(0.44),
        })
        .unwrap();
        let state: Game = serde_json::from_slice(&state_json).unwrap();
        let cmd_decoded: Command = serde_json::from_slice(&cmd).unwrap();
        let r = dispatch(&state, &cmd_decoded);
        assert!(!r.events.is_empty());
        assert!(r
            .events
            .iter()
            .any(|e| matches!(e, embersong_core::Event::BardBeat { .. })));
        let _ = serde_json::to_vec(&r).unwrap();
    }

    #[test]
    fn dispatch_song_and_soothe() {
        let g = Game::new(11);
        let r = dispatch(
            &g,
            &Command::Act {
                action: Action::Song(Song::MothLullaby),
                beat_time: None,
            },
        );
        assert!(!r.events.is_empty());
        let r = dispatch(
            &r.state,
            &Command::Act {
                action: Action::Soothe,
                beat_time: None,
            },
        );
        assert!(!r.events.is_empty());
    }

    #[test]
    fn dispatch_legacy_act_without_beat_time() {
        // Pre-bard hosts send act with no clock.
        let g = Game::new(7);
        let cmd: Command = serde_json::from_str(r#"{"op":"Act","action":"Strike"}"#).unwrap();
        let r = dispatch(&g, &cmd);
        assert!(!r.events.is_empty());
    }
}
