//! The one channel whose native device meters the engine reports: the
//! visible device chain, while it holds a metered native device.

use std::time::Instant;

use vibez_core::id::{EffectId, TrackId};
use vibez_engine::metering::MeterValues;

use crate::domains::gain::is_native_gain;
use crate::state::{DetailPanelTab, UiEffect};

use super::*;

#[derive(Default)]
pub(super) struct DeviceMeters {
    pub channel: Option<TrackId>,
    pub generation: u64,
}

fn is_metered(effect: &UiEffect) -> bool {
    is_native_gain(effect)
}

impl App {
    pub(super) fn poll_device_meters(&mut self) {
        let wanted = (self.state.view.detail_panel_tab == DetailPanelTab::Devices)
            .then(|| self.state.active_timeline_editor().selected_track)
            .flatten()
            .filter(|track_id| {
                self.state
                    .find_track(*track_id)
                    .is_some_and(|track| track.effects.iter().any(is_metered))
            });
        if wanted != self.device_meters.channel {
            self.device_meters.channel = wanted;
            self.restart_device_metering();
        }
        self.poll_gain_devices(wanted);
    }

    /// Metering restarts under a new generation after undo or redo (which
    /// re-add every channel) and on a channel change, while each panel keeps
    /// its open controls.
    pub(super) fn restart_device_metering(&mut self) {
        self.device_meters.generation = self.device_meters.generation.wrapping_add(1);
        for panel in self.gain_devices.panels.values_mut() {
            panel.restart_metering();
        }
        self.send_command(EngineCommand::SetEffectMeterChannel {
            track_id: self.device_meters.channel,
            generation: self.device_meters.generation,
        });
    }

    pub(super) fn reset_device_meters(&mut self) {
        self.device_meters.channel = None;
        self.device_meters.generation = self.device_meters.generation.wrapping_add(1);
        self.gain_devices.panels.clear();
        self.send_command(EngineCommand::SetEffectMeterChannel {
            track_id: None,
            generation: self.device_meters.generation,
        });
    }

    pub(super) fn accept_device_meter(
        &mut self,
        track_id: TrackId,
        effect_id: EffectId,
        generation: u64,
        input: MeterValues,
        output: MeterValues,
    ) {
        if self.device_meters.channel != Some(track_id)
            || self.device_meters.generation != generation
        {
            return;
        }
        let Some(effect) = self
            .state
            .find_track(track_id)
            .and_then(|track| track.effects.iter().find(|effect| effect.id == effect_id))
        else {
            return;
        };
        if is_native_gain(effect) {
            self.gain_devices
                .panels
                .entry(effect_id)
                .or_default()
                .accept_meter(input, output, Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::ProjectTrack;
    use vibez_core::effect::EffectType;

    #[test]
    fn a_native_eq_on_the_visible_chain_is_metered() {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Vocals".into(), 2);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(id, EffectType::Eq)));
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.poll_device_meters();
        assert_eq!(app.device_meters.channel, Some(id));
    }
}
