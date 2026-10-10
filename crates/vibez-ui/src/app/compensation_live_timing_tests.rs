//! Configured live routes publish running timing changes through the real App bridge.

use super::*;
use crate::state::ProjectTrack;
use vibez_core::{id::TrackId, midi::TrackKind};
use vibez_engine::test_support::DelayProbeInstrument;

#[test]
fn midi_connection_selection_and_monitoring_changes_keep_running_audio_continuous() {
    let mut app = super::test_support::app();
    let (mut engine, commands, _events) = vibez_engine::engine::AudioEngine::new();
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    let first = TrackId::new();
    let second = TrackId::new();
    for (id, latency) in [(first, 0), (second, 521)] {
        let mut track = ProjectTrack::new_instrument(id, "Instrument".into(), TrackKind::Midi, 0);
        track.has_instrument = true;
        track.instrument_latency_samples = Some(latency);
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        app.send_command(EngineCommand::AddMidiTrack(id, "Instrument".into()));
        app.send_command(EngineCommand::SetPluginInstrument {
            track_id: id,
            instrument: Box::new(DelayProbeInstrument::new(latency, 0.25)),
        });
        app.send_command(EngineCommand::ExternalNoteOn {
            track_id: id,
            pitch: 60,
            velocity: 100,
        });
    }
    app.state.arrangement.selected_track = Some(first);
    app.sync_sidechain_routing_for_input(false);
    app.send_command(EngineCommand::Play);
    let mut output = [0.0_f32; 1024];
    for _ in 0..4 {
        engine.process_block(vibez_engine::engine::AudioProcessBlock::new(&mut output, 2));
    }
    let steady = output[1023];
    assert!(steady > 0.1);
    for (connected, enabled, selected) in [
        (true, true, first),
        (true, true, second),
        (true, false, first),
        (false, true, first),
        (true, true, first),
    ] {
        let _task = app.update(Message::SetReducedLatencyMonitoring(enabled));
        app.state.arrangement.selected_track = Some(selected);
        app.sync_sidechain_routing_for_input(connected);
        let timing = app.state.devices.last_timing.as_ref().unwrap();
        assert_eq!(
            timing.reduced_tracks.contains(&selected),
            connected && enabled
        );
        engine.process_block(vibez_engine::engine::AudioProcessBlock::new(&mut output, 2));
        assert!(
            output.iter().all(|&sample| (sample - steady).abs() < 1e-7),
            "connection{connected}, monitoring{enabled}, selection{selected:?}"
        );
    }
}
