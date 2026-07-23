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
        let _ = self.event_tx.push(EngineEvent::PerformancePosition(0));
    }

    pub(super) fn effective_position(&self) -> u64 {
        match self.clock_domain {
            ClockDomain::Arrange => self.transport.position(),
            ClockDomain::Perform => self.performance_position,
        }
    }
}
