//! Source recording coordinates and paired repeat observations.

use super::*;
use crate::note_repeat::NoteRepeatTrigger;

impl SourceRecordingPosition {
    pub(crate) fn at(
        effective_at_samples: u64,
        canonical_at_samples: u64,
        section: Option<SourceSectionClock>,
    ) -> Self {
        let local = |sample| {
            section.map(|clock| {
                crate::engine::section_record::section_sample_for_performance(
                    clock.local_sample,
                    clock.performance_sample,
                    sample,
                    clock.length,
                )
            })
        };
        Self {
            effective_at_samples,
            canonical_at_samples,
            section_id: section.map(|clock| clock.id),
            section_position_samples: local(effective_at_samples),
            canonical_section_position_samples: local(canonical_at_samples),
        }
    }
}

impl From<SourceRecordingPosition> for RepeatNoticePosition {
    fn from(position: SourceRecordingPosition) -> Self {
        Self {
            effective_at_samples: position.effective_at_samples,
            canonical_at_samples: position.canonical_at_samples,
            section_id: position.section_id,
            section_position_samples: position.section_position_samples,
            canonical_section_position_samples: position.canonical_section_position_samples,
        }
    }
}

impl EngineEvent {
    pub(crate) fn input_pair(
        track_id: TrackId,
        note: vibez_core::note_output::BlockNote,
        source: SourceRecordingPosition,
        notice: RepeatNoticePosition,
    ) -> (Self, Self) {
        (
            Self::SourceNoteInput(SourceNoteInputEvent {
                track_id,
                pitch: note.pitch,
                velocity: note.velocity,
                on: note.on,
                position: source,
            }),
            Self::InstrumentNoteInput(InstrumentNoteInputEvent {
                track_id,
                pitch: note.pitch,
                velocity: note.velocity,
                on: note.on,
                effective_at_samples: notice.effective_at_samples,
                section_id: notice.section_id,
                section_position_samples: notice.section_position_samples,
            }),
        )
    }

    pub(crate) fn repeated_pair(
        track_id: TrackId,
        trigger: NoteRepeatTrigger,
        source: SourceRecordingPosition,
        notice: RepeatNoticePosition,
    ) -> (Self, Self) {
        // Notice coordinates are supplied independently: presentation mapping
        // must never move the Source take behind its own recording boundary.
        (
            Self::SourceNoteRepeated(SourceNoteRepeatedEvent {
                track_id,
                pitch: trigger.pitch,
                velocity: trigger.velocity,
                rate: trigger.rate,
                position: source,
            }),
            Self::NoteRepeated(NoteRepeatedEvent {
                track_id,
                pitch: trigger.pitch,
                velocity: trigger.velocity,
                rate: trigger.rate,
                effective_at_samples: notice.effective_at_samples,
                canonical_at_samples: notice.canonical_at_samples,
                section_id: notice.section_id,
                section_position_samples: notice.section_position_samples,
                canonical_section_position_samples: notice.canonical_section_position_samples,
            }),
        )
    }
}
