//! Browser domain: the sample library, Dropbox browsing state, and
//! drag-and-drop from the browser into the arrangement.
//!
//! Only the synchronous state transitions live here. Anything that
//! spawns work (file dialogs, decode, Dropbox HTTP) stays in app.rs
//! as an iced Task and routes results back through these messages.

use std::path::PathBuf;

use vibez_core::id::TrackId;
use vibez_core::track::MediaSourceRef;
use vibez_dropbox::DropboxEntry;

use crate::message::SampleLibraryScanResult;
use crate::state::{BrowserState, SampleBrowserMode};

/// Messages the browser domain handles (sync tranche).
#[derive(Debug, Clone)]
pub enum BrowserMsg {
    ToggleSampleBrowser,
    BeginDockResize,
    ResizeDock(f32),
    EndDockResize,
    NudgeDockWidth(f32),
    SampleBrowserSearchChanged(String),
    SelectLocalFolder(Option<PathBuf>),
    ToggleLocalFolder(PathBuf),
    CycleSearchScope,
    ShowMoreLocalResults,
    SelectSampleBrowserEntry(MediaSourceRef),
    SetSampleBrowserMode(SampleBrowserMode),
    StartDragSample {
        source: MediaSourceRef,
        label: String,
    },
    /// Fired by a clip canvas whenever the cursor moves while a sample
    /// drag is in flight and the cursor is inside that lane.
    DragHoverTrack {
        track_id: TrackId,
        beat: f64,
    },
    EndDragSample,
    SampleLibraryScanned(Result<SampleLibraryScanResult, String>),
    RemoveSampleLibraryRoot(PathBuf),
    DropboxCollapseFolder(String),
    DropboxSelectEntry(DropboxEntry),
    SetDropboxAppKey(String),
}

/// Cross-domain effects requested by a browser update.
#[derive(Debug, Default, PartialEq)]
pub struct BrowserAction {
    /// Status bar text.
    pub status: Option<String>,
    /// UI settings changed (browser visibility, library roots).
    pub persist_settings: bool,
    /// Dropbox mode was entered with no cached root listing; the
    /// router should kick off a root list_folder call if a client is
    /// connected.
    pub expand_dropbox_root: bool,
    /// A drag was released over a lane: import `source` onto this
    /// track at this beat.
    pub drop_on_arrangement: Option<(TrackId, f64, MediaSourceRef)>,
    /// Decode a Local selection for the truthful Audition waveform without
    /// starting playback.
    pub load_waveform: Option<MediaSourceRef>,
}

