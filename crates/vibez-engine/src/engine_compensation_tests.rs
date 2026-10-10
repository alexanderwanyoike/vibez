use super::*;
use crate::test_support::DelayProbe;
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::{ClipId, EffectId};
use vibez_core::routing::*;
use vibez_dsp::effect::AudioEffect;

fn channel(id: TrackId, fx: &[EffectId]) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: false,
        sends: vec![],
        effects: fx
            .iter()
            .map(|&id| RoutingEffect {
                inactive_inputs: vec![],
                id,
                inputs: vec![],
                assignments: vec![],
            })
            .collect(),
    }
}

fn pulse_clip(track_id: TrackId, amplitude: f32) -> EngineCommand {
    let mut samples = vec![0.0; 4096];
    for onset in [0, 17, 511, 1030] {
        samples[onset] = amplitude;
    }
    EngineCommand::AddClip {
        track_id,
        clip_id: ClipId::new(),
        audio: Arc::new(DecodedAudio {
            channels: vec![samples.clone(), samples],
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
    }
}

fn cancellation_run(reported: u32, bypass: bool, order_reversed: bool) -> Vec<f32> {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let delayed = TrackId::new();
    let dry = TrackId::new();
    let fx = EffectId::new();
    for id in if order_reversed {
        [dry, delayed]
    } else {
        [delayed, dry]
    } {
        commands
            .push(EngineCommand::AddTrack(id, "Pulse".into()))
            .unwrap();
    }
    commands.push(pulse_clip(delayed, 1.0)).unwrap();
    commands.push(pulse_clip(dry, -1.0)).unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: delayed,
            effect_id: fx,
            effect: Box::new(DelayProbe::with_report(137, reported)),
            position: None,
        })
        .unwrap();
    let channels = [
        channel(dry, &[]),
        channel(delayed, &[fx]),
        channel(TrackId::MASTER, &[]),
    ];
    let reports = [(
        RoutingNode {
            channel: delayed,
            stage: NodeStage::Effect(fx),
        },
        reported,
    )];
    let plan =
        crate::routing::PreparedRouting::prepare_compensated(&channels, 512, &reports, &[], 3)
            .unwrap();
    commands.push(EngineCommand::SetRouting(plan)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    engine.drain_commands();
    engine
        .tracks
        .iter_mut()
        .find(|track| track.id == delayed)
        .unwrap()
        .effects[0]
        .bypass = bypass;
    let mut result = Vec::new();
    for frames in [1, 17, 511, 3, 64].into_iter().cycle().take(20) {
        let mut block = vec![0.0; frames * 2];
        engine.process(&mut block, 2);
        result.extend(block);
        while events.pop().is_ok() {}
    }
    result
}

#[test]
fn production_graph_aligns_parallel_pulses_independent_of_source_order() {
    for reversed in [false, true] {
        let actual = cancellation_run(137, false, reversed);
        assert!(actual.iter().all(|sample| sample.abs() <= 1e-7));
    }
    let wrong = cancellation_run(138, false, false);
    assert!(wrong.iter().any(|sample| sample.abs() > 0.1));
    let uncompensated = cancellation_run(0, false, false);
    assert!(uncompensated.iter().any(|sample| sample.abs() > 0.1));
}

#[test]
fn host_bypass_preserves_declared_processing_delay() {
    let bypassed = cancellation_run(137, true, false);
    assert!(bypassed.iter().all(|sample| sample.abs() <= 1e-7));
}

struct ChangingDelay {
    inner: DelayProbe,
    pending: Arc<std::sync::atomic::AtomicU32>,
    stopped: bool,
}

impl AudioEffect for ChangingDelay {
    fn latency_samples(&self) -> u32 {
        self.inner.latency_samples()
    }
    fn reconfiguration_requested(&self) -> bool {
        self.pending.load(std::sync::atomic::Ordering::Acquire) != self.inner.latency_samples()
    }
    fn stop_for_reconfiguration(&mut self) {
        self.stopped = true;
    }
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        assert!(self.stopped);
        let report = self.pending.load(std::sync::atomic::Ordering::Acquire);
        self.inner.set_report(report);
        if report <= 4096 {
            self.inner.set_actual_delay(report);
        }
        self.stopped = false;
        Ok(())
    }
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
        assert!(!self.stopped);
        self.inner.process(samples, channels);
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
}

