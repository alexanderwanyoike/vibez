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
                .push(EngineCommand::set_drum_rack_pad_state(
                    track,
                    pad,
                    vibez_core::track::DrumPadState {
                        fade_in_ms: 0.0,
                        fade_out_ms: 0.0,
                        ..Default::default()
                    },
                ))
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
            .push(EngineCommand::set_drum_rack_pad_state(
                track,
                0,
                vibez_core::track::DrumPadState {
                    fade_in_ms: 0.0,
                    fade_out_ms: 0.0,
                    ..Default::default()
                },
            ))
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
        commands
            .push(EngineCommand::set_built_in_instrument(
                track,
                InstrumentKind::DrumRack,
                1000,
            ))
            .unwrap();
        for pad in [0, 1] {
            commands
                .push(EngineCommand::set_drum_rack_pad_state(
                    track,
                    pad,
                    vibez_core::track::DrumPadState {
                        fade_in_ms: 0.0,
                        fade_out_ms: 0.0,
                        ..Default::default()
                    },
                ))
                .unwrap();
        }
        commands
            .push(EngineCommand::LoadDrumRackPadSample {
                track_id: track,
                pad_index: 0,
                sample: original,
                sample_name: name,
            })
            .unwrap();
        commands
            .push(EngineCommand::LoadDrumRackPadSample {
                track_id: track,
                pad_index: 1,
                sample: unrelated,
                sample_name: "unrelated-ramp.wav".into(),
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
        for (frame, sample) in output.iter().enumerate() {
            assert!((*sample - (0.5 + frame as f32 * 0.01)).abs() < 1e-6);
        }
        while events.pop().is_ok() {}
        if full_ring {
            while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        }
        commands
            .push(EngineCommand::ClearDrumRackPad {
                track_id: track,
                pad_index: 0,
            })
            .unwrap();
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
            engine.process(&mut [], 1);
        }
        assert_eq!(
            owner.strong_count(),
            1,
            "cleared voices no longer retain the sample"
        );
        drop(returned.expect("cleared pad owners returned"));
        assert_eq!(owner.strong_count(), 0);
        commands
            .push(EngineCommand::ExternalNoteOn {
                track_id: track,
                pitch: 36,
                velocity: 127,
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut output, 1)),
            (0, 0)
        );
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
        commands
            .push(EngineCommand::set_built_in_instrument(
                track,
                InstrumentKind::DrumRack,
                1000,
            ))
            .unwrap();
        commands
            .push(EngineCommand::set_drum_rack_pad_state(
                track,
                0,
                DrumPadState {
                    fade_in_ms: 0.0,
                    fade_out_ms: 0.0,
                    ..Default::default()
                },
            ))
            .unwrap();
        commands
            .push(EngineCommand::LoadDrumRackPadSample {
                track_id: track,
                pad_index: 0,
                sample: audio(0.5),
                sample_name: old_name,
            })
            .unwrap();
        commands
            .push(EngineCommand::ExternalNoteOn {
                track_id: track,
                pitch: 36,
                velocity: 127,
            })
            .unwrap();
        let mut output = [0.0; 16];
        engine.process(&mut output, 1);
        assert_eq!(output, [0.5; 16]);
        while events.pop().is_ok() {}
        let canonical = DrumPadState {
            source: Some(MediaSourceRef::LocalFile {
                path: "/sample-library/from-source.wav".into(),
            }),
            name: None,
            gain: 0.25,
            pan: -0.5,
            start: 0.25,
            end: 0.75,
            fade_in_ms: 0.0,
            fade_out_ms: 0.0,
            coarse_tune: 12,
            fine_tune: 25.0,
            one_shot: false,
            choke_group: None,
        };
        let unchanged = canonical.clone();
        commands
            .push(EngineCommand::set_drum_rack_pad_state(
                track,
                0,
                canonical.clone(),
            ))
            .unwrap();
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
                    assert_eq!(
                        owners.name.as_ref().map(|name| name.as_ptr()),
                        Some(old_name_pointer)
                    );
                    returned = Some(owners);
                }
            }
            engine.process(&mut [], 1);
        }
        drop(returned.expect("displaced pad name returned"));
        commands
            .push(EngineCommand::ExternalNoteOn {
                track_id: track,
                pitch: 36,
                velocity: 127,
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut output, 1)),
            (0, 0)
        );
        assert_eq!(output, [0.625; 16], "new voice adopts prepared gain");
        commands
            .push(EngineCommand::set_drum_rack_pad_state(
                track,
                0,
                DrumPadState::default(),
            ))
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [], 1)),
            (0, 0)
        );
        let names: Vec<_> = std::iter::from_fn(|| events.pop().ok())
            .filter_map(|event| match event {
                EngineEvent::RetiredInstrumentMedia(owners) => owners.name,
                _ => None,
            })
            .collect();
        assert_eq!(names, ["from-source.wav"]);
    }
}

