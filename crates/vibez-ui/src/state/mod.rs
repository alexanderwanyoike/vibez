use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

mod browser_results;
mod browser_state;
mod ui_types;
pub use browser_results::LocalResults;
pub use browser_state::*;
pub use ui_types::*;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::DEFAULT_BPM;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::MediaSourceRef;
use vibez_engine::commands::AuditionSync;
use vibez_plugin_host::PluginSettings;

use crate::remote_provider::RemoteCatalogSnapshot;

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
    Perform,
    Appearance,
}

/// A point-in-time snapshot of the editable project state, used to implement
/// undo / redo. Project Tracks, Arrange, and the Section store are independently
/// shared so edits clone only the project-owned structure they change.
#[derive(Debug, Clone)]
pub struct ProjectSnapshot {
    pub project_tracks: Arc<ProjectTracksState>,
    pub arrange_timeline: Arc<ArrangementTimeline>,
    pub sections: Arc<crate::domains::perform::SectionStore>,
    pub bpm: f64,
    pub bpm_text: String,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
    pub selected_track: Option<TrackId>,
    pub selected_clips: HashSet<ArrangementSelection>,
    pub selected_note_clip: Option<(TrackId, ClipId)>,
    pub selected_section: Option<vibez_core::id::SectionId>,
}

/// Runtime identity for one continuous pointer gesture. Every incremental
/// project edit emitted while the pointer remains held carries the same id so
/// undo history can retain the pre-gesture snapshot only once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UndoGestureId(u64);

