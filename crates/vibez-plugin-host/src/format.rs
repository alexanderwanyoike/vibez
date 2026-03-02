use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PluginFormat {
    Vst3,
    Clap,
}

impl PluginFormat {
    pub fn name(self) -> &'static str {
        match self {
            PluginFormat::Vst3 => "VST3",
            PluginFormat::Clap => "CLAP",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            PluginFormat::Vst3 => "vst3",
            PluginFormat::Clap => "clap",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PluginCategory {
    Effect,
    Instrument,
    Both,
}

impl PluginCategory {
    pub fn is_effect(self) -> bool {
        matches!(self, PluginCategory::Effect | PluginCategory::Both)
    }

    pub fn is_instrument(self) -> bool {
        matches!(self, PluginCategory::Instrument | PluginCategory::Both)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginId {
    pub format: PluginFormat,
    pub uid: String,
}