#[test]
fn completed_or_released_old_voices_leave_the_returned_ui_guard_as_final_owner() {
    for note_off in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let original = audio(0.5);
        let weak = Arc::downgrade(&original);
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
            .push(EngineCommand::set_drum_rack_pad_state(
                track,
                0,
                vibez_core::track::DrumPadState {
                    fade_in_ms: 0.0,
                    fade_out_ms: 0.0,
                    one_shot: false,
                    ..Default::default()
                },
            ))
            .unwrap();
        commands
            .push(EngineCommand::LoadDrumRackPadSample {
                track_id: track,
                pad_index: 0,
                sample: original,
                sample_name: "old-playing-voice.wav".into(),
            })
            .unwrap();
        commands
            .push(EngineCommand::ExternalNoteOn {
                track_id: track,
                pitch: 36,
                velocity: 127,
            })
            .unwrap();
        engine.process(&mut [0.0; 16], 1);
        while events.pop().is_ok() {}
        commands
            .push(EngineCommand::LoadDrumRackPadSample {
                track_id: track,
                pad_index: 0,
                sample: audio(0.25),
                sample_name: "next-generation.wav".into(),
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 16], 1)),
            (0, 0)
        );
        let guard = std::iter::from_fn(|| events.pop().ok())
            .find_map(|event| match event {
                EngineEvent::RetiredInstrumentMedia(owners) => owners.sample,
                _ => None,
            })
            .expect("UI retains the returned old sample while a voice still reads it");
        assert_eq!(Arc::as_ptr(&guard), weak.as_ptr());
        assert_eq!(weak.strong_count(), 2);
        if note_off {
            commands
                .push(EngineCommand::ExternalNoteOff {
                    track_id: track,
                    pitch: 36,
                })
                .unwrap();
        }
        let mut output = [0.0; 4096];
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut output, 1)),
            (0, 0)
        );
        if note_off {
            assert_eq!(output, [0.0; 4096]);
        } else {
            assert_eq!(&output[..4064], &[0.5; 4064]);
            assert_eq!(&output[4064..], &[0.0; 32]);
        }
        assert_eq!(
            weak.strong_count(),
            1,
            "only the UI guard remains, note_off={note_off}"
        );
        drop(guard);
        assert_eq!(weak.strong_count(), 0);
        commands
            .push(EngineCommand::ExternalNoteOn {
                track_id: track,
                pitch: 36,
                velocity: 127,
            })
            .unwrap();
        let mut next = [0.0; 16];
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut next, 1)),
            (0, 0)
        );
        assert_eq!(next, [0.25; 16]);
    }
}

