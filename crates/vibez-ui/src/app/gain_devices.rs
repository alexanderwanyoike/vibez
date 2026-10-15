//! Connects native Gain controls and meter readings to the visible device chain.

use std::collections::HashMap;
use std::time::Instant;

use vibez_core::id::{EffectId, TrackId};
use vibez_engine::metering::MeterValues;

use crate::domains::gain::{is_native_gain, GainMsg, GainPanelState};
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
            .then(|| self.state.active_timeline_editor().selected_track)
            .flatten()
            .filter(|track_id| {
                self.state
                    .find_track(*track_id)
                    .is_some_and(|track| track.effects.iter().any(is_native_gain))
            });
        if wanted != self.gain_devices.channel {
            self.gain_devices.channel = wanted;
            self.restart_gain_metering();
        }
        let project = &self.state.project_tracks;
        self.gain_devices.panels.retain(|id, _| {
            project
                .tracks
                .iter()
                .chain(&project.buses)
                .chain(std::iter::once(&project.master))
                .any(|track| track.effects.iter().any(|effect| effect.id == *id))
        });
        if let Some(track) = wanted.and_then(|track_id| self.state.find_track(track_id)) {
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

    /// Metering restarts under a new generation after undo or redo (which
    /// re-add every channel) and on a channel change, while each panel keeps
    /// its open controls.
    pub(super) fn restart_gain_metering(&mut self) {
        self.gain_devices.generation = self.gain_devices.generation.wrapping_add(1);
        for panel in self.gain_devices.panels.values_mut() {
            panel.restart_metering();
        }
        self.send_command(EngineCommand::SetEffectMeterChannel {
            track_id: self.gain_devices.channel,
            generation: self.gain_devices.generation,
        });
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
        let is_gain = self.state.find_track(track_id).is_some_and(|track| {
            track
                .effects
                .iter()
                .any(|effect| effect.id == effect_id && is_native_gain(effect))
        });
        if let Some(panel) = self
            .gain_devices
            .panels
            .get_mut(&effect_id)
            .filter(|_| is_gain)
        {
            panel.update(message);
        }
        Task::none()
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
    fn switching_tracks_and_back_keeps_the_open_advanced_controls() {
        let (mut app, track, effect) = gain_app();
        let _ = app.update(Message::GainControl(GainMsg::ToggleAdvanced(effect)));
        let other = ProjectTrack::new(TrackId::new(), "Other".into(), 1);
        let other_id = other.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(other);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            other_id,
            EffectType::Gain,
        )));
        app.state.arrangement.selected_track = Some(other_id);
        app.poll_gain_devices();
        app.state.arrangement.selected_track = Some(track);
        app.poll_gain_devices();
        assert!(app.gain_devices.panels[&effect].advanced);
    }

    #[test]
    fn an_unchanged_edit_to_any_effect_is_not_a_project_edit() {
        let (mut app, track, _) = gain_app();
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            track,
            EffectType::Compressor,
        )));
        let compressor = app.state.find_track(track).unwrap().effects[1].clone();
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        let current = compressor.params[0];
        let past_the_limit = compressor.descriptors[0].max + 10.0;
        let _ = app.update(Message::set_effect_param(track, compressor.id, 0, current));
        assert!(!app.state.project.dirty);
        assert!(!app.state.project.history.can_undo());

        let _ = app.update(Message::set_effect_param(
            track,
            compressor.id,
            0,
            compressor.descriptors[0].max,
        ));
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        let _ = app.update(Message::set_effect_param(
            track,
            compressor.id,
            0,
            past_the_limit,
        ));
        assert!(
            !app.state.project.dirty,
            "clamps to the value it already has"
        );
        assert!(!app.state.project.history.can_undo());
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
        assert_eq!(app.gain_devices.panels[&effect].output, MeterValues::SILENT);
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
