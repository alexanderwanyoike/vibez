//! Compatible plan publications preserve processing history and morph output waits.

use super::*;

impl AudioEngine {
    pub(super) fn publish_compensation_plan(
        &mut self,
        mut prepared: Box<crate::routing::PreparedRouting>,
    ) {
        if let Some(mut previous) = self.routing.take() {
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
            self.retired_routing = Some(previous);
            self.return_retired_routing();
            if signal_path_changed && self.transport.is_playing() && !self.was_project_muted {
                // Unchanged paths keep their own history. A short blend covers
                // newly prepared paths without gating the unrelated mix.
                self.compensation_plan_fade = 64;
            }
        }
        self.routing = Some(prepared);
    }

    pub(super) fn apply_compensation_transition(
        &mut self,
        output: &mut [f32],
        channels: usize,
        project_muted: bool,
    ) {
        if project_muted && !self.was_project_muted {
            self.compensation_fade_out = 64;
            self.compensation_fade_in = 64;
        }
        self.was_project_muted = project_muted;
        if project_muted {
            self.compensation_plan_fade = 0;
        }
        for frame in output.chunks_exact_mut(channels) {
            if self.compensation_fade_out > 0 {
                let gain = self.compensation_fade_out as f32 / 64.0;
                for (channel, sample) in frame.iter_mut().enumerate() {
                    *sample = self.last_mix_frame[channel.min(1)] * gain;
                }
                self.compensation_fade_out -= 1;
            } else if !project_muted && self.compensation_fade_in > 0 {
                let gain = 1.0 - self.compensation_fade_in as f32 / 64.0;
                frame.iter_mut().for_each(|sample| *sample *= gain);
                self.compensation_fade_in -= 1;
            }
            if !project_muted && self.compensation_plan_fade > 0 {
                let blend = 1.0 - self.compensation_plan_fade as f32 / 64.0;
                for (channel, sample) in frame.iter_mut().enumerate() {
                    *sample = self.last_mix_frame[channel.min(1)] * (1.0 - blend) + *sample * blend;
                }
                self.compensation_plan_fade -= 1;
            }
            if self.compensation_fade_out == 0 && self.compensation_plan_fade == 0 {
                self.last_mix_frame = [frame[0], frame[channels.min(2) - 1]];
            }
        }
    }
}
