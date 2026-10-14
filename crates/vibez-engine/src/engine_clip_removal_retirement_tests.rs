//! Complete clip removal preserves playback and returns owned storage outside the callback.

use super::*;
use vibez_core::id::ClipId;
use vibez_core::track::{ClipFades, ClipPlaybackDirection};
use vibez_core::warp_marker::WarpMarkers;

fn audio(value: f32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![value; 512]],
        sample_rate: 44_100,
    })
}

fn add_clip(track: TrackId, id: ClipId, audio: Arc<DecodedAudio>, duration: u64) -> EngineCommand {
    let mut warp_markers = WarpMarkers::default();
    assert!(warp_markers.add(32, 32, 0, 512, 512));
    EngineCommand::AddClip {
        track_id: track,
        clip_id: id,
        audio,
        position: 0,
        source_offset: 4,
        start_marker: 7,
        duration,
        loop_enabled: true,
        loop_start: 8,
        loop_end: 100,
        linear_gain: 0.75,
        fades: ClipFades::default(),
        playback_direction: ClipPlaybackDirection::Reverse,
        warp_markers,
    }
}

#[test]
fn removing_a_unique_audio_clip_preserves_survivors_without_callback_destruction() {
    for full_ring in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let ids = [ClipId::new(), ClipId::new(), ClipId::new()];
        let removed_audio = audio(0.25);
        let removed_owner = Arc::downgrade(&removed_audio);
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands
            .push(add_clip(track, ids[0], audio(0.125), 128))
            .unwrap();
        commands
            .push(add_clip(track, ids[1], removed_audio, 300))
            .unwrap();
        commands
            .push(add_clip(track, ids[2], audio(0.5), 200))
            .unwrap();
        commands.push(EngineCommand::Play).unwrap();
        let mut before = [0.0; 16];
        engine.process(&mut before, 2);
        let expected_before = (0.125 + 0.25 + 0.5) * 0.75 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(before
            .iter()
            .all(|sample| (*sample - expected_before).abs() < 1e-6));
        assert_eq!(engine.transport().audio_length(), Some(300));
        let survivor_geometry: Vec<_> = engine.tracks()[0]
            .playback_source
            .clips
            .iter()
            .filter(|clip| clip.id != ids[1])
            .map(|clip| {
                (
                    (
                        clip.id,
                        Arc::as_ptr(&clip.audio),
                        clip.position,
                        clip.source_offset,
                        clip.start_marker,
                        clip.duration,
                        clip.loop_enabled,
                    ),
                    (
                        clip.loop_start,
                        clip.loop_end,
                        clip.linear_gain,
                        clip.fades,
                        clip.playback_direction,
                        clip.warp_markers.as_slice().as_ptr(),
                        clip.warp_markers.clone(),
                    ),
                )
            })
            .collect();
        while events.pop().is_ok() {}
        if full_ring {
            while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        }
        commands
            .push(EngineCommand::RemoveClip(track, ids[1]))
            .unwrap();
        let mut after = [0.0; 16];
        let allocation = crate::retirement::tests::allocations(|| engine.process(&mut after, 2));
        let expected_after = (0.125 + 0.5) * 0.75 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(after
            .iter()
            .all(|sample| (*sample - expected_after).abs() < 1e-6));
        assert_eq!(engine.transport().audio_length(), Some(200));
        let survivors = &engine.tracks()[0].playback_source.clips;
        assert_eq!(survivors.len(), 2);
        for (clip, expected) in survivors.iter().zip(&survivor_geometry) {
            assert_eq!(
                (
                    clip.id,
                    Arc::as_ptr(&clip.audio),
                    clip.position,
                    clip.source_offset,
                    clip.start_marker,
                    clip.duration,
                    clip.loop_enabled
                ),
                expected.0
            );
            assert_eq!(
                (
                    clip.loop_start,
                    clip.loop_end,
                    clip.linear_gain,
                    clip.fades,
                    clip.playback_direction,
                    clip.warp_markers.as_slice().as_ptr(),
                    &clip.warp_markers
                ),
                (
                    expected.1 .0,
                    expected.1 .1,
                    expected.1 .2,
                    expected.1 .3,
                    expected.1 .4,
                    expected.1 .5,
                    &expected.1 .6
                )
            );
        }
        assert_eq!(allocation, (0, 0), "full_ring={full_ring}");
        assert_eq!(removed_owner.strong_count(), 1);
        while events.pop().is_ok() {}
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.flush_retirements()),
            (0, 0)
        );
        while events.pop().is_ok() {}
        assert_eq!(removed_owner.strong_count(), 0);
    }
}

