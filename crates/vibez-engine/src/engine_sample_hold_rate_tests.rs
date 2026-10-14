//! A host-rate request cannot relabel an old owner; recreation starts a new prepared epoch.

use super::*;
use std::sync::Arc;
use vibez_core::{audio_buffer::DecodedAudio, id::ClipId};
use vibez_dsp::factory::create_effect_for_instance;

const PARAMS: [f32; 9] = [8.25, 5.49, 0.65, 6.0, 1.0, 1.0, 0.75, 1.0, 700.0];

fn rate_window(fixture: &mut Fixture) -> [f32; 1920] {
    let mut input = [0.0; 1920];
    for frame in input.chunks_exact_mut(2) { frame.copy_from_slice(&[0.3, -0.19]); }
    let mut output = [0.0; 1920];
    fixture.engine.process_block(AudioProcessBlock::new(&mut output, 2)
        .with_live_input(fixture.source.raw(), &input));
    assert!(output.iter().all(|value| value.is_finite()));
    output
}

fn unavailable_rate_events(fixture: &mut Fixture) -> Observed {
    let mut invalid = 0;
    let mut observed = Observed::default();
    while let Ok(event) = fixture.events.pop() {
        match event {
            EngineEvent::CompensationInvalid { track_id, effect_id, reason } => {
                assert_eq!((track_id, effect_id), (fixture.selected, Some(fixture.effect)));
                assert!(reason.contains("sample rate"));
                invalid += 1;
            }
            EngineEvent::EffectSampleHold { track_id, effect_id, generation, meter } => {
                observed.scopes.push((track_id, effect_id, generation, meter));
            }
            EngineEvent::EffectMeter { track_id, effect_id, generation, input, output } => {
                observed.levels.push((track_id, effect_id, generation, input, output));
            }
            EngineEvent::CompensationFailed { .. } | EngineEvent::DeviceProcessingFailed { .. } => {
                panic!("unexpected failure kind: {event:?}");
            }
            _ => {}
        }
    }
    assert!(invalid > 0, "the real owner/rate mismatch needs a named invalidation");
    assert_eq!(observed.scopes.len(), 1);
    for &(track, effect, generation, meter) in &observed.scopes {
        assert_eq!((track, effect, generation), (fixture.selected, fixture.effect, 71));
        assert_eq!(meter.count, 0, "an old-rate owner must not look actively available");
        assert!(meter.input.iter().chain(&meter.output).all(|&value| value == 0.0));
    }
    assert!(!fixture.engine.compensation_valid);
    assert!(!fixture.engine.transport.is_playing());
    let slot = &fixture.engine.tracks[0].effects[0];
    assert_eq!(slot.id, fixture.effect);
    assert_eq!(slot.effect.activation_sample_rate(), Some(48_000));
    for (index, value) in PARAMS.into_iter().enumerate() {
        assert_eq!(slot.effect.get_param(index), value);
    }
    observed
}

