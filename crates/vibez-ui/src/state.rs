use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::DEFAULT_BPM;
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};
use vibez_core::track::{DrumPadState, MediaSourceRef};
use vibez_dropbox::DropboxEntry;
use vibez_plugin_host::PluginSettings;

/// A clip as represented in the UI.
#[derive(Debug, Clone)]
pub struct UiClip {
    pub id: ClipId,
    pub name: String,
    pub audio: Arc<DecodedAudio>,
    pub source: Option<MediaSourceRef>,
    /// Position on the timeline in samples.
    pub position: u64,
    /// Offset into the source audio in samples.
    pub source_offset: u64,
    /// Duration in samples.
    pub duration: u64,
    // Looping
    pub loop_enabled: bool,
    pub loop_start: u64,
    pub loop_end: u64,
    /// Nominal BPM of the underlying sample. `None` until detected or
    /// entered manually.
    pub original_bpm: Option<f64>,
    /// Whether `audio` has been time-stretched to fit the project
    /// tempo.
    pub warped: bool,
    /// Project BPM the current warped audio was stretched to. Used to
    /// flag staleness in the timeline when the project tempo changes.
    pub warped_to_bpm: Option<f64>,
    /// Un-warped source audio, retained so the UI can re-warp to a new
    /// project BPM or clear the warp without re-decoding. Populated on
    /// import or on first warp. Not persisted: on reload the UI
    /// re-decodes from the source and re-warps.
    pub original_audio: Option<Arc<DecodedAudio>>,
}

#[derive(Debug, Clone)]
pub struct UiDrumPad {
    pub name: Option<String>,
    pub source: Option<MediaSourceRef>,
    /// Decoded audio kept on the UI side so offline bounce can re-seed a
    /// drum rack without a round-trip through the audio thread.
    pub audio: Option<Arc<DecodedAudio>>,
    pub gain: f32,
    pub pan: f32,
    pub start: f32,
    pub end: f32,
    pub coarse_tune: i8,
    pub fine_tune: f32,
    pub one_shot: bool,
    pub choke_group: Option<u8>,
}

impl Default for UiDrumPad {
    fn default() -> Self {
        Self {
            name: None,
            source: None,
            audio: None,
            gain: 1.0,
            pan: 0.0,
            start: 0.0,
            end: 1.0,
            coarse_tune: 0,
            fine_tune: 0.0,
            one_shot: true,
            choke_group: None,
        }
    }
}

impl UiDrumPad {
    pub fn to_state(&self) -> DrumPadState {
        DrumPadState {
            source: self.source.clone(),
            gain: self.gain,
            pan: self.pan,
            start: self.start,
            end: self.end,
            coarse_tune: self.coarse_tune,
            fine_tune: self.fine_tune,
            one_shot: self.one_shot,
            choke_group: self.choke_group,
        }
    }

