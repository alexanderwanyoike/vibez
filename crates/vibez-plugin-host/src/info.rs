use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::format::{PluginCategory, PluginFormat, PluginId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: PluginId,
    pub name: String,
    pub vendor: String,
    pub category: PluginCategory,
    pub format: PluginFormat,
    pub path: PathBuf,
}
