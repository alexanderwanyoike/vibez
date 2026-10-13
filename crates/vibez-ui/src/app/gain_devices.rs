//! Connects native Gain controls and meter readings to the visible device chain.

use std::collections::HashMap;
use std::time::Instant;

use vibez_core::effect::EffectType;
use vibez_core::id::{EffectId, TrackId};
use vibez_engine::metering::MeterValues;

use crate::domains::gain::{GainMsg, GainPanelState};
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
            Message::Devices(DevicesMsg::SetEffectParams(track, effect, updates)) =>
                (*track, *effect, updates.as_slice()),
            _ => return false,
        };
        let Some(effect) = self.state.find_track(track_id)
            .and_then(|track| track.effects.iter().find(|effect| effect.id == effect_id))
            .filter(|effect| effect.effect_type == EffectType::Gain
                && effect.plugin_ref.is_none() && effect.plugin_name.is_none()) else {
            return false;
        };
        if let Some(panel) = self.gain_devices.panels.get_mut(&effect_id) {
            for &(index, _) in updates {
                if index < panel.input_text.len() {
                    panel.input_text[index] = None;
                    panel.input_error[index] = false;
                }
            }
        }
        updates.iter().all(|&(index, value)| effect.params.get(index) == Some(&value))
    }

    pub(super) fn poll_gain_devices(&mut self) {
        let wanted = (self.state.view.detail_panel_tab == DetailPanelTab::Devices)
            .then(|| self.state.active_timeline_editor().selected_track).flatten()
            .filter(|track_id| self.state.find_track(*track_id).is_some_and(|track| {
                track.effects.iter().any(|effect| effect.effect_type == EffectType::Gain
                    && effect.plugin_ref.is_none() && effect.plugin_name.is_none())
            }));
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
            self.gain_devices.panels.retain(|id, _| track.effects.iter().any(|effect| effect.id == *id));
            let now = Instant::now();
            for effect in &track.effects {
                if effect.effect_type == EffectType::Gain && effect.plugin_ref.is_none()
                    && effect.plugin_name.is_none()
                {
                    self.gain_devices.panels.entry(effect.id).or_default().reset_stale(now);
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
        if self.gain_devices.channel != Some(track_id)
            || self.gain_devices.generation != generation
        {
            return;
        }
        if self.state.find_track(track_id).is_some_and(|track| track.effects.iter().any(|effect| {
            effect.id == effect_id && effect.effect_type == EffectType::Gain
                && effect.plugin_ref.is_none() && effect.plugin_name.is_none()
        })) {
            self.gain_devices.panels.entry(effect_id).or_default()
                .accept_meter(input, output, Instant::now());
        }
    }

    pub(super) fn update_gain_control(&mut self, message: GainMsg) -> Task<Message> {
        match message {
            GainMsg::ToggleAdvanced(effect_id) => {
                if let Some(panel) = self.gain_devices.panels.get_mut(&effect_id) {
                    panel.advanced = !panel.advanced;
                }
            }
            GainMsg::ClearClip(effect_id) => {
                if let Some(panel) = self.gain_devices.panels.get_mut(&effect_id) {
                    panel.clear_clip();
                }
            }
            GainMsg::SetInput(track_id, effect_id, index, value) if index < 3 => {
                if self.gain_devices.channel == Some(track_id) {
                    if let Some(panel) = self.gain_devices.panels.get_mut(&effect_id) {
                        panel.input_text[index] = Some(value);
                        panel.input_error[index] = false;
                    }
                }
            }
            GainMsg::SubmitInput(track_id, effect_id, index) if index < 3 => {
                let Some(panel) = self.gain_devices.panels.get_mut(&effect_id) else {
                    return Task::none();
                };
                let Some(text) = panel.input_text[index].as_deref() else {
                    return Task::none();
                };
                let Some(value) = crate::domains::gain::parse_input(index, text) else {
                    panel.input_error[index] = true;
                    return Task::none();
                };
                panel.input_text[index] = None;
                panel.input_error[index] = false;
                let current = self.state.find_track(track_id)
                    .and_then(|track| track.effects.iter().find(|effect| effect.id == effect_id))
                    .filter(|effect| effect.effect_type == EffectType::Gain && effect.plugin_ref.is_none())
                    .and_then(|effect| effect.params.get(index)).copied();
                if current.is_some_and(|current| current != value) {
                    return self.update(Message::set_effect_param(track_id, effect_id, index, value));
                }
            }
            _ => {}
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::ProjectTrack;

    fn gain_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Metered".into(), 3);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks).tracks.push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(id, EffectType::Gain)));
        let effect = app.state.find_track(id).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        app.poll_gain_devices();
        (app, id, effect)
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
}
