//! A newly admitted Capture closes on same-drain refailure without repeating its automatic stop.

use super::capture_lifecycle_admission_tests::{fixture, state, teardown, Fixture};
use super::*;

fn local(fixture: &Fixture, value: bool) -> EngineCommand {
    EngineCommand::SetEffectParam {
        track_id: fixture.tracks[0],
        effect_id: fixture.effect,
        param_index: 0,
        value: f32::from(u8::from(value)),
    }
}

#[derive(Default)]
struct Events {
    order: Vec<&'static str>,
    times: Vec<u64>,
    sources: Vec<(usize, u64)>,
    offset_owners: usize,
}

fn collect(fixture: &mut Fixture) -> Events {
    let mut observed = Events::default();
    while let Ok(event) = fixture.events.pop() {
        match event {
            EngineEvent::PerformanceCaptureStarted {
                offsets,
                effective_at_samples,
                section_id,
                section_position_samples,
            } => {
                assert!(Arc::ptr_eq(
                    &offsets,
                    &fixture.engine.routing.capture_offsets
                ));
                assert_eq!((section_id, section_position_samples), (None, None));
                observed.order.push("start");
                observed.times.push(effective_at_samples);
                observed.offset_owners += 1;
            }
            EngineEvent::ClipCaptureSource {
                track_id,
                position,
                effective_at_samples,
            } => {
                let index = fixture
                    .tracks
                    .iter()
                    .position(|id| *id == track_id)
                    .unwrap();
                observed.order.push("source");
                observed.times.push(effective_at_samples);
                observed.sources.push((index, position));
            }
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                observed.order.push("stop");
                observed.times.push(effective_at_samples);
            }
            EngineEvent::TrackMeter { .. }
            | EngineEvent::Metering { .. }
            | EngineEvent::PlaybackPosition(_) => {}
            event => panic!("unexpected Capture Start rearm event: {event:?}"),
        }
    }
    observed
}

#[test]
fn successful_start_rearms_local_close_before_clear_validation_without_repeating_stop() {
    let mut fixture = fixture();
    fixture.commands.push(local(&fixture, true)).unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture
            .engine
            .process_block(AudioProcessBlock::new(&mut [], 2))),
        (0, 0)
    );
    let setup = collect(&mut fixture);
    assert_eq!(setup.order, ["stop"]);
    assert_eq!(setup.times, [0]);
    assert!(setup.sources.is_empty());
    assert_eq!(setup.offset_owners, 0);
    assert_eq!(state(&fixture.engine), [0, 1, 1, 1, 0, 0, 0]);

    // Admission sees recovery, but validation cannot see a clear observation between these commands.
    fixture.commands.push(local(&fixture, false)).unwrap();
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    fixture.commands.push(local(&fixture, true)).unwrap();
    let offsets = Arc::strong_count(&fixture.engine.routing.capture_offsets);
    for (phase, frames) in [
        ("same-drain-refail", 8),
        ("persistent-after-published-stop", 16),
    ] {
        fixture.calls.clear();
        let mut audio = [0.0; 16];
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture
                .engine
                .process_block(AudioProcessBlock::new(&mut audio, 2))),
            (0, 0)
        );
        assert!(fixture.engine.cmd_rx.peek().is_err());
        let after = state(&fixture.engine);
        let calls = fixture.calls.observed();
        let starts = usize::from(frames == 8);
        assert_eq!(
            Arc::strong_count(&fixture.engine.routing.capture_offsets),
            offsets + starts
        );
        let observed = collect(&mut fixture);
        eprintln!("capture-start-rearm: phase={phase}; calls={calls:?}; after={after:?}; order={:?}; times={:?}; sources={:?}; offset_owners={}; audio_bits={:?}",
            observed.order, observed.times, observed.sources, observed.offset_owners, audio.map(f32::to_bits));
        assert_eq!(after, [0, 1, 1, 1, frames, frames, frames], "{phase}");
        assert_eq!(
            calls,
            if starts == 1 {
                vec![110, 210, 110, 211, 110, 211]
            } else {
                vec![110, 211, 110, 211]
            }
        );
        assert_eq!(
            observed.order,
            if starts == 1 {
                vec!["start", "source", "source", "stop"]
            } else {
                vec![]
            },
            "{phase}"
        );
        assert_eq!(observed.times, vec![0; starts * 4]);
        assert_eq!(observed.sources, [(0, 0), (1, 0)].repeat(starts));
        assert_eq!(observed.offset_owners, starts);
        assert_eq!(
            Arc::strong_count(&fixture.engine.routing.capture_offsets),
            offsets
        );
        assert!(!fixture.engine.capture_stop_pending());
        assert!(fixture.owner.is_none());
        for frame in audio.as_chunks::<2>().0 {
            assert_eq!(*frame, [0.25, 0.0]);
        }
    }
    teardown(fixture);
}