#[test]
fn duplicate_audio_removal_is_atomic_under_retirement_backpressure_and_reuses_capacity() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let duplicate = ClipId::new();
    let survivors = [ClipId::new(), ClipId::new()];
    let count = crate::retirement::RETIREMENT_CAPACITY * 2 + 3;
    let mut identities = Vec::new();
    commands.push(EngineCommand::add_track(track)).unwrap();
    commands
        .push(add_clip(track, survivors[0], audio(0.125), 128))
        .unwrap();
    for _ in 0..count {
        let value = audio(0.001);
        identities.push(Arc::downgrade(&value));
        commands
            .push(add_clip(track, duplicate, value, 400))
            .unwrap();
    }
    commands
        .push(add_clip(track, survivors[1], audio(0.5), 200))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0; 16], 2);
    while events.pop().is_ok() {}
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    commands
        .push(EngineCommand::RemoveClip(track, duplicate))
        .unwrap();
    commands
        .push(EngineCommand::SetTrackPan(track, 0.25))
        .unwrap();
    let mut output = [0.0; 16];
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
        (0, 0)
    );
    assert_eq!(
        engine.pending_retirements.len(),
        crate::retirement::RETIREMENT_CAPACITY
    );
    assert_eq!(engine.cmd_rx.slots(), 1);
    assert_eq!(engine.tracks()[0].pan, 0.5);
    assert_eq!(engine.transport().audio_length(), Some(200));
    assert_eq!(
        engine.tracks()[0]
            .playback_source
            .clips
            .iter()
            .map(|clip| clip.id)
            .collect::<Vec<_>>(),
        survivors
    );
    let expected = 0.625 * 0.75 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(output
        .iter()
        .all(|sample| (*sample - expected).abs() < 1e-6));
    assert!(identities.iter().all(|owner| owner.strong_count() == 1));
    for _ in 0..2 {
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
            (0, 0)
        );
        assert_eq!(engine.cmd_rx.slots(), 1);
        assert!(output
            .iter()
            .all(|sample| (*sample - expected).abs() < 1e-6));
    }
    let mut returned = Vec::new();
    for _ in 0..4 {
        while let Ok(event) = events.pop() {
            if let EngineEvent::RetiredAudioClip(clip) = event {
                returned.push(Arc::as_ptr(&clip.audio));
                assert_eq!(clip.id, duplicate);
                assert!(!clip.warp_markers.is_empty());
            }
        }
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [], 2)),
            (0, 0)
        );
    }
    assert_eq!(engine.cmd_rx.slots(), 0);
    assert_eq!(engine.tracks()[0].pan, 0.25);
    assert_eq!(returned.len(), count);
    for owner in &identities {
        assert_eq!(
            returned
                .iter()
                .filter(|pointer| **pointer == owner.as_ptr())
                .count(),
            1
        );
        assert_eq!(owner.strong_count(), 0);
    }
    commands
        .push(add_clip(track, duplicate, audio(0.001), 400))
        .unwrap();
    engine.process(&mut [], 2);
    commands
        .push(EngineCommand::RemoveClip(track, duplicate))
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [], 2)),
        (0, 0)
    );
    assert_eq!(engine.tracks()[0].playback_source.clips.len(), 2);
}

#[test]
fn removing_populated_note_clips_flushes_sound_without_callback_destruction() {
    use crate::playback_source::PreparedPlaybackSource;
    use crate::test_support::DelayProbeInstrument;
    use vibez_core::midi::MidiNote;
    use vibez_core::perform::GrooveGrid;
    for full_ring in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let duplicate = ClipId::new();
        let survivor = ClipId::new();
        let clip = |id, duration, start| {
            EngineNoteClip::new(
                id,
                0.0,
                duration,
                vec![
                    MidiNote {
                        pitch: 60,
                        velocity: 100,
                        start_beat: start,
                        duration_beats: 1.5,
                    },
                    MidiNote {
                        pitch: 60,
                        velocity: 90,
                        start_beat: 2.0,
                        duration_beats: 1.0,
                    },
                    MidiNote {
                        pitch: 61,
                        velocity: 80,
                        start_beat: 2.0,
                        duration_beats: 1.0,
                    },
                ],
                0.0,
                false,
                0.0,
                duration,
                GrooveGrid::Off,
            )
        };
        let source = PreparedPlaybackSource::new(
            vec![],
            vec![
                clip(duplicate, 4.0, 0.0),
                clip(survivor, 2.0, 1.0),
                clip(duplicate, 6.0, 0.0),
            ],
            vec![],
        );
        for command in [
            EngineCommand::SetSampleRate(96),
            EngineCommand::SetBpm(60.0),
            EngineCommand::AddTrack(Box::new(EngineTrack::with_playback_source(track, source))),
            EngineCommand::SetInstrument {
                track_id: track,
                instrument: Box::new(DelayProbeInstrument::new(0, 0.25)),
            },
            EngineCommand::Play,
        ] {
            commands.push(command).unwrap();
        }
        let mut sounding = [0.0; 32];
        engine.process(&mut sounding, 2);
        assert!(sounding.iter().all(|sample| *sample > 0.1));
        assert_eq!(engine.transport().audio_length(), Some(576));
        while events.pop().is_ok() {}
        if full_ring {
            while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        }
        commands
            .push(EngineCommand::RemoveNoteClip(track, duplicate))
            .unwrap();
        let measured = crate::retirement::tests::allocations(|| engine.process(&mut sounding, 2));
        assert_eq!(sounding, [0.0; 32]);
        assert_eq!(engine.transport().audio_length(), Some(192));
        assert_eq!(engine.tracks()[0].playback_source.note_clips.len(), 1);
        let remaining = &engine.tracks()[0].playback_source.note_clips[0];
        assert_eq!(remaining.id, survivor);
        assert_eq!(remaining.notes.len(), 3);
        assert_eq!(remaining.notes[0].start_beat, 1.0);
        assert_eq!(measured, (0, 0), "full_ring={full_ring}");
    }
}
