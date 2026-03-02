use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::info::PluginInfo;

/// Plugin host settings, persisted to `~/.config/vibez/plugins.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSettings {
    pub extra_scan_paths: Vec<PathBuf>,
    pub scan_default_paths: bool,
    pub cache: Vec<PluginInfo>,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self {
            extra_scan_paths: Vec::new(),
            scan_default_paths: true,
            cache: Vec::new(),
        }
    }
}

impl PluginSettings {
    /// Path to the settings JSON file.
    pub fn settings_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vibez")
            .join("plugins.json")
    }

    /// Load settings from disk. Returns default if file doesn't exist or can't be parsed.
    pub fn load() -> Self {
        let path = Self::settings_path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save settings to disk.
    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::settings_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)?;
        std::fs::write(&path, json)
    }
}
