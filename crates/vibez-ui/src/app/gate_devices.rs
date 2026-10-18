//! Native Gate cards on the visible device chain and their panel state.

use crate::domains::gate::GateMsg;

use super::*;

impl App {
    pub(super) fn update_gate(&mut self, message: GateMsg) -> Task<Message> {
        self.gate_panels.entry(message.effect_id()).update(message);
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::{DetailPanelTab, ProjectTrack};
    use vibez_core::effect::{DynamicsMeter, EffectType, GateMeter, GatePhase};
    use vibez_core::id::{EffectId, TrackId};
    use vibez_engine::metering::MeterValues;

    fn gate_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Drums".into(), 1);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            id,
            EffectType::Gate,
        )));
        let effect = app.state.find_track(id).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.poll_device_meters();
        (app, id, effect)
    }

    #[test]
    fn a_visible_gate_receives_its_levels_and_decisions() {
        let (mut app, track, effect) = gate_app();
        assert_eq!(app.device_meters.channel, Some(track));
        let generation = app.device_meters.generation;
        let level = MeterValues {
            peak_l: 0.5,
            peak_r: 0.4,
            rms_l: 0.3,
            rms_r: 0.2,
        };
        app.accept_device_meter(track, effect, generation, level, level);
        let meter = DynamicsMeter {
            gate: Some(GateMeter {
                phase: GatePhase::Open,
                phases_seen: GatePhase::Open as u8,
                ..Default::default()
            }),
            ..Default::default()
        };
        app.accept_device_dynamics(track, effect, generation.wrapping_sub(1), meter);
        assert_eq!(app.gate_panels.panel(effect).phase, GatePhase::Closed);
        app.accept_device_dynamics(track, effect, generation, meter);
        let panel = app.gate_panels.panel(effect);
        assert_eq!(panel.phase, GatePhase::Open);
        assert_eq!(panel.input, level);
    }

    #[test]
    fn a_removed_gate_forgets_its_panel() {
        let (mut app, track, effect) = gate_app();
        let _ = app.update(Message::Gate(GateMsg::ToggleAdvanced(effect)));
        let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, effect)));
        app.poll_device_meters();
        assert!(!app.gate_panels.contains(effect));
    }
}
