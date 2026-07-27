use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::info::PluginInfo;

const CURRENT_CACHE_REVISION: u32 = 1;

/// Plugin host settings, persisted to `~/.config/vibez/plugins.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSettings {
    pub extra_scan_paths: Vec<PathBuf>,
    pub scan_default_paths: bool,
    pub cache: Vec<PluginInfo>,
    #[serde(default)]
    pub cache_revision: u32,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self {
            extra_scan_paths: Vec::new(),
            scan_default_paths: true,
            cache: Vec::new(),
            cache_revision: CURRENT_CACHE_REVISION,
        }
    }
}

impl PluginSettings {
    pub fn cache_needs_refresh(&self) -> bool {
        self.cache_revision < CURRENT_CACHE_REVISION
    }

    pub fn mark_cache_refreshed(&mut self) {
        self.cache_revision = CURRENT_CACHE_REVISION;
    }

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
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::PluginSettings;

    #[test]
    fn legacy_catalog_requests_exactly_one_refresh() {
        let legacy = r#"{
            "extra_scan_paths": [],
            "scan_default_paths": true,
            "cache": []
        }"#;
        let mut settings: PluginSettings = serde_json::from_str(legacy).unwrap();

        assert!(settings.cache_needs_refresh());

        settings.mark_cache_refreshed();
        let saved = serde_json::to_string(&settings).unwrap();
        let reloaded: PluginSettings = serde_json::from_str(&saved).unwrap();
        assert!(!reloaded.cache_needs_refresh());
    }
}
