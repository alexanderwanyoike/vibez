use super::project_io::drum_rack_pads_for_save;
use super::*;
use crate::state::ProjectTrack;
use vibez_core::{
    midi::InstrumentKind,
    track::{InstrumentStateInfo, TrackInfo},
};

impl App {
    pub(super) fn project_for_offline_render(&self) -> vibez_project::Project {
        let mut project = self.project_from_state();
        attach_render_automation(&mut project);
        project
    }

    pub(super) fn track_info_from_ui(&self, track: &ProjectTrack) -> TrackInfo {
        let effects = track
            .effects
            .iter()
            .map(|effect| {
                let plugin = effect.plugin_ref.as_ref().map(|dev| {
                    let mut dev = dev.clone();
                    dev.state_b64 = self
                        .capture_device_state(PluginGuiKey::Effect {
                            track_id: track.id,
                            effect_id: effect.id,
                        })
                        .or(dev.state_b64);
                    dev
                });
                vibez_core::effect::EffectInfo {
                    inactive_sidechains: effect.inactive_sidechains.clone(),
                    sidechains: effect.sidechains.clone(),

                    id: effect.id,
                    effect_type: effect.effect_type,
                    bypass: effect.bypass,
                    params: effect.params.clone(),
                    plugin,
                }
            })
            .collect();

        let plugin_instrument = track
            .instrument_kind
            .is_none()
            .then(|| {
                track.plugin_instrument_ref.as_ref().map(|dev| {
                    let mut dev = dev.clone();
                    dev.state_b64 = self
                        .capture_device_state(PluginGuiKey::Instrument { track_id: track.id })
                        .or(dev.state_b64);
                    dev
                })
            })
            .flatten();

        let native_instrument = match track.instrument_kind {
            Some(InstrumentKind::SubtractiveSynth) => Some(InstrumentStateInfo::SubtractiveSynth {
                params: track.instrument_params.clone(),
            }),
            Some(InstrumentKind::Sampler) => Some(InstrumentStateInfo::Sampler {
                params: track.instrument_params.clone(),
                source: track.sample_source.clone(),
            }),
            Some(InstrumentKind::DrumRack) => Some(InstrumentStateInfo::DrumRack {
                pads: drum_rack_pads_for_save(&track.drum_rack_pads),
            }),
            None => None,
        };

        TrackInfo {
            id: track.id,
            name: track.name.clone(),
            gain: track.gain,
            pan: track.pan,
            mute: track.mute,
            solo: track.solo,
            audio_input_route: track.audio_input_route,
            input_monitoring: track.input_monitoring,
            swing_offset: track.swing_offset,
            effects,
            kind: track.kind,
            color_index: track.color_index,
            instrument: track.instrument_kind,
            native_instrument,
            plugin_instrument,
            automation: Vec::new(),
            sends: track.sends.clone(),
        }
    }
}

fn attach_render_automation(project: &mut vibez_project::Project) {
    for saved in &project.arrange.automation {
        if let Some(channel) = project
            .tracks
            .iter_mut()
            .chain(project.buses.iter_mut())
            .chain(project.master.iter_mut())
            .find(|channel| channel.id == saved.track_id)
        {
            channel.automation = saved.lanes.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::automation::{AutomationLane, AutomationTarget};

    #[test]
    fn offline_channels_receive_arrange_automation_including_bus_and_master() {
        let mut project = vibez_project::Project::default();
        let track = TrackInfo::new("Bass");
        let bus = TrackInfo::new("Return");
        let mut master = TrackInfo::new("Master");
        master.id = vibez_core::id::TrackId::MASTER;
        for channel in [&track, &bus, &master] {
            project
                .arrange
                .automation
                .push(vibez_project::TimelineAutomationInfo {
                    track_id: channel.id,
                    lanes: vec![AutomationLane::new(AutomationTarget::TrackGain)],
                });
        }
        project.tracks.push(track);
        project.buses.push(bus);
        project.master = Some(master);
        attach_render_automation(&mut project);
        for channel in project
            .tracks
            .iter()
            .chain(project.buses.iter())
            .chain(project.master.iter())
        {
            assert_eq!(channel.automation.len(), 1);
            assert_eq!(channel.automation[0].target, AutomationTarget::TrackGain);
        }
    }
}