#[test]
fn coordinated_latency_changes_keep_transport_running_and_regain_alignment() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let delayed = TrackId::new();
    let dry = TrackId::new();
    let effect = EffectId::new();
    let pending = Arc::new(AtomicU32::new(137));
    for (id, amplitude) in [(delayed, 1.0), (dry, -1.0)] {
        commands
            .push(EngineCommand::AddTrack(id, "Constant".into()))
            .unwrap();
        let mut clip = pulse_clip(id, amplitude);
        if let EngineCommand::AddClip { audio, .. } = &mut clip {
            *audio = Arc::new(DecodedAudio {
                channels: vec![vec![amplitude; 4096]; 2],
                sample_rate: 44100,
            });
        }
        commands.push(clip).unwrap();
    }
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: delayed,
            effect_id: effect,
            effect: Box::new(ChangingDelay {
                inner: DelayProbe::with_report(137, 137),
                pending: pending.clone(),
                stopped: false,
            }),
            position: None,
        })
        .unwrap();
    let channels = [
        channel(delayed, &[effect]),
        channel(dry, &[]),
        channel(TrackId::MASTER, &[]),
    ];
    let make_plan = |report, generation| {
        crate::routing::PreparedRouting::prepare_compensated(
            &channels,
            64,
            &[(
                RoutingNode {
                    channel: delayed,
                    stage: NodeStage::Effect(effect),
                },
                report,
            )],
            &[],
            generation,
        )
    };
    commands
        .push(EngineCommand::SetRouting(make_plan(137, 1).unwrap()))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 128];
    for _ in 0..10 {
        engine.process(&mut output, 2);
        assert!(output.iter().all(|sample| sample.abs() < 1e-7));
        while events.pop().is_ok() {}
    }
    for (generation, report) in [(2, 521), (3, 137)] {
        pending.store(report, Ordering::Release);
        engine.process(&mut output, 2);
        assert!(engine.transport.is_playing());
        assert!(output.iter().all(|sample| *sample == 0.0));
        let mut device = loop {
            if let EngineEvent::DeviceReconfiguration(device) = events.pop().unwrap() {
                break device;
            }
        };
        device.reconfigure_on_main_thread().unwrap();
        commands
            .push(EngineCommand::ResumeDeviceReconfiguration {
                device,
                routing: make_plan(report, generation).unwrap(),
            })
            .unwrap();
        for _ in 0..12 {
            engine.process(&mut output, 2);
            assert!(engine.transport.is_playing());
            assert!(output.iter().all(|sample| sample.abs() < 1e-7));
            while events.pop().is_ok() {}
        }
        assert_eq!(
            engine.routing.as_ref().unwrap().compensation.generation,
            generation
        );
    }
    pending.store(
        crate::compensation::MAX_DEVICE_LATENCY + 1,
        Ordering::Release,
    );
    engine.process(&mut output, 2);
    let mut device = loop {
        if let EngineEvent::DeviceReconfiguration(device) = events.pop().unwrap() {
            break device;
        }
    };
    device.reconfigure_on_main_thread().unwrap();
    assert!(make_plan(device.latency_samples(), 4).is_err());
    commands
        .push(EngineCommand::RejectDeviceReconfiguration {
            device,
            reason: "Controlled excessive latency report".into(),
        })
        .unwrap();
    engine.process(&mut output, 2);
    assert!(!engine.transport.is_playing());
    assert!(output.iter().all(|sample| *sample == 0.0));
}

