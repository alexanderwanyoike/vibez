//! Maintains native device control state and routes edits to project parameters.

use super::*;
use crate::domains::device_controls::{DeviceControlMsg, DeviceControlState};
use vibez_core::id::EffectId;

impl App {
    pub(super) fn update_device_control(&mut self, message: DeviceControlMsg) -> Task<Message> {
        let (track_id, effect_id) = message.destination();
        let Some(effect) = self
            .state
            .find_track(track_id)
            .and_then(|track| track.effects.iter().find(|effect| effect.id == effect_id))
            .filter(|effect| effect.plugin_ref.is_none() && effect.plugin_name.is_none())
        else {
            return Task::none();
        };
        let descriptors: Vec<_> = (0..effect.descriptors.len())
            .map(|index| crate::domains::device_controls::presentation_descriptor(effect, index))
            .collect();
        let sample_rate = self.cmd_tx.prepared_effect_sample_rate(track_id, effect_id);
        let state = self.device_controls.entry(effect_id).or_default();
        let edit = if effect.effect_type == vibez_core::effect::EffectType::Bitcrush {
            state.update_with_parser(message, &descriptors, &effect.params, |index, text| {
                if index == vibez_dsp::bitcrush::param::SR_REDUCE {
                    let rate = sample_rate.filter(|_| !effect.reconfiguration_failed)?;
                    crate::domains::bitcrush_values::parse_rate(
                        text,
                        rate,
                        *effect.params.get(index)?,
                    )
                } else {
                    crate::domains::device_controls::parse_value(text, descriptors.get(index)?)
                }
            })
        } else {
            state.update(message, &descriptors, &effect.params)
        };
        if let Some((index, value)) = edit {
            return self.update(Message::set_effect_param(track_id, effect_id, index, value));
        }
        Task::none()
    }

    pub(super) fn discard_device_entries(&mut self, message: &crate::domains::devices::DevicesMsg) {
        if let Some(edit) = message.param_edit() {
            if let Some(state) = self.device_controls.get_mut(&edit.effect_id) {
                for (index, _) in edit.updates {
                    state.discard(index);
                }
            }
        }
    }

    pub(super) fn prune_device_controls(&mut self) {
        let project = &self.state.project_tracks;
        self.device_controls.retain(|id, _| {
            project
                .tracks
                .iter()
                .chain(&project.buses)
                .chain(std::iter::once(&project.master))
                .any(|track| track.effects.iter().any(|effect| effect.id == *id))
        });
    }

    pub(super) fn discard_all_device_entries(&mut self) {
        for state in self.device_controls.values_mut() {
            state.entries.clear();
            state.errors.clear();
        }
    }

    pub(super) fn reset_device_controls(&mut self) {
        self.device_controls.clear();
    }

    pub(super) fn device_control_state(&self, effect_id: EffectId) -> &DeviceControlState {
        self.device_controls
            .get(&effect_id)
            .unwrap_or(&self.empty_device_controls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::ProjectTrack;
    use vibez_core::effect::EffectType;
    use vibez_core::id::TrackId;
    use vibez_dsp::gain::param;
    fn gain_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Gain".into(), 3);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            id,
            EffectType::Gain,
        )));
        let effect = app.state.find_track(id).unwrap().effects[0].id;
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        (app, id, effect)
    }
    #[test]
    fn numeric_entry_rejects_invalid_values_and_undo_preserves_disclosure() {
        let (mut app, track, effect) = gain_app();
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::ToggleAdvanced(
            track, effect,
        )));
        for value in ["NaN", "-200", "200"] {
            let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
                track,
                effect,
                param::TRIM[0],
                value.into(),
            )));
            let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
                track,
                effect,
                param::TRIM[0],
            )));
            assert!(!app.state.project.dirty);
            assert!(!app.state.project.history.can_undo());
        }
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
            track,
            effect,
            param::TRIM[0],
            "-6dB".into(),
        )));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
            track,
            effect,
            param::TRIM[0],
        )));
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params[param::TRIM[0]],
            -6.0
        );
        let snapshot = app.state.project.history.pop_undo().unwrap();
        app.apply_snapshot(snapshot);
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params[param::TRIM[0]],
            0.0
        );
        assert!(app.device_controls[&effect].advanced);
    }

    fn legacy_bitcrush_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        app.state.transport.sample_rate = 48_000;
        app.send_command(EngineCommand::SetSampleRate(48_000));
        let channel = ProjectTrack::new(TrackId::new(), "Bitcrush".into(), 3);
        let track = channel.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(channel);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            track,
            EffectType::Bitcrush,
        )));
        let target = &mut app.state.find_track_mut(track).unwrap().effects[0];
        let effect = target.id;
        target.params[1] = 4.49;
        (app, track, effect)
    }

    #[test]
    fn bitcrush_rate_entry_selects_the_nearest_actual_hertz_and_undo_restores_legacy_storage() {
        let (mut app, track, effect) = legacy_bitcrush_app();
        let target = &app.state.find_track(track).unwrap().effects[0];
        let before = target.params.clone();
        assert_eq!(target.descriptors[1].unit, "x");
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
            track,
            effect,
            1,
            "19.8 kHz".into(),
        )));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
            track, effect, 1,
        )));
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params[1],
            3.0,
            "16 kHz is closer to 19.8 kHz than 24 kHz"
        );
        assert!(app.device_controls[&effect].errors.is_empty());
        assert!(app.state.project.dirty);
        let snapshot = app.state.project.history.pop_undo().expect("one rate edit");
        assert!(!app.state.project.history.can_undo());
        app.apply_snapshot(snapshot);
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params,
            before
        );
    }

    #[test]
    fn bitcrush_rate_entry_waits_for_a_processor_prepared_at_the_new_host_rate() {
        let (mut app, track, effect) = legacy_bitcrush_app();
        let target = &app.state.find_track(track).unwrap().effects[0];
        let before = target.params.clone();
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        app.state.transport.sample_rate = 96_000;
        app.send_command(EngineCommand::SetSampleRate(96_000));
        assert!(!app.state.find_track(track).unwrap().effects[0].reconfiguration_failed);
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
            track,
            effect,
            1,
            "24 kHz".into(),
        )));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
            track, effect, 1,
        )));
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params,
            before
        );
        assert!(app.device_controls[&effect].errors.contains(&1));
        assert!(!app.state.project.history.can_undo());
        assert!(!app.state.project.dirty);
    }

    #[test]
    fn bitcrush_reentering_the_effective_rate_keeps_exact_fractional_legacy_storage() {
        let (mut app, track, effect) = legacy_bitcrush_app();
        let target = &app.state.find_track(track).unwrap().effects[0];
        let before = target.params.clone();
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
            track,
            effect,
            1,
            "12 kHz".into(),
        )));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
            track, effect, 1,
        )));
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params,
            before
        );
        assert!(app.device_controls[&effect].errors.is_empty());
        assert!(!app.state.project.history.can_undo());
        assert!(!app.state.project.dirty);
    }
}