#[test]
fn full_event_ring_retains_thirty_two_media_owners_and_resumes_later_commands_in_fifo_order() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let sampler = TrackId::new();
    let drums = TrackId::new();
    let mut expected = Vec::new();
    let mut audio_owners = Vec::new();
    let mut record = |sample: Option<Arc<DecodedAudio>>, name: String| {
        if let Some(sample) = &sample {
            audio_owners.push(Arc::downgrade(sample));
        }
        expected.push((
            name.clone(),
            name.as_ptr(),
            sample.as_ref().map(Arc::as_ptr),
        ));
        (sample, name)
    };
    commands.push(EngineCommand::add_track(sampler)).unwrap();
    commands.push(EngineCommand::add_track(drums)).unwrap();
    for (track, kind) in [
        (sampler, InstrumentKind::Sampler),
        (drums, InstrumentKind::DrumRack),
    ] {
        commands
            .push(EngineCommand::set_built_in_instrument(track, kind, 1000))
            .unwrap();
    }
    let (sample, name) = record(Some(audio(0.25)), "sampler-0".into());
    commands
        .push(EngineCommand::LoadSamplerSample {
            track_id: sampler,
            sample: sample.unwrap(),
            sample_name: name,
        })
        .unwrap();
    let (sample, name) = record(Some(audio(0.5)), "drum-original".into());
    commands
        .push(EngineCommand::LoadDrumRackPadSample {
            track_id: drums,
            pad_index: 0,
            sample: sample.unwrap(),
            sample_name: name,
        })
        .unwrap();
    engine.process(&mut [], 1);
    while events.pop().is_ok() {}
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}

    for generation in 1..=30 {
        let sample = audio(0.25);
        let name = format!("sampler-{generation}");
        let (sample, name) = if generation < 30 {
            record(Some(sample), name)
        } else {
            (Some(sample), name)
        };
        commands
            .push(EngineCommand::LoadSamplerSample {
                track_id: sampler,
                sample: sample.unwrap(),
                sample_name: name,
            })
            .unwrap();
    }
    let (sample, clear_name) = record(Some(audio(0.75)), "state-after-32".into());
    let (_, name) = record(None, "drum-new".into());
    commands
        .push(EngineCommand::LoadDrumRackPadSample {
            track_id: drums,
            pad_index: 0,
            sample: sample.unwrap(),
            sample_name: name,
        })
        .unwrap();
    commands
        .push(EngineCommand::set_drum_rack_pad_state(
            drums,
            0,
            vibez_core::track::DrumPadState {
                name: Some(clear_name),
                ..Default::default()
            },
        ))
        .unwrap();
    commands
        .push(EngineCommand::ClearDrumRackPad {
            track_id: drums,
            pad_index: 0,
        })
        .unwrap();
    let (sample, name) = record(Some(audio(1.0)), "rejected-after-clear".into());
    commands
        .push(EngineCommand::LoadDrumRackPadSample {
            track_id: drums,
            pad_index: usize::MAX,
            sample: sample.unwrap(),
            sample_name: name,
        })
        .unwrap();
    commands.push(EngineCommand::SetBpm(137.0)).unwrap();
    drop(record);

    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [], 1)),
        (0, 0)
    );
    assert_eq!(engine.pending_retirements.len(), 32);
    assert!(matches!(
        engine.cmd_rx.peek(),
        Ok(EngineCommand::ClearDrumRackPad { .. })
    ));
    assert_ne!(engine.transport().bpm(), 137.0);
    assert!(audio_owners.iter().all(|owner| owner.strong_count() == 1));
    // A second blocked callback cannot partially execute the queued clear.
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [], 1)),
        (0, 0)
    );
    assert_eq!(engine.pending_retirements.len(), 32);
    assert!(matches!(
        engine.cmd_rx.peek(),
        Ok(EngineCommand::ClearDrumRackPad { .. })
    ));
    while let Ok(event) = events.pop() {
        assert!(!matches!(event, EngineEvent::RetiredInstrumentMedia(_)));
    }

    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [], 1)),
        (0, 0)
    );
    assert!(engine.cmd_rx.peek().is_err());
    assert!(engine.pending_retirements.is_empty());
    assert_eq!(engine.transport().bpm(), 137.0);
    assert_eq!(
        engine
            .tracks()
            .iter()
            .find(|track| track.id == sampler)
            .unwrap()
            .instrument
            .as_ref()
            .unwrap()
            .sample_name(),
        Some("sampler-30")
    );
    let mut returned = Vec::new();
    while let Ok(event) = events.pop() {
        if let EngineEvent::RetiredInstrumentMedia(owners) = event {
            returned.push(owners);
        }
    }
    assert_eq!(returned.len(), 34);
    for (name, name_pointer, sample_pointer) in expected {
        let matching: Vec<_> = returned
            .iter()
            .filter(|owners| owners.name.as_deref() == Some(name.as_str()))
            .collect();
        assert_eq!(matching.len(), 1, "owner {name} returns exactly once");
        assert_eq!(matching[0].name.as_ref().unwrap().as_ptr(), name_pointer);
        assert_eq!(matching[0].sample.as_ref().map(Arc::as_ptr), sample_pointer);
    }
    assert!(audio_owners.iter().all(|owner| owner.strong_count() == 1));
    drop(returned);
    assert!(audio_owners.iter().all(|owner| owner.strong_count() == 0));
}

