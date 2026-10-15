//! Public transport and Section boundaries retain Capture activity and episode ownership at real PDC.

use super::capture_lifecycle_admission_tests::{discard_setup, fixture, state, teardown, Fixture};
use super::*;
use graph_test_support::{routing_channel, routing_effect};
use vibez_core::id::EffectId;
use vibez_core::perform::SectionLaunchQuantization;
use vibez_core::routing::{NodeStage, RoutingNode};

pub(super) fn local(fixture: &Fixture, value: bool) -> EngineCommand {
    EngineCommand::SetEffectParam {
        track_id: fixture.tracks[0],
        effect_id: fixture.effect,
        param_index: 0,
        value: f32::from(u8::from(value)),
    }
}

pub(super) fn drain(fixture: &mut Fixture) {
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.drain_commands()),
        (0, 0)
    );
    assert!(fixture.engine.cmd_rx.peek().is_err());
}

fn render(fixture: &mut Fixture, output: &mut [f32]) {
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture
            .engine
            .process_block(AudioProcessBlock::new(output, 2))),
        (0, 0)
    );
}

pub(super) fn prepared(delay: u32) -> (Fixture, [f32; 64]) {
    let mut fixture = fixture();
    if delay != 0 {
        let effect = EffectId::new();
        fixture
            .commands
            .push(EngineCommand::InsertEffect {
                track_id: TrackId::MASTER,
                effect_id: effect,
                effect: Box::new(crate::test_support::DelayProbe::new(delay)),
                position: None,
            })
            .unwrap();
        let mut affected = routing_channel(fixture.tracks[0]);
        affected.effects.push(routing_effect(fixture.effect));
        let mut master = routing_channel(TrackId::MASTER);
        master.effects.push(routing_effect(effect));
        let routing = crate::routing::PreparedRouting::prepare_compensated(
            &[affected, routing_channel(fixture.tracks[1]), master],
            64,
            &[(
                RoutingNode {
                    channel: TrackId::MASTER,
                    stage: NodeStage::Effect(effect),
                },
                delay,
            )],
            &[],
            1,
        )
        .unwrap();
        fixture
            .commands
            .push(EngineCommand::SetRouting(routing))
            .unwrap();
        fixture.commands.push(EngineCommand::Play).unwrap();
        drain(&mut fixture);
        discard_setup(&mut fixture);
    }
    assert_eq!(fixture.engine.mix_latency(), delay);
    let mut audio = [0.0; 64];
    render(&mut fixture, &mut audio);
    for (index, frame) in audio.as_chunks::<2>().0.iter().enumerate() {
        assert_eq!(
            *frame,
            [if index < delay as usize { 0.0 } else { 0.25 }, 0.0]
        );
    }
    discard_setup(&mut fixture);
    fixture.commands.push(local(&fixture, true)).unwrap();
    render(&mut fixture, &mut []);
    discard_setup(&mut fixture);
    assert_eq!(state(&fixture.engine), [0, 1, 1, 1, 32, 32, 32]);
    fixture.commands.push(local(&fixture, false)).unwrap();
    drain(&mut fixture);
    fixture.calls.clear();
    (fixture, audio)
}

#[derive(Default)]
struct Events {
    order: Vec<&'static str>,
    times: Vec<u64>,
    owners: Vec<u32>,
}

fn collect(fixture: &mut Fixture, sections: &[(SectionId, usize)]) -> Events {
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
            EngineEvent::SectionCaptureStopped {
                effective_at_samples,
            } => {
                observed.order.push("section-stop");
                observed.times.push(effective_at_samples);
            }
            EngineEvent::SectionCaptureSource {
                section_id,
                effective_at_samples,
                section_position_samples,
                refreshed,
                offsets,
            } => {
                assert!(sections.iter().any(|(id, _)| *id == section_id));
                assert_eq!(section_position_samples, 0);
                assert!(!refreshed);
                assert!(Arc::ptr_eq(
                    &offsets,
                    &fixture.engine.routing.capture_offsets
                ));
                observed.order.push("section-source");
                observed.times.push(effective_at_samples);
            }
            EngineEvent::SectionTransitioned {
                section_id,
                effective_at_samples,
                retired,
            } => {
                let index = sections
                    .iter()
                    .position(|(id, _)| *id == section_id)
                    .unwrap();
                assert_eq!(
                    std::ptr::from_ref(retired.as_ref()) as usize,
                    sections[index].1
                );
                observed.owners.push(index as u32 + 1);
                observed.order.push("section");
                observed.times.push(effective_at_samples);
            }
            EngineEvent::SectionQueued {
                section_id,
                effective_at_samples,
                retired,
            } => {
                assert_eq!(section_id, sections[1].0);
                assert_eq!(effective_at_samples, 8);
                assert!(retired.is_none());
                observed.order.push("queued");
            }
            EngineEvent::TrackMeter { .. }
            | EngineEvent::Metering { .. }
            | EngineEvent::PlaybackPosition(_)
            | EngineEvent::PerformancePosition(_)
            | EngineEvent::SectionPlaybackPosition { .. } => {}
            event => panic!("unexpected Capture transport event: {event:?}"),
        }
    }
    observed
}

