//! A set of mixer channels selected together, and edits that move the
//! whole set.

use super::*;

fn send(a: &mut ArrangementFixture, msg: ArrangementMsg) -> RecordingEngine {
    let mut engine = RecordingEngine::default();
    a.update(msg, &mut engine, ArrangementCtx::default());
    engine
}

fn gain(a: &ArrangementFixture, id: TrackId) -> f32 {
    a.tracks
        .iter()
        .map(|track| &**track)
        .chain(a.buses.iter().map(|bus| &**bus))
        .find(|track| track.id == id)
        .unwrap()
        .gain
}

#[test]
fn ctrl_and_shift_clicks_build_a_channel_set_in_mixer_order() {
    let mut a = arrangement_with_tracks(4);
    send(&mut a, ArrangementMsg::AddBus);
    let t: Vec<_> = a.tracks.iter().map(|track| track.id).collect();
    let bus = a.buses[0].id;

    send(&mut a, ArrangementMsg::SelectTrack(t[0]));
    assert_eq!(a.arrangement.selected_channels, vec![t[0]]);
    send(&mut a, ArrangementMsg::ToggleChannelSelection(t[2]));
    assert_eq!(a.arrangement.selected_channels, vec![t[0], t[2]]);
    send(&mut a, ArrangementMsg::ToggleChannelSelection(t[0]));
    assert_eq!(a.arrangement.selected_channels, vec![t[2]]);
    send(&mut a, ArrangementMsg::ToggleChannelSelection(t[0]));
    send(&mut a, ArrangementMsg::SelectChannelRange(bus));
    assert_eq!(
        a.arrangement.selected_channels,
        vec![t[0], t[1], t[2], t[3], bus],
        "a range runs from the last channel clicked, through the buses"
    );

    send(&mut a, ArrangementMsg::SelectTrack(t[1]));
    assert_eq!(a.arrangement.selected_channels, vec![t[1]]);
    send(&mut a, ArrangementMsg::SelectAllChannels);
    assert_eq!(
        a.arrangement.selected_channels,
        vec![t[0], t[1], t[2], t[3], bus]
    );

    send(&mut a, ArrangementMsg::SelectTrack(TrackId::MASTER));
    send(
        &mut a,
        ArrangementMsg::ToggleChannelSelection(TrackId::MASTER),
    );
    assert!(
        a.arrangement.selected_channels.is_empty(),
        "the master never joins a set"
    );
}

#[test]
fn moving_a_selected_channel_moves_the_whole_set_together() {
    let mut a = arrangement_with_tracks(3);
    let t: Vec<_> = a.tracks.iter().map(|track| track.id).collect();
    a.tracks[1].gain = 0.5;
    send(&mut a, ArrangementMsg::SelectTrack(t[0]));
    send(&mut a, ArrangementMsg::ToggleChannelSelection(t[1]));

    let engine = send(&mut a, ArrangementMsg::SetTrackGain(t[0], 0.5));
    assert_eq!(gain(&a, t[0]), 0.5);
    assert!(
        (gain(&a, t[1]) - 0.25).abs() < 1e-6,
        "same dB change, so the balance holds"
    );
    assert_eq!(gain(&a, t[2]), 1.0);
    assert!(engine
        .0
        .iter()
        .any(|command| matches!(command, EngineCommand::SetTrackGain(id, _) if *id == t[1])));

    send(&mut a, ArrangementMsg::SetTrackPan(t[1], 0.7));
    assert!((a.tracks[0].pan - 0.7).abs() < 1e-6);
    assert!((a.tracks[1].pan - 0.7).abs() < 1e-6);
    assert!((a.tracks[2].pan - 0.5).abs() < 1e-6);

    send(&mut a, ArrangementMsg::SetTrackMute(t[0]));
    assert!(a.tracks[0].mute && a.tracks[1].mute && !a.tracks[2].mute);
    send(&mut a, ArrangementMsg::SetTrackSolo(t[1]));
    assert!(a.tracks[0].solo && a.tracks[1].solo && !a.tracks[2].solo);

    send(&mut a, ArrangementMsg::SetTrackGain(t[2], 0.2));
    assert_eq!(gain(&a, t[2]), 0.2, "an unselected channel moves alone");
    assert_eq!(gain(&a, t[0]), 0.5);
}

#[test]
fn sends_on_a_selected_set_move_together() {
    let mut a = arrangement_with_tracks(2);
    send(&mut a, ArrangementMsg::AddBus);
    let t: Vec<_> = a.tracks.iter().map(|track| track.id).collect();
    let bus = a.buses[0].id;
    a.tracks[0].sends.push((bus, 0.2));
    a.tracks[1].sends.push((bus, 0.5));
    send(&mut a, ArrangementMsg::SelectTrack(t[0]));
    send(&mut a, ArrangementMsg::ToggleChannelSelection(t[1]));

    send(
        &mut a,
        ArrangementMsg::SetSend {
            track_id: t[0],
            bus_id: bus,
            amount: 0.4,
        },
    );
    assert!((a.tracks[0].sends[0].1 - 0.4).abs() < 1e-6);
    assert!((a.tracks[1].sends[0].1 - 0.7).abs() < 1e-6);
}
