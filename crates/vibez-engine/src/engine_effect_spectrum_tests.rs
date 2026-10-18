//! Selected insert spectra must retain the exact boundary and selection generation.

use super::*;
use crate::effect_spectrum::{EffectSpectrumPosition, EffectSpectrumSample, EffectSpectrumTarget};
use vibez_core::effect::EffectType;
use vibez_core::id::EffectId;
use vibez_core::routing::{RoutingChannel, RoutingEffect};

fn fixture(kind: EffectType) -> (AudioEngine, Producer<EngineCommand>, TrackId, EffectId) {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let ids = [EffectId::new(), EffectId::new(), EffectId::new()];
    commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    for (index, effect_id) in ids.into_iter().enumerate() {
        commands
            .push(EngineCommand::insert_built_in_effect(
                track_id,
                effect_id,
                if index == 1 { kind } else { EffectType::Gain },
                48_000,
            ))
            .unwrap();
        if index != 1 || kind == EffectType::Gain {
            commands
                .push(EngineCommand::SetEffectParam {
                    track_id,
                    effect_id,
                    param_index: 0,
                    value: if index == 2 { 0.125 } else { 0.5 },
                })
                .unwrap();
        }
    }
    let routing = crate::routing::PreparedRouting::prepare(
        &[
            RoutingChannel {
                id: track_id,
                is_bus: false,
                effects: ids
                    .into_iter()
                    .map(|id| RoutingEffect {
                        id,
                        inputs: vec![],
                        assignments: vec![],
                        inactive_inputs: vec![],
                    })
                    .collect(),
                sends: vec![],
            },
            RoutingChannel {
                id: TrackId::MASTER,
                is_bus: true,
                effects: vec![],
                sends: vec![],
            },
        ],
        128,
    )
    .unwrap();
    commands.push(EngineCommand::SetRouting(routing)).unwrap();
    engine.process(&mut [], 2);
    (engine, commands, track_id, ids[1])
}

fn target(
    track_id: TrackId,
    effect_id: EffectId,
    position: EffectSpectrumPosition,
    generation: u64,
) -> EngineCommand {
    EngineCommand::SetEffectSpectrumTap(Some(EffectSpectrumTarget {
        track_id,
        effect_id,
        position,
        generation,
    }))
}

fn render(engine: &mut AudioEngine, track: TrackId, input: &[f32], output: &mut [f32]) {
    engine.process_block(AudioProcessBlock::new(output, 2).with_live_input(track.raw(), input));
}

#[test]
fn selected_effect_pre_post_excludes_preceding_and_later_gain_in_both_paths() {
    let (mut engine, mut commands, track, effect) = fixture(EffectType::Gain);
    let mut receiver = engine.take_effect_spectrum_consumer().unwrap();
    assert!(engine.take_effect_spectrum_consumer().is_none());
    let input = [0.6, -0.2].repeat(256);
    let mut output = vec![0.0; input.len()];
    for (position, expected) in [
        (EffectSpectrumPosition::Input, 0.1),
        (EffectSpectrumPosition::Output, 0.05),
    ] {
        commands.push(target(track, effect, position, 7)).unwrap();
        render(&mut engine, track, &input, &mut output);
        let samples: Vec<_> = std::iter::from_fn(|| receiver.pop().ok()).collect();
        assert_eq!(samples.len(), 256, "one mono frame per insert boundary");
        for sample in samples {
            assert_eq!(sample.generation, 7);
            assert!(
                (sample.value - expected).abs() < 1e-6,
                "boundary sample {}, expected {expected}",
                sample.value
            );
        }
    }
}