#[test]
fn structural_effect_edit_can_wait_for_plan_in_a_later_callback() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    commands
        .push(EngineCommand::AddTrack(track, "Pulse".into()))
        .unwrap();
    commands.push(pulse_clip(track, 1.0)).unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: effect,
            effect: Box::new(DelayProbe::with_report(137, 137)),
            position: None,
        })
        .unwrap();
    let channels = [channel(track, &[effect]), channel(TrackId::MASTER, &[])];
    let reports = [(
        RoutingNode {
            channel: track,
            stage: NodeStage::Effect(effect),
        },
        137,
    )];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&channels, 64, &reports, &[], 1)
                .unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut block = [0.0; 128];
    for _ in 0..5 {
        engine.process(&mut block, 2);
        while events.pop().is_ok() {}
    }
    commands
        .push(EngineCommand::RemoveEffect(track, effect))
        .unwrap();
    engine.process(&mut block, 2);
    assert!(engine.transport.is_playing());
    assert!(engine.graph_edit_pending);
    assert!(block.iter().all(|&value| value == 0.0));
    assert!(
        !std::iter::from_fn(|| events.pop().ok()).any(|event| matches!(
            event,
            EngineEvent::CompensationInvalid { .. } | EngineEvent::PlaybackStopped
        ))
    );
    let channels = [channel(track, &[]), channel(TrackId::MASTER, &[])];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&channels, 64, &[], &[], 2)
                .unwrap(),
        ))
        .unwrap();
    engine.process(&mut block, 2);
    assert!(engine.transport.is_playing());
    assert!(!engine.graph_edit_pending);
    assert!(engine.compensation_valid);
}

#[test]
fn automation_target_addition_and_point_edit_preserve_running_delay_history() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    commands
        .push(EngineCommand::AddTrack(track, "Constant".into()))
        .unwrap();
    let mut clip = pulse_clip(track, 1.0);
    if let EngineCommand::AddClip { audio, .. } = &mut clip {
        *audio = Arc::new(DecodedAudio {
            channels: vec![vec![1.0; 4096]; 2],
            sample_rate: 44100,
        });
    }
    commands.push(clip).unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: effect,
            effect: Box::new(DelayProbe::with_report(137, 137)),
            position: None,
        })
        .unwrap();
    let channels = [channel(track, &[effect]), channel(TrackId::MASTER, &[])];
    let reports = [(
        RoutingNode {
            channel: track,
            stage: NodeStage::Effect(effect),
        },
        137,
    )];
    let make = |generation, targets: &[AutomationTarget]| {
        let mut plan = crate::routing::PreparedRouting::prepare_compensated(
            &channels,
            64,
            &reports,
            &[],
            generation,
        )
        .unwrap();
        plan.configure_automation(
            &targets
                .iter()
                .map(|&target| (track, target))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        plan
    };
    let gain = AutomationTarget::TrackGain;
    let mut lane = AutomationLane::new(gain);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 0.5,
        curve: 0.0,
    });
    commands
        .push(EngineCommand::SetAutomationLane {
            track_id: track,
            lane: lane.clone(),
        })
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(make(1, &[gain])))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut block = [0.0; 128];
    for _ in 0..5 {
        engine.process(&mut block, 2);
        while events.pop().is_ok() {}
    }
    let before = block[0];
    assert!(before > 0.1);
    commands
        .push(EngineCommand::UpdateAutomationRouting(make(
            2,
            &[gain, AutomationTarget::TrackPan],
        )))
        .unwrap();
    engine.process(&mut block, 2);
    assert!(block.iter().all(|&value| (value - before).abs() < 1e-6));
    assert_eq!(engine.compensation_plan_fade, 0);
    lane.points[0].value = 0.25;
    commands
        .push(EngineCommand::SetAutomationLane {
            track_id: track,
            lane,
        })
        .unwrap();
    engine.process(&mut block, 2);
    assert!(block.iter().all(|&value| (value - before).abs() < 1e-6));
    for _ in 0..3 {
        engine.process(&mut block, 2);
    }
    assert!(block
        .iter()
        .all(|&value| (value - before * 0.5).abs() < 1e-6));
    assert!(engine.transport.is_playing());
}

