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

type Fixture = (
    AudioEngine,
    Producer<EngineCommand>,
    Consumer<EngineEvent>,
    TrackId,
    EffectId,
);

fn fixture() -> Fixture {
    let (engine, mut commands, events) = AudioEngine::new();
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
    (engine, commands, events, track_id, effect_id)
}

fn render_window(engine: &mut AudioEngine, track: TrackId, amplitude: f32) -> [f32; 1920] {
    let input = [amplitude; 1920];
    let mut output = [0.0; 1920];
    engine.process_block(
        AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &input),
    );
    output
}

fn snapshots(
    events: &mut Consumer<EngineEvent>,
) -> Vec<(TrackId, EffectId, u64, vibez_core::effect::WaveShaperMeter)> {
    std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::EffectWaveshaper {
                track_id,
                effect_id,
                generation,
                meter,
            } => Some((track_id, effect_id, generation, meter)),
            _ => None,
        })
        .collect()
}

#[test]
fn drive_snapshots_clear_on_bypass_and_resume_with_fresh_selection_generation() {
    let (mut engine, mut commands, mut events, track, effect) = fixture();
    render_window(&mut engine, track, 0.5);
    assert_eq!(snapshots(&mut events)[0].3.count, 128);
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: None,
            generation: 8,
        })
        .unwrap();
    render_window(&mut engine, track, 0.9);
    assert!(snapshots(&mut events).is_empty());
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: Some(track),
            generation: 9,
        })
        .unwrap();
    render_window(&mut engine, track, 0.03);
    let fresh = snapshots(&mut events);
    assert_eq!(fresh.len(), 1);
    assert_eq!((fresh[0].0, fresh[0].1, fresh[0].2), (track, effect, 9));
    assert_eq!(fresh[0].3.count, 128);
    assert!(fresh[0].3.input.iter().all(|sample| (*sample - 0.03).abs() < 1e-6));
    assert!(fresh[0].3.output.iter().all(|sample| (*sample - 0.03).abs() < 1e-6));

    commands
        .push(EngineCommand::SetEffectBypass {
            track_id: track,
            effect_id: effect,
            bypass: true,
        })
        .unwrap();
    render_window(&mut engine, track, 0.8);
    let bypassed = snapshots(&mut events);
    assert_eq!(bypassed.len(), 1);
    assert_eq!(bypassed[0].2, 9);
    assert_eq!(bypassed[0].3.count, 0);

    commands
        .push(EngineCommand::SetEffectBypass {
            track_id: track,
            effect_id: effect,
            bypass: false,
        })
        .unwrap();
    render_window(&mut engine, track, 0.0);
    let silence = snapshots(&mut events);
    assert_eq!(silence.len(), 1);
    assert_eq!(silence[0].3.count, 128, "real silence is available data");
    assert!(silence[0].3.input.iter().all(|sample| *sample == 0.0));
    assert!(silence[0].3.output.iter().all(|sample| *sample == 0.0));

    commands.push(EngineCommand::Play).unwrap();
    render_window(&mut engine, track, 0.0);
    let reset = snapshots(&mut events);
    assert_eq!(reset.len(), 1);
    assert_eq!(reset[0].3.first_output_sample, 832);
    assert!(reset[0].3.input.iter().all(|sample| *sample == 0.0));
    assert!(reset[0].3.output.iter().all(|sample| *sample == 0.0));
}

#[test]
fn drive_waveform_publishes_real_chronological_pairs_at_fifty_hertz() {
    let (mut engine, _commands, mut events, track_id, effect_id) = fixture();
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
