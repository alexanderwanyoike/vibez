//! Connects native Gain controls and meter readings to the visible device chain.

use std::collections::HashMap;
use std::time::Instant;

use vibez_core::id::{EffectId, TrackId};
use vibez_engine::metering::MeterValues;

use crate::domains::gain::{is_native_gain, GainAction, GainMsg, GainPanelState};
use crate::state::DetailPanelTab;

use super::*;

#[derive(Default)]
pub(super) struct GainDevices {
    pub channel: Option<TrackId>,
    pub generation: u64,
    pub empty_panel: GainPanelState,
    pub panels: HashMap<EffectId, GainPanelState>,
}

impl App {
    pub(super) fn prepare_gain_edit(&mut self, message: &Message) -> bool {
        use crate::domains::devices::DevicesMsg;
        let single;
        let (track_id, effect_id, updates) = match message {
            Message::Devices(DevicesMsg::SetEffectParam(track, effect, index, value)) => {
                single = [(*index, *value)];
                (*track, *effect, single.as_slice())
            }
            Message::Devices(DevicesMsg::SetEffectParams(track, effect, updates)) => {
                (*track, *effect, updates.as_slice())
            }
            _ => return false,
        };
        let Some(effect) = self
            .state
            .find_track(track_id)
            .and_then(|track| track.effects.iter().find(|effect| effect.id == effect_id))
            .filter(|effect| is_native_gain(effect))
        else {
            return false;
        };
        if let Some(panel) = self.gain_devices.panels.get_mut(&effect_id) {
            for &(index, _) in updates {
                panel.discard_input(index);
            }
        }
        updates
            .iter()
            .all(|&(index, value)| effect.params.get(index) == Some(&value))
    }

    pub(super) fn poll_gain_devices(&mut self) {
        let wanted = (self.state.view.detail_panel_tab == DetailPanelTab::Devices)
            .then(|| self.state.active_timeline_editor().selected_track)
            .flatten()
            .filter(|track_id| {
                self.state
                    .find_track(*track_id)
                    .is_some_and(|track| track.effects.iter().any(is_native_gain))
            });
        if wanted != self.gain_devices.channel {
            self.gain_devices.channel = wanted;
            self.gain_devices.generation = self.gain_devices.generation.wrapping_add(1);
            self.gain_devices.panels.clear();
            self.send_command(EngineCommand::SetEffectMeterChannel {
                track_id: wanted,
                generation: self.gain_devices.generation,
            });
        }
        if let Some(track) = wanted.and_then(|track_id| self.state.find_track(track_id)) {
            self.gain_devices
                .panels
                .retain(|id, _| track.effects.iter().any(|effect| effect.id == *id));
            let now = Instant::now();
            for effect in &track.effects {
                if is_native_gain(effect) {
                    self.gain_devices
                        .panels
                        .entry(effect.id)
                        .or_default()
                        .reset_stale(now);
                }
            }
        }
    }

    pub(super) fn reset_gain_devices(&mut self) {
        self.gain_devices.channel = None;
        self.gain_devices.generation = self.gain_devices.generation.wrapping_add(1);
        self.gain_devices.panels.clear();
        self.send_command(EngineCommand::SetEffectMeterChannel {
            track_id: None,
            generation: self.gain_devices.generation,
        });
    }

    pub(super) fn accept_gain_meter(
        &mut self,
        track_id: TrackId,
        effect_id: EffectId,
        generation: u64,
        input: MeterValues,
        output: MeterValues,
    ) {
        if self.gain_devices.channel != Some(track_id) || self.gain_devices.generation != generation
        {
            return;
        }
        if self.state.find_track(track_id).is_some_and(|track| {
            track
                .effects
                .iter()
                .any(|effect| effect.id == effect_id && is_native_gain(effect))
        }) {
            self.gain_devices
                .panels
                .entry(effect_id)
                .or_default()
                .accept_meter(input, output, Instant::now());
        }
    }

