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

