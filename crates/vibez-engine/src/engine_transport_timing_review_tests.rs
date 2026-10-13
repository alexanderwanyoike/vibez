//! Transport edits preserve audible processing history and current state.

use super::*;
use crate::playback_source::{EngineClip, PreparedPlaybackSource, PreparedSectionPlaybackSource};
use crate::test_support::DelayProbe;
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::{ClipId, EffectId};
use vibez_core::routing::*;
use vibez_dsp::effect::AudioEffect;

struct MusicalTail([f32; 2]);
impl AudioEffect for MusicalTail {
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, samples: &mut [f32], channels: usize) {
        for frame in samples.chunks_exact_mut(channels) {
            for (channel, sample) in frame.iter_mut().enumerate() {
                self.0[channel] = if *sample != 0.0 {
                    *sample
                } else {
                    self.0[channel] * 0.999
                };
                *sample = self.0[channel];
            }
        }
    }
    fn reset(&mut self) {
        self.0 = [0.0; 2];
    }
}

#[test]
fn stop_then_seek_preserves_a_musical_tail_with_zero_processing_latency() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    commands
        .push(EngineCommand::AddTrack(track, "Tail".into()))
        .unwrap();
    commands.push(source(track)).unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: effect,
            effect: Box::new(MusicalTail([0.0; 2])),
            position: None,
        })
        .unwrap();
    let channel = RoutingChannel {
        id: track,
        is_bus: false,
        sends: vec![],
        effects: vec![RoutingEffect {
            id: effect,
            inputs: vec![],
            assignments: vec![],
            inactive_inputs: vec![],
        }],
    };
    let master = RoutingChannel {
        id: TrackId::MASTER,
        is_bus: true,
        sends: vec![],
        effects: vec![],
    };
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&[channel, master], 256).unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0; 512], 2);
    while events.pop().is_ok() {}
    commands.push(EngineCommand::Stop).unwrap();
    commands.push(EngineCommand::Seek(0)).unwrap();
    let mut output = [0.0; 512];
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
        (0, 0)
    );
    assert!(output.iter().all(|&sample| sample > 0.1));
}

fn source(track: TrackId) -> EngineCommand {
    EngineCommand::AddClip {
        track_id: track,
        clip_id: ClipId::new(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![0.2; 8192]; 2],
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
    }
}

fn model(
    dry: TrackId,
    delayed: TrackId,
    effect: EffectId,
    extra: Option<EffectId>,
) -> Vec<RoutingChannel> {
    [dry, delayed, TrackId::MASTER]
        .into_iter()
        .map(|id| RoutingChannel {
            id,
            is_bus: id.is_master(),
            sends: vec![],
            effects: if id == delayed {
                Some(effect)
            } else if id == dry {
                extra
            } else {
                None
            }
            .into_iter()
            .map(|id| RoutingEffect {
                id,
                inputs: vec![],
                assignments: vec![],
                inactive_inputs: vec![],
            })
            .collect(),
        })
        .collect()
}

fn setup() -> (
    AudioEngine,
    Producer<EngineCommand>,
    Consumer<EngineEvent>,
    TrackId,
    TrackId,
    EffectId,
) {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let dry = TrackId::new();
    let delayed = TrackId::new();
    let effect = EffectId::new();
    for id in [dry, delayed] {
        commands
            .push(EngineCommand::AddTrack(id, "Constant".into()))
            .unwrap();
        commands.push(source(id)).unwrap();
    }
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: delayed,
            effect_id: effect,
            effect: Box::new(DelayProbe::new(137)),
            position: None,
        })
        .unwrap();
    let report = [(
        RoutingNode {
            channel: delayed,
            stage: NodeStage::Effect(effect),
        },
        137,
    )];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(
                &model(dry, delayed, effect, None),
                256,
                &report,
                &[],
                1,
            )
            .unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    for _ in 0..4 {
        engine.process(&mut [0.0; 512], 2);
        while events.pop().is_ok() {}
    }
    (engine, commands, events, dry, delayed, effect)
}

#[test]
fn seek_preserves_the_pending_audio_and_device_delay_history() {
    let (mut engine, mut commands, mut events, ..) = setup();
    let mut output = [0.0; 512];
    engine.process(&mut output, 2);
    let steady = output[510];
    while events.pop().is_ok() {}
    commands.push(EngineCommand::Seek(4000)).unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
        (0, 0)
    );
    assert!(
        output.iter().all(|&sample| (sample - steady).abs() < 1e-7),
        "seek dropped pending audio"
    );
}