#[test]
fn a_rate_change_keeps_old_native_scope_unavailable_until_same_id_complete_recreation() {
    let mut fixture = Fixture::new(true, Owner::Track);
    for (param_index, value) in PARAMS.into_iter().enumerate() {
        fixture.commands.push(EngineCommand::SetEffectParam { track_id: fixture.selected,
            effect_id: fixture.effect, param_index, value }).unwrap();
    }
    fixture.commands.push(EngineCommand::AddClip {
        track_id: fixture.source, clip_id: ClipId::new(),
        audio: Arc::new(DecodedAudio { channels: vec![vec![0.0; 16_000]; 2], sample_rate: 48_000 }),
        position: 0, source_offset: 0, start_marker: 0, duration: 16_000,
        loop_enabled: false, loop_start: 0, loop_end: 0, linear_gain: 1.0,
        fades: Default::default(), playback_direction: Default::default(), warp_markers: Default::default(),
    }).unwrap();
    fixture.select(true, 71);
    fixture.commands.push(EngineCommand::Play).unwrap();
    let active = rate_window(&mut fixture);
    assert!(active.iter().any(|value| value.abs() > 1e-4));
    let old = fixture.observe();
    assert_eq!(old.scopes.len(), 1);
    assert_eq!((old.scopes[0].3.count, old.scopes[0].3.sample_rate), (128, 48_000.0));
    assert!(fixture.engine.transport.is_playing());
    let retained = [active[active.len() - 2], active[active.len() - 1]];
    let saved_id = serde_json::to_string(&fixture.effect).unwrap();
    fixture.commands.push(EngineCommand::SetSampleRate(96_000)).unwrap();
    let unavailable = rate_window(&mut fixture);
    for (frame, stereo) in unavailable.chunks_exact(2).enumerate() {
        for channel in 0..2 {
            let expected = if frame < 64 {
                retained[channel] * ((64 - frame) as f32 / 64.0)
            } else {
                0.0
            };
            assert_eq!(stereo[channel], expected,
                "rate invalidation loss ramp at frame {frame}, channel {channel}");
        }
    }
    assert!(rate_window(&mut fixture).iter().all(|&value| value == 0.0));
    unavailable_rate_events(&mut fixture);
    fixture.publish_routing(Some(fixture.effect));
    fixture.commands.push(EngineCommand::Play).unwrap();
    // Play preserves the already-muted gate, so it cannot rearm the completed loss ramp.
    for _ in 0..2 {
        assert!(rate_window(&mut fixture).iter().all(|&value| value == 0.0));
    }
    unavailable_rate_events(&mut fixture);
    let restored: EffectId = serde_json::from_str(&saved_id).unwrap();
    assert_eq!(restored, fixture.effect);
    let prepared = create_effect_for_instance(EffectType::Bitcrush, 96_000.0, &PARAMS, restored);
    assert_eq!(prepared.activation_sample_rate(), Some(96_000));
    assert_eq!(prepared.latency_samples(), 0);
    for (index, value) in PARAMS.into_iter().enumerate() { assert_eq!(prepared.get_param(index), value); }
    fixture.commands.push(EngineCommand::RemoveEffect(fixture.selected, fixture.effect)).unwrap();
    fixture.commands.push(EngineCommand::InsertEffect { track_id: fixture.selected,
        effect_id: restored, effect: prepared, position: None }).unwrap();
    fixture.publish_routing(Some(restored));
    fixture.select(true, 72);
    fixture.commands.push(EngineCommand::Play).unwrap();
    let first = rate_window(&mut fixture);
    assert!(first.iter().any(|value| value.abs() > 1e-4));
    assert!(fixture.observe().scopes.is_empty(), "96 kHz deadline is1920 source frames");
    let second = rate_window(&mut fixture);
    assert!(second.iter().any(|value| value.abs() > 1e-4));
    let fresh = fixture.observe();
    assert!(fixture.engine.compensation_valid);
    assert!(fixture.engine.transport.is_playing());
    assert_eq!(fresh.scopes.len(), 1);
    assert_eq!(fresh.levels.len(), 1);
    let (track, effect, generation, meter) = fresh.scopes[0];
    assert_eq!((track, effect, generation), (fixture.selected, restored, 72));
    assert_eq!((meter.count, meter.sample_rate, meter.first_source_frame, meter.latency_samples),
        (128, 96_000.0, 1_792, 0));
    // This comparison checks recreation/dispatch, after independent DSP seed and response proofs.
    let mut reference = create_effect_for_instance(EffectType::Bitcrush, 96_000.0, &PARAMS, restored);
    for _ in 0..2 {
        let mut input = [0.0; 1920];
        for frame in input.chunks_exact_mut(2) { frame.copy_from_slice(&[0.3, -0.19]); }
        reference.process(&mut input, 2);
    }
    assert_eq!(meter, reference.take_sample_hold_meter().unwrap(),
        "a correctly re-prepared same-ID owner starts from source0 at its actual new rate");
    let slot = &fixture.engine.tracks[0].effects[0];
    assert_eq!(slot.id, restored);
    assert_eq!(slot.effect.activation_sample_rate(), Some(96_000));
    for (index, value) in PARAMS.into_iter().enumerate() { assert_eq!(slot.effect.get_param(index), value); }
}
