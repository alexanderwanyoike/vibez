//! On-disk cache for Dropbox-sourced audio files.
//!
//! Keyed by `(path_lower, rev)`; older revisions of the same path
//! coexist in the cache because clips in saved projects pin a
//! specific `rev`. No eviction in v1: if this grows uncomfortably
//! large, a size-capped LRU policy is a follow-up.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DropboxCache {
    root: PathBuf,
}

impl DropboxCache {
    pub fn new() -> Self {
        let root = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vibez")
            .join("dropbox");
        Self { root }
    }

    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a given `(path_lower, rev)` lives on disk.
    ///
    /// `rev` is optional so manual testing paths still produce
    /// deterministic names. In real Dropbox responses `rev` is always
    /// present for files.
    pub fn path_for(&self, path_lower: &str, rev: Option<&str>) -> PathBuf {
        let key = sanitize_path(path_lower);
        let rev_suffix = rev.unwrap_or("norev");
        let ext = extension_of(path_lower).unwrap_or("bin");
        let filename = format!("{key}__{rev_suffix}.{ext}");
        self.root.join(filename)
    }

    pub fn is_cached(&self, path_lower: &str, rev: Option<&str>) -> bool {
        self.path_for(path_lower, rev).is_file()
    }

    /// Create the cache directory if missing. Safe to call every
    /// time; recursive.
    pub fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)
    }

    /// Write `bytes` to the cache path for `(path_lower, rev)`.
    pub fn write(
        &self,
        path_lower: &str,
        rev: Option<&str>,
        bytes: &[u8],
    ) -> std::io::Result<PathBuf> {
        self.ensure_dir()?;
        let target = self.path_for(path_lower, rev);
        std::fs::write(&target, bytes)?;
        Ok(target)
    }
}

impl Default for DropboxCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Turn a Dropbox `path_lower` like `/some/folder/kick 01.wav` into a
/// deterministic, filesystem-safe key that is free of path separators
/// and shell metacharacters.
fn sanitize_path(path_lower: &str) -> String {
    let mut out = String::with_capacity(path_lower.len());
    for c in path_lower.chars() {
        match c {
            '/' | '\\' => out.push('_'),
            c if c.is_ascii_alphanumeric() => out.push(c),
            '.' | '-' | '_' => out.push(c),
            _ => out.push('_'),
        }
    }
    // Avoid leading underscore from root slash.
    out.trim_start_matches('_').to_string()
}

fn extension_of(path: &str) -> Option<&str> {
    path.rsplit_once('.').map(|(_, ext)| ext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn sanitizes_path_separators() {
        let key = sanitize_path("/drums/kick 01.wav");
        assert!(!key.contains('/'));
        assert!(!key.contains(' '));
        assert!(key.ends_with(".wav"));
    }

    #[test]
    fn lowercases_are_preserved() {
        assert_eq!(sanitize_path("/a/B/c.wav"), "a_B_c.wav");
    }

    #[test]
    fn different_revs_produce_different_paths() {
        let cache = DropboxCache::with_root(PathBuf::from("/tmp/vibez-test"));
        let p1 = cache.path_for("/kick.wav", Some("abc"));
        let p2 = cache.path_for("/kick.wav", Some("def"));
        assert_ne!(p1, p2);
    }

    #[test]
    fn same_rev_produces_same_path() {
        let cache = DropboxCache::with_root(PathBuf::from("/tmp/vibez-test"));
        let p1 = cache.path_for("/kick.wav", Some("abc"));
        let p2 = cache.path_for("/kick.wav", Some("abc"));
        assert_eq!(p1, p2);
    }

    #[test]
    fn write_round_trips() {
        let dir = TempDir::new().unwrap();
        let cache = DropboxCache::with_root(dir.path().to_path_buf());
        let path = cache
            .write("/kick.wav", Some("abc"), b"hello")
            .unwrap();
        assert!(path.is_file());
        assert!(cache.is_cached("/kick.wav", Some("abc")));
        assert!(!cache.is_cached("/kick.wav", Some("different")));
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn no_extension_falls_back_to_bin() {
        let cache = DropboxCache::with_root(PathBuf::from("/tmp"));
        let path = cache.path_for("/data", Some("rev"));
        assert!(path.to_string_lossy().ends_with(".bin"));
    }
}
