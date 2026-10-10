//! Playback clock-domain ownership.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClockDomain {
    Arrange,
    Perform,
}

impl AudioEngine {
    pub(super) fn begin_performance_clock(&mut self) {
        if self.clock_domain == ClockDomain::Perform {
            return;
        }
        let ending_capture = self.capture_stop_pending();
        self.cancel_presentation();
        if ending_capture {
            self.stop_heard_capture();
        }
        self.clock_domain = ClockDomain::Perform;
        self.performance_position = 0;
        if let Some(queued) = self.audition.resync_on_transport_start(
            0,
            self.transport.bpm(),
            self.sample_rate,
            audition_fade_frames(self.sample_rate),
        ) {
            let event = if queued {
                EngineEvent::AuditionQueued
            } else {
                EngineEvent::AuditionStarted
            };
            self.present_event(event);
        }
        self.present_event(EngineEvent::PerformancePosition(0));
    }

    pub(super) fn effective_position(&self) -> u64 {
        match self.clock_domain {
            ClockDomain::Arrange => self.transport.position(),
            ClockDomain::Perform => self.performance_position,
        }
    }
    pub(super) fn source_recording_position(&self) -> crate::events::SourceRecordingPosition {
        crate::events::SourceRecordingPosition {
            effective_at_samples: self.performance_position,
            canonical_at_samples: self.performance_position,
            section_id: self.active_section.map(|section| section.section_id),
            section_position_samples: self.active_section.map(|section| section.position_samples),
            canonical_section_position_samples: self
                .active_section
                .map(|section| section.position_samples),
        }
    }
}
