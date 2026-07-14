//! Disposable Media Cache for materialized Remote media.
//!
//! Entries are keyed by provider path + revision, so a changed revision never
//! reuses stale bytes or Derived Metadata. The persisted access sequence makes
//! eviction deterministic without relying on filesystem timestamps.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub const DEFAULT_MEDIA_CACHE_BUDGET_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const INDEX_FILE: &str = "index.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaCachePolicy {
    pub budget_bytes: u64,
    pub automatic_eviction: bool,
}

impl Default for MediaCachePolicy {
    fn default() -> Self {
        Self {
            budget_bytes: DEFAULT_MEDIA_CACHE_BUDGET_BYTES,
            automatic_eviction: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DerivedMetadata {
    pub provider_revision: Option<String>,
    pub duration_seconds: f64,
    pub channels: u16,
    pub sample_rate: u32,
    pub bpm: Option<f64>,
    pub bpm_confidence: Option<f32>,
    #[serde(default)]
    pub waveform_peaks: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheRecord {
    path_lower: String,
    revision: Option<String>,
    file_name: String,
    size_bytes: u64,
    last_access_sequence: u64,
    #[serde(default)]
    derived_metadata: Option<DerivedMetadata>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheIndex {
    #[serde(default)]
    access_sequence: u64,
    #[serde(default)]
    entries: HashMap<String, CacheRecord>,
}

#[derive(Debug)]
struct RuntimeState {
    policy: MediaCachePolicy,
    protected: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct DropboxCache {
    root: PathBuf,
    runtime: Arc<Mutex<RuntimeState>>,
}

#[derive(Debug)]
pub struct CacheLease {
    key: String,
    runtime: Arc<Mutex<RuntimeState>>,
}

impl Clone for CacheLease {
    fn clone(&self) -> Self {
        *self
            .runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .protected
            .entry(self.key.clone())
            .or_insert(0) += 1;
        Self {
            key: self.key.clone(),
            runtime: Arc::clone(&self.runtime),
        }
    }
}

impl Drop for CacheLease {
    fn drop(&mut self) {
        let mut runtime = self
            .runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(count) = runtime.protected.get_mut(&self.key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                runtime.protected.remove(&self.key);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheUsage {
    pub bytes: u64,
    pub entries: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheClearReport {
    pub removed_bytes: u64,
    pub removed_entries: usize,
    pub protected_entries: usize,
}

impl DropboxCache {
    pub fn new() -> Self {
        Self::with_policy(MediaCachePolicy::default())
    }

    pub fn with_policy(policy: MediaCachePolicy) -> Self {
        let root = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vibez")
            .join("media");
        Self::with_root_and_policy(root, policy)
    }

    pub fn with_root(root: PathBuf) -> Self {
        Self::with_root_and_policy(root, MediaCachePolicy::default())
    }

    pub fn with_root_and_policy(root: PathBuf, policy: MediaCachePolicy) -> Self {
        Self {
            root,
            runtime: Arc::new(Mutex::new(RuntimeState {
                policy,
                protected: HashMap::new(),
            })),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn policy(&self) -> MediaCachePolicy {
        self.runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .policy
    }

    pub fn set_policy(&self, policy: MediaCachePolicy) -> std::io::Result<CacheUsage> {
        self.runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .policy = policy;
        self.enforce_budget()
    }

    pub fn path_for(&self, path_lower: &str, revision: Option<&str>) -> PathBuf {
        let key = cache_key(path_lower, revision);
        let extension = extension_of(path_lower).unwrap_or("bin");
        self.root.join(format!("{key}.{extension}"))
    }

    pub fn is_cached(&self, path_lower: &str, revision: Option<&str>) -> bool {
        self.path_for(path_lower, revision).is_file()
    }

    /// Return cached bytes' path and persist an LRU touch.
    pub fn lookup(
        &self,
        path_lower: &str,
        revision: Option<&str>,
    ) -> std::io::Result<Option<PathBuf>> {
        let path = self.path_for(path_lower, revision);
        if !path.is_file() {
            return Ok(None);
        }
        let mut index = self.load_index().unwrap_or_default();
        let key = cache_key(path_lower, revision);
        let size = path.metadata()?.len();
        let sequence = next_sequence(&mut index);
        let record = index.entries.entry(key).or_insert_with(|| CacheRecord {
            path_lower: path_lower.into(),
            revision: revision.map(ToOwned::to_owned),
            file_name: path.file_name().unwrap().to_string_lossy().into_owned(),
            size_bytes: size,
            last_access_sequence: sequence,
            derived_metadata: None,
        });
        record.size_bytes = size;
        record.last_access_sequence = sequence;
        self.save_index(&index)?;
        Ok(Some(path))
    }

    pub fn protect(&self, path_lower: &str, revision: Option<&str>) -> CacheLease {
        let key = cache_key(path_lower, revision);
        *self
            .runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .protected
            .entry(key.clone())
            .or_insert(0) += 1;
        CacheLease {
            key,
            runtime: Arc::clone(&self.runtime),
        }
    }

    pub fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)
    }

    /// Atomically commit a complete materialization, then enforce the budget.
    pub fn write(
        &self,
        path_lower: &str,
        revision: Option<&str>,
        bytes: &[u8],
    ) -> std::io::Result<PathBuf> {
        self.ensure_dir()?;
        let target = self.path_for(path_lower, revision);
        let temporary = target.with_extension("materializing");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, &target)?;

        let mut index = self.load_index()?;
        let sequence = next_sequence(&mut index);
        let key = cache_key(path_lower, revision);
        let previous_metadata = index
            .entries
            .get(&key)
            .and_then(|record| record.derived_metadata.clone());
        index.entries.insert(
            key,
            CacheRecord {
                path_lower: path_lower.into(),
                revision: revision.map(ToOwned::to_owned),
                file_name: target.file_name().unwrap().to_string_lossy().into_owned(),
                size_bytes: bytes.len() as u64,
                last_access_sequence: sequence,
                derived_metadata: previous_metadata,
            },
        );
        self.save_index(&index)?;
        self.enforce_budget()?;
        Ok(target)
    }

    pub fn store_derived_metadata(
        &self,
        path_lower: &str,
        revision: Option<&str>,
        metadata: DerivedMetadata,
    ) -> std::io::Result<()> {
        if metadata.provider_revision.as_deref() != revision {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Derived Metadata revision does not match the cache entry",
            ));
        }
        let mut index = self.load_index()?;
        let key = cache_key(path_lower, revision);
        let record = index.entries.get_mut(&key).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "cache entry is missing")
        })?;
        record.derived_metadata = Some(metadata);
        self.save_index(&index)
    }

    pub fn derived_metadata(
        &self,
        path_lower: &str,
        revision: Option<&str>,
    ) -> std::io::Result<Option<DerivedMetadata>> {
        let index = self.load_index()?;
        Ok(index
            .entries
            .get(&cache_key(path_lower, revision))
            .and_then(|record| record.derived_metadata.clone())
            .filter(|metadata| metadata.provider_revision.as_deref() == revision))
    }

    pub fn usage(&self) -> std::io::Result<CacheUsage> {
        let index = self.load_index()?;
        Ok(CacheUsage {
            bytes: index.entries.values().map(|record| record.size_bytes).sum(),
            entries: index.entries.len(),
        })
    }

    pub fn clear(&self) -> std::io::Result<CacheClearReport> {
        let protected = self
            .runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .protected
            .clone();
        let mut index = self.load_index().unwrap_or_default();
        let mut report = CacheClearReport::default();
        index.entries.retain(|key, record| {
            if protected.contains_key(key) {
                report.protected_entries += 1;
                return true;
            }
            let path = self.root.join(&record.file_name);
            match std::fs::remove_file(path) {
                Ok(()) => {
                    report.removed_entries += 1;
                    report.removed_bytes += record.size_bytes;
                    false
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(_) => true,
            }
        });
        let retained_files: std::collections::HashSet<String> = index
            .entries
            .values()
            .map(|record| record.file_name.clone())
            .collect();
        if let Ok(files) = std::fs::read_dir(&self.root) {
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                if name == INDEX_FILE || retained_files.contains(&name) {
                    continue;
                }
                let size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
                if file.file_type().is_ok_and(|kind| kind.is_file())
                    && std::fs::remove_file(file.path()).is_ok()
                {
                    report.removed_entries += 1;
                    report.removed_bytes += size;
                }
            }
        }
        self.save_index(&index)?;
        Ok(report)
    }

    fn enforce_budget(&self) -> std::io::Result<CacheUsage> {
        let runtime = self
            .runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let policy = runtime.policy;
        let protected = runtime.protected.clone();
        drop(runtime);
        let mut index = self.load_index()?;
        let mut usage: u64 = index.entries.values().map(|record| record.size_bytes).sum();
        if policy.automatic_eviction && usage > policy.budget_bytes {
            let mut candidates: Vec<(String, u64)> = index
                .entries
                .iter()
                .filter(|(key, _)| !protected.contains_key(*key))
                .map(|(key, record)| (key.clone(), record.last_access_sequence))
                .collect();
            candidates.sort_by_key(|(_, sequence)| *sequence);
            for (key, _) in candidates {
                if usage <= policy.budget_bytes {
                    break;
                }
                if let Some(record) = index.entries.remove(&key) {
                    let path = self.root.join(&record.file_name);
                    match std::fs::remove_file(path) {
                        Ok(()) => usage = usage.saturating_sub(record.size_bytes),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            usage = usage.saturating_sub(record.size_bytes);
                        }
                        Err(error) => {
                            index.entries.insert(key, record);
                            return Err(error);
                        }
                    }
                }
            }
            self.save_index(&index)?;
        }
        Ok(CacheUsage {
            bytes: usage,
            entries: index.entries.len(),
        })
    }

    fn load_index(&self) -> std::io::Result<CacheIndex> {
        let path = self.root.join(INDEX_FILE);
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(CacheIndex::default()),
            Err(error) => Err(error),
        }
    }

    fn save_index(&self, index: &CacheIndex) -> std::io::Result<()> {
        self.ensure_dir()?;
        let target = self.root.join(INDEX_FILE);
        let temporary = self.root.join("index.json.partial");
        std::fs::write(
            &temporary,
            serde_json::to_vec_pretty(index).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(temporary, target)
    }
}

impl Default for DropboxCache {
    fn default() -> Self {
        Self::new()
    }
}

fn next_sequence(index: &mut CacheIndex) -> u64 {
    index.access_sequence = index.access_sequence.saturating_add(1);
    index.access_sequence
}

fn cache_key(path_lower: &str, revision: Option<&str>) -> String {
    format!(
        "{}__{}",
        sanitize_path(path_lower),
        sanitize_path(revision.unwrap_or("norev"))
    )
}

fn sanitize_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for character in path.chars() {
        match character {
            '/' | '\\' => out.push('_'),
            character if character.is_ascii_alphanumeric() => out.push(character),
            '.' | '-' | '_' => out.push(character),
            _ => out.push('_'),
        }
    }
    out.trim_start_matches('_').to_string()
}

fn extension_of(path: &str) -> Option<&str> {
    path.rsplit_once('.').map(|(_, extension)| extension)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cache(dir: &TempDir, budget_bytes: u64) -> DropboxCache {
        DropboxCache::with_root_and_policy(
            dir.path().to_path_buf(),
            MediaCachePolicy {
                budget_bytes,
                automatic_eviction: true,
            },
        )
    }

    #[test]
    fn revision_changes_identity_and_invalidates_derived_metadata() {
        let dir = TempDir::new().unwrap();
        let cache = cache(&dir, 1_000);
        cache.write("/kick.wav", Some("one"), b"first").unwrap();
        cache
            .store_derived_metadata(
                "/kick.wav",
                Some("one"),
                DerivedMetadata {
                    provider_revision: Some("one".into()),
                    duration_seconds: 1.0,
                    channels: 1,
                    sample_rate: 44_100,
                    ..DerivedMetadata::default()
                },
            )
            .unwrap();
        assert!(cache
            .derived_metadata("/kick.wav", Some("one"))
            .unwrap()
            .is_some());
        assert!(cache
            .derived_metadata("/kick.wav", Some("two"))
            .unwrap()
            .is_none());
        assert_ne!(
            cache.path_for("/kick.wav", Some("one")),
            cache.path_for("/kick.wav", Some("two"))
        );
    }

    #[test]
    fn lru_eviction_is_deterministic_and_touch_updates_recency() {
        let dir = TempDir::new().unwrap();
        let cache = cache(&dir, 8);
        cache.write("/one.wav", Some("1"), b"1111").unwrap();
        cache.write("/two.wav", Some("1"), b"2222").unwrap();
        cache.lookup("/one.wav", Some("1")).unwrap();
        cache.write("/three.wav", Some("1"), b"3333").unwrap();
        assert!(cache.is_cached("/one.wav", Some("1")));
        assert!(!cache.is_cached("/two.wav", Some("1")));
        assert!(cache.is_cached("/three.wav", Some("1")));
    }

    #[test]
    fn protected_entries_survive_eviction_and_clear() {
        let dir = TempDir::new().unwrap();
        let cache = cache(&dir, 4);
        let lease = cache.protect("/active.wav", Some("1"));
        cache.write("/active.wav", Some("1"), b"active").unwrap();
        cache.write("/other.wav", Some("1"), b"other").unwrap();
        assert!(cache.is_cached("/active.wav", Some("1")));
        let report = cache.clear().unwrap();
        assert_eq!(report.protected_entries, 1);
        assert!(cache.is_cached("/active.wav", Some("1")));
        drop(lease);
        let report = cache.clear().unwrap();
        assert_eq!(report.removed_entries, 1);
        assert!(!cache.is_cached("/active.wav", Some("1")));
    }

    #[test]
    fn disabled_eviction_can_exceed_budget_and_reenable_repairs_usage() {
        let dir = TempDir::new().unwrap();
        let cache = DropboxCache::with_root_and_policy(
            dir.path().to_path_buf(),
            MediaCachePolicy {
                budget_bytes: 4,
                automatic_eviction: false,
            },
        );
        cache.write("/one.wav", Some("1"), b"1111").unwrap();
        cache.write("/two.wav", Some("1"), b"2222").unwrap();
        assert_eq!(cache.usage().unwrap().bytes, 8);
        let usage = cache
            .set_policy(MediaCachePolicy {
                budget_bytes: 4,
                automatic_eviction: true,
            })
            .unwrap();
        assert_eq!(usage.bytes, 4);
        assert_eq!(usage.entries, 1);
    }

    #[test]
    fn clear_removes_orphan_and_interrupted_materializations_but_not_the_index() {
        let dir = TempDir::new().unwrap();
        let cache_root = dir.path().join("media-cache");
        let cache = DropboxCache::with_root_and_policy(
            cache_root.clone(),
            MediaCachePolicy {
                budget_bytes: 1_000,
                automatic_eviction: true,
            },
        );
        let project_media = dir.path().join("project-media.wav");
        let remote_catalog = dir.path().join("remote-catalog.json");
        std::fs::write(&project_media, b"project-owned").unwrap();
        std::fs::write(&remote_catalog, b"metadata").unwrap();
        cache.write("/known.wav", Some("1"), b"known").unwrap();
        std::fs::write(cache_root.join("orphan.wav"), b"orphan").unwrap();
        std::fs::write(cache_root.join("stale.materializing"), b"partial").unwrap();
        let report = cache.clear().unwrap();
        assert_eq!(report.removed_entries, 3);
        assert!(cache_root.join(INDEX_FILE).is_file());
        assert!(!cache_root.join("orphan.wav").exists());
        assert!(!cache_root.join("stale.materializing").exists());
        assert_eq!(std::fs::read(project_media).unwrap(), b"project-owned");
        assert_eq!(std::fs::read(remote_catalog).unwrap(), b"metadata");
    }

    #[test]
    fn write_is_complete_and_usage_is_persisted_across_instances() {
        let dir = TempDir::new().unwrap();
        let first = cache(&dir, 1_000);
        let path = first.write("/kick.wav", Some("abc"), b"hello").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"hello");
        assert!(!dir.path().join("kick.wav__abc.materializing").exists());
        let reopened = cache(&dir, 1_000);
        assert_eq!(
            reopened.usage().unwrap(),
            CacheUsage {
                bytes: 5,
                entries: 1
            }
        );
        assert!(reopened.lookup("/kick.wav", Some("abc")).unwrap().is_some());
    }
}
