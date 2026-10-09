//! Adapt wider hardware layouts around the stereo project renderer.

use super::*;

impl AudioEngine {
    pub(super) fn render_hardware_routing(
        &mut self,
        output: &mut [f32],
        block: render_paths::MultitrackRenderBlock<'_>,
        mut capture: Option<&mut TrackOutputCapture<'_>>,
        idle: bool,
        capture_audible_only: bool,
    ) {
        let Some(plan) = self.routing.as_mut() else {
            return;
        };
        let mut stereo = std::mem::take(&mut plan.layout_output);
        let mut input = std::mem::take(&mut plan.layout_input);
        let mut selected = std::mem::take(&mut plan.layout_capture);
        let len = block.frames * 2;
        stereo[..len].fill(0.0);
        input[..len].fill(0.0);
        selected[..len].fill(0.0);
        if let Some(live) = block.live_input {
            for frame in 0..block.frames {
                for channel in 0..2 {
                    input[frame * 2 + channel] = live
                        .samples
                        .get(frame * block.channels + channel)
                        .copied()
                        .unwrap_or(0.0);
                }
            }
        }
        let mut selected_capture = capture.as_deref().map(|capture| TrackOutputCapture {
            source_track_raw: capture.source_track_raw,
            samples: &mut selected[..len],
        });
        self.render_routing_graph(
            &mut stereo[..len],
            render_paths::MultitrackRenderBlock {
                channels: 2,
                live_input: block.live_input.map(|live| LiveInputBlock {
                    target_track_raw: live.target_track_raw,
                    samples: &input[..len],
                }),
                ..block
            },
            selected_capture.as_mut(),
            idle,
            capture_audible_only,
        );
        output.fill(0.0);
        if let Some(capture) = capture.as_deref_mut() {
            capture.samples.fill(0.0);
        }
        for frame in 0..block.frames {
            for channel in 0..2 {
                output[frame * block.channels + channel] = stereo[frame * 2 + channel];
                if let Some(capture) = capture.as_deref_mut() {
                    if let Some(sample) = capture.samples.get_mut(frame * block.channels + channel)
                    {
                        *sample = selected[frame * 2 + channel];
                    }
                }
            }
        }
        if let Some(plan) = self.routing.as_mut() {
            plan.layout_output = stereo;
            plan.layout_input = input;
            plan.layout_capture = selected;
        }
    }
}