#[test]
fn drum_media_commands_return_rejected_owners_without_touching_sounding_audio() {
    for target_case in 0..6 {
        for command_case in 0..3 {
            for full_ring in [false, true] {
                let (mut engine, mut commands, mut events) = AudioEngine::new();
                let sounding = TrackId::new();
                let other = TrackId::new();
                commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
                commands.push(EngineCommand::add_track(sounding)).unwrap();
                commands
                    .push(EngineCommand::set_built_in_instrument(
                        sounding,
                        InstrumentKind::DrumRack,
                        1000,
                    ))
                    .unwrap();
                commands
                    .push(EngineCommand::set_drum_rack_pad_state(
                        sounding,
                        0,
                        vibez_core::track::DrumPadState {
                            fade_in_ms: 0.0,
                            fade_out_ms: 0.0,
                            ..Default::default()
                        },
                    ))
                    .unwrap();
                commands
                    .push(EngineCommand::LoadDrumRackPadSample {
                        track_id: sounding,
                        pad_index: 0,
                        sample: Arc::new(DecodedAudio {
                            channels: vec![(0..4096)
                                .map(|frame| 0.1 + frame as f32 * 0.00001)
                                .collect()],
                            sample_rate: 1000,
                        }),
                        sample_name: "unrelated-playing-ramp.wav".into(),
                    })
                    .unwrap();
                commands
                    .push(EngineCommand::ExternalNoteOn {
                        track_id: sounding,
                        pitch: 36,
                        velocity: 127,
                    })
                    .unwrap();
                if (1..=3).contains(&target_case) {
                    commands.push(EngineCommand::add_track(other)).unwrap();
                }
                let wrong_kind = match target_case {
                    2 => Some(InstrumentKind::SubtractiveSynth),
                    3 => Some(InstrumentKind::Sampler),
                    _ => None,
                };
                if let Some(kind) = wrong_kind {
                    commands
                        .push(EngineCommand::set_built_in_instrument(other, kind, 1000))
                        .unwrap();
                }
                let mut output = [0.0; 16];
                engine.process(&mut output, 1);
                for (frame, actual) in output.iter().enumerate() {
                    assert!((*actual - (0.1 + frame as f32 * 0.00001)).abs() < 1e-6);
                }
                while events.pop().is_ok() {}
                if full_ring {
                    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
                }
                let track_id = if target_case >= 4 { sounding } else { other };
                let pad_index = match target_case {
                    4 => vibez_core::track::DRUM_RACK_PAD_COUNT,
                    5 => usize::MAX,
                    _ => 0,
                };
                let name = String::from("rejected-complete-owned-name.wav");
                let name_pointer = name.as_ptr();
                let mut sample_owner = None;
                let command = match command_case {
                    0 => {
                        let sample = audio(0.75);
                        sample_owner = Some(Arc::downgrade(&sample));
                        EngineCommand::LoadDrumRackPadSample {
                            track_id,
                            pad_index,
                            sample,
                            sample_name: name,
                        }
                    }
                    1 => EngineCommand::set_drum_rack_pad_state(
                        track_id,
                        pad_index,
                        vibez_core::track::DrumPadState {
                            name: Some(name),
                            gain: 0.25,
                            ..Default::default()
                        },
                    ),
                    _ => EngineCommand::ClearDrumRackPad {
                        track_id,
                        pad_index,
                    },
                };
                commands.push(command).unwrap();
                assert_eq!(
                    crate::retirement::tests::allocations(|| engine.process(&mut output, 1)),
                    (0, 0),
                    "target={target_case}, command={command_case}, full={full_ring}"
                );
                for (frame, actual) in output.iter().enumerate() {
                    assert!((*actual - (0.1 + (frame + 16) as f32 * 0.00001)).abs() < 1e-6);
                }
                let mut returned = None;
                for _ in 0..2 {
                    while let Ok(event) = events.pop() {
                        if let EngineEvent::RetiredInstrumentMedia(owners) = event {
                            assert!(returned.is_none());
                            returned = Some(owners);
                        }
                    }
                    assert_eq!(
                        crate::retirement::tests::allocations(|| engine.process(&mut [], 1)),
                        (0, 0)
                    );
                }
                if command_case == 2 {
                    assert!(returned.is_none(), "a rejected clear has no owned input");
                } else {
                    let returned = returned.expect("rejected owned arguments return intact");
                    assert_eq!(returned.name.as_ref().unwrap().as_ptr(), name_pointer);
                    assert_eq!(
                        returned.sample.as_ref().map(Arc::as_ptr),
                        sample_owner.as_ref().map(|owner| owner.as_ptr())
                    );
                    drop(returned);
                }
                if let Some(owner) = sample_owner {
                    assert_eq!(owner.strong_count(), 0);
                }
            }
        }
    }
}

