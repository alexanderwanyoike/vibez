//! Gain panel readings identify each insert and measure its actual stereo signal.

use super::*;
use vibez_core::effect::EffectType;
use vibez_core::id::EffectId;
use vibez_core::routing::{RoutingChannel, RoutingEffect};

#[test]
fn monitored_gain_chain_reports_each_inserts_stereo_input_and_output() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let effects = [EffectId::new(), EffectId::new()];
    commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Metered".into()))
        .unwrap();
    for effect_id in effects {
        commands
            .push(EngineCommand::insert_built_in_effect(
                track_id,
                effect_id,
                EffectType::Gain,
                48_000,
            ))
            .unwrap();
        commands
            .push(EngineCommand::SetEffectParam {
                track_id,
                effect_id,
                param_index: 0,
                value: 0.5,
            })
            .unwrap();
    }
    commands
        .push(EngineCommand::SetTrackGain(track_id, 0.125))
        .unwrap();
    commands
        .push(EngineCommand::SetTrackGain(TrackId::MASTER, 0.125))
        .unwrap();
    let routing = crate::routing::PreparedRouting::prepare(
        &[
            RoutingChannel {
                id: track_id,
                is_bus: false,
                effects: effects
                    .iter()
                    .map(|&id| RoutingEffect {
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
    commands
        .push(EngineCommand::SetRouting(routing))
        .unwrap();
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: Some(track_id),
            generation: 7,
        })
        .unwrap();

    let input: Vec<f32> = [0.8, -0.1, 0.0, 0.3].repeat(480);
    let mut output = vec![0.0; input.len()];
    let mut readings = [None; 2];
    for _ in 0..12 {
        engine.process_block(
            AudioProcessBlock::new(&mut output, 2).with_live_input(track_id.raw(), &input),
        );
        while let Ok(event) = events.pop() {
            if let EngineEvent::EffectMeter {
                track_id: observed_track,
                effect_id,
                generation,
                input,
                output,
            } = event
            {
                assert_eq!(observed_track, track_id);
                assert_eq!(generation, 7);
                let index = effects.iter().position(|&id| id == effect_id).unwrap();
                readings[index] = Some((input, output));
            }
        }
    }

    for (index, reading) in readings.into_iter().enumerate() {
        let (input, output) = reading.expect("each visible Gain needs its own real meter reading");
        let incoming_gain = 0.5_f32.powi(index as i32);
        for (actual, expected) in [
            (input.peak_l, 0.8 * incoming_gain),
            (input.peak_r, 0.3 * incoming_gain),
            (input.rms_l, 0.32_f32.sqrt() * incoming_gain),
            (input.rms_r, 0.05_f32.sqrt() * incoming_gain),
            (output.peak_l, 0.4 * incoming_gain),
            (output.peak_r, 0.15 * incoming_gain),
            (output.rms_l, 0.32_f32.sqrt() * incoming_gain * 0.5),
            (output.rms_r, 0.05_f32.sqrt() * incoming_gain * 0.5),
        ] {
            assert!(
                (actual - expected).abs() < 1e-5,
                "insert {index}: expected {expected}, got {actual}"
            );
        }
    }
}