#[test]
fn transport_commands_preserve_the_validation_episode_at_zero_and_positive_pdc() {
    for delay in [0, 7] {
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
            let (mut fixture, audio) = prepared(delay);
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
            fixture
                .commands
                .push(EngineCommand::StartPerformanceCapture)
                .unwrap();
            drain(&mut fixture);
            discard_setup(&mut fixture);
            let before = state(&fixture.engine);
            assert_eq!(
                before,
                [
                    1,
                    1,
                    1,
                    u64::from(case != "stopped-play-reset"),
                    32,
                    if case == "perform-seek" { 0 } else { 32 },
                    32
                ]
            );
            let boundary = fixture.engine.heard_capture_position();
            assert_eq!(
                boundary,
                if case == "perform-seek" {
                    0
                } else {
                    32 - u64::from(delay)
                }
            );
            let (command, expected, order, perform) = match case {
                "stop" => (
                    EngineCommand::Stop,
                    [0, 1, 1, 0, 32, 32, 32],
                    vec!["stop", "playback-stop"],
                    false,
                ),
                "play" | "stopped-play-reset" => (
                    EngineCommand::Play,
                    [0, 1, 1, 1, 32, 32, 32],
                    vec!["stop", "playback-start"],
                    false,
                ),
                "arrange-seek" => (
                    EngineCommand::Seek(3),
                    [0, 1, 1, 1, 3, 3, 32],
                    vec!["stop"],
                    false,
                ),
                "perform-seek" => (EngineCommand::Seek(3), [1, 1, 1, 1, 3, 0, 32], vec![], true),
                "unload" => (
                    EngineCommand::UnloadAudio,
                    [0, 1, 1, 0, 32, 32, 32],
                    vec!["stop", "playback-stop"],
                    false,
                ),
                "same-rate" => (
                    EngineCommand::SetSampleRate(DEFAULT_SAMPLE_RATE),
                    [1, 1, 1, 1, 32, 32, 32],
                    vec![],
                    false,
                ),
                "begin-perform" => (
                    EngineCommand::BeginClipPerformance,
                    [1, 1, 1, 1, 32, 0, 32],
                    vec![],
                    true,
                ),
                _ => unreachable!(),
            };
            fixture.calls.clear();
            fixture.commands.push(command).unwrap();
            drain(&mut fixture);
            let after = state(&fixture.engine);
            assert_eq!(after, expected, "{case}/{delay}");
            assert_eq!(fixture.engine.clock_domain == ClockDomain::Perform, perform);
            let observed = collect(&mut fixture, &[]);
            assert_eq!(observed.order, order, "{case}/{delay}");
            assert_eq!(
                observed.times,
                if order.contains(&"stop") {
                    vec![boundary]
                } else {
                    vec![]
                }
            );
            assert!(observed.owners.is_empty());
            assert!(!fixture.engine.capture_stop_pending());
            let calls = fixture.calls.observed();
            assert!(calls.is_empty());
            eprintln!("capture-lifecycle-g4-control: delay={delay}; case={case}; calls={calls:?}; before={before:?}; after={after:?}; perform={perform}; order={:?}; times={:?}; audio_bits={:?}",
                observed.order, observed.times, audio.map(f32::to_bits));
            teardown(fixture);
        }
    }
}

fn section(length: u64, looping: bool) -> Box<PreparedSectionPlaybackSource> {
    Box::new(PreparedSectionPlaybackSource::new(
        SectionId::new(),
        length as f64 * 120.0 / (f64::from(DEFAULT_SAMPLE_RATE) * 60.0),
        looping,
        vec![],
    ))
}

