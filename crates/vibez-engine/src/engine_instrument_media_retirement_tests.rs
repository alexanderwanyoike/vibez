//! Native sample commands return complete owners while preserving musical behavior.

use super::*;
use vibez_core::midi::InstrumentKind;

fn audio(value: f32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio { channels: vec![vec![value; 4096]], sample_rate: 1000 })
}

#[test]
fn actual_sampler_replacement_preserves_reset_params_and_returns_unique_media_off_callback() {
    for full_ring in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let original = audio(0.5);
        let owner = Arc::downgrade(&original);
        commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands.push(EngineCommand::set_built_in_instrument(track, InstrumentKind::Sampler, 1000)).unwrap();
        commands.push(EngineCommand::SetInstrumentParam { track_id: track, param_index: 9, value: 0.5 }).unwrap();
        commands.push(EngineCommand::LoadSamplerSample { track_id: track, sample: original, sample_name: "original-owned-sample.wav".into() }).unwrap();
        commands.push(EngineCommand::ExternalNoteOn { track_id: track, pitch: 60, velocity: 127 }).unwrap();
        let mut output = [0.0; 32];
        engine.process(&mut output, 2);
        let expected = 0.25 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(output.iter().all(|sample| (*sample - expected).abs() < 1e-6));
        while events.pop().is_ok() {}
        if full_ring {
            while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        }
        commands.push(EngineCommand::LoadSamplerSample { track_id: track, sample: audio(0.25), sample_name: "replacement-owned-sample.wav".into() }).unwrap();
        let measured = crate::retirement::tests::allocations(|| engine.process(&mut output, 2));
        assert_eq!(output, [0.0; 32]);
        let instrument = engine.tracks()[0].instrument.as_ref().unwrap();
        assert_eq!(instrument.sample_name(), Some("replacement-owned-sample.wav"));
        assert_eq!(instrument.get_param(9), 0.5);
        assert_eq!(measured, (0, 0), "full_ring={full_ring}");
        assert_eq!(owner.strong_count(), 1);
        while events.pop().is_ok() {}
        engine.process(&mut [], 2);
        while events.pop().is_ok() {}
        assert_eq!(owner.strong_count(), 0);
        commands.push(EngineCommand::ExternalNoteOn { track_id: track, pitch: 60, velocity: 127 }).unwrap();
        assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut output, 2)), (0, 0));
        let expected = 0.125 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(output.iter().all(|sample| (*sample - expected).abs() < 1e-6));
    }
}
