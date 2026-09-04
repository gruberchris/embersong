//! `cargo xtask dist` — copy the built host binary + `core.wasm` into `dist/`.

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) != Some("dist") {
        eprintln!("usage: cargo xtask dist [--target <triple>]");
        std::process::exit(2);
    }
    let target = args
        .windows(2)
        .find(|w| w[0] == "--target")
        .map(|w| w[1].clone());

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    // Prefer the triple-qualified dir (`--target <triple>` builds), fall back
    // to the plain profile dir (bare `cargo build --release`).
    let candidates = match &target {
        Some(t) => vec![
            root.join("target").join(t).join("release"),
            root.join("target").join("release"),
        ],
        None => vec![root.join("target").join("release")],
    };
    let label = target.clone().unwrap_or_else(|| "native".to_string());
    let bin_dir = candidates
        .iter()
        .find(|d| d.exists())
        .unwrap_or(&candidates[0]);
    let host_bin = bin_dir.join(format!("embersong-host{}", exe_suffix(&label)));
    if !host_bin.exists() {
        eprintln!("missing {host_bin:?}: build it first:");
        eprintln!("  cargo build -p embersong-host --release --target <triple>");
        std::process::exit(1);
    }
    let dist = root.join("dist").join(&label);
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::copy(&host_bin, dist.join(host_bin.file_name().unwrap())).unwrap();

    let wasm = root.join("target/wasm32-unknown-unknown/release/embersong_wasm.wasm");
    if wasm.exists() {
        std::fs::copy(&wasm, dist.join("core.wasm")).unwrap();
        println!("bundled core.wasm");
    } else {
        println!("note: core.wasm not built; host will use its embedded copy");
    }
    // Ship Bevy assets (vendored font) next to the binary.
    let staged_assets = bin_dir.join("assets");
    if staged_assets.exists() {
        copy_tree(&staged_assets, &dist.join("assets"));
        println!("bundled assets/");
    }
    println!("dist ready: {}", dist.display());
}

fn copy_tree(src: &std::path::Path, dst: &std::path::Path) {
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
            let _ = std::fs::copy(&from, &to);
        }
    }
}

fn exe_suffix(target: &str) -> &'static str {
    if target.contains("windows") {
        ".exe"
    } else {
        ""
    }
}
