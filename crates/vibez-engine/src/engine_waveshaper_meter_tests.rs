//! Selected Drive snapshots retain real aligned samples through lossy observation.

use super::*;
use vibez_core::{effect::EffectType, id::EffectId};

#[test]
fn waveshaper_event_equality_compares_identity_generation_and_every_snapshot_field() {
    let track = TrackId::new();
    let effect = EffectId::new();
    let meter = vibez_core::effect::WaveShaperMeter {
        input: [0.25; 128],
        output: [0.5; 128],
        count: 128,
        sample_rate: 48_000.0,
        first_output_sample: 64,
        latency_samples: 64,
        transfer: [0.75; 65],
    };
    let event = |track_id, effect_id, generation, meter| EngineEvent::EffectWaveshaper {
        track_id,
        effect_id,
        generation,
        meter,
    };
    assert_eq!(event(track, effect, 7, meter), event(track, effect, 7, meter));
    for changed in [
        event(TrackId::new(), effect, 7, meter),
        event(track, EffectId::new(), 7, meter),
        event(track, effect, 8, meter),
    ] {
        assert_ne!(event(track, effect, 7, meter), changed);
    }
    for field in 0..7 {
        let mut changed = meter;
        match field {
            0 => changed.input[127] = -0.25,
            1 => changed.output[127] = -0.5,
            2 => changed.count = 127,
            3 => changed.sample_rate = 44_100.0,
            4 => changed.first_output_sample += 1,
            5 => changed.latency_samples += 1,
            6 => changed.transfer[64] = -0.75,
            _ => unreachable!(),
        }
        assert_ne!(event(track, effect, 7, meter), event(track, effect, 7, changed));
    }
    assert_eq!(event(track, effect, 7, meter).time_domain(), crate::events::TimeDomain::Untimed);
}

#[test]
fn drive_waveform_publishes_real_chronological_pairs_at_fifty_hertz() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let effect_id = EffectId::new();
    commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    commands
        .push(EngineCommand::insert_built_in_effect(
            track_id,
            effect_id,
            EffectType::Drive,
            48_000,
        ))
        .unwrap();
    commands
        .push(EngineCommand::SetEffectParam {
            track_id,
            effect_id,
            param_index: 0,
            value: 0.0,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: Some(track_id),
            generation: 7,
        })
        .unwrap();
    let input: [f32; 1920] = std::array::from_fn(|index| {
        if index % 2 == 0 {
            0.01 + index as f32 * 0.000_005
        } else {
            -0.7
        }
    });
    let mut output = [0.0; 1920];
    engine.process_block(
        AudioProcessBlock::new(&mut output[..960], 2)
            .with_live_input(track_id.raw(), &input[..960]),
    );
    assert!(std::iter::from_fn(|| events.pop().ok())
        .all(|event| !matches!(event, EngineEvent::EffectWaveshaper { .. })));
    engine.process_block(
        AudioProcessBlock::new(&mut output[960..], 2)
            .with_live_input(track_id.raw(), &input[960..]),
    );
    let windows: Vec<_> = std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::EffectWaveshaper {
                track_id: observed_track,
                effect_id: observed_effect,
                generation,
                meter,
            } => {
                assert_eq!((observed_track, observed_effect, generation), (track_id, effect_id, 7));
                Some(meter)
            }
            _ => None,
        })
        .collect();
    assert_eq!(windows.len(), 1, "the selected Drive needs a genuine window");
    let meter = windows[0];
    assert_eq!(meter.count, 128);
    assert_eq!(meter.sample_rate, 48_000.0);
    assert_eq!(meter.latency_samples, 64);
    assert_eq!(meter.first_output_sample, 832);
    for index in 0..usize::from(meter.count) {
        let source_frame = meter.first_output_sample as usize + index - 64;
        let expected = input[source_frame * 2];
        assert!((meter.input[index] - expected).abs() < 1e-6);
        assert!((meter.output[index] - expected).abs() < 1e-6);
    }
}
