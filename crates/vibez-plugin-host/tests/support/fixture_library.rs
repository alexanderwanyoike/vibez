//! Finds the routing fixture's shared library by asking cargo to build it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Cargo's build directory layout is private (1.100 moved test binaries out of
/// `deps/`) and artifact dependencies are unstable, so the `compiler-artifact`
/// message is the stable way to learn where the cdylib landed.
pub fn path() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(build)
}

fn build() -> PathBuf {
    let output = Command::new(env!("CARGO"))
        .args(["build", "--message-format=json", "--manifest-path"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/Cargo.toml"
        ))
        .output()
        .expect("cargo should start");
    assert!(
        output.status.success(),
        "Fixture library build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let name = format!(
        "{}vibez_routing_fixture{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .flat_map(|message| message["filenames"].as_array().cloned().unwrap_or_default())
        .filter_map(|file| file.as_str().map(PathBuf::from))
        .find(|file| file.file_name().is_some_and(|file| file == name.as_str()))
        .expect("cargo reported no fixture library artifact")
}
