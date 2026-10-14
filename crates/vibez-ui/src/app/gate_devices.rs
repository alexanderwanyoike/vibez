//! Routes native Gate page choices and generation-checked measured detector windows.

use super::*;
use crate::domains::gate::{is_native_gate, GateMsg, GatePanel};
use vibez_core::{
    effect::DynamicsMeter,
    id::{EffectId, TrackId},
};

impl App {
    pub(super) fn update_gate_control(&mut self, message: GateMsg) -> Task<Message> {
        let (track, effect) = message.destination();
        if self.state.find_track(track).is_some_and(|track| {
            track
                .effects
                .iter()
                .any(|candidate| candidate.id == effect && is_native_gate(candidate))
        }) {
            self.gate_panels.entry(effect).or_default().update(message);
        }
        Task::none()
    }
    pub(super) fn gate_panel(&self, effect: EffectId) -> &GatePanel {
        self.gate_panels
            .get(&effect)
            .unwrap_or(&self.empty_gate_panel)
    }
    pub(super) fn poll_gate_panels(&mut self, selected: Option<TrackId>, now: std::time::Instant) {
        let project = &self.state.project_tracks;
        self.gate_panels.retain(|id, _| {
            project
                .tracks
                .iter()
                .chain(&project.buses)
                .chain(std::iter::once(&project.master))
                .any(|track| {
                    track
                        .effects
                        .iter()
                        .any(|effect| effect.id == *id && is_native_gate(effect))
                })
        });
        if let Some(track) = selected.and_then(|track| self.state.find_track(track)) {
            for effect in track.effects.iter().filter(|effect| is_native_gate(effect)) {
                let panel = self.gate_panels.entry(effect.id).or_default();
                if effect.bypass {
                    panel.history.reset();
                } else {
                    panel.history.reset_stale(now);
                }
            }
        }
    }
    pub(super) fn accept_gate_dynamics(
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
                .find(|candidate| candidate.id == effect && is_native_gate(candidate))
        }) else {
            return;
        };
        let panel = self.gate_panels.entry(effect).or_default();
        if device.bypass {
            panel.history.reset();
        } else {
            panel.history.accept(meter, std::time::Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domains::{device_controls::DeviceControlMsg, devices::DevicesMsg, gate::GatePage},
        state::{DetailPanelTab, ProjectTrack, Workspace},
    };
    use vibez_core::effect::{EffectType, GateMeter, GatePhase};

    fn gate_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        app.state.view.workspace = Workspace::Arrange;
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        let channel = ProjectTrack::new(TrackId::new(), "Gate".into(), 3);
        let track = channel.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(channel);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            track,
            EffectType::Gate,
        )));
        let effect = app.state.find_track(track).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(track);
        app.device_meters.channel = Some(track);
        app.state.project.history = Default::default();
        (app, track, effect)
    }

    #[test]
    fn gate_phase_requires_current_generation_and_bypass_clears_real_history() {
        let (mut app, track, effect) = gate_app();
        let generation = app.device_meters.generation;
        let meter = DynamicsMeter {
            gate: Some(GateMeter {
                phase: GatePhase::Open,
                envelope_db: -12.0,
                peak_envelope_db: -10.0,
                open_threshold_db: -40.0,
                close_threshold_db: -46.0,
                hold_remaining_ms: 10.0,
                phases_seen: 4,
            }),
            current_reduction_db: [2.0, 2.0],
            peak_reduction_db: [3.0, 3.0],
            detector_peak: [0.5, 0.5],
        };
        app.accept_gate_dynamics(track, effect, generation.wrapping_sub(1), meter);
        assert!(app.gate_panel(effect).history.meter().is_none());
        app.accept_gate_dynamics(track, effect, generation, meter);
        assert_eq!(app.gate_panel(effect).history.meter(), Some(meter));
        assert_eq!(app.gate_panel(effect).history.windows().count(), 1);
        app.state.find_track_mut(track).unwrap().effects[0].bypass = true;
        app.accept_gate_dynamics(track, effect, generation, meter);
        assert!(app.gate_panel(effect).history.meter().is_none());
        assert_eq!(app.gate_panel(effect).history.windows().count(), 0);
    }

    #[test]
    fn gate_details_and_page_survive_one_sound_undo_and_removed_panels_are_pruned() {
        let (mut app, track, effect) = gate_app();
        let _ = app.update(Message::GateControl(GateMsg::Page(
            track,
            effect,
            GatePage::Detector,
        )));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::ToggleAdvanced(
            track, effect,
        )));
        let original = app.state.find_track(track).unwrap().effects[0].params[0];
        let _ = app.update(Message::set_effect_param(track, effect, 0, -27.0));
        let snapshot = app.state.project.history.pop_undo().unwrap();
        app.apply_snapshot(snapshot);
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params[0],
            original
        );
        assert_eq!(app.gate_panel(effect).page, GatePage::Detector);
        assert!(app.device_control_state(effect).advanced);
        app.state.find_track_mut(track).unwrap().effects.clear();
        app.poll_gate_panels(Some(track), std::time::Instant::now());
        assert!(!app.gate_panels.contains_key(&effect));
    }
}
