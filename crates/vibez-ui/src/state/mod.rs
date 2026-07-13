use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod ui_types;
pub use ui_types::*;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::DEFAULT_BPM;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::MediaSourceRef;
use vibez_dropbox::DropboxEntry;
use vibez_engine::commands::AuditionSync;
use vibez_plugin_host::PluginSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuditionMode {
    #[default]
    Raw,
    Warp,
}

impl AuditionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Raw => "RAW",
            Self::Warp => "WARP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuditionImportInput {
    pub mode: AuditionMode,
    pub source_bpm: Option<f64>,
}

pub const MEDIA_DRAG_THRESHOLD_PX: f32 = 6.0;

#[derive(Debug, Clone, PartialEq)]
pub struct PendingMediaDrag {
    pub source: MediaSourceRef,
    pub label: String,
    pub origin_x: f32,
    pub origin_y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BrowserDropTarget {
    ArrangementLane {
        track_id: TrackId,
        beat: f64,
        compatible: bool,
    },
    EmptyArrangement {
        beat: f64,
    },
    Sampler {
        track_id: TrackId,
    },
    DrumRackPad {
        track_id: TrackId,
        pad_index: usize,
    },
}

/// View domain slice: everything about how the project is being
/// looked at, none of it part of the project itself.
#[derive(Debug)]
pub struct ViewState {
    pub workspace: Workspace,
    pub detail_panel_tab: DetailPanelTab,
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,
    pub snap_grid: SnapGrid,
    pub snap_enabled: bool,
    pub adaptive_grid: bool,
    pub adaptive_grid_bias: i8,
    pub context_menu: Option<ContextMenu>,
    pub edit_menu_open: bool,
    /// Cursor tracking (for right-click positioning from mouse_area).
    pub cursor_x: f32,
    pub cursor_y: f32,
    /// Last known window size, for clamping popup menus on-screen.
    pub window_width: f32,
    pub window_height: f32,
    // Inline renaming
    pub editing_track_name: Option<TrackId>,
    pub editing_clip_name: Option<(TrackId, ClipId)>,
    pub edit_name_text: String,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            workspace: Workspace::Arrange,
            detail_panel_tab: DetailPanelTab::Clip,
            zoom_level: 1.0,
            scroll_offset_beats: 0.0,
            snap_grid: SnapGrid::EIGHTH,
            snap_enabled: true,
            adaptive_grid: false,
            adaptive_grid_bias: 0,
            context_menu: None,
            edit_menu_open: false,
            cursor_x: 0.0,
            cursor_y: 0.0,
            window_width: 1400.0,
            window_height: 900.0,
            editing_track_name: None,
            editing_clip_name: None,
            edit_name_text: String::new(),
        }
    }
}

impl ViewState {
    pub fn grid_config(&self) -> GridConfig {
        GridConfig::new(
            self.snap_grid,
            self.snap_enabled,
            self.adaptive_grid,
            self.adaptive_grid_bias,
        )
    }
}

/// Right-click context menu state.
#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub x: f32,
    pub y: f32,
    pub target: ContextMenuTarget,
}

