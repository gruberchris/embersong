//! Host build script:
//! 1. Vendor a system TTF into `assets/fonts/body.ttf` so Bevy text renders on
//!    every target without shipping a font binary in git.
//! 2. Stage `assets/**` next to the built binary (`target/<profile>/assets`),
//!    which is where Bevy 0.15 resolves asset paths from at runtime. This
//!    keeps `cargo run -p embersong-host` working from any directory, and
//!    `cargo xtask dist` bundles the same tree.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dest = manifest.join("assets/fonts/body.ttf");
    if !dest.exists() {
        vendor_font(&dest);
    }
    stage_assets(&manifest);
}

fn vendor_font(dest: &Path) {
    let candidates: &[&str] = if cfg!(target_os = "windows") {
        &[
            "C:\\Windows\\Fonts\\arial.ttf",
            "C:\\Windows\\Fonts\\calibri.ttf",
        ]
    } else if cfg!(target_os = "macos") {
        &[
            "/Library/Fonts/Arial.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ]
    } else {
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        ]
    };
    if let Some(src) = candidates.iter().map(Path::new).find(|p| p.exists()) {
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::copy(src, dest).unwrap();
        println!(
            "cargo:warning=embersong-host: vendored font from {}",
            src.display()
        );
    } else {
        println!("cargo:warning=embersong-host: no system font found; UI text may not render");
    }
}

/// Copy `assets/**` next to the binary so the runtime file-asset reader finds
/// it regardless of the working directory. `OUT_DIR` looks like
/// `target/<profile>/build/<pkg>-<hash>/out`.
fn stage_assets(manifest: &Path) {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let profile_dir = match out.ancestors().nth(3) {
        Some(d) => d.to_path_buf(),
        None => return,
    };
    // Never stage into a borrowed target dir of another workspace layout; the
    // ancestors check above already anchors us at target/<profile>.
    let src_dir = manifest.join("assets");
    let dst_dir = profile_dir.join("assets");
    if !src_dir.exists() {
        return;
    }
    copy_tree(&src_dir, &dst_dir);
    // Also stage the WASM guest beside the binary when it has been built —
    // the backend probes the executable's directory first.
    for cand in [manifest.join("../../target/wasm32-unknown-unknown/release/embersong_wasm.wasm")] {
        if cand.exists() {
            let _ = std::fs::copy(&cand, profile_dir.join("core.wasm"));
            let _ = std::fs::copy(&cand, dst_dir.join("core.wasm"));
        }
    }
    println!("cargo:rerun-if-changed=assets/");
    println!("cargo:rerun-if-changed=build.rs");
}

fn copy_tree(src: &Path, dst: &Path) {
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    let _ = std::fs::create_dir_all(dst);
    for entry in entries.flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else if from.is_file() {
            // Skip the 300KB guest unless explicitly staged above; dev copies
            // it via the documented `cp` step (keeps rebuilds quiet).
            if from.extension().map(|e| e == "wasm").unwrap_or(false) {
                continue;
            }
            let _ = std::fs::copy(&from, &to);
        }
    }
}
