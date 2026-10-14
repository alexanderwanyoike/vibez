//! Native sample commands return complete owners while preserving musical behavior.

use super::*;
use vibez_core::midi::InstrumentKind;

fn audio(value: f32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![value; 4096]],
        sample_rate: 1000,
    })
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
        commands
            .push(EngineCommand::set_built_in_instrument(
                track,
                InstrumentKind::Sampler,
                1000,
            ))
            .unwrap();
        commands
            .push(EngineCommand::SetInstrumentParam {
                track_id: track,
                param_index: 9,
                value: 0.5,
            })
            .unwrap();
        commands
            .push(EngineCommand::LoadSamplerSample {
                track_id: track,
                sample: original,
                sample_name: "original-owned-sample.wav".into(),
            })
            .unwrap();
        commands
            .push(EngineCommand::ExternalNoteOn {
                track_id: track,
                pitch: 60,
                velocity: 127,
            })
            .unwrap();
        let mut output = [0.0; 32];
        engine.process(&mut output, 2);
        let expected = 0.25 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(output
            .iter()
            .all(|sample| (*sample - expected).abs() < 1e-6));
        while events.pop().is_ok() {}
        if full_ring {
            while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        }
        commands
            .push(EngineCommand::LoadSamplerSample {
                track_id: track,
                sample: audio(0.25),
                sample_name: "replacement-owned-sample.wav".into(),
            })
            .unwrap();
        let measured = crate::retirement::tests::allocations(|| engine.process(&mut output, 2));
        assert_eq!(output, [0.0; 32]);
        let instrument = engine.tracks()[0].instrument.as_ref().unwrap();
        assert_eq!(
            instrument.sample_name(),
            Some("replacement-owned-sample.wav")
        );
        assert_eq!(instrument.get_param(9), 0.5);
        assert_eq!(measured, (0, 0), "full_ring={full_ring}");
        assert_eq!(owner.strong_count(), 1);
        while events.pop().is_ok() {}
        engine.process(&mut [], 2);
        while events.pop().is_ok() {}
        assert_eq!(owner.strong_count(), 0);
        commands
            .push(EngineCommand::ExternalNoteOn {
                track_id: track,
                pitch: 60,
                velocity: 127,
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
            (0, 0)
        );
        let expected = 0.125 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(output
            .iter()
            .all(|sample| (*sample - expected).abs() < 1e-6));
    }
}

#[test]
fn unsupported_and_missing_sampler_targets_return_exact_incoming_owners() {
    for kind in [
        None,
        Some(InstrumentKind::SubtractiveSynth),
        Some(InstrumentKind::DrumRack),
    ] {
        for missing_track in [false, true] {
            for full_ring in [false, true] {
                let (mut engine, mut commands, mut events) = AudioEngine::new();
                let track = TrackId::new();
                commands.push(EngineCommand::add_track(track)).unwrap();
                if let Some(kind) = kind {
                    commands
                        .push(EngineCommand::set_built_in_instrument(track, kind, 44_100))
                        .unwrap();
                }
                engine.process(&mut [], 2);
                while events.pop().is_ok() {}
                if full_ring {
                    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
                }
                let sample = audio(0.75);
                let owner = Arc::downgrade(&sample);
                let name = String::from("unsupported-unique-owned-name.wav");
                let name_pointer = name.as_ptr();
                commands
                    .push(EngineCommand::LoadSamplerSample {
                        track_id: if missing_track { TrackId::new() } else { track },
                        sample,
                        sample_name: name,
                    })
                    .unwrap();
                assert_eq!(
                    crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 16], 2)),
                    (0, 0)
                );
                assert_eq!(owner.strong_count(), 1);
                let mut returned = None;
                for _ in 0..2 {
                    while let Ok(event) = events.pop() {
                        if let EngineEvent::RetiredInstrumentMedia(owners) = event {
                            assert!(returned.is_none());
                            assert_eq!(
                                owners.sample.as_ref().map(Arc::as_ptr),
                                Some(owner.as_ptr())
                            );
                            assert_eq!(
                                owners.name.as_ref().map(|name| name.as_ptr()),
                                Some(name_pointer)
                            );
                            returned = Some(owners);
                        }
                    }
                    assert_eq!(
                        crate::retirement::tests::allocations(|| engine.process(&mut [], 2)),
                        (0, 0)
                    );
                }
                drop(returned.expect("complete rejected owners return to UI"));
                assert_eq!(owner.strong_count(), 0);
            }
        }
    }
}

