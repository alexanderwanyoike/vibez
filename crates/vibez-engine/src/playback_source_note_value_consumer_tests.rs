//! A native consumer preserves note indices and audible sample edges while serving stale storage hints.

use super::*;
use crate::clip_insertion::{ClipInsertionOutcome, PreparedNoteInsertion};
use crate::events::EngineEvent;

pub(super) fn measured(engine: &mut AudioEngine, output: &mut [f32]) {
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(output, 2));
        }),
        (0, 0)
    );
}

#[test]
fn native_many_note_consumer_keeps_indices_equal_starts_and_ceil_audio_while_serving_each_id_once()
{
    let track_id = TrackId::new();
    let clip_id = ClipId::new();
    let original = vec![note(60, 32, 0.0, 1.0), note(70, 64, 200.0, 0.25)];
    let source = PreparedPlaybackSource::new(
        Vec::new(),
        vec![
            EngineNoteClip::new(
                clip_id,
                0.0,
                256.0,
                original.clone(),
                0.0,
                false,
                0.0,
                256.0,
                GrooveGrid::Off,
            ),
            EngineNoteClip::new(
                clip_id,
                400.0,
                8.0,
                vec![note(72, 16, 0.5, 1.0)],
                0.25,
                true,
                0.25,
                4.0,
                GrooveGrid::Eighth,
            ),
        ],
        Vec::new(),
    );
    let mut channel = EngineTrack::with_playback_source(track_id, source);
    channel.pan = 0.0;
    let calls = Arc::new(VoiceCalls::default());
    channel.instrument = Some(Box::new(VelocityProbe {
        native: crate::test_support::DelayProbeInstrument::new(0, 0.25),
        scale: 1.0,
        calls: Arc::clone(&calls),
    }));
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands
        .push(EngineCommand::AddTrack(Box::new(channel)))
        .unwrap();
    let model: Vec<_> = [track_id, TrackId::MASTER]
        .into_iter()
        .map(|id| RoutingChannel {
            id,
            is_bus: id.is_master(),
            sends: Vec::new(),
            effects: Vec::new(),
        })
        .collect();
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&model, 8).unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    assert_signal(&output, 0.25);
    while events.pop().is_ok() {}
    let first = ClipSnapshot::observe(&engine.tracks()[0].playback_source.note_clips[0]);
    first.assert_full();
    let second = ClipSnapshot::observe(&engine.tracks()[0].playback_source.note_clips[1]);
    let mut expected = original.clone();
    let mut operations = Vec::new();
    for index in 0..257 {
        let value = note(
            61 + ((index / 2) % 60) as u8,
            32 + (index % 64) as u8,
            2.0 + (index / 2) as f64 * 0.5,
            0.25,
        );
        let insertion = PreparedNoteInsertion::new(value);
        operations.push(insertion.id());
        commands
            .push(EngineCommand::AddNote {
                track_id,
                clip_id,
                insertion,
            })
            .unwrap();
        expected.push(value);
    }
    commands.push(EngineCommand::SetBpm(137.0)).unwrap();
    commands.push(EngineCommand::Stop).unwrap();
    measured(&mut engine, &mut []);
    assert!(
        matches!(events.peek().unwrap(), EngineEvent::ClipInsertionStorageRequested(request) if request.id == operations[0])
    );
    for position in [16, 24] {
        measured(&mut engine, &mut output);
        assert_signal(&output, 0.25);
        assert_eq!(engine.transport().position(), position);
        assert_eq!(engine.transport().bpm(), 120.0);
        first.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[0]);
        second.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[1]);
    }
    assert_eq!(calls.observed(), (1, 0, 0));
    let mut requested = Vec::new();
    let mut completed = 0;
    for _ in 0..=operations.len() {
        measured(&mut engine, &mut []);
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::ClipInsertionStorageRequested(request) => {
                    assert!(operations.contains(&request.id));
                    assert_eq!((request.track_id, request.clip_id), (track_id, clip_id));
                    assert!(!requested.contains(&request.id));
                    requested.push(request.id.clone());
                    request.fulfill();
                }
                EngineEvent::ClipInsertionCompleted(owner) => {
                    assert_eq!(owner.id(), operations[completed]);
                    assert_eq!((owner.track_id, owner.clip_id), (track_id, clip_id));
                    assert_eq!(owner.outcome, ClipInsertionOutcome::Applied);
                    completed += 1;
                    drop(owner);
                }
                _ => {}
            }
        }
        if completed == operations.len() {
            break;
        }
    }
    assert_eq!(completed, 257);
    assert!(requested.len() > 1);
    assert_eq!(
        commands.slots(),
        vibez_core::constants::RING_BUFFER_CAPACITY
    );
    assert!(!engine.transport().is_playing());
    assert_eq!(engine.transport().position(), 24);
    assert_eq!(engine.transport().bpm(), 137.0);
    assert_eq!(
        engine.tracks()[0].playback_source.note_clips[0].notes,
        expected
    );
    assert_eq!(
        engine.tracks()[0].playback_source.note_clips[0].next_same_pitch_start_beats,
        expected_clamps(&expected)
    );
    second.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[1]);
    // EditNote still rebuilds its cache on this base. Exercise original-index
    // semantics separately, outside every AddNote allocation measurement.
    expected[1] = note(70, 91, 201.5, 0.375);
    commands
        .push(EngineCommand::EditNote {
            track_id,
            clip_id,
            note_index: 1,
            note: expected[1],
        })
        .unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [], 2));
    while events.pop().is_ok() {}
    assert_eq!(
        engine.tracks()[0].playback_source.note_clips[0].notes,
        expected
    );
    assert_eq!(
        engine.tracks()[0].playback_source.note_clips[0].next_same_pitch_start_beats,
        expected_clamps(&expected)
    );
    second.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[1]);
    let samples_per_beat = 44_100.0 * 60.0 / 137.0;
    assert_eq!(
        engine.transport().audio_length(),
        Some((408.0_f64 * samples_per_beat).round() as u64)
    );
    for pair in [0, 1, 19, 63, 127] {
        let last_equal_start = expected[2 + pair * 2 + 1];
        let edge = (last_equal_start.start_beat * samples_per_beat).ceil() as u64;
        commands.push(EngineCommand::Seek(edge - 1)).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        let mut edge_audio = [0.0; 4];
        measured(&mut engine, &mut edge_audio);
        assert_signal(&edge_audio[..2], 0.0);
        assert_signal(&edge_audio[2..], last_equal_start.velocity as f32 / 128.0);
        assert_eq!(engine.transport().position(), edge + 1);
        let mut heard_position = None;
        while let Ok(event) = events.pop() {
            if let EngineEvent::PlaybackPosition(position) = event {
                assert_eq!(event.time_domain(), crate::events::TimeDomain::Heard);
                heard_position = Some(position);
            }
        }
        assert_eq!(heard_position, Some(edge + 1));
        commands.push(EngineCommand::Stop).unwrap();
        measured(&mut engine, &mut []);
        while events.pop().is_ok() {}
    }
    assert_eq!(
        engine.tracks()[0].playback_source.note_clips[0].notes,
        expected
    );
    second.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[1]);
}
