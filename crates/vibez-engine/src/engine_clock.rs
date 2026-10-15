//! Playback clock-domain ownership.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClockDomain {
    Arrange,
    Perform,
}

impl AudioEngine {
    pub(super) fn begin_performance_clock(&mut self, allowance: &mut RetirementAllowance) {
        if self.clock_domain == ClockDomain::Perform {
            return;
        }
        let ending_capture = self.capture_stop_pending();
        self.cancel_presentation();
        if ending_capture {
            self.stop_heard_capture( allowance);
        }
        self.routing.presentation.rebase_performance();
        if let Some(first) = self.routing.presentation_start.as_mut() {
            first.perform = 0;
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
            self.present_event(event, allowance);
        }
        self.present_event(EngineEvent::PerformancePosition(0), allowance);
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

    pub(super) fn automation_presentation_delay(
        &self,
        track_id: TrackId,
        target: vibez_core::automation::AutomationTarget,
    ) -> u32 {
        automation_presentation_delay_fields(&self.routing, track_id, target)
    }

    pub(super) fn target_automation_position(
        &self,
        track_id: TrackId,
        target: vibez_core::automation::AutomationTarget,
    ) -> u64 {
        target_automation_position_fields(&self.routing, &self.transport, self.clock_domain, self.performance_position, self.active_section, track_id, target)
    }
}

pub(super) fn automation_presentation_delay_fields(
    routing: &crate::routing::PreparedRouting, track_id: TrackId,
    target: vibez_core::automation::AutomationTarget,
) -> u32 {
        let stage = crate::compensation_controls::automation_stage(target);
        let input = routing
            .graph
            .index(track_id, stage)
            .map_or(0, |node| routing.compensation.node_input_latency[node]);
        super::presentation::live_path_latency_fields(routing, track_id).saturating_sub(input)
    }

pub(super) fn target_automation_position_fields(
    routing: &crate::routing::PreparedRouting, transport: &Transport,
    clock_domain: ClockDomain, performance_position: u64,
    active_section: Option<ActiveSectionPlayback>, track_id: TrackId,
    target: vibez_core::automation::AutomationTarget,
) -> u64 {
        let stage = crate::compensation_controls::automation_stage(target);
        let position = match clock_domain { ClockDomain::Arrange => transport.position(), ClockDomain::Perform => performance_position };
        let Some(node) = routing.graph.index(track_id, stage) else {
            return position;
        };
        let omitted = routing
            .compensation
            .output_latency
            .saturating_sub(super::presentation::live_path_latency_fields(routing, track_id));
        let context = super::presentation::presentation_context_fields(
            routing, transport, performance_position, active_section,
            routing.compensation.node_input_latency[node].saturating_add(omitted),
        );
        if clock_domain == ClockDomain::Perform {
            context.perform
        } else {
            context.arrange
        }
    }
