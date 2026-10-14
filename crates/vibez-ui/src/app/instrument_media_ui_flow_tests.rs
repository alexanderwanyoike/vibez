//! Native drum UI transactions preserve FIFO replay and retired-media lifetimes through Undo.

use super::*;
use crate::domains::devices::DevicesMsg;
use crate::domains::project::ProjectMsg;
use crate::state::{ProjectTrack, UiDrumPad};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::TrackId;
use vibez_core::midi::InstrumentKind;
use vibez_core::midi::TrackKind;
use vibez_core::track::{DrumPadState, MediaProvenance, MediaSourceRef};
use vibez_engine::engine::AudioProcessBlock;
use vibez_instruments::PreparedDrumPadState;

fn forward_ui_commands(
    app: &mut App,
    observed: &mut rtrb::Consumer<EngineCommand>,
    engine_commands: &mut rtrb::Producer<EngineCommand>,
    inspect: impl FnOnce(&[EngineCommand]),
) {
    app.cmd_tx.flush();
    assert_eq!(app.cmd_tx.pending_len(), 0);
    let commands: Vec<_> = std::iter::from_fn(|| observed.pop().ok()).collect();
    inspect(&commands);
    for command in commands {
        engine_commands.push(command).unwrap();
    }
}

fn assert_pad_load_fifo(commands: &[EngineCommand], track: TrackId, canonical: &DrumPadState, audio: *const DecodedAudio) {
    let state_index = commands.iter().position(|command| matches!(command,
        EngineCommand::SetDrumRackPadState { track_id, pad_index: 0, .. } if *track_id == track)).unwrap();
    let load_index = commands.iter().position(|command| matches!(command,
        EngineCommand::LoadDrumRackPadSample { track_id, pad_index: 0, .. } if *track_id == track)).unwrap();
    assert_eq!(load_index, state_index + 1, "sample load follows its prepared settings without reordering");
    let EngineCommand::SetDrumRackPadState { state, .. } = &commands[state_index] else { unreachable!() };
    assert_eq!(state, &PreparedDrumPadState::from(canonical.clone()));
    let EngineCommand::LoadDrumRackPadSample { sample, sample_name, .. } = &commands[load_index] else { unreachable!() };
    assert_eq!(Arc::as_ptr(sample), audio);
    assert_eq!(Some(sample_name.as_str()), canonical.name.as_deref());
}

fn assert_level(engine: &mut AudioEngine, expected: f32) {
    let mut output = [0.0; 32];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    assert!(output.as_chunks::<2>().0.iter().all(|frame| (frame[0] - expected).abs() < 1e-6 && frame[1].abs() < 1e-6),
        "expected={expected}, actual={output:?}");
}

