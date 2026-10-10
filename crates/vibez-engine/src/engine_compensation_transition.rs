//! Compatible plan publications preserve processing history and morph output waits.

use super::*;

impl AudioEngine {
    pub(super) fn publish_compensation_plan(
        &mut self,
        mut prepared: Box<crate::routing::PreparedRouting>,
    ) {
        let mut retained = false;
        if let Some(mut previous) = self.routing.take() {
            retained = previous
                .channels
                .iter()
                .map(|channel| channel.id)
                .eq(prepared.channels.iter().map(|channel| channel.id))
                && previous.graph.nodes == prepared.graph.nodes
                && previous.graph.edges == prepared.graph.edges
                && previous.device_latencies == prepared.device_latencies
                && previous.compensation.node_input_latency
                    == prepared.compensation.node_input_latency
                && previous.compensation.node_output_latency
                    == prepared.compensation.node_output_latency
                && previous.max_frames == prepared.max_frames;
            if retained {
                prepared
                    .compensation
                    .retain_history_from(&mut previous.compensation);
                std::mem::swap(&mut previous.bypass_delays, &mut prepared.bypass_delays);
                std::mem::swap(&mut previous.presentation, &mut prepared.presentation);
                std::mem::swap(
                    &mut previous.presentation_start,
                    &mut prepared.presentation_start,
                );
                std::mem::swap(&mut previous.channel_clocks, &mut prepared.channel_clocks);
                for (next, old) in prepared.nodes.iter_mut().zip(&previous.nodes) {
                    next.bypass_state = old.bypass_state;
                    next.wet_warmup = old.wet_warmup;
                    next.wet_fade_in = old.wet_fade_in;
                }
                for next in &mut prepared.automation_controls {
                    if let Some(old) = previous
                        .automation_controls
                        .iter_mut()
                        .find(|old| old.track == next.track && old.target == next.target)
                    {
                        std::mem::swap(old, next);
                    }
                }
            }
            self.retired_routing = Some(previous);
            self.return_retired_routing();
        }
        if !retained && self.transport.is_playing() {
            self.compensation_transition_frames = prepared.compensation.output_latency as u64;
            self.compensation_fade_out = 64;
            self.compensation_fade_in = 64;
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
        for frame in output.chunks_exact_mut(channels) {
            let waiting = self.compensation_transition_frames > 0;
            self.compensation_transition_frames =
                self.compensation_transition_frames.saturating_sub(1);
            if self.compensation_fade_out > 0 {
                let gain = self.compensation_fade_out as f32 / 64.0;
                for (channel, sample) in frame.iter_mut().enumerate() {
                    *sample = self.last_mix_frame[channel.min(1)] * gain;
                }
                self.compensation_fade_out -= 1;
            } else if waiting {
                frame.fill(0.0);
            } else if !project_muted && self.compensation_fade_in > 0 {
                let gain = 1.0 - self.compensation_fade_in as f32 / 64.0;
                frame.iter_mut().for_each(|sample| *sample *= gain);
                self.compensation_fade_in -= 1;
            }
            if self.compensation_fade_out == 0 {
                self.last_mix_frame = [frame[0], frame[channels.min(2) - 1]];
            }
        }
    }
}