#[test]
fn one_shot_and_queued_successor_keep_the_original_capture_end_boundary_at_real_pdc() {
    for delay in [0, 7] {
        for queued in [false, true] {
            let (mut fixture, prime) = prepared(delay);
            fixture
                .commands
                .push(EngineCommand::StartPerformanceCapture)
                .unwrap();
            drain(&mut fixture);
            discard_setup(&mut fixture);
            let first = section(8, false);
            let mut sections = vec![(
                first.section_id,
                std::ptr::from_ref(first.as_ref()) as usize,
            )];
            fixture
                .commands
                .push(EngineCommand::LaunchSection(first))
                .unwrap();
            if queued {
                let next = section(16, true);
                sections.push((next.section_id, std::ptr::from_ref(next.as_ref()) as usize));
                fixture
                    .commands
                    .push(EngineCommand::QueueSection {
                        prepared: next,
                        quantization: SectionLaunchQuantization::EndOfSection,
                    })
                    .unwrap();
            }
            fixture.commands.push(local(&fixture, true)).unwrap();
            fixture.calls.clear();
            for (stage, frames) in [("end", 8), ("settled", 7)] {
                let mut audio = [0.0; 16];
                render(&mut fixture, &mut audio[..frames * 2]);
                for (index, frame) in audio[..frames * 2].as_chunks::<2>().0.iter().enumerate() {
                    assert_eq!(
                        *frame,
                        [
                            if stage == "end" && index < delay as usize {
                                0.25
                            } else {
                                0.0
                            },
                            0.0
                        ]
                    );
                }
                let after = state(&fixture.engine);
                let elapsed = if stage == "end" { 8 } else { 15 };
                assert_eq!(
                    after,
                    [
                        u64::from(queued),
                        1,
                        1,
                        u64::from(queued),
                        32,
                        elapsed,
                        32 + elapsed
                    ]
                );
                assert_eq!(fixture.engine.clock_domain == ClockDomain::Perform, queued);
                assert_eq!(
                    fixture
                        .engine
                        .active_section
                        .map(|section| section.section_id),
                    queued.then(|| sections[usize::from(stage == "settled")].0)
                );
                let pending = fixture.engine.capture_stop_pending();
                assert_eq!(pending, !queued && delay != 0 && stage == "end");
                let observed = collect(&mut fixture, &sections);
                let (order, times, owners) = match (queued, delay, stage) {
                    (false, 0, "end") => (
                        vec![
                            "section-source",
                            "section",
                            "section-stop",
                            "stop",
                            "playback-stop",
                        ],
                        vec![0, 0, 8, 8],
                        vec![1],
                    ),
                    (false, 7, "end") => (vec!["section-source", "section"], vec![0, 0], vec![1]),
                    (false, 7, "settled") => (
                        vec!["section-stop", "stop", "playback-stop"],
                        vec![8, 8],
                        vec![],
                    ),
                    (true, 0, "end") => (
                        vec!["section-source", "section", "queued"],
                        vec![0, 0],
                        vec![1],
                    ),
                    (true, 7, "end") => (
                        vec!["queued", "section-source", "section"],
                        vec![0, 0],
                        vec![1],
                    ),
                    (true, _, "settled") => {
                        (vec!["section-source", "section"], vec![8, 8], vec![2])
                    }
                    (false, 0, "settled") => (vec![], vec![], vec![]),
                    _ => unreachable!(),
                };
                assert_eq!(observed.order, order, "{queued}/{delay}/{stage}");
                assert_eq!(observed.times, times);
                assert_eq!(observed.owners, owners);
                let calls = fixture.calls.observed();
                assert!(!calls.is_empty());
                eprintln!("capture-lifecycle-g4-section: delay={delay}; queued={queued}; stage={stage}; calls={calls:?}; after={after:?}; pending={pending}; order={:?}; times={:?}; owners={:?}; prime_bits={:?}; audio_bits={:?}",
                    observed.order, observed.times, observed.owners, if stage == "end" { prime.map(f32::to_bits).to_vec() } else { vec![] }, audio[..frames * 2].iter().map(|sample| sample.to_bits()).collect::<Vec<_>>());
                fixture.calls.clear();
            }
            teardown(fixture);
        }
    }
}