impl BrowserState {
    pub fn update(&mut self, msg: BrowserMsg) -> BrowserAction {
        let mut action = BrowserAction::default();
        match msg {
            BrowserMsg::ToggleSampleBrowser => {
                self.open = !self.open;
                action.persist_settings = true;
            }
            BrowserMsg::BeginDockResize => {
                self.dock_resize_active = true;
            }
            BrowserMsg::ResizeDock(width) => {
                if self.dock_resize_active {
                    self.set_dock_width(width);
                }
            }
            BrowserMsg::EndDockResize => {
                if self.dock_resize_active {
                    self.dock_resize_active = false;
                    action.persist_settings = true;
                }
            }
            BrowserMsg::NudgeDockWidth(delta) => {
                self.set_dock_width(self.dock_width + delta);
                action.persist_settings = true;
            }
            BrowserMsg::SampleBrowserSearchChanged(query) => {
                self.search = query;
                self.reset_results_window();
            }
            BrowserMsg::SelectLocalFolder(folder) => {
                self.select_local_folder(folder);
            }
            BrowserMsg::ToggleLocalFolder(folder) => {
                if !self.expanded_local_folders.remove(&folder) {
                    self.expanded_local_folders.insert(folder);
                }
            }
            BrowserMsg::CycleSearchScope => {
                self.cycle_search_scope();
            }
            BrowserMsg::ShowMoreLocalResults => {
                self.results_visible_limit = self
                    .results_visible_limit
                    .saturating_add(crate::state::BROWSER_RESULTS_PAGE_SIZE);
            }
            BrowserMsg::SelectSampleBrowserEntry(source) => {
                if self.select_source(source.clone()) {
                    action.load_waveform = Some(source);
                }
            }
            BrowserMsg::SetSampleBrowserMode(mode) => {
                self.mode = mode;
                if mode == crate::state::SampleBrowserMode::Dropbox
                    && !self.dropbox.folders.contains_key("")
                    && !self.dropbox.listing_in_progress.contains("")
                {
                    // The router only acts on this when a Dropbox
                    // client is actually connected.
                    action.expand_dropbox_root = true;
                }
            }
            BrowserMsg::StartDragSample { source, label } => {
                action.status = Some(format!("Dragging {label} - drop on a lane or drum pad"));
                self.drag_source = Some(source);
                self.drag_label = Some(label);
                self.drag_hover_track = None;
                self.drag_hover_beat = 0.0;
            }
            BrowserMsg::DragHoverTrack { track_id, beat } => {
                self.drag_hover_track = Some(track_id);
                self.drag_hover_beat = beat;
            }
            BrowserMsg::EndDragSample => {
                if let Some(source) = self.drag_source.take() {
                    self.drag_label = None;
                    // If a drop target was hovered recently, route the drop
                    // there instead of cancelling. Protects against
                    // sub-pixel release-outside-bounds misses.
                    if let Some(track_id) = self.drag_hover_track.take() {
                        let beat = self.drag_hover_beat;
                        self.drag_hover_beat = 0.0;
                        action.drop_on_arrangement = Some((track_id, beat, source));
                        return action;
                    }
                    self.drag_hover_beat = 0.0;
                    action.status = Some("Drag cancelled".to_string());
                }
            }
            BrowserMsg::SampleLibraryScanned(result) => {
                self.scan_in_progress = false;
                match result {
                    Ok(scan) => {
                        self.entries = scan.entries;
                        self.folders = scan.folders;
                        self.scan_warnings = scan.warnings;
                        self.scan_error = None;
                        let current_exists = self.current_folder.as_ref().is_none_or(|current| {
                            self.roots.iter().any(|root| root == current)
                                || self.folders.iter().any(|folder| &folder.path == current)
                        });
                        if !current_exists {
                            self.select_local_folder(None);
                        }
                        if self
                            .selected_source
                            .as_ref()
                            .and_then(|selected| {
                                self.entries.iter().find(|entry| &entry.source == selected)
                            })
                            .is_none()
                        {
                            self.clear_selection();
                        }
                        action.status = Some(if self.scan_warnings.is_empty() {
                            format!("Indexed {} samples", self.entries.len())
                        } else {
                            format!(
                                "Indexed {} samples with {} warning(s)",
                                self.entries.len(),
                                self.scan_warnings.len()
                            )
                        });
                    }
                    Err(err) => {
                        self.scan_error = Some(err.clone());
                        action.status = Some(format!("Sample scan error: {err}"));
                    }
                }
            }
            BrowserMsg::RemoveSampleLibraryRoot(path) => {
                self.roots.retain(|root| root != &path);
                if self
                    .current_folder
                    .as_ref()
                    .is_some_and(|folder| folder.starts_with(&path))
                {
                    self.select_local_folder(None);
                }
                self.entries.retain(|entry| entry.root_path != path);
                self.folders.retain(|folder| folder.root_path != path);
                self.expanded_local_folders
                    .retain(|folder| !folder.starts_with(&path));
                self.scan_warnings.clear();
                self.scan_error = None;
                if self
                    .selected_source
                    .as_ref()
                    .and_then(|selected| {
                        self.entries.iter().find(|entry| &entry.source == selected)
                    })
                    .is_none()
                {
                    self.clear_selection();
                }
                action.persist_settings = true;
                action.status = Some("Removed sample root".to_string());
            }
            BrowserMsg::DropboxCollapseFolder(path) => {
                self.dropbox.expanded.remove(&path);
            }
            BrowserMsg::DropboxSelectEntry(entry) => {
                self.dropbox.selected_path = Some(entry.path_lower.clone());
                self.select_source(MediaSourceRef::DropboxFile {
                    path_lower: entry.path_lower,
                    display_path: entry.path_display,
                    rev: entry.rev,
                });
            }
            BrowserMsg::SetDropboxAppKey(key) => {
                self.dropbox.app_key_input = key;
            }
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_persists_settings() {
        let mut b = BrowserState::default();
        let action = b.update(BrowserMsg::ToggleSampleBrowser);
        assert!(!b.open);
        assert!(action.persist_settings);
    }

    #[test]
    fn dock_width_clamps_and_persists_only_when_committed() {
        let mut browser = BrowserState::default();

        let action = browser.update(BrowserMsg::BeginDockResize);
        assert!(!action.persist_settings);
        browser.update(BrowserMsg::ResizeDock(900.0));
        assert_eq!(browser.dock_width, crate::state::BROWSER_DOCK_MAX_WIDTH);
        let action = browser.update(BrowserMsg::EndDockResize);
        assert!(action.persist_settings);

        let action = browser.update(BrowserMsg::NudgeDockWidth(-1_000.0));
        assert_eq!(browser.dock_width, crate::state::BROWSER_DOCK_MIN_WIDTH);
        assert!(action.persist_settings);
    }

    #[test]
    fn dock_yields_to_arrange_without_forgetting_preference() {
        let mut browser = BrowserState::default();
        browser.set_dock_width(620.0);
        assert_eq!(browser.effective_dock_width(1_500.0), 620.0);
        assert_eq!(browser.effective_dock_width(900.0), 340.0);
        assert_eq!(browser.dock_width, 620.0);
        assert!((browser.places_pane_width(900.0) - 124.0).abs() < f32::EPSILON * 100.0);
    }

    #[test]
    fn resizing_keeps_one_places_and_results_shell_without_changing_context() {
        let mut browser = BrowserState {
            search: "break".into(),
            current_folder: Some(PathBuf::from("/samples")),
            selected_source: Some(MediaSourceRef::LocalFile {
                path: PathBuf::from("/samples/break.wav"),
            }),
            ..BrowserState::default()
        };
        browser.set_dock_width(350.0);
        assert!((browser.places_pane_width(1_400.0) - 126.0).abs() < f32::EPSILON * 100.0);
        browser.set_dock_width(460.0);
        assert!((browser.places_pane_width(1_400.0) - 165.6).abs() < f32::EPSILON * 100.0);
        browser.set_dock_width(580.0);
        assert!((browser.places_pane_width(1_400.0) - 176.0).abs() < f32::EPSILON * 100.0);
        assert_eq!(browser.search, "break");
        assert_eq!(browser.current_folder, Some(PathBuf::from("/samples")));
        assert!(browser.selected_source.is_some());
    }

    #[test]
    fn results_table_promotes_metadata_to_columns_only_when_space_allows() {
        let mut browser = BrowserState::default();

        browser.set_dock_width(crate::state::BROWSER_DOCK_MIN_WIDTH);
        assert!(!browser.results_use_wide_columns(1_400.0));

        browser.set_dock_width(crate::state::BROWSER_DOCK_DEFAULT_WIDTH);
        assert!(!browser.results_use_wide_columns(1_400.0));

        browser.set_dock_width(crate::state::BROWSER_DOCK_MAX_WIDTH);
        assert!(browser.results_use_wide_columns(1_400.0));
    }

    #[test]
    fn changing_selection_clears_waveform_and_rejects_stale_decode() {
        let first = MediaSourceRef::LocalFile {
            path: PathBuf::from("/samples/first.wav"),
        };
        let second = MediaSourceRef::LocalFile {
            path: PathBuf::from("/samples/second.wav"),
        };
        let audio = std::sync::Arc::new(vibez_core::audio_buffer::DecodedAudio {
            channels: vec![vec![0.0, 0.8, -0.4]],
            sample_rate: 44_100,
        });
        let mut browser = BrowserState::default();

        browser.select_source(first.clone());
        browser.begin_waveform_load(&first);
        assert!(browser.install_waveform(first.clone(), std::sync::Arc::clone(&audio)));
        assert!(browser.waveform_audio.is_some());

        browser.select_source(second);
        assert!(browser.waveform_audio.is_none());
        assert!(!browser.install_waveform(first, audio));
        assert!(browser.waveform_audio.is_none());
    }

    #[test]
    fn end_drag_over_lane_requests_drop() {
        let mut b = BrowserState::default();
        let tid = TrackId::new();
        let source = MediaSourceRef::LocalFile {
            path: PathBuf::from("/tmp/kick.wav"),
        };
        b.update(BrowserMsg::StartDragSample {
            source: source.clone(),
            label: "kick.wav".to_string(),
        });
        b.update(BrowserMsg::DragHoverTrack {
            track_id: tid,
            beat: 8.0,
        });
        let action = b.update(BrowserMsg::EndDragSample);
        assert_eq!(action.drop_on_arrangement, Some((tid, 8.0, source)));
        assert!(b.drag_source.is_none());
        assert!(b.drag_label.is_none());
    }

    #[test]
    fn end_drag_without_target_cancels() {
        let mut b = BrowserState::default();
        b.update(BrowserMsg::StartDragSample {
            source: MediaSourceRef::LocalFile {
                path: PathBuf::from("/tmp/kick.wav"),
            },
            label: "kick.wav".to_string(),
        });
        let action = b.update(BrowserMsg::EndDragSample);
        assert_eq!(action.drop_on_arrangement, None);
        assert_eq!(action.status.as_deref(), Some("Drag cancelled"));
    }

    #[test]
    fn dropbox_mode_requests_root_listing_once() {
        let mut b = BrowserState::default();
        let action = b.update(BrowserMsg::SetSampleBrowserMode(SampleBrowserMode::Dropbox));
        assert!(action.expand_dropbox_root);
        b.dropbox.folders.insert(String::new(), Vec::new());
        let action = b.update(BrowserMsg::SetSampleBrowserMode(SampleBrowserMode::Dropbox));
        assert!(!action.expand_dropbox_root);
    }

    #[test]
    fn remove_root_clears_dependent_selection_and_filter() {
        let mut b = BrowserState::default();
        let root = PathBuf::from("/samples");
        b.roots.push(root.clone());
        b.current_folder = Some(root.clone());
        let action = b.update(BrowserMsg::RemoveSampleLibraryRoot(root));
        assert!(b.roots.is_empty());
        assert_eq!(b.current_folder, None);
        assert!(action.persist_settings);
        assert_eq!(action.status.as_deref(), Some("Removed sample root"));
    }

    #[test]
    fn removing_a_root_never_mutates_source_storage() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("Source Storage");
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("kick.wav");
        std::fs::write(&source, b"source bytes").unwrap();
        let mut browser = BrowserState {
            roots: vec![root.clone()],
            current_folder: Some(root.clone()),
            ..BrowserState::default()
        };

        browser.update(BrowserMsg::RemoveSampleLibraryRoot(root));

        assert_eq!(std::fs::read(source).unwrap(), b"source bytes");
        assert!(browser.roots.is_empty());
    }

    #[test]
    fn scan_completion_and_failure_remain_visible_in_browser_state() {
        let mut browser = BrowserState {
            scan_in_progress: true,
            ..BrowserState::default()
        };
        let folder = crate::state::SampleBrowserFolder {
            path: PathBuf::from("/samples/drums"),
            root_path: PathBuf::from("/samples"),
            relative_path: PathBuf::from("drums"),
            name: "drums".into(),
            search_text: "drums".into(),
        };

        browser.update(BrowserMsg::SampleLibraryScanned(Ok(
            crate::message::SampleLibraryScanResult {
                entries: Vec::new(),
                folders: vec![folder.clone()],
                warnings: vec!["Unreadable folder".into()],
            },
        )));
        assert!(!browser.scan_in_progress);
        assert_eq!(browser.folders, vec![folder]);
        assert_eq!(browser.scan_warnings, vec!["Unreadable folder"]);
        assert!(browser.scan_error.is_none());

        browser.scan_in_progress = true;
        browser.update(BrowserMsg::SampleLibraryScanned(Err(
            "catalog failed".into()
        )));
        assert!(!browser.scan_in_progress);
        assert_eq!(browser.scan_error.as_deref(), Some("catalog failed"));
    }

    #[test]
    fn local_search_scope_widens_from_folder_to_root_to_everywhere() {
        let mut browser = BrowserState {
            roots: vec![PathBuf::from("/samples"), PathBuf::from("/other")],
            ..BrowserState::default()
        };
        browser.select_local_folder(Some(PathBuf::from("/samples/drums")));

        assert_eq!(browser.search_scope_label(), "THIS FOLDER");
        assert!(browser.path_is_in_search_scope(std::path::Path::new("/samples/drums/kick.wav")));
        assert!(!browser.path_is_in_search_scope(std::path::Path::new("/samples/bass/sub.wav")));

        browser.update(BrowserMsg::CycleSearchScope);
        assert_eq!(browser.search_scope_label(), "THIS ROOT");
        assert!(browser.path_is_in_search_scope(std::path::Path::new("/samples/bass/sub.wav")));
        assert!(!browser.path_is_in_search_scope(std::path::Path::new("/other/kick.wav")));

        browser.update(BrowserMsg::CycleSearchScope);
        assert_eq!(browser.search_scope_label(), "EVERYWHERE");
        assert!(browser.path_is_in_search_scope(std::path::Path::new("/other/kick.wav")));
    }

    #[test]
    fn local_filtering_uses_immediate_children_for_navigation_and_subtrees_for_search() {
        fn entry(root: &str, relative: &str) -> crate::state::SampleBrowserEntry {
            let root_path = PathBuf::from(root);
            let relative_path = PathBuf::from(relative);
            let path = root_path.join(&relative_path);
            crate::state::SampleBrowserEntry {
                source: MediaSourceRef::LocalFile { path },
                name: relative_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                root_path,
                relative_path: relative_path.clone(),
                format: "WAV".into(),
                file_size: Some(4),
                modified: None,
                search_text: relative_path.display().to_string().to_lowercase(),
            }
        }

        let mut browser = BrowserState {
            roots: vec![PathBuf::from("/samples"), PathBuf::from("/other")],
            ..BrowserState::default()
        };
        browser.select_local_folder(Some(PathBuf::from("/samples/drums")));
        let immediate = entry("/samples", "drums/kick.wav");
        let nested = entry("/samples", "drums/one-shots/kick-deep.wav");
        let sibling = entry("/samples", "bass/kick-bass.wav");
        let other_root = entry("/other", "kick-other.wav");

        assert!(browser.local_entry_is_result(&immediate, ""));
        assert!(!browser.local_entry_is_result(&nested, ""));
        assert!(browser.local_entry_is_result(&immediate, "kick"));
        assert!(browser.local_entry_is_result(&nested, "kick"));
        assert!(!browser.local_entry_is_result(&sibling, "kick"));

        browser.cycle_search_scope();
        assert!(browser.local_entry_is_result(&sibling, "kick"));
        assert!(!browser.local_entry_is_result(&other_root, "kick"));

        browser.cycle_search_scope();
        assert!(browser.local_entry_is_result(&other_root, "kick"));
    }

    #[test]
    fn selecting_a_folder_expands_it_and_resets_the_result_window() {
        let mut browser = BrowserState {
            results_visible_limit: 800,
            search_scope: crate::state::BrowserSearchScope::Everywhere,
            ..BrowserState::default()
        };
        let folder = PathBuf::from("/samples/drums");

        browser.update(BrowserMsg::SelectLocalFolder(Some(folder.clone())));

        assert_eq!(browser.current_folder, Some(folder.clone()));
        assert!(browser.expanded_local_folders.contains(&folder));
        assert_eq!(
            browser.search_scope,
            crate::state::BrowserSearchScope::SelectedFolder
        );
        assert_eq!(
            browser.results_visible_limit,
            crate::state::BROWSER_RESULTS_PAGE_SIZE
        );
    }

    #[test]
    fn large_results_are_rendered_in_bounded_windows_without_losing_total() {
        let mut browser = BrowserState::default();
        let total = 25_000;

        assert_eq!(
            browser.visible_result_count(total),
            crate::state::BROWSER_RESULTS_PAGE_SIZE
        );
        assert!(browser.has_more_results(total));

        browser.update(BrowserMsg::ShowMoreLocalResults);
        assert_eq!(
            browser.visible_result_count(total),
            crate::state::BROWSER_RESULTS_PAGE_SIZE * 2
        );
        assert!(browser.has_more_results(total));
    }
}
