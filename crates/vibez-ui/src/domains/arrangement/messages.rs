//! Arrange messages, cross-domain actions, and read-only update context.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::{ClipTranspose, MediaSourceRef};

use crate::state::{ArrangementSelection, AudioClipInspectorField, AudioClipRotaryField};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSliceMarkers {
    Transients,
    Warp,
}

#[derive(Debug, Clone)]
pub struct ClipTransposeRenderRequest {
    pub track_id: TrackId,
    pub clip_id: ClipId,
    pub source_audio: Arc<DecodedAudio>,
    pub target_frames: usize,
    pub transpose: ClipTranspose,
    pub expected_warped: bool,
    /// Audio buffer the clip still held when this render was requested.
    pub expected_audio: Arc<DecodedAudio>,
    /// Geometry the render was calculated from. `None` means geometry is not
    /// replaced by this render and therefore does not make the result stale.
    pub expected_geometry: Option<ClipRenderedGeometry>,
    pub geometry: Option<ClipRenderedGeometry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipRenderedGeometry {
    pub source_offset: u64,
    pub start_marker: u64,
    pub duration: u64,
    pub loop_start: u64,
    pub loop_end: u64,
}

impl PartialEq for ClipTransposeRenderRequest {
    fn eq(&self, other: &Self) -> bool {
        self.track_id == other.track_id
            && self.clip_id == other.clip_id
            && self.target_frames == other.target_frames
            && self.transpose == other.transpose
            && self.expected_warped == other.expected_warped
            && Arc::ptr_eq(&self.expected_audio, &other.expected_audio)
            && self.expected_geometry == other.expected_geometry
            && self.geometry == other.geometry
            && Arc::ptr_eq(&self.source_audio, &other.source_audio)
    }
}

/// Messages the arrangement domain handles (track tranche).
#[derive(Debug, Clone)]
pub enum ArrangementMsg {
    AddTrack,
    AddMidiTrack,
    AddInstrumentTrack,
    RequestRemoveTrack(TrackId),
    CancelRemoveTrack,
    ConfirmRemoveTrack(TrackId),
    /// Delete immediately without opening the optional confirmation UI.
    RemoveTrack(TrackId),
    SelectTrack(TrackId),
    RenameTrack(TrackId, String),
    RenameClip(TrackId, ClipId, String),
    MoveTrackUp(TrackId),
    MoveTrackDown(TrackId),
    MoveSelectedTrackUp,
    MoveSelectedTrackDown,
    SetTrackGain(TrackId, f32),
    SetTrackPan(TrackId, f32),
    SetTrackMute(TrackId),
    SetTrackSolo(TrackId),
    /// Add a return bus (mixer-only channel).
    AddBus,
    /// Remove a bus and every send pointing at it.
    RemoveBus(TrackId),
    /// Set a track's post-fader send amount into a bus.
    SetSend {
        track_id: TrackId,
        bus_id: TrackId,
        amount: f32,
    },
    EngineTrackMeter {
        track_id: TrackId,
        peak_l: f32,
        peak_r: f32,
    },
    // ── Clip tranche ──
    RemoveClip(TrackId, ClipId),
    SelectArrangementClip {
        selection: ArrangementSelection,
        shift_held: bool,
    },
    MoveAudioClip {
        track_id: TrackId,
        clip_id: ClipId,
        new_position: u64,
    },
    MoveNoteClipPosition {
        track_id: TrackId,
        clip_id: ClipId,
        new_position_beats: f64,
    },
    ResizeAudioClip {
        track_id: TrackId,
        clip_id: ClipId,
        new_duration: u64,
    },
    SetAudioClipFade {
        track_id: TrackId,
        clip_id: ClipId,
        edge: crate::state::AudioClipFadeEdge,
        frames: u64,
    },
    MoveClipToTrack {
        source_track: TrackId,
        target_track: TrackId,
        clip_id: ClipId,
        is_note_clip: bool,
    },
    ToggleClipLoop(TrackId, ClipId),
    ToggleClipReverse(TrackId, ClipId),
    SelectTransientMarker {
        track_id: TrackId,
        clip_id: ClipId,
        source_frame: Option<u64>,
    },
    AddTransientMarker {
        track_id: TrackId,
        clip_id: ClipId,
        source_frame: u64,
    },
    MoveTransientMarker {
        track_id: TrackId,
        clip_id: ClipId,
        from: u64,
        to: u64,
    },
    RemoveTransientMarker {
        track_id: TrackId,
        clip_id: ClipId,
        source_frame: u64,
    },
    ReplaceDetectedTransientMarkers {
        track_id: TrackId,
        clip_id: ClipId,
        source_frames: Vec<u64>,
    },
    SelectWarpMarker {
        track_id: TrackId,
        clip_id: ClipId,
        source_frame: Option<u64>,
    },
    AddWarpMarker {
        track_id: TrackId,
        clip_id: ClipId,
        source_frame: u64,
        timeline_frame: u64,
    },
    MoveWarpMarker {
        track_id: TrackId,
        clip_id: ClipId,
        source_frame: u64,
        timeline_frame: u64,
    },
    RemoveWarpMarker {
        track_id: TrackId,
        clip_id: ClipId,
        source_frame: u64,
    },
    SetClipLoopRegion {
        track_id: TrackId,
        clip_id: ClipId,
        loop_start: u64,
        loop_end: u64,
    },
    SetClipStartMarker {
        track_id: TrackId,
        clip_id: ClipId,
        start_marker: u64,
    },
    SetTimeSelection {
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    },
    /// Select every clip on every track of this timeline.
    SelectAllClips,
    SetTimeSelectionActive(bool),
    /// Live rubber-band update. `track_ids` are the lanes the box spans,
    /// resolved by the widget from real row geometry. Carries the time
    /// selection too, so one drag event stays one message.
    MarqueeSelect {
        anchor_track: TrackId,
        start_beats: f64,
        end_beats: f64,
        top_y: f32,
        bottom_y: f32,
        track_ids: Vec<TrackId>,
        additive: bool,
    },
    /// Rubber-band drag finished: drop the box, keep the selection.
    EndMarqueeSelect,
    SetSelectionAsLoop,
    DeleteSelectedClip,
    DuplicateSelectedClip,
    CopySelectedClips,
    CutSelectedClips,
    PasteClips,
    ToggleSelectedClipLoop,
    ResizeSelectedClips {
        anchor: ArrangementSelection,
        new_duration_beats: f64,
    },
    DuplicateNoteClip(TrackId, ClipId),
    SplitAudioClip {
        track_id: TrackId,
        clip_id: ClipId,
        split_position: u64,
    },
    SliceAudioClipAtMarkers {
        track_id: TrackId,
        clip_id: ClipId,
        markers: AudioSliceMarkers,
    },
    RequestSliceAudioClipToDrumRack {
        track_id: TrackId,
        clip_id: ClipId,
    },
    SliceAudioClipToDrumRack {
        track_id: TrackId,
        clip_id: ClipId,
        source: MediaSourceRef,
        audio: Arc<DecodedAudio>,
    },
    SplitNoteClip {
        track_id: TrackId,
        clip_id: ClipId,
        split_beat: f64,
    },
    SplitSelectedAtPlayhead,
    JoinSelectedClips,
    CrossfadeSelectedAudioClips,
    /// Replace selected clips with the portions where their track's captured
    /// Track Mute automation is off.
    TrimSelectedByTrackMutes,
    DeleteClipsInRegion {
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    },
    SplitClipsAtRegion {
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    },
    CreateClipFromSelection,
    CreateNoteClipFromSelection(TrackId),
    AudioClipInspectorInputChanged {
        clip_id: ClipId,
        field: AudioClipInspectorField,
        text: String,
    },
    DiscardAudioClipInspectorEdit {
        clip_id: ClipId,
        field: AudioClipInspectorField,
    },
    SubmitAudioClipInspectorField {
        track_id: TrackId,
        clip_id: ClipId,
        field: AudioClipInspectorField,
    },
    SetAudioClipRotaryValue {
        track_id: TrackId,
        clip_id: ClipId,
        field: AudioClipRotaryField,
        value: f32,
    },
    /// Update a rotary readout and schedule one Transpose commit after wheel
    /// input settles. Gain never uses this path because it is real-time safe.
    PreviewAudioClipRotaryValue {
        track_id: TrackId,
        clip_id: ClipId,
        field: AudioClipRotaryField,
        value: f32,
    },
    SetClipNominalBpm {
        track_id: TrackId,
        clip_id: ClipId,
        bpm: f64,
    },
    ClearClipWarp {
        track_id: TrackId,
        clip_id: ClipId,
    },
}

impl ArrangementMsg {
    pub(crate) fn is_timeline_editor_message(&self) -> bool {
        matches!(
            self,
            Self::RenameClip(..)
                | Self::RemoveClip(..)
                | Self::SelectArrangementClip { .. }
                | Self::MoveAudioClip { .. }
                | Self::MoveNoteClipPosition { .. }
                | Self::ResizeAudioClip { .. }
                | Self::SetAudioClipFade { .. }
                | Self::MoveClipToTrack { .. }
                | Self::ToggleClipLoop(..)
                | Self::ToggleClipReverse(..)
                | Self::SelectTransientMarker { .. }
                | Self::AddTransientMarker { .. }
                | Self::MoveTransientMarker { .. }
                | Self::RemoveTransientMarker { .. }
                | Self::ReplaceDetectedTransientMarkers { .. }
                | Self::SelectWarpMarker { .. }
                | Self::AddWarpMarker { .. }
                | Self::MoveWarpMarker { .. }
                | Self::RemoveWarpMarker { .. }
                | Self::SetClipLoopRegion { .. }
                | Self::SetClipStartMarker { .. }
                | Self::SetTimeSelection { .. }
                | Self::SelectAllClips
                | Self::SetTimeSelectionActive(_)
                | Self::MarqueeSelect { .. }
                | Self::EndMarqueeSelect
                | Self::SetSelectionAsLoop
                | Self::DeleteSelectedClip
                | Self::DuplicateSelectedClip
                | Self::CopySelectedClips
                | Self::CutSelectedClips
                | Self::PasteClips
                | Self::ToggleSelectedClipLoop
                | Self::ResizeSelectedClips { .. }
                | Self::DuplicateNoteClip(..)
                | Self::SplitAudioClip { .. }
                | Self::SliceAudioClipAtMarkers { .. }
                | Self::RequestSliceAudioClipToDrumRack { .. }
                | Self::SliceAudioClipToDrumRack { .. }
                | Self::SplitNoteClip { .. }
                | Self::SplitSelectedAtPlayhead
                | Self::JoinSelectedClips
                | Self::CrossfadeSelectedAudioClips
                | Self::TrimSelectedByTrackMutes
                | Self::DeleteClipsInRegion { .. }
                | Self::SplitClipsAtRegion { .. }
                | Self::CreateClipFromSelection
                | Self::CreateNoteClipFromSelection(_)
                | Self::AudioClipInspectorInputChanged { .. }
                | Self::DiscardAudioClipInspectorEdit { .. }
                | Self::SubmitAudioClipInspectorField { .. }
                | Self::SetAudioClipRotaryValue { .. }
                | Self::PreviewAudioClipRotaryValue { .. }
                | Self::SetClipNominalBpm { .. }
                | Self::ClearClipWarp { .. }
        )
    }

