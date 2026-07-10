//! Engine unit tests (rendering, transport, looping, metering).

use super::*;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, EffectId, TrackId};

/// Helper to create a simple stereo decoded audio with a known pattern.
fn make_test_audio(frames: usize) -> Arc<DecodedAudio> {
    let left: Vec<f32> = (0..frames).map(|i| (i as f32) / (frames as f32)).collect();
    let right: Vec<f32> = (0..frames)
        .map(|i| -((i as f32) / (frames as f32)))
        .collect();
    Arc::new(DecodedAudio {
        channels: vec![left, right],
        sample_rate: 44_100,
    })
}

fn make_constant_audio(frames: usize, value: f32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![value; frames], vec![value; frames]],
        sample_rate: 44_100,
    })
}

#[test]
fn new_returns_ring_buffer_endpoints() {
    let (engine, _cmd_tx, _event_rx) = AudioEngine::new();
    assert!(!engine.transport().is_playing());
    assert!(engine.audio().is_none());
}

#[test]
fn process_outputs_silence_when_stopped() {
    let (mut engine, _cmd_tx, _event_rx) = AudioEngine::new();
    let mut buf = vec![999.0f32; 512];
    engine.process(&mut buf, 2);

    assert!(buf.iter().all(|&s| s == 0.0));
}

#[test]
fn midi_clip_commands_keep_project_length_in_sync() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let track_id = TrackId::new();
    let clip_id = ClipId::new();
    cmd_tx
        .push(EngineCommand::AddMidiTrack(track_id, "MIDI".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddNoteClip {
            track_id,
            clip_id,
            position_beats: 2.0,
            duration_beats: 4.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
        })
        .unwrap();
    let mut output = [0.0; 2];
    engine.process(&mut output, 2);
    assert_eq!(engine.transport().audio_length(), Some(132_300));

    cmd_tx.push(EngineCommand::SetBpm(60.0)).unwrap();
    cmd_tx
        .push(EngineCommand::MoveNoteClip {
            track_id,
            clip_id,
            new_position_beats: 4.0,
        })
        .unwrap();
    engine.process(&mut output, 2);
    assert_eq!(engine.transport().audio_length(), Some(352_800));

    cmd_tx
        .push(EngineCommand::RemoveNoteClip(track_id, clip_id))
        .unwrap();
    engine.process(&mut output, 2);
    assert_eq!(engine.transport().audio_length(), None);
}

#[test]
fn play_command_starts_transport() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();

    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 8];
    engine.process(&mut buf, 2);

    assert!(engine.transport().is_playing());

    // Should have received PlaybackStarted event.
    let mut found_started = false;
    while let Ok(event) = event_rx.pop() {
        if event == EngineEvent::PlaybackStarted {
            found_started = true;
        }
    }
    assert!(found_started);
}

#[test]
fn stop_command_stops_transport() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();

    cmd_tx.push(EngineCommand::Play).unwrap();
    cmd_tx.push(EngineCommand::Stop).unwrap();

    let mut buf = vec![0.0f32; 8];
    engine.process(&mut buf, 2);

    assert!(!engine.transport().is_playing());

    let mut found_stopped = false;
    while let Ok(event) = event_rx.pop() {
        if event == EngineEvent::PlaybackStopped {
            found_stopped = true;
        }
    }
    assert!(found_stopped);
}

#[test]
fn load_audio_and_play() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let audio = make_test_audio(1024);

    cmd_tx
        .push(EngineCommand::LoadAudio(audio.clone()))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16]; // 8 frames stereo
    engine.process(&mut buf, 2);

    // The output should contain the first 8 frames of the test audio.
    for frame in 0..8 {
        let expected_l = audio.sample(0, frame);
        let expected_r = audio.sample(1, frame);
        let actual_l = buf[frame * 2];
        let actual_r = buf[frame * 2 + 1];
        assert!(
            (actual_l - expected_l).abs() < 1e-6,
            "frame {frame} L: expected {expected_l} got {actual_l}"
        );
        assert!(
            (actual_r - expected_r).abs() < 1e-6,
            "frame {frame} R: expected {expected_r} got {actual_r}"
        );
    }

    // Transport should have advanced by 8 frames.
    assert_eq!(engine.transport().position(), 8);
}

#[test]
fn seek_then_play() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let audio = make_test_audio(1024);

    cmd_tx
        .push(EngineCommand::LoadAudio(audio.clone()))
        .unwrap();
    cmd_tx.push(EngineCommand::Seek(100)).unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 8]; // 4 frames stereo
    engine.process(&mut buf, 2);

    // Should be playing from position 100.
    let expected_l = audio.sample(0, 100);
    assert!((buf[0] - expected_l).abs() < 1e-6);
    assert_eq!(engine.transport().position(), 104);
}

