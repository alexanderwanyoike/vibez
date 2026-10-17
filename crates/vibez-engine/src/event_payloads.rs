//! Engine-event payload values and identity-only Section ownership comparisons.

use super::*;

#[derive(Debug, PartialEq)]
pub struct SourceNoteInputEvent {
    pub track_id: TrackId,
    pub pitch: u8,
    pub velocity: u8,
    pub on: bool,
    pub position: SourceRecordingPosition,
}

#[derive(Debug, PartialEq)]
pub struct SourceNoteRepeatedEvent {
    pub track_id: TrackId,
    pub pitch: u8,
    pub velocity: u8,
    pub rate: NoteRepeatRate,
    pub position: SourceRecordingPosition,
}

#[derive(Debug, PartialEq)]
pub struct MeteringEvent {
    pub peak_l: f32,
    pub peak_r: f32,
    pub rms_l: f32,
    pub rms_r: f32,
}

#[derive(Debug, PartialEq)]
pub struct TrackMeterEvent {
    pub track_id: TrackId,
    pub peak_l: f32,
    pub peak_r: f32,
}

#[derive(Debug, PartialEq)]
pub struct TrackNoteActivityEvent {
    pub track_id: TrackId,
    pub triggered_notes: u128,
}

#[derive(Debug, PartialEq)]
pub struct TrackMuteChangedEvent {
    pub track_id: TrackId,
    pub muted: bool,
    pub effective_at_samples: u64,
}

#[derive(Debug, PartialEq)]
pub struct AutomationOverrideChangedEvent {
    pub track_id: TrackId,
    pub target: vibez_core::automation::AutomationTarget,
    pub overridden: bool,
}

#[derive(Debug, PartialEq)]
pub struct AutomationGestureChangedEvent {
    pub track_id: TrackId,
    pub target: vibez_core::automation::AutomationTarget,
    pub normalized_value: f32,
    pub phase: AutomationGesturePhase,
    pub effective_at_samples: u64,
}

#[derive(Debug, PartialEq)]
pub struct NoteRepeatedEvent {
    pub track_id: TrackId,
    pub pitch: u8,
    pub velocity: u8,
    pub rate: NoteRepeatRate,
    pub effective_at_samples: u64,
    pub canonical_at_samples: u64,
    pub section_id: Option<SectionId>,
    pub section_position_samples: Option<u64>,
    pub canonical_section_position_samples: Option<u64>,
}

#[derive(Debug, PartialEq)]
pub struct InstrumentNoteInputEvent {
    pub track_id: TrackId,
    pub pitch: u8,
    pub velocity: u8,
    pub on: bool,
    pub effective_at_samples: u64,
    pub section_id: Option<SectionId>,
    pub section_position_samples: Option<u64>,
}

#[derive(Debug, PartialEq)]
pub struct SectionRecordArmedEvent {
    pub section_id: SectionId,
    pub track_id: TrackId,
    pub effective_at_samples: u64,
    pub section_position_samples: u64,
}

#[derive(Debug, PartialEq)]
pub struct SectionRecordStartedEvent {
    pub section_id: SectionId,
    pub track_id: TrackId,
    pub effective_at_samples: u64,
    pub section_position_samples: u64,
}

#[derive(Debug, PartialEq)]
pub struct PerformanceCaptureStartedEvent {
    pub offsets: std::sync::Arc<[(TrackId, u32)]>,
    pub effective_at_samples: u64,
    pub section_id: Option<SectionId>,
    pub section_position_samples: Option<u64>,
}

#[derive(Debug, PartialEq)]
pub struct SectionPlaybackPositionEvent {
    pub section_id: SectionId,
    pub position_samples: u64,
}

#[derive(Debug, PartialEq)]
pub struct SectionRecordStoppedEvent {
    pub section_id: SectionId,
    pub track_id: TrackId,
    pub effective_at_samples: u64,
    pub section_position_samples: u64,
    pub started: bool,
    pub retired: Option<DisposalCell<PreparedSectionPlaybackSource>>,
}

#[derive(Debug, PartialEq)]
pub struct SectionQueuedEvent {
    pub section_id: SectionId,
    pub effective_at_samples: u64,
    pub retired: Option<DisposalCell<PreparedSectionPlaybackSource>>,
}

#[derive(Debug, PartialEq)]
pub struct SectionTransitionedEvent {
    pub section_id: SectionId,
    pub effective_at_samples: u64,
    pub retired: DisposalCell<PreparedSectionPlaybackSource>,
}

#[derive(Debug, PartialEq)]
pub struct SectionSourceRefreshedEvent {
    pub section_id: SectionId,
    pub applied: bool,
    pub effective_at_samples: u64,
    pub section_position_samples: Option<u64>,
    pub retired: DisposalCell<PreparedSectionPlaybackSource>,
}