/// What the context menu targets.
#[derive(Debug, Clone)]
pub enum ContextMenuTarget {
    Clip {
        track_id: TrackId,
        clip_id: ClipId,
        is_note_clip: bool,
    },
    TimeSelection {
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    },
    ArrangementEmpty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PianoRollEditMode {
    #[default]
    Select,
    Draw,
}

/// Piano roll domain slice: view scroll and the edit-mode toggle.
#[derive(Debug)]
pub struct PianoRollState {
    pub scroll_y: f32,
    pub edit_mode: PianoRollEditMode,
}

impl Default for PianoRollState {
    fn default() -> Self {
        Self {
            scroll_y: crate::widgets::piano_roll::default_scroll_y(200.0),
            edit_mode: PianoRollEditMode::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Audio,
    Plugins,
    Dropbox,
    Warping,
    Appearance,
}

/// A point-in-time snapshot of the editable project state, used to
/// implement undo / redo. `UiTrack` clones share audio via `Arc` so
/// each snapshot is cheap on memory despite the full tree.
#[derive(Debug, Clone)]
pub struct ProjectSnapshot {
    pub tracks: Vec<UiTrack>,
    pub master: UiTrack,
    pub buses: Vec<UiTrack>,
    pub bpm: f64,
    pub bpm_text: String,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
    pub selected_track: Option<TrackId>,
    pub selected_clips: HashSet<ArrangementSelection>,
    pub selected_note_clip: Option<(TrackId, ClipId)>,
    pub next_track_number: u32,
}

/// Project domain slice: file-menu visibility, the current file,
/// the dirty flag, and the undo/redo history.
#[derive(Debug, Default)]
pub struct ProjectState {
    pub file_menu_open: bool,
    pub current_path: Option<PathBuf>,
    pub dirty: bool,
    pub history: UndoHistory,
}

#[derive(Debug, Default)]
pub struct UndoHistory {
    pub undo: VecDeque<ProjectSnapshot>,
    pub redo: VecDeque<ProjectSnapshot>,
}

impl UndoHistory {
    pub const CAPACITY: usize = 100;

    pub fn push_undo(&mut self, snapshot: ProjectSnapshot) {
        self.undo.push_back(snapshot);
        if self.undo.len() > Self::CAPACITY {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    pub fn pop_undo(&mut self) -> Option<ProjectSnapshot> {
        self.undo.pop_back()
    }

    pub fn push_redo(&mut self, snapshot: ProjectSnapshot) {
        self.redo.push_back(snapshot);
        if self.redo.len() > Self::CAPACITY {
            self.redo.pop_front();
        }
    }

    pub fn pop_redo(&mut self) -> Option<ProjectSnapshot> {
        self.redo.pop_back()
    }

    #[allow(dead_code)]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SampleBrowserMode {
    #[default]
    Local,
    Dropbox,
}

pub const BROWSER_DOCK_MIN_WIDTH: f32 = 300.0;
pub const BROWSER_DOCK_DEFAULT_WIDTH: f32 = 410.0;
pub const BROWSER_DOCK_MAX_WIDTH: f32 = 650.0;
pub const ARRANGE_MIN_WIDTH_WITH_BROWSER: f32 = 560.0;
pub const BROWSER_PLACES_MIN_WIDTH: f32 = 124.0;
pub const BROWSER_PLACES_MAX_WIDTH: f32 = 176.0;
pub const BROWSER_RESULTS_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserSearchScope {
    #[default]
    SelectedFolder,
    Root,
    Everywhere,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalRootCatalogState {
    Indexing,
    Updating,
    Ready { warnings: Vec<String> },
    Stale { error: String },
}

impl LocalRootCatalogState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Indexing => "INDEXING",
            Self::Updating => "UPDATING",
            Self::Ready { warnings } if warnings.is_empty() => "READY",
            Self::Ready { .. } => "WARN",
            Self::Stale { .. } => "STALE",
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Indexing | Self::Updating)
    }
}

/// Browser domain slice: sample library, Dropbox browsing, and
/// drag-and-drop from the browser into the arrangement.
#[derive(Debug, Clone)]
pub struct BrowserState {
    pub open: bool,
    /// Remembered user width. The rendered width may temporarily yield to a
    /// narrow window without overwriting this preference.
    pub dock_width: f32,
    pub dock_resize_active: bool,
    pub search: String,
    pub roots: Vec<PathBuf>,
    pub entries: Vec<SampleBrowserEntry>,
    pub folders: Vec<SampleBrowserFolder>,
    /// Absolute Local Source Storage folder currently shown in Results. `None`
    /// is the All Roots location.
    pub current_folder: Option<PathBuf>,
    pub expanded_local_folders: HashSet<PathBuf>,
    pub search_scope: BrowserSearchScope,
    pub results_visible_limit: usize,
    pub root_catalog_states: HashMap<PathBuf, LocalRootCatalogState>,
    pub root_refresh_revisions: HashMap<PathBuf, u64>,
    pub root_watch_errors: HashMap<PathBuf, String>,
    pub scan_warnings: Vec<String>,
    pub scan_error: Option<String>,
    pub selected_source: Option<MediaSourceRef>,
    /// Decoded audio used only for the selected Browser source's visual
    /// waveform. Audition still travels through the existing engine path.
    pub waveform_source: Option<MediaSourceRef>,
    pub waveform_audio: Option<Arc<DecodedAudio>>,
    pub waveform_loading: bool,
    pub waveform_error: Option<String>,
    pub audition_enabled: bool,
    pub audition_gain: f32,
    pub audition_loading: bool,
    pub audition_playing: bool,
    pub audition_queued: bool,
    pub audition_mode: AuditionMode,
    pub audition_sync: AuditionSync,
    pub audition_loop: bool,
    pub audition_bpm_source: Option<MediaSourceRef>,
    pub audition_bpm_suggestion: Option<f64>,
    pub audition_bpm_confidence: Option<f32>,
    pub audition_bpm_confirmed: Option<f64>,
    pub audition_bpm_edit: String,
    pub audition_bpm_detecting: bool,
    pub scan_in_progress: bool,
    pub mode: SampleBrowserMode,
    pub dropbox: DropboxUiState,
    pub pending_drag: Option<PendingMediaDrag>,
    pub drag_source: Option<MediaSourceRef>,
    pub drag_label: Option<String>,
    pub drag_target: Option<BrowserDropTarget>,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            open: true,
            dock_width: BROWSER_DOCK_DEFAULT_WIDTH,
            dock_resize_active: false,
            search: String::new(),
            roots: Vec::new(),
            entries: Vec::new(),
            folders: Vec::new(),
            current_folder: None,
            expanded_local_folders: HashSet::new(),
            search_scope: BrowserSearchScope::default(),
            results_visible_limit: BROWSER_RESULTS_PAGE_SIZE,
            root_catalog_states: HashMap::new(),
            root_refresh_revisions: HashMap::new(),
            root_watch_errors: HashMap::new(),
            scan_warnings: Vec::new(),
            scan_error: None,
            selected_source: None,
            waveform_source: None,
            waveform_audio: None,
            waveform_loading: false,
            waveform_error: None,
            audition_enabled: true,
            audition_gain: 1.0,
            audition_loading: false,
            audition_playing: false,
            audition_queued: false,
            audition_mode: AuditionMode::default(),
            audition_sync: AuditionSync::Off,
            audition_loop: false,
            audition_bpm_source: None,
            audition_bpm_suggestion: None,
            audition_bpm_confidence: None,
            audition_bpm_confirmed: None,
            audition_bpm_edit: String::new(),
            audition_bpm_detecting: false,
            scan_in_progress: false,
            mode: SampleBrowserMode::default(),
            dropbox: DropboxUiState::default(),
            pending_drag: None,
            drag_source: None,
            drag_label: None,
            drag_target: None,
        }
    }
}

impl BrowserState {
    pub fn begin_root_scan(&mut self, root: &Path, from_watcher: bool) -> u64 {
        let revision = *self
            .root_refresh_revisions
            .entry(root.to_path_buf())
            .and_modify(|revision| *revision = revision.saturating_add(1))
            .or_insert(1);
        self.root_catalog_states.insert(
            root.to_path_buf(),
            if from_watcher {
                LocalRootCatalogState::Updating
            } else {
                LocalRootCatalogState::Indexing
            },
        );
        self.refresh_scan_diagnostics();
        revision
    }