#[test]
fn unload_audio_stops_and_clears() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let audio = make_test_audio(1024);

    cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);
    assert!(engine.audio().is_some());

    cmd_tx.push(EngineCommand::UnloadAudio).unwrap();

    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);

    assert!(engine.audio().is_none());
    assert!(!engine.transport().is_playing());

    // Drain events and check for PlaybackStopped.
    let mut found_stopped = false;
    while let Ok(event) = event_rx.pop() {
        if event == EngineEvent::PlaybackStopped {
            found_stopped = true;
        }
    }
    assert!(found_stopped);
}

#[test]
fn set_bpm_command() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    cmd_tx.push(EngineCommand::SetBpm(140.0)).unwrap();

    let mut buf = vec![0.0f32; 8];
    engine.process(&mut buf, 2);

    assert!((engine.transport().bpm() - 140.0).abs() < f64::EPSILON);
}

#[test]
fn metering_events_are_sent() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let audio = make_test_audio(1024);

    cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 512];
    engine.process(&mut buf, 2);

    let mut found_metering = false;
    while let Ok(event) = event_rx.pop() {
        if let EngineEvent::Metering { .. } = event {
            found_metering = true;
        }
    }
    assert!(found_metering);
}

#[test]
fn position_events_are_sent() {
    let (mut engine, _cmd_tx, mut event_rx) = AudioEngine::new();

    let mut buf = vec![0.0f32; 64];
    engine.process(&mut buf, 2);

    let mut found_position = false;
    while let Ok(event) = event_rx.pop() {
        if let EngineEvent::PlaybackPosition(pos) = event {
            found_position = true;
            assert_eq!(pos, 0); // transport is stopped, position stays 0
        }
    }
    assert!(found_position);
}

#[test]
fn auto_stop_at_end_of_audio() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let audio = make_test_audio(16); // only 16 frames

    cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    // Request 32 frames (more than the 16 available).
    let mut buf = vec![0.0f32; 64]; // 32 frames stereo
    engine.process(&mut buf, 2);

    // Transport should have auto-stopped at frame 16.
    assert!(!engine.transport().is_playing());
    assert_eq!(engine.transport().position(), 16);

    // Samples beyond the audio length should be 0 (DecodedAudio::sample
    // returns 0 for out-of-bounds).
    // Frames 16..31 should be silence.
    for frame in 16..32 {
        assert_eq!(buf[frame * 2], 0.0, "frame {frame} L should be 0");
        assert_eq!(buf[frame * 2 + 1], 0.0, "frame {frame} R should be 0");
    }
}

#[test]
fn multiple_process_calls_advance_position() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let audio = make_test_audio(1024);

    cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 128]; // 64 frames
    engine.process(&mut buf, 2);
    assert_eq!(engine.transport().position(), 64);

    engine.process(&mut buf, 2);
    assert_eq!(engine.transport().position(), 128);

    engine.process(&mut buf, 2);
    assert_eq!(engine.transport().position(), 192);
}

#[test]
fn mono_audio_to_stereo_output() {
    let mono_audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.5; 64]],
        sample_rate: 44_100,
    });

    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    cmd_tx.push(EngineCommand::LoadAudio(mono_audio)).unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16]; // 8 frames stereo
    engine.process(&mut buf, 2);

    // Both channels should get the mono signal.
    for frame in 0..8 {
        assert!((buf[frame * 2] - 0.5).abs() < 1e-6);
        assert!((buf[frame * 2 + 1] - 0.5).abs() < 1e-6);
    }
}

#[test]
fn process_with_zero_length_buffer() {
    let (mut engine, _cmd_tx, _event_rx) = AudioEngine::new();
    let mut buf: Vec<f32> = vec![];
    // Should not panic.
    engine.process(&mut buf, 2);
}

// -- Multi-track tests --

#[test]
fn add_and_remove_tracks() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid1 = TrackId::new();
    let tid2 = TrackId::new();

    cmd_tx
        .push(EngineCommand::AddTrack(tid1, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(tid2, "Track 2".into()))
        .unwrap();

    let mut buf = vec![0.0f32; 8];
    engine.process(&mut buf, 2);
    assert_eq!(engine.tracks().len(), 2);

    cmd_tx.push(EngineCommand::RemoveTrack(tid1)).unwrap();
    engine.process(&mut buf, 2);
    assert_eq!(engine.tracks().len(), 1);
    assert_eq!(engine.tracks()[0].id, tid2);
}

