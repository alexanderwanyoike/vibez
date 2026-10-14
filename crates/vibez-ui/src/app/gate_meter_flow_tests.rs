//! Verifies selected native Gate inserts receive only current-generation measured levels.

use super::*;
use crate::domains::devices::DevicesMsg;
use crate::state::{ProjectTrack, Workspace};
use vibez_core::effect::EffectType;

#[test]
fn native_gate_subscribes_to_real_levels_and_rejects_stale_generations() {
    let mut app = super::super::test_support::app();
    app.state.view.workspace = Workspace::Arrange;
    app.state.view.detail_panel_tab = DetailPanelTab::Devices;
    let channel = ProjectTrack::new(TrackId::new(), "Gate".into(), 3);
    let track = channel.id;
    Arc::make_mut(&mut app.state.project_tracks).tracks.push(channel);
    let _ = app.update(Message::Devices(DevicesMsg::AddEffect(track, EffectType::Gate)));
    let effect = app.state.find_track(track).unwrap().effects[0].id;
    app.state.arrangement.selected_track = Some(track);
    app.poll_device_meters();
    assert_eq!(app.device_meters.channel, Some(track));
    let generation = app.device_meters.generation;
    let input = MeterValues { peak_l: 0.25, peak_r: 0.125, rms_l: 0.1, rms_r: 0.05 };
    let output = MeterValues { peak_l: 0.1, peak_r: 0.05, rms_l: 0.02, rms_r: 0.01 };
    app.accept_device_meter(track, effect, generation.wrapping_sub(1), input, output);
    let previous = app.device_meters.readings.get(&effect).map_or(0.0, |meter| meter.input.peak_l);
    assert_eq!(previous, 0.0);
    app.accept_device_meter(track, effect, generation, input, output);
    let actual = &app.device_meters.readings[&effect];
    assert_eq!(actual.input.peak_l, input.peak_l);
    assert_eq!(actual.output.peak_r, output.peak_r);
    app.restart_device_metering();
    app.accept_device_meter(track, effect, generation, input, output);
    assert_eq!(app.device_meters.readings[&effect].input.peak_l, 0.0);
}

#[test]
fn gate_mix_accepts_percentages_and_keeps_normalized_storage_with_one_undo() {
    use crate::domains::device_controls::DeviceControlMsg;
    let mut app = super::super::test_support::app();
    let channel = ProjectTrack::new(TrackId::new(), "Gate".into(), 3);
    let track = channel.id;
    Arc::make_mut(&mut app.state.project_tracks).tracks.push(channel);
    let _ = app.update(Message::Devices(DevicesMsg::AddEffect(track, EffectType::Gate)));
    let effect = app.state.find_track(track).unwrap().effects[0].id;
    app.state.project.history = Default::default();
    let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(track, effect,
        vibez_dsp::gate::param::MIX, "50%".into())));
    let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(track, effect,
        vibez_dsp::gate::param::MIX)));
    assert_eq!(app.state.find_track(track).unwrap().effects[0].params[vibez_dsp::gate::param::MIX], 0.5);
    let snapshot = app.state.project.history.pop_undo().expect("one mix edit");
    assert!(!app.state.project.history.can_undo());
    app.apply_snapshot(snapshot);
    assert_eq!(app.state.find_track(track).unwrap().effects[0].params[vibez_dsp::gate::param::MIX], 1.0);
}
