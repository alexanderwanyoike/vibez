//! Running monitoring changes retain audio already waiting on direct paths.

use super::*;
use crate::test_support::DelayProbe;
use vibez_core::id::{ClipId, EffectId};
use vibez_core::routing::*;

#[test]
fn running_reduced_monitoring_changes_never_refill_or_drop_constant_audio() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let dry = TrackId::new();
    let delayed = TrackId::new();
    let effect = EffectId::new();
    let channels: Vec<_> = [dry, delayed, TrackId::MASTER]
        .into_iter()
        .map(|id| RoutingChannel {
            id,
            is_bus: id.is_master(),
            sends: vec![],
            effects: if id == delayed {
                vec![RoutingEffect {
                    id: effect,
                    inputs: vec![],
                    assignments: vec![],
                    inactive_inputs: vec![],
                }]
            } else {
                vec![]
            },
        })
        .collect();
    let reports = [(
        RoutingNode {
            channel: delayed,
            stage: NodeStage::Effect(effect),
        },
        521,
    )];
    for id in [dry, delayed] {
        commands
            .push(EngineCommand::AddTrack(id, "Constant".into()))
            .unwrap();
        commands
            .push(EngineCommand::AddClip {
                track_id: id,
                clip_id: ClipId::new(),
                audio: Arc::new(DecodedAudio {
                    channels: vec![vec![0.25; 8192]; 2],
                    sample_rate: 44100,
                }),
                position: 0,
                source_offset: 0,
                start_marker: 0,
                duration: 8192,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                linear_gain: 1.0,
                fades: Default::default(),
                playback_direction: Default::default(),
                warp_markers: Default::default(),
            })
            .unwrap();
    }
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: delayed,
            effect_id: effect,
            effect: Box::new(DelayProbe::with_report(521, 521)),
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&channels, 512, &reports, &[], 1)
                .unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 1024];
    for _ in 0..4 {
        engine.process(&mut output, 2);
        while events.pop().is_ok() {}
    }
    let steady = output[1023];
    assert!(steady > 0.1);
    for (generation, reduced) in [(2, vec![dry]), (3, vec![]), (4, vec![dry]), (5, vec![])] {
        let plan = crate::routing::PreparedRouting::prepare_compensated(
            &channels, 512, &reports, &reduced, generation,
        )
        .unwrap();
        commands.push(EngineCommand::SetRouting(plan)).unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
            (0, 0)
        );
        assert!(output.iter().all(|&sample| (sample - steady).abs() < 1e-7));
        assert_eq!(engine.compensation_transition_frames, 0);
        assert!(engine.transport.is_playing());
        while events.pop().is_ok() {}
    }
}

#[test]
fn incompatible_wait_preparation_has_bounded_fades_instead_of_steps() {
    let (mut engine, _, _) = AudioEngine::new();
    engine.last_mix_frame = [0.5; 2];
    engine.compensation_transition_frames = 137;
    engine.compensation_fade_out = 64;
    engine.compensation_fade_in = 64;
    let mut output = [0.5; 512];
    engine.apply_compensation_transition(&mut output, 2, false);
    assert_eq!(output[0], 0.5);
    assert!(output
        .chunks_exact(2)
        .map(|frame| frame[0])
        .collect::<Vec<_>>()
        .windows(2)
        .all(|pair| (pair[1] - pair[0]).abs() <= 0.5 / 64.0));
    assert_eq!(output[510], 0.5);
}

