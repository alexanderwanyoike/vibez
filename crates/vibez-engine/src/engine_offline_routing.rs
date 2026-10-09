use super::*;

pub(crate) struct OfflineRoutingSetup {
    pub sample_rate: u32,
    pub bpm: f64,
    pub swing: SwingAmount,
    pub tracks: Vec<EngineTrack>,
    pub buses: Vec<EngineTrack>,
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
        engine.master = setup.master;
        for channel in engine
            .tracks
            .iter_mut()
            .chain(engine.buses.iter_mut())
            .chain(std::iter::once(&mut engine.master))
        {
            channel.set_manual_mute(channel.mute, true);
        }
        engine.routing = Some(setup.routing);
        engine
    }

    pub(crate) fn render_offline_routing_segment(
        &mut self,
        position: u64,
        output: &mut [f32],
        selected_output: Option<(TrackId, &mut [f32])>,
    ) {
        output.fill(0.0);
        let mut capture = selected_output.map(|(track, samples)| {
            samples.fill(0.0);
            TrackOutputCapture {
                source_track_raw: track.raw(),
                samples,
            }
        });
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
        for track in self
            .tracks
            .iter()
            .chain(&self.buses)
            .chain(std::iter::once(&self.master))
        {
            if track.instrument.as_ref().is_some_and(|instrument| {
                instrument.reconfiguration_requested()
                    || !instrument.processing_configuration_valid()
            }) {
                return Some(format!(
                    "Instrument on channel {} requires a valid processing configuration during Bounce",
                    track.id.raw()
                ));
            }
            for slot in &track.effects {
                if slot.effect.reconfiguration_requested()
                    || !slot.effect.processing_configuration_valid()
                {
                    return Some(format!(
                        "Effect {} on channel {} requires a valid processing configuration during Bounce",
                        slot.id.raw(),
                        track.id.raw()
                    ));
                }
            }
        }
        None
    }

    pub(crate) fn take_offline_channels(
        &mut self,
    ) -> (Vec<EngineTrack>, Vec<EngineTrack>, EngineTrack) {
        (
            std::mem::take(&mut self.tracks),
            std::mem::take(&mut self.buses),
            std::mem::replace(&mut self.master, EngineTrack::new(TrackId::MASTER)),
        )
    }
}