    pub(super) fn update_gain_control(&mut self, message: GainMsg) -> Task<Message> {
        let Some((track_id, effect_id)) = message.destination(self.gain_devices.channel) else {
            return Task::none();
        };
        let Some(effect) = self
            .state
            .find_track(track_id)
            .and_then(|track| track.effects.iter().find(|effect| effect.id == effect_id))
            .filter(|effect| is_native_gain(effect))
        else {
            return Task::none();
        };
        let Some(panel) = self.gain_devices.panels.get_mut(&effect_id) else {
            return Task::none();
        };
        match panel.update(message, &effect.params) {
            GainAction::SetParameter(index, value) => {
                self.update(Message::set_effect_param(track_id, effect_id, index, value))
            }
            GainAction::None => Task::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::ProjectTrack;
    use vibez_core::effect::EffectType;

    fn gain_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Metered".into(), 3);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            id,
            EffectType::Gain,
        )));
        let effect = app.state.find_track(id).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        app.poll_gain_devices();
        (app, id, effect)
    }

    #[test]
    fn undo_restores_values_without_closing_the_open_advanced_controls() {
        let (mut app, track, effect) = gain_app();
        let _ = app.update(Message::GainControl(GainMsg::ToggleAdvanced(effect)));
        let _ = app.update(Message::set_effect_param(track, effect, 1, 8.0));
        let _ = app.update(Message::Project(crate::domains::project::ProjectMsg::Undo));
        app.poll_gain_devices();
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params[1],
            0.0
        );
        assert!(app.gain_devices.panels[&effect].advanced);
    }

    #[test]
    fn gain_unity_clears_a_stale_input_without_dirtying_an_unchanged_device() {
        let (mut app, track, effect) = gain_app();
        let panel = app.gain_devices.panels.get_mut(&effect).unwrap();
        panel.input_text[0] = Some("invalid".into());
        panel.input_error[0] = true;
        let _ = app.update(Message::set_effect_param(track, effect, 0, 1.0));
        assert!(!app.state.project.dirty);
        assert!(!app.state.project.history.can_undo());
        let panel = &app.gain_devices.panels[&effect];
        assert!(panel.input_text[0].is_none());
        assert!(!panel.input_error[0]);
    }

    #[test]
    fn numeric_gain_entry_rejects_invalid_values_and_commits_once() {
        let (mut app, track, effect) = gain_app();
        for input in ["NaN", "oops", "100"] {
            let _ = app.update(Message::GainControl(GainMsg::SetInput(
                track,
                effect,
                0,
                input.into(),
            )));
            let _ = app.update(Message::GainControl(GainMsg::SubmitInput(track, effect, 0)));
            assert!(app.gain_devices.panels[&effect].input_error[0]);
            assert!(!app.state.project.dirty);
            assert!(!app.state.project.history.can_undo());
        }
        let _ = app.update(Message::GainControl(GainMsg::SetInput(
            track,
            effect,
            0,
            "-6".into(),
        )));
        let _ = app.update(Message::GainControl(GainMsg::SubmitInput(track, effect, 0)));
        assert!(
            (app.state.find_track(track).unwrap().effects[0].params[0] - 0.501_187_2).abs() < 1e-6
        );
        assert!(app.state.project.dirty);
        assert!(app.state.project.history.pop_undo().is_some());
        assert!(!app.state.project.history.can_undo());
        assert!(!app.gain_devices.panels[&effect].input_error[0]);
    }

    #[test]
    fn late_gain_meters_cannot_cross_selection_or_device_removal() {
        let (mut app, track, effect) = gain_app();
        let stale_generation = app.gain_devices.generation;
        let levels = MeterValues {
            peak_l: 0.5,
            peak_r: 0.25,
            rms_l: 0.3,
            rms_r: 0.15,
        };
        app.accept_gain_meter(track, effect, stale_generation, levels, levels);
        assert_eq!(app.gain_devices.panels[&effect].output, levels);
        app.state.arrangement.selected_track = None;
        app.poll_gain_devices();
        assert!(app.gain_devices.panels.is_empty());
        app.state.arrangement.selected_track = Some(track);
        app.poll_gain_devices();
        app.accept_gain_meter(track, effect, stale_generation, levels, levels);
        assert_eq!(app.gain_devices.panels[&effect].output, MeterValues::SILENT);
        app.accept_gain_meter(track, effect, app.gain_devices.generation, levels, levels);
        assert_eq!(app.gain_devices.panels[&effect].output, levels);
        let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, effect)));
        app.poll_gain_devices();
        app.accept_gain_meter(track, effect, app.gain_devices.generation, levels, levels);
        assert!(app.gain_devices.panels.is_empty());
    }
}