#[test]
fn arrange_stop_then_seek_retains_the_processing_tail() {
    let (mut engine, mut commands, mut events, ..) = setup();
    let mut output = [0.0; 512];
    engine.process(&mut output, 2);
    let steady = output[510];
    while events.pop().is_ok() {}
    commands.push(EngineCommand::Stop).unwrap();
    commands.push(EngineCommand::Seek(0)).unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
        (0, 0)
    );
    assert!(
        output[..137 * 2]
            .iter()
            .all(|&sample| (sample - steady).abs() < 1e-7),
        "stop cut pending tail"
    );
    assert!(output[137 * 2..].iter().all(|sample| sample.abs() < 1e-7));
}

fn section(tracks: &[TrackId], frames: u64, looping: bool) -> Box<PreparedSectionPlaybackSource> {
    let clip = || EngineClip {
        id: ClipId::new(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![0.2; 8192]; 2],
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
    };
    Box::new(PreparedSectionPlaybackSource::new(
        SectionId::new(),
        frames as f64 * 120.0 / (44100.0 * 60.0),
        looping,
        tracks
            .iter()
            .map(|&track| {
                (
                    track,
                    PreparedPlaybackSource::new(vec![clip()], vec![], vec![]),
                )
            })
            .collect(),
    ))
}

#[test]
fn first_section_launch_preserves_unaffected_alignment_history() {
    let (mut engine, mut commands, mut events, dry, delayed, ..) = setup();
    let mut output = [0.0; 512];
    engine.process(&mut output, 2);
    let steady = output[510];
    while events.pop().is_ok() {}
    let next = section(&[dry, delayed], 8192, true);
    commands.push(EngineCommand::LaunchSection(next)).unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
        (0, 0)
    );
    assert!(
        output.iter().all(|&sample| (sample - steady).abs() < 1e-7),
        "Perform reset audible history"
    );
    assert!(engine.heard_capture_position() < 256);
}

#[test]
fn structural_zero_delay_insert_keeps_existing_parallel_audio_continuous() {
    let (mut engine, mut commands, mut events, dry, delayed, effect) = setup();
    let mut output = [0.0; 512];
    engine.process(&mut output, 2);
    let steady = output[510];
    while events.pop().is_ok() {}
    engine
        .tracks
        .iter_mut()
        .find(|track| track.id == dry)
        .unwrap()
        .effects
        .reserve(1);
    let extra = EffectId::new();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: dry,
            effect_id: extra,
            effect: Box::new(DelayProbe::new(0)),
            position: None,
        })
        .unwrap();
    let reports = [(
        RoutingNode {
            channel: delayed,
            stage: NodeStage::Effect(effect),
        },
        137,
    )];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(
                &model(dry, delayed, effect, Some(extra)),
                256,
                &reports,
                &[],
                2,
            )
            .unwrap(),
        ))
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
        (0, 0)
    );
    assert!(
        output.iter().all(|&sample| (sample - steady).abs() < 1e-7),
        "structural edit gated the mix"
    );
    assert_eq!(engine.compensation_plan_fade, 0);
}

#[test]
fn play_after_one_shot_source_end_cannot_receive_the_old_heard_stop() {
    let (mut engine, mut commands, mut events, dry, ..) = setup();
    commands
        .push(EngineCommand::LaunchSection(section(&[dry], 64, false)))
        .unwrap();
    engine.process(&mut [0.0; 128], 2);
    while events.pop().is_ok() {}
    assert!(!engine.transport.is_playing());
    commands.push(EngineCommand::Play).unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 512], 2)),
        (0, 0)
    );
    let mut started = false;
    while let Ok(event) = events.pop() {
        if matches!(event, EngineEvent::PlaybackStarted) {
            started = true;
        }
        assert!(
            !started
                || !matches!(
                    event,
                    EngineEvent::PlaybackStopped | EngineEvent::SectionTransitioned { .. }
                ),
            "old heard state followed Play"
        );
    }
    assert!(started);
    assert!(engine.transport.is_playing());
}