    pub fn root_refresh_is_current(&self, root: &Path, revision: u64) -> bool {
        self.roots.iter().any(|configured| configured == root)
            && self.root_refresh_revisions.get(root).copied() == Some(revision)
    }

    pub fn root_catalog_label(&self, root: &Path) -> &'static str {
        if self.root_watch_errors.contains_key(root) {
            "WATCH ERR"
        } else {
            self.root_catalog_states
                .get(root)
                .map(LocalRootCatalogState::label)
                .unwrap_or("PENDING")
        }
    }

    pub fn root_catalog_message(&self, root: &Path) -> Option<String> {
        if let Some(error) = self.root_watch_errors.get(root) {
            return Some(format!("WATCH ERROR · {error}"));
        }
        match self.root_catalog_states.get(root) {
            Some(LocalRootCatalogState::Indexing) => Some("INDEXING LOCAL ROOT…".into()),
            Some(LocalRootCatalogState::Updating) => Some("UPDATING LOCAL ROOT…".into()),
            Some(LocalRootCatalogState::Ready { warnings }) if !warnings.is_empty() => {
                Some(format!("WARN {} · {}", warnings.len(), warnings[0]))
            }
            Some(LocalRootCatalogState::Stale { error }) => {
                Some(format!("STALE · {error} · RESCAN TO REPAIR"))
            }
            _ => None,
        }
    }

    pub fn refresh_scan_diagnostics(&mut self) {
        self.scan_in_progress = self
            .root_catalog_states
            .values()
            .any(LocalRootCatalogState::is_busy);
        self.scan_warnings = self
            .root_catalog_states
            .values()
            .filter_map(|state| match state {
                LocalRootCatalogState::Ready { warnings } => Some(warnings.as_slice()),
                _ => None,
            })
            .flatten()
            .cloned()
            .collect();
        self.scan_error = self
            .root_catalog_states
            .values()
            .find_map(|state| match state {
                LocalRootCatalogState::Stale { error } => Some(error.clone()),
                _ => None,
            });
    }

    pub fn reset_results_window(&mut self) {
        self.results_visible_limit = BROWSER_RESULTS_PAGE_SIZE;
    }

    pub fn select_local_folder(&mut self, folder: Option<PathBuf>) {
        self.current_folder = folder;
        if let Some(folder) = &self.current_folder {
            self.expanded_local_folders.insert(folder.clone());
        }
        self.search_scope = BrowserSearchScope::SelectedFolder;
        self.reset_results_window();
    }

    pub fn current_local_root(&self) -> Option<&PathBuf> {
        let current = self.current_folder.as_ref()?;
        self.roots
            .iter()
            .filter(|root| current.starts_with(root))
            .max_by_key(|root| root.components().count())
    }

    pub fn search_scope_path(&self) -> Option<&std::path::Path> {
        match self.search_scope {
            BrowserSearchScope::SelectedFolder => self.current_folder.as_deref(),
            BrowserSearchScope::Root => self.current_local_root().map(PathBuf::as_path),
            BrowserSearchScope::Everywhere => None,
        }
    }