    /// Whether this message edits the project (drives the dirty flag).
    pub fn marks_dirty(&self) -> bool {
        !matches!(
            self,
            ArrangementMsg::SelectTrack(_)
                | ArrangementMsg::RequestRemoveTrack(_)
                | ArrangementMsg::CancelRemoveTrack
                | ArrangementMsg::EngineTrackMeter { .. }
                | ArrangementMsg::SelectArrangementClip { .. }
                | ArrangementMsg::SetTimeSelection { .. }
                | ArrangementMsg::SelectAllClips
                | ArrangementMsg::SetTimeSelectionActive(_)
                | ArrangementMsg::MarqueeSelect { .. }
                | ArrangementMsg::EndMarqueeSelect
                | ArrangementMsg::SetSelectionAsLoop
                | ArrangementMsg::CopySelectedClips
                | ArrangementMsg::AudioClipInspectorInputChanged { .. }
                | ArrangementMsg::DiscardAudioClipInspectorEdit { .. }
                | ArrangementMsg::PreviewAudioClipRotaryValue { .. }
                | ArrangementMsg::SelectTransientMarker { .. }
                | ArrangementMsg::SelectWarpMarker { .. }
                | ArrangementMsg::RequestSliceAudioClipToDrumRack { .. }
        )
    }

