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
            let _ = self.event_tx.push(event);
        }
        let _ = self.event_tx.push(EngineEvent::PerformancePosition(0));
    }

    pub(super) fn effective_position(&self) -> u64 {
        match self.clock_domain {
            ClockDomain::Arrange => self.transport.position(),
            ClockDomain::Perform => self.performance_position,
        }
    }
}
