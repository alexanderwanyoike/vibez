//! Engine unit tests (rendering, transport, looping, metering).

use super::*;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};

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