#[test]
fn actual_drum_ui_clear_load_and_undo_preserve_fifo_media_audio_and_retired_guards() {
    let mut app = test_support::app();
    app.state.transport.sample_rate = 1000;
    let (commands, mut observed) = rtrb::RingBuffer::new(2048);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    let (mut engine, mut engine_commands, events) = AudioEngine::new();
    app.event_rx = Some(events);
    let track_id = TrackId::new();
    let old = Arc::new(DecodedAudio { channels: vec![vec![0.25; 8192]], sample_rate: 1000 });
    let old_owner = Arc::downgrade(&old);
    app.state.browser.waveform_audio = Some(Arc::clone(&old));
    let mut track = ProjectTrack::new_instrument(track_id, "Undo drums".into(), TrackKind::Instrument(InstrumentKind::DrumRack), 0);
    track.pan = 0.0;
    track.drum_rack_pads[0] = UiDrumPad {
        name: Some("original.wav".into()), source: Some(MediaSourceRef::LocalFile { path: "/original/drum.wav".into() }),
        audio: Some(old), gain: 0.5, pan: -1.0, start: 0.125, end: 0.875,
        fade_in_ms: 0.0, fade_out_ms: 0.0, one_shot: false, choke_group: None,
        ..UiDrumPad::default()
    };
    let initial_state = track.drum_rack_pads[0].to_state();
    Arc::make_mut(&mut app.state.project_tracks).tracks.push(track.clone());
    app.send_command(EngineCommand::SetSampleRate(1000));
    app.replay_track_to_engine(&track);
    drop(track);
    forward_ui_commands(&mut app, &mut observed, &mut engine_commands, |commands| {
        assert_pad_load_fifo(commands, track_id, &initial_state, old_owner.as_ptr());
    });
    app.send_command(EngineCommand::ExternalNoteOn { track_id, pitch: 36, velocity: 127 });
    forward_ui_commands(&mut app, &mut observed, &mut engine_commands, |_| {});
    assert_level(&mut engine, 0.125);
    app.poll_engine_events();
    app.push_undo_snapshot(None);

    let loaded = Arc::new(DecodedAudio { channels: vec![vec![0.5; 8192]], sample_rate: 1000 });
    let loaded_owner = Arc::downgrade(&loaded);
    let loaded_source = MediaSourceRef::ProjectMedia {
        id: "undo-embedded-id".into(), file_name: "loaded.wav".into(),
        provenance: Some(Box::new(MediaProvenance::Remote {
            provider: "dropbox".into(), connection_id: "studio-id".into(), connection_name: Some("Studio".into()),
            source_id: "source-id".into(), source_path: "/remote/original-loaded.wav".into(), revision: Some("revision-id".into()),
        })),
    };
    app.apply_drum_rack_pad_loaded(track_id, 0, loaded, "loaded.wav".into(), loaded_source.clone());
    let loaded_state = app.state.find_track(track_id).unwrap().drum_rack_pads[0].to_state();
    assert_eq!(loaded_state.source, Some(loaded_source));
    assert_eq!(loaded_state.gain, initial_state.gain);
    assert_eq!(loaded_state.start, initial_state.start);
    assert_eq!(loaded_state.end, initial_state.end);
    forward_ui_commands(&mut app, &mut observed, &mut engine_commands, |commands| {
        assert_pad_load_fifo(commands, track_id, &loaded_state, loaded_owner.as_ptr());
    });
    assert_level(&mut engine, 0.125);
    app.poll_engine_events();
    assert_eq!(old_owner.strong_count(), 4, "old voice, Undo history, waveform cache and actual UI guard");
    app.send_command(EngineCommand::ExternalNoteOn { track_id, pitch: 36, velocity: 127 });
    forward_ui_commands(&mut app, &mut observed, &mut engine_commands, |_| {});
    assert_level(&mut engine, 0.375);

    let _ = app.update_and_refresh_clips(Message::Devices(DevicesMsg::ClearDrumRackPad(track_id, 0)));
    forward_ui_commands(&mut app, &mut observed, &mut engine_commands, |commands| {
        let state_index = commands.iter().position(|command| matches!(command,
            EngineCommand::SetDrumRackPadState { track_id: id, pad_index: 0, .. } if *id == track_id)).unwrap();
        let clear_index = commands.iter().position(|command| matches!(command,
            EngineCommand::ClearDrumRackPad { track_id: id, pad_index: 0 } if *id == track_id)).unwrap();
        assert_eq!(clear_index, state_index + 1);
        let EngineCommand::SetDrumRackPadState { state, .. } = &commands[state_index] else { unreachable!() };
        assert_eq!(state, &PreparedDrumPadState::from(DrumPadState::default()));
    });
    assert_level(&mut engine, 0.0);
    app.poll_engine_events();
    assert_eq!(old_owner.strong_count(), 3, "Undo history and cache delay the actual guard after Clear stops the voice");
    assert_eq!(app.state.find_track(track_id).unwrap().drum_rack_pads[0].to_state(), DrumPadState::default());

    let _ = app.update_and_refresh_clips(Message::Project(ProjectMsg::Undo));
    let restored = &app.state.find_track(track_id).unwrap().drum_rack_pads[0];
    assert_eq!(restored.to_state(), loaded_state);
    assert_eq!(restored.audio.as_ref().map(Arc::as_ptr), Some(loaded_owner.as_ptr()));
    forward_ui_commands(&mut app, &mut observed, &mut engine_commands, |commands| {
        assert!(commands.iter().any(|command| matches!(command, EngineCommand::RemoveTrack(id) if *id == track_id)), "actual Undo takes the native replay path");
        assert_pad_load_fifo(commands, track_id, &loaded_state, loaded_owner.as_ptr());
    });
    assert_level(&mut engine, 0.0);
    app.poll_engine_events();
    assert_eq!(old_owner.strong_count(), 3, "Undo cannot clear an old engine's retained UI guard");
    engine.process_block(AudioProcessBlock::new(&mut [0.0; 256], 2));
    app.poll_engine_events();
    app.send_command(EngineCommand::ExternalNoteOn { track_id, pitch: 36, velocity: 127 });
    forward_ui_commands(&mut app, &mut observed, &mut engine_commands, |_| {});
    assert_level(&mut engine, 0.25);
    app.poll_engine_events();
    app.state.project.history.clear();
    app.poll_engine_events();
    assert_eq!(old_owner.strong_count(), 2, "only current waveform cache and guard remain after history ends");
    app.state.browser.waveform_audio = None;
    assert_eq!(old_owner.strong_count(), 1);
    app.poll_engine_events();
    assert_eq!(old_owner.strong_count(), 0, "actual UI releases the last old owner");
    assert_eq!(app.state.find_track(track_id).unwrap().drum_rack_pads[0].to_state(), loaded_state);
}