    pub fn search_scope_label(&self) -> &'static str {
        match self.search_scope {
            BrowserSearchScope::SelectedFolder if self.current_folder.is_none() => "EVERYWHERE",
            BrowserSearchScope::SelectedFolder
                if self.current_folder.as_ref() == self.current_local_root() =>
            {
                "THIS ROOT"
            }
            BrowserSearchScope::SelectedFolder => "THIS FOLDER",
            BrowserSearchScope::Root => "THIS ROOT",
            BrowserSearchScope::Everywhere => "EVERYWHERE",
        }
    }

    pub fn cycle_search_scope(&mut self) {
        self.search_scope = match self.search_scope {
            BrowserSearchScope::SelectedFolder
                if self.current_folder.is_some()
                    && self.current_folder.as_ref() != self.current_local_root() =>
            {
                BrowserSearchScope::Root
            }
            BrowserSearchScope::SelectedFolder | BrowserSearchScope::Root => {
                BrowserSearchScope::Everywhere
            }
            BrowserSearchScope::Everywhere if self.current_folder.is_some() => {
                BrowserSearchScope::SelectedFolder
            }
            BrowserSearchScope::Everywhere => BrowserSearchScope::Everywhere,
        };
        self.reset_results_window();
    }

    pub fn path_is_in_search_scope(&self, path: &std::path::Path) -> bool {
        self.search_scope_path()
            .is_none_or(|scope| path.starts_with(scope))
    }

    pub fn local_folder_is_result(
        &self,
        folder: &SampleBrowserFolder,
        normalized_query: &str,
    ) -> bool {
        if normalized_query.is_empty() {
            return self
                .current_folder
                .as_deref()
                .is_some_and(|current| folder.path.parent() == Some(current));
        }
        self.path_is_in_search_scope(&folder.path) && folder.search_text.contains(normalized_query)
    }

    pub fn local_entry_is_result(
        &self,
        entry: &SampleBrowserEntry,
        normalized_query: &str,
    ) -> bool {
        let path = entry.root_path.join(&entry.relative_path);
        if normalized_query.is_empty() {
            return self
                .current_folder
                .as_deref()
                .is_some_and(|current| path.parent() == Some(current));
        }
        self.path_is_in_search_scope(&path) && entry.search_text.contains(normalized_query)
    }

    pub fn visible_result_count(&self, total: usize) -> usize {
        total.min(self.results_visible_limit)
    }

    pub fn has_more_results(&self, total: usize) -> bool {
        self.results_visible_limit < total
    }

    pub fn select_source(&mut self, source: MediaSourceRef) -> bool {
        let changed = self.selected_source.as_ref() != Some(&source);
        self.selected_source = Some(source);
        if changed {
            self.clear_waveform();
            self.clear_audition_bpm();
        }
        changed
    }

    pub fn clear_selection(&mut self) {
        self.selected_source = None;
        self.clear_waveform();
        self.clear_audition_bpm();
    }

    pub fn begin_waveform_load(&mut self, source: &MediaSourceRef) {
        if self.selected_source.as_ref() == Some(source) {
            self.waveform_loading = true;
        }
    }

    pub fn begin_audition_load(&mut self, source: &MediaSourceRef) {
        self.begin_waveform_load(source);
        if self.selected_source.as_ref() == Some(source) {
            self.audition_loading = true;
        }
    }

    pub fn install_audition(&mut self, source: MediaSourceRef, audio: Arc<DecodedAudio>) -> bool {
        if !self.install_waveform(source, audio) {
            return false;
        }
        self.audition_loading = false;
        self.audition_playing = true;
        true
    }

    pub fn stop_audition_state(&mut self) {
        self.audition_loading = false;
        self.audition_playing = false;
        self.audition_queued = false;
    }

    pub fn toggle_audition_enabled(&mut self) -> bool {
        self.audition_enabled = !self.audition_enabled;
        self.audition_enabled
    }

    pub fn set_audition_gain(&mut self, gain: f32) {
        self.audition_gain = gain.clamp(0.0, 2.0);
    }

    pub fn mark_audition_requested(&mut self, queued: bool) {
        self.audition_loading = false;
        self.audition_queued = queued;
        self.audition_playing = !queued;
    }

    pub fn begin_bpm_detection(&mut self, source: &MediaSourceRef) -> bool {
        if self.selected_source.as_ref() != Some(source)
            || self.audition_bpm_source.as_ref() == Some(source)
            || self.audition_bpm_detecting
        {
            return false;
        }
        self.audition_bpm_detecting = true;
        true
    }

    pub fn install_bpm_suggestion(
        &mut self,
        source: MediaSourceRef,
        estimate: Option<(f64, f32)>,
    ) -> bool {
        if self.selected_source.as_ref() != Some(&source) {
            return false;
        }
        self.audition_bpm_source = Some(source);
        self.audition_bpm_detecting = false;
        self.audition_bpm_suggestion = estimate.map(|value| value.0);
        self.audition_bpm_confidence = estimate.map(|value| value.1);
        if self.audition_bpm_edit.is_empty() {
            self.audition_bpm_edit = estimate
                .map(|value| format!("{:.1}", value.0))
                .unwrap_or_default();
        }
        true
    }

    pub fn confirm_audition_bpm(&mut self) -> Result<f64, &'static str> {
        let bpm = self
            .audition_bpm_edit
            .trim()
            .parse::<f64>()
            .map_err(|_| "Enter a positive source BPM")?;
        if !bpm.is_finite() || bpm <= 0.0 {
            return Err("Enter a positive source BPM");
        }
        self.audition_bpm_confirmed = Some(bpm);
        Ok(bpm)
    }

    pub fn clear_audition_bpm(&mut self) {
        self.audition_bpm_source = None;
        self.audition_bpm_suggestion = None;
        self.audition_bpm_confidence = None;
        self.audition_bpm_confirmed = None;
        self.audition_bpm_edit.clear();
        self.audition_bpm_detecting = false;
    }

    pub fn audition_import_input(&self) -> Option<AuditionImportInput> {
        match self.audition_mode {
            AuditionMode::Raw => Some(AuditionImportInput {
                mode: AuditionMode::Raw,
                source_bpm: None,
            }),
            AuditionMode::Warp => {
                self.audition_bpm_confirmed
                    .map(|source_bpm| AuditionImportInput {
                        mode: AuditionMode::Warp,
                        source_bpm: Some(source_bpm),
                    })
            }
        }
    }

    pub fn begin_pending_drag(
        &mut self,
        source: MediaSourceRef,
        label: String,
        origin_x: f32,
        origin_y: f32,
    ) {
        self.cancel_media_drag();
        self.pending_drag = Some(PendingMediaDrag {
            source,
            label,
            origin_x,
            origin_y,
        });
    }

    pub fn move_pending_drag(&mut self, x: f32, y: f32) -> bool {
        let Some(pending) = self.pending_drag.as_ref() else {
            return false;
        };
        let dx = x - pending.origin_x;
        let dy = y - pending.origin_y;
        if dx * dx + dy * dy <= MEDIA_DRAG_THRESHOLD_PX * MEDIA_DRAG_THRESHOLD_PX {
            return false;
        }
        let pending = self.pending_drag.take().expect("pending drag exists");
        self.drag_source = Some(pending.source);
        self.drag_label = Some(pending.label);
        self.drag_target = None;
        true
    }

    pub fn cancel_pending_drag(&mut self) {
        self.pending_drag = None;
    }

    pub fn cancel_media_drag(&mut self) {
        self.pending_drag = None;
        self.drag_source = None;
        self.drag_label = None;
        self.drag_target = None;
    }

    pub fn drag_preview_beats(&self, project_bpm: f64) -> Option<f64> {
        let source = self.drag_source.as_ref()?;
        if self.waveform_source.as_ref()? != source {
            return None;
        }
        let audio = self.waveform_audio.as_ref()?;
        if audio.sample_rate == 0 || audio.num_frames() == 0 {
            return None;
        }
        let seconds = audio.num_frames() as f64 / audio.sample_rate as f64;
        match self.audition_import_input()? {
            AuditionImportInput {
                mode: AuditionMode::Raw,
                ..
            } => (project_bpm > 0.0).then_some(seconds * project_bpm / 60.0),
            AuditionImportInput {
                mode: AuditionMode::Warp,
                source_bpm: Some(source_bpm),
            } if project_bpm > 0.0 => {
                let target_frames = crate::warp::warp_target_frames(
                    audio.num_frames(),
                    audio.sample_rate as f64,
                    source_bpm,
                    project_bpm,
                );
                Some(target_frames as f64 * project_bpm / (audio.sample_rate as f64 * 60.0))
            }
            _ => None,
        }
    }

    pub fn install_waveform(&mut self, source: MediaSourceRef, audio: Arc<DecodedAudio>) -> bool {
        if self.selected_source.as_ref() != Some(&source) {
            return false;
        }
        self.waveform_source = Some(source);
        self.waveform_audio = Some(audio);
        self.waveform_loading = false;
        self.waveform_error = None;
        true
    }

    pub fn fail_waveform_load(&mut self, source: &MediaSourceRef, error: String) {
        if self.selected_source.as_ref() == Some(source) {
            self.waveform_loading = false;
            self.waveform_error = Some(error);
        }
    }

    fn clear_waveform(&mut self) {
        self.waveform_source = None;
        self.waveform_audio = None;
        self.waveform_loading = false;
        self.waveform_error = None;
    }

    pub fn set_dock_width(&mut self, width: f32) {
        self.dock_width = width.clamp(BROWSER_DOCK_MIN_WIDTH, BROWSER_DOCK_MAX_WIDTH);
    }

    pub fn effective_dock_width(&self, window_width: f32) -> f32 {
        let available = (window_width - ARRANGE_MIN_WIDTH_WITH_BROWSER)
            .clamp(BROWSER_DOCK_MIN_WIDTH, BROWSER_DOCK_MAX_WIDTH);
        self.dock_width.min(available).max(BROWSER_DOCK_MIN_WIDTH)
    }

    pub fn places_pane_width(&self, window_width: f32) -> f32 {
        (self.effective_dock_width(window_width) * 0.36)
            .clamp(BROWSER_PLACES_MIN_WIDTH, BROWSER_PLACES_MAX_WIDTH)
    }

    /// The single Results table keeps Name and Status visible throughout the
    /// resize range, then promotes BPM and Length into dedicated columns once
    /// the Results pane has enough room to keep every column readable.
    pub fn results_use_wide_columns(&self, window_width: f32) -> bool {
        let results_width =
            self.effective_dock_width(window_width) - self.places_pane_width(window_width);
        results_width >= 400.0
    }
}

