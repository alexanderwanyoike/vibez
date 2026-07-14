//! Browser domain: local and remote sample browsing state, and
//! drag-and-drop from the browser into the arrangement.
//!
//! Only the synchronous state transitions live here. Anything that
//! spawns work (file dialogs, decode, provider HTTP) stays in app.rs
//! as an iced Task and routes results back through these messages.

use std::path::PathBuf;

use crate::message::{LocalRootWatchEvent, SampleLibraryScanResult};
use crate::remote_provider::RemoteCatalogEntry;
use crate::state::{BrowserState, LocalRootCatalogState, SampleBrowserMode};
use vibez_core::id::TrackId;
use vibez_core::track::MediaSourceRef;

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
    BeginPendingDrag {
        source: MediaSourceRef,
        label: String,
        origin_x: f32,
        origin_y: f32,
    },
    PendingDragMoved {
        x: f32,
        y: f32,
    },
    DragHoverTrack {
        track_id: TrackId,
        beat: f64,
        compatible: bool,
    },
    DragHoverEmptyArrangement {
        beat: f64,
    },
    DragHoverSampler {
        track_id: TrackId,
    },
    DragHoverDrumRackPad {
        track_id: TrackId,
        pad_index: usize,
    },
    ClearDragTarget,
    CancelDrag(String),
    EndDragSample,
    LocalRootWatchEvent(LocalRootWatchEvent),
    ReconcileLocalRoot {
        root: PathBuf,
        revision: u64,
    },
    LocalRootCatalogReconciled {
        root: PathBuf,
        revision: u64,
        result: Result<SampleLibraryScanResult, String>,
    },
    RemoveSampleLibraryRoot(PathBuf),
    ToggleRemotePlace,
    ToggleRemoteConnection,
    SelectRemoteFolder(String),
    ToggleRemoteFolder(String),
    SelectRemoteEntry(RemoteCatalogEntry),
    SetDropboxAppKey(String),
}

