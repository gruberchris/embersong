//! Game backend: the WASM guest under wasmtime, with a native fallback.
//!
//! Both paths run the same [`embersong_core`] logic. The host prefers
//! `core.wasm` (the sandboxed moddable build) and falls back to linked-in
//! native code when the file is absent or fails to instantiate.

use embersong_core::{Action, Event, Game};
use embersong_wasm::{Command, CommandResult};

/// A loaded `core.wasm` guest.
pub struct WasmGuest {
    store: wasmtime::Store<()>,
    memory: wasmtime::Memory,
    alloc: wasmtime::TypedFunc<i32, i32>,
    free: wasmtime::TypedFunc<(i32, i32), ()>,
    command: wasmtime::TypedFunc<(i32, i32, i32, i32, i32), i32>,
}

impl WasmGuest {
    /// Search the usual spots for `core.wasm` (or `$EMBERSONG_WASM`).
    pub fn load() -> Result<Self, String> {
        let mut candidates = Vec::new();
        if let Ok(p) = std::env::var("EMBERSONG_WASM") {
            candidates.push(std::path::PathBuf::from(p));
        }
        candidates.push(std::path::PathBuf::from("assets/core.wasm"));
        candidates.push(std::path::PathBuf::from(
            "crates/embersong-host/assets/core.wasm",
        ));
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("core.wasm"));
                candidates.push(dir.join("assets/core.wasm"));
            }
        }
        let path = candidates
            .iter()
            .find(|p| p.exists())
            .cloned()
            .ok_or_else(|| {
                format!("core.wasm not found (tried {candidates:?}); using native core")
            })?;

        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::from_file(&engine, &path)
            .map_err(|e| format!("wasmtime: cannot load {}: {e}", path.display()))?;
        let mut store = wasmtime::Store::new(&engine, ());
        // Pure-computation guest: no imports. If a future guest needs WASI the
        // instantiate below fails and we fall back to native logic.
        let linker = wasmtime::Linker::new(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| format!("wasmtime: instantiate failed: {e}"))?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| "wasmtime: guest exports no `memory`".to_string())?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "embersong_alloc")
            .map_err(|e| format!("wasmtime: missing embersong_alloc: {e}"))?;
        let free = instance
            .get_typed_func::<(i32, i32), ()>(&mut store, "embersong_free")
            .map_err(|e| format!("wasmtime: missing embersong_free: {e}"))?;
        let command = instance
            .get_typed_func::<(i32, i32, i32, i32, i32), i32>(&mut store, "embersong_command")
            .map_err(|e| format!("wasmtime: missing embersong_command: {e}"))?;
        eprintln!("embersong: WASM guest loaded from {}", path.display());
        Ok(Self {
            store,
            memory,
            alloc,
            free,
            command,
        })
    }

    fn roundtrip(&mut self, state: &Game, cmd: &Command) -> Result<CommandResult, String> {
        let state_bytes = serde_json::to_vec(state).map_err(|e| format!("state encode: {e}"))?;
        let cmd_bytes = serde_json::to_vec(cmd).map_err(|e| format!("cmd encode: {e}"))?;

        // Copy inputs into guest memory.
        let s_ptr = self
            .alloc
            .call(&mut self.store, state_bytes.len() as i32)
            .map_err(|e| e.to_string())?;
        let c_ptr = self
            .alloc
            .call(&mut self.store, cmd_bytes.len() as i32)
            .map_err(|e| e.to_string())?;
        // out_len cell (4 bytes).
        let o_ptr = self
            .alloc
            .call(&mut self.store, 4)
            .map_err(|e| e.to_string())?;
        self.memory
            .write(&mut self.store, s_ptr as usize, &state_bytes)
            .map_err(|e| e.to_string())?;
        self.memory
            .write(&mut self.store, c_ptr as usize, &cmd_bytes)
            .map_err(|e| e.to_string())?;

        let r_ptr = self
            .command
            .call(
                &mut self.store,
                (
                    s_ptr,
                    state_bytes.len() as i32,
                    c_ptr,
                    cmd_bytes.len() as i32,
                    o_ptr,
                ),
            )
            .map_err(|e| format!("guest trap: {e}"))?;

        let mut len_buf = [0u8; 4];
        self.memory
            .read(&self.store, o_ptr as usize, &mut len_buf)
            .map_err(|e| e.to_string())?;
        let out_len = u32::from_le_bytes(len_buf) as usize;
        if out_len == 0 || out_len > 8 * 1024 * 1024 {
            return Err(format!("guest returned implausible length {out_len}"));
        }
        let mut out = vec![0u8; out_len];
        self.memory
            .read(&self.store, r_ptr as usize, &mut out)
            .map_err(|e| e.to_string())?;

        self.free
            .call(&mut self.store, (s_ptr, state_bytes.len() as i32))
            .ok();
        self.free
            .call(&mut self.store, (c_ptr, cmd_bytes.len() as i32))
            .ok();
        self.free.call(&mut self.store, (o_ptr, 4)).ok();

        serde_json::from_slice(&out).map_err(|e| format!("result decode: {e}"))
    }
}

/// Where the rules run: WASM sandbox or linked-in native (identical logic).
#[allow(clippy::large_enum_variant)] // Wasm guest owns its store; Native is unit.
pub enum Backend {
    Wasm(WasmGuest),
    Native,
}

impl Backend {
    /// Prefer WASM, fall back to native with a stderr note.
    pub fn prefer_wasm() -> Self {
        match WasmGuest::load() {
            Ok(g) => Backend::Wasm(g),
            Err(note) => {
                eprintln!("embersong: {note}");
                Backend::Native
            }
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Backend::Wasm(_) => "wasm32-unknown-unknown (wasmtime)",
            Backend::Native => "native",
        }
    }

    /// Run one hero turn; on WASM failure, degrade to native for this turn.
    /// `beat_time`: wall-clock seconds since the fight started (bard clock).
    pub fn act(&mut self, game: &mut Game, action: Action, beat_time: Option<f32>) -> Vec<Event> {
        match self {
            Backend::Native => game.act_at(action, beat_time),
            Backend::Wasm(g) => {
                match g.roundtrip(game, &Command::Act { action, beat_time }) {
                    Ok(res) => {
                        // Guest JSON skips the transient RNG; it restarts
                        // deterministically from the seed on next load.
                        *game = res.state;
                        res.events
                    }
                    Err(e) => {
                        eprintln!("embersong: guest failed ({e}); native fallback for this turn");
                        game.act_at(action, beat_time)
                    }
                }
            }
        }
    }
}