#[test]
fn reduced_monitoring_capture_uses_the_same_heard_section_identity_and_local_clock() {
    use crate::playback_source::{PreparedPlaybackSource, PreparedSectionPlaybackSource};
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let slow = TrackId::new();
    let fast_fx = EffectId::new();
    let slow_fx = EffectId::new();
    let old = SectionId::new();
    let new = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(128)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
    for (id, fx, delay) in [(track, fast_fx, 137), (slow, slow_fx, 521)] {
        commands
            .push(EngineCommand::AddTrack(id, "Input".into()))
            .unwrap();
        commands
            .push(EngineCommand::AddPluginEffect {
                track_id: id,
                effect_id: fx,
                effect: Box::new(DelayProbe::with_report(delay, delay)),
                position: None,
            })
            .unwrap();
    }
    let channels = [
        channel(track, &[fast_fx]),
        channel(slow, &[slow_fx]),
        channel(TrackId::MASTER, &[]),
    ];
    let reports = [
        (
            RoutingNode {
                channel: track,
                stage: NodeStage::Effect(fast_fx),
            },
            137,
        ),
        (
            RoutingNode {
                channel: slow,
                stage: NodeStage::Effect(slow_fx),
            },
            521,
        ),
    ];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(
                &channels,
                64,
                &reports,
                &[track],
                1,
            )
            .unwrap(),
        ))
        .unwrap();
    let section = |id| {
        Box::new(PreparedSectionPlaybackSource::new(
            id,
            4.0,
            true,
            vec![(track, PreparedPlaybackSource::new(vec![], vec![], vec![]))],
        ))
    };
    commands
        .push(EngineCommand::LaunchSection(section(old)))
        .unwrap();
    for _ in 0..16 {
        engine.process(&mut [0.0; 128], 2);
        while events.pop().is_ok() {}
    }
    assert_eq!(engine.performance_position, 1024);
    commands
        .push(EngineCommand::LaunchSection(section(new)))
        .unwrap();
    commands
        .push(EngineCommand::ExternalNoteOn {
            track_id: track,
            pitch: 60,
            velocity: 100,
        })
        .unwrap();
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    engine.process(&mut [0.0; 2], 2);
    let immediate: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert!(immediate.iter().any(|event|matches!(event,EngineEvent::SourceNoteInput {position,..} if position.effective_at_samples==1024 && position.section_id==Some(new) && position.section_position_samples==Some(0))));
    assert!(!immediate.iter().any(
        |event| matches!(event,EngineEvent::SectionTransitioned{section_id,..} if *section_id==new)
    ));
    assert!(immediate.iter().any(|event| matches!(event,EngineEvent::PerformanceCaptureStarted{effective_at_samples:503,section_id:Some(id),section_position_samples:Some(503),..} if *id==old)));
    for _ in 0..3 {
        engine.process(&mut [0.0; 128], 2);
    }
    let notes: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert!(notes.iter().any(|event| matches!(event,EngineEvent::InstrumentNoteInput{effective_at_samples:640,section_id:Some(id),section_position_samples:Some(128),..} if *id==old)));
    assert!(!notes.iter().any(
        |event| matches!(event,EngineEvent::SectionTransitioned{section_id,..} if *section_id==new)
    ));
    assert!(notes.iter().any(|event| matches!(event, EngineEvent::SectionCaptureSource {section_id,effective_at_samples:1024,offsets,..} if *section_id==new && offsets.as_ref()==[(track,384)])));
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    engine.process(&mut [0.0; 2], 2);
    let restarted: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert!(restarted.iter().any(|event| matches!(event,EngineEvent::PerformanceCaptureStarted{effective_at_samples:696,section_id:Some(id),section_position_samples:Some(184),..} if *id==old)));
    assert!(restarted.iter().any(|event| matches!(event,EngineEvent::SectionCaptureSource{section_id,effective_at_samples:1024,offsets,..} if *section_id==new && offsets.as_ref()==[(track,384)])));
    for _ in 0..6 {
        engine.process(&mut [0.0; 128], 2);
    }
    assert!(std::iter::from_fn(|| events.pop().ok()).any(|event| matches!(event,EngineEvent::SectionTransitioned{section_id,effective_at_samples:1024,..} if section_id==new)));
}