#[test]
fn drum_sample_replacement_returns_old_owners_and_keeps_sounding_pad_voices() {
    for full_ring in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let original = audio(0.5);
        let owner = Arc::downgrade(&original);
        commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands
            .push(EngineCommand::set_built_in_instrument(
                track,
                InstrumentKind::DrumRack,
                1000,
            ))
            .unwrap();
        for pad in [0, 1] {
            commands
                .push(EngineCommand::set_drum_rack_pad_state(track, pad, vibez_core::track::DrumPadState {
                        fade_in_ms: 0.0,
                        fade_out_ms: 0.0,
                        ..Default::default()
                    }))
                .unwrap();
        }
        commands
            .push(EngineCommand::LoadDrumRackPadSample {
                track_id: track,
                pad_index: 0,
                sample: original,
                sample_name: "sounding-original-pad.wav".into(),
            })
            .unwrap();
        commands
            .push(EngineCommand::LoadDrumRackPadSample {
                track_id: track,
                pad_index: 1,
                sample: audio(0.125),
                sample_name: "unrelated-pad.wav".into(),
            })
            .unwrap();
        for pitch in [36, 37] {
            commands
                .push(EngineCommand::ExternalNoteOn {
                    track_id: track,
                    pitch,
                    velocity: 127,
                })
                .unwrap();
        }
        let mut output = [0.0; 16];
        engine.process(&mut output, 1);
        assert_eq!(output, [0.625; 16]);
        while events.pop().is_ok() {}
        if full_ring {
            while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        }
        commands
            .push(EngineCommand::LoadDrumRackPadSample {
                track_id: track,
                pad_index: 0,
                sample: audio(0.25),
                sample_name: "replacement-pad.wav".into(),
            })
            .unwrap();
        let measured = crate::retirement::tests::allocations(|| engine.process(&mut output, 1));
        assert_eq!(output, [0.625; 16]);
        assert_eq!(measured, (0, 0), "full_ring={full_ring}");
        assert_eq!(owner.strong_count(), 2);
        let mut returned = None;
        for _ in 0..2 {
            while let Ok(event) = events.pop() {
                if let EngineEvent::RetiredInstrumentMedia(owners) = event {
                    assert!(returned.is_none());
                    assert_eq!(
                        owners.sample.as_ref().map(Arc::as_ptr),
                        Some(owner.as_ptr())
                    );
                    assert_eq!(owners.name.as_deref(), Some("sounding-original-pad.wav"));
                    returned = Some(owners);
                }
            }
            engine.process(&mut [], 1);
        }
        drop(returned.expect("displaced pad owners returned"));
        assert_eq!(owner.strong_count(), 1);
        commands
            .push(EngineCommand::ExternalNoteOn {
                track_id: track,
                pitch: 36,
                velocity: 127,
            })
            .unwrap();
        engine.process(&mut output, 1);
        assert_eq!(output, [0.875; 16]);
    }
}

#[test]
fn first_drum_notes_do_not_destroy_dummy_voice_allocations_in_callback() {
    for count in [1, 32] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands
            .push(EngineCommand::set_built_in_instrument(
                track,
                InstrumentKind::DrumRack,
                1000,
            ))
            .unwrap();
        commands
            .push(EngineCommand::set_drum_rack_pad_state(track, 0, vibez_core::track::DrumPadState {
                    fade_in_ms: 0.0,
                    fade_out_ms: 0.0,
                    ..Default::default()
                }))
            .unwrap();
        commands
            .push(EngineCommand::LoadDrumRackPadSample {
                track_id: track,
                pad_index: 0,
                sample: audio(0.125),
                sample_name: "first-note-pad.wav".into(),
            })
            .unwrap();
        engine.process(&mut [0.0; 16], 1);
        while events.pop().is_ok() {}
        for _ in 0..count {
            commands
                .push(EngineCommand::ExternalNoteOn {
                    track_id: track,
                    pitch: 36,
                    velocity: 127,
                })
                .unwrap();
        }
        let mut output = [0.0; 16];
        let measured = crate::retirement::tests::allocations(|| engine.process(&mut output, 1));
        assert_eq!(output, [0.125 * count as f32; 16]);
        assert_eq!(measured, (0, 0), "first voices={count}");
    }
}

