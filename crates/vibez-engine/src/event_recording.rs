//! Source recording coordinates and paired repeat observations.

use super::*;
use crate::note_repeat::NoteRepeatTrigger;

impl SourceRecordingPosition {
    pub(crate) fn from_trigger(
        trigger: NoteRepeatTrigger,
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
            effective_at_samples: trigger.effective_at_samples,
            canonical_at_samples: trigger.canonical_at_samples,
            section_id: section.map(|clock| clock.id),
            section_position_samples: local(trigger.effective_at_samples),
            canonical_section_position_samples: local(trigger.canonical_at_samples),
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
    pub(crate) fn repeated_pair(
        track_id: TrackId,
        trigger: NoteRepeatTrigger,
        source: SourceRecordingPosition,
        notice: RepeatNoticePosition,
    ) -> (Self, Self) {
        // Notice coordinates are supplied independently: presentation mapping
        // must never move the Source take behind its own recording boundary.
        (
            Self::SourceNoteRepeated {
                track_id,
                pitch: trigger.pitch,
                velocity: trigger.velocity,
                rate: trigger.rate,
                position: source,
            },
            Self::NoteRepeated {
                track_id,
                pitch: trigger.pitch,
                velocity: trigger.velocity,
                rate: trigger.rate,
                effective_at_samples: notice.effective_at_samples,
                canonical_at_samples: notice.canonical_at_samples,
                section_id: notice.section_id,
                section_position_samples: notice.section_position_samples,
                canonical_section_position_samples: notice.canonical_section_position_samples,
            },
        )
    }
}

#[derive(PartialEq, Eq)]
enum SourcePayload<'a> {
    Input(
        &'a TrackId,
        &'a u8,
        &'a u8,
        &'a bool,
        &'a SourceRecordingPosition,
    ),
    Repeat(
        &'a TrackId,
        &'a u8,
        &'a u8,
        &'a NoteRepeatRate,
        &'a SourceRecordingPosition,
    ),
}

fn source_payload(event: &EngineEvent) -> Option<SourcePayload<'_>> {
    // Exhaustive fields force schema changes through this projection; derived
    // equality then compares every projected value without a separate list.
    match event {
        EngineEvent::SourceNoteInput {
            track_id,
            pitch,
            velocity,
            on,
            position,
        } => Some(SourcePayload::Input(
            track_id, pitch, velocity, on, position,
        )),
        EngineEvent::SourceNoteRepeated {
            track_id,
            pitch,
            velocity,
            rate,
            position,
        } => Some(SourcePayload::Repeat(
            track_id, pitch, velocity, rate, position,
        )),
        _ => None,
    }
}

pub(super) fn source_eq(left: &EngineEvent, right: &EngineEvent) -> bool {
    let (Some(left), Some(right)) = (source_payload(left), source_payload(right)) else {
        return false;
    };
    left == right
}
