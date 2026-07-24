use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod browser_results;
mod browser_state;
mod snapshot;
mod ui_types;

use crate::remote_provider::RemoteCatalogSnapshot;
pub use browser_results::LocalResults;
pub use browser_state::*;
pub use snapshot::*;
pub use ui_types::*;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::DEFAULT_BPM;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::MediaSourceRef;
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
pub const PERFORM_SURFACE_DEFAULT_WIDTH: f32 = 560.0;
pub const DETAIL_PANEL_DEFAULT_HEIGHT: f32 = 280.0;
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
/// View-domain state; none of it is part of the project itself.
#[derive(Debug)]
pub struct ViewState {
    pub workspace: Workspace,
    pub detail_panel_tab: DetailPanelTab,
    pub detail_panel_height: f32,
    pub detail_panel_resize_active: bool,
    pub perform_surface_width: f32,
    pub perform_surface_resize_active: bool,
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,
    pub snap_grid: SnapGrid,
    pub snap_enabled: bool,
    pub adaptive_grid: bool,
    pub adaptive_grid_bias: i8,
    pub context_menu: Option<ContextMenu>,
    pub edit_menu_open: bool,
    pub cursor_x: f32, // globally tracked for popup positioning and pane drags
    pub cursor_y: f32,
    pub window_width: f32, // last known size for responsive view clamping
    pub window_height: f32,
    pub editing_track_name: Option<TrackId>,
    pub editing_clip_name: Option<(TrackId, ClipId)>,
    pub edit_name_text: String,
}
impl Default for ViewState {
    fn default() -> Self {
        Self {
            workspace: Workspace::Arrange,
            detail_panel_tab: DetailPanelTab::Clip,
            detail_panel_height: DETAIL_PANEL_DEFAULT_HEIGHT,
            detail_panel_resize_active: false,
            perform_surface_width: PERFORM_SURFACE_DEFAULT_WIDTH,
            perform_surface_resize_active: false,
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
    pub pending_project_track_deletion: Option<TrackId>,
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AudioStreamHealth {
    #[default]
    Running,
    Rebuilding,
    Error(String),
}

impl AudioStreamHealth {
    pub fn description(&self) -> &str {
        match self {
            Self::Running => "Audio stream running",
            Self::Rebuilding => "Rebuilding audio stream",
            Self::Error(cause) => cause,
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

    pub status_text: String,
    /// Active project-export completion percentage.
    pub export_progress: Option<u8>,
    pub audio_stream_health: AudioStreamHealth,
    pub view: ViewState,

    pub piano_roll: PianoRollState,

    // Perform domain slice (runtime mode, bank, selection, and focus).
    pub perform: crate::domains::perform::PerformState,

    // Project Track domain slice (shared channels and devices).
    pub project_tracks: Arc<ProjectTracksState>,

    // Arrange-owned timeline content and editor state.
    pub arrangement: ArrangementState,

    // One runtime clipboard shared by Arrange and every Section editor.
    pub clip_clipboard: ClipClipboard,

    /// In-progress manual BPM input text keyed by clip id. Only
    /// populated while the user is actively editing the field in the
    // Device context menu
    pub devices: crate::domains::devices::DevicesState,

    // File menu / Settings
    pub settings_open: bool,
    pub settings_tab: SettingsTab,
    pub settings_buffer_size: u32,
    pub confirm_project_track_deletion: bool,
    // Project domain slice: file menu, path, dirty flag, undo.
    pub project: ProjectState,
    /// Automatically detect sample BPM and warp to project tempo on
    /// import. Mirrored from `UiSettings::auto_warp_on_import`.
    pub auto_warp_on_import: bool,
    /// Minimum BPM-detect confidence required to auto-warp. Mirrored
    /// from `UiSettings::warp_confidence_threshold`.
    pub warp_confidence_threshold: f32,
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
            export_progress: None,
            audio_stream_health: AudioStreamHealth::default(),
            view: ViewState::default(),
            piano_roll: PianoRollState::default(),
            perform: crate::domains::perform::PerformState::default(),
            project_tracks: Arc::new(ProjectTracksState {
                next_track_number: 1,
                ..ProjectTracksState::default()
            }),
            arrangement: ArrangementState::default(),
            clip_clipboard: ClipClipboard::default(),
            devices: crate::domains::devices::DevicesState::default(),
            settings_open: false,
            settings_tab: SettingsTab::default(),
            settings_buffer_size: 512,
            confirm_project_track_deletion: false,
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
    pub fn apply_audio_stream_event(
        &mut self,
        event: vibez_audio_io::audio_stream::AudioStreamEvent,
    ) {
        use vibez_audio_io::audio_stream::AudioStreamEvent;

        match event {
            AudioStreamEvent::Running => {
                self.audio_stream_health = AudioStreamHealth::Running;
            }
            AudioStreamEvent::Error(cause) => {
                self.status_text = format!("Audio stream error: {cause}");
                self.audio_stream_health = AudioStreamHealth::Error(cause);
            }
            AudioStreamEvent::Rebuilding => {
                self.status_text = "Rebuilding audio stream…".into();
                self.audio_stream_health = AudioStreamHealth::Rebuilding;
            }
            AudioStreamEvent::Recovered => {
                self.status_text = "Audio stream recovered".into();
                self.audio_stream_health = AudioStreamHealth::Running;
            }
        }
    }

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

    /// The Timeline Editor currently visible to the producer.
    ///
    /// Workspace identity is resolved here at the application boundary; the
    /// editor and its widgets only receive the resolved target.
    pub fn active_timeline_editor(&self) -> &TimelineEditorState {
        if self.view.workspace == Workspace::Perform && self.perform.selected_section.is_some() {
            self.perform.section_editor.editor()
        } else {
            &self.arrangement.editor
        }
    }

    pub fn active_timeline_content(&self, id: TrackId) -> Option<&TrackTimelineContent> {
        self.active_timeline_editor().timeline.get(id)
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
mod audio_stream_health_tests {
    use super::{AppState, AudioStreamHealth};
    use vibez_audio_io::audio_stream::AudioStreamEvent;

    #[test]
    fn stream_error_and_recovery_update_persistent_health_and_status() {
        let mut state = AppState::default();

        state.apply_audio_stream_event(AudioStreamEvent::Error(
            "device disconnected mid-session".into(),
        ));
        assert_eq!(
            state.audio_stream_health,
            AudioStreamHealth::Error("device disconnected mid-session".into())
        );
        assert_eq!(
            state.status_text,
            "Audio stream error: device disconnected mid-session"
        );

        state.apply_audio_stream_event(AudioStreamEvent::Rebuilding);
        assert_eq!(state.audio_stream_health, AudioStreamHealth::Rebuilding);
        assert_eq!(state.status_text, "Rebuilding audio stream…");

        state.apply_audio_stream_event(AudioStreamEvent::Recovered);
        assert_eq!(state.audio_stream_health, AudioStreamHealth::Running);
        assert_eq!(state.status_text, "Audio stream recovered");

        state.apply_audio_stream_event(AudioStreamEvent::Running);
        assert_eq!(state.audio_stream_health, AudioStreamHealth::Running);
        assert_eq!(state.status_text, "Audio stream recovered");
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
            groove_grid: vibez_core::perform::GrooveGrid::Off,
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