impl UndoGestureId {
    pub fn new() -> Self {
        static NEXT_GESTURE_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_GESTURE_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Project domain slice: file-menu visibility, the current file,
/// the dirty flag, and the undo/redo history.
#[derive(Debug, Default)]
pub struct ProjectState {
    pub file_menu_open: bool,
    pub current_path: Option<PathBuf>,
    pub dirty: bool,
    pub history: UndoHistory,
    /// Clips whose media could not be hydrated at load time. Invisible in
    /// the arrangement, but serialized back into every save so unavailable
    /// media stays relinkable instead of silently vanishing.
    pub unresolved_clips: Vec<crate::message::UnresolvedTimelineClip>,
}

#[derive(Debug, Default)]
pub struct UndoHistory {
    pub undo: VecDeque<ProjectSnapshot>,
    pub redo: VecDeque<ProjectSnapshot>,
    last_gesture: Option<UndoGestureId>,
}

impl UndoHistory {
    pub const CAPACITY: usize = 100;

    pub fn push_undo(&mut self, snapshot: ProjectSnapshot) {
        self.last_gesture = None;
        self.push_snapshot(snapshot);
    }

    fn push_snapshot(&mut self, snapshot: ProjectSnapshot) {
        self.undo.push_back(snapshot);
        if self.undo.len() > Self::CAPACITY {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    pub fn push_edit(&mut self, snapshot: ProjectSnapshot, gesture: Option<UndoGestureId>) {
        if gesture.is_some() && self.last_gesture == gesture {
            return;
        }
        self.push_snapshot(snapshot);
        self.last_gesture = gesture;
    }

    pub fn pop_undo(&mut self) -> Option<ProjectSnapshot> {
        self.last_gesture = None;
        self.undo.pop_back()
    }

    pub fn push_redo(&mut self, snapshot: ProjectSnapshot) {
        self.redo.push_back(snapshot);
        if self.redo.len() > Self::CAPACITY {
            self.redo.pop_front();
        }
    }

    pub fn pop_redo(&mut self) -> Option<ProjectSnapshot> {
        self.last_gesture = None;
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
        self.last_gesture = None;
    }
}

#[cfg(test)]
mod undo_tests;

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
pub fn new_master_track() -> ProjectTrack {
    let mut master = ProjectTrack::new(TrackId::MASTER, "Master".to_string(), 0);
    master.pan = 0.5;
    master
}

/// Project-owned tracks and channels shared by every musical timeline.
#[derive(Debug, Clone)]
pub struct ProjectTracksState {
    pub tracks: Vec<ProjectTrack>,
    /// The master bus channel (see [`new_master_track`]).
    pub master: ProjectTrack,
    /// Return channels: mixer-only tracks fed by per-track sends.
    pub buses: Vec<ProjectTrack>,
    pub next_track_number: u32,
}

impl Default for ProjectTracksState {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            master: new_master_track(),
            buses: Vec::new(),
            next_track_number: 0,
        }
    }
}

impl ProjectTracksState {
    pub fn find(&self, id: TrackId) -> Option<&ProjectTrack> {
        if id.is_master() {
            return Some(&self.master);
        }
        self.tracks
            .iter()
            .chain(self.buses.iter())
            .find(|t| t.id == id)
    }

    pub fn find_mut(&mut self, id: TrackId) -> Option<&mut ProjectTrack> {
        if id.is_master() {
            return Some(&mut self.master);
        }
        self.tracks
            .iter_mut()
            .chain(self.buses.iter_mut())
            .find(|t| t.id == id)
    }
}

/// One musical timeline's content, associated with shared Project Tracks by
/// stable identity. Hash-map storage prevents track reordering from moving or
/// cloning timeline content.
#[derive(Debug, Clone, Default)]
pub struct TimelineContent {
    pub by_track: HashMap<TrackId, TrackTimelineContent>,
}

impl TimelineContent {
    pub fn get(&self, track_id: TrackId) -> Option<&TrackTimelineContent> {
        self.by_track.get(&track_id)
    }

    pub fn get_mut(&mut self, track_id: TrackId) -> Option<&mut TrackTimelineContent> {
        self.by_track.get_mut(&track_id)
    }

    pub fn ensure(&mut self, track_id: TrackId) -> &mut TrackTimelineContent {
        self.by_track.entry(track_id).or_default()
    }

    pub fn remove(&mut self, track_id: TrackId) -> Option<TrackTimelineContent> {
        self.by_track.remove(&track_id)
    }
}

/// Backward-compatible name for the project's linear Arrange content.
pub type ArrangementTimeline = TimelineContent;

/// Editing state shared by every resolved musical timeline.
#[derive(Debug)]
pub struct TimelineEditorState {
    pub timeline: Arc<TimelineContent>,
    pub selected_track: Option<TrackId>,
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

impl Default for TimelineEditorState {
    fn default() -> Self {
        Self {
            timeline: Arc::new(TimelineContent::default()),
            selected_track: None,
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

impl std::ops::Deref for TimelineEditorState {
    type Target = TimelineContent;

    fn deref(&self) -> &Self::Target {
        &self.timeline
    }
}

impl std::ops::DerefMut for TimelineEditorState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.timeline)
    }
}

/// Arrange's thin adapter over the shared Timeline Editor state.
#[derive(Debug, Default)]
pub struct ArrangementState {
    pub(crate) editor: TimelineEditorState,
}

impl std::ops::Deref for ArrangementState {
    type Target = TimelineEditorState;

    fn deref(&self) -> &Self::Target {
        &self.editor
    }
}

impl std::ops::DerefMut for ArrangementState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.editor
    }
}

/// An adapter-resolved, read-only Timeline Editor target.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedTimeline<'a> {
    pub editor: &'a TimelineEditorState,
}

