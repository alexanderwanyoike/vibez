use super::*;
use crate::playback_source::{PreparedPlaybackSource, PreparedSectionPlaybackSource};
use std::sync::atomic::{AtomicU32, Ordering};
use vibez_core::{
    effect::{EffectType, ParamDescriptor},
    id::EffectId,
    routing::*,
};
use vibez_dsp::{compensation_delay::CompensationDelay, effect::AudioEffect};

struct Drift {
    line: CompensationDelay,
    report: Arc<AtomicU32>,
}
impl AudioEffect for Drift {
    fn latency_samples(&self) -> u32 {
        self.report.load(Ordering::Acquire)
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
        self.line.process_layout(samples, channels);
    }
    fn reset(&mut self) {
        self.line.clear();
    }
}
fn setup() -> (
    AudioEngine,
    Producer<EngineCommand>,
    Consumer<EngineEvent>,
    TrackId,
    EffectId,
    Arc<AtomicU32>,
) {
    let (mut engine, mut commands, events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    let report = Arc::new(AtomicU32::new(521));
    commands.push(EngineCommand::SetSampleRate(128)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track, "Stop".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: effect,
            effect: Box::new(Drift {
                line: CompensationDelay::prepare(521, 2, 1042).unwrap(),
                report: Arc::clone(&report),
            }),
            position: None,
        })
        .unwrap();
    let channels = [
        RoutingChannel {
            id: track,
            is_bus: false,
            sends: vec![],
            effects: vec![RoutingEffect {
                inactive_inputs: vec![],
                id: effect,
                inputs: vec![],
                assignments: vec![],
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
        521,
    )];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&channels, 64, &reports, &[], 1)
                .unwrap(),
        ))
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                8.0,
                true,
                vec![(track, PreparedPlaybackSource::new(vec![], vec![], vec![]))],
            ),
        )))
        .unwrap();
    engine.process(&mut [0.0; 128], 2);
    (engine, commands, events, track, effect, report)
}

#[test]
fn stop_and_unload_retire_delayed_sources_without_resurrecting_playing_state() {
    for unload in [false, true] {
        let (mut engine, mut commands, mut events, _, _, _) = setup();
        while events.pop().is_ok() {}
        commands
            .push(if unload {
                EngineCommand::UnloadAudio
            } else {
                EngineCommand::Stop
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 128], 2)),
            (0, 0)
        );
        for _ in 0..12 {
            engine.process(&mut [0.0; 128], 2);
        }
        let notifications: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
        assert!(notifications
            .iter()
            .any(|event| matches!(event, EngineEvent::PlaybackStopped)));
        assert!(!notifications.iter().any(|event| matches!(
            event,
            EngineEvent::SectionTransitioned { .. } | EngineEvent::SectionCaptureSource { .. }
        )));
        assert!(notifications
            .iter()
            .any(|event| matches!(event, EngineEvent::SectionQueueCancelled { .. })));
    }
}

#[test]
fn invalid_configuration_and_rejected_updates_close_capture_at_the_heard_stop_boundary() {
    for failure in 0..3 {
        let (mut engine, mut commands, mut events, track, effect, report) = setup();
        for _ in 0..8 {
            engine.process(&mut [0.0; 128], 2);
            while events.pop().is_ok() {}
        }
        commands
            .push(EngineCommand::StartPerformanceCapture)
            .unwrap();
        engine.process(&mut [0.0; 128], 2);
        while events.pop().is_ok() {}
        let expected = engine.heard_capture_position();
        match failure {
            0 => report.store(522, Ordering::Release),
            1 => {
                commands
                    .push(EngineCommand::RemoveEffect(track, effect))
                    .unwrap();
                commands
                    .push(EngineCommand::RejectRoutingUpdate {
                        reason: "Controlled invalid edit".into(),
                    })
                    .unwrap();
            }
            _ => {
                let slot = engine.tracks[0].effects.remove(0);
                commands
                    .push(EngineCommand::RejectDeviceReconfiguration {
                        device: reconfiguration::DeviceReconfiguration::Effect {
                            track_id: track,
                            position: 0,
                            slot,
                        },
                        reason: "Controlled reactivation failure".into(),
                    })
                    .unwrap();
            }
        }
        engine.process(&mut [0.0; 128], 2);
        assert!(!engine.transport.is_playing());
        let notifications: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
        assert!(notifications.iter().any(|event|matches!(event,EngineEvent::PerformanceCaptureStopped{effective_at_samples} if *effective_at_samples==expected)),"failure{failure}");
    }
}

#[test]
fn failure_capture_stop_survives_a_full_ui_event_ring() {
    let (mut engine, _, mut events, _, _, report) = setup();
    for _ in 0..10 {
        engine.process(&mut [0.0; 128], 2);
        while events.pop().is_ok() {}
    }
    let expected = engine.heard_capture_position();
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    report.store(522, Ordering::Release);
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 128], 2)),
        (0, 0)
    );
    assert_eq!(engine.pending_capture_stop, Some(expected));
    while events.pop().is_ok() {}
    engine.process(&mut [0.0; 128], 2);
    let delivered: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert!(delivered.iter().any(|event| matches!(event,EngineEvent::PerformanceCaptureStopped{effective_at_samples} if *effective_at_samples==expected)));
    assert!(delivered.iter().any(|event| matches!(event,EngineEvent::CompensationInvalid{reason,..} if reason.contains("latency"))));
    assert_eq!(engine.pending_capture_stop, None);
}
