//! Maintains native device control state and routes edits to project parameters.

use super::*;
use crate::domains::device_controls::{DeviceControlMsg, DeviceControlState};
use vibez_core::id::EffectId;

impl App {
    pub(super) fn device_parameter_presentation(
        &self,
        track_id: vibez_core::id::TrackId,
        effect_id: EffectId,
        index: usize,
    ) -> Option<crate::domains::device_controls::ParameterPresentation> {
        let effect = self
            .state
            .find_track(track_id)?
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)?;
        crate::domains::device_controls::parameter_presentation(
            effect,
            index,
            self.cmd_tx.prepared_effect_sample_rate(track_id, effect_id),
        )
    }

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
                if index == vibez_dsp::bitcrush::param::BITS {
                    crate::domains::bitcrush_values::parse_bits(text, *effect.params.get(index)?)
                } else if index == vibez_dsp::bitcrush::param::SR_REDUCE {
                    let rate = sample_rate.filter(|_| !effect.reconfiguration_failed)?;
                    crate::domains::bitcrush_values::parse_rate(
                        text,
                        rate,
                        *effect.params.get(index)?,
                    )
                } else if [
                    vibez_dsp::bitcrush::param::MIX,
                    vibez_dsp::bitcrush::param::JITTER,
                ]
                .contains(&index)
                {
                    crate::domains::device_controls::parse_percent(text, *effect.params.get(index)?)
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

    #[test]
    fn rate_field_presentation_tracks_prepared_owners_without_changing_native_storage() {
        use crate::domains::EngineHandle;
        use vibez_dsp::bitcrush::param;

        let (mut app, track, effect) = legacy_bitcrush_app();
        let before = app.state.find_track(track).unwrap().effects[0]
            .params
            .clone();
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        let assert_known = |app: &App, value: &str, minimum: &str, maximum: &str| {
            let presentation = app
                .device_parameter_presentation(track, effect, param::SR_REDUCE)
                .expect("native parameter presentation");
            assert_eq!(presentation.name, "Nominal Rate");
            assert_eq!(presentation.unit, "Hz");
            assert_eq!(presentation.value.as_deref(), Some(value));
            assert_eq!(
                presentation
                    .range
                    .as_ref()
                    .map(|(low, high)| (low.as_str(), high.as_str())),
                Some((minimum, maximum))
            );
            assert!(presentation.is_editable());
        };
        let assert_unavailable = |app: &App| {
            let presentation = app
                .device_parameter_presentation(track, effect, param::SR_REDUCE)
                .expect("unknown context does not remove the native control");
            assert_eq!(presentation.name, "Nominal Rate");
            assert_eq!(presentation.unit, "Hz");
            assert!(presentation.value.is_none());
            assert!(presentation.range.is_none());
            assert!(!presentation.is_editable());
        };

        assert_eq!(
            app.cmd_tx.prepared_effect_sample_rate(track, effect),
            Some(48_000)
        );
        assert_known(&app, "12.000k", "1.500k", "48.000k");

        app.send_command(EngineCommand::RemoveEffect(track, effect));
        assert_eq!(app.cmd_tx.prepared_effect_sample_rate(track, effect), None);
        assert_unavailable(&app);
        app.cmd_tx
            .insert_built_in_effect_with_params(track, effect, EffectType::Bitcrush, &before);
        assert_known(&app, "12.000k", "1.500k", "48.000k");

        app.state.transport.sample_rate = 96_000;
        app.send_command(EngineCommand::SetSampleRate(96_000));
        assert_eq!(app.cmd_tx.prepared_effect_sample_rate(track, effect), None);
        assert_unavailable(&app);
        app.cmd_tx
            .insert_built_in_effect_with_params(track, effect, EffectType::Bitcrush, &before);
        assert_eq!(
            app.cmd_tx.prepared_effect_sample_rate(track, effect),
            Some(96_000)
        );
        assert_known(&app, "24.000k", "3.000k", "96.000k");

        app.state.find_track_mut(track).unwrap().effects[0].reconfiguration_failed = true;
        assert_unavailable(&app);
        app.state.find_track_mut(track).unwrap().effects[0].reconfiguration_failed = false;
        assert_known(&app, "24.000k", "3.000k", "96.000k");

        let target = &app.state.find_track(track).unwrap().effects[0];
        assert_eq!(target.params, before);
        let stored = target.descriptors[param::SR_REDUCE];
        assert_eq!((stored.unit, stored.min, stored.max), ("x", 1.0, 32.0));
        assert!(!app.state.project.history.can_undo());
        assert!(!app.state.project.dirty);

        app.state.find_track_mut(track).unwrap().effects[0].plugin_name = Some("Hosted".into());
        assert!(app
            .device_parameter_presentation(track, effect, param::SR_REDUCE)
            .is_none());
        let target = &mut app.state.find_track_mut(track).unwrap().effects[0];
        target.plugin_name = None;
        target.plugin_ref = Some(vibez_core::effect::PluginDeviceInfo {
            format: "clap".into(),
            uid: "hosted-bitcrush-fallback".into(),
            path: std::path::PathBuf::new(),
            name: "Hosted".into(),
            state_b64: None,
        });
        assert!(app
            .device_parameter_presentation(track, effect, param::SR_REDUCE)
            .is_none());
    }

    #[test]
    fn bitcrush_minimum_rate_readout_reenters_without_relaxing_the_prepared_rate_range() {
        use vibez_dsp::bitcrush::param;

        let mut app = super::super::test_support::app();
        app.state.transport.sample_rate = 44_100;
        app.send_command(EngineCommand::SetSampleRate(44_100));
        let channel = ProjectTrack::new(TrackId::new(), "Bitcrush".into(), 3);
        let track = channel.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(channel);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            track,
            EffectType::Bitcrush,
        )));
        let effect = app.state.find_track(track).unwrap().effects[0].id;
        let _ = app.update(Message::set_effect_param(
            track,
            effect,
            param::SR_REDUCE,
            32.0,
        ));
        let prepared_rate = app
            .cmd_tx
            .prepared_effect_sample_rate(track, effect)
            .expect("the native owner must be prepared at the requested rate");
        assert_eq!(prepared_rate, 44_100);
        let shown = format!("{:.3}k", f64::from(prepared_rate) / 32.0 / 1000.0);
        assert_eq!(shown, "1.378k");
        let before = app.state.find_track(track).unwrap().effects[0]
            .params
            .clone();
        app.state.project.history = Default::default();
        app.state.project.dirty = false;

        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
            track,
            effect,
            param::SR_REDUCE,
            shown,
        )));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
            track,
            effect,
            param::SR_REDUCE,
        )));
        assert!(
            app.device_controls[&effect].errors.is_empty(),
            "the canonical minimum-rate readout was rejected"
        );
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params,
            before
        );
        assert!(!app.state.project.history.can_undo());
        assert!(!app.state.project.dirty);

        for invalid in ["1.377k", "1378.001 Hz", "44.101k", "NaN", "inf"] {
            let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
                track,
                effect,
                param::SR_REDUCE,
                invalid.into(),
            )));
            let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
                track,
                effect,
                param::SR_REDUCE,
            )));
            assert!(app.device_controls[&effect]
                .errors
                .contains(&param::SR_REDUCE));
            assert_eq!(
                app.state.find_track(track).unwrap().effects[0].params,
                before,
                "{invalid} changed the sound"
            );
            assert!(!app.state.project.history.can_undo());
            assert!(!app.state.project.dirty);
        }
    }

    #[test]
    fn bitcrush_effective_bits_readout_reenters_without_changing_fractional_legacy_storage() {
        let (mut app, track, effect) = legacy_bitcrush_app();
        let target = &mut app.state.find_track_mut(track).unwrap().effects[0];
        target.params[vibez_dsp::bitcrush::param::BITS] = 8.75;
        let before = target.params.clone();
        let shown = crate::domains::device_controls::presented_value(
            target,
            vibez_dsp::bitcrush::param::BITS,
            target.params[vibez_dsp::bitcrush::param::BITS],
        );
        assert_eq!(shown, "8");
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
            track,
            effect,
            0,
            format!("{shown} bit"),
        )));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
            track, effect, 0,
        )));
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params,
            before
        );
        assert!(app.device_controls[&effect].errors.is_empty());
        assert!(!app.state.project.history.can_undo());
        assert!(!app.state.project.dirty);
    }

    #[test]
    fn bitcrush_percent_controls_preserve_rounded_noops_and_edit_with_one_undo() {
        use vibez_dsp::bitcrush::param;
        for index in [param::MIX, param::JITTER] {
            let (mut app, track, effect) = legacy_bitcrush_app();
            let target = &mut app.state.find_track_mut(track).unwrap().effects[0];
            target.params[index] = 0.373_14;
            assert_eq!(target.descriptors[index].unit, "");
            let before = target.params.clone();
            app.state.project.history = Default::default();
            app.state.project.dirty = false;
            let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
                track,
                effect,
                index,
                "37 %".into(),
            )));
            let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
                track, effect, index,
            )));
            assert_eq!(
                app.state.find_track(track).unwrap().effects[0].params,
                before
            );
            assert!(
                app.device_controls[&effect].errors.is_empty(),
                "unchanged percentage rejected"
            );
            assert!(!app.state.project.history.can_undo());
            assert!(!app.state.project.dirty);
            let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
                track,
                effect,
                index,
                "65 %".into(),
            )));
            let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
                track, effect, index,
            )));
            assert_eq!(
                app.state.find_track(track).unwrap().effects[0].params[index],
                0.65
            );
            let snapshot = app
                .state
                .project
                .history
                .pop_undo()
                .expect("one percent edit");
            assert!(!app.state.project.history.can_undo());
            app.apply_snapshot(snapshot);
            assert_eq!(
                app.state.find_track(track).unwrap().effects[0].params,
                before
            );
        }
    }

    #[test]
    fn bitcrush_nominal_rate_label_never_claims_the_target_is_the_actual_clock() {
        use vibez_dsp::bitcrush::param;

        let (mut app, track, effect) = legacy_bitcrush_app();
        for jitter in [0.0, 0.35, 1.0, 0.0] {
            app.state.find_track_mut(track).unwrap().effects[0].params[param::JITTER] = jitter;
            let presentation = app
                .device_parameter_presentation(track, effect, param::SR_REDUCE)
                .unwrap();
            assert_eq!(presentation.name, "Nominal Rate");
            assert_eq!(presentation.value.as_deref(), Some("12.000k"));
            assert_eq!(presentation.unit, "Hz");
        }
        app.send_command(EngineCommand::SetSampleRate(96_000));
        let unavailable = app
            .device_parameter_presentation(track, effect, param::SR_REDUCE)
            .unwrap();
        assert_eq!(unavailable.name, "Nominal Rate");
        assert!(!unavailable.is_editable());
    }

    #[test]
    fn bitcrush_jitter_discloses_effective_n1_inactivity_without_changing_the_stored_target() {
        use vibez_dsp::bitcrush::param;

        let (mut app, track, effect) = legacy_bitcrush_app();
        for divider in [1.0, 1.49, 1.5, 4.49] {
            let target = &mut app.state.find_track_mut(track).unwrap().effects[0];
            target.params[param::SR_REDUCE] = divider;
            target.params[param::JITTER] = 0.35;
            let before = target.params.clone();
            let presentation = app
                .device_parameter_presentation(track, effect, param::JITTER)
                .unwrap();
            assert_eq!(presentation.name, "Jitter");
            assert_eq!(presentation.value.as_deref(), Some("35"));
            assert_eq!(presentation.unit, "%");
            assert!(presentation.is_editable());
            let explanation = presentation
                .explanation
                .as_deref()
                .expect("native jitter explanation");
            if divider < 1.5 {
                assert!(explanation.contains("N1"), "{explanation}");
                assert!(explanation.contains("inactive"), "{explanation}");
            } else {
                assert!(!explanation.contains("inactive"), "{explanation}");
            }
            assert_eq!(
                app.state.find_track(track).unwrap().effects[0].params,
                before
            );
        }
    }

    #[test]
    fn bitcrush_cutoff_discloses_prepared_effective_target_cap_without_rewriting_hertz() {
        use crate::domains::EngineHandle;
        use vibez_dsp::bitcrush::param;

        let (mut app, track, effect) = legacy_bitcrush_app();
        app.state.find_track_mut(track).unwrap().effects[0].params[param::POST_LP_HZ] = 20_000.0;
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        let before = app.state.find_track(track).unwrap().effects[0]
            .params
            .clone();
        let stored = app.state.find_track(track).unwrap().effects[0].descriptors[param::POST_LP_HZ];
        let disclosure = |app: &App| {
            let presentation = app
                .device_parameter_presentation(track, effect, param::POST_LP_HZ)
                .unwrap();
            assert_eq!(presentation.name, "Cutoff");
            assert_eq!(presentation.value.as_deref(), Some("20.00k"));
            assert_eq!(presentation.unit, "Hz");
            assert!(
                presentation.is_editable(),
                "target Hz does not require prepared clock context"
            );
            presentation.explanation.expect("native cutoff explanation")
        };
        for (sample_rate, effective) in [
            (8_000, "3.920k Hz"),
            (16_000, "7.840k Hz"),
            (44_100, "20.000k Hz"),
        ] {
            app.send_command(EngineCommand::SetSampleRate(sample_rate));
            app.cmd_tx.insert_built_in_effect_with_params(
                track,
                effect,
                EffectType::Bitcrush,
                &before,
            );
            assert_eq!(
                app.cmd_tx.prepared_effect_sample_rate(track, effect),
                Some(sample_rate)
            );
            let explanation = disclosure(&app);
            assert!(explanation.contains(effective), "{explanation}");
            assert!(explanation.contains("prepared target"), "{explanation}");
            assert!(explanation.contains("0.49"), "{explanation}");
        }
        app.send_command(EngineCommand::SetSampleRate(96_000));
        assert_eq!(app.cmd_tx.prepared_effect_sample_rate(track, effect), None);
        assert!(disclosure(&app).contains("unavailable"));
        app.cmd_tx
            .insert_built_in_effect_with_params(track, effect, EffectType::Bitcrush, &before);
        assert!(disclosure(&app).contains("20.000k Hz"));
        app.state.find_track_mut(track).unwrap().effects[0].reconfiguration_failed = true;
        assert!(disclosure(&app).contains("unavailable"));
        let target = &app.state.find_track(track).unwrap().effects[0];
        assert_eq!(target.params, before);
        let descriptor = target.descriptors[param::POST_LP_HZ];
        assert_eq!(
            (
                descriptor.name,
                descriptor.unit,
                descriptor.min,
                descriptor.max
            ),
            (stored.name, stored.unit, stored.min, stored.max)
        );
        assert!(!app.state.project.history.can_undo());
        assert!(!app.state.project.dirty);
    }
}
