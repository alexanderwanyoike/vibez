//! Bounded event delivery preserves state and recovers from UI backpressure.

use super::*;

#[test]
fn cancellation_delivers_applied_mute_and_stop_state_immediately() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    engine.present_event_at(
        EngineEvent::TrackMuteChanged {
            track_id,
            muted: true,
            effective_at_samples: 1000,
        },
        engine.output_position.saturating_add(521_u64),
    );
    engine.present_event_at(
        EngineEvent::PlaybackStopped,
        engine.output_position.saturating_add(521_u64),
    );
    engine.cancel_presentation();
    engine.flush_presentation();
    let events: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert!(events.iter().any(|event| matches!(event, EngineEvent::TrackMuteChanged { track_id: id, muted: true, .. } if *id == track_id)));
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::PlaybackStopped)));
}

#[test]
fn full_presentation_queue_stops_and_can_restart_without_replacing_valid_plan() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    engine.transport.play();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY {
        engine.present_event_at(
            EngineEvent::PlaybackStarted,
            engine.output_position.saturating_add(521_u64),
        );
    }
    engine.present_event_at(
        EngineEvent::PlaybackStarted,
        engine.output_position.saturating_add(521_u64),
    );
    assert!(engine.presentation_fault);
    assert!(!engine.transport.is_playing());
    assert!(engine.compensation_valid);
    let mut stopped = false;
    let mut failure = false;
    for _ in 0..4 {
        while let Ok(event) = events.pop() {
            stopped |= matches!(event, EngineEvent::PlaybackStopped);
            failure |= matches!(event, EngineEvent::CompensationInvalid { .. });
        }
        engine.flush_presentation();
    }
    assert!(stopped && failure);
    assert!(engine.scheduled_presentation.is_empty());
    commands.push(EngineCommand::Play).unwrap();
    engine.drain_commands();
    assert!(!engine.presentation_fault);
    assert!(engine.transport.is_playing());
    assert!(engine.compensation_valid);
}

#[test]
fn due_event_batch_is_bounded_by_free_ring_slots() {
    let (mut engine, _, mut events) = AudioEngine::new();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..64 {
        engine.present_event_at(
            EngineEvent::PlaybackStopped,
            engine.output_position.saturating_add(1_u64),
        );
    }
    engine.output_position = 1;
    for _ in 0..7 {
        events.pop().unwrap();
    }
    engine.flush_presentation();
    assert_eq!(engine.scheduled_presentation.len(), 57);
    while events.pop().is_ok() {}
    engine.flush_presentation();
    assert!(engine.scheduled_presentation.is_empty());
}

#[test]
fn legacy_repeat_overflow_closes_capture_in_both_render_paths() {
    use crate::note_repeat::NoteRepeatClock;
    use vibez_core::midi::InstrumentKind;
    use vibez_core::perform::NoteRepeatRate;
    for idle in [false, true] {
        let (mut engine, _, _) = AudioEngine::new();
        engine.sample_rate = 100;
        engine.transport.set_bpm(60.0);
        let mut track = EngineTrack::new(TrackId::new());
        track.instrument = Some(create_instrument(InstrumentKind::SubtractiveSynth, 100.0));
        engine.tracks.push(track);
        engine.render_idle_instruments(&mut [0.0; 16], 8, 2, 0, None, None);
        engine.tracks[0].start_note_repeat(
            NoteRepeatStart {
                id: 0,
                pitch: 42,
                velocity: 100,
                rate: NoteRepeatRate::Sixteenth,
            },
            NoteRepeatClock {
                after_sample: 0,
                anchor_sample: 0,
                include_after_sample: true,
                bpm: 60.0,
                sample_rate: 100,
                swing: SwingAmount::default(),
            },
        );
        while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY {
            engine.present_event_at(
                EngineEvent::PlaybackStarted,
                engine.output_position.saturating_add(521_u64),
            );
        }
        engine.transport.play();
        let counts = crate::retirement::tests::allocations(|| {
            if idle {
                engine.render_idle_instruments(&mut [0.0; 16], 8, 2, 0, None, None);
            } else {
                engine.render_multitrack_segment(
                    &mut [0.0; 16],
                    render_paths::MultitrackRenderBlock {
                        pos: 0,
                        repeat_pos: 0,
                        frames: 8,
                        channels: 2,
                        loop_region: None,
                        live_input: None,
                    },
                    None,
                );
            }
        });
        assert_eq!(counts, (0, 0));
        assert!(engine.presentation_fault, "idle={idle}");
        assert!(!engine.transport.is_playing());
        assert_eq!(engine.pending_capture_stop, Some(0));
        assert!(engine.pending_compensation_failure.is_some());
    }
}