#[test]
fn reorder_tracks() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid1 = TrackId::new();
    let tid2 = TrackId::new();
    let tid3 = TrackId::new();

    cmd_tx
        .push(EngineCommand::AddTrack(tid1, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(tid2, "Track 2".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(tid3, "Track 3".into()))
        .unwrap();

    let mut buf = vec![0.0f32; 8];
    engine.process(&mut buf, 2);
    assert_eq!(engine.tracks().len(), 3);
    assert_eq!(engine.tracks()[0].id, tid1);
    assert_eq!(engine.tracks()[1].id, tid2);
    assert_eq!(engine.tracks()[2].id, tid3);

    // Reverse the order
    cmd_tx
        .push(EngineCommand::ReorderTracks(vec![tid3, tid2, tid1]))
        .unwrap();
    engine.process(&mut buf, 2);
    assert_eq!(engine.tracks()[0].id, tid3);
    assert_eq!(engine.tracks()[1].id, tid2);
    assert_eq!(engine.tracks()[2].id, tid1);
}

#[test]
fn add_clip_and_play() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();
    let audio = make_constant_audio(100, 0.5);

    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16]; // 8 frames
    engine.process(&mut buf, 2);

    // With center pan (0.5), equal power gives ~0.707 on each channel
    let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    for frame in 0..8 {
        assert!(
            (buf[frame * 2] - expected).abs() < 1e-4,
            "frame {} L: expected {} got {}",
            frame,
            expected,
            buf[frame * 2]
        );
        assert!(
            (buf[frame * 2 + 1] - expected).abs() < 1e-4,
            "frame {} R: expected {} got {}",
            frame,
            expected,
            buf[frame * 2 + 1]
        );
    }
}

#[test]
fn mute_silences_track() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();
    let audio = make_constant_audio(100, 0.8);

    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::SetTrackMute(tid, true)).unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);

    assert!(buf.iter().all(|&s| s == 0.0));
}

#[test]
fn solo_isolates_track() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid1 = TrackId::new();
    let tid2 = TrackId::new();
    let cid1 = ClipId::new();
    let cid2 = ClipId::new();

    let audio1 = make_constant_audio(100, 0.5);
    let audio2 = make_constant_audio(100, 0.3);

    cmd_tx
        .push(EngineCommand::AddTrack(tid1, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(tid2, "Track 2".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid1,
            clip_id: cid1,
            audio: audio1,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid2,
            clip_id: cid2,
            audio: audio2,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    // Solo track 1 only
    cmd_tx
        .push(EngineCommand::SetTrackSolo(tid1, true))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16]; // 8 frames
    engine.process(&mut buf, 2);

    // Only track 1 should be audible (0.5 * pan_gain)
    let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    for frame in 0..8 {
        assert!(
            (buf[frame * 2] - expected).abs() < 1e-4,
            "frame {} L: expected {} got {}",
            frame,
            expected,
            buf[frame * 2]
        );
    }
}

#[test]
fn gain_scaling() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();
    let audio = make_constant_audio(100, 1.0);

    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::SetTrackGain(tid, 0.5)).unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);

    let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!((buf[0] - expected).abs() < 1e-4);
}

#[test]
fn pan_hard_left() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();
    let audio = make_constant_audio(100, 1.0);

    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::SetTrackPan(tid, 0.0)).unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);

    // Left channel should be full (1.0 * 1.0), right should be ~0
    assert!((buf[0] - 1.0).abs() < 1e-4, "left should be ~1.0");
    assert!(buf[1].abs() < 1e-4, "right should be ~0.0");
}

#[test]
fn multi_track_summing() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid1 = TrackId::new();
    let tid2 = TrackId::new();
    let cid1 = ClipId::new();
    let cid2 = ClipId::new();

    let audio1 = make_constant_audio(100, 0.3);
    let audio2 = make_constant_audio(100, 0.4);

    cmd_tx
        .push(EngineCommand::AddTrack(tid1, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(tid2, "Track 2".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid1,
            clip_id: cid1,
            audio: audio1,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid2,
            clip_id: cid2,
            audio: audio2,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);

    // Both at center pan: each channel = (0.3 + 0.4) * FRAC_1_SQRT_2
    let expected = (0.3 + 0.4) * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (buf[0] - expected).abs() < 1e-3,
        "expected {} got {}",
        expected,
        buf[0]
    );
}

#[test]
fn legacy_compat_with_tracks_present() {
    // When tracks exist, legacy audio is ignored (multi-track path is used)
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let legacy_audio = make_constant_audio(100, 0.9);
    let tid = TrackId::new();

    cmd_tx.push(EngineCommand::LoadAudio(legacy_audio)).unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);

    // Track has no clips, so output should be silent despite legacy audio being loaded
    assert!(buf.iter().all(|&s| s == 0.0));
}

#[test]
fn per_track_metering_events() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();
    let audio = make_constant_audio(100, 0.5);

    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);

    let mut found_track_meter = false;
    while let Ok(event) = event_rx.pop() {
        if let EngineEvent::TrackMeter {
            track_id,
            peak_l,
            peak_r,
        } = event
        {
            if track_id == tid {
                found_track_meter = true;
                assert!(peak_l > 0.0);
                assert!(peak_r > 0.0);
            }
        }
    }
    assert!(found_track_meter);
}