#[test]
fn clearing_populated_pad_returns_owners_and_preserves_unrelated_voice_phase() {
    for full_ring in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let original = audio(0.5);
        let owner = Arc::downgrade(&original);
        let name = String::from("cleared-selected-pad-owned-name.wav");
        let name_pointer = name.as_ptr();
        let unrelated = Arc::new(DecodedAudio {
            channels: vec![(0..4096).map(|frame| (frame % 64) as f32 * 0.01).collect()],
            sample_rate: 1000,
        });
        commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands.push(EngineCommand::set_built_in_instrument(track, InstrumentKind::DrumRack, 1000)).unwrap();
        for pad in [0, 1] {
            commands.push(EngineCommand::set_drum_rack_pad_state(track, pad, vibez_core::track::DrumPadState { fade_in_ms: 0.0, fade_out_ms: 0.0, ..Default::default() })).unwrap();
        }
        commands.push(EngineCommand::LoadDrumRackPadSample { track_id: track, pad_index: 0,
            sample: original, sample_name: name }).unwrap();
        commands.push(EngineCommand::LoadDrumRackPadSample { track_id: track, pad_index: 1,
            sample: unrelated, sample_name: "unrelated-ramp.wav".into() }).unwrap();
        for pitch in [36, 37] {
            commands.push(EngineCommand::ExternalNoteOn { track_id: track, pitch, velocity: 127 }).unwrap();
        }
        let mut output = [0.0; 16];
        engine.process(&mut output, 1);
        for (frame, sample) in output.iter().enumerate() {
            assert!((*sample - (0.5 + frame as f32 * 0.01)).abs() < 1e-6);
        }
        while events.pop().is_ok() {}
        if full_ring {
            while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        }
        commands.push(EngineCommand::ClearDrumRackPad { track_id: track, pad_index: 0 }).unwrap();
        let measured = crate::retirement::tests::allocations(|| engine.process(&mut output, 1));
        for (frame, sample) in output.iter().enumerate() {
            assert!((*sample - (frame + 16) as f32 * 0.01).abs() < 1e-6);
        }
        assert_eq!(measured, (0, 0), "full_ring={full_ring}");
        let mut returned = None;
        for _ in 0..2 {
            while let Ok(event) = events.pop() {
                if let EngineEvent::RetiredInstrumentMedia(owners) = event {
                    assert!(returned.is_none());
                    assert_eq!(owners.sample.as_ref().map(Arc::as_ptr), Some(owner.as_ptr()));
                    assert_eq!(owners.name.as_ref().map(|name| name.as_ptr()), Some(name_pointer));
                    returned = Some(owners);
                }
            }
            engine.process(&mut [], 1);
        }
        assert_eq!(owner.strong_count(), 1, "cleared voices no longer retain the sample");
        drop(returned.expect("cleared pad owners returned"));
        assert_eq!(owner.strong_count(), 0);
        commands.push(EngineCommand::ExternalNoteOn { track_id: track, pitch: 36, velocity: 127 }).unwrap();
        assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut output, 1)), (0, 0));
        for (frame, sample) in output.iter().enumerate() {
            assert!((*sample - (frame + 32) as f32 * 0.01).abs() < 1e-6);
        }
    }
}

#[test]
fn pad_state_name_and_source_preparation_preserves_audio_and_canonical_metadata() {
    use vibez_core::track::{DrumPadState, MediaSourceRef};
    for full_ring in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let old_name = String::from("old-owned-display-name.wav");
        let old_name_pointer = old_name.as_ptr();
        commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands.push(EngineCommand::set_built_in_instrument(track, InstrumentKind::DrumRack, 1000)).unwrap();
        commands.push(EngineCommand::set_drum_rack_pad_state(track, 0, DrumPadState { fade_in_ms: 0.0, fade_out_ms: 0.0, ..Default::default() })).unwrap();
        commands.push(EngineCommand::LoadDrumRackPadSample { track_id: track, pad_index: 0,
            sample: audio(0.5), sample_name: old_name }).unwrap();
        commands.push(EngineCommand::ExternalNoteOn { track_id: track, pitch: 36, velocity: 127 }).unwrap();
        let mut output = [0.0; 16];
        engine.process(&mut output, 1);
        assert_eq!(output, [0.5; 16]);
        while events.pop().is_ok() {}
        let canonical = DrumPadState { source: Some(MediaSourceRef::LocalFile { path: "/sample-library/from-source.wav".into() }),
            name: None, gain: 0.25, pan: -0.5, start: 0.25, end: 0.75,
            fade_in_ms: 0.0, fade_out_ms: 0.0, coarse_tune: 12, fine_tune: 25.0,
            one_shot: false, choke_group: None };
        let unchanged = canonical.clone();
        commands.push(EngineCommand::set_drum_rack_pad_state(track, 0, canonical.clone())).unwrap();
        if full_ring {
            while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        }
        let measured = crate::retirement::tests::allocations(|| engine.process(&mut output, 1));
        assert_eq!(output, [0.5; 16], "active voice retains captured settings");
        assert_eq!(canonical, unchanged);
        assert_eq!(measured, (0, 0), "full_ring={full_ring}");
        let mut returned = None;
        for _ in 0..2 {
            while let Ok(event) = events.pop() {
                if let EngineEvent::RetiredInstrumentMedia(owners) = event {
                    assert!(returned.is_none());
                    assert!(owners.sample.is_none());
                    assert_eq!(owners.name.as_ref().map(|name| name.as_ptr()), Some(old_name_pointer));
                    returned = Some(owners);
                }
            }
            engine.process(&mut [], 1);
        }
        drop(returned.expect("displaced pad name returned"));
        commands.push(EngineCommand::ExternalNoteOn { track_id: track, pitch: 36, velocity: 127 }).unwrap();
        assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut output, 1)), (0, 0));
        assert_eq!(output, [0.625; 16], "new voice adopts prepared gain");
        commands.push(EngineCommand::set_drum_rack_pad_state(track, 0, DrumPadState::default())).unwrap();
        assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut [], 1)), (0, 0));
        let names: Vec<_> = std::iter::from_fn(|| events.pop().ok()).filter_map(|event| match event {
            EngineEvent::RetiredInstrumentMedia(owners) => owners.name,
            _ => None,
        }).collect();
        assert_eq!(names, ["from-source.wav"]);
    }
}
