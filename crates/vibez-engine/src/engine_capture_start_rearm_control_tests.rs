//! Non-Start commands and refused Starts cannot rearm a closed local Capture episode.

use super::capture_lifecycle_admission_tests::{discard_setup, state, teardown, Fixture};
use super::capture_lifecycle_transport_tests::{drain, prepared};
use super::*;
use vibez_core::effect::EffectType;
use vibez_core::id::EffectId;

#[derive(Default)]
struct Events {
    order: Vec<&'static str>,
    times: Vec<u64>,
    reasons: Vec<&'static str>,
}

fn collect(fixture: &mut Fixture) -> Events {
    let mut observed = Events::default();
    while let Ok(event) = fixture.events.pop() {
        match event {
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                observed.order.push("stop");
                observed.times.push(effective_at_samples);
            }
            EngineEvent::PlaybackStopped => observed.order.push("playback-stop"),
            EngineEvent::PlaybackStarted => observed.order.push("playback-start"),
            EngineEvent::CompensationInvalid {
                track_id,
                effect_id,
                reason,
            } => {
                assert_eq!((track_id, effect_id), (TrackId::MASTER, None));
                observed.order.push("refused");
                observed.reasons.push(reason);
            }
            EngineEvent::PerformancePosition(0) => {}
            event => panic!("unexpected Capture rearm control event: {event:?}"),
        }
    }
    observed
}

#[test]
fn idle_closed_episode_survives_each_non_start_transport_command() {
    for case in [
        "stop",
        "play",
        "arrange-seek",
        "perform-seek",
        "unload",
        "same-rate",
        "stopped-play-reset",
        "begin-perform",
    ] {
        let (mut fixture, audio) = prepared(0);
        if case == "perform-seek" {
            fixture
                .commands
                .push(EngineCommand::BeginClipPerformance)
                .unwrap();
        }
        if case == "stopped-play-reset" {
            fixture.commands.push(EngineCommand::Stop).unwrap();
        }
        drain(&mut fixture);
        discard_setup(&mut fixture);
        let before = state(&fixture.engine);
        assert_eq!(
            before,
            [
                0,
                1,
                1,
                u64::from(case != "stopped-play-reset"),
                32,
                if case == "perform-seek" { 0 } else { 32 },
                32
            ]
        );
        let (command, expected, order) = match case {
            "stop" => (
                EngineCommand::Stop,
                [0, 1, 1, 0, 32, 32, 32],
                vec!["playback-stop"],
            ),
            "play" | "stopped-play-reset" => (
                EngineCommand::Play,
                [0, 1, 1, 1, 32, 32, 32],
                vec!["playback-start"],
            ),
            "arrange-seek" => (EngineCommand::Seek(3), [0, 1, 1, 1, 3, 3, 32], vec![]),
            "perform-seek" => (EngineCommand::Seek(3), [0, 1, 1, 1, 3, 0, 32], vec![]),
            "unload" => (
                EngineCommand::UnloadAudio,
                [0, 1, 1, 0, 32, 32, 32],
                vec!["stop", "playback-stop"],
            ),
            "same-rate" => (
                EngineCommand::SetSampleRate(DEFAULT_SAMPLE_RATE),
                [0, 1, 1, 1, 32, 32, 32],
                vec![],
            ),
            "begin-perform" => (
                EngineCommand::BeginClipPerformance,
                [0, 1, 1, 1, 32, 0, 32],
                vec![],
            ),
            _ => unreachable!(),
        };
        fixture.calls.clear();
        fixture.commands.push(command).unwrap();
        drain(&mut fixture);
        let after = state(&fixture.engine);
        let calls = fixture.calls.observed();
        let observed = collect(&mut fixture);
        assert_eq!(after, expected, "{case}");
        assert!(calls.is_empty());
        assert_eq!(observed.order, order, "{case}");
        assert_eq!(
            observed.times,
            if case == "unload" { vec![32] } else { vec![] }
        );
        assert!(observed.reasons.is_empty());
        assert!(!fixture.engine.capture_stop_pending());
        assert_eq!(
            Arc::strong_count(&fixture.engine.routing.capture_offsets),
            1
        );
        assert!(fixture.owner.is_none());
        teardown(fixture);
    }
}

#[test]
fn applied_invalid_refusal_preserves_an_already_closed_local_episode() {
    let (mut fixture, _) = prepared(0);
    fixture
        .commands
        .push(EngineCommand::insert_built_in_effect(
            fixture.tracks[0],
            EffectId::new(),
            EffectType::Gain,
            DEFAULT_SAMPLE_RATE,
        ))
        .unwrap();
    fixture
        .commands
        .push(EngineCommand::RejectRoutingUpdate {
            reason: "Capture rearm negative control".into(),
        })
        .unwrap();
    drain(&mut fixture);
    discard_setup(&mut fixture);
    let before = state(&fixture.engine);
    assert_eq!(before, [0, 1, 0, 0, 32, 32, 32]);
    fixture.calls.clear();
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    drain(&mut fixture);
    let after = state(&fixture.engine);
    let calls = fixture.calls.observed();
    let observed = collect(&mut fixture);
    assert_eq!(after, before);
    assert_eq!(calls, [110, 210]);
    assert_eq!(observed.order, ["stop", "refused"]);
    assert_eq!(observed.times, [32]);
    assert_eq!(
        observed.reasons,
        ["Capture requires a valid applied audio configuration"]
    );
    assert!(!fixture.engine.capture_stop_pending());
    assert_eq!(
        Arc::strong_count(&fixture.engine.routing.capture_offsets),
        1
    );
    assert!(fixture.owner.is_none());
    teardown(fixture);
}