/// Cross-domain effects requested by a browser update.
#[derive(Debug, Default, PartialEq)]
pub struct BrowserAction {
    /// Status bar text.
    pub status: Option<String>,
    /// UI settings changed (browser visibility, library roots).
    pub persist_settings: bool,
    /// Decode a Local selection for the truthful Audition waveform without
    /// starting playback.
    pub load_waveform: Option<MediaSourceRef>,
    /// Debounce filesystem activity before asking the domain to reconcile.
    pub debounce_root_scans: Vec<(PathBuf, u64)>,
    /// The newest debounce token for this root matured and may be scanned.
    pub scan_root: Option<(PathBuf, u64)>,
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
                self.reset_results_window();
            }
            BrowserMsg::BeginPendingDrag {
                source,
                label,
                origin_x,
                origin_y,
            } => {
                self.begin_pending_drag(source, label, origin_x, origin_y);
            }
            BrowserMsg::PendingDragMoved { x, y } => {
                if self.move_pending_drag(x, y) {
                    let label = self.drag_label.as_deref().unwrap_or("media");
                    action.status = Some(format!(
                        "Moving {label} - choose an audio lane, empty Arrange, Sampler, or Drum Rack pad"
                    ));
                }
            }
            BrowserMsg::DragHoverTrack {
                track_id,
                beat,
                compatible,
            } => {
                if self.drag_source.is_none() {
                    return action;
                }
                self.drag_target = Some(crate::state::BrowserDropTarget::ArrangementLane {
                    track_id,
                    beat,
                    compatible,
                });
                action.status = Some(if compatible {
                    format!("Audio lane at beat {beat:.2}")
                } else {
                    "Invalid target: audio cannot be imported to a MIDI/instrument lane".into()
                });
            }
            BrowserMsg::DragHoverEmptyArrangement { beat } => {
                if self.drag_source.is_none() {
                    return action;
                }
                self.drag_target = Some(crate::state::BrowserDropTarget::EmptyArrangement { beat });
                action.status = Some(format!("New audio track at beat {beat:.2}"));
            }
            BrowserMsg::DragHoverSampler { track_id } => {
                if self.drag_source.is_none() {
                    return action;
                }
                self.drag_target = Some(crate::state::BrowserDropTarget::Sampler { track_id });
                action.status = Some("Load Sampler sample zone".into());
            }
            BrowserMsg::DragHoverDrumRackPad {
                track_id,
                pad_index,
            } => {
                if self.drag_source.is_none() {
                    return action;
                }
                self.drag_target = Some(crate::state::BrowserDropTarget::DrumRackPad {
                    track_id,
                    pad_index,
                });
                action.status = Some(format!("Assign Drum Rack pad {}", pad_index + 1));
            }
            BrowserMsg::ClearDragTarget => {
                self.drag_target = None;
            }
            BrowserMsg::CancelDrag(reason) => {
                self.cancel_media_drag();
                action.status = Some(reason);
            }
            BrowserMsg::EndDragSample => {
                if self.drag_source.is_some() {
                    self.cancel_media_drag();
                    action.status = Some("Drag cancelled".to_string());
                } else {
                    self.cancel_pending_drag();
                }
            }
            BrowserMsg::LocalRootWatchEvent(event) => match event {
                LocalRootWatchEvent::Changed(roots) => {
                    for root in roots {
                        if self.roots.iter().any(|configured| configured == &root) {
                            let revision = self.begin_root_scan(&root, true);
                            action.debounce_root_scans.push((root, revision));
                        }
                    }
                }
                LocalRootWatchEvent::Watching(roots) => {
                    for root in roots {
                        self.root_watch_errors.remove(&root);
                    }
                    self.refresh_scan_diagnostics();
                }
                LocalRootWatchEvent::Failed { roots, message } => {
                    for root in roots {
                        if self.roots.iter().any(|configured| configured == &root) {
                            self.root_watch_errors.insert(root, message.clone());
                        }
                    }
                    self.refresh_scan_diagnostics();
                    action.status = Some(format!("Local watch error: {message}"));
                }
            },
            BrowserMsg::ReconcileLocalRoot { root, revision } => {
                if self.root_refresh_is_current(&root, revision) {
                    action.scan_root = Some((root, revision));
                }
            }
            BrowserMsg::LocalRootCatalogReconciled {
                root,
                revision,
                result,
            } => {
                if !self.root_refresh_is_current(&root, revision) {
                    return action;
                }
                // Watcher-driven refreshes (Updating) happen behind the
                // user's back; they must not collapse an expanded
                // results window the way user-initiated scans do.
                let from_watcher = matches!(
                    self.root_catalog_states.get(&root),
                    Some(LocalRootCatalogState::Updating)
                );
                match result {
                    Ok(scan) => {
                        self.entries.retain(|entry| entry.root_path != root);
                        self.entries.extend(scan.entries);
                        self.entries.sort_by(|a, b| {
                            a.root_path
                                .cmp(&b.root_path)
                                .then_with(|| a.relative_path.cmp(&b.relative_path))
                        });
                        self.folders.retain(|folder| folder.root_path != root);
                        self.folders.extend(scan.folders);
                        self.folders.sort_by(|a, b| {
                            a.root_path
                                .cmp(&b.root_path)
                                .then_with(|| a.relative_path.cmp(&b.relative_path))
                        });
                        self.bump_catalog_revision();
                        let warning_count = scan.warnings.len();
                        self.root_catalog_states.insert(
                            root.clone(),
                            LocalRootCatalogState::Ready {
                                warnings: scan.warnings,
                            },
                        );
                        if !from_watcher {
                            self.reset_results_window();
                        }
                        let current_exists = self.current_folder.as_ref().is_none_or(|current| {
                            self.roots.iter().any(|root| root == current)
                                || self.folders.iter().any(|folder| &folder.path == current)
                        });
                        if !current_exists {
                            self.select_local_folder(Some(root.clone()));
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
                        self.refresh_scan_diagnostics();
                        let root_count = self
                            .entries
                            .iter()
                            .filter(|entry| entry.root_path == root)
                            .count();
                        action.status = Some(if warning_count == 0 {
                            format!("Indexed {root_count} samples in {}", root.display())
                        } else {
                            format!(
                                "Indexed {root_count} samples in {} with {warning_count} warning(s)",
                                root.display()
                            )
                        });
                    }
                    Err(error) => {
                        self.root_catalog_states.insert(
                            root.clone(),
                            LocalRootCatalogState::Stale {
                                error: error.clone(),
                            },
                        );
                        self.refresh_scan_diagnostics();
                        action.status = Some(format!(
                            "Local root unavailable; kept stale catalog for {} ({error})",
                            root.display()
                        ));
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
                self.bump_catalog_revision();
                self.expanded_local_folders
                    .retain(|folder| !folder.starts_with(&path));
                self.root_catalog_states.remove(&path);
                self.root_refresh_revisions.remove(&path);
                self.root_watch_errors.remove(&path);
                self.scan_warnings.clear();
                self.scan_error = None;
                self.refresh_scan_diagnostics();
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
            BrowserMsg::ToggleRemotePlace => {
                self.remote.place_expanded = !self.remote.place_expanded;
            }
            BrowserMsg::ToggleRemoteConnection => {
                self.remote.connection_expanded = !self.remote.connection_expanded;
            }
            BrowserMsg::SelectRemoteFolder(path) => {
                self.remote.current_path = path;
                if !self.remote.current_path.is_empty() {
                    self.remote
                        .expanded
                        .insert(self.remote.current_path.clone());
                }
                self.remote.selected_path = None;
                self.search_scope = crate::state::BrowserSearchScope::SelectedFolder;
                self.clear_selection();
                self.reset_results_window();
            }
            BrowserMsg::ToggleRemoteFolder(path) => {
                if !self.remote.expanded.remove(&path) {
                    self.remote.expanded.insert(path);
                }
            }
            BrowserMsg::SelectRemoteEntry(entry) => {
                self.remote.selected_path = Some(entry.provider_item_id.clone());
                self.select_source(MediaSourceRef::DropboxFile {
                    path_lower: entry.provider_item_id,
                    display_path: entry.path,
                    rev: entry.revision,
                });
            }
            BrowserMsg::SetDropboxAppKey(key) => {
                self.remote.app_key_input = key;
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
    fn splitter_drag_tracks_the_cursor_at_the_narrow_window_yield_point() {
        let mut browser = BrowserState::default();
        let window = 900.0;

        // Dragging past the yield point stores the width the layout
        // can actually render, so the handle stays under the cursor.
        browser.set_dock_width(browser.dock_drag_width(620.0, window));
        assert_eq!(browser.dock_width, browser.effective_dock_width(window));
        assert_eq!(browser.dock_width, 340.0);

        // A wide window leaves drags untouched within the static range.
        browser.set_dock_width(browser.dock_drag_width(620.0, 1_500.0));
        assert_eq!(browser.dock_width, 620.0);
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
        browser.begin_audition_load(&first);
        assert!(browser.install_audition(first.clone(), std::sync::Arc::clone(&audio)));
        assert!(browser.waveform_audio.is_some());
        assert!(browser.audition_playing);

        browser.select_source(second);
        assert!(browser.waveform_audio.is_none());
        assert!(!browser.install_audition(first, audio));
        assert!(browser.waveform_audio.is_none());
    }

    #[test]
    fn audition_defaults_on_and_remembers_clamped_gain_state() {
        let mut browser = BrowserState::default();
        assert!(browser.audition_enabled);
        assert_eq!(browser.audition_gain, 1.0);
        assert_eq!(browser.audition_mode, crate::state::AuditionMode::Raw);
        assert_eq!(
            browser.audition_sync,
            vibez_engine::commands::AuditionSync::Off
        );
        assert!(!browser.audition_loop);

        assert!(!browser.toggle_audition_enabled());
        browser.set_audition_gain(3.0);
        assert_eq!(browser.audition_gain, 2.0);
        browser.set_audition_gain(-1.0);
        assert_eq!(browser.audition_gain, 0.0);
    }

    #[test]
    fn warp_import_input_requires_positive_confirmed_bpm_for_the_current_source() {
        let first = MediaSourceRef::LocalFile {
            path: PathBuf::from("/samples/loop.wav"),
        };
        let second = MediaSourceRef::LocalFile {
            path: PathBuf::from("/samples/other.wav"),
        };
        let mut browser = BrowserState::default();
        browser.select_source(first.clone());
        browser.audition_mode = crate::state::AuditionMode::Warp;
        assert!(browser.audition_import_input().is_none());

        assert!(browser.install_bpm_suggestion(first, Some((127.8, 0.42)), 0.6));
        assert_eq!(browser.audition_bpm_edit, "127.8");
        assert!(browser.audition_bpm_confirmed.is_none());
        assert_eq!(browser.confirm_audition_bpm().unwrap(), 127.8);
        let input = browser.audition_import_input().unwrap();
        assert_eq!(input.mode, crate::state::AuditionMode::Warp);
        assert_eq!(input.source_bpm, Some(127.8));

        browser.select_source(second);
        assert!(browser.audition_bpm_confirmed.is_none());
        assert!(browser.audition_import_input().is_none());
        browser.audition_bpm_edit = "0".into();
        assert!(browser.confirm_audition_bpm().is_err());
    }

    #[test]
    fn trustworthy_detected_source_bpm_is_ready_without_manual_entry() {
        let source = MediaSourceRef::LocalFile {
            path: PathBuf::from("/samples/loop.wav"),
        };
        let mut browser = BrowserState::default();
        browser.select_source(source.clone());
        browser.audition_mode = crate::state::AuditionMode::Warp;

        assert!(browser.install_bpm_suggestion(source, Some((124.0, 0.91)), 0.6));
        assert_eq!(browser.audition_bpm_confirmed, Some(124.0));
        assert_eq!(
            browser.audition_import_input().unwrap().source_bpm,
            Some(124.0)
        );
    }

    #[test]
    fn click_motion_stays_pending_until_six_pixel_threshold() {
        let mut b = BrowserState::default();
        let source = MediaSourceRef::LocalFile {
            path: PathBuf::from("/tmp/kick.wav"),
        };
        b.update(BrowserMsg::BeginPendingDrag {
            source: source.clone(),
            label: "kick.wav".to_string(),
            origin_x: 10.0,
            origin_y: 20.0,
        });
        b.update(BrowserMsg::PendingDragMoved { x: 15.9, y: 20.0 });
        assert!(b.pending_drag.is_some());
        assert!(b.drag_source.is_none());

        b.update(BrowserMsg::PendingDragMoved { x: 16.0, y: 20.0 });
        assert!(b.pending_drag.is_some());
        let action = b.update(BrowserMsg::PendingDragMoved { x: 16.1, y: 20.0 });
        assert_eq!(b.drag_source, Some(source));
        assert!(b.pending_drag.is_none());
        assert!(action.status.unwrap().starts_with("Moving kick.wav"));
    }

    #[test]
    fn arrangement_hover_reports_compatible_and_invalid_targets() {
        let mut b = BrowserState::default();
        let tid = TrackId::new();
        b.drag_source = Some(MediaSourceRef::LocalFile {
            path: PathBuf::from("/tmp/kick.wav"),
        });
        b.update(BrowserMsg::DragHoverTrack {
            track_id: tid,
            beat: 8.0,
            compatible: true,
        });
        assert_eq!(
            b.drag_target,
            Some(crate::state::BrowserDropTarget::ArrangementLane {
                track_id: tid,
                beat: 8.0,
                compatible: true,
            })
        );
        let action = b.update(BrowserMsg::DragHoverTrack {
            track_id: tid,
            beat: 8.5,
            compatible: false,
        });
        assert!(action.status.unwrap().starts_with("Invalid target"));
    }

    #[test]
    fn drag_preview_reports_exact_raw_and_warp_musical_lengths() {
        let source = MediaSourceRef::LocalFile {
            path: PathBuf::from("/tmp/loop.wav"),
        };
        let mut browser = BrowserState {
            drag_source: Some(source.clone()),
            waveform_source: Some(source.clone()),
            waveform_audio: Some(std::sync::Arc::new(
                vibez_core::audio_buffer::DecodedAudio {
                    channels: vec![vec![0.25; 44_100]],
                    sample_rate: 44_100,
                },
            )),
            ..BrowserState::default()
        };
        assert_eq!(browser.drag_preview_beats(120.0), Some(2.0));

        browser.selected_source = Some(source);
        browser.audition_mode = crate::state::AuditionMode::Warp;
        browser.audition_bpm_confirmed = Some(120.0);
        assert_eq!(browser.drag_preview_beats(60.0), Some(2.0));
    }

    #[test]
    fn end_drag_without_target_cancels() {
        let mut b = BrowserState::default();
        b.update(BrowserMsg::BeginPendingDrag {
            source: MediaSourceRef::LocalFile {
                path: PathBuf::from("/tmp/kick.wav"),
            },
            label: "kick.wav".to_string(),
            origin_x: 0.0,
            origin_y: 0.0,
        });
        b.update(BrowserMsg::PendingDragMoved { x: 7.0, y: 0.0 });
        let action = b.update(BrowserMsg::EndDragSample);
        assert_eq!(action.status.as_deref(), Some("Drag cancelled"));
        assert!(b.drag_source.is_none());
        assert!(b.drag_target.is_none());
    }

    #[test]
    fn remote_mode_uses_the_persisted_catalog_without_requesting_network_work() {
        let mut b = BrowserState::default();
        let action = b.update(BrowserMsg::SetSampleBrowserMode(SampleBrowserMode::Remote));
        assert_eq!(b.mode, SampleBrowserMode::Remote);
        assert_eq!(action, BrowserAction::default());
    }

    #[test]
    fn remote_navigation_cycles_folder_connection_and_everywhere_scopes() {
        let mut browser = BrowserState::default();
        browser.update(BrowserMsg::SetSampleBrowserMode(SampleBrowserMode::Remote));
        browser.update(BrowserMsg::SelectRemoteFolder("/megalodon".into()));
        assert_eq!(browser.remote.current_path, "/megalodon");
        assert!(browser.remote.expanded.contains("/megalodon"));
        assert_eq!(
            browser.search_scope,
            crate::state::BrowserSearchScope::SelectedFolder
        );

        browser.update(BrowserMsg::CycleSearchScope);
        assert_eq!(browser.search_scope, crate::state::BrowserSearchScope::Root);
        browser.update(BrowserMsg::CycleSearchScope);
        assert_eq!(
            browser.search_scope,
            crate::state::BrowserSearchScope::Everywhere
        );
        browser.update(BrowserMsg::CycleSearchScope);
        assert_eq!(
            browser.search_scope,
            crate::state::BrowserSearchScope::SelectedFolder
        );
    }

    #[test]
    fn remote_place_and_connection_collapse_without_losing_navigation_state() {
        let mut browser = BrowserState::default();
        browser.remote.current_path = "/megalodon/drums".into();
        browser.remote.expanded.insert("/megalodon".into());

        browser.update(BrowserMsg::ToggleRemoteConnection);
        browser.update(BrowserMsg::ToggleRemotePlace);

        assert!(!browser.remote.connection_expanded);
        assert!(!browser.remote.place_expanded);
        assert_eq!(browser.remote.current_path, "/megalodon/drums");
        assert!(browser.remote.expanded.contains("/megalodon"));

        browser.update(BrowserMsg::ToggleRemotePlace);
        browser.update(BrowserMsg::ToggleRemoteConnection);
        assert!(browser.remote.place_expanded);
        assert!(browser.remote.connection_expanded);
    }

    #[test]
    fn remote_catalog_children_are_indexed_folder_first_per_parent() {
        let entry = |id: &str, parent: &str, name: &str, is_folder: bool| RemoteCatalogEntry {
            provider_item_id: id.into(),
            path: id.into(),
            parent_path: parent.into(),
            name: name.into(),
            is_folder,
            revision: None,
            size: None,
            derived_metadata: None,
        };
        let mut browser = BrowserState::default();
        browser.remote.catalog.entries = vec![
            entry("/z.wav", "", "z.wav", false),
            entry("/beats", "", "Beats", true),
            entry("/a.wav", "", "a.wav", false),
            entry("/beats/kick.wav", "/beats", "kick.wav", false),
        ];

        browser.remote.rebuild_catalog_children();

        let root_names: Vec<_> = browser
            .remote
            .catalog_child_indices("")
            .iter()
            .map(|&index| browser.remote.catalog.entries[index].name.as_str())
            .collect();
        assert_eq!(root_names, ["Beats", "a.wav", "z.wav"]);
        assert_eq!(browser.remote.catalog_child_indices("/beats").len(), 1);
        assert!(browser.remote.catalog_child_indices("/missing").is_empty());
    }

    #[test]
    fn remote_selection_preserves_provider_identity_and_source_location() {
        let mut browser = BrowserState::default();
        browser.update(BrowserMsg::SelectRemoteEntry(RemoteCatalogEntry {
            provider_item_id: "/megalodon/kick.wav".into(),
            path: "/Megalodon/Kick.wav".into(),
            parent_path: "/megalodon".into(),
            name: "Kick.wav".into(),
            is_folder: false,
            revision: Some("rev-7".into()),
            size: Some(2048),
            derived_metadata: None,
        }));
        assert_eq!(
            browser.selected_source,
            Some(MediaSourceRef::DropboxFile {
                path_lower: "/megalodon/kick.wav".into(),
                display_path: "/Megalodon/Kick.wav".into(),
                rev: Some("rev-7".into()),
            })
        );
        assert_eq!(
            browser.remote.selected_path.as_deref(),
            Some("/megalodon/kick.wav")
        );
    }

    #[test]
    fn remote_search_edits_are_catalog_only_and_request_no_provider_effect() {
        let mut browser = BrowserState {
            mode: SampleBrowserMode::Remote,
            ..BrowserState::default()
        };
        let action = browser.update(BrowserMsg::SampleBrowserSearchChanged("kick".into()));
        assert_eq!(browser.search, "kick");
        assert_eq!(action, BrowserAction::default());
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
        let root = PathBuf::from("/samples");
        let mut browser = BrowserState {
            roots: vec![root.clone()],
            ..BrowserState::default()
        };
        let revision = browser.begin_root_scan(&root, false);
        let folder = crate::state::SampleBrowserFolder {
            path: PathBuf::from("/samples/drums"),
            root_path: root.clone(),
            relative_path: PathBuf::from("drums"),
            name: "drums".into(),
            search_text: "drums".into(),
        };

        browser.update(BrowserMsg::LocalRootCatalogReconciled {
            root: root.clone(),
            revision,
            result: Ok(crate::message::SampleLibraryScanResult {
                entries: Vec::new(),
                folders: vec![folder.clone()],
                warnings: vec!["Unreadable folder".into()],
            }),
        });
        assert!(!browser.scan_in_progress);
        assert_eq!(browser.folders, vec![folder]);
        assert_eq!(browser.scan_warnings, vec!["Unreadable folder"]);
        assert!(browser.scan_error.is_none());

        let revision = browser.begin_root_scan(&root, false);
        browser.update(BrowserMsg::LocalRootCatalogReconciled {
            root,
            revision,
            result: Err("catalog failed".into()),
        });
        assert!(!browser.scan_in_progress);
        assert_eq!(browser.scan_error.as_deref(), Some("catalog failed"));
        assert_eq!(browser.folders.len(), 1, "stale catalog must be retained");
    }

    #[test]
    fn watcher_bursts_only_reconcile_the_latest_root_revision() {
        let root = PathBuf::from("/samples");
        let mut browser = BrowserState {
            roots: vec![root.clone()],
            ..BrowserState::default()
        };

        let first = browser.update(BrowserMsg::LocalRootWatchEvent(
            crate::message::LocalRootWatchEvent::Changed(vec![root.clone()]),
        ));
        let second = browser.update(BrowserMsg::LocalRootWatchEvent(
            crate::message::LocalRootWatchEvent::Changed(vec![root.clone()]),
        ));
        let first_revision = first.debounce_root_scans[0].1;
        let second_revision = second.debounce_root_scans[0].1;
        assert!(second_revision > first_revision);

        let stale = browser.update(BrowserMsg::ReconcileLocalRoot {
            root: root.clone(),
            revision: first_revision,
        });
        assert!(stale.scan_root.is_none());
        let current = browser.update(BrowserMsg::ReconcileLocalRoot {
            root: root.clone(),
            revision: second_revision,
        });
        assert_eq!(current.scan_root, Some((root, second_revision)));
    }

    #[test]
    fn watcher_reconciles_preserve_an_expanded_results_window() {
        let root = PathBuf::from("/samples");
        let mut browser = BrowserState {
            roots: vec![root.clone()],
            ..BrowserState::default()
        };
        browser.update(BrowserMsg::ShowMoreLocalResults);
        let expanded = browser.results_visible_limit;
        assert!(expanded > crate::state::BROWSER_RESULTS_PAGE_SIZE);
        let empty_scan = || crate::message::SampleLibraryScanResult {
            entries: Vec::new(),
            folders: Vec::new(),
            warnings: Vec::new(),
        };

        // Background filesystem activity must not collapse the list
        // the user expanded.
        let revision = browser.begin_root_scan(&root, true);
        browser.update(BrowserMsg::LocalRootCatalogReconciled {
            root: root.clone(),
            revision,
            result: Ok(empty_scan()),
        });
        assert_eq!(browser.results_visible_limit, expanded);

        // User-initiated scans still reset the window.
        let revision = browser.begin_root_scan(&root, false);
        browser.update(BrowserMsg::LocalRootCatalogReconciled {
            root,
            revision,
            result: Ok(empty_scan()),
        });
        assert_eq!(
            browser.results_visible_limit,
            crate::state::BROWSER_RESULTS_PAGE_SIZE
        );
    }

    #[test]
    fn scan_diagnostics_roll_up_deterministically_in_configured_root_order() {
        let first = PathBuf::from("/a-samples");
        let second = PathBuf::from("/b-samples");
        let mut browser = BrowserState {
            roots: vec![first.clone(), second.clone()],
            ..BrowserState::default()
        };
        browser.root_catalog_states.insert(
            second.clone(),
            crate::state::LocalRootCatalogState::Stale {
                error: "second offline".into(),
            },
        );
        browser.root_catalog_states.insert(
            first.clone(),
            crate::state::LocalRootCatalogState::Stale {
                error: "first offline".into(),
            },
        );

        // Regardless of map iteration order, the first configured
        // stale root is surfaced.
        browser.refresh_scan_diagnostics();
        assert_eq!(browser.scan_error.as_deref(), Some("first offline"));

        browser.root_catalog_states.insert(
            first,
            crate::state::LocalRootCatalogState::Ready {
                warnings: vec!["unreadable folder".into()],
            },
        );
        browser.refresh_scan_diagnostics();
        assert_eq!(browser.scan_error.as_deref(), Some("second offline"));
        assert_eq!(browser.scan_warnings, vec!["unreadable folder"]);
    }

    #[test]
    fn watcher_failures_are_scoped_to_the_affected_root() {
        let samples = PathBuf::from("/samples");
        let other = PathBuf::from("/other");
        let mut browser = BrowserState {
            roots: vec![samples.clone(), other.clone()],
            ..BrowserState::default()
        };

        browser.update(BrowserMsg::LocalRootWatchEvent(
            crate::message::LocalRootWatchEvent::Failed {
                roots: vec![samples.clone()],
                message: "watch limit reached".into(),
            },
        ));

        assert_eq!(browser.root_catalog_label(&samples), "WATCH ERR");
        assert_eq!(browser.root_catalog_label(&other), "PENDING");
        assert!(browser.root_catalog_message(&samples).is_some());
        assert!(browser.root_catalog_message(&other).is_none());
    }

    #[test]
    fn affected_root_reconciliation_preserves_other_catalogs_and_updates_search() {
        fn entry(root: &std::path::Path, name: &str) -> crate::state::SampleBrowserEntry {
            crate::state::SampleBrowserEntry {
                source: MediaSourceRef::LocalFile {
                    path: root.join(name),
                },
                name: name.into(),
                root_path: root.to_path_buf(),
                relative_path: PathBuf::from(name),
                format: "WAV".into(),
                duration_seconds: None,
                channels: None,
                sample_rate: None,
                file_size: Some(1),
                modified: None,
                search_text: name.to_lowercase(),
            }
        }

        let samples = PathBuf::from("/samples");
        let other = PathBuf::from("/other");
        let mut browser = BrowserState {
            roots: vec![samples.clone(), other.clone()],
            entries: vec![entry(&samples, "old.wav"), entry(&other, "keep.wav")],
            ..BrowserState::default()
        };
        let revision = browser.begin_root_scan(&samples, true);

        browser.update(BrowserMsg::LocalRootCatalogReconciled {
            root: samples.clone(),
            revision,
            result: Ok(crate::message::SampleLibraryScanResult {
                entries: vec![entry(&samples, "new.wav")],
                folders: Vec::new(),
                warnings: Vec::new(),
            }),
        });

        assert!(browser.entries.iter().any(|entry| entry.name == "new.wav"));
        assert!(browser.entries.iter().any(|entry| entry.name == "keep.wav"));
        assert!(!browser.entries.iter().any(|entry| entry.name == "old.wav"));
        browser.select_local_folder(Some(samples));
        assert!(browser.local_entry_is_result(
            browser
                .entries
                .iter()
                .find(|entry| entry.name == "new.wav")
                .unwrap(),
            "new"
        ));
    }

    #[test]
    fn manual_rescan_repairs_a_stale_root_without_discarding_old_data_first() {
        let root = PathBuf::from("/samples");
        let mut browser = BrowserState {
            roots: vec![root.clone()],
            entries: vec![crate::state::SampleBrowserEntry {
                source: MediaSourceRef::LocalFile {
                    path: root.join("old.wav"),
                },
                name: "old.wav".into(),
                root_path: root.clone(),
                relative_path: PathBuf::from("old.wav"),
                format: "WAV".into(),
                duration_seconds: None,
                channels: None,
                sample_rate: None,
                file_size: Some(1),
                modified: None,
                search_text: "old.wav".into(),
            }],
            ..BrowserState::default()
        };
        let revision = browser.begin_root_scan(&root, true);
        browser.update(BrowserMsg::LocalRootCatalogReconciled {
            root: root.clone(),
            revision,
            result: Err("offline".into()),
        });
        assert_eq!(browser.root_catalog_label(&root), "STALE");
        assert_eq!(browser.entries.len(), 1);

        let revision = browser.begin_root_scan(&root, false);
        browser.update(BrowserMsg::LocalRootCatalogReconciled {
            root: root.clone(),
            revision,
            result: Ok(crate::message::SampleLibraryScanResult {
                entries: Vec::new(),
                folders: Vec::new(),
                warnings: Vec::new(),
            }),
        });

        assert_eq!(browser.root_catalog_label(&root), "READY");
        assert!(browser.entries.is_empty());
        assert!(browser.scan_error.is_none());
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
                duration_seconds: None,
                channels: None,
                sample_rate: None,
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