/// UI-side state for the Dropbox browser and Settings tab.
#[derive(Debug, Default, Clone)]
pub struct DropboxUiState {
    pub connected: bool,
    pub account_email: Option<String>,
    /// App key entered in settings (may be empty until the user pastes one).
    pub app_key_input: String,
    /// Whether any source of app key is present (settings, env, build-time).
    pub has_app_key: bool,
    /// An OAuth flow is in progress; Connect button is disabled.
    pub auth_in_progress: bool,
    pub last_error: Option<String>,
    /// Listing cache keyed by Dropbox folder path (`""` for root).
    pub folders: HashMap<String, Vec<DropboxEntry>>,
    /// Paths for which a list_folder call is currently in flight.
    pub listing_in_progress: HashSet<String>,
    /// Paths expanded in the tree UI.
    pub expanded: HashSet<String>,
    /// `path_lower` of the currently-selected Dropbox entry, if any.
    pub selected_path: Option<String>,
    /// A preview fetch / playback is in flight.
    pub preview_in_progress: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceMenuCategory {
    Instruments,
    Effects,
    Plugins,
}

#[derive(Debug, Clone)]
pub struct DeviceContextMenu {
    pub x: f32,
    pub y: f32,
    pub track_id: TrackId,
    pub category: Option<DeviceMenuCategory>,
    pub search: String,
}

/// Transport domain state: playback position, tempo, and the
/// arrangement loop. First extracted domain slice of the
/// architecture refactor: owns everything the transport bar and the
/// beat/sample math need, and nothing else.
#[derive(Debug)]
pub struct TransportState {
    pub playing: bool,
    pub position_samples: u64,
    pub sample_rate: u32,
    pub bpm: f64,
    pub bpm_text: String,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
}

impl Default for TransportState {
    fn default() -> Self {
        Self {
            playing: false,
            position_samples: 0,
            sample_rate: 44_100,
            bpm: 120.0,
            bpm_text: "120".to_string(),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 4.0,
        }
    }
}

/// The master bus as a track-shaped UI channel: gain plus an effect
/// chain, no clips or instrument. Lives outside `tracks` so the
/// arrangement never shows it, but `find_track` resolves it, so the
/// mixer strip, device chain, and effect commands all work on it.
pub fn new_master_track() -> UiTrack {
    let mut master = UiTrack::new(TrackId::MASTER, "Master".to_string(), 0);
    master.pan = 0.5;
    master
}

/// Arrangement domain state: the track list (the shared model other
/// domains receive explicitly), selection, and track numbering.
#[derive(Debug)]
pub struct ArrangementState {
    pub tracks: Vec<UiTrack>,
    /// The master bus channel (see [`new_master_track`]).
    pub master: UiTrack,
    /// Return channels: mixer-only tracks fed by per-track sends.
    pub buses: Vec<UiTrack>,
    pub selected_track: Option<TrackId>,
    pub next_track_number: u32,
    pub selected_clips: HashSet<ArrangementSelection>,
    pub selected_note_clip: Option<(TrackId, ClipId)>,
    pub clipboard: ClipClipboard,
    // Time selection (visible brackets; independent from the loop).
    pub time_selection_active: bool,
    pub selection_start_beats: f64,
    pub selection_end_beats: f64,
    pub time_selection_track: Option<TrackId>,
    /// An arrangement drag (move/resize) is active; drives edge
    /// auto-scroll on ticks.
    pub drag_resize_active: bool,
    /// In-flight text edits for the clip BPM field in the clip detail
    /// panel; a missing entry means show the committed
    /// `UiClip::original_bpm` value instead.
    pub clip_bpm_edit: HashMap<ClipId, String>,
}

impl Default for ArrangementState {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            master: new_master_track(),
            buses: Vec::new(),
            selected_track: None,
            next_track_number: 0,
            selected_clips: HashSet::new(),
            selected_note_clip: None,
            clipboard: ClipClipboard::default(),
            time_selection_active: false,
            selection_start_beats: 0.0,
            selection_end_beats: 0.0,
            time_selection_track: None,
            drag_resize_active: false,
            clip_bpm_edit: HashMap::new(),
        }
    }
}

