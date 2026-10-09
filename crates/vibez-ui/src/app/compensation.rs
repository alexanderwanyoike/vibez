use super::*;
use vibez_engine::engine::reconfiguration::DeviceReconfiguration;

impl App {
    pub(super) fn compensation_signature(&self) -> crate::domains::compensation::TimingSignature {
        if self.midi_input.is_none() {
            return crate::domains::compensation::signature_for_state(&self.state);
        }
        let midi_target = self.midi_input.as_ref().and_then(|_| {
            self.state.arrangement.selected_track.filter(|id| {
                self.state
                    .find_track(*id)
                    .is_some_and(|track| track.is_playable_midi_target())
            })
        });
        crate::domains::compensation::signature_for_live_routes(&self.state, midi_target)
    }

    pub(super) fn reconfigure_device_timing(&mut self, mut device: DeviceReconfiguration) {
        if !self.cmd_tx.owns_reconfiguration(&device) {
            return;
        }
        let track_id = device.track_id();
        let effect_id = device.effect_id();
        let name = self
            .state
            .find_track(track_id)
            .map(|track| {
                effect_id
                    .and_then(|id| track.effects.iter().find(|effect| effect.id == id))
                    .map(|effect| {
                        effect
                            .plugin_name
                            .clone()
                            .unwrap_or_else(|| effect.effect_type.name().into())
                    })
                    .unwrap_or_else(|| {
                        track
                            .plugin_instrument_name
                            .clone()
                            .unwrap_or_else(|| track.name.clone())
                    })
            })
            .unwrap_or_else(|| "Removed device".into());
        if let Some(track) = self.state.find_track(track_id) {
            device.prepare_effect_storage(track.effects.len());
        }
        let reconfigured = device.reconfigure_on_main_thread();
        let reported = reconfigured.as_ref().ok().map(|_| device.latency_samples());
        if let Some(track) = self.state.find_track_mut(track_id) {
            if let Some(id) = effect_id {
                if let Some(effect) = track.effects.iter_mut().find(|effect| effect.id == id) {
                    effect.latency_samples = reported;
                    if reconfigured.is_ok() {
                        effect.external_inputs = device.external_inputs().to_vec();
                    }
                }
            } else {
                track.instrument_latency_samples = reported;
            }
        }
        let mut channels = self.sidechain_model();
        for channel in &mut channels {
            for effect in &mut channel.effects {
                for route in &mut effect.assignments {
                    route.source_name.clear();
                }
            }
        }
        let timing = self.compensation_signature();
        let generation = self.state.devices.compensation_generation.checked_add(1);
        let prepared = reconfigured.and_then(|()| {
            let model = channels.clone();
            channels = vibez_core::routing::resolve_restored(
                &channels,
                self.state.devices.last_routing.as_deref(),
            )
            .map_err(|error| format!("Routing unavailable: {error:?}"))?;
            self.retain_routing_activation(&channels, &model);
            let generation = generation.ok_or("Compensation plan generation overflow")?;
            let mut prepared = vibez_engine::routing::PreparedRouting::prepare_compensated(
                &channels,
                4096,
                &timing.reports,
                &timing.reduced_tracks,
                generation,
            )?;
            prepared.configure_automation(&timing.controls)?;
            Ok(prepared)
        });
        match prepared {
            Ok(routing) => {
                if self.state.devices.last_routing.as_ref() != Some(&channels) {
                    self.state.devices.sidechain_choices =
                        crate::domains::sidechain::input_source_choices(&channels);
                }
                self.state.devices.last_routing = Some(channels);
                self.state.devices.last_timing = Some(timing);
                self.state.devices.compensation_generation = generation.unwrap();
                self.send_command(EngineCommand::ResumeDeviceReconfiguration { device, routing });
                self.state.status_text = format!("Updated {name} processing latency");
            }
            Err(reason) => {
                let reason = format!("{name}: {reason}");
                self.state.status_text = reason.clone();
                self.send_command(EngineCommand::RejectDeviceReconfiguration { device, reason });
            }
        }
    }
}
