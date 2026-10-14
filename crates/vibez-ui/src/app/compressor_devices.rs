//! Native Compressor cards on the visible device chain: their panel state
//! and the gain reduction the engine measures for them.

use std::collections::HashMap;
use std::time::Instant;

use vibez_core::effect::DynamicsMeter;
use vibez_core::id::{EffectId, TrackId};

use crate::domains::compressor::{is_native_compressor, CompressorMsg, CompressorPanelState};

use super::*;

#[derive(Default)]
pub(super) struct CompressorDevices {
    pub panels: HashMap<EffectId, CompressorPanelState>,
    pub empty_panel: CompressorPanelState,
}

impl CompressorDevices {
    pub fn panel(&self, effect_id: EffectId) -> &CompressorPanelState {
        self.panels.get(&effect_id).unwrap_or(&self.empty_panel)
    }
}

impl App {
    pub(super) fn update_compressor(&mut self, message: CompressorMsg) -> Task<Message> {
        self.compressor_devices
            .panels
            .entry(message.effect_id())
            .or_default()
            .update(message);
        Task::none()
    }

    pub(super) fn poll_compressor_devices(&mut self) {
        let state = &self.state;
        self.compressor_devices
            .panels
            .retain(|id, _| state.has_effect(*id));
        let now = Instant::now();
        for panel in self.compressor_devices.panels.values_mut() {
            panel.reset_stale(now);
        }
    }

    pub(super) fn accept_device_dynamics(
        &mut self,
        track_id: TrackId,
        effect_id: EffectId,
        generation: u64,
        meter: DynamicsMeter,
    ) {
        if self.device_meters.channel != Some(track_id)
            || self.device_meters.generation != generation
        {
            return;
        }
        let is_compressor = self.state.find_track(track_id).is_some_and(|track| {
            track
                .effects
                .iter()
                .any(|effect| effect.id == effect_id && is_native_compressor(effect))
        });
        if is_compressor {
            self.compressor_devices
                .panels
                .entry(effect_id)
                .or_default()
                .accept_dynamics(meter, Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::{DetailPanelTab, ProjectTrack};
    use vibez_core::effect::EffectType;
    use vibez_engine::metering::MeterValues;

    fn compressor_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Drums".into(), 1);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            id,
            EffectType::Compressor,
        )));
        let effect = app.state.find_track(id).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.poll_device_meters();
        (app, id, effect)
    }

    #[test]
    fn a_visible_compressor_receives_its_levels_and_reduction() {
        let (mut app, track, effect) = compressor_app();
        assert_eq!(app.device_meters.channel, Some(track));
        let generation = app.device_meters.generation;
        let level = MeterValues {
            peak_l: 0.5,
            peak_r: 0.5,
            rms_l: 0.3,
            rms_r: 0.3,
        };
        app.accept_device_meter(track, effect, generation, level, level);
        let meter = DynamicsMeter {
            peak_reduction_db: [3.0, 2.0],
            ..Default::default()
        };
        app.accept_device_dynamics(track, effect, generation.wrapping_sub(1), meter);
        assert_eq!(app.compressor_devices.panel(effect).reduction_db, 0.0);
        app.accept_device_dynamics(track, effect, generation, meter);
        let panel = app.compressor_devices.panel(effect);
        assert_eq!(panel.reduction_db, 3.0);
        assert_eq!(panel.input, level);
    }

    #[test]
    fn a_removed_compressor_forgets_its_panel() {
        let (mut app, track, effect) = compressor_app();
        let _ = app.update(Message::Compressor(CompressorMsg::ToggleAdvanced(effect)));
        let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, effect)));
        app.poll_compressor_devices();
        assert!(!app.compressor_devices.panels.contains_key(&effect));
    }
}