    pub fn from_state(state: &DrumPadState) -> Self {
        Self {
            name: state.source.as_ref().map(MediaSourceRef::display_name),
            source: state.source.clone(),
            audio: None,
            gain: state.gain,
            pan: state.pan,
            start: state.start,
            end: state.end,
            coarse_tune: state.coarse_tune,
            fine_tune: state.fine_tune,
            one_shot: state.one_shot,
            choke_group: state.choke_group,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SampleBrowserEntry {
    pub source: MediaSourceRef,
    pub name: String,
    pub root_path: PathBuf,
    pub relative_path: PathBuf,
    pub search_text: String,
}

/// An effect instance as represented in the UI.
#[derive(Debug, Clone)]
pub struct UiEffect {
    pub id: EffectId,
    pub effect_type: EffectType,
    pub bypass: bool,
    pub params: Vec<f32>,
    pub descriptors: &'static [ParamDescriptor],
    /// Display name override for external plugins.
    pub plugin_name: Option<String>,
    /// Whether this effect has a native plugin GUI available.
    pub has_plugin_gui: bool,
    /// Persistent identity of the plugin backing this slot, if any.
    pub plugin_ref: Option<vibez_core::effect::PluginDeviceInfo>,
}

/// A note clip (MIDI pattern) as represented in the UI.
#[derive(Debug, Clone)]
pub struct UiNoteClip {
    pub id: ClipId,
    pub name: String,
    pub position_beats: f64,
    pub duration_beats: f64,
    pub notes: Vec<MidiNote>,
    pub selected_notes: HashSet<usize>,
    // Looping
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
}

/// A track as represented in the UI.
#[derive(Debug, Clone)]
pub struct UiTrack {
    pub id: TrackId,
    pub name: String,
    pub clips: Vec<UiClip>,
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub peak_l: f32,
    pub peak_r: f32,
    pub effects: Vec<UiEffect>,
    pub note_clips: Vec<UiNoteClip>,
    pub kind: TrackKind,
    pub color_index: u8,
    pub has_instrument: bool,
    pub instrument_kind: Option<InstrumentKind>,
    pub sample_name: Option<String>,
    pub sample_source: Option<MediaSourceRef>,
    /// Decoded audio for the sampler, kept UI-side so offline bounce can
    /// re-seed a fresh sampler instance.
    pub sample_audio: Option<Arc<DecodedAudio>>,
    pub instrument_params: Vec<f32>,
    pub drum_rack_pads: Vec<UiDrumPad>,
    pub selected_drum_pad: usize,
    /// Display name for external plugin instruments (e.g. "Dexed", "Surge XT").
    pub plugin_instrument_name: Option<String>,
    /// Persistent identity of the plugin instrument, if any.
    pub plugin_instrument_ref: Option<vibez_core::effect::PluginDeviceInfo>,
    /// Whether the plugin instrument has a native GUI.
    pub has_plugin_instrument_gui: bool,
}

impl UiTrack {
    pub fn new(id: TrackId, name: String, color_index: u8) -> Self {
        Self {
            id,
            name,
            clips: Vec::new(),
            gain: 1.0,
            pan: 0.5,
            mute: false,
            solo: false,
            peak_l: 0.0,
            peak_r: 0.0,
            effects: Vec::new(),
            note_clips: Vec::new(),
            kind: TrackKind::Audio,
            color_index,
            has_instrument: false,
            instrument_kind: None,
            sample_name: None,
            sample_source: None,
            sample_audio: None,
            instrument_params: Vec::new(),
            drum_rack_pads: default_drum_rack_pads(),
            selected_drum_pad: 0,
            plugin_instrument_name: None,
            plugin_instrument_ref: None,
            has_plugin_instrument_gui: false,
        }
    }

    pub fn new_instrument(id: TrackId, name: String, kind: TrackKind, color_index: u8) -> Self {
        let (has_instrument, instrument_kind) = match kind {
            TrackKind::Instrument(ik) => (true, Some(ik)),
            _ => (false, None),
        };
        Self {
            id,
            name,
            clips: Vec::new(),
            gain: 1.0,
            pan: 0.5,
            mute: false,
            solo: false,
            peak_l: 0.0,
            peak_r: 0.0,
            effects: Vec::new(),
            note_clips: Vec::new(),
            kind,
            color_index,
            has_instrument,
            instrument_kind,
            sample_name: None,
            sample_source: None,
            sample_audio: None,
            instrument_params: Vec::new(),
            drum_rack_pads: default_drum_rack_pads(),
            selected_drum_pad: 0,
            plugin_instrument_name: None,
            plugin_instrument_ref: None,
            has_plugin_instrument_gui: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArrangementSelection {
    AudioClip { track_id: TrackId, clip_id: ClipId },
    NoteClip { track_id: TrackId, clip_id: ClipId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Arrange,
    Mix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailPanelTab {
    Clip,
    Devices,
}

/// Snap grid for piano roll quantization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapGrid {
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
}

impl SnapGrid {
    /// Duration of one grid unit in beats.
    pub fn beat_size(self) -> f64 {
        match self {
            SnapGrid::Quarter => 1.0,
            SnapGrid::Eighth => 0.5,
            SnapGrid::Sixteenth => 0.25,
            SnapGrid::ThirtySecond => 0.125,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SnapGrid::Quarter => "1/4",
            SnapGrid::Eighth => "1/8",
            SnapGrid::Sixteenth => "1/16",
            SnapGrid::ThirtySecond => "1/32",
        }
    }

    pub fn all() -> &'static [SnapGrid] {
        &[
            SnapGrid::Quarter,
            SnapGrid::Eighth,
            SnapGrid::Sixteenth,
            SnapGrid::ThirtySecond,
        ]
    }

    /// Snap a beat value to the nearest grid position.
    pub fn snap_beat(self, beat: f64) -> f64 {
        let size = self.beat_size();
        (beat / size).round() * size
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Audio,
    Plugins,
    Dropbox,
    Warping,
}

/// A point-in-time snapshot of the editable project state, used to
/// implement undo / redo. `UiTrack` clones share audio via `Arc` so
/// each snapshot is cheap on memory despite the full tree.
#[derive(Debug, Clone)]
pub struct ProjectSnapshot {
    pub tracks: Vec<UiTrack>,
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

pub struct AppState {
    // Transport domain slice (playback, tempo, arrangement loop).
    pub transport: TransportState,

    // Metering (master)
    pub peak_l: f32,
    pub peak_r: f32,

    // UI
    pub status_text: String,
    pub workspace: Workspace,

    // Zoom / scroll (arrangement timeline)
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,

    // Piano roll
    pub snap_grid: SnapGrid,
    pub piano_roll_scroll_y: f32,

    // Multi-track
    pub tracks: Vec<UiTrack>,
    pub selected_track: Option<TrackId>,
    pub next_track_number: u32,

    // Detail panel: which note clip is selected for piano roll editing
    pub selected_note_clip: Option<(TrackId, ClipId)>,

    // Arrangement clip selection (multi-select)
    pub selected_clips: HashSet<ArrangementSelection>,

    // Detail panel tab
    pub detail_panel_tab: DetailPanelTab,

    // Arrangement loop
    // Time selection (visible brackets on arrangement — independent from loop)
    pub time_selection_active: bool,
    pub selection_start_beats: f64,
    pub selection_end_beats: f64,
    /// Track whose lane was dragged to create the current time selection,
    /// if any. `None` means the selection is arrangement-wide (ruler drag).
    pub time_selection_track: Option<TrackId>,

    // Context menu
    pub context_menu: Option<ContextMenu>,

    // Cursor tracking (for right-click positioning from mouse_area)
    pub cursor_x: f32,
    pub cursor_y: f32,

    // Last known window size, for clamping popup menus on-screen
    pub window_width: f32,
    pub window_height: f32,

    // Arrangement drag auto-scroll: tracks active resize/move for tick-driven edge scrolling
    pub drag_resize_active: bool,

    // Inline renaming
    pub editing_track_name: Option<TrackId>,
    pub editing_clip_name: Option<(TrackId, ClipId)>,
    pub edit_name_text: String,
    /// In-progress manual BPM input text keyed by clip id. Only
    /// populated while the user is actively editing the field in the
    /// clip detail panel; `None` / missing means show the committed
    /// `UiClip::original_bpm` value instead.
    pub clip_bpm_edit: HashMap<ClipId, String>,

    // Piano roll edit mode
    pub piano_roll_edit_mode: PianoRollEditMode,

    // Device context menu
    pub devices: crate::domains::devices::DevicesState,

    // File menu / Settings
    pub file_menu_open: bool,
    pub settings_open: bool,
    pub settings_tab: SettingsTab,
    pub settings_buffer_size: u32,
    pub current_project_path: Option<PathBuf>,
    pub project_dirty: bool,
    pub sample_browser_open: bool,
    /// Automatically detect sample BPM and warp to project tempo on
    /// import. Mirrored from `UiSettings::auto_warp_on_import`.
    pub auto_warp_on_import: bool,
    /// Minimum BPM-detect confidence required to auto-warp. Mirrored
    /// from `UiSettings::warp_confidence_threshold`.
    pub warp_confidence_threshold: f32,
    pub sample_browser_search: String,
    pub sample_browser_roots: Vec<PathBuf>,
    pub sample_browser_entries: Vec<SampleBrowserEntry>,
    pub sample_browser_root_filter: Option<PathBuf>,
    pub sample_browser_selected_source: Option<MediaSourceRef>,
    pub sample_browser_scan_in_progress: bool,
    pub sample_browser_mode: SampleBrowserMode,

    // Plugin hosting
    pub plugin_settings: PluginSettings,
    pub plugin_scan_in_progress: bool,
    pub plugin_scan_status: String,

    // Dropbox
    pub dropbox: DropboxUiState,

    // Drag-and-drop from sample browser
    pub drag_source: Option<MediaSourceRef>,
    pub drag_label: Option<String>,
    /// Most recent track the cursor has been confirmed over while a drag
    /// is in flight. Used as the drop target if the release happens on a
    /// sub-pixel boundary between lanes.
    pub drag_hover_track: Option<TrackId>,
    pub drag_hover_beat: f64,
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
            status_text: "Ready — Add a track to get started".to_string(),
            workspace: Workspace::Arrange,
            zoom_level: 1.0,
            scroll_offset_beats: 0.0,
            snap_grid: SnapGrid::Eighth,
            piano_roll_scroll_y: crate::widgets::piano_roll::default_scroll_y(200.0),
            tracks: Vec::new(),
            selected_track: None,
            next_track_number: 1,
            selected_note_clip: None,
            selected_clips: HashSet::new(),
            detail_panel_tab: DetailPanelTab::Clip,
            time_selection_active: false,
            selection_start_beats: 0.0,
            selection_end_beats: 0.0,
            time_selection_track: None,
            context_menu: None,
            cursor_x: 0.0,
            cursor_y: 0.0,
            window_width: 1400.0,
            window_height: 900.0,
            drag_resize_active: false,
            editing_track_name: None,
            editing_clip_name: None,
            edit_name_text: String::new(),
            clip_bpm_edit: HashMap::new(),
            piano_roll_edit_mode: PianoRollEditMode::default(),
            devices: crate::domains::devices::DevicesState::default(),
            file_menu_open: false,
            settings_open: false,
            settings_tab: SettingsTab::default(),
            settings_buffer_size: 512,
            current_project_path: None,
            project_dirty: false,
            sample_browser_open: true,
            auto_warp_on_import: false,
            warp_confidence_threshold: 0.6,
            sample_browser_search: String::new(),
            sample_browser_roots: Vec::new(),
            sample_browser_entries: Vec::new(),
            sample_browser_root_filter: None,
            sample_browser_selected_source: None,
            sample_browser_scan_in_progress: false,
            sample_browser_mode: SampleBrowserMode::default(),
            plugin_settings: PluginSettings::load(),
            plugin_scan_in_progress: false,
            plugin_scan_status: String::new(),
            dropbox: DropboxUiState::default(),
            drag_source: None,
            drag_label: None,
            drag_hover_track: None,
            drag_hover_beat: 0.0,
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
        20.0 * self.zoom_level
    }

    /// Number of beats visible in a canvas of the given width.
    #[allow(dead_code)]
    pub fn visible_beats(&self, canvas_width: f32) -> f64 {
        canvas_width as f64 / self.pixels_per_beat() as f64
    }

    /// Convert a beat value to a pixel x coordinate in the viewport.
    #[allow(dead_code)]
    pub fn beat_to_x(&self, beat: f64) -> f32 {
        ((beat - self.scroll_offset_beats) * self.pixels_per_beat() as f64) as f32
    }

    /// Convert a pixel x coordinate in the viewport to a beat value.
    #[allow(dead_code)]
    pub fn x_to_beat(&self, x: f32) -> f64 {
        x as f64 / self.pixels_per_beat() as f64 + self.scroll_offset_beats
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
        self.selected_clips.iter().any(|sel| match sel {
            ArrangementSelection::AudioClip { clip_id: cid, .. } => *cid == clip_id,
            ArrangementSelection::NoteClip { clip_id: cid, .. } => *cid == clip_id,
        })
    }

    /// Returns the single selected clip if exactly one is selected.
    #[allow(dead_code)]
    pub fn single_selected_clip(&self) -> Option<ArrangementSelection> {
        if self.selected_clips.len() == 1 {
            self.selected_clips.iter().next().copied()
        } else {
            None
        }
    }

    pub fn find_track(&self, id: TrackId) -> Option<&UiTrack> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn find_track_mut(&mut self, id: TrackId) -> Option<&mut UiTrack> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// Total duration in samples across all tracks (max clip end position).
    pub fn total_duration_samples(&self) -> u64 {
        let audio_max = self
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
            self.tracks
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

    fn make_state_with(tracks: Vec<UiTrack>) -> AppState {
        AppState {
            tracks,
            ..Default::default()
        }
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
        let id0 = state.tracks[0].id;
        let id1 = state.tracks[1].id;

        if let Some(idx) = state.tracks.iter().position(|t| t.id == id1) {
            if idx > 0 {
                state.tracks.swap(idx, idx - 1);
            }
        }
        assert_eq!(state.tracks[0].id, id1);
        assert_eq!(state.tracks[1].id, id0);
    }

    #[test]
    fn move_track_down() {
        let mut state = make_state_with(make_two_tracks());
        let id0 = state.tracks[0].id;
        let id1 = state.tracks[1].id;

        if let Some(idx) = state.tracks.iter().position(|t| t.id == id0) {
            if idx + 1 < state.tracks.len() {
                state.tracks.swap(idx, idx + 1);
            }
        }
        assert_eq!(state.tracks[0].id, id1);
        assert_eq!(state.tracks[1].id, id0);
    }

    #[test]
    fn move_first_track_up_noop() {
        let mut state = make_state_with(vec![UiTrack::new(TrackId::new(), "Track 1".into(), 0)]);
        let id0 = state.tracks[0].id;

        if let Some(idx) = state.tracks.iter().position(|t| t.id == id0) {
            if idx > 0 {
                state.tracks.swap(idx, idx - 1);
            }
        }
        assert_eq!(state.tracks[0].id, id0);
    }

    #[test]
    fn move_last_track_down_noop() {
        let mut state = make_state_with(vec![UiTrack::new(TrackId::new(), "Track 1".into(), 0)]);
        let id0 = state.tracks[0].id;

        if let Some(idx) = state.tracks.iter().position(|t| t.id == id0) {
            if idx + 1 < state.tracks.len() {
                state.tracks.swap(idx, idx + 1);
            }
        }
        assert_eq!(state.tracks[0].id, id0);
    }

    #[test]
    fn rename_track() {
        let mut state = make_state_with(vec![UiTrack::new(TrackId::new(), "Track 1".into(), 0)]);
        let id = state.tracks[0].id;

        if let Some(track) = state.find_track_mut(id) {
            track.name = "My Custom Track".into();
        }
        assert_eq!(state.tracks[0].name, "My Custom Track");
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
        assert_eq!(state.tracks[0].note_clips[0].name, "Intro Pattern");
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
        let id = state.tracks[0].id;

        // Simulate the FinishEditing guard: empty name doesn't rename
        let new_name = "";
        if !new_name.is_empty() {
            if let Some(track) = state.find_track_mut(id) {
                track.name = new_name.to_string();
            }
        }
        assert_eq!(state.tracks[0].name, "Track 1");
    }
}
