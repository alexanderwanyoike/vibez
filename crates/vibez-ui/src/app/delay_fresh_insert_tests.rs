//! Verifies a fresh Delay is published atomically in its factory sound through App backpressure.

use super::*;
use crate::domains::{device_presets, devices::DevicesMsg, EngineCommandQueue};
use crate::state::ProjectTrack;
use vibez_core::{effect::EffectType, id::TrackId};
use vibez_dsp::delay::param;
use vibez_engine::engine::{AudioEngine, AudioProcessBlock};

#[test]
fn fresh_delay_carries_dotted_bounce_before_first_audio_even_when_the_ui_ring_is_full() {
    let rate = 48000;
    let expected = [
        500.0_f32, 0.35, 0.22, 1.0, 3.0, 1.0, 0.0, 1.0, 500.0, 3.0, 1.0, 1.0, 120.0, 1.0, 8000.0,
        0.2, 0.0,
    ];
    let (mut engine, mut native, mut events) = AudioEngine::new();
    let (producer, mut published) = rtrb::RingBuffer::new(1);
    let mut app = super::test_support::app();
    app.cmd_tx = EngineCommandQueue::new(producer);
    app.state.transport.sample_rate = rate;
    let track = ProjectTrack::new(TrackId::new(), "Fresh Delay".into(), 3);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    app.send_command(EngineCommand::SetSampleRate(rate));
    app.send_command(EngineCommand::add_track(track_id));
    let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
        track_id,
        EffectType::Gain,
    )));
    let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
        track_id,
        EffectType::Delay,
    )));
    assert!(
        app.cmd_tx.pending_len() >= 3,
        "the actual capacity1 ring blocks the prepared insertions"
    );
    let ui_track = app.state.find_track(track_id).unwrap();
    assert_eq!(ui_track.effects.len(), 2);
    let ids: Vec<_> = ui_track.effects.iter().map(|effect| effect.id).collect();
    let delay = &ui_track.effects[1];
    assert_eq!(delay.effect_type, EffectType::Delay);
    let ui_values = delay.params.clone();
    let ui_latency = delay.latency_samples;
    let selection = app
        .device_presets
        .selection
        .get(&(track_id, delay.id))
        .cloned();
    let mut insertions = Vec::new();
    let mut prepared = None;
    let mut late_parameter_commands = 0;
    let mut first = true;
    // The published owner is inspected off callback, then moved unchanged into the real engine.
    loop {
        app.cmd_tx.flush();
        let Ok(command) = published.pop() else {
            assert_eq!(app.cmd_tx.pending_len(), 0);
            break;
        };
        if first {
            assert!(matches!(&command, EngineCommand::SetSampleRate(48000)));
            first = false;
        }
        if let EngineCommand::InsertEffect {
            track_id: owner_track,
            effect_id,
            effect,
            position,
            ..
        } = &command
        {
            assert_eq!(*owner_track, track_id);
            assert!(position.is_none());
            insertions.push(*effect_id);
            if *effect_id == ids[1] {
                let values: [f32; param::COUNT] =
                    std::array::from_fn(|index| effect.get_param(index));
                prepared = Some((
                    values,
                    effect.activation_sample_rate(),
                    effect.latency_samples(),
                ));
            }
        }
        if matches!(&command, EngineCommand::SetEffectParam { effect_id, .. } if *effect_id == ids[1])
        {
            late_parameter_commands += 1;
        }
        native.push(command).unwrap();
    }
    assert_eq!(insertions, ids);
    assert!(
        engine.tracks().is_empty(),
        "the engine has not received its first callback"
    );
    let repeat = 3 * rate as usize / 8;
    let frames = rate as usize / 2 + 1;
    let mut input = [0.0_f32; 1024];
    let mut output = [0.0_f32; 1024];
    let (mut position, mut part, mut errors) = (0, 0, 0);
    let mut first_error = None;
    let (mut factory_arrival, mut legacy_arrival) = ([0.0; 2], [0.0; 2]);
    let parts = [1, 7, 31, 128, 512];
    while position < frames {
        let count = parts[part % parts.len()].min(frames - position);
        input[..2 * count].fill(0.0);
        if position == 0 {
            input[0] = 0.25;
        }
        engine.process_block(
            AudioProcessBlock::new(&mut output[..2 * count], 2)
                .with_live_input(track_id.raw(), &input[..2 * count]),
        );
        for (offset, pair) in output[..2 * count].chunks_exact(2).enumerate() {
            let at = position + offset;
            if at == repeat {
                factory_arrival = [pair[0], pair[1]];
            }
            if at == rate as usize / 2 {
                legacy_arrival = [pair[0], pair[1]];
            }
            let envelope = 0.25
                * (1.0 - (-1.0_f64 / (0.005 * f64::from(rate))).exp())
                * (-(at as f64) / (0.150 * f64::from(rate))).exp();
            let gain =
                10.0_f64.powf(-24.0 * f64::from(expected[param::DUCK]) * envelope / 0.25 / 20.0);
            let left = if at == 0 {
                0.25 * (1.0 - f64::from(expected[param::MIX]))
            } else if at == repeat {
                0.25 * f64::from(expected[param::MIX]) * gain
            } else {
                0.0
            };
            for (channel, &sample) in pair.iter().enumerate() {
                let wanted = if channel == 0 {
                    left * f64::from(std::f32::consts::FRAC_1_SQRT_2)
                } else {
                    0.0
                };
                if !sample.is_finite() || (f64::from(sample) - wanted).abs() > 2e-7 {
                    errors += 1;
                    first_error.get_or_insert((at, channel, wanted, sample));
                }
            }
        }
        while events.pop().is_ok() {}
        position += count;
        part += 1;
    }
    assert_eq!(errors, 0, "factory375ms vs legacy500ms audio: first={first_error:?}, at375={factory_arrival:?}, at500={legacy_arrival:?}");
    assert_eq!(ui_values, expected);
    assert_eq!(ui_latency, Some(0));
    assert_eq!(
        selection,
        Some(device_presets::factory_presets(EffectType::Delay)[1].clone())
    );
    let (values, prepared_rate, latency) = prepared.expect("a complete Delay owner was queued");
    assert_eq!(values.map(f32::to_bits), expected.map(f32::to_bits));
    assert_eq!(
        (prepared_rate, latency, late_parameter_commands),
        (Some(rate), 0, 0)
    );
    assert_eq!(
        engine.tracks()[0]
            .effects
            .iter()
            .map(|slot| slot.id)
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(engine.tracks()[0].effects[1].effect.latency_samples(), 0);
}
