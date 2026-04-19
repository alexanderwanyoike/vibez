use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSettings {
    #[serde(default)]
    pub sample_library_roots: Vec<PathBuf>,
    #[serde(default = "default_sample_browser_open")]
    pub sample_browser_open: bool,
    /// Automatically detect each dropped sample's BPM and warp it to
    /// the project tempo on import. Off by default; users opt in from
    /// Settings → Warping.
    #[serde(default)]
    pub auto_warp_on_import: bool,
    /// Minimum BPM-detector confidence below which import-time auto-
    /// warp refuses to stretch. 0.0 warps everything (even bad
    /// guesses); 1.0 means only stretch when the detector is very
    /// sure. Default is a moderate gate.
    #[serde(default = "default_warp_confidence_threshold")]
    pub warp_confidence_threshold: f32,
    /// Name of the external MIDI input port to auto-connect on
    /// startup. `None` means auto-pick the first visible port.
    #[serde(default)]
    pub preferred_midi_input: Option<String>,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            sample_library_roots: Vec::new(),
            sample_browser_open: default_sample_browser_open(),
            auto_warp_on_import: false,
            warp_confidence_threshold: default_warp_confidence_threshold(),
            preferred_midi_input: None,
        }
    }
}

impl UiSettings {
    pub fn settings_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vibez")
            .join("ui.json")
    }

    pub fn load() -> Self {
        let path = Self::settings_path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::settings_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }
}

fn default_sample_browser_open() -> bool {
    true
}

fn default_warp_confidence_threshold() -> f32 {
    0.6
}