pub struct AppState {
    // Transport domain slice (playback, tempo, arrangement loop).
    pub transport: TransportState,

    // Metering (master)
    pub peak_l: f32,
    pub peak_r: f32,
    /// Spectrum analyser fed by the engine's per-track tap (follows
    /// the selected track); drawn behind the channel EQ curve.
    pub spectrum: crate::spectrum::SpectrumState,

    // UI
    pub status_text: String,
    // View domain slice (workspace, zoom, snap, menus, renames).
    pub view: ViewState,

    pub piano_roll: PianoRollState,

    // Arrangement domain slice (tracks, selection, numbering).
    pub arrangement: ArrangementState,

    /// In-progress manual BPM input text keyed by clip id. Only
    /// populated while the user is actively editing the field in the
    // Device context menu
    pub devices: crate::domains::devices::DevicesState,

    // File menu / Settings
    pub settings_open: bool,
    pub settings_tab: SettingsTab,
    pub settings_buffer_size: u32,
    // Project domain slice (file menu, path, dirty flag, undo).
    pub project: ProjectState,
    /// Automatically detect sample BPM and warp to project tempo on
    /// import. Mirrored from `UiSettings::auto_warp_on_import`.
    pub auto_warp_on_import: bool,
    /// Minimum BPM-detect confidence required to auto-warp. Mirrored
    /// from `UiSettings::warp_confidence_threshold`.
    pub warp_confidence_threshold: f32,
    // Automation domain slice (lane expansion, point selection).
    pub automation_ui: crate::domains::automation::AutomationState,

    // Browser domain slice (sample library, Dropbox, drag-drop).
    pub browser: BrowserState,

    // Appearance / themes
    pub current_theme_name: String,
    pub user_themes: Vec<crate::themes::UserTheme>,
    pub theme_save_name: String,

    // Plugin hosting
    pub plugin_settings: PluginSettings,
    pub plugin_scan_in_progress: bool,
    pub plugin_scan_status: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            transport: TransportState {
                bpm: DEFAULT_BPM,
                bpm_text: format!("{DEFAULT_BPM:.0}"),
                ..TransportState::default()
            },
            peak_l: 0.0,
            peak_r: 0.0,
            spectrum: crate::spectrum::SpectrumState::default(),
            status_text: "Ready — Add a track to get started".to_string(),
            view: ViewState::default(),
            piano_roll: PianoRollState::default(),
            arrangement: ArrangementState {
                next_track_number: 1,
                ..ArrangementState::default()
            },
            devices: crate::domains::devices::DevicesState::default(),
            settings_open: false,
            settings_tab: SettingsTab::default(),
            settings_buffer_size: 512,
            project: ProjectState::default(),
            auto_warp_on_import: false,
            warp_confidence_threshold: 0.6,
            automation_ui: crate::domains::automation::AutomationState::default(),
            browser: BrowserState::default(),
            current_theme_name: "Charcoal".to_string(),
            user_themes: Vec::new(),
            theme_save_name: String::new(),
            plugin_settings: PluginSettings::load(),
            plugin_scan_in_progress: false,
            plugin_scan_status: String::new(),
        }
    }
}

