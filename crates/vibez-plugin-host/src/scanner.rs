use std::path::{Path, PathBuf};

use crate::format::PluginFormat;
use crate::info::PluginInfo;
use crate::paths;
use crate::settings::PluginSettings;

/// Scan for plugins based on the current settings.
pub fn scan_plugins(settings: &PluginSettings) -> Vec<PluginInfo> {
    let mut results = Vec::new();
    let mut scan_dirs = Vec::new();

    if settings.scan_default_paths {
        scan_dirs.extend(paths::all_default_scan_paths());
    }
    scan_dirs.extend(settings.extra_scan_paths.clone());

    // Deduplicate paths
    scan_dirs.sort();
    scan_dirs.dedup();

    for dir in &scan_dirs {
        if dir.exists() && dir.is_dir() {
            scan_directory(dir, &mut results);
        }
    }

    results
}

fn scan_directory(dir: &Path, results: &mut Vec<PluginInfo>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            // VST3 bundles are directories ending in .vst3
            if let Some(ext) = path.extension() {
                if ext == "vst3" {
                    scan_vst3_bundle(&path, results);
                    continue;
                }
            }
            // Recurse into subdirectories (one level for organization folders)
            scan_directory(&path, results);
        } else if let Some(ext) = path.extension() {
            if ext == "clap" {
                scan_clap_file(&path, results);
            }
        }
    }
}

fn scan_clap_file(path: &Path, results: &mut Vec<PluginInfo>) {
    match crate::clap_host::scanner::scan_clap(path) {
        Ok(infos) => results.extend(infos),
        Err(e) => {
            log::warn!("Failed to scan CLAP plugin {path:?}: {e}");
        }
    }
}

fn scan_vst3_bundle(path: &Path, results: &mut Vec<PluginInfo>) {
    match crate::vst3_host::scanner::scan_vst3(path) {
        Ok(infos) => results.extend(infos),
        Err(e) => {
            log::warn!("Failed to scan VST3 plugin {path:?}: {e}");
        }
    }
}

/// Collect all plugin file paths from scan directories without loading them.
pub fn collect_plugin_paths(settings: &PluginSettings) -> Vec<(PathBuf, PluginFormat)> {
    let mut paths_out = Vec::new();
    let mut scan_dirs = Vec::new();

    if settings.scan_default_paths {
        scan_dirs.extend(paths::all_default_scan_paths());
    }
    scan_dirs.extend(settings.extra_scan_paths.clone());

    scan_dirs.sort();
    scan_dirs.dedup();

    for dir in &scan_dirs {
        if dir.exists() && dir.is_dir() {
            collect_from_directory(dir, &mut paths_out);
        }
    }

    paths_out
}

fn collect_from_directory(dir: &Path, paths_out: &mut Vec<(PathBuf, PluginFormat)>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            if let Some(ext) = path.extension() {
                if ext == "vst3" {
                    paths_out.push((path, PluginFormat::Vst3));
                    continue;
                }
            }
            collect_from_directory(&path, paths_out);
        } else if let Some(ext) = path.extension() {
            if ext == "clap" {
                paths_out.push((path, PluginFormat::Clap));
            }
        }
    }
}