#[test]
fn cold_reduced_monitoring_keeps_the_first_clip_sample_on_its_actual_path() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let fast = TrackId::new();
    let slow = TrackId::new();
    let fast_fx = EffectId::new();
    let slow_fx = EffectId::new();
    for (track, effect, delay) in [(fast, fast_fx, 137), (slow, slow_fx, 521)] {
        commands
            .push(EngineCommand::AddTrack(track, "Pulse".into()))
            .unwrap();
        commands
            .push(pulse_clip(track, if track == fast { 1.0 } else { 0.0 }))
            .unwrap();
        commands
            .push(EngineCommand::AddPluginEffect {
                track_id: track,
                effect_id: effect,
                effect: Box::new(DelayProbe::with_report(delay, delay)),
                position: None,
            })
            .unwrap();
    }
    let channels = [
        channel(fast, &[fast_fx]),
        channel(slow, &[slow_fx]),
        channel(TrackId::MASTER, &[]),
    ];
    let reports = [
        (
            RoutingNode {
                channel: fast,
                stage: NodeStage::Effect(fast_fx),
            },
            137,
        ),
        (
            RoutingNode {
                channel: slow,
                stage: NodeStage::Effect(slow_fx),
            },
            521,
        ),
    ];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(
                &channels,
                64,
                &reports,
                &[fast],
                1,
            )
            .unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = Vec::new();
    for _ in 0..10 {
        let mut block = [0.0; 128];
        engine.process(&mut block, 2);
        output.extend(block);
        while events.pop().is_ok() {}
    }
    assert_eq!(
        output
            .chunks_exact(2)
            .position(|frame| frame[0].abs() > 0.1),
        Some(137)
    );
    commands.push(EngineCommand::Seek(511)).unwrap();
    let mut output = Vec::new();
    for _ in 0..5 {
        let mut block = [0.0; 128];
        engine.process(&mut block, 2);
        output.extend(block);
        while events.pop().is_ok() {}
    }
    assert_eq!(
        output
            .chunks_exact(2)
            .position(|frame| frame[0].abs() > 0.1),
        Some(8)
    );
    // The already rendered pulse drains at 648; the newly sought pulse still
    // traverses its actual 137-sample device path instead of being advanced.
    assert!(output[137 * 2].abs() > 0.1);
    assert!(output[136 * 2].abs() < 1e-7);
}

#[test]
fn changed_rate_keeps_native_owners_but_rejects_their_old_configuration_even_after_republication() {
    for instrument in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let effect = EffectId::new();
        commands
            .push(EngineCommand::AddTrack(track, "Rate".into()))
            .unwrap();
        if instrument {
            commands
                .push(EngineCommand::SetTrackInstrument(
                    track,
                    vibez_core::midi::InstrumentKind::SubtractiveSynth,
                ))
                .unwrap();
        } else {
            commands
                .push(EngineCommand::AddEffect {
                    track_id: track,
                    effect_id: effect,
                    effect_type: EffectType::Filter,
                    position: None,
                })
                .unwrap();
        }
        let channels = [
            channel(
                track,
                if instrument {
                    &[]
                } else {
                    std::slice::from_ref(&effect)
                },
            ),
            channel(TrackId::MASTER, &[]),
        ];
        let make = || {
            crate::routing::PreparedRouting::prepare_compensated(&channels, 64, &[], &[], 1)
                .unwrap()
        };
        commands.push(EngineCommand::SetRouting(make())).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        engine.process(&mut [0.0; 128], 2);
        while events.pop().is_ok() {}
        commands.push(EngineCommand::SetSampleRate(48000)).unwrap();
        engine.process(&mut [0.0; 128], 2);
        assert!(!engine.transport.is_playing());
        assert!(!engine.compensation_valid);
        assert!(std::iter::from_fn(||events.pop().ok()).any(|event|matches!(event,EngineEvent::CompensationInvalid{track_id,reason,..} if track_id==track && reason.contains("sample rate"))));
        let channel = engine
            .tracks
            .iter()
            .find(|channel| channel.id == track)
            .unwrap();
        assert!(if instrument {
            channel.instrument.is_some()
        } else {
            !channel.effects.is_empty()
        });
        commands.push(EngineCommand::SetRouting(make())).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        engine.process(&mut [0.0; 128], 2);
        assert!(!engine.compensation_valid);
        assert!(!engine.transport.is_playing());
    }
}