#[test]
fn retired_voice_guards_cover_repeated_stealing_choke_release_and_reset_without_callback_destruction() {
    for boundary in ["steal", "choke", "reset"] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let old = audio(0.1);
        let weak = Arc::downgrade(&old);
        commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands.push(EngineCommand::set_built_in_instrument(track, InstrumentKind::DrumRack, 1000)).unwrap();
        for pad in 0..4 {
            commands.push(EngineCommand::set_drum_rack_pad_state(track, pad, vibez_core::track::DrumPadState {
                fade_in_ms: 0.0,
                fade_out_ms: if boundary == "choke" && pad == 0 { 3.0 } else { 0.0 },
                choke_group: if pad == 0 || pad == 3 { Some(7) } else { None },
                ..Default::default()
            })).unwrap();
        }
        for (pad_index, sample) in [
            (0, old),
            (1, Arc::new(DecodedAudio {
                channels: vec![(0..4096).map(|frame| 0.2 + frame as f32 * 0.00001).collect()],
                sample_rate: 1000,
            })),
            (2, audio(0.001)),
            (3, audio(0.0)),
        ] {
            commands.push(EngineCommand::LoadDrumRackPadSample {
                track_id: track, pad_index, sample, sample_name: format!("pad-{pad_index}.wav"),
            }).unwrap();
        }
        for pitch in [36, 37] {
            commands.push(EngineCommand::ExternalNoteOn { track_id: track, pitch, velocity: 127 }).unwrap();
        }
        if boundary == "steal" {
            for _ in 0..30 {
                commands.push(EngineCommand::ExternalNoteOn { track_id: track, pitch: 38, velocity: 127 }).unwrap();
            }
        }
        engine.process(&mut [0.0; 16], 1);
        while events.pop().is_ok() {}
        commands.push(EngineCommand::LoadDrumRackPadSample {
            track_id: track, pad_index: 0, sample: audio(0.05), sample_name: "next-generation.wav".into(),
        }).unwrap();
        assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut [], 1)), (0, 0));
        let guard = std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
            EngineEvent::RetiredInstrumentMedia(owners) => owners.sample,
            _ => None,
        }).expect("ordinary UI receipt retains the returned old sample");
        assert_eq!(Arc::as_ptr(&guard), weak.as_ptr());
        assert_eq!(weak.strong_count(), 2);
        let mut output = [0.0; 16];
        assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut output, 1)), (0, 0));
        let fillers = if boundary == "steal" { 0.03 } else { 0.0 };
        for (frame, actual) in output.iter().enumerate() {
            let expected = 0.3 + (frame + 16) as f32 * 0.00001 + fillers;
            assert!((*actual - expected).abs() < 1e-6, "{boundary}");
        }
        match boundary {
            "steal" => {
                // No event flush occurs between these notes; current pad and returned guard protect both generations.
                for _ in 0..65 {
                    commands.push(EngineCommand::ExternalNoteOn { track_id: track, pitch: 36, velocity: 127 }).unwrap();
                }
            }
            "choke" => {
                commands.push(EngineCommand::ExternalNoteOn { track_id: track, pitch: 39, velocity: 127 }).unwrap();
            }
            _ => {}
        }
        assert_eq!(crate::retirement::tests::allocations(|| {
            if boundary == "reset" {
                engine.tracks[0].instrument.as_mut().unwrap().reset();
            }
            engine.process(&mut output, 1);
        }), (0, 0), "{boundary}");
        for (frame, actual) in output.iter().enumerate() {
            let unrelated = 0.2 + (frame + 32) as f32 * 0.00001;
            let expected = match boundary {
                "steal" => unrelated + 0.03 + 0.05,
                "choke" => unrelated + [0.1, 0.05, 0.0].get(frame).copied().unwrap_or(0.0),
                _ => 0.0,
            };
            assert!((*actual - expected).abs() < 1e-6, "{boundary}, frame={frame}, actual={actual}, expected={expected}");
        }
        assert_eq!(weak.strong_count(), 1, "only returned UI guard remains, {boundary}");
        drop(guard);
        assert_eq!(weak.strong_count(), 0);
    }
}
