//! Compatible plan publications preserve processing history and morph output waits.

use super::*;

impl AudioEngine {
    pub(super) fn publish_compensation_plan(
        &mut self,
        mut prepared: Box<crate::routing::PreparedRouting>,
    ) {
        if !self.project_routing_published {
            debug_assert!(self.bootstrap_routing.is_none());
            self.bootstrap_routing = Some(std::mem::replace(&mut self.routing, prepared));
            self.project_routing_published = true;
            return;
        }
        {
            let previous = &mut self.routing;
            let signal_path_changed = previous.graph.nodes != prepared.graph.nodes
                || previous.graph.edges != prepared.graph.edges
                || previous.device_latencies != prepared.device_latencies;
            prepared
                .compensation
                .retain_history_from(&mut previous.compensation);
            if prepared
                .presentation
                .retain_from(&mut previous.presentation)
            {
                prepared.presentation_start = previous.presentation_start;
            }
            for (index, node) in prepared.graph.nodes.iter().enumerate() {
                if let Some(&old) = previous.node_indices.get(node) {
                    prepared.bypass_delays[index].retain_from(&mut previous.bypass_delays[old]);
                    let next = &mut prepared.nodes[index];
                    let old = &previous.nodes[old];
                    next.bypass_state = old.bypass_state;
                    next.wet_warmup = old.wet_warmup;
                    next.wet_fade_in = old.wet_fade_in;
                }
            }
            for next in &mut prepared.channel_clocks {
                if let Some(&old) = previous.channel_indices.get(&next.track) {
                    next.retain_from(&mut previous.channel_clocks[old]);
                }
            }
            for next in &mut prepared.automation_controls {
                if let Some(&old) = previous.control_indices.get(&(next.track, next.target)) {
                    next.retain_from(&mut previous.automation_controls[old]);
                }
            }
            std::mem::swap(&mut self.routing, &mut prepared);
            self.retired_routing = Some(prepared);
            self.return_retired_routing();
            if signal_path_changed && self.transport.is_playing() {
                self.output_gate.morph_after_plan_change();
            }
        }
    }

    pub(super) fn apply_compensation_transition(
        &mut self,
        output: &mut [f32],
        channels: usize,
        project_muted: bool,
    ) {
        let permit = if project_muted {
            output_gate::OutputPermit::Muted
        } else {
            output_gate::OutputPermit::Audible
        };
        self.output_gate.apply(output, channels, permit);
    }
}
