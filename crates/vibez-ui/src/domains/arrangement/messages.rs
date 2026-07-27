//! Arrange messages, cross-domain actions, and read-only update context.

use vibez_core::id::{ClipId, TrackId};

use crate::state::ArrangementSelection;

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
    MoveClipToTrack {
        source_track: TrackId,
        target_track: TrackId,
        clip_id: ClipId,
        is_note_clip: bool,
    },
    ToggleClipLoop(TrackId, ClipId),
    SetClipLoopRegion {
        track_id: TrackId,
        clip_id: ClipId,
        loop_start: u64,
        loop_end: u64,
    },
    SetTimeSelection {
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    },
    SetTimeSelectionActive(bool),
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
    SplitNoteClip {
        track_id: TrackId,
        clip_id: ClipId,
        split_beat: f64,
    },
    SplitSelectedAtPlayhead,
    JoinSelectedClips,
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
    ClipBpmInputChanged {
        track_id: TrackId,
        clip_id: ClipId,
        text: String,
    },
    SubmitClipBpm {
        track_id: TrackId,
        clip_id: ClipId,
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
                | Self::MoveClipToTrack { .. }
                | Self::ToggleClipLoop(..)
                | Self::SetClipLoopRegion { .. }
                | Self::SetTimeSelection { .. }
                | Self::SetTimeSelectionActive(_)
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
                | Self::SplitNoteClip { .. }
                | Self::SplitSelectedAtPlayhead
                | Self::JoinSelectedClips
                | Self::DeleteClipsInRegion { .. }
                | Self::SplitClipsAtRegion { .. }
                | Self::CreateClipFromSelection
                | Self::CreateNoteClipFromSelection(_)
                | Self::ClipBpmInputChanged { .. }
                | Self::SubmitClipBpm { .. }
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
                | ArrangementMsg::SetTimeSelectionActive(_)
                | ArrangementMsg::SetSelectionAsLoop
                | ArrangementMsg::CopySelectedClips
                | ArrangementMsg::ClipBpmInputChanged { .. }
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
}

/// Cross-domain effects requested by an arrangement update.
#[derive(Debug, Default, PartialEq)]
pub struct ArrangementAction {
    /// All plugin GUI windows and raw pointers of this track must go
    /// (the track's devices are being destroyed).
    pub close_track_guis: Option<TrackId>,
    /// Remove this shared identity from every Section timeline too.
    pub remove_track_from_sections: Option<TrackId>,
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