#[test]
fn transport_auto_stop_multitrack() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();
    let audio = make_constant_audio(16, 0.5);

    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio,
            position: 0,
            source_offset: 0,
            duration: 16,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    // Request 32 frames, but only 16 frames of audio exist
    let mut buf = vec![0.0f32; 64];
    engine.process(&mut buf, 2);

    assert!(!engine.transport().is_playing());
    assert_eq!(engine.transport().position(), 16);
}

#[test]
fn move_clip_changes_position() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();
    let audio = make_constant_audio(100, 0.5);

    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::MoveClip {
            track_id: tid,
            clip_id: cid,
            new_position: 50,
        })
        .unwrap();

    let mut buf = vec![0.0f32; 8];
    engine.process(&mut buf, 2);

    // Clip is now at position 50, engine should recognize this
    assert_eq!(engine.tracks()[0].clips[0].position, 50);
}

#[test]
fn add_clip_with_loop_plays_looped_audio() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();
    let audio = make_constant_audio(100, 0.5);

    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio,
            position: 0,
            source_offset: 0,
            duration: 200,
            loop_enabled: true,
            loop_start: 0,
            loop_end: 100,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    // Process 200 frames (source is only 100 frames, but loop should fill)
    let mut buf = vec![0.0f32; 400]; // 200 frames stereo
    engine.process(&mut buf, 2);

    // Frame 150 should have non-zero audio (looped region)
    let pan_gain = std::f32::consts::FRAC_1_SQRT_2;
    let expected = 0.5 * pan_gain;
    assert!(
        (buf[150 * 2] - expected).abs() < 1e-4,
        "frame 150 L: expected ~{expected}, got {}",
        buf[150 * 2]
    );
}

#[test]
fn resize_clip_preserves_loop_state() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();
    let audio = make_constant_audio(100, 0.5);

    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    // Add clip without loop
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio: audio.clone(),
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    // Enable loop via SetClipLoop
    cmd_tx
        .push(EngineCommand::SetClipLoop {
            track_id: tid,
            clip_id: cid,
            enabled: true,
            loop_start: 0,
            loop_end: 100,
        })
        .unwrap();
    // Process to apply commands
    let mut buf = vec![0.0f32; 8];
    engine.process(&mut buf, 2);

    // Simulate resize: Remove + Add with loop fields
    cmd_tx.push(EngineCommand::RemoveClip(tid, cid)).unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio,
            position: 0,
            source_offset: 0,
            duration: 200,
            loop_enabled: true,
            loop_start: 0,
            loop_end: 100,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Seek(0)).unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 400]; // 200 frames
    engine.process(&mut buf, 2);

    // Frame 150 (in looped region) should have audio
    let pan_gain = std::f32::consts::FRAC_1_SQRT_2;
    let expected = 0.5 * pan_gain;
    assert!(
        (buf[150 * 2] - expected).abs() < 1e-4,
        "frame 150 L after resize: expected ~{expected}, got {}",
        buf[150 * 2]
    );
}

#[test]
fn preview_plays_even_when_transport_stopped() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let audio = make_constant_audio(64, 0.4);

    cmd_tx.push(EngineCommand::StartPreview(audio)).unwrap();

    // Transport is stopped: regular tracks would produce silence,
    // but the preview voice should still render.
    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);

    let peak = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    assert!(peak > 0.3, "preview should be audible: peak {peak}");
}

#[test]
fn stop_preview_silences_playback() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let audio = make_constant_audio(1024, 0.5);

    cmd_tx.push(EngineCommand::StartPreview(audio)).unwrap();
    cmd_tx.push(EngineCommand::StopPreview).unwrap();

    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);
    assert!(buf.iter().all(|s| s.abs() < 1e-6));
}

#[test]
fn preview_auto_completes_at_end_of_audio() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let audio = make_constant_audio(8, 0.6);

    cmd_tx.push(EngineCommand::StartPreview(audio)).unwrap();

    // First 4 frames: audible
    let mut buf = vec![0.0f32; 8];
    engine.process(&mut buf, 2);
    assert!(buf.iter().any(|s| s.abs() > 0.5));

    // Next 16 frames: past end of 8-frame audio. Preview auto-clears.
    let mut buf = vec![0.0f32; 32];
    engine.process(&mut buf, 2);
    // First 8 samples of buf correspond to frames 4-7 of preview (audible).
    // The remaining should be silence.
    let tail = &buf[16..];
    assert!(tail.iter().all(|s| s.abs() < 1e-6));
}

#[test]
fn starting_new_preview_interrupts_previous() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let a = make_constant_audio(1024, 0.8);
    let b = make_constant_audio(1024, 0.2);

    cmd_tx.push(EngineCommand::StartPreview(a)).unwrap();
    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);
    assert!(buf[0].abs() > 0.7);

    cmd_tx.push(EngineCommand::StartPreview(b)).unwrap();
    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);
    assert!(buf[0].abs() < 0.3);
}