pub fn default_drum_rack_pads() -> Vec<UiDrumPad> {
    (0..16).map(|_| UiDrumPad::default()).collect()
}

impl AppState {
    pub fn position_seconds(&self) -> f64 {
        self.transport.position_samples as f64 / self.transport.sample_rate as f64
    }

    pub fn position_beats(&self) -> f64 {
        self.position_seconds() * self.transport.bpm / 60.0
    }

    pub fn duration_seconds(&self) -> f64 {
        let samples = self.total_duration_samples();
        if samples > 0 {
            samples as f64 / self.transport.sample_rate as f64
        } else {
            0.0
        }
    }

    #[allow(dead_code)]
    pub fn position_normalized(&self) -> f64 {
        let dur = self.duration_seconds();
        if dur <= 0.0 {
            0.0
        } else {
            (self.position_seconds() / dur).clamp(0.0, 1.0)
        }
    }

    pub fn format_time(seconds: f64) -> String {
        let mins = (seconds / 60.0) as u32;
        let secs = seconds % 60.0;
        format!("{mins:02}:{secs:05.2}")
    }

    /// Pixels per beat at the current zoom level.
    #[allow(dead_code)]
    pub fn pixels_per_beat(&self) -> f32 {
        20.0 * self.view.zoom_level
    }

    /// Number of beats visible in a canvas of the given width.
    #[allow(dead_code)]
    pub fn visible_beats(&self, canvas_width: f32) -> f64 {
        canvas_width as f64 / self.pixels_per_beat() as f64
    }

    /// Convert a beat value to a pixel x coordinate in the viewport.
    #[allow(dead_code)]
    pub fn beat_to_x(&self, beat: f64) -> f32 {
        ((beat - self.view.scroll_offset_beats) * self.pixels_per_beat() as f64) as f32
    }

    /// Convert a pixel x coordinate in the viewport to a beat value.
    #[allow(dead_code)]
    pub fn x_to_beat(&self, x: f32) -> f64 {
        x as f64 / self.pixels_per_beat() as f64 + self.view.scroll_offset_beats
    }

    /// Total duration in beats across all tracks, with generous padding.
    pub fn total_beats(&self) -> f64 {
        let dur = self.duration_seconds();
        if dur > 0.0 && self.transport.bpm > 0.0 {
            let content_beats = dur * self.transport.bpm / 60.0;
            // Pad by 32 beats or 25% of content, whichever is larger
            let padding = (content_beats * 0.25).max(32.0);
            (content_beats + padding).max(64.0)
        } else {
            // Minimum 64 beats for empty projects
            64.0
        }
    }

    /// Convert a beat value to a sample position.
    pub fn beats_to_samples(&self, beats: f64) -> u64 {
        if self.transport.bpm > 0.0 {
            (beats * self.transport.sample_rate as f64 * 60.0 / self.transport.bpm) as u64
        } else {
            0
        }
    }

    /// Check if a clip is in the multi-selection set.
    #[allow(dead_code)]
    pub fn is_clip_selected(&self, clip_id: ClipId) -> bool {
        self.arrangement.selected_clips.iter().any(|sel| match sel {
            ArrangementSelection::AudioClip { clip_id: cid, .. } => *cid == clip_id,
            ArrangementSelection::NoteClip { clip_id: cid, .. } => *cid == clip_id,
        })
    }

    /// Returns the single selected clip if exactly one is selected.
    #[allow(dead_code)]
    pub fn single_selected_clip(&self) -> Option<ArrangementSelection> {
        if self.arrangement.selected_clips.len() == 1 {
            self.arrangement.selected_clips.iter().next().copied()
        } else {
            None
        }
    }

    pub fn find_track(&self, id: TrackId) -> Option<&UiTrack> {
        if id.is_master() {
            return Some(&self.arrangement.master);
        }
        self.arrangement
            .tracks
            .iter()
            .chain(self.arrangement.buses.iter())
            .find(|t| t.id == id)
    }

    pub fn find_track_mut(&mut self, id: TrackId) -> Option<&mut UiTrack> {
        if id.is_master() {
            return Some(&mut self.arrangement.master);
        }
        self.arrangement
            .tracks
            .iter_mut()
            .chain(self.arrangement.buses.iter_mut())
            .find(|t| t.id == id)
    }

    /// Total duration in samples across all tracks (max clip end position).
    pub fn total_duration_samples(&self) -> u64 {
        let audio_max = self
            .arrangement
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.position.saturating_add(c.duration))
            .max()
            .unwrap_or(0);

        // Include note clips: convert beat positions to samples
        let spb = if self.transport.bpm > 0.0 {
            self.transport.sample_rate as f64 * 60.0 / self.transport.bpm
        } else {
            0.0
        };
        let note_max = if spb > 0.0 {
            self.arrangement
                .tracks
                .iter()
                .flat_map(|t| t.note_clips.iter())
                .map(|c| ((c.position_beats + c.duration_beats) * spb) as u64)
                .max()
                .unwrap_or(0)
        } else {
            0
        };

