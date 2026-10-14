//! Returned instrument media survives UI consumption until its final shared reference ends.

use super::*;
use std::sync::Arc;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_engine::events::EngineEvent;
use vibez_instruments::InstrumentMediaOwners;

fn sample() -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![0.25; 128]],
        sample_rate: 48_000,
    })
}

#[test]
fn returned_media_keeps_one_ui_guard_while_other_owners_live_and_reaps_afterward() {
    let mut app = test_support::app();
    let (mut events, receiver) = rtrb::RingBuffer::new(64);
    app.event_rx = Some(receiver);
    let voice = sample();
    let weak = Arc::downgrade(&voice);
    for _ in 0..32 {
        events
            .push(EngineEvent::RetiredInstrumentMedia(
                InstrumentMediaOwners::incoming(Arc::clone(&voice), "repeated-owner.wav".into()),
            ))
            .unwrap();
    }
    app.poll_engine_events();
    assert_eq!(weak.strong_count(), 2, "one voice and exactly one UI guard");
    app.clear_project_runtime();
    assert_eq!(
        weak.strong_count(),
        2,
        "project reset cannot discard old-engine guards"
    );
    app.poll_engine_events();
    assert_eq!(weak.strong_count(), 2);
    drop(voice);
    app.poll_engine_events();
    assert_eq!(
        weak.strong_count(),
        0,
        "duplicate guards must not pin one another"
    );
}

#[test]
fn equal_audio_buffers_are_distinct_guards_and_ui_cache_owners_delay_reaping() {
    let mut app = test_support::app();
    let (mut events, receiver) = rtrb::RingBuffer::new(8);
    app.event_rx = Some(receiver);
    let first = sample();
    let second = sample();
    let weak_first = Arc::downgrade(&first);
    let weak_second = Arc::downgrade(&second);
    let cached = Arc::clone(&first);
    for owner in [&first, &second] {
        events
            .push(EngineEvent::RetiredInstrumentMedia(
                InstrumentMediaOwners::incoming(
                    Arc::clone(owner),
                    "same-name-and-values.wav".into(),
                ),
            ))
            .unwrap();
    }
    app.poll_engine_events();
    assert_eq!(weak_first.strong_count(), 3);
    assert_eq!(weak_second.strong_count(), 2);
    drop(first);
    drop(second);
    app.poll_engine_events();
    assert_eq!(
        weak_first.strong_count(),
        2,
        "a real cache owner is still present"
    );
    assert_eq!(weak_second.strong_count(), 0);
    drop(cached);
    app.poll_engine_events();
    assert_eq!(weak_first.strong_count(), 0);
}

#[test]
fn thirty_two_sounding_retired_generations_do_not_block_note_off_or_stop_and_ui_reaping() {
    use vibez_core::id::TrackId;
    use vibez_core::midi::InstrumentKind;
    use vibez_core::track::DrumPadState;
    use vibez_engine::commands::EngineCommand;
    use vibez_engine::engine::{AudioEngine, AudioProcessBlock};

    for stop in [false, true] {
        let mut app = test_support::app();
        let (mut engine, mut commands, events) = AudioEngine::new();
        app.event_rx = Some(events);
        let track = TrackId::new();
        commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands
            .push(EngineCommand::set_built_in_instrument(
                track,
                InstrumentKind::DrumRack,
                1000,
            ))
            .unwrap();
        commands
            .push(EngineCommand::set_drum_rack_pad_state(
                track,
                0,
                DrumPadState {
                    fade_in_ms: 0.0,
                    fade_out_ms: 0.0,
                    one_shot: false,
                    ..Default::default()
                },
            ))
            .unwrap();
        commands.push(EngineCommand::Play).unwrap();
        engine.process_block(AudioProcessBlock::new(&mut [0.0; 1], 1));
        app.poll_engine_events();
        assert!(engine.transport().is_playing());

        let make_sample = || {
            Arc::new(DecodedAudio {
                channels: vec![vec![0.001; 4096]],
                sample_rate: 1000,
            })
        };
        let initial = make_sample();
        let mut old_generations = vec![Arc::downgrade(&initial)];
        commands
            .push(EngineCommand::LoadDrumRackPadSample {
                track_id: track,
                pad_index: 0,
                sample: initial,
                sample_name: "generation-0.wav".into(),
            })
            .unwrap();
        engine.process_block(AudioProcessBlock::new(&mut [], 1));
        app.poll_engine_events();
        // Distinct generations fill every voice slot without letting any voice finish.
        for generation in 0..32 {
            commands
                .push(EngineCommand::ExternalNoteOn {
                    track_id: track,
                    pitch: 36,
                    velocity: 127,
                })
                .unwrap();
            let mut sounding = [0.0; 1];
            engine.process_block(AudioProcessBlock::new(&mut sounding, 1));
            assert!((sounding[0] - (generation + 1) as f32 * 0.001).abs() < 1e-6);
            let next = make_sample();
            if generation < 31 {
                old_generations.push(Arc::downgrade(&next));
            }
            commands
                .push(EngineCommand::LoadDrumRackPadSample {
                    track_id: track,
                    pad_index: 0,
                    sample: next,
                    sample_name: format!("generation-{}.wav", generation + 1),
                })
                .unwrap();
            engine.process_block(AudioProcessBlock::new(&mut [], 1));
            app.poll_engine_events();
        }
        assert!(old_generations
            .iter()
            .all(|sample| sample.strong_count() == 2));
        let mut sustained = [0.0; 16];
        engine.process_block(AudioProcessBlock::new(&mut sustained, 1));
        app.poll_engine_events();
        assert!(sustained
            .iter()
            .all(|sample| (*sample - 0.032).abs() < 1e-6));
        assert!(old_generations
            .iter()
            .all(|sample| sample.strong_count() == 2));

        commands
            .push(if stop {
                EngineCommand::Stop
            } else {
                EngineCommand::ExternalNoteOff {
                    track_id: track,
                    pitch: 36,
                }
            })
            .unwrap();
        commands.push(EngineCommand::SetBpm(137.0)).unwrap();
        let mut released = [0.0; 16];
        engine.process_block(AudioProcessBlock::new(&mut released, 1));
        assert_eq!(
            engine.transport().bpm(),
            137.0,
            "FIFO command after release must run"
        );
        assert_eq!(engine.transport().is_playing(), !stop);
        if stop {
            // Stop flushes scheduled notes; external live notes retain their existing behavior.
            assert!(released.iter().all(|sample| (*sample - 0.032).abs() < 1e-6));
            assert!(old_generations
                .iter()
                .all(|sample| sample.strong_count() == 2));
            app.poll_engine_events();
            assert!(old_generations
                .iter()
                .all(|sample| sample.strong_count() == 2));
            commands
                .push(EngineCommand::ExternalNoteOff {
                    track_id: track,
                    pitch: 36,
                })
                .unwrap();
            engine.process_block(AudioProcessBlock::new(&mut released, 1));
        }
        assert_eq!(
            released, [0.0; 16],
            "NoteOff must not wait for voice completion"
        );
        assert!(old_generations
            .iter()
            .all(|sample| sample.strong_count() == 1));
        app.poll_engine_events();
        assert!(old_generations
            .iter()
            .all(|sample| sample.strong_count() == 0));
    }
}