#[test]
fn capture_replay_with_more_than_retirement_capacity_resumes_without_callback_memory_operations() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let fast = TrackId::new();
    let slow = TrackId::new();
    let effect = EffectId::new();
    let channels = [
        channel(fast, &[]),
        channel(slow, &[effect]),
        channel(TrackId::MASTER, &[]),
    ];
    let reports = [(
        RoutingNode {
            channel: slow,
            stage: NodeStage::Effect(effect),
        },
        521,
    )];
    engine.routing = Some(
        crate::routing::PreparedRouting::prepare_compensated(&channels, 64, &reports, &[fast], 1)
            .unwrap(),
    );
    engine.output_position = 1000;
    for index in 0..40 {
        engine.section_capture_source(SectionId::new(), index, 0, false);
    }
    while events.pop().is_ok() {}
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    for _ in 0..engine.scheduled_presentation.capacity() - 20 {
        engine
            .scheduled_presentation
            .push(presentation_queue::ScheduledPresentation {
                due: 1000,
                event: EngineEvent::PlaybackPosition(0),
            });
    }
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [], 2)),
        (0, 0)
    );
    assert!(engine.capture_replay.is_some());
    assert!(engine.pending_retirements.len() <= engine.pending_retirements.capacity());
    let mut replayed = 0;
    let mut starts = 0;
    for _ in 0..10 {
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::SectionCaptureSource { .. } => replayed += 1,
                EngineEvent::PerformanceCaptureStarted { .. } => starts += 1,
                _ => {}
            }
        }
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [], 2)),
            (0, 0)
        );
    }
    assert_eq!(replayed, 40);
    assert_eq!(starts, 1);
    assert!(engine.capture_replay.is_none());
    assert!(engine.pending_retirements.len() <= engine.pending_retirements.capacity());
}

#[test]
fn short_one_shot_section_drains_last_content_before_its_heard_stop_notification() {
    use crate::playback_source::{PreparedPlaybackSource, PreparedSectionPlaybackSource};
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    let section = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(128)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track, "One shot".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: effect,
            effect: Box::new(DelayProbe::with_report(521, 521)),
            position: None,
        })
        .unwrap();
    let channels = [channel(track, &[effect]), channel(TrackId::MASTER, &[])];
    let reports = [(
        RoutingNode {
            channel: track,
            stage: NodeStage::Effect(effect),
        },
        521,
    )];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&channels, 64, &reports, &[], 1)
                .unwrap(),
        ))
        .unwrap();
    let clip = EngineClip {
        id: ClipId::new(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![1.0; 128]; 2],
            sample_rate: 128,
        }),
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 128,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        linear_gain: 1.0,
        fades: Default::default(),
        playback_direction: Default::default(),
        warp_markers: Default::default(),
    };
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                section,
                1.0,
                false,
                vec![(
                    track,
                    PreparedPlaybackSource::new(vec![clip], vec![], vec![]),
                )],
            ),
        )))
        .unwrap();
    let mut rendered = Vec::new();
    let mut transitioned = None;
    let mut stopped = None;
    for block in 0..12 {
        let mut output = [0.0; 128];
        engine.process(&mut output, 2);
        rendered.extend(output);
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::SectionTransitioned { section_id, .. } if section_id == section => {
                    transitioned = Some((block + 1) * 64)
                }
                EngineEvent::PlaybackStopped => stopped = Some((block + 1) * 64),
                _ => {}
            }
        }
        if (block + 1) * 64 < 521 {
            assert!(transitioned.is_none());
            assert!(stopped.is_none());
        }
    }
    assert!(transitioned.unwrap() >= 521);
    assert!(stopped.unwrap() >= 128 + 521);
    assert!(stopped > transitioned);
    for (frame, samples) in rendered.chunks_exact(2).enumerate() {
        assert_eq!(
            samples[0] > 0.1,
            (521..649).contains(&frame),
            "frame{frame}"
        );
    }
}
