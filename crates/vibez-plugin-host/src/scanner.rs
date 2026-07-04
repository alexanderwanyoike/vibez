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

/// Result of a sandboxed scan: everything found, plus the bundles
/// that could not be scanned (crashed, timed out, or errored).
#[derive(Debug, Clone, Default)]
pub struct ScanReport {
    pub plugins: Vec<PluginInfo>,
    pub failed: Vec<(PathBuf, String)>,
}

/// How long a single plugin bundle gets in the scan subprocess before
/// it is presumed hung. Generous because one .clap can contain a
/// whole suite (LSP ships 100+ plugins in one file).
const SCAN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Scan for plugins, loading each bundle in a throwaway subprocess so
/// a crashing or hanging plugin cannot take the DAW down with it. A
/// bundle that dies is recorded in `ScanReport::failed` and skipped.
///
/// Falls back to the in-process scan if the `vibez-plugin-scan`
/// helper binary is not next to the current executable (e.g. ad-hoc
/// cargo invocations of a single crate).
pub fn scan_plugins_sandboxed(settings: &PluginSettings) -> ScanReport {
    let Some(helper) = helper_binary() else {
        log::warn!("vibez-plugin-scan helper not found; scanning in-process (unsafe)");
        return ScanReport {
            plugins: scan_plugins(settings),
            failed: Vec::new(),
        };
    };

    let mut report = ScanReport::default();
    for (path, format) in collect_plugin_paths(settings) {
        let format_arg = match format {
            PluginFormat::Clap => "clap",
            PluginFormat::Vst3 => "vst3",
        };
        match scan_in_subprocess(&helper, format_arg, &path) {
            Ok(infos) => report.plugins.extend(infos),
            Err(reason) => {
                log::warn!("plugin scan failed for {path:?}: {reason}");
                report.failed.push((path, reason));
            }
        }
    }
    report
}

fn helper_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    // Same directory as the app binary; one level up covers cargo's
    // target/<profile>/examples/ layout.
    [dir, dir.parent()?]
        .iter()
        .map(|d| d.join("vibez-plugin-scan"))
        .find(|p| p.is_file())
}

fn scan_in_subprocess(
    helper: &Path,
    format_arg: &str,
    path: &Path,
) -> Result<Vec<PluginInfo>, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let mut child = Command::new(helper)
        .arg(format_arg)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn scan helper: {e}"))?;

    // Drain stdout/stderr on threads so a chatty plugin cannot fill
    // the pipe and deadlock against our timeout polling.
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let out_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        buf
    });

    let deadline = std::time::Instant::now() + SCAN_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "timed out after {}s (presumed hung)",
                        SCAN_TIMEOUT.as_secs()
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(format!("failed to wait for scan helper: {e}")),
        }
    };

    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();

    if !status.success() {
        let detail = stderr.lines().next().unwrap_or("").trim();
        return Err(match status.code() {
            // A signal death (no exit code on Unix) is the crash case
            // this whole subprocess dance exists for.
            None => format!("crashed during scan ({})", describe_signal(&status)),
            Some(code) if detail.is_empty() => format!("scan helper exited with code {code}"),
            Some(_) => detail.to_string(),
        });
    }

    serde_json::from_str(stdout.trim()).map_err(|e| format!("unparseable scan helper output: {e}"))
}

#[cfg(unix)]
fn describe_signal(status: &std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match status.signal() {
        Some(sig) => format!("signal {sig}"),
        None => "unknown signal".to_string(),
    }
}

#[cfg(not(unix))]
fn describe_signal(_status: &std::process::ExitStatus) -> String {
    "abnormal termination".to_string()
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