#[test]
fn eq_spectrum_measures_actual_eq_processing_before_the_later_insert() {
    let (mut engine, mut commands, track, effect) = fixture(EffectType::Eq);
    let mut receiver = engine.take_effect_spectrum_consumer().unwrap();
    commands
        .push(EngineCommand::SetEffectParam {
            track_id: track,
            effect_id: effect,
            param_index: 3,
            value: 6.0,
        })
        .unwrap();
    let input: Vec<f32> = (0..4800)
        .flat_map(|frame| {
            let sample = 0.2 * (std::f32::consts::TAU * 700.0 * frame as f32 / 48_000.0).cos();
            [sample, sample]
        })
        .collect();
    let mut output = vec![0.0; input.len()];
    render(&mut engine, track, &input, &mut output);
    commands
        .push(target(track, effect, EffectSpectrumPosition::Output, 42))
        .unwrap();
    render(&mut engine, track, &input, &mut output);
    let samples: Vec<_> = std::iter::from_fn(|| receiver.pop().ok()).collect();
    assert_eq!(samples.len(), 4800);
    let measured = (samples
        .iter()
        .map(|s| f64::from(s.value).powi(2))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt();
    let expected = 0.1 * 10.0_f64.powf(6.0 / 20.0) / 2.0_f64.sqrt();
    assert!(
        (measured - expected).abs() < 2e-4,
        "EQ output {measured}, expected {expected}"
    );
    assert!(samples.iter().all(|sample| sample.generation == 42));
}

#[test]
fn retarget_generation_keeps_queued_samples_identifiable_and_disable_stops_writes() {
    let (mut engine, mut commands, track, effect) = fixture(EffectType::Gain);
    let mut receiver = engine.take_effect_spectrum_consumer().unwrap();
    let input = [0.6, -0.2].repeat(64);
    let mut output = vec![0.0; input.len()];
    commands
        .push(target(track, effect, EffectSpectrumPosition::Input, 9))
        .unwrap();
    render(&mut engine, track, &input, &mut output);
    commands
        .push(target(track, effect, EffectSpectrumPosition::Output, 10))
        .unwrap();
    render(&mut engine, track, &input, &mut output);
    let samples: Vec<EffectSpectrumSample> = std::iter::from_fn(|| receiver.pop().ok()).collect();
    assert_eq!(samples.len(), 128);
    assert!(samples[..64]
        .iter()
        .all(|sample| sample.generation == 9 && (sample.value - 0.1).abs() < 1e-6));
    assert!(samples[64..]
        .iter()
        .all(|sample| sample.generation == 10 && (sample.value - 0.05).abs() < 1e-6));
    commands
        .push(EngineCommand::SetEffectSpectrumTap(None))
        .unwrap();
    render(&mut engine, track, &input, &mut output);
    assert!(receiver.pop().is_err());
}

#[test]
fn bypass_streams_dry_samples_and_missing_target_streams_nothing() {
    let (mut engine, mut commands, track, effect) = fixture(EffectType::Gain);
    let mut receiver = engine.take_effect_spectrum_consumer().unwrap();
    let input = [0.6, -0.2].repeat(64);
    let mut output = vec![0.0; input.len()];
    commands
        .push(EngineCommand::SetEffectBypass {
            track_id: track,
            effect_id: effect,
            bypass: true,
        })
        .unwrap();
    commands
        .push(target(track, effect, EffectSpectrumPosition::Output, 11))
        .unwrap();
    render(&mut engine, track, &input, &mut output);
    let samples: Vec<_> = std::iter::from_fn(|| receiver.pop().ok()).collect();
    assert_eq!(samples.len(), 64);
    assert!(samples
        .iter()
        .all(|sample| (sample.value - 0.1).abs() < 1e-6));
    for (selected_track, selected_effect) in [(TrackId::new(), effect), (track, EffectId::new())] {
        commands
            .push(target(
                selected_track,
                selected_effect,
                EffectSpectrumPosition::Input,
                12,
            ))
            .unwrap();
        render(&mut engine, track, &input, &mut output);
        assert!(receiver.pop().is_err());
    }
}

#[test]
fn effect_spectrum_backpressure_is_bounded_and_cannot_change_audio_or_allocate() {
    let (mut engine, mut commands, track, effect) = fixture(EffectType::Gain);
    let mut receiver = engine.take_effect_spectrum_consumer().unwrap();
    let input = [0.6, -0.2].repeat(256);
    let mut output = vec![0.0; input.len()];
    commands
        .push(target(track, effect, EffectSpectrumPosition::Output, 13))
        .unwrap();
    render(&mut engine, track, &input, &mut output);
    let audible = output.clone();
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            for _ in 0..100 {
                render(&mut engine, track, &input, &mut output);
            }
        }),
        (0, 0)
    );
    assert_eq!(output, audible);
    let mut count = 0;
    while receiver.pop().is_ok() {
        count += 1;
    }
    assert!(count > 0 && count <= 16_384);
    render(&mut engine, track, &input, &mut output);
    assert_eq!(std::iter::from_fn(|| receiver.pop().ok()).count(), 256);
}

