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
                self.mode = SampleBrowserMode::Local;
                self.select_local_folder(folder);
            }
            BrowserMsg::ToggleLocalFolder(folder) => {
                let should_expand = !self.expanded_local_folders.remove(&folder);
                if should_expand {
                    self.expanded_local_folders.insert(folder.clone());
                }
                self.mode = SampleBrowserMode::Local;
                self.select_local_folder(Some(folder.clone()));
                if !should_expand {
                    self.expanded_local_folders.remove(&folder);
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
#[path = "browser_tests.rs"]
mod tests;