#[test]
fn note_clip_loop_renders() {
    use vibez_core::midi::{InstrumentKind, MidiNote};

    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();

    // Set sample rate first so synth is initialized properly
    cmd_tx.push(EngineCommand::SetSampleRate(44_100)).unwrap();
    cmd_tx
        .push(EngineCommand::AddInstrumentTrack(
            tid,
            "Synth 1".into(),
            InstrumentKind::SubtractiveSynth,
        ))
        .unwrap();
    // Add note clip: 2 beats, looped over [0, 2) with total duration 4 beats
    cmd_tx
        .push(EngineCommand::AddNoteClip {
            track_id: tid,
            clip_id: cid,
            position_beats: 0.0,
            duration_beats: 4.0,
            loop_enabled: true,
            loop_start_beats: 0.0,
            loop_end_beats: 2.0,
        })
        .unwrap();
    // Add a note at beat 0, 1 beat long
    cmd_tx
        .push(EngineCommand::AddNote {
            track_id: tid,
            clip_id: cid,
            note: MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 1.0,
            },
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    // At 120 BPM, 1 beat = 22050 samples (44100 / 2)
    // Process enough frames to reach beat 3 (in the looped region)
    // 3 beats = 66150 samples
    let frames = 66_150;
    let mut buf = vec![0.0f32; frames * 2];
    engine.process(&mut buf, 2);

    // Check that there's audio in the looped region (around beat 2-3)
    // At beat 2.5 = sample 55125, the looped note should trigger again
    let looped_region_start = 44_100; // beat 2.0
    let looped_region_end = 66_150; // beat 3.0
    let has_audio_in_loop = buf[looped_region_start * 2..looped_region_end * 2]
        .iter()
        .any(|&s| s.abs() > 1e-6);
    assert!(
        has_audio_in_loop,
        "Expected synth audio in looped region (beat 2-3)"
    );
}

// ---- Automation ----

fn rms(buf: &[f32]) -> f32 {
    (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
}

fn constant_clip_track(
    cmd_tx: &mut rtrb::Producer<EngineCommand>,
    frames: usize,
) -> (TrackId, ClipId) {
    let tid = TrackId::new();
    let cid = ClipId::new();
    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio: make_constant_audio(frames, 0.5),
            position: 0,
            source_offset: 0,
            duration: frames as u64,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    (tid, cid)
}

#[test]
fn gain_lane_ramps_track_volume_down() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};

    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let sr = engine.sample_rate();
    let frames_total = sr as usize; // one second of audio
    let (tid, _cid) = constant_clip_track(&mut cmd_tx, frames_total);

    // 120 BPM: one second = 2 beats. Ramp 1.0 -> 0.0 across it.
    let mut lane = AutomationLane::new(AutomationTarget::TrackGain);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 1.0,
        curve: 0.0,
    });
    lane.insert_point(AutomationPoint {
        beat: 2.0,
        value: 0.0,
        curve: 0.0,
    });
    cmd_tx
        .push(EngineCommand::SetAutomationLane {
            track_id: tid,
            lane,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let block = 512usize;
    let mut buf = vec![0.0f32; block * 2];
    let mut first_rms = None;
    let mut mid_rms = None;
    let blocks = frames_total / block;
    for i in 0..blocks {
        buf.fill(0.0);
        engine.process(&mut buf, 2);
        if i == 0 {
            first_rms = Some(rms(&buf));
        }
        if i == blocks / 2 {
            mid_rms = Some(rms(&buf));
        }
    }
    let last_rms = rms(&buf);
    let (first, mid) = (first_rms.unwrap(), mid_rms.unwrap());

    // 0.5 amplitude through center equal-power pan = ~0.354 RMS.
    assert!(first > 0.3, "start should be near full volume: {first}");
    assert!(
        mid < first * 0.7 && mid > first * 0.3,
        "midpoint should be roughly half: first {first}, mid {mid}"
    );
    assert!(
        last_rms < first * 0.1,
        "end should be near silent: first {first}, last {last_rms}"
    );
}

#[test]
fn pan_lane_moves_signal_between_channels() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};

    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let sr = engine.sample_rate();
    let frames_total = sr as usize;
    let (tid, _cid) = constant_clip_track(&mut cmd_tx, frames_total);

    // Hard left for the first beat, hard right after.
    let mut lane = AutomationLane::new(AutomationTarget::TrackPan);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 0.0,
        curve: 0.0,
    });
    lane.insert_point(AutomationPoint {
        beat: 1.0,
        value: 0.0,
        curve: 0.0,
    });
    lane.insert_point(AutomationPoint {
        beat: 1.01,
        value: 1.0,
        curve: 0.0,
    });
    cmd_tx
        .push(EngineCommand::SetAutomationLane {
            track_id: tid,
            lane,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let block = 512usize;
    let mut buf = vec![0.0f32; block * 2];
    engine.process(&mut buf, 2); // first block: hard left
    let l: f32 = buf.iter().step_by(2).map(|s| s.abs()).sum();
    let r: f32 = buf.iter().skip(1).step_by(2).map(|s| s.abs()).sum();
    assert!(l > 1.0 && r < 0.01, "expected hard left: l {l}, r {r}");

    // Jump past beat 1 and render again.
    let one_beat_samples = (sr as f64 * 60.0 / 120.0) as u64;
    cmd_tx
        .push(EngineCommand::Seek(one_beat_samples + 4096))
        .unwrap();
    buf.fill(0.0);
    engine.process(&mut buf, 2);
    let l: f32 = buf.iter().step_by(2).map(|s| s.abs()).sum();
    let r: f32 = buf.iter().skip(1).step_by(2).map(|s| s.abs()).sum();
    assert!(r > 1.0 && l < 0.01, "expected hard right: l {l}, r {r}");
}

#[test]
fn removing_a_lane_restores_the_knob_value() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};

    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let sr = engine.sample_rate();
    let (tid, _cid) = constant_clip_track(&mut cmd_tx, sr as usize);

    let mut lane = AutomationLane::new(AutomationTarget::TrackGain);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 0.0,
        curve: 0.0,
    });
    let lane_id = lane.id;
    cmd_tx
        .push(EngineCommand::SetAutomationLane {
            track_id: tid,
            lane,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 1024];
    engine.process(&mut buf, 2);
    assert!(rms(&buf) < 1e-6, "lane at zero should silence the track");

    cmd_tx
        .push(EngineCommand::RemoveAutomationLane {
            track_id: tid,
            lane_id,
        })
        .unwrap();
    buf.fill(0.0);
    engine.process(&mut buf, 2);
    assert!(
        rms(&buf) > 0.2,
        "removing the lane should restore the track gain"
    );
}

