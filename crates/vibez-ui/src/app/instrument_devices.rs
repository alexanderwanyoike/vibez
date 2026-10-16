//! Native instruments on the device chain: their card's tabs, and their
//! controls, typed values and presets, which act as an effect's do.

use crate::domains::instrument_card::InstrumentCardMsg;

use super::*;

impl App {
    pub(super) fn update_instrument_card(&mut self, message: InstrumentCardMsg) -> Task<Message> {
        let track_id = message.track_id();
        let named = self
            .instrument_panels
            .entry(track_id)
            .or_default()
            .update(message);
        match named {
            Some((pad, name)) => self.update(Message::Devices(
                crate::domains::devices::DevicesMsg::RenameDrumPad(track_id, pad, name),
            )),
            None => Task::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::domains::device_params::{DeviceParams, DeviceSlot};
    use crate::domains::devices::DevicesMsg;
    use crate::domains::param_entry::EntryMsg;
    use crate::domains::project::ProjectMsg;
    use crate::state::{DetailPanelTab, ProjectTrack};
    use vibez_core::id::TrackId;
    use vibez_core::midi::{InstrumentKind, TrackKind};
    use vibez_instruments::synth::param;

    fn synth_app() -> (App, TrackId) {
        let mut app = super::super::test_support::app();
        let mut track = ProjectTrack::new(TrackId::new(), "Keys".into(), 2);
        track.kind = TrackKind::Midi;
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::SetTrackInstrument(
            id,
            InstrumentKind::SubtractiveSynth,
        )));
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        (app, id)
    }

    fn value(app: &App, track: TrackId, index: usize) -> f32 {
        DeviceParams::instrument(app.state.find_track(track).unwrap())
            .unwrap()
            .value(index)
    }

    fn type_value(app: &mut App, track: TrackId, index: usize, text: &str) {
        let _ = app.update(Message::ParamEntry(EntryMsg::Begin {
            track_id: track,
            slot: DeviceSlot::Instrument,
            index,
            text: String::new(),
        }));
        let _ = app.update(Message::ParamEntry(EntryMsg::Edit(text.into())));
        let _ = app.update(Message::ParamEntry(EntryMsg::Submit));
    }

    #[test]
    fn typing_a_synth_value_sets_it() {
        let (mut app, track) = synth_app();
        type_value(&mut app, track, param::CUTOFF, "1.2 kHz");
        assert_eq!(value(&app, track, param::CUTOFF), 1_200.0);
    }

    #[test]
    fn moving_a_synth_knob_closes_its_typed_entry() {
        let (mut app, track) = synth_app();
        let _ = app.update(Message::ParamEntry(EntryMsg::Begin {
            track_id: track,
            slot: DeviceSlot::Instrument,
            index: param::CUTOFF,
            text: "900".into(),
        }));
        let _ = app.update(Message::set_instrument_param(track, param::CUTOFF, 2_000.0));
        assert!(app.param_entry.is_none());
    }

    #[test]
    fn a_synth_preset_applies_in_one_undo_step() {
        let (mut app, track) = synth_app();
        let device = DeviceParams::instrument(app.state.find_track(track).unwrap()).unwrap();
        let defaults = crate::domains::device_presets::capture(device, "Defaults").unwrap();
        let _ = app.update(Message::set_instrument_param(track, param::CUTOFF, 1_000.0));
        let _ = app.update(Message::set_instrument_param(track, param::RESONANCE, 4.0));
        let _ = app.update(Message::ApplyDevicePreset(
            track,
            DeviceSlot::Instrument,
            defaults,
        ));
        assert_eq!(value(&app, track, param::CUTOFF), 5_000.0);
        assert_eq!(value(&app, track, param::RESONANCE), 1.0);
        let _ = app.update(Message::Project(ProjectMsg::Undo));
        assert_eq!(value(&app, track, param::CUTOFF), 1_000.0);
        assert_eq!(value(&app, track, param::RESONANCE), 4.0);
    }

    #[test]
    fn an_older_synth_takes_edits_to_its_new_controls() {
        let (mut app, track) = synth_app();
        Arc::make_mut(&mut app.state.project_tracks)
            .find_mut(track)
            .unwrap()
            .instrument_params
            .truncate(8);
        let _ = app.update(Message::set_instrument_param(track, param::LFO_SHAPE, 2.0));
        let params = &app.state.find_track(track).unwrap().instrument_params;
        assert_eq!(params.get(param::LFO_SHAPE), Some(&2.0));
        assert_eq!(
            params.get(param::LFO_RATE),
            Some(&1.2),
            "the rest at defaults"
        );
    }

    fn drum_rack_app() -> (App, TrackId) {
        let (mut app, id) = synth_app();
        let _ = app.update(Message::Devices(DevicesMsg::SetTrackInstrument(
            id,
            InstrumentKind::DrumRack,
        )));
        app.state.project.history = Default::default();
        (app, id)
    }

    fn pad_value(app: &App, track: TrackId, pad: usize, index: usize) -> f32 {
        DeviceParams::find(
            app.state.find_track(track).unwrap(),
            DeviceSlot::DrumPad(pad),
        )
        .unwrap()
        .value(index)
    }

    #[test]
    fn typing_a_pad_value_sets_that_pad_alone() {
        use vibez_instruments::sampler::param;
        let (mut app, track) = drum_rack_app();
        let _ = app.update(Message::ParamEntry(EntryMsg::Begin {
            track_id: track,
            slot: DeviceSlot::DrumPad(3),
            index: param::TUNE,
            text: String::new(),
        }));
        let _ = app.update(Message::ParamEntry(EntryMsg::Edit("7".into())));
        let _ = app.update(Message::ParamEntry(EntryMsg::Submit));
        assert_eq!(pad_value(&app, track, 3, param::TUNE), 7.0);
        assert_eq!(pad_value(&app, track, 2, param::TUNE), 0.0);
    }

    #[test]
    fn a_sampler_preset_applies_to_a_drum_pad_in_one_undo_step() {
        use vibez_instruments::sampler::param;
        let (mut app, track) = drum_rack_app();
        let pad = DeviceSlot::DrumPad(5);
        let device = DeviceParams::find(app.state.find_track(track).unwrap(), pad).unwrap();
        let defaults = crate::domains::device_presets::capture(device, "Defaults").unwrap();
        let _ = app.update(pad.set(track, param::CUTOFF, 900.0));
        let _ = app.update(pad.set(track, param::DECAY, 0.4));
        let _ = app.update(Message::ApplyDevicePreset(track, pad, defaults));
        assert_eq!(pad_value(&app, track, 5, param::CUTOFF), 20_000.0);
        assert_eq!(pad_value(&app, track, 5, param::DECAY), 0.1);
        let _ = app.update(Message::Project(ProjectMsg::Undo));
        assert_eq!(pad_value(&app, track, 5, param::CUTOFF), 900.0);
        assert_eq!(pad_value(&app, track, 5, param::DECAY), 0.4);
    }
}
