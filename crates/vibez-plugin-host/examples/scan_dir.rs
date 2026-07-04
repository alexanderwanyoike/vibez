//! Scan a plugin directory, printing each candidate before it loads
//! so a crashing plugin identifies itself in the output.
//!
//!   cargo run -p vibez-plugin-host --example scan_dir -- <dir>            # in-process (crashy)
//!   cargo run -p vibez-plugin-host --example scan_dir -- --sandboxed <dir> # via subprocess helper

use std::path::PathBuf;

use vibez_plugin_host::scanner;
use vibez_plugin_host::settings::PluginSettings;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let sandboxed = args.iter().any(|a| a == "--sandboxed");
    args.retain(|a| a != "--sandboxed");
    let dir = args
        .first()
        .map(PathBuf::from)
        .expect("usage: scan_dir [--sandboxed] <plugin directory>");

    let settings = PluginSettings {
        extra_scan_paths: vec![dir],
        scan_default_paths: false,
        cache: Vec::new(),
    };

    if sandboxed {
        let report = scanner::scan_plugins_sandboxed(&settings);
        println!("{} plugins found", report.plugins.len());
        for info in &report.plugins {
            println!("  OK: {}", info.name);
        }
        for (path, reason) in &report.failed {
            println!("  SKIPPED: {} ({reason})", path.display());
        }
        println!("--- sandboxed scan completed without crashing ---");
        return;
    }

    let candidates = scanner::collect_plugin_paths(&settings);
    println!("{} candidates:", candidates.len());
    for (path, format) in &candidates {
        println!("  [{format:?}] {}", path.display());
    }

    println!("--- loading one by one ---");
    for (path, format) in &candidates {
        println!("LOADING [{format:?}] {} ...", path.display());
        let single = PluginSettings {
            extra_scan_paths: vec![path.clone()],
            scan_default_paths: false,
            cache: Vec::new(),
        };
        // scan_plugins only walks directories, so scan the parent dir
        // trick doesn't isolate; call the per-format scanners directly.
        let _ = single;
        let result = match format {
            vibez_plugin_host::format::PluginFormat::Clap => {
                vibez_plugin_host::clap_host::scanner::scan_clap(path)
            }
            vibez_plugin_host::format::PluginFormat::Vst3 => {
                vibez_plugin_host::vst3_host::scanner::scan_vst3(path)
            }
        };
        match result {
            Ok(infos) => {
                for info in infos {
                    println!("  OK: {} ({:?})", info.name, info.id);
                }
            }
            Err(e) => println!("  ERR: {e}"),
        }
    }
    println!("--- scan completed without crashing ---");
}
