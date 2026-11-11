use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod browser_results;
mod browser_state;
mod clip_drag;
mod project_tracks;
mod same_channels;
mod snapshot;
pub use project_tracks::ProjectTracksState;
mod transport_state;
mod ui_types;
pub use transport_state::{PlaybackPhase, TransportState};

use crate::remote_provider::RemoteCatalogSnapshot;
use crate::widgets::clip_editor_view::StoredClipView;
pub use browser_results::LocalResults;
pub use browser_state::*;
pub use clip_drag::*;
pub use snapshot::*;
pub use ui_types::*;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::DEFAULT_BPM;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::MediaSourceRef;
use vibez_plugin_host::PluginSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuditionMode {
    Raw,
    #[default]
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

impl AuditionImportInput {
    /// Resolve Browser WARP from source evidence only. Project tempo is not an
    /// input because it must never change the identified source tempo.
    pub fn resolve_for_analysis(
        mut self,
        analysed: &crate::message::AnalysedBrowserAudio,
    ) -> Result<Self, String> {
        if self.mode == AuditionMode::Raw {
            return Ok(self);
        }
        if analysed.audio.num_frames() == 0 || analysed.audio.sample_rate == 0 {
            return Err("Cannot prepare empty or invalid audio".into());
        }
        if self.source_bpm.is_some() {
            return Ok(self);
        }
        let Some(fit) = analysed.loop_fit else {
            self.mode = AuditionMode::Raw;
            self.source_bpm = None;
            return Ok(self);
        };
        self.source_bpm = Some(fit.source_bpm);
        Ok(self)
    }
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

#[derive(Debug, Clone, PartialEq)]
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
    /// A Drum Rack card away from its pads, where a drop loads nothing.
    DrumRack {
        track_id: TrackId,
    },
    /// A device panel outside any instrument card, and what a drop there does.
    DevicePanel {
        track_id: TrackId,
        drop: crate::domains::device_panel_drop::DevicePanelDrop,
    },
}
/// View-domain state; none of it is part of the project itself.
#[derive(Debug)]
pub struct ViewState {
    pub workspace: Workspace,
    pub detail_panel_tab: DetailPanelTab,
    pub detail_panel_height: f32,
    pub detail_panel_resize_active: bool,
    /// The detail card's height in Mix once its divider has been dragged.
    pub mix_detail_panel_height: Option<f32>,
    pub perform_surface_width: f32,
    pub perform_surface_resize_active: bool,
    /// How far the Arrange timeline is zoomed and scrolled.
    pub viewport: crate::timeline_geometry::TimelineViewport,
    /// The Arrange view a zoom to selection replaced, for zoom back.
    pub zoom_before_selection: Option<crate::timeline_geometry::TimelineViewport>,
    /// What the producer asked the clip editor to show of the clip it last
    /// showed. Opening another clip shows that clip afresh.
    pub stored_clip_editor_view: Option<(ClipId, StoredClipView)>,
    /// The clip editor view a frame of the loop replaced, for zoom back.
    pub clip_view_before_loop: Option<(ClipId, Option<StoredClipView>)>,
    /// Arrange and Section timelines. The transport bar always edits it; the
    /// grid shortcuts edit it unless a clip editor has focus.
    pub arrange_grid: GridConfig,
    /// Piano roll and audio clip editor, following their own zoom.
    pub clip_grid: GridConfig,
    pub context_menu: Option<ContextMenu>,
    pub drum_rack_slice_dialog: Option<DrumRackSliceDialog>,
    pub transient_analysis_dialog: Option<TransientAnalysisDialog>,
    pub edit_menu_open: bool,
    pub cursor_x: f32, // globally tracked for popup positioning and pane drags
    pub cursor_y: f32,
    pub window_width: f32, // last known size for responsive view clamping
    pub window_height: f32,
    pub editing_track_name: Option<TrackId>,
    pub editing_clip_name: Option<(TrackId, ClipId)>,
    pub edit_name_text: String,
    /// The Arrange track header under the pointer, which shows its quieter
    /// actions only while hovered.
    pub hovered_track_header: Option<TrackId>,
    /// Short-lived, engine-authored note flashes for device feedback.
    /// This is view state only and is never persisted with the project.
    pub(crate) drum_pad_flash_until: HashMap<(TrackId, u8), std::time::Instant>,
    /// Modifier keys held now, for mouse clicks that change meaning with
    /// them.
    pub held_modifiers: iced::keyboard::Modifiers,
    /// The bank of buses the mixer's send rows show.
    pub mixer_send_bank: usize,
}
impl Default for ViewState {
    fn default() -> Self {
        Self {
            workspace: Workspace::Perform,
            detail_panel_tab: DetailPanelTab::Clip,
            detail_panel_height: DETAIL_PANEL_DEFAULT_HEIGHT,
            detail_panel_resize_active: false,
            mix_detail_panel_height: None,
            perform_surface_width: PERFORM_SURFACE_DEFAULT_WIDTH,
            perform_surface_resize_active: false,
            viewport: crate::timeline_geometry::TimelineViewport::new(1.0, 0.0),
            zoom_before_selection: None,
            stored_clip_editor_view: None,
            clip_view_before_loop: None,
            arrange_grid: GridConfig::default(),
            clip_grid: GridConfig::default(),
            context_menu: None,
            drum_rack_slice_dialog: None,
            transient_analysis_dialog: None,
            edit_menu_open: false,
            cursor_x: 0.0,
            cursor_y: 0.0,
            window_width: 1400.0,
            window_height: 900.0,
            editing_track_name: None,
            editing_clip_name: None,
            edit_name_text: String::new(),
            hovered_track_header: None,
            drum_pad_flash_until: HashMap::new(),
            held_modifiers: iced::keyboard::Modifiers::empty(),
            mixer_send_bank: 0,
        }
    }
}

impl ViewState {
    pub(crate) fn clip_editor_view(&self, clip_id: ClipId) -> Option<StoredClipView> {
        self.stored_clip_editor_view
            .filter(|(shown, _)| *shown == clip_id)
            .map(|(_, viewport)| viewport)
    }

    const DRUM_PAD_FLASH_HOLD: std::time::Duration = std::time::Duration::from_millis(140);

    pub fn trigger_drum_pad_flash(
        &mut self,
        track_id: TrackId,
        pitch: u8,
        now: std::time::Instant,
    ) {
        self.drum_pad_flash_until
            .insert((track_id, pitch), now + Self::DRUM_PAD_FLASH_HOLD);
    }

    pub fn drum_pad_is_flashing(
        &self,
        track_id: TrackId,
        pitch: u8,
        now: std::time::Instant,
    ) -> bool {
        self.drum_pad_flash_until
            .get(&(track_id, pitch))
            .is_some_and(|expires_at| *expires_at > now)
    }

    pub fn drum_pad_bank_is_flashing(
        &self,
        track_id: TrackId,
        bank: usize,
        now: std::time::Instant,
    ) -> bool {
        let first_pad = bank.saturating_mul(vibez_core::track::DRUM_RACK_BANK_SIZE);
        let end_pad = first_pad
            .saturating_add(vibez_core::track::DRUM_RACK_BANK_SIZE)
            .min(vibez_core::track::DRUM_RACK_PAD_COUNT);
        (first_pad..end_pad).any(|pad| {
            vibez_core::track::drum_rack_pad_pitch(pad)
                .is_some_and(|pitch| self.drum_pad_is_flashing(track_id, pitch, now))
        })
    }

    pub fn prune_drum_pad_flashes(&mut self, now: std::time::Instant) {
        self.drum_pad_flash_until
            .retain(|_, expires_at| *expires_at > now);
    }

    pub fn dismiss_clip_dialogs_for(
        &mut self,
        location: vibez_project::TimelineLocation,
        deleted: &HashSet<(TrackId, ClipId)>,
    ) {
        if self.drum_rack_slice_dialog.is_some_and(|dialog| {
            dialog.location == location && deleted.contains(&(dialog.track_id, dialog.clip_id))
        }) {
            self.drum_rack_slice_dialog = None;
        }
        if self
            .transient_analysis_dialog
            .as_ref()
            .is_some_and(|dialog| {
                dialog.location == location && deleted.contains(&(dialog.track_id, dialog.clip_id))
            })
        {
            self.transient_analysis_dialog = None;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrumRackSliceDialog {
    pub location: vibez_project::TimelineLocation,
    pub track_id: TrackId,
    pub clip_id: ClipId,
    pub markers: crate::domains::arrangement::AudioSliceMarkers,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransientAnalysisDialog {
    pub location: vibez_project::TimelineLocation,
    pub track_id: TrackId,
    pub clip_id: ClipId,
    pub sensitivity: vibez_core::onset::TransientSensitivity,
    pub sensitivity_input: String,
}

impl TransientAnalysisDialog {
    pub fn set_sensitivity(&mut self, percent: u8) {
        self.sensitivity = vibez_core::onset::TransientSensitivity::new(percent);
        self.sensitivity_input = self.sensitivity.percent().to_string();
    }

    pub fn edit_sensitivity_input(&mut self, input: String) {
        if let Some(sensitivity) = parse_transient_sensitivity(&input) {
            self.sensitivity = sensitivity;
        }
        self.sensitivity_input = input;
    }

    pub fn normalize_sensitivity_input(&mut self) {
        self.sensitivity_input = self.sensitivity.percent().to_string();
    }

    pub fn commit_sensitivity_input(&mut self) -> bool {
        let Some(sensitivity) = parse_transient_sensitivity(&self.sensitivity_input) else {
            return false;
        };
        self.set_sensitivity(sensitivity.percent());
        true
    }
}

fn parse_transient_sensitivity(input: &str) -> Option<vibez_core::onset::TransientSensitivity> {
    let percent = input
        .trim()
        .trim_end_matches('%')
        .trim()
        .parse::<u16>()
        .ok()?;
    Some(vibez_core::onset::TransientSensitivity::new(
        percent.min(u8::MAX.into()) as u8,
    ))
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
    AudioClipDetail {
        location: vibez_project::TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
        source_frame: u64,
        timeline_frame: u64,
        transient_marker: Option<u64>,
        warp_marker: Option<u64>,
    },
    TrackHeader {
        track_id: TrackId,
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
    Project,
    Plugins,
    Dropbox,
    Warping,
    Perform,
    Appearance,
    Updates,
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

/// The master bus as a track-shaped UI channel: gain plus an effect
/// chain, no clips or instrument. Lives outside `tracks` so the
/// arrangement never shows it, but `find_track` resolves it, so the
/// mixer strip, device chain, and effect commands all work on it.
pub fn new_master_track() -> ProjectTrack {
    let mut master = ProjectTrack::new(TrackId::MASTER, "Master".to_string(), 0);
    master.pan = 0.5;
    master
}

/// One musical timeline's content, associated with shared Project Tracks by
/// stable identity. Hash-map storage prevents track reordering from moving or
/// cloning timeline content.
/// Whether an editor drag is in flight, and what a trim drag began from.
#[derive(Debug, Clone, Default)]
pub enum EditDrag {
    #[default]
    Idle,
    Active,
    /// Each trim step trims from the content the drag began from, so markers
    /// the edge passes over come back when it returns.
    Trimming(Arc<TimelineContent>),
}

impl EditDrag {
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }

    /// A trim already in flight keeps its origin.
    pub fn mark_active(&mut self) {
        if matches!(self, Self::Idle) {
            *self = Self::Active;
        }
    }
}

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

    /// Audio and note clips on `tracks`, leaving out content kept for a
    /// track no longer in the list.
    pub fn clip_count(&self, tracks: impl IntoIterator<Item = TrackId>) -> usize {
        tracks
            .into_iter()
            .filter_map(|track_id| self.get(track_id))
            .map(|content| content.clips.len() + content.note_clips.len())
            .sum()
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
    /// Runtime-only Transient Marker focus in the Audio Clip editor.
    pub selected_transient_marker: Option<(TrackId, ClipId, u64)>,
    /// Runtime-only interior Warp Marker focus in the Audio Clip editor.
    pub selected_warp_marker: Option<(TrackId, ClipId, u64)>,
    // Time selection (visible brackets; independent from the loop).
    pub time_selection_active: bool,
    pub selection_start_beats: f64,
    pub selection_end_beats: f64,
    pub time_selection_track: Option<TrackId>,
    /// Live rubber-band, present only while a box drag is in flight.
    /// Every lane draws its own slice of it.
    pub marquee: Option<ArrangementMarquee>,
    /// The Clip drag in flight, present from its first step until it drops
    /// or is cancelled. Every lane draws its preview.
    pub clip_drag: Option<ClipDrag>,
    /// The move, resize or trim drag in flight, which drives edge
    /// auto-scroll on ticks.
    pub edit_drag: EditDrag,
    /// In-flight numeric Inspector edits. A missing entry means the control
    /// shows its committed Audio Clip value.
    pub audio_clip_inspector_edits: HashMap<(ClipId, AudioClipInspectorField), String>,
    /// Monotonic per-clip token used to reject superseded Transpose wheel
    /// settle tasks, including gestures that revisit an earlier value.
    pub audio_clip_transpose_debounce: HashMap<ClipId, u64>,
}

impl TimelineEditorState {
    pub fn discard_orphaned_audio_clip_inspector_edits(&mut self) {
        let existing: HashSet<ClipId> = self
            .timeline
            .by_track
            .values()
            .flat_map(|content| content.clips.iter().map(|clip| clip.id))
            .collect();
        self.audio_clip_inspector_edits
            .retain(|(clip_id, _), _| existing.contains(clip_id));
        self.audio_clip_transpose_debounce
            .retain(|clip_id, _| existing.contains(clip_id));
        if let Some((track_id, clip_id, source_frame)) = self.selected_transient_marker {
            let marker_exists = self
                .timeline
                .by_track
                .get(&track_id)
                .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id))
                .is_some_and(|clip| clip.transient_markers.contains(source_frame));
            if !marker_exists {
                self.selected_transient_marker = None;
            }
        }
        if let Some((track_id, clip_id, source_frame)) = self.selected_warp_marker {
            let marker_exists = self
                .timeline
                .by_track
                .get(&track_id)
                .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id))
                .is_some_and(|clip| {
                    clip.warp_markers
                        .interior()
                        .iter()
                        .any(|marker| marker.source_frame() == source_frame)
                });
            if !marker_exists {
                self.selected_warp_marker = None;
            }
        }
    }

    pub fn discard_audio_clip_inspector_edits(&mut self) {
        self.audio_clip_inspector_edits.clear();
        self.audio_clip_transpose_debounce.clear();
        self.selected_transient_marker = None;
        self.selected_warp_marker = None;
    }

    pub fn discard_audio_clip_inspector_edits_for(&mut self, clip_id: ClipId) {
        self.audio_clip_inspector_edits
            .retain(|(draft_clip_id, _), _| *draft_clip_id != clip_id);
        self.audio_clip_transpose_debounce.remove(&clip_id);
    }
}

impl Default for TimelineEditorState {
    fn default() -> Self {
        Self {
            timeline: Arc::new(TimelineContent::default()),
            selected_track: None,
            selected_clips: HashSet::new(),
            selected_note_clip: None,
            selected_transient_marker: None,
            selected_warp_marker: None,
            time_selection_active: false,
            selection_start_beats: 0.0,
            selection_end_beats: 0.0,
            time_selection_track: None,
            marquee: None,
            clip_drag: None,
            edit_drag: EditDrag::Idle,
            audio_clip_inspector_edits: HashMap::new(),
            audio_clip_transpose_debounce: HashMap::new(),
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
    /// Mixer channels selected together, in mixer order. A fader, pan,
    /// send, mute or solo moved on one of them moves them all.
    pub selected_channels: Vec<TrackId>,
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
    /// Smoothed audio callback work as a percentage of its buffer deadline.
    pub audio_cpu_load_percent: f32,

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
    pub about_open: bool,
    pub settings_tab: SettingsTab,
    /// Application-scoped hardware state; never serialized into a Project.
    pub audio_settings: crate::domains::audio_settings::AudioSettingsState,
    /// Runtime Audio Input arm, meter, and take buffer; never persisted or undone.
    pub audio_recording: crate::domains::audio_recording::AudioRecordingState,
    pub confirm_project_track_deletion: bool,
    /// Global autosave preference mirrored from [`crate::ui_settings::UiSettings`].
    pub auto_save_enabled: bool,
    /// Startup release check: preference, throttle, and pending notice.
    pub update_check: crate::update_check::UpdateCheckState,
    // Project domain slice: file menu, path, dirty flag, undo.
    pub project: ProjectState,
    /// Automatically detect sample BPM and warp to project tempo on
    /// import. Mirrored from `UiSettings::auto_warp_on_import`.
    pub auto_warp_on_import: bool,
    /// Minimum BPM-detect confidence required to auto-warp. Mirrored
    /// from `UiSettings::warp_confidence_threshold`.
    pub warp_confidence_threshold: f32,
    pub automation_ui: crate::domains::automation::AutomationState,
    /// Arrange lane heights, saved with the project but outside undo.
    pub track_heights: crate::domains::track_heights::TrackHeights,

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
            audio_cpu_load_percent: 0.0,
            status_text: "Ready — Add a track to get started".to_string(),
            export_progress: None,
            audio_stream_health: AudioStreamHealth::default(),
            view: ViewState::default(),
            piano_roll: PianoRollState::default(),
            perform: crate::domains::perform::PerformState::default(),
            project_tracks: Arc::new(ProjectTracksState {
                reduced_latency_monitoring: false,
                next_track_number: 1,
                ..ProjectTracksState::default()
            }),
            arrangement: ArrangementState::default(),
            clip_clipboard: ClipClipboard::default(),
            devices: crate::domains::devices::DevicesState::default(),
            settings_open: false,
            about_open: false,
            settings_tab: SettingsTab::default(),
            audio_settings: crate::domains::audio_settings::AudioSettingsState::default(),
            audio_recording: crate::domains::audio_recording::AudioRecordingState::default(),
            confirm_project_track_deletion: false,
            auto_save_enabled: true,
            update_check: crate::update_check::UpdateCheckState::default(),
            project: ProjectState::default(),
            auto_warp_on_import: false,
            warp_confidence_threshold: 0.6,
            automation_ui: crate::domains::automation::AutomationState::default(),
            track_heights: Default::default(),
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
    (0..vibez_core::track::DRUM_RACK_PAD_COUNT)
        .map(|_| UiDrumPad::default())
        .collect()
}

impl AppState {
    /// Capture the complete canonical project state shared by Undo, Capture,
    /// engine-event commits, and project replay.
    pub fn project_snapshot(&self) -> ProjectSnapshot {
        ProjectSnapshot {
            project_tracks: Arc::clone(&self.project_tracks),
            arrange_timeline: Arc::clone(&self.arrangement.timeline),
            sections: Arc::clone(&self.perform.sections),
            launcher_clips: Arc::clone(&self.perform.clips),
            bpm: self.transport.bpm,
            project_swing: self.perform.project_swing(),
            loop_enabled: self.transport.loop_enabled,
            loop_start_beats: self.transport.loop_start_beats,
            loop_end_beats: self.transport.loop_end_beats,
        }
    }

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
                self.audio_cpu_load_percent = 0.0;
            }
            AudioStreamEvent::Rebuilding => {
                self.status_text = "Rebuilding audio stream…".into();
                self.audio_stream_health = AudioStreamHealth::Rebuilding;
            }
            AudioStreamEvent::Recovered => {
                self.status_text = "Audio stream recovered".into();
                self.audio_stream_health = AudioStreamHealth::Running;
            }
            AudioStreamEvent::ConfigurationRejected(cause) => {
                self.status_text = format!(
                    "Audio configuration rejected — previous output remains active: {cause}"
                );
                self.audio_stream_health = AudioStreamHealth::Running;
            }
        }
    }

    pub fn update_audio_cpu_load(&mut self, measured_percent: Option<f32>) {
        self.audio_cpu_load_percent =
            if matches!(self.audio_stream_health, AudioStreamHealth::Running) {
                measured_percent.unwrap_or(0.0)
            } else {
                0.0
            };
    }

    pub fn position_seconds(&self) -> f64 {
        self.transport.position_samples as f64 / self.transport.sample_rate as f64
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
        self.view.viewport.geometry().pixels_per_beat()
    }

    /// Number of beats visible in a canvas of the given width.
    #[allow(dead_code)]
    pub fn visible_beats(&self, canvas_width: f32) -> f64 {
        self.view.viewport.geometry().visible_beats(canvas_width)
    }

    /// Convert a beat value to a pixel x coordinate in the viewport.
    #[allow(dead_code)]
    pub fn beat_to_x(&self, beat: f64) -> f32 {
        self.view.viewport.geometry().beat_to_x(beat)
    }

    /// Convert a pixel x coordinate in the viewport to a beat value.
    #[allow(dead_code)]
    pub fn x_to_beat(&self, x: f32) -> f64 {
        self.view.viewport.geometry().x_to_beat(x)
    }

    /// Where the song's last clip ends, in beats; zero for an empty song.
    pub fn song_beats(&self) -> f64 {
        if self.transport.bpm > 0.0 {
            self.duration_seconds() * self.transport.bpm / 60.0
        } else {
            0.0
        }
    }

    /// Total duration in beats across all tracks, with generous padding.
    pub fn total_beats(&self) -> f64 {
        let content_beats = self.song_beats();
        if content_beats > 0.0 {
            // Pad by 32 beats or 25% of content, whichever is larger
            let padding = (content_beats * 0.25).max(32.0);
            (content_beats + padding).max(64.0)
        } else {
            // Minimum 64 beats for empty projects
            64.0
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

    /// Whether any channel, bus or the master still holds this effect.
    pub fn has_effect(&self, id: vibez_core::id::EffectId) -> bool {
        let project = &self.project_tracks;
        project
            .tracks
            .iter()
            .chain(&project.buses)
            .chain(std::iter::once(&project.master))
            .any(|track| track.effects.iter().any(|effect| effect.id == id))
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
        if self.view.workspace == Workspace::Perform
            && (self.perform.layout == vibez_project::PerformLayout::Clips
                || self.perform.has_selected_timeline())
        {
            self.perform.timeline_editor()
        } else {
            &self.arrangement.editor
        }
    }

    pub fn active_timeline_editor_mut(&mut self) -> &mut TimelineEditorState {
        if self.view.workspace == Workspace::Perform
            && (self.perform.layout == vibez_project::PerformLayout::Clips
                || self.perform.has_selected_timeline())
        {
            self.perform.timeline_editor_mut()
        } else {
            &mut self.arrangement.editor
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
#[path = "audio_stream_health_tests.rs"]
mod audio_stream_health_tests;

#[cfg(test)]
#[path = "app_state_tests.rs"]
mod tests;
