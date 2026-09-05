//! `cargo xtask <cmd>` — repo task runner.
//!
//! - `dist [--target <triple>]`: copy the built host binary + `core.wasm` into `dist/`.
//! - `verify [--turns N] [--seeds a,b,c]`: rebuild the WASM guest, stage it
//!   for the host, build the host, and run the native-vs-WASM differential
//!   check (the gate CI used to run on Ubuntu).

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("dist") => dist(&args),
        Some("verify") => verify(&args),
        _ => {
            eprintln!("usage: cargo xtask <dist [--target <triple>] | verify [--turns N] [--seeds a,b,c]>");
            std::process::exit(2);
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn run(cmd: &str, args: &[&str], dir: &std::path::Path) {
    println!("+ {cmd} {}", args.join(" "));
    let status = std::process::Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("cannot run {cmd}: {e}");
            std::process::exit(1);
        });
    if !status.success() {
        eprintln!("{cmd} failed: {status}");
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn verify(args: &[String]) {
    let root = workspace_root();
    let turns = flag_value(args, "--turns").unwrap_or_else(|| "250".to_string());
    let seeds = flag_value(args, "--seeds").unwrap_or_else(|| "7,1,42,1234,99999".to_string());

    // The guest must be fresh: the host silently falls back to native when
    // core.wasm is missing, and --verify refuses without it.
    run(
        "cargo",
        &[
            "build",
            "--locked",
            "-p",
            "embersong-wasm",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
        ],
        &root,
    );
    let built = root.join("target/wasm32-unknown-unknown/release/embersong_wasm.wasm");
    let staged = root.join("crates/embersong-host/assets/core.wasm");
    std::fs::copy(&built, &staged).unwrap_or_else(|e| {
        eprintln!("cannot stage {}: {e}", staged.display());
        std::process::exit(1);
    });
    println!("staged {}", staged.display());

    run(
        "cargo",
        &["build", "--locked", "-p", "embersong-host"],
        &root,
    );

    let bin = root.join("target/debug").join(format!(
        "embersong-host{}",
        if cfg!(windows) { ".exe" } else { "" }
    ));
    for seed in seeds.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        run(
            bin.to_str().unwrap(),
            &[
                "--headless",
                "--native",
                "--verify",
                "--seed",
                seed,
                "--turns",
                &turns,
            ],
            &root,
        );
    }
    println!("verify clean: seeds [{seeds}] x {turns} turns");
}

fn dist(args: &[String]) {
    let target = flag_value(args, "--target");
    let root = workspace_root();
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
