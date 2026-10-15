//! Checks musical Delay timing and stereo feedback through the actual routed engine.

use vibez_core::{id::{EffectId, TrackId}, routing::{RoutingChannel, RoutingEffect}};
use vibez_dsp::delay::param;
use vibez_engine::{commands::EngineCommand, engine::{AudioEngine, AudioProcessBlock}, routing::PreparedRouting};

#[test]
fn sync_uses_engine_tempo_and_preserves_ping_pong_repeat_positions() {
    for rate in [1000, 48000] {
        for change_tempo in [false, true] {
            let (mut engine, mut commands, mut events) = AudioEngine::new();
            let track = TrackId::new();
            let effect = EffectId::new();
            commands.push(EngineCommand::SetSampleRate(rate)).unwrap();
            commands.push(EngineCommand::SetBpm(20.0)).unwrap();
            commands.push(EngineCommand::add_track(track)).unwrap();
            commands.push(EngineCommand::insert_built_in_effect(
                track, effect, vibez_core::effect::EffectType::Delay, rate,
            )).unwrap();
            for (index, value) in [(param::SYNC, 1.0), (param::LEFT_DIVISION, 3.0),
                (param::LEFT_MODIFIER, 1.0), (param::ROUTING, 1.0),
                (param::FEEDBACK, 0.5), (param::MIX, 1.0)] {
                commands.push(EngineCommand::SetEffectParam {
                    track_id: track, effect_id: effect, param_index: index, value,
                }).unwrap();
            }
            let routing = PreparedRouting::prepare(&[
                RoutingChannel { id: track, is_bus: false, sends: vec![], effects: vec![RoutingEffect {
                    id: effect, inputs: vec![], assignments: vec![], inactive_inputs: vec![],
                }] },
                RoutingChannel { id: TrackId::MASTER, is_bus: true, sends: vec![], effects: vec![] },
            ], 512).unwrap();
            commands.push(EngineCommand::SetRouting(routing)).unwrap();
            let bpm = if change_tempo { 40.0 } else { 20.0 };
            // Dotted eighth at20BPM exceeds the old Free-time limit, without becoming PDC.
            let distance = (rate as f64 * 60.0 / bpm * 0.5 * 1.5) as usize;
            let frames = 3 * distance + 2;
            let mut input = [0.0_f32; 1024];
            let mut output = [0.0_f32; 1024];
            let mut position = 0;
            let mut part = 0;
            let mut repeats = 0;
            let parts = [1, 7, 31, 128, 512];
            while position < frames {
                if position == 1 && change_tempo {
                    commands.push(EngineCommand::SetBpm(bpm)).unwrap();
                }
                let count = parts[part % parts.len()].min(frames - position);
                input[..2 * count].fill(0.0);
                if position == 0 { input[0] = 0.25; }
                engine.process_block(AudioProcessBlock::new(&mut output[..2 * count], 2)
                    .with_live_input(track.raw(), &input[..2 * count]));
                for (offset, pair) in output[..2 * count].chunks_exact(2).enumerate() {
                    let at = position + offset;
                    let repeat = (1..=3).find(|repeat| at == repeat * distance);
                    for (channel, &actual) in pair.iter().enumerate() {
                        let expected = repeat.filter(|repeat| channel == (repeat - 1) % 2)
                            .map_or(0.0, |repeat| 0.25 * 0.5_f32.powi(repeat as i32 - 1)
                                * std::f32::consts::FRAC_1_SQRT_2);
                        assert!((actual - expected).abs() <= 2e-7,
                            "rate={rate}, tempo_change={change_tempo}, frame={at}, channel={channel}: actual={actual}, expected={expected}");
                    }
                    repeats += usize::from(repeat.is_some());
                }
                while events.pop().is_ok() {}
                position += count;
                part += 1;
            }
            assert_eq!(repeats, 3);
            let slot = &engine.tracks()[0].effects[0];
            assert_eq!(slot.id, effect);
            assert_eq!(slot.effect.latency_samples(), 0);
            assert_eq!(slot.effect.activation_sample_rate(), Some(rate));
            assert_eq!(engine.transport().bpm(), bpm);
            println!("DELAY_ENGINE_TEMPO rate={rate} changed={change_tempo} bpm={bpm} distance={distance} repeats={repeats} latency=0");
        }
    }
}
