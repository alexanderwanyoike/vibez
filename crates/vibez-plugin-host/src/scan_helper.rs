//! Out-of-process plugin scan helper entry point.
//!
//! Loads ONE plugin bundle in a throwaway process and prints its
//! `Vec<PluginInfo>` as JSON on stdout. Plugins execute arbitrary
//! native code the moment they are dlopen'd, and a misbehaving one
//! (e.g. a segfaulting VST3 factory) must kill that process, not the
//! DAW. The parent treats a non-zero / signal exit as "plugin is bad,
//! skip it" and keeps scanning.
//!
//! Shared by two identical `vibez-plugin-scan` binaries: one in this
//! crate (for `cargo run -p vibez-plugin-host` workflows) and one in
//! the root `vibez` package, which exists so a plain `cargo build`
//! of the app also puts the helper next to the `vibez` executable.
//! Without it the sandboxed scan silently falls back to in-process.
//!
//!   vibez-plugin-scan <clap|vst3> <path>

use std::path::PathBuf;

/// Run the helper; returns the process exit code
/// (0 = ok, 1 = scan failed, 2 = bad usage).
pub fn run() -> u8 {
    let mut args = std::env::args().skip(1);
    let (Some(format), Some(path)) = (args.next(), args.next()) else {
        eprintln!("usage: vibez-plugin-scan <clap|vst3> <path>");
        return 2;
    };
    let path = PathBuf::from(path);

    let result = match format.as_str() {
        "clap" => crate::clap_host::scanner::scan_clap(&path),
        "vst3" => crate::vst3_host::scanner::scan_vst3(&path),
        other => {
            eprintln!("unknown plugin format: {other}");
            return 2;
        }
    };

    match result {
        Ok(infos) => match serde_json::to_string(&infos) {
            Ok(json) => {
                println!("{json}");
                0
            }
            Err(e) => {
                eprintln!("failed to serialize scan result: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
