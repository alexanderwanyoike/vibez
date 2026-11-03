//! Finds samples in local folders and the Browser's Dropbox catalog, and fetches Dropbox files into Vibez's media cache.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vibez_dropbox::catalog::{RemoteCatalogEntry, RemoteCatalogStore};
use vibez_dropbox::{
    load_app_key_with_env_override, DropboxCache, DropboxClient, DropboxEntry, DropboxSettings,
};

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
    #[error("could not decode {path}: {reason}")]
    Decode { path: PathBuf, reason: String },
}

/// What a sample is, so an agent can choose loops that fit the project.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SampleInfo {
    pub path: PathBuf,
    pub seconds: f64,
    pub channels: usize,
    pub sample_rate: u32,
    /// Vibez's tempo estimate, when the sample is rhythmic enough to have one.
    pub estimated_bpm: Option<f64>,
    pub bpm_confidence: Option<f32>,
    pub peak_dbfs: f32,
}

pub fn inspect(path: &Path) -> Result<SampleInfo, SampleLibraryError> {
    let _ = path;
    todo!()
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
        let local_roots = match std::env::var_os(SAMPLE_DIRS_ENV) {
            Some(value) => std::env::split_paths(&value).collect(),
            None => dirs::home_dir()
                .map(|home| vec![home.join("Music").join("Samples")])
                .unwrap_or_default(),
        };
        Self::new(
            local_roots,
            Some(RemoteCatalogStore::for_dropbox()),
            DropboxCache::new(),
        )
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
        let words: Vec<String> = query
            .split_whitespace()
            .map(|word| word.to_lowercase())
            .collect();
        if words.is_empty() {
            return Err(SampleLibraryError::EmptyQuery);
        }
        let matches = |path: &str| {
            let lower = path.to_lowercase();
            words.iter().all(|word| lower.contains(word.as_str()))
        };
        let mut hits = Vec::new();
        if source != SampleSource::Dropbox {
            let mut files = Vec::new();
            for root in &self.local_roots {
                collect_audio_files(root, &mut files);
            }
            files.sort();
            hits.extend(
                files
                    .into_iter()
                    .filter(|file| matches(&file.to_string_lossy()))
                    .map(|file| SampleHit {
                        source: SampleSource::Local,
                        path: file.to_string_lossy().into_owned(),
                        size_bytes: std::fs::metadata(&file).ok().map(|m| m.len()),
                        local_path: Some(file),
                    }),
            );
        }
        if source != SampleSource::Local && self.catalog.is_some() {
            let mut remote: Vec<&RemoteCatalogEntry> = self
                .catalog_entries()?
                .iter()
                .filter(|entry| matches(&entry.path))
                .collect();
            remote.sort_by(|a, b| a.path.cmp(&b.path));
            hits.extend(remote.into_iter().map(|entry| SampleHit {
                source: SampleSource::Dropbox,
                path: entry.path.clone(),
                size_bytes: entry.size,
                local_path: self.cached_path(entry),
            }));
        }
        hits.truncate(limit);
        Ok(hits)
    }

    /// The catalog entry for a Dropbox path, as shown in search results.
    pub fn dropbox_entry(&self, path: &str) -> Result<RemoteCatalogEntry, SampleLibraryError> {
        let lower = path.trim().to_lowercase();
        self.catalog_entries()?
            .iter()
            .find(|entry| entry.provider_item_id == lower)
            .cloned()
            .ok_or_else(|| SampleLibraryError::NotInCatalog(path.to_string()))
    }

    /// Downloads a Dropbox sample into Vibez's media cache (or reuses it) and
    /// returns the local path.
    pub async fn fetch(&self, path: &str) -> Result<PathBuf, SampleLibraryError> {
        let entry = self.dropbox_entry(path)?;
        if let Some(cached) = self.cached_path(&entry) {
            return Ok(cached);
        }
        let mut settings = DropboxSettings::load();
        let tokens = settings
            .tokens
            .clone()
            .ok_or(SampleLibraryError::NotConnected)?;
        let app_key =
            load_app_key_with_env_override(&settings).ok_or(SampleLibraryError::NotConnected)?;
        let client = DropboxClient::new(app_key, tokens.clone());
        let local = client
            .download_to_cache(
                &DropboxEntry {
                    path_lower: entry.provider_item_id.clone(),
                    path_display: entry.path.clone(),
                    name: entry.name.clone(),
                    is_folder: false,
                    rev: entry.revision.clone(),
                    size: entry.size,
                },
                &self.cache,
            )
            .await
            .map_err(|error| SampleLibraryError::Download(error.to_string()))?;
        // The client refreshes expired access tokens; keeping the new one
        // spares the app a refresh round-trip on its next start.
        let refreshed = client.tokens().await;
        if refreshed.access_token != tokens.access_token {
            settings.tokens = Some(refreshed);
            if let Err(error) = settings.save() {
                eprintln!("vibez-mcp: could not store refreshed Dropbox tokens: {error}");
            }
        }
        Ok(local)
    }

    fn catalog_entries(&self) -> Result<&[RemoteCatalogEntry], SampleLibraryError> {
        let Some(store) = &self.catalog else {
            return Ok(&[]);
        };
        // The catalog can hold hundreds of thousands of entries; it is read
        // once and only audio files are kept.
        self.entries
            .get_or_init(|| {
                store.load().map(|snapshot| {
                    snapshot
                        .entries
                        .into_iter()
                        .filter(RemoteCatalogEntry::is_supported_audio)
                        .collect()
                })
            })
            .as_deref()
            .map_err(|error| SampleLibraryError::Catalog(error.clone()))
    }

    fn cached_path(&self, entry: &RemoteCatalogEntry) -> Option<PathBuf> {
        let revision = entry.revision.as_deref();
        self.cache
            .is_cached(&entry.provider_item_id, revision)
            .then(|| self.cache.path_for(&entry.provider_item_id, revision))
    }
}

fn collect_audio_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let hidden = entry.file_name().to_string_lossy().starts_with('.');
        if kind.is_dir() && !hidden {
            collect_audio_files(&path, files);
        } else if kind.is_file() && is_audio(&path) {
            files.push(path);
        }
    }
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(vibez_core::audio_format::audio_format_for_extension)
        .is_some()
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

    #[test]
    fn inspecting_a_sample_reports_its_shape_and_tempo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clicks.wav");
        let rate = 44_100;
        let mut wave = vec![0f32; rate * 8];
        // A short burst every half second is 120 BPM.
        for beat in 0..16 {
            for i in 0..400 {
                wave[beat * rate / 2 + i] = if i % 2 == 0 { 0.8 } else { -0.8 };
            }
        }
        vibez_audio_io::file_io::write_wav_file(
            &path,
            &vibez_core::audio_buffer::DecodedAudio {
                channels: vec![wave],
                sample_rate: rate as u32,
            },
        )
        .unwrap();
        let info = inspect(&path).unwrap();
        assert_eq!(info.channels, 1);
        assert!((info.seconds - 8.0).abs() < 0.01);
        let bpm = info.estimated_bpm.expect("a steady pulse has a tempo");
        assert!((bpm - 120.0).abs() < 2.0, "estimated {bpm}");
        assert!(matches!(
            inspect(&dir.path().join("missing.wav")),
            Err(SampleLibraryError::Decode { .. })
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
