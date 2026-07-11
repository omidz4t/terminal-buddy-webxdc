//! Embed the webxdc package and project VERSION into the agent binary.
//!
//! `make build` / `make build-webxdc` places `assets/terminalbuddy.xdc` before
//! `cargo build`. If the file is missing (e.g. plain `cargo check`), an empty
//! blob is embedded so the crate still compiles — runtime will require a path
//! or a rebuild with the mini app present.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // ── VERSION (repo root) ─────────────────────────────────────────────
    let version_file = manifest_dir.join("../VERSION");
    let version = fs::read_to_string(&version_file)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into()));
    println!("cargo:rerun-if-changed={}", version_file.display());
    println!("cargo:rustc-env=TERMINAL_BUDDY_VERSION={version}");

    // ── embedded terminalbuddy.xdc ──────────────────────────────────────
    let xdc_src = manifest_dir.join("assets/terminalbuddy.xdc");
    let xdc_out = out_dir.join("embedded_xdc.bin");
    println!("cargo:rerun-if-changed={}", xdc_src.display());

    if xdc_src.is_file() {
        let bytes = fs::read(&xdc_src).expect("read assets/terminalbuddy.xdc");
        fs::write(&xdc_out, &bytes).expect("write embedded_xdc.bin");
        println!("cargo:rustc-cfg=has_embedded_xdc");
        println!(
            "cargo:warning=TerminalBuddy: embedding webxdc ({} bytes)",
            bytes.len()
        );
    } else {
        // Empty placeholder — `cargo check` / first compile without make.
        fs::write(&xdc_out, []).expect("write empty embedded_xdc.bin");
        println!(
            "cargo:warning=TerminalBuddy: no assets/terminalbuddy.xdc — \
             run `make build` (or build-webxdc + copy) to embed the mini app"
        );
    }
}
