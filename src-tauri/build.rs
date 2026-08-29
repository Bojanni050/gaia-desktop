//! Build script for Gaia Desktop.
//!
//! Generates build metadata at compile time, embedding the build timestamp
//! into the binary. This ensures every Desktop build has a unique identifier.

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

fn main() {
    // Run Tauri's build first — tauri_build::build() returns () in this
    // version, not a Result (it panics internally on failure), so there is
    // nothing to .expect() here.
    tauri_build::build();

    // Generate build metadata file
    generate_build_meta();
}

/// Generates build-meta.json with build timestamp in YYYYMMDDHHmm format (UTC)
fn generate_build_meta() {
    use chrono::Utc;
    
    let build_id = Utc::now().format("%Y%m%d%H%M").to_string();
    
    // Get version from Cargo.toml
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".to_string());
    
    // Get git commit if available (optional)
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string());
    
    let build_meta = serde_json::json!({
        "name": "Gaia Desktop",
        "version": version,
        "build": build_id,
        "commit": commit
    });
    
    // Write to target directory so it can be bundled
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest_path = Path::new(&out_dir).join("build-meta.json");
    
    let mut file = File::create(&dest_path).expect("Failed to create build-meta.json");
    let pretty = serde_json::to_string_pretty(&build_meta).expect("Failed to serialize build-meta.json");
    file.write_all(pretty.as_bytes())
        .expect("Failed to write build-meta.json");
    
    println!("cargo:rustc-env=GAIA_DESKTOP_BUILD_META={}", dest_path.display());
    println!("cargo:rerun-if-changed=build.rs");
}
