//! Public local recovery and resident restart keep independent handoff and output lifetimes.

use super::*;
use compensation_state_test_support::Readiness;
use graph_test_support::{constant_clip, graph_engine, routing_channel, routing_effect, routing_plan};
use std::sync::atomic::{AtomicBool, Ordering};
use vibez_core::{effect::{EffectType, ParamDescriptor}, id::EffectId};

struct HandoffProbe {
    requested: Arc<AtomicBool>,
    local: bool,
    stopped: bool,
}

impl vibez_dsp::effect::AudioEffect for HandoffProbe {
    fn reconfiguration_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
    fn processing_recovery_requested(&self) -> bool {
        self.local && self.requested.load(Ordering::Acquire)
    }
    fn processing_failure_is_local(&self) -> bool {
        self.local && self.requested.load(Ordering::Acquire)
    }
    fn stop_for_reconfiguration(&mut self) {
        assert!(!self.stopped);
        self.stopped = true;
    }
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        assert!(self.stopped);
        self.requested.store(false, Ordering::Release);
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
    fn process(&mut self, _: &mut [f32], _: usize) {
        assert!(!self.stopped);
    }
    fn reset(&mut self) {}
}

#[derive(Default)]
struct OverlapTrace {
    owners: [Option<reconfiguration::DeviceReconfiguration>; 2],
    order: Vec<&'static str>,
    retired_plans: Vec<usize>,
    capture_stops: Vec<u64>,
}

impl OverlapTrace {
    fn collect(&mut self, engine: &mut AudioEngine, events: &mut Consumer<EngineEvent>, tracks: [TrackId; 2]) {
        loop {
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::DeviceReconfiguration(device) => {
                        let index = tracks.iter().position(|id| *id == device.track_id()).unwrap();
                        assert!(self.owners[index].is_none());
                        self.owners[index] = Some(device);
                        self.order.push(if index == 0 { "local-handoff" } else { "project-handoff" });
                    }
                    EngineEvent::PerformanceCaptureStopped { effective_at_samples } => {
                        self.capture_stops.push(effective_at_samples);
                        self.order.push("capture-stop");
                    }
                    EngineEvent::RoutingRetired(plan) => {
                        self.retired_plans.push(std::ptr::from_ref(plan.as_ref()) as usize);
                        self.order.push(if self.retired_plans.len() == 1 { "original-plan-return" } else { "local-plan-return" });
                    }
                    EngineEvent::Metering { .. } | EngineEvent::TrackMeter { .. }
                    | EngineEvent::PlaybackPosition(_) | EngineEvent::PerformancePosition(_) => {}
                    event => panic!("unexpected overlap event: {event:?}"),
                }
            }
            assert_eq!(crate::retirement::tests::allocations(|| engine.flush_presentation()), (0, 0));
            if events.is_empty() {
                break;
            }
        }
    }
}

fn overlap_render(engine: &mut AudioEngine, events: &mut Consumer<EngineEvent>, tracks: [TrackId; 2], trace: &mut OverlapTrace, partitions: &[usize]) -> Vec<f32> {
    let mut samples = Vec::with_capacity(partitions.iter().sum::<usize>() * 2);
    for frames in partitions {
        let mut output = vec![0.0; frames * 2];
        assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut output, 2)), (0, 0));
        samples.extend_from_slice(&output);
        trace.collect(engine, events, tracks);
    }
    samples
}

