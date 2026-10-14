//! Routes native Compressor panel choices and generation-checked dynamics diagnostics.

use super::*;
use crate::domains::compressor::{is_native_compressor, CompressorMsg, CompressorPanel};
use vibez_core::{
    effect::DynamicsMeter,
    id::{EffectId, TrackId},
};

impl App {
    pub(super) fn update_compressor_control(&mut self, message: CompressorMsg) -> Task<Message> {
        let (track, effect) = message.destination();
        if self.state.find_track(track).is_some_and(|track| {
            track
                .effects
                .iter()
                .any(|candidate| candidate.id == effect && is_native_compressor(candidate))
        }) {
            self.compressor_panels
                .entry(effect)
                .or_default()
                .update(message);
        }
        Task::none()
    }
    pub(super) fn compressor_panel(&self, effect: EffectId) -> &CompressorPanel {
        self.compressor_panels
            .get(&effect)
            .unwrap_or(&self.empty_compressor_panel)
    }
    pub(super) fn poll_compressor_panels(
        &mut self,
        selected: Option<TrackId>,
        now: std::time::Instant,
    ) {
        let project = &self.state.project_tracks;
        self.compressor_panels.retain(|id, _| {
            project
                .tracks
                .iter()
                .chain(&project.buses)
                .chain(std::iter::once(&project.master))
                .any(|track| {
                    track
                        .effects
                        .iter()
                        .any(|effect| effect.id == *id && is_native_compressor(effect))
                })
        });
        if let Some(track) = selected.and_then(|track| self.state.find_track(track)) {
            for effect in track
                .effects
                .iter()
                .filter(|effect| is_native_compressor(effect))
            {
                let panel = self.compressor_panels.entry(effect.id).or_default();
                if effect.bypass {
                    panel.reset_metering();
                } else {
                    panel.reset_stale(now);
                }
            }
        }
    }
    pub(super) fn accept_compressor_dynamics(
        &mut self,
        track: TrackId,
        effect: EffectId,
        generation: u64,
        meter: DynamicsMeter,
    ) {
        if self.device_meters.channel != Some(track) || self.device_meters.generation != generation
        {
            return;
        }
        let Some(device) = self.state.find_track(track).and_then(|track| {
            track
                .effects
                .iter()
                .find(|candidate| candidate.id == effect && is_native_compressor(candidate))
        }) else {
            return;
        };
        let panel = self.compressor_panels.entry(effect).or_default();
        if device.bypass {
            panel.reset_metering();
        } else {
            panel.accept(meter, std::time::Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domains::{
            compressor::{CompressorPage, CompressorVisual},
            device_controls::DeviceControlMsg,
            devices::DevicesMsg,
        },
        state::{DetailPanelTab, ProjectTrack, Workspace},
    };
    use vibez_core::effect::EffectType;
    fn compressor_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        app.state.view.workspace = Workspace::Arrange;
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        let channel = ProjectTrack::new(TrackId::new(), "Compressor".into(), 3);
        let track = channel.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(channel);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            track,
            EffectType::Compressor,
        )));
        let effect = app.state.find_track(track).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(track);
        app.poll_device_meters();
        app.state.project.history = Default::default();
        (app, track, effect)
    }
    #[test]
    fn dynamics_require_current_channel_generation_and_bypass_clears_them() {
        let (mut app, track, effect) = compressor_app();
        let generation = app.device_meters.generation;
        let meter = DynamicsMeter {
            current_reduction_db: [2.0, 3.0],
            peak_reduction_db: [4.0, 5.0],
            detector_peak: [0.5, 0.25],
        };
        app.accept_compressor_dynamics(track, effect, generation.wrapping_sub(1), meter);
        app.accept_compressor_dynamics(TrackId::new(), effect, generation, meter);
        assert_eq!(app.compressor_panel(effect).meter, DynamicsMeter::default());
        app.accept_compressor_dynamics(track, effect, generation, meter);
        assert_eq!(app.compressor_panel(effect).meter, meter);
        assert_eq!(
            app.compressor_panel(effect).history().collect::<Vec<_>>(),
            vec![5.0]
        );
        app.state.find_track_mut(track).unwrap().effects[0].bypass = true;
        app.accept_compressor_dynamics(track, effect, generation, meter);
        assert_eq!(app.compressor_panel(effect).meter, DynamicsMeter::default());
        assert_eq!(app.compressor_panel(effect).history().count(), 0);
    }
    #[test]
    fn advanced_pages_survive_undo_and_selection_but_removed_panels_are_pruned() {
        let (mut app, track, effect) = compressor_app();
        let _ = app.update_compressor_control(CompressorMsg::Page(
            track,
            effect,
            CompressorPage::Detector,
        ));
        let _ = app.update_compressor_control(CompressorMsg::Visual(
            track,
            effect,
            CompressorVisual::History,
        ));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::ToggleAdvanced(
            track, effect,
        )));
        let _ = app.update(Message::set_effect_param(track, effect, 0, -24.0));
        let snapshot = app.state.project.history.pop_undo().unwrap();
        app.apply_snapshot(snapshot);
        assert!(app.device_control_state(effect).advanced);
        assert_eq!(app.compressor_panel(effect).page, CompressorPage::Detector);
        assert_eq!(
            app.compressor_panel(effect).visual,
            CompressorVisual::History
        );
        app.state.arrangement.selected_track = None;
        app.poll_device_meters();
        assert_eq!(app.compressor_panel(effect).page, CompressorPage::Detector);
        app.state.find_track_mut(track).unwrap().effects.clear();
        app.poll_device_meters();
        assert!(!app.compressor_panels.contains_key(&effect));
    }
    #[test]
    fn shared_insert_readings_preserve_distinct_input_and_output() {
        let (mut app, track, effect) = compressor_app();
        let input = vibez_engine::metering::MeterValues {
            peak_l: 0.5,
            peak_r: 0.25,
            rms_l: 0.35,
            rms_r: 0.17,
        };
        let output = vibez_engine::metering::MeterValues {
            peak_l: 0.25,
            peak_r: 0.125,
            rms_l: 0.17,
            rms_r: 0.08,
        };
        app.accept_device_meter(track, effect, app.device_meters.generation, input, output);
        assert_eq!(app.device_meters.readings[&effect].input, input);
        assert_eq!(app.device_meters.readings[&effect].output, output);
    }
}