#[test]
fn reduced_repeat_coordinates_follow_arrange_history_across_loop_wrap() {
    use crate::test_support::DelayProbeInstrument;
    use vibez_core::perform::NoteRepeatRate;
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    engine.sample_rate = 100;
    engine.transport.set_bpm(60.0);
    engine.transport.set_loop_region(0, 512);
    engine.transport.set_loop_enabled(true);
    let dry = TrackId::new();
    let delayed = TrackId::new();
    let effect = EffectId::new();
    let mut dry_track = EngineTrack::new(dry);
    dry_track.instrument = Some(Box::new(DelayProbeInstrument::new(0, 0.1)));
    engine.tracks.push(dry_track);
    let mut delayed_track = EngineTrack::new(delayed);
    delayed_track.effects.push(EffectSlot {
        id: effect,
        bypass: false,
        effect: Box::new(DelayProbe::new(137)),
    });
    engine.tracks.push(delayed_track);
    let channels: Vec<_> = [dry, delayed, TrackId::MASTER]
        .into_iter()
        .map(|id| RoutingChannel {
            id,
            is_bus: id.is_master(),
            sends: vec![],
            effects: if id == delayed {
                vec![RoutingEffect {
                    id: effect,
                    inputs: vec![],
                    assignments: vec![],
                    inactive_inputs: vec![],
                }]
            } else {
                vec![]
            },
        })
        .collect();
    let reports = [(
        RoutingNode {
            channel: delayed,
            stage: NodeStage::Effect(effect),
        },
        137,
    )];
    engine.routing = Some(
        crate::routing::PreparedRouting::prepare_compensated(&channels, 512, &reports, &[dry], 1)
            .unwrap(),
    );
    engine.transport.play();
    engine.process(&mut [0.0; 1000], 2);
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 1,
            track_id: dry,
            pitch: 60,
            velocity: 100,
            rate: NoteRepeatRate::Sixteenth,
        })
        .unwrap();
    engine.process(&mut [0.0; 64], 2);
    let mut heard = Vec::new();
    let mut raw = Vec::new();
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::SourceNoteRepeated { position, .. } => {
                raw.push(position.effective_at_samples)
            }
            EngineEvent::NoteRepeated {
                effective_at_samples,
                ..
            } => heard.push(effective_at_samples),
            _ => {}
        }
    }
    assert!(
        raw.contains(&0),
        "repeat delivery must cross the source loop boundary: {raw:?}"
    );
    assert!(
        heard.contains(&375),
        "heard delivery must retain the preceding loop coordinate: {heard:?}"
    );
    assert!(!heard.contains(&0));
}

#[test]
fn midrun_bypass_and_unbypass_keep_the_current_delayed_audio_without_stale_wet_input() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    commands
        .push(EngineCommand::AddTrack(track, "Ramp".into()))
        .unwrap();
    let signal: Vec<_> = (0..4096).map(|sample| sample as f32 / 4096.0).collect();
    commands
        .push(EngineCommand::AddClip {
            track_id: track,
            clip_id: ClipId::new(),
            audio: Arc::new(DecodedAudio {
                channels: vec![signal.clone(), signal],
                sample_rate: 44100,
            }),
            position: 0,
            source_offset: 0,
            start_marker: 0,
            duration: 4096,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            linear_gain: 1.0,
            fades: Default::default(),
            playback_direction: Default::default(),
            warp_markers: Default::default(),
        })
        .unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: effect,
            effect: Box::new(DelayProbe::new(137)),
            position: None,
        })
        .unwrap();
    let model = [
        RoutingChannel {
            id: track,
            is_bus: false,
            sends: vec![],
            effects: vec![RoutingEffect {
                id: effect,
                inputs: vec![],
                assignments: vec![],
                inactive_inputs: vec![],
            }],
        },
        RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            sends: vec![],
            effects: vec![],
        },
    ];
    let reports = [(
        RoutingNode {
            channel: track,
            stage: NodeStage::Effect(effect),
        },
        137,
    )];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&model, 512, &reports, &[], 1)
                .unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    engine.process(&mut [], 2);
    let mut output = [0.0; 128];
    for block in 0..28 {
        if let Some(bypass) = match block {
            8 => Some(true),
            16 => Some(false),
            _ => None,
        } {
            commands
                .push(EngineCommand::SetEffectBypass {
                    track_id: track,
                    effect_id: effect,
                    bypass,
                })
                .unwrap();
        }
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
            (0, 0)
        );
        for frame in 0..64 {
            let source: usize = block * 64 + frame;
            let expected =
                source.saturating_sub(137) as f32 / 4096.0 * std::f32::consts::FRAC_1_SQRT_2;
            assert!(
                (output[frame * 2] - expected).abs() < 1e-7,
                "block{block} frame{frame}: {} vs{expected}",
                output[frame * 2]
            );
        }
        while events.pop().is_ok() {}
    }
}

#[test]
fn awaiting_main_thread_preparation_fades_out_and_retains_the_resume_fade() {
    let (mut engine, _, _) = AudioEngine::new();
    engine.last_mix_frame = [0.5; 2];
    let mut paused = [0.0; 256];
    engine.apply_compensation_transition(&mut paused, 2, true);
    assert_eq!(paused[0], 0.5);
    assert_eq!(paused[254], 0.0);
    assert!(paused
        .chunks_exact(2)
        .map(|frame| frame[0])
        .collect::<Vec<_>>()
        .windows(2)
        .all(|pair| (pair[1] - pair[0]).abs() <= 0.5 / 64.0));
    assert_eq!(engine.compensation_fade_in, 64);
    engine.apply_compensation_transition(&mut paused, 2, true);
    assert_eq!(engine.compensation_fade_in, 64);
    let mut resumed = [0.5; 256];
    engine.apply_compensation_transition(&mut resumed, 2, false);
    assert_eq!(resumed[0], 0.0);
    assert_eq!(resumed[254], 0.5);
}