#[test]
fn effect_tails_ring_out_after_stop() {
    // A delay tail must keep sounding after the transport stops:
    // effect chains process while stopped. (This is also what
    // delivers queued plugin param changes without pressing play.)
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let sr = engine.sample_rate() as usize;
    let (tid, _cid) = constant_clip_track(&mut cmd_tx, sr);

    let effect_id = vibez_core::id::EffectId::new();
    cmd_tx
        .push(EngineCommand::AddEffect {
            track_id: tid,
            effect_id,
            effect_type: vibez_core::effect::EffectType::Delay,
            position: None,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    // Delay time defaults to 500 ms: play well past it so echoes
    // exist, then listen for them after stop.
    let mut buf = vec![0.0f32; 1024];
    for _ in 0..60 {
        buf.fill(0.0);
        engine.process(&mut buf, 2);
    }
    cmd_tx.push(EngineCommand::Stop).unwrap();
    buf.fill(0.0);
    engine.process(&mut buf, 2); // drains Stop, first stopped block

    let mut tail = 0.0f32;
    for _ in 0..40 {
        buf.fill(0.0);
        engine.process(&mut buf, 2);
        tail += buf.iter().map(|s| s.abs()).sum::<f32>();
    }
    assert!(
        tail > 0.01,
        "delay tail should ring out after stop, got {tail}"
    );
}

#[test]
fn master_gain_scales_the_summed_mix() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();
    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio: make_constant_audio(100, 0.5),
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetTrackGain(TrackId::MASTER, 0.5))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);

    let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2 * 0.5;
    assert!(
        (buf[0] - expected).abs() < 1e-4,
        "master gain 0.5 should halve the mix: expected {expected} got {}",
        buf[0]
    );
}