#[test]
fn local_away_owner_and_resident_restart_keep_independent_gates() {
    for bypass in [false, true] {
        for partitions in [&[128][..], &[1, 7, 56, 64][..], &[16, 16, 16, 16, 64][..]] {
            let tracks = [TrackId::new(), TrackId::new()];
            let effects = [EffectId::new(), EffectId::new()];
            let channels: Vec<_> = tracks.into_iter().enumerate().map(|(index, id)| {
                let mut channel = routing_channel(id);
                channel.effects.push(routing_effect(effects[index]));
                channel
            }).chain(std::iter::once(routing_channel(TrackId::MASTER))).collect();
            let (mut engine, mut commands, mut events) = graph_engine(&channels);
            let requests = [Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false))];
            let mut identities = [0; 2];
            for index in 0..2 {
                let mut track = EngineTrack::new(tracks[index]);
                track.pan = 0.5;
                track.playback_source.clips.push(constant_clip(32_768, 0.25));
                let effect: Box<dyn vibez_dsp::effect::AudioEffect> = Box::new(HandoffProbe {
                    requested: Arc::clone(&requests[index]), local: index == 0, stopped: false,
                });
                identities[index] = reconfiguration::effect_owner_identity(&*effect);
                track.effects.push(EffectSlot { id: effects[index], effect, bypass: index == 0 && bypass });
                commands.push(EngineCommand::AddTrack(Box::new(track))).unwrap();
            }
            commands.push(EngineCommand::Play).unwrap();
            commands.push(EngineCommand::StartPerformanceCapture).unwrap();
            assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 128], 2)), (0, 0));
            let mut starts = 0;
            let mut sources = Vec::new();
            let mut playing = 0;
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::PerformanceCaptureStarted { effective_at_samples, .. } => {
                        assert_eq!(effective_at_samples, 0);
                        starts += 1;
                    }
                    EngineEvent::ClipCaptureSource { track_id, effective_at_samples, .. } => {
                        assert_eq!(effective_at_samples, 0);
                        sources.push(track_id);
                    }
                    EngineEvent::PlaybackStarted => playing += 1,
                    EngineEvent::Metering { .. } | EngineEvent::TrackMeter { .. }
                    | EngineEvent::PlaybackPosition(_) | EngineEvent::PerformancePosition(_) => {}
                    event => panic!("unexpected overlap setup event: {event:?}"),
                }
            }
            assert_eq!((starts, playing), (1, 1));
            assert_eq!(sources, tracks);
            let running = Readiness { valid: true, awaiting_plan: false, suspended: false,
                handoff: false, restart_requested: false, presentation_fault: false, playing: true, muted: false };
            let mut readiness = vec![Readiness::observe(&engine)];
            assert_eq!(readiness[0], running);
            let full_level = engine.output_gate.last_frame();
            assert!(full_level.into_iter().all(|value| value > 0.1));
            let capture_stop = engine.heard_capture_position();
            assert_eq!(capture_stop, 64);
            let original_plan = std::ptr::from_ref(engine.routing.as_ref()) as usize;
            let mut trace = OverlapTrace::default();
            requests[0].store(true, Ordering::Release);
            let local_audio = overlap_render(&mut engine, &mut events, tracks, &mut trace, partitions);
            let local = engine.device_handoff.pending().unwrap();
            assert!(local.local_recovery);
            assert_eq!((local.handoff_id, local.track_id, local.effect_id, local.position, local.bypass), (1, tracks[0], Some(effects[0]), 0, bypass));
            assert_eq!(trace.owners[0].as_ref().unwrap().owner_identity(), identities[0]);
            assert!(trace.owners[1].is_none());
            assert_eq!(trace.capture_stops, [capture_stop]);
            assert_eq!(trace.order, ["local-handoff", "capture-stop"]);
            readiness.push(Readiness::observe(&engine));
            assert_eq!(readiness[1], Readiness { handoff: true, ..running });
            let local_level = full_level.map(|value| if bypass { value } else { value * 0.5 });
            for pair in local_audio.as_chunks::<2>().0 {
                for channel in 0..2 {
                    assert!((pair[channel] - local_level[channel]).abs() < 1e-7);
                }
            }
            requests[1].store(true, Ordering::Release);
            readiness.push(Readiness::observe(&engine));
            let requested = Readiness { handoff: true, restart_requested: true, muted: true, ..running };
            assert_eq!(readiness[2], requested);
            let loss = overlap_render(&mut engine, &mut events, tracks, &mut trace, partitions);
            assert_eq!(Readiness::observe(&engine), requested);
            let still_local = engine.device_handoff.pending().unwrap();
            assert_eq!((still_local.handoff_id, still_local.track_id, still_local.effect_id, still_local.position, still_local.bypass, still_local.local_recovery), (local.handoff_id, local.track_id, local.effect_id, local.position, local.bypass, local.local_recovery));
            assert!(trace.owners[1].is_none());
            for (frame, pair) in loss.as_chunks::<2>().0.iter().enumerate() {
                for channel in 0..2 {
                    let expected = local_level[channel] * 64usize.saturating_sub(frame) as f32 / 64.0;
                    assert!((pair[channel] - expected).abs() < 1e-7);
                }
            }
            assert_eq!(engine.output_gate.remaining_loss_frames(), 0);
            assert_eq!(engine.output_gate.remaining_resume_frames(), 64);
            let mut owner = trace.owners[0].take().unwrap();
            assert_eq!(owner.handoff_id(), 1);
            assert!(owner.processing_recovery_requested());
            owner.reconfigure_on_main_thread().unwrap();
            let local_plan = routing_plan(&channels);
            let local_plan_identity = std::ptr::from_ref(local_plan.as_ref()) as usize;
            commands.push(EngineCommand::ResumeDeviceReconfiguration { device: owner, routing: local_plan }).unwrap();
            assert_eq!(crate::retirement::tests::allocations(|| engine.drain_commands()), (0, 0));
            trace.collect(&mut engine, &mut events, tracks);
            assert_eq!(reconfiguration::effect_owner_identity(&*engine.tracks[0].effects[0].effect), identities[0]);
            readiness.push(Readiness::observe(&engine));
            assert_eq!(readiness[3], Readiness { restart_requested: true, muted: true, ..running });
            assert_eq!(trace.retired_plans, [original_plan]);
            let gated = overlap_render(&mut engine, &mut events, tracks, &mut trace, &[32]);
            assert_eq!(gated, [0.0; 64]);
            let project = engine.device_handoff.pending().unwrap();
            assert_eq!((project.handoff_id, project.track_id, project.effect_id, project.position, project.bypass, project.local_recovery), (2, tracks[1], Some(effects[1]), 0, false, false));
            assert_eq!(trace.owners[1].as_ref().unwrap().owner_identity(), identities[1]);
            readiness.push(Readiness::observe(&engine));
            assert_eq!(readiness[4], Readiness { suspended: true, handoff: true, muted: true, ..running });
            let mut owner = trace.owners[1].take().unwrap();
            assert_eq!(owner.handoff_id(), 2);
            assert!(!owner.processing_recovery_requested());
            owner.reconfigure_on_main_thread().unwrap();
            let accepted = routing_plan(&channels);
            let accepted_identity = std::ptr::from_ref(accepted.as_ref()) as usize;
            commands.push(EngineCommand::ResumeDeviceReconfiguration { device: owner, routing: accepted }).unwrap();
            assert_eq!(crate::retirement::tests::allocations(|| engine.drain_commands()), (0, 0));
            trace.collect(&mut engine, &mut events, tracks);
            readiness.push(Readiness::observe(&engine));
            assert_eq!(readiness[5], running);
            assert_eq!(std::ptr::from_ref(engine.routing.as_ref()) as usize, accepted_identity);
            assert_eq!(trace.retired_plans, [original_plan, local_plan_identity]);
            for index in 0..2 {
                assert_eq!(reconfiguration::effect_owner_identity(&*engine.tracks[index].effects[0].effect), identities[index]);
            }
            let resumed = overlap_render(&mut engine, &mut events, tracks, &mut trace, partitions);
            for (frame, pair) in resumed.as_chunks::<2>().0.iter().enumerate() {
                for channel in 0..2 {
                    let expected = full_level[channel] * frame.min(64) as f32 / 64.0;
                    assert!((pair[channel] - expected).abs() < 1e-7);
                }
            }
            assert_eq!(Readiness::observe(&engine), running);
            assert_eq!(engine.output_gate.remaining_resume_frames(), 0);
            assert!(engine.device_handoff.retained_parameters().is_empty());
            assert!(trace.owners.iter().all(Option::is_none));
            assert_eq!(trace.capture_stops, [capture_stop]);
            assert_eq!(trace.order, ["local-handoff", "capture-stop", "original-plan-return", "project-handoff", "local-plan-return"]);
            let owner_roundtrips = [0, 1].map(|index| reconfiguration::effect_owner_identity(&*engine.tracks[index].effects[0].effect) == identities[index]);
            assert_eq!(owner_roundtrips, [true, true]);
            let final_position = engine.transport.position();
            let ids = [local.handoff_id, project.handoff_id];
            let scopes = [local.local_recovery, project.local_recovery];
            eprintln!("device-handoff-overlap baseline: bypass={bypass}; partitions={partitions:?}; capture_stop={capture_stop}; ids={ids:?}; scopes={scopes:?}; owner_roundtrips={owner_roundtrips:?}; final_position={final_position}; readiness={readiness:?}; order={:?}; local_bits={:?}; loss_bits={:?}; gated_bits={:?}; resumed_bits={:?}",
                trace.order,
                local_audio.iter().copied().map(f32::to_bits).collect::<Vec<_>>(),
                loss.iter().copied().map(f32::to_bits).collect::<Vec<_>>(),
                gated.iter().copied().map(f32::to_bits).collect::<Vec<_>>(),
                resumed.iter().copied().map(f32::to_bits).collect::<Vec<_>>());
        }
    }
}
