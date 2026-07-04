//! Out-of-process plugin scan helper.
//!
//! Loads ONE plugin bundle in this throwaway process and prints its
//! `Vec<PluginInfo>` as JSON on stdout. Plugins execute arbitrary
//! native code the moment they are dlopen'd, and a misbehaving one
//! (e.g. a segfaulting VST3 factory) must kill this process, not the
//! DAW. The parent treats a non-zero / signal exit as "plugin is bad,
//! skip it" and keeps scanning.
//!
//!   vibez-plugin-scan <clap|vst3> <path>

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(format), Some(path)) = (args.next(), args.next()) else {
        eprintln!("usage: vibez-plugin-scan <clap|vst3> <path>");
        return ExitCode::from(2);
    };
    let path = PathBuf::from(path);

    let result = match format.as_str() {
        "clap" => vibez_plugin_host::clap_host::scanner::scan_clap(&path),
        "vst3" => vibez_plugin_host::vst3_host::scanner::scan_vst3(&path),
        other => {
            eprintln!("unknown plugin format: {other}");
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(infos) => match serde_json::to_string(&infos) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("failed to serialize scan result: {e}");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