/// An adapter-resolved, mutable Timeline Editor target.
#[derive(Debug)]
pub struct ResolvedTimelineMut<'a> {
    pub editor: &'a mut TimelineEditorState,
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

    // Perform domain slice (runtime mode, bank, selection, and focus).
    pub perform: crate::domains::perform::PerformState,

    // Project Track domain slice (shared channels and devices).
    pub project_tracks: Arc<ProjectTracksState>,

    // Arrange-owned timeline content and editor state.
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
            perform: crate::domains::perform::PerformState::default(),
            project_tracks: Arc::new(ProjectTracksState {
                next_track_number: 1,
                ..ProjectTracksState::default()
            }),
            arrangement: ArrangementState::default(),
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
        crate::timeline_geometry::TimelineGeometry::from_zoom(
            self.view.zoom_level,
            self.view.scroll_offset_beats,
        )
        .pixels_per_beat()
    }

    /// Number of beats visible in a canvas of the given width.
    #[allow(dead_code)]
    pub fn visible_beats(&self, canvas_width: f32) -> f64 {
        crate::timeline_geometry::TimelineGeometry::from_zoom(
            self.view.zoom_level,
            self.view.scroll_offset_beats,
        )
        .visible_beats(canvas_width)
    }

    /// Convert a beat value to a pixel x coordinate in the viewport.
    #[allow(dead_code)]
    pub fn beat_to_x(&self, beat: f64) -> f32 {
        crate::timeline_geometry::TimelineGeometry::from_zoom(
            self.view.zoom_level,
            self.view.scroll_offset_beats,
        )
        .beat_to_x(beat)
    }

    /// Convert a pixel x coordinate in the viewport to a beat value.
    #[allow(dead_code)]
    pub fn x_to_beat(&self, x: f32) -> f64 {
        crate::timeline_geometry::TimelineGeometry::from_zoom(
            self.view.zoom_level,
            self.view.scroll_offset_beats,
        )
        .x_to_beat(x)
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

    pub fn find_track(&self, id: TrackId) -> Option<&ProjectTrack> {
        self.project_tracks.find(id)
    }

    pub fn find_track_mut(&mut self, id: TrackId) -> Option<&mut ProjectTrack> {
        Arc::make_mut(&mut self.project_tracks).find_mut(id)
    }

    pub fn arrange_content(&self, id: TrackId) -> Option<&TrackTimelineContent> {
        self.arrangement.timeline.get(id)
    }

    pub fn arrange_content_mut(&mut self, id: TrackId) -> &mut TrackTimelineContent {
        Arc::make_mut(&mut self.arrangement.timeline).ensure(id)
    }

    /// Total duration in samples across all tracks (max clip end position).
    pub fn total_duration_samples(&self) -> u64 {
        let audio_max = self
            .arrangement
            .timeline
            .by_track
            .values()
            .flat_map(|content| content.clips.iter())
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
                .timeline
                .by_track
                .values()
                .flat_map(|content| content.note_clips.iter())
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

    fn make_state_with(tracks: Vec<ProjectTrack>) -> AppState {
        let mut state = AppState::default();
        Arc::make_mut(&mut state.project_tracks).tracks = tracks;
        state
    }

    fn make_two_tracks() -> Vec<ProjectTrack> {
        vec![
            ProjectTrack::new(TrackId::new(), "Track 1".into(), 0),
            ProjectTrack::new(TrackId::new(), "Track 2".into(), 1),
        ]
    }

    #[test]
    fn move_track_up() {
        let mut state = make_state_with(make_two_tracks());
        let id0 = state.project_tracks.tracks[0].id;
        let id1 = state.project_tracks.tracks[1].id;

        if let Some(idx) = state.project_tracks.tracks.iter().position(|t| t.id == id1) {
            if idx > 0 {
                Arc::make_mut(&mut state.project_tracks)
                    .tracks
                    .swap(idx, idx - 1);
            }
        }
        assert_eq!(state.project_tracks.tracks[0].id, id1);
        assert_eq!(state.project_tracks.tracks[1].id, id0);
    }

    #[test]
    fn move_track_down() {
        let mut state = make_state_with(make_two_tracks());
        let id0 = state.project_tracks.tracks[0].id;
        let id1 = state.project_tracks.tracks[1].id;

        if let Some(idx) = state.project_tracks.tracks.iter().position(|t| t.id == id0) {
            if idx + 1 < state.project_tracks.tracks.len() {
                Arc::make_mut(&mut state.project_tracks)
                    .tracks
                    .swap(idx, idx + 1);
            }
        }
        assert_eq!(state.project_tracks.tracks[0].id, id1);
        assert_eq!(state.project_tracks.tracks[1].id, id0);
    }

    #[test]
    fn move_first_track_up_noop() {
        let mut state =
            make_state_with(vec![ProjectTrack::new(TrackId::new(), "Track 1".into(), 0)]);
        let id0 = state.project_tracks.tracks[0].id;

        if let Some(idx) = state.project_tracks.tracks.iter().position(|t| t.id == id0) {
            if idx > 0 {
                Arc::make_mut(&mut state.project_tracks)
                    .tracks
                    .swap(idx, idx - 1);
            }
        }
        assert_eq!(state.project_tracks.tracks[0].id, id0);
    }

    #[test]
    fn move_last_track_down_noop() {
        let mut state =
            make_state_with(vec![ProjectTrack::new(TrackId::new(), "Track 1".into(), 0)]);
        let id0 = state.project_tracks.tracks[0].id;

        if let Some(idx) = state.project_tracks.tracks.iter().position(|t| t.id == id0) {
            if idx + 1 < state.project_tracks.tracks.len() {
                Arc::make_mut(&mut state.project_tracks)
                    .tracks
                    .swap(idx, idx + 1);
            }
        }
        assert_eq!(state.project_tracks.tracks[0].id, id0);
    }

    #[test]
    fn rename_track() {
        let mut state =
            make_state_with(vec![ProjectTrack::new(TrackId::new(), "Track 1".into(), 0)]);
        let id = state.project_tracks.tracks[0].id;

        if let Some(track) = state.find_track_mut(id) {
            track.name = "My Custom Track".into();
        }
        assert_eq!(state.project_tracks.tracks[0].name, "My Custom Track");
    }

    #[test]
    fn rename_note_clip() {
        let tid = TrackId::new();
        let cid = ClipId::new();
        let track = ProjectTrack::new_instrument(
            tid,
            "Synth".into(),
            TrackKind::Instrument(vibez_core::midi::InstrumentKind::SubtractiveSynth),
            0,
        );
        let mut state = make_state_with(vec![track]);
        state.arrange_content_mut(tid).note_clips.push(UiNoteClip {
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
        if let Some(t) = Arc::make_mut(&mut state.arrangement.timeline).get_mut(tid) {
            if let Some(c) = t.note_clips.iter_mut().find(|c| c.id == cid) {
                c.name = "Intro Pattern".into();
            }
        }
        assert_eq!(
            state.arrange_content(tid).unwrap().note_clips[0].name,
            "Intro Pattern"
        );
    }

    #[test]
    fn timeline_edits_clone_only_timeline_content() {
        let track_id = TrackId::new();
        let mut state = make_state_with(vec![ProjectTrack::new(
            track_id,
            "Shared Project Track".into(),
            0,
        )]);
        state.arrange_content_mut(track_id);

        let project_tracks_before = Arc::clone(&state.project_tracks);
        let timeline_before = Arc::clone(&state.arrangement.timeline);
        state.arrange_content_mut(track_id).automation.push(
            vibez_core::automation::AutomationLane::new(
                vibez_core::automation::AutomationTarget::TrackGain,
            ),
        );

        assert!(Arc::ptr_eq(&project_tracks_before, &state.project_tracks));
        assert!(!Arc::ptr_eq(&timeline_before, &state.arrangement.timeline));
        assert_eq!(state.project_tracks.tracks[0].name, "Shared Project Track");
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
        let mut state =
            make_state_with(vec![ProjectTrack::new(TrackId::new(), "Track 1".into(), 0)]);
        let id = state.project_tracks.tracks[0].id;

        // Simulate the FinishEditing guard: empty name doesn't rename
        let new_name = "";
        if !new_name.is_empty() {
            if let Some(track) = state.find_track_mut(id) {
                track.name = new_name.to_string();
            }
        }
        assert_eq!(state.project_tracks.tracks[0].name, "Track 1");
    }
}