#[test]
fn master_effect_chain_processes_the_summed_mix() {
    use vibez_core::effect::EffectType;
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let cid = ClipId::new();
    cmd_tx
        .push(EngineCommand::AddTrack(tid, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio: make_constant_audio(4_096, 0.5),
            position: 0,
            source_offset: 0,
            duration: 4_096,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    // A master EQ with the LF shelf slammed down should attenuate a
    // constant (DC-heavy) signal relative to the flat default.
    let eq_id = EffectId::new();
    cmd_tx
        .push(EngineCommand::AddEffect {
            track_id: TrackId::MASTER,
            effect_id: eq_id,
            effect_type: EffectType::Eq,
            position: None,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetEffectParam {
            track_id: TrackId::MASTER,
            effect_id: eq_id,
            param_index: 0, // LF gain
            value: -15.0,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetEffectParam {
            track_id: TrackId::MASTER,
            effect_id: eq_id,
            param_index: 1, // LF freq up to make the cut obvious
            value: 450.0,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 2_048];
    let mut last_rms = 0.0f32;
    for _ in 0..3 {
        buf.fill(0.0);
        engine.process(&mut buf, 2);
        last_rms = (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
    }
    let flat = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        last_rms < flat * 0.5,
        "master LF cut should attenuate the constant mix well below {flat}, got {last_rms}"
    );

    // Bypassing the master EQ restores the flat mix.
    cmd_tx
        .push(EngineCommand::SetEffectBypass {
            track_id: TrackId::MASTER,
            effect_id: eq_id,
            bypass: true,
        })
        .unwrap();
    buf.fill(0.0);
    engine.process(&mut buf, 2);
    assert!(
        (buf[0] - flat).abs() < 1e-3,
        "bypassed master chain should pass the mix through, got {}",
        buf[0]
    );
}

#[test]
fn spectrum_tap_streams_track_samples() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let mut spectrum_rx = engine.take_spectrum_consumer().unwrap();
    let tid = TrackId::new();
    let cid = ClipId::new();
    cmd_tx
        .push(EngineCommand::AddTrack(tid, "T".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio: make_constant_audio(10_000, 0.5),
            position: 0,
            source_offset: 0,
            duration: 10_000,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetSpectrumTap(Some(tid)))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut buf = vec![0.0f32; 1024];
    engine.process(&mut buf, 2);

    let mut peak = 0.0f32;
    let mut count = 0;
    while let Ok(s) = spectrum_rx.pop() {
        peak = peak.max(s.abs());
        count += 1;
    }
    assert_eq!(count, 512, "one mono sample per frame");
    assert!(peak > 0.4, "tap should carry the clip audio, got {peak}");

    // Retarget to master: still streams (the summed mix).
    cmd_tx
        .push(EngineCommand::SetSpectrumTap(Some(TrackId::MASTER)))
        .unwrap();
    engine.process(&mut buf, 2);
    let mut master_peak = 0.0f32;
    while let Ok(s) = spectrum_rx.pop() {
        master_peak = master_peak.max(s.abs());
    }
    assert!(
        master_peak > 0.3,
        "master tap should carry the mix, got {master_peak}"
    );
}

// ── Busses (return channels) ────────────────────────────────────────

/// One track sending to one bus; block of constant audio.
fn engine_with_send(send: f32) -> (AudioEngine, rtrb::Producer<EngineCommand>, TrackId, TrackId) {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    let bus = TrackId::new();
    cmd_tx
        .push(EngineCommand::AddTrack(tid, "T".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddClip {
            track_id: tid,
            clip_id: ClipId::new(),
            audio: make_constant_audio(50_000, 0.5),
            position: 0,
            source_offset: 0,
            duration: 50_000,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::AddBus(bus, "A".into())).unwrap();
    cmd_tx
        .push(EngineCommand::SetSend {
            track_id: tid,
            bus_id: bus,
            amount: send,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();
    // Drain the setup commands with one silent-prep block.
    let mut buf = vec![0.0f32; 16];
    engine.process(&mut buf, 2);
    (engine, cmd_tx, tid, bus)
}

#[test]
fn send_routes_post_fader_signal_into_the_bus() {
    // Full send on a flat bus doubles the track's contribution
    // (dry path + bus path), zero send leaves it dry.
    let (mut engine, _cmd, _tid, _bus) = engine_with_send(1.0);
    let mut buf = vec![0.0f32; 512];
    engine.process(&mut buf, 2);
    let dry = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (buf[0] - dry * 2.0).abs() < 1e-3,
        "dry + unity send should double: expected {} got {}",
        dry * 2.0,
        buf[0]
    );

    let (mut engine, _cmd, _tid, _bus) = engine_with_send(0.0);
    let mut buf = vec![0.0f32; 512];
    engine.process(&mut buf, 2);
    assert!(
        (buf[0] - dry).abs() < 1e-3,
        "zero send stays dry: expected {dry} got {}",
        buf[0]
    );
}

#[test]
fn bus_gain_and_mute_shape_the_return() {
    let (mut engine, mut cmd_tx, _tid, bus) = engine_with_send(1.0);
    cmd_tx.push(EngineCommand::SetTrackGain(bus, 0.5)).unwrap();
    let mut buf = vec![0.0f32; 512];
    engine.process(&mut buf, 2);
    let dry = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (buf[0] - dry * 1.5).abs() < 1e-3,
        "half-gain bus adds half a contribution: expected {} got {}",
        dry * 1.5,
        buf[0]
    );

    cmd_tx.push(EngineCommand::SetTrackMute(bus, true)).unwrap();
    buf.fill(0.0);
    engine.process(&mut buf, 2);
    assert!(
        (buf[0] - dry).abs() < 1e-3,
        "muted bus contributes nothing: expected {dry} got {}",
        buf[0]
    );
}

#[test]
fn soloed_bus_keeps_its_send_input_and_suppresses_the_dry_path() {
    let (mut engine, mut cmd_tx, _track_id, bus_id) = engine_with_send(1.0);
    cmd_tx
        .push(EngineCommand::SetTrackGain(bus_id, 0.5))
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetTrackSolo(bus_id, true))
        .unwrap();

    let mut buf = vec![0.0f32; 512];
    engine.process(&mut buf, 2);

    let dry = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (buf[0] - dry * 0.5).abs() < 1e-3,
        "bus solo should output only the wet return: expected {} got {}",
        dry * 0.5,
        buf[0]
    );
}

#[test]
fn bus_effect_chain_processes_the_send_mix() {
    let (mut engine, mut cmd_tx, _tid, bus) = engine_with_send(1.0);
    // Slam the bus EQ's LF shelf: the return should stop doubling
    // the constant (DC-heavy) signal.
    let eq_id = EffectId::new();
    cmd_tx
        .push(EngineCommand::AddEffect {
            track_id: bus,
            effect_id: eq_id,
            effect_type: vibez_core::effect::EffectType::Eq,
            position: None,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetEffectParam {
            track_id: bus,
            effect_id: eq_id,
            param_index: 0,
            value: -15.0,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetEffectParam {
            track_id: bus,
            effect_id: eq_id,
            param_index: 1,
            value: 450.0,
        })
        .unwrap();

    let mut buf = vec![0.0f32; 2_048];
    let mut rms = 0.0f32;
    for _ in 0..3 {
        buf.fill(0.0);
        engine.process(&mut buf, 2);
        rms = (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
    }
    let dry = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        rms > dry * 0.8 && rms < dry * 1.6,
        "cut return should sit near the dry level, got {rms} (dry {dry})"
    );
}

#[test]
fn remove_bus_drops_its_sends() {
    let (mut engine, mut cmd_tx, _tid, bus) = engine_with_send(1.0);
    cmd_tx.push(EngineCommand::RemoveBus(bus)).unwrap();
    let mut buf = vec![0.0f32; 512];
    engine.process(&mut buf, 2);
    let dry = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (buf[0] - dry).abs() < 1e-3,
        "after RemoveBus only the dry path remains: expected {dry} got {}",
        buf[0]
    );
    assert!(engine.tracks()[0].sends.is_empty(), "sends cleaned up");
}

#[test]
fn remove_bus_drops_its_send_automation() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};

    let (mut engine, mut cmd_tx, track_id, bus_id) = engine_with_send(0.0);
    let mut lane = AutomationLane::new(AutomationTarget::Send { bus_id });
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 1.0,
        curve: 0.0,
    });
    cmd_tx
        .push(EngineCommand::SetAutomationLane { track_id, lane })
        .unwrap();
    cmd_tx.push(EngineCommand::RemoveBus(bus_id)).unwrap();

    let mut buf = vec![0.0f32; 512];
    engine.process(&mut buf, 2);

    assert!(
        engine.tracks()[0].sends.is_empty(),
        "removed bus automation must not recreate a stale send"
    );
}

#[test]
fn bus_gain_automation_rides_the_return() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    let (mut engine, mut cmd_tx, _tid, bus) = engine_with_send(1.0);
    // Lane pinned at zero: the return contributes nothing even
    // though the bus fader sits at unity.
    let mut lane = AutomationLane::new(AutomationTarget::TrackGain);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 0.0,
        curve: 0.0,
    });
    cmd_tx
        .push(EngineCommand::SetAutomationLane {
            track_id: bus,
            lane,
        })
        .unwrap();
    let mut buf = vec![0.0f32; 512];
    engine.process(&mut buf, 2);
    let dry = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (buf[0] - dry).abs() < 1e-3,
        "zeroed bus gain lane should leave only the dry path: expected {dry} got {}",
        buf[0]
    );
}

