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
        use vibez_core::automation::AutomationTarget;
        use vibez_core::routing::NodeStage;
        let stage = match target {
            AutomationTarget::EffectParam { effect_id, .. }
            | AutomationTarget::PluginParam {
                effect_id: Some(effect_id),
                ..
            } => NodeStage::Effect(effect_id),
            AutomationTarget::InstrumentParam { .. }
            | AutomationTarget::PluginParam {
                effect_id: None, ..
            }
            | AutomationTarget::TrackSwingOffset => NodeStage::Source,
            _ => NodeStage::AfterFader,
        };
        self.routing.as_ref().map_or(0, |routing| {
            let input = routing
                .graph
                .index(track_id, stage)
                .map_or(0, |node| routing.compensation.node_input_latency[node]);
            self.live_path_latency(track_id).saturating_sub(input)
        })
    }

    pub(super) fn target_automation_position(
        &self,
        track_id: TrackId,
        target: vibez_core::automation::AutomationTarget,
    ) -> u64 {
        use vibez_core::automation::AutomationTarget;
        use vibez_core::routing::NodeStage;
        let stage = match target {
            AutomationTarget::EffectParam { effect_id, .. }
            | AutomationTarget::PluginParam {
                effect_id: Some(effect_id),
                ..
            } => NodeStage::Effect(effect_id),
            AutomationTarget::InstrumentParam { .. }
            | AutomationTarget::PluginParam {
                effect_id: None, ..
            }
            | AutomationTarget::TrackSwingOffset => NodeStage::Source,
            _ => NodeStage::AfterFader,
        };
        let position = self.effective_position();
        let Some(routing) = self.routing.as_ref() else {
            return position;
        };
        let Some(node) = routing.graph.index(track_id, stage) else {
            return position;
        };
        let omitted = routing.compensation.output_latency.saturating_sub(
            routing
                .compensation
                .direct_path_latency(&routing.graph, track_id),
        );
        let context = self.presentation_context(
            routing.compensation.node_input_latency[node].saturating_add(omitted),
        );
        if self.clock_domain == ClockDomain::Perform {
            context.perform
        } else {
            context.arrange
        }
    }
}
