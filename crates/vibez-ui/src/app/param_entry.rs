//! Connects typed device values to the parameter they edit.

use crate::domains::device_params::DeviceParams;
use crate::domains::devices::DevicesMsg;
use crate::domains::param_entry::{param_format, update, EntryAction, EntryMsg};
use crate::widgets::device_controls::entry_id;
use iced::widget::text_input;

use super::*;

impl App {
    pub(super) fn update_param_entry(&mut self, message: EntryMsg) -> Task<Message> {
        let begins = matches!(message, EntryMsg::Begin { .. });
        let device = self.param_entry.as_ref().and_then(|entry| {
            DeviceParams::find(self.state.find_track(entry.track_id)?, entry.slot)
        });
        if matches!(message, EntryMsg::Submit) && device.is_none() {
            self.param_entry = None;
            return Task::none();
        }
        let current = device
            .zip(self.param_entry.as_ref())
            .and_then(|(device, entry)| device.params.get(entry.index).copied());
        let host_rate = self.state.transport.sample_rate;
        let parse = |entry: &crate::domains::param_entry::ValueEntry| {
            let device = device?;
            let descriptor = device.descriptors.get(entry.index)?;
            param_format(device.kind, entry.index, descriptor, Some(host_rate)).parse(
                &entry.text,
                descriptor.min,
                descriptor.max,
            )
        };
        match update(&mut self.param_entry, message, parse, current) {
            EntryAction::SetParameter {
                track_id,
                slot,
                index,
                value,
            } => self.update(slot.set(track_id, index, value)),
            EntryAction::None if begins => {
                text_input::focus(entry_id()).chain(text_input::select_all(entry_id()))
            }
            EntryAction::None => Task::none(),
        }
    }

    /// An edit from anywhere else supersedes what was being typed for it.
    pub(super) fn discard_param_entry(&mut self, message: &DevicesMsg) {
        let Some(edit) = message.param_edit() else {
            return;
        };
        if self.param_entry.as_ref().is_some_and(|entry| {
            entry.slot == edit.slot && edit.updates.iter().any(|(index, _)| *index == entry.index)
        }) {
            self.param_entry = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::device_params::DeviceSlot;
    use crate::state::{DetailPanelTab, ProjectTrack};
    use vibez_core::effect::EffectType;
    use vibez_core::id::{EffectId, TrackId};

    fn app_with(effect_type: EffectType) -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Typed".into(), 3);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(id, effect_type)));
        let effect = app.state.find_track(id).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        (app, id, effect)
    }

    fn type_value(app: &mut App, track_id: TrackId, effect_id: EffectId, index: usize, text: &str) {
        let _ = app.update(Message::ParamEntry(EntryMsg::Begin {
            track_id,
            slot: DeviceSlot::Effect(effect_id),
            index,
            text: String::new(),
        }));
        let _ = app.update(Message::ParamEntry(EntryMsg::Edit(text.into())));
        let _ = app.update(Message::ParamEntry(EntryMsg::Submit));
    }

    fn param(app: &App, track_id: TrackId, index: usize) -> f32 {
        app.state.find_track(track_id).unwrap().effects[0].params[index]
    }

    #[test]
    fn a_typed_frequency_sets_the_parameter_as_one_undoable_edit() {
        let (mut app, track, effect) = app_with(EffectType::Filter);
        let cutoff = app.state.find_track(track).unwrap().effects[0]
            .descriptors
            .iter()
            .position(|descriptor| descriptor.name == "Cutoff")
            .unwrap();
        let before = param(&app, track, cutoff);
        type_value(&mut app, track, effect, cutoff, "1.2k");
        assert_eq!(param(&app, track, cutoff), 1200.0);
        assert!(app.param_entry.is_none());
        let _ = app.update(Message::Project(crate::domains::project::ProjectMsg::Undo));
        assert_eq!(param(&app, track, cutoff), before);
    }

    #[test]
    fn typed_gain_in_decibels_sets_the_stored_amplitude() {
        let (mut app, track, effect) = app_with(EffectType::Gain);
        type_value(&mut app, track, effect, vibez_dsp::gain::param::GAIN, "-6");
        let expected = vibez_core::level::db_to_amplitude(-6.0);
        assert!((param(&app, track, vibez_dsp::gain::param::GAIN) - expected).abs() < 1e-6);
    }

    #[test]
    fn typed_gain_refuses_nonsense_and_levels_out_of_range_then_commits_once() {
        let (mut app, track, effect) = app_with(EffectType::Gain);
        for text in ["NaN", "oops", "100"] {
            type_value(&mut app, track, effect, vibez_dsp::gain::param::GAIN, text);
            assert!(app.param_entry.as_ref().unwrap().invalid);
            assert!(!app.state.project.dirty);
        }
        let _ = app.update(Message::ParamEntry(EntryMsg::Edit("-6".into())));
        let _ = app.update(Message::ParamEntry(EntryMsg::Submit));
        assert!((param(&app, track, 0) - 0.501_187_2).abs() < 1e-6);
        assert!(app.state.project.history.pop_undo().is_some());
        assert!(!app.state.project.history.can_undo());
    }

    #[test]
    fn nonsense_stays_open_and_changes_nothing() {
        let (mut app, track, effect) = app_with(EffectType::Filter);
        type_value(&mut app, track, effect, 0, "warm");
        assert!(app.param_entry.as_ref().unwrap().invalid);
        assert!(!app.state.project.dirty);
    }

    #[test]
    fn an_unchanged_knob_edit_still_closes_its_entry_without_dirtying() {
        let (mut app, track, effect) = app_with(EffectType::Gain);
        let _ = app.update(Message::ParamEntry(EntryMsg::Begin {
            track_id: track,
            slot: DeviceSlot::Effect(effect),
            index: 0,
            text: "invalid".into(),
        }));
        let _ = app.update(Message::set_effect_param(track, effect, 0, 1.0));
        assert!(app.param_entry.is_none());
        assert!(!app.state.project.dirty);
    }

    #[test]
    fn moving_the_knob_closes_the_entry_for_that_parameter() {
        let (mut app, track, effect) = app_with(EffectType::Filter);
        let _ = app.update(Message::ParamEntry(EntryMsg::Begin {
            track_id: track,
            slot: DeviceSlot::Effect(effect),
            index: 0,
            text: "500".into(),
        }));
        let _ = app.update(Message::set_effect_param(track, effect, 0, 900.0));
        assert!(app.param_entry.is_none());
    }

    #[test]
    fn an_entry_for_a_removed_device_closes_on_submit() {
        let (mut app, track, effect) = app_with(EffectType::Filter);
        let _ = app.update(Message::ParamEntry(EntryMsg::Begin {
            track_id: track,
            slot: DeviceSlot::Effect(effect),
            index: 0,
            text: "500".into(),
        }));
        let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, effect)));
        let _ = app.update(Message::ParamEntry(EntryMsg::Submit));
        assert!(app.param_entry.is_none());
    }
}
