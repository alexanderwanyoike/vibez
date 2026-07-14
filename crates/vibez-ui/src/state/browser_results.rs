//! Memoized filtered + sorted Local browser results.
//!
//! `view()` reruns on every UI tick, so filtering and sorting the
//! whole catalog per frame is unbounded work on large libraries. The
//! memo is keyed on the catalog revision plus the query/scope inputs
//! and recomputed only when one of them changes; rendering then just
//! windows the cached index lists.

use std::cell::Ref;
use std::path::PathBuf;

use super::{BrowserSearchScope, BrowserState};

#[derive(Debug, Clone, PartialEq)]
struct LocalResultsKey {
    catalog_revision: u64,
    query: String,
    scope: BrowserSearchScope,
    current_folder: Option<PathBuf>,
}

/// Indices into [`BrowserState::folders`] / [`BrowserState::entries`],
/// filtered by the active query/scope and sorted for display
/// (case-insensitive name, then path).
#[derive(Debug, Clone, Default)]
pub struct LocalResults {
    key: Option<LocalResultsKey>,
    pub folders: Vec<usize>,
    pub entries: Vec<usize>,
}

impl BrowserState {
    /// Filtered + sorted Local results for `normalized_query`
    /// (trimmed, lowercased). Recomputed only when the catalog
    /// revision, query, scope, or current folder changes, so calling
    /// this per frame does bounded work.
    pub fn local_results(&self, normalized_query: &str) -> Ref<'_, LocalResults> {
        let cached = self.local_results_cache.borrow();
        let fresh = cached.key.as_ref().is_some_and(|key| {
            key.catalog_revision == self.catalog_revision
                && key.query == normalized_query
                && key.scope == self.search_scope
                && key.current_folder == self.current_folder
        });
        if fresh {
            return cached;
        }
        drop(cached);

        // Precompute the lowercase sort keys once instead of
        // allocating them per comparison.
        let mut folders: Vec<(String, usize)> = self
            .folders
            .iter()
            .enumerate()
            .filter(|(_, folder)| self.local_folder_is_result(folder, normalized_query))
            .map(|(index, folder)| (folder.name.to_lowercase(), index))
            .collect();
        folders.sort_by(|(a_name, a), (b_name, b)| {
            a_name
                .cmp(b_name)
                .then_with(|| self.folders[*a].path.cmp(&self.folders[*b].path))
        });
        let mut entries: Vec<(String, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| self.local_entry_is_result(entry, normalized_query))
            .map(|(index, entry)| (entry.name.to_lowercase(), index))
            .collect();
        entries.sort_by(|(a_name, a), (b_name, b)| {
            a_name.cmp(b_name).then_with(|| {
                self.entries[*a]
                    .relative_path
                    .cmp(&self.entries[*b].relative_path)
            })
        });

        *self.local_results_cache.borrow_mut() = LocalResults {
            key: Some(LocalResultsKey {
                catalog_revision: self.catalog_revision,
                query: normalized_query.to_owned(),
                scope: self.search_scope,
                current_folder: self.current_folder.clone(),
            }),
            folders: folders.into_iter().map(|(_, index)| index).collect(),
            entries: entries.into_iter().map(|(_, index)| index).collect(),
        };
        self.local_results_cache.borrow()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SampleBrowserEntry;
    use vibez_core::track::MediaSourceRef;

    fn entry(root: &str, relative: &str) -> SampleBrowserEntry {
        let root_path = PathBuf::from(root);
        let relative_path = PathBuf::from(relative);
        SampleBrowserEntry {
            source: MediaSourceRef::LocalFile {
                path: root_path.join(&relative_path),
            },
            name: relative_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            root_path,
            search_text: relative_path.display().to_string().to_lowercase(),
            relative_path,
            format: "WAV".into(),
            duration_seconds: None,
            channels: None,
            sample_rate: None,
            file_size: Some(1),
            modified: None,
        }
    }

    #[test]
    fn results_sort_case_insensitively_and_window_over_indices() {
        let mut browser = BrowserState {
            roots: vec![PathBuf::from("/samples")],
            entries: vec![
                entry("/samples", "Zeta.wav"),
                entry("/samples", "alpha.wav"),
                entry("/samples", "Kick.wav"),
            ],
            ..BrowserState::default()
        };
        browser.bump_catalog_revision();

        let results = browser.local_results("wav");
        let names: Vec<&str> = results
            .entries
            .iter()
            .map(|&index| browser.entries[index].name.as_str())
            .collect();
        assert_eq!(names, ["alpha.wav", "Kick.wav", "Zeta.wav"]);
    }

    #[test]
    fn results_are_memoized_until_catalog_query_or_scope_changes() {
        let mut browser = BrowserState {
            roots: vec![PathBuf::from("/samples")],
            entries: vec![entry("/samples", "kick.wav")],
            ..BrowserState::default()
        };
        browser.bump_catalog_revision();
        assert_eq!(browser.local_results("kick").entries.len(), 1);

        // Mutating the catalog without a revision bump proves the memo
        // is served: the stale list is returned unchanged.
        browser.entries.push(entry("/samples", "kick-2.wav"));
        assert_eq!(browser.local_results("kick").entries.len(), 1);

        browser.bump_catalog_revision();
        assert_eq!(browser.local_results("kick").entries.len(), 2);

        // Query and scope participate in the key.
        assert_eq!(browser.local_results("kick-2").entries.len(), 1);
        browser.select_local_folder(Some(PathBuf::from("/samples")));
        assert_eq!(browser.local_results("").entries.len(), 2);
    }
}
