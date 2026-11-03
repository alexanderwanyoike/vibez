//! Finds samples in local folders and the Browser's Dropbox catalog, and fetches Dropbox files into Vibez's media cache.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vibez_dropbox::catalog::{RemoteCatalogEntry, RemoteCatalogStore};
use vibez_dropbox::DropboxCache;

/// Colon-separated (semicolon on Windows) folders searched for local samples.
pub const SAMPLE_DIRS_ENV: &str = "VIBEZ_MCP_SAMPLE_DIRS";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SampleSource {
    Local,
    Dropbox,
    #[default]
    Any,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SampleHit {
    pub source: SampleSource,
    /// A local file path, or the Dropbox path to pass to `fetch_sample`.
    pub path: String,
    pub size_bytes: Option<u64>,
    /// Where the file can be read now; Dropbox hits only have one once fetched.
    pub local_path: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum SampleLibraryError {
    #[error("the search needs at least one word")]
    EmptyQuery,
    #[error("the Dropbox catalog could not be read: {0}")]
    Catalog(String),
    #[error("{0} is not an audio file in the Dropbox catalog")]
    NotInCatalog(String),
    #[error("Dropbox is not connected; connect it once in Vibez's Browser")]
    NotConnected,
    #[error("Dropbox download failed: {0}")]
    Download(String),
}

pub struct SampleLibrary {
    local_roots: Vec<PathBuf>,
    catalog: Option<RemoteCatalogStore>,
    cache: DropboxCache,
    entries: OnceLock<Result<Vec<RemoteCatalogEntry>, String>>,
}

impl SampleLibrary {
    pub fn new(
        local_roots: Vec<PathBuf>,
        catalog: Option<RemoteCatalogStore>,
        cache: DropboxCache,
    ) -> Self {
        Self {
            local_roots,
            catalog,
            cache,
            entries: OnceLock::new(),
        }
    }

    /// Local folders from [`SAMPLE_DIRS_ENV`] (default `~/Music/Samples`), the
    /// app's Dropbox catalog and the app's media cache.
    pub fn from_environment() -> Self {
        todo!()
    }

    pub fn local_roots(&self) -> &[PathBuf] {
        &self.local_roots
    }

    /// Audio files whose path contains every word of `query`, case-insensitively.
    pub fn search(
        &self,
        query: &str,
        source: SampleSource,
        limit: usize,
    ) -> Result<Vec<SampleHit>, SampleLibraryError> {
        let _ = (query, source, limit);
        todo!()
    }

    /// The catalog entry for a Dropbox path, as shown in search results.
    pub fn dropbox_entry(&self, path: &str) -> Result<RemoteCatalogEntry, SampleLibraryError> {
        let _ = path;
        todo!()
    }

    /// Downloads a Dropbox sample into Vibez's media cache (or reuses it) and
    /// returns the local path.
    pub async fn fetch(&self, path: &str) -> Result<PathBuf, SampleLibraryError> {
        let _ = path;
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_dropbox::catalog::RemoteCatalogSnapshot;

    fn entry(path: &str, folder: bool) -> RemoteCatalogEntry {
        RemoteCatalogEntry {
            provider_item_id: path.to_ascii_lowercase(),
            path: path.into(),
            parent_path: String::new(),
            name: path.rsplit('/').next().unwrap().into(),
            is_folder: folder,
            revision: Some("r1".into()),
            size: Some(1_000),
            derived_metadata: None,
        }
    }

    fn library(dir: &Path) -> SampleLibrary {
        let local = dir.join("local");
        std::fs::create_dir_all(local.join("Breaks/Funk")).unwrap();
        std::fs::write(local.join("Breaks/Funk/Amen 136.wav"), b"x").unwrap();
        std::fs::write(local.join("Breaks/Funk/notes.txt"), b"x").unwrap();
        std::fs::write(local.join("kick.wav"), b"x").unwrap();
        let store = RemoteCatalogStore::at(dir.join("catalog.json"));
        store
            .save(&RemoteCatalogSnapshot {
                entries: vec![
                    entry("/Packs/Norman Cook", true),
                    entry("/Packs/Norman Cook/BTS DHALL0LL.wav", false),
                    entry("/Packs/Norman Cook/readme.txt", false),
                    entry("/Packs/Other/Kick 1.wav", false),
                ],
                ..RemoteCatalogSnapshot::default()
            })
            .unwrap();
        SampleLibrary::new(
            vec![local],
            Some(store),
            DropboxCache::with_root(dir.join("cache")),
        )
    }

    #[test]
    fn search_matches_every_word_across_both_sources() {
        let dir = tempfile::tempdir().unwrap();
        let lib = library(dir.path());
        let hits = lib.search("funk AMEN", SampleSource::Any, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, SampleSource::Local);
        assert!(hits[0]
            .local_path
            .as_ref()
            .unwrap()
            .ends_with("Amen 136.wav"));

        let hits = lib
            .search("norman cook", SampleSource::Dropbox, 10)
            .unwrap();
        assert_eq!(hits.len(), 1, "folders and non-audio files are skipped");
        assert_eq!(hits[0].path, "/Packs/Norman Cook/BTS DHALL0LL.wav");
        assert_eq!(hits[0].local_path, None);
    }

    #[test]
    fn source_and_limit_narrow_results() {
        let dir = tempfile::tempdir().unwrap();
        let lib = library(dir.path());
        let both = lib.search("kick", SampleSource::Any, 10).unwrap();
        assert_eq!(both.len(), 2);
        assert_eq!(
            lib.search("kick", SampleSource::Local, 10).unwrap().len(),
            1
        );
        assert_eq!(lib.search("kick", SampleSource::Any, 1).unwrap().len(), 1);
        assert!(matches!(
            lib.search("   ", SampleSource::Any, 10),
            Err(SampleLibraryError::EmptyQuery)
        ));
    }

    #[test]
    fn cached_dropbox_files_report_their_local_path() {
        let dir = tempfile::tempdir().unwrap();
        let lib = library(dir.path());
        let cache = DropboxCache::with_root(dir.path().join("cache"));
        cache
            .write("/packs/other/kick 1.wav", Some("r1"), b"audio")
            .unwrap();
        let hit = &lib.search("kick 1", SampleSource::Dropbox, 10).unwrap()[0];
        assert!(hit.local_path.as_ref().unwrap().exists());
    }

    #[test]
    fn dropbox_lookups_resolve_display_paths_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let lib = library(dir.path());
        let found = lib
            .dropbox_entry("/packs/norman cook/bts dhall0ll.wav")
            .unwrap();
        assert_eq!(found.name, "BTS DHALL0LL.wav");
        assert!(matches!(
            lib.dropbox_entry("/Packs/Norman Cook/readme.txt"),
            Err(SampleLibraryError::NotInCatalog(_))
        ));
    }

    #[tokio::test]
    async fn fetching_a_cached_file_needs_no_connection() {
        let dir = tempfile::tempdir().unwrap();
        let lib = library(dir.path());
        DropboxCache::with_root(dir.path().join("cache"))
            .write("/packs/other/kick 1.wav", Some("r1"), b"audio")
            .unwrap();
        let path = lib.fetch("/Packs/Other/Kick 1.wav").await.unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"audio");
    }
}
