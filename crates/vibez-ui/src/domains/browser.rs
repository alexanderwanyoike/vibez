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
    SelectSampleBrowserRoot(Option<PathBuf>),
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
            }
            BrowserMsg::SelectSampleBrowserRoot(root) => {
                self.root_filter = root;
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
                        if self
                            .selected_source
                            .as_ref()
                            .and_then(|selected| {
                                self.entries.iter().find(|entry| &entry.source == selected)
                            })
                            .is_none()
                        {
                            if let Some(source) =
                                self.entries.first().map(|entry| entry.source.clone())
                            {
                                self.select_source(source.clone());
                                action.load_waveform = Some(source);
                            } else {
                                self.clear_selection();
                            }
                        }
                        action.status = Some(if scan.warnings.is_empty() {
                            format!("Indexed {} samples", self.entries.len())
                        } else {
                            format!(
                                "Indexed {} samples with {} warning(s)",
                                self.entries.len(),
                                scan.warnings.len()
                            )
                        });
                    }
                    Err(err) => {
                        action.status = Some(format!("Sample scan error: {err}"));
                    }
                }
            }
            BrowserMsg::RemoveSampleLibraryRoot(path) => {
                self.roots.retain(|root| root != &path);
                if self.root_filter.as_ref().is_some_and(|root| root == &path) {
                    self.root_filter = None;
                }
                self.entries.retain(|entry| entry.root_path != path);
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
            root_filter: Some(PathBuf::from("/samples")),
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
        assert_eq!(browser.root_filter, Some(PathBuf::from("/samples")));
        assert!(browser.selected_source.is_some());
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
        b.root_filter = Some(root.clone());
        let action = b.update(BrowserMsg::RemoveSampleLibraryRoot(root));
        assert!(b.roots.is_empty());
        assert_eq!(b.root_filter, None);
        assert!(action.persist_settings);
        assert_eq!(action.status.as_deref(), Some("Removed sample root"));
    }
}