#[test]
fn restarting_before_the_heard_section_end_closes_the_old_capture_first() {
    let (mut engine, mut commands, mut events, dry, ..) = setup();
    commands
        .push(EngineCommand::LaunchSection(section(&[dry], 64, false)))
        .unwrap();
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    engine.process(&mut [0.0; 128], 2);
    let mut recording = false;
    while let Ok(event) = events.pop() {
        if matches!(event, EngineEvent::PerformanceCaptureStarted { .. }) {
            recording = true;
        }
    }
    assert!(recording);
    assert!(!engine.transport.is_playing());
    let heard_end = engine.heard_capture_position();
    commands.push(EngineCommand::Play).unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 512], 2)),
        (0, 0)
    );
    let mut stopped = false;
    let mut playing = false;
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                assert!(!playing, "old capture closure arrived after the new Play");
                assert_eq!(effective_at_samples, heard_end);
                stopped = true;
                recording = false;
            }
            EngineEvent::PlaybackStarted => {
                assert!(stopped);
                playing = true;
            }
            EngineEvent::SectionCaptureSource { .. }
            | EngineEvent::SectionTransitioned { .. }
            | EngineEvent::PlaybackStopped => {
                assert!(!playing, "old Section lifetime survived Play")
            }
            _ => {}
        }
    }
    assert!(playing && !recording);
}

#[test]
fn unchanged_alignment_publication_preserves_a_varying_waveform_sample_for_sample() {
    let (mut engine, mut commands, mut events, dry, delayed, effect) = setup();
    let ramp: Vec<_> = (0..8192).map(|index| 0.2 * index as f32 / 8192.0).collect();
    engine
        .tracks
        .iter_mut()
        .find(|track| track.id == dry)
        .unwrap()
        .playback_source
        .clips[0]
        .audio = Arc::new(DecodedAudio {
        channels: vec![ramp.clone(), ramp],
        sample_rate: 44100,
    });
    for _ in 0..4 {
        engine.process(&mut [0.0; 512], 2);
        while events.pop().is_ok() {}
    }
    let position = engine.transport.position();
    let reports = [(
        RoutingNode {
            channel: delayed,
            stage: NodeStage::Effect(effect),
        },
        137,
    )];
    let plan = crate::routing::PreparedRouting::prepare_compensated(
        &model(dry, delayed, effect, None),
        256,
        &reports,
        &[],
        2,
    )
    .unwrap();
    commands
        .push(EngineCommand::UpdateAutomationRouting(plan))
        .unwrap();
    let mut output = [0.0; 512];
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
        (0, 0)
    );
    for (frame, pair) in output.chunks_exact(2).enumerate() {
        let expected = (0.2 + 0.2 * (position + frame as u64 - 137) as f32 / 8192.0)
            * std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (pair[0] - expected).abs() < 1e-7,
            "publication changed frame{frame}: {} vs{expected}",
            pair[0]
        );
    }
}

#[test]
fn perform_rebase_preserves_heard_arrange_history_across_a_loop_wrap() {
    let (mut engine, mut commands, mut events, dry, delayed, ..) = setup();
    commands
        .push(EngineCommand::SetArrangementLoopRegion {
            start: 0,
            end: 1024,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetArrangementLoop(true))
        .unwrap();
    engine.process(&mut [0.0; 64], 2);
    while events.pop().is_ok() {}
    let heard = engine.presentation_context(137).arrange;
    assert!(heard > engine.transport.position());
    commands
        .push(EngineCommand::LaunchSection(section(
            &[dry, delayed],
            8192,
            true,
        )))
        .unwrap();
    let mut output = [0.0; 32];
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
        (0, 0)
    );
    let context = engine.presentation_context(137);
    assert_eq!(context.arrange, heard + 16);
    assert_eq!(context.perform, 0);
    assert!(output.iter().all(|sample| *sample > 0.2));
}

#[test]
fn restarting_perform_does_not_cancel_a_take_end_without_delivering_its_closure() {
    let (mut engine, mut commands, mut events, dry, ..) = setup();
    commands
        .push(EngineCommand::LaunchSection(section(&[dry], 64, false)))
        .unwrap();
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    engine.process(&mut [0.0; 128], 2);
    while events.pop().is_ok() {}
    assert!(!engine.transport.is_playing());
    commands
        .push(EngineCommand::LaunchSection(section(&[dry], 8192, true)))
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 512], 2)),
        (0, 0)
    );
    let mut closed = false;
    while let Ok(event) = events.pop() {
        if matches!(event, EngineEvent::PerformanceCaptureStopped { .. }) {
            closed = true;
        }
        if matches!(event, EngineEvent::PlaybackStarted) {
            assert!(
                closed,
                "ending take lost its stop before the next Perform lifetime"
            );
        }
    }
    assert!(closed);
}