#[test]
fn return_and_master_taps_use_the_selected_insert_before_channel_gain() {
    for master in [false, true] {
        let (mut engine, mut commands, _events) = AudioEngine::new();
        let source = TrackId::new();
        let selected = if master {
            TrackId::MASTER
        } else {
            TrackId::new()
        };
        let ids = [EffectId::new(), EffectId::new(), EffectId::new()];
        commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
        commands.push(EngineCommand::add_track(source)).unwrap();
        if !master {
            commands.push(EngineCommand::add_bus(selected)).unwrap();
            commands
                .push(EngineCommand::SetSend {
                    track_id: source,
                    bus_id: selected,
                    amount: 0.25,
                })
                .unwrap();
        }
        for (index, effect_id) in ids.into_iter().enumerate() {
            commands
                .push(EngineCommand::insert_built_in_effect(
                    selected,
                    effect_id,
                    EffectType::Gain,
                    48_000,
                ))
                .unwrap();
            commands
                .push(EngineCommand::SetEffectParam {
                    track_id: selected,
                    effect_id,
                    param_index: 0,
                    value: if index == 2 { 0.125 } else { 0.5 },
                })
                .unwrap();
        }
        commands
            .push(EngineCommand::SetTrackGain(selected, 0.125))
            .unwrap();
        let effects = || {
            ids.into_iter()
                .map(|id| RoutingEffect {
                    id,
                    inputs: vec![],
                    assignments: vec![],
                    inactive_inputs: vec![],
                })
                .collect()
        };
        let mut channels = vec![RoutingChannel {
            id: source,
            is_bus: false,
            effects: vec![],
            sends: if master { vec![] } else { vec![selected] },
        }];
        if !master {
            channels.push(RoutingChannel {
                id: selected,
                is_bus: true,
                effects: effects(),
                sends: vec![],
            });
        }
        channels.push(RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            effects: if master { effects() } else { vec![] },
            sends: vec![],
        });
        commands
            .push(EngineCommand::SetRouting(
                crate::routing::PreparedRouting::prepare(&channels, 128).unwrap(),
            ))
            .unwrap();
        let mut receiver = engine.take_effect_spectrum_consumer().unwrap();
        let input = [0.6, -0.2].repeat(256);
        let mut output = vec![0.0; input.len()];
        let incoming = 0.1 * std::f32::consts::FRAC_1_SQRT_2 * if master { 1.0 } else { 0.25 };
        for (position, expected) in [
            (EffectSpectrumPosition::Input, incoming),
            (EffectSpectrumPosition::Output, incoming * 0.5),
        ] {
            commands
                .push(target(selected, ids[1], position, 23))
                .unwrap();
            render(&mut engine, source, &input, &mut output);
            let samples: Vec<_> = std::iter::from_fn(|| receiver.pop().ok()).collect();
            assert_eq!(samples.len(), 256);
            assert!(
                samples
                    .iter()
                    .all(|sample| sample.generation == 23
                        && (sample.value - expected).abs() < 1e-6),
                "master={master}, position={position:?}"
            );
        }
    }
}

#[test]
fn selected_insert_and_existing_whole_channel_spectra_stream_independently() {
    let (mut engine, mut commands, track, effect) = fixture(EffectType::Gain);
    let mut selected = engine.take_effect_spectrum_consumer().unwrap();
    let mut whole = engine.take_spectrum_consumer().unwrap();
    let input = [0.6, -0.2].repeat(64);
    let mut output = vec![0.0; input.len()];
    commands
        .push(target(track, effect, EffectSpectrumPosition::Output, 31))
        .unwrap();
    commands
        .push(EngineCommand::SetSpectrumTap(Some(track)))
        .unwrap();
    render(&mut engine, track, &input, &mut output);
    let selected_samples: Vec<_> = std::iter::from_fn(|| selected.pop().ok()).collect();
    let whole_samples: Vec<_> = std::iter::from_fn(|| whole.pop().ok()).collect();
    assert_eq!(selected_samples.len(), 64);
    assert_eq!(whole_samples.len(), 64);
    assert!(selected_samples
        .iter()
        .all(|sample| (sample.value - 0.05).abs() < 1e-6));
    assert!(whole_samples
        .iter()
        .all(|sample| (*sample - 0.00625).abs() < 1e-6));
}
