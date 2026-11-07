//! On-disk snapshot of a remote connection's file metadata, shared by the app's Browser and headless tools.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::DerivedMetadata;

pub const DROPBOX_PROVIDER_ID: &str = "dropbox";
pub const DROPBOX_CONNECTION_ID: &str = "dropbox-primary";
/// Display label for the Dropbox connection. This ships to every user and
/// is written into saved projects, so it names the provider, never a person.
pub const DROPBOX_CONNECTION_NAME: &str = "Dropbox";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteCatalogEntry {
    pub provider_item_id: String,
    pub path: String,
    pub parent_path: String,
    pub name: String,
    pub is_folder: bool,
    pub revision: Option<String>,
    pub size: Option<u64>,
    #[serde(default)]
    pub derived_metadata: Option<DerivedMetadata>,
}

impl RemoteCatalogEntry {
    pub fn is_supported_audio(&self) -> bool {
        !self.is_folder
            && self
                .name
                .rsplit_once('.')
                .and_then(|(_, extension)| {
                    vibez_core::audio_format::audio_format_for_extension(extension)
                })
                .is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteCatalogSnapshot {
    pub provider_id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub checkpoint: Option<String>,
    pub entries: Vec<RemoteCatalogEntry>,
}

impl Default for RemoteCatalogSnapshot {
    fn default() -> Self {
        Self {
            provider_id: DROPBOX_PROVIDER_ID.into(),
            connection_id: DROPBOX_CONNECTION_ID.into(),
            connection_name: DROPBOX_CONNECTION_NAME.into(),
            checkpoint: None,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteCatalogStore {
    path: PathBuf,
}

impl RemoteCatalogStore {
    pub fn for_dropbox() -> Self {
        let base = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("vibez")
            .join("remote-catalogs");
        Self {
            path: base.join(format!("{DROPBOX_CONNECTION_ID}.json")),
        }
    }

    /// A store at an explicit path, for tools and tests that do not use the app's data folder.
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<RemoteCatalogSnapshot, String> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("remote catalog is invalid: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(RemoteCatalogSnapshot::default())
            }
            Err(error) => Err(format!("could not read remote catalog: {error}")),
        }
    }

    pub fn save(&self, snapshot: &RemoteCatalogSnapshot) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or("remote catalog path has no parent")?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create remote catalog folder: {error}"))?;
        // Unique per writer so overlapping saves (e.g. a progress save racing
        // a metadata save) cannot rename each other's half-written temp file.
        static SAVE_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temporary = self.path.with_extension(format!(
            "json.{}-{}.partial",
            std::process::id(),
            SAVE_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let bytes = serde_json::to_vec_pretty(snapshot)
            .map_err(|error| format!("could not encode remote catalog: {error}"))?;
        std::fs::write(&temporary, bytes)
            .map_err(|error| format!("could not write remote catalog: {error}"))?;
        std::fs::rename(&temporary, &self.path)
            .map_err(|error| format!("could not commit remote catalog: {error}"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> RemoteCatalogEntry {
        let path_lower = path.to_ascii_lowercase();
        RemoteCatalogEntry {
            parent_path: path_lower
                .rsplit_once('/')
                .map(|(parent, _)| parent.to_string())
                .unwrap_or_default(),
            provider_item_id: path_lower,
            path: path.into(),
            name: path.rsplit('/').next().unwrap().into(),
            is_folder: false,
            revision: Some("1".into()),
            size: Some(42),
            derived_metadata: None,
        }
    }

    #[test]
    fn store_round_trips_and_reports_corruption_without_erasing_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = RemoteCatalogStore::at(dir.path().join("catalog.json"));
        let snapshot = RemoteCatalogSnapshot {
            entries: vec![entry("/Megalodon/Kick.wav")],
            ..RemoteCatalogSnapshot::default()
        };
        store.save(&snapshot).unwrap();
        assert_eq!(store.load().unwrap(), snapshot);
        std::fs::write(store.path(), b"not-json").unwrap();
        assert!(store.load().unwrap_err().contains("invalid"));
        assert_eq!(std::fs::read(store.path()).unwrap(), b"not-json");
    }

    #[test]
    fn support_filter_uses_the_shared_audio_matrix() {
        assert!(entry("/Idea.M4A").is_supported_audio());
        assert!(!entry("/notes.txt").is_supported_audio());
        assert!(!entry("/raw.aac").is_supported_audio());
    }
}