#[test]
fn send_automation_opens_the_send() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    // Send starts closed; a lane at 1.0 opens it.
    let (mut engine, mut cmd_tx, tid, bus) = engine_with_send(0.0);
    let mut lane = AutomationLane::new(AutomationTarget::Send { bus_id: bus });
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 1.0,
        curve: 0.0,
    });
    cmd_tx
        .push(EngineCommand::SetAutomationLane {
            track_id: tid,
            lane,
        })
        .unwrap();
    let mut buf = vec![0.0f32; 512];
    engine.process(&mut buf, 2);
    let dry = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (buf[0] - dry * 2.0).abs() < 1e-3,
        "send lane at 1.0 should double like a unity send: expected {} got {}",
        dry * 2.0,
        buf[0]
    );
}

#[test]
fn master_gain_automation_shapes_the_mix() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    let (mut engine, mut cmd_tx, _tid, _bus) = engine_with_send(0.0);
    let mut lane = AutomationLane::new(AutomationTarget::TrackGain);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 0.25, // normalized: gain range 0..2 -> 0.5x
        curve: 0.0,
    });
    cmd_tx
        .push(EngineCommand::SetAutomationLane {
            track_id: TrackId::MASTER,
            lane,
        })
        .unwrap();
    let mut buf = vec![0.0f32; 512];
    engine.process(&mut buf, 2);
    let dry = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        (buf[0] - dry * 0.5).abs() < 1e-3,
        "master lane at 0.25 (=0.5x) should halve the mix: expected {} got {}",
        dry * 0.5,
        buf[0]
    );
}
