use std::path::PathBuf;

use crate::format::PluginFormat;

/// Returns the default scan directories for a given plugin format on the current platform.
pub fn default_scan_paths(format: PluginFormat) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    match format {
        PluginFormat::Clap => {
            #[cfg(target_os = "linux")]
            {
                if let Some(home) = dirs::home_dir() {
                    paths.push(home.join(".clap"));
                }
                paths.push(PathBuf::from("/usr/lib/clap"));
                if let Ok(clap_path) = std::env::var("CLAP_PATH") {
                    for p in clap_path.split(':') {
                        paths.push(PathBuf::from(p));
                    }
                }
            }
            #[cfg(target_os = "macos")]
            {
                if let Some(home) = dirs::home_dir() {
                    paths.push(home.join("Library/Audio/Plug-Ins/CLAP"));
                }
                paths.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
            }
            #[cfg(target_os = "windows")]
            {
                if let Ok(common) = std::env::var("COMMONPROGRAMFILES") {
                    paths.push(PathBuf::from(common).join("CLAP"));
                }
            }
        }
        PluginFormat::Vst3 => {
            #[cfg(target_os = "linux")]
            {
                if let Some(home) = dirs::home_dir() {
                    paths.push(home.join(".vst3"));
                }
                paths.push(PathBuf::from("/usr/lib/vst3"));
            }
            #[cfg(target_os = "macos")]
            {
                if let Some(home) = dirs::home_dir() {
                    paths.push(home.join("Library/Audio/Plug-ins/VST3"));
                }
                paths.push(PathBuf::from("/Library/Audio/Plug-ins/VST3"));
            }
            #[cfg(target_os = "windows")]
            {
                paths.push(PathBuf::from(
                    r"C:\Program Files\Common Files\VST3",
                ));
            }
        }
    }

    paths
}

/// Returns all default scan paths for both CLAP and VST3.
pub fn all_default_scan_paths() -> Vec<PathBuf> {
    let mut paths = default_scan_paths(PluginFormat::Clap);
    paths.extend(default_scan_paths(PluginFormat::Vst3));
    paths
}