    pub(crate) const fn is_clipboard_message(&self) -> bool {
        matches!(
            self,
            Self::CopySelectedClips | Self::CutSelectedClips | Self::PasteClips
        )
    }

    pub(crate) const fn is_clipboard_project_edit(&self) -> bool {
        matches!(self, Self::CutSelectedClips | Self::PasteClips)
    }

    /// Edits whose domain result decides whether canonical state changed.
    /// The app must defer its snapshot and dirty flag until after `update`.
    pub(crate) const fn defers_project_edit(&self) -> bool {
        self.is_clipboard_project_edit()
            || matches!(
                self,
                Self::SubmitAudioClipInspectorField { .. }
                    | Self::SetAudioClipFade { .. }
                    | Self::AddTransientMarker { .. }
                    | Self::MoveTransientMarker { .. }
                    | Self::RemoveTransientMarker { .. }
                    | Self::ReplaceDetectedTransientMarkers { .. }
                    | Self::AddWarpMarker { .. }
                    | Self::MoveWarpMarker { .. }
                    | Self::RemoveWarpMarker { .. }
                    | Self::SliceAudioClipAtMarkers { .. }
                    | Self::SliceAudioClipToDrumRack { .. }
            )
    }
}

/// Cross-domain effects requested by an arrangement update.
#[derive(Debug, Default, PartialEq)]
pub struct ArrangementAction {
    /// All plugin GUI windows and raw pointers of this track must go
    /// (the track's devices are being destroyed).
    pub close_track_guis: Option<TrackId>,
    /// Remove this shared identity from every Section timeline too.
    pub remove_track_from_sections: Option<TrackId>,
    /// Seed one newly-created Project Track and its Arrange content through
    /// the canonical replay path after all project stores have been updated.
    pub replay_project_track: Option<TrackId>,
    /// Status bar text.
    pub status: Option<String>,
    /// Selecting a clip focuses the detail panel's Clip tab.
    pub focus_clip_tab: bool,
    /// A time selection was promoted to the transport loop region.
    pub loop_from_selection: Option<(f64, f64)>,
    /// A drag moved a clip near the view edge; auto-scroll to it.
    pub scroll_to_beat: Option<f64>,
    /// The project content changed outside the undo-snapshot path.
    pub mark_dirty: bool,
    /// Duration-preserving pitch render requested by a committed Transpose.
    pub transpose_render: Option<ClipTransposeRenderRequest>,
    /// Transpose wheel preview to commit after input has settled.
    pub transpose_debounce: Option<(TrackId, ClipId, i8, u64)>,
    /// A changed source tempo must immediately rebuild an already-warped Clip
    /// against the current Project tempo.
    pub warp_refresh: Option<(TrackId, ClipId)>,
}

/// Read-only cross-domain facts for arrangement updates.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArrangementCtx {
    /// Samples per beat at the current tempo (clip drag snapping).
    pub samples_per_beat: f64,
    /// Playhead position in samples (split-at-playhead).
    pub playhead_samples: u64,
    /// Playhead position in beats.
    pub playhead_beats: f64,
}