        audio_max.max(note_max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::id::TrackId;
    use vibez_core::midi::TrackKind;

    fn make_state_with(tracks: Vec<UiTrack>) -> AppState {
        let mut state = AppState::default();
        state.arrangement.tracks = tracks;
        state
    }

    fn make_two_tracks() -> Vec<UiTrack> {
        vec![
            UiTrack::new(TrackId::new(), "Track 1".into(), 0),
            UiTrack::new(TrackId::new(), "Track 2".into(), 1),
        ]
    }

    #[test]
    fn move_track_up() {
        let mut state = make_state_with(make_two_tracks());
        let id0 = state.arrangement.tracks[0].id;
        let id1 = state.arrangement.tracks[1].id;

        if let Some(idx) = state.arrangement.tracks.iter().position(|t| t.id == id1) {
            if idx > 0 {
                state.arrangement.tracks.swap(idx, idx - 1);
            }
        }
        assert_eq!(state.arrangement.tracks[0].id, id1);
        assert_eq!(state.arrangement.tracks[1].id, id0);
    }

    #[test]
    fn move_track_down() {
        let mut state = make_state_with(make_two_tracks());
        let id0 = state.arrangement.tracks[0].id;
        let id1 = state.arrangement.tracks[1].id;

        if let Some(idx) = state.arrangement.tracks.iter().position(|t| t.id == id0) {
            if idx + 1 < state.arrangement.tracks.len() {
                state.arrangement.tracks.swap(idx, idx + 1);
            }
        }
        assert_eq!(state.arrangement.tracks[0].id, id1);
        assert_eq!(state.arrangement.tracks[1].id, id0);
    }

    #[test]
    fn move_first_track_up_noop() {
        let mut state = make_state_with(vec![UiTrack::new(TrackId::new(), "Track 1".into(), 0)]);
        let id0 = state.arrangement.tracks[0].id;

        if let Some(idx) = state.arrangement.tracks.iter().position(|t| t.id == id0) {
            if idx > 0 {
                state.arrangement.tracks.swap(idx, idx - 1);
            }
        }
        assert_eq!(state.arrangement.tracks[0].id, id0);
    }

    #[test]
    fn move_last_track_down_noop() {
        let mut state = make_state_with(vec![UiTrack::new(TrackId::new(), "Track 1".into(), 0)]);
        let id0 = state.arrangement.tracks[0].id;

        if let Some(idx) = state.arrangement.tracks.iter().position(|t| t.id == id0) {
            if idx + 1 < state.arrangement.tracks.len() {
                state.arrangement.tracks.swap(idx, idx + 1);
            }
        }
        assert_eq!(state.arrangement.tracks[0].id, id0);
    }

    #[test]
    fn rename_track() {
        let mut state = make_state_with(vec![UiTrack::new(TrackId::new(), "Track 1".into(), 0)]);
        let id = state.arrangement.tracks[0].id;

        if let Some(track) = state.find_track_mut(id) {
            track.name = "My Custom Track".into();
        }
        assert_eq!(state.arrangement.tracks[0].name, "My Custom Track");
    }

    #[test]
    fn rename_note_clip() {
        let tid = TrackId::new();
        let cid = ClipId::new();
        let mut track = UiTrack::new_instrument(
            tid,
            "Synth".into(),
            TrackKind::Instrument(vibez_core::midi::InstrumentKind::SubtractiveSynth),
            0,
        );
        track.note_clips.push(UiNoteClip {
            id: cid,
            name: "Pattern 1".into(),
            position_beats: 0.0,
            duration_beats: 4.0,
            notes: Vec::new(),
            selected_notes: HashSet::new(),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
        });
        let mut state = make_state_with(vec![track]);

        if let Some(t) = state.find_track_mut(tid) {
            if let Some(c) = t.note_clips.iter_mut().find(|c| c.id == cid) {
                c.name = "Intro Pattern".into();
            }
        }
        assert_eq!(
            state.arrangement.tracks[0].note_clips[0].name,
            "Intro Pattern"
        );
    }

    #[test]
    fn settings_tab_default() {
        assert_eq!(SettingsTab::default(), SettingsTab::Audio);
    }

    #[test]
    fn settings_tab_equality() {
        assert_ne!(SettingsTab::Audio, SettingsTab::Plugins);
        assert_eq!(SettingsTab::Audio, SettingsTab::Audio);
    }

    #[test]
    fn app_state_default_buffer_size() {
        let state = AppState::default();
        assert_eq!(state.settings_buffer_size, 512);
    }

    #[test]
    fn app_state_default_settings_tab() {
        let state = AppState::default();
        assert_eq!(state.settings_tab, SettingsTab::Audio);
    }

    #[test]
    fn rename_empty_rejected() {
        let mut state = make_state_with(vec![UiTrack::new(TrackId::new(), "Track 1".into(), 0)]);
        let id = state.arrangement.tracks[0].id;

        // Simulate the FinishEditing guard: empty name doesn't rename
        let new_name = "";
        if !new_name.is_empty() {
            if let Some(track) = state.find_track_mut(id) {
                track.name = new_name.to_string();
            }
        }
        assert_eq!(state.arrangement.tracks[0].name, "Track 1");
    }
}
