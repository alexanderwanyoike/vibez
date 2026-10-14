//! Caller-thread construction and graph rendering for isolated offline projects.

use super::*;

pub(crate) struct OfflineRoutingSetup {
    pub sample_rate: u32,
    pub bpm: f64,
    pub swing: SwingAmount,
    pub tracks: crate::mixer::ChannelList,
    pub buses: crate::mixer::ChannelList,
    pub master: EngineTrack,
    pub routing: Box<crate::routing::PreparedRouting>,
}

impl AudioEngine {
    pub(crate) fn for_offline_routing(setup: OfflineRoutingSetup) -> Self {
        let (mut engine, _commands, _events) = Self::new();
        engine.sample_rate = setup.sample_rate;
        engine.transport.set_bpm(setup.bpm);
        engine.project_swing = setup.swing;
        engine.tracks = setup.tracks;
        engine.buses = setup.buses;
        engine.master = Box::new(setup.master);
        for channel in engine
            .tracks
            .iter_mut()
            .chain(engine.buses.iter_mut())
            .chain(std::iter::once(&mut engine.master))
        {
            channel.set_manual_mute(channel.mute, true);
        }
        engine.routing = setup.routing;
        engine.project_routing_published = true;
        engine
    }

    pub(crate) fn render_offline_routing_segment(
        &mut self,
        position: u64,
        output: &mut [f32],
        selected_output: Option<(TrackId, &mut [f32])>,
    ) {
        output.fill(0.0);
        self.validate_compensation();
        let mut capture = selected_output.map(|(track, samples)| {
            samples.fill(0.0);
            TrackOutputCapture {
                source_track_raw: track.raw(),
                samples,
            }
        });
        if !self.compensation_plan.applied_plan_is_valid() {
            return;
        }
        self.render_routing_graph(
            output,
            render_paths::MultitrackRenderBlock {
                pos: position,
                repeat_pos: position,
                frames: output.len() / 2,
                channels: 2,
                loop_region: None,
                live_input: None,
            },
            capture.as_mut(),
            false,
            false,
        );
    }

    pub(crate) fn offline_compensation_failure(&self) -> Option<String> {
        self.observe_offline_configuration().failure()
    }

    pub(crate) fn take_offline_channels(
        &mut self,
    ) -> (
        crate::mixer::ChannelList,
        crate::mixer::ChannelList,
        EngineTrack,
    ) {
        (
            std::mem::take(&mut self.tracks),
            std::mem::take(&mut self.buses),
            *std::mem::replace(
                &mut self.master,
                Box::new(EngineTrack::new(TrackId::MASTER)),
            ),
        )
    }
}
