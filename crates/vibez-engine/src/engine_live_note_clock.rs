//! Places a live note played inside the rendering block on the recording and heard clocks.

use super::*;
use crate::compensation_clock::{PresentationHistory, PresentationPosition};
use crate::events::{HeardNotePosition, SourceRecordingPosition, SourceSectionClock};

/// Live notes carry Perform samples inside the block being rendered.
#[derive(Clone, Copy)]
pub(super) struct LiveNoteClock<'a> {
    pub(super) arrange: u64,
    pub(super) local: u64,
    pub(super) perform: u64,
    pub(super) section: Option<ActiveSectionPlayback>,
    pub(super) presentation: &'a PresentationHistory,
    pub(super) omitted: u32,
    pub(super) path: u32,
    pub(super) performing: bool,
    pub(super) continuous_sample: u64,
}

impl LiveNoteClock<'_> {
    pub(super) fn source(&self, effective: u64, canonical: u64) -> SourceRecordingPosition {
        SourceRecordingPosition::at(
            effective,
            canonical,
            self.section.map(|active| SourceSectionClock {
                id: active.section_id,
                local_sample: self.local,
                performance_sample: self.perform,
                length: active.length_samples,
            }),
        )
    }

    pub(super) fn heard(&self, effective: u64, canonical: u64) -> HeardNotePosition {
        let effective_context = self.context(effective);
        let canonical_context = self.context(canonical);
        HeardNotePosition {
            effective_at_samples: self.heard_sample(effective, effective_context),
            canonical_at_samples: self.heard_sample(canonical, canonical_context),
            section_id: effective_context.and_then(|context| context.section_id),
            section_position_samples: effective_context.and_then(|context| context.section),
            canonical_section_position_samples: canonical_context
                .and_then(|context| context.section),
        }
    }

    /// When the note is heard, on the continuous output clock.
    pub(super) fn due(&self, effective: u64) -> u64 {
        self.continuous_sample
            .saturating_add(effective.saturating_sub(self.perform))
            .saturating_add(self.path as u64)
    }

    fn heard_sample(&self, sample: u64, context: Option<PresentationPosition>) -> u64 {
        context.map_or_else(
            || sample.saturating_sub(self.omitted as u64),
            |context| {
                if self.performing {
                    context.perform
                } else {
                    context.arrange
                }
            },
        )
    }

    fn context(&self, sample: u64) -> Option<PresentationPosition> {
        let relative = sample as i128 - self.perform as i128 - self.omitted as i128;
        if relative < 0 {
            return u32::try_from(-relative)
                .ok()
                .and_then(|delay| self.presentation.before_block(delay));
        }
        let delta = relative as u64;
        Some(PresentationPosition {
            arrange: self.arrange.saturating_add(delta),
            perform: self.perform.saturating_add(delta),
            section_id: self.section.map(|active| active.section_id),
            section: self.section.map(|active| {
                section_record::section_sample_for_performance(
                    self.local,
                    self.perform,
                    sample.saturating_sub(self.omitted as u64),
                    active.length_samples,
                )
            }),
            ..Default::default()
        })
    }
}
