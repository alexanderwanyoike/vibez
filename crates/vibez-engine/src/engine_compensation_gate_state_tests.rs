//! Output gate loss, resume and plan morph retain their exact sample envelopes.

use super::*;
use compensation_state_test_support::{Fixture, Readiness, StartingCondition};
use std::sync::atomic::Ordering;

fn render(fixture: &mut Fixture, partitions: &[usize]) -> Vec<f32> {
    let mut result = Vec::with_capacity(partitions.iter().sum::<usize>() * 2);
    for frames in partitions {
        let mut output = vec![0.0; frames * 2];
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.process(&mut output, 2)),
            (0, 0)
        );
        result.extend_from_slice(&output);
        fixture.drain_events();
    }
    result
}

#[test]
fn structural_wait_and_resume_keep_the_64_frame_envelopes_across_callback_partitions() {
    for partitions in [&[128][..], &[1, 7, 56, 64][..], &[16, 16, 16, 16, 64][..]] {
        let mut fixture = Fixture::new(StartingCondition::AwaitingPlan);
        let level = fixture.engine.output_gate.last_frame();
        let paused = render(&mut fixture, partitions);
        for (frame, pair) in paused.as_chunks::<2>().0.iter().enumerate() {
            for channel in 0..2 {
                let expected = level[channel] * 64usize.saturating_sub(frame) as f32 / 64.0;
                assert!(
                    (pair[channel] - expected).abs() < 1e-7,
                    "loss {partitions:?}, frame={frame}, channel={channel}: {} != {expected}",
                    pair[channel]
                );
            }
        }
        assert!(fixture.engine.transport.is_playing());
        assert_eq!(fixture.engine.output_gate.remaining_loss_frames(), 0);
        assert_eq!(fixture.engine.output_gate.remaining_resume_frames(), 64);
        let plan = fixture.plan(0);
        fixture.drain_command(EngineCommand::SetRouting(plan));
        assert_eq!(fixture.engine.output_gate.remaining_morph_frames(), 0);
        let resumed = render(&mut fixture, partitions);
        for (frame, pair) in resumed.as_chunks::<2>().0.iter().enumerate() {
            for channel in 0..2 {
                let expected = level[channel] * frame.min(64) as f32 / 64.0;
                assert!(
                    (pair[channel] - expected).abs() < 1e-7,
                    "resume {partitions:?}, frame={frame}, channel={channel}: {} != {expected}",
                    pair[channel]
                );
            }
        }
        assert_eq!(fixture.engine.output_gate.remaining_resume_frames(), 0);
    }
}

#[test]
fn latency_restart_gates_the_healthy_branch_until_main_returns_the_owner() {
    let mut fixture = Fixture::new(StartingCondition::Running);
    let level = fixture.engine.output_gate.last_frame();
    fixture.requested.store(1, Ordering::Release);
    let mut output = [0.0; 128];
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.process(&mut output, 2)),
        (0, 0)
    );
    assert!(fixture.engine.project_processing_muted());
    assert!(fixture.engine.compensation_suspended);
    assert!(fixture.engine.transport.is_playing());
    for (frame, pair) in output.as_chunks::<2>().0.iter().enumerate() {
        for channel in 0..2 {
            let expected = level[channel] * (64 - frame) as f32 / 64.0;
            assert!((pair[channel] - expected).abs() < 1e-7);
        }
    }
    let mut owner = fixture
        .drain_events()
        .into_iter()
        .find_map(|event| match event {
            EngineEvent::DeviceReconfiguration(device) => Some(device),
            _ => None,
        })
        .expect("latency change hands the owner to main");
    assert_eq!(render(&mut fixture, &[64]), [0.0; 128]);
    owner.reconfigure_on_main_thread().unwrap();
    let plan = fixture.plan(owner.latency_samples());
    fixture.drain_command(EngineCommand::ResumeDeviceReconfiguration {
        device: owner,
        routing: plan,
    });
    assert!(!fixture.engine.project_processing_muted());
    let resumed = render(&mut fixture, &[64, 64]);
    for (frame, pair) in resumed.as_chunks::<2>().0.iter().enumerate() {
        for channel in 0..2 {
            let expected = level[channel] * frame.min(64) as f32 / 64.0;
            assert!(
                (pair[channel] - expected).abs() < 1e-7,
                "frame={frame}, channel={channel}: {} != {expected}",
                pair[channel]
            );
        }
    }
    assert!(fixture.engine.transport.is_playing());
}

#[test]
fn unchanged_publication_keeps_audio_and_does_not_start_an_output_morph() {
    let mut fixture = Fixture::new(StartingCondition::Running);
    let level = fixture.engine.output_gate.last_frame();
    let plan = fixture.plan(0);
    fixture.drain_command(EngineCommand::SetRouting(plan));
    assert_eq!(fixture.engine.output_gate.remaining_morph_frames(), 0);
    for pair in render(&mut fixture, &[1, 7, 56, 64]).as_chunks::<2>().0 {
        assert_eq!(*pair, level);
    }
}

#[test]
fn changed_plan_morph_uses_the_last_frame_without_a_global_processing_gate() {
    let mut fixture = Fixture::new(StartingCondition::Running);
    let old = fixture.engine.output_gate.last_frame();
    let command = EngineCommand::insert_built_in_effect(
        fixture.tracks[1],
        vibez_core::id::EffectId::new(),
        vibez_core::effect::EffectType::Gain,
        DEFAULT_SAMPLE_RATE,
    );
    fixture.drain_command(command);
    fixture.drain_command(EngineCommand::SetTrackGain(fixture.tracks[1], 0.5));
    let plan = fixture.plan(0);
    fixture.drain_command(EngineCommand::SetRouting(plan));
    assert!(!fixture.engine.project_processing_muted());
    assert_eq!(fixture.engine.output_gate.remaining_morph_frames(), 64);
    let output = render(&mut fixture, &[1, 7, 56, 64]);
    for (frame, pair) in output.as_chunks::<2>().0.iter().enumerate() {
        let blend = frame.min(64) as f32 / 64.0;
        for channel in 0..2 {
            let expected = old[channel] * (1.0 - blend) + old[channel] * 0.75 * blend;
            assert!(
                (pair[channel] - expected).abs() < 1e-7,
                "frame={frame}, channel={channel}: {} != {expected}",
                pair[channel]
            );
        }
    }
    assert_eq!(fixture.engine.output_gate.remaining_morph_frames(), 0);
}

#[test]
fn play_during_an_unfinished_loss_preserves_the_remaining_loss_and_resume_envelopes() {
    let mut fixture = Fixture::new(StartingCondition::AwaitingPlan);
    let level = fixture.engine.output_gate.last_frame();
    render(&mut fixture, &[8]);
    assert_eq!(fixture.engine.output_gate.remaining_loss_frames(), 56);
    let plan = fixture.plan(0);
    fixture.drain_command(EngineCommand::SetRouting(plan));
    fixture.drain_command(EngineCommand::Play);
    assert_eq!(fixture.engine.output_gate.remaining_loss_frames(), 56);
    assert_eq!(fixture.engine.output_gate.remaining_resume_frames(), 64);
    let output = render(&mut fixture, &[32, 32, 64]);
    for (frame, pair) in output.as_chunks::<2>().0.iter().enumerate() {
        let gain = if frame < 56 {
            (56 - frame) as f32 / 64.0
        } else {
            (frame - 56).min(64) as f32 / 64.0
        };
        for channel in 0..2 {
            let expected = level[channel] * gain;
            assert!(
                (pair[channel] - expected).abs() < 1e-7,
                "frame={frame}, channel={channel}: {} != {expected}",
                pair[channel]
            );
        }
    }
}

#[test]
fn rejected_plan_and_accepted_recovery_keep_audio_envelopes_across_callback_partitions() {
    for partitions in [&[128][..], &[1, 7, 56, 64][..], &[16, 16, 16, 16, 64][..]] {
        let mut fixture = Fixture::new(StartingCondition::AwaitingPlan);
        let level = fixture.engine.output_gate.last_frame();
        assert!(level.into_iter().all(|sample| sample > 0.1));
        let previous_plan = std::ptr::from_ref(fixture.engine.routing.as_ref());
        let stopped_at = fixture.engine.transport.position();
        fixture.drain_command(EngineCommand::RejectRoutingUpdate {
            reason: "characterized failed plan publication".into(),
        });
        assert!(!fixture.engine.compensation_valid);
        assert!(fixture.engine.graph_edit_pending);
        assert!(!fixture.engine.compensation_suspended);
        assert!(fixture.engine.pending_device_reconfiguration.is_none());
        assert!(!fixture.engine.presentation_state.is_faulted());
        assert!(!fixture.engine.transport.is_playing());
        assert!(fixture.engine.project_processing_muted());
        assert_eq!(
            std::ptr::from_ref(fixture.engine.routing.as_ref()),
            previous_plan
        );
        let events = fixture.drain_events();
        let failure_order = events
            .iter()
            .filter_map(|event| match event {
                EngineEvent::PlaybackStopped => Some("stopped"),
                EngineEvent::CompensationFailed { reason } => {
                    assert_eq!(reason, "characterized failed plan publication");
                    Some("failed")
                }
                EngineEvent::RoutingRetired(_) | EngineEvent::DeviceReconfiguration(_) => {
                    panic!("a rejected plan must retain the installed graph and device owners")
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(failure_order, ["stopped", "failed"]);
        let failed = render(&mut fixture, partitions);
        for (frame, pair) in failed.as_chunks::<2>().0.iter().enumerate() {
            for channel in 0..2 {
                let expected = level[channel] * 64usize.saturating_sub(frame) as f32 / 64.0;
                assert!(
                    (pair[channel] - expected).abs() < 1e-7,
                    "failed {partitions:?}, frame={frame}, channel={channel}: {} != {expected}",
                    pair[channel]
                );
            }
        }
        assert_eq!(fixture.engine.transport.position(), stopped_at);
        assert_eq!(fixture.engine.output_gate.remaining_loss_frames(), 0);
        assert_eq!(fixture.engine.output_gate.remaining_resume_frames(), 64);
        let plan = fixture.plan(0);
        let accepted_plan = std::ptr::from_ref(plan.as_ref());
        fixture.drain_command(EngineCommand::SetRouting(plan));
        assert!(fixture.engine.compensation_valid);
        assert!(!fixture.engine.graph_edit_pending);
        assert!(!fixture.engine.transport.is_playing());
        assert_eq!(fixture.engine.output_gate.remaining_morph_frames(), 0);
        let events = fixture.drain_events();
        let retired = events
            .iter()
            .filter_map(|event| match event {
                EngineEvent::RoutingRetired(plan) => Some(std::ptr::from_ref(plan.as_ref())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(retired, [previous_plan]);
        assert_eq!(
            std::ptr::from_ref(fixture.engine.routing.as_ref()),
            accepted_plan
        );
        assert!(!events.iter().any(|event| matches!(
            event,
            EngineEvent::PlaybackStarted | EngineEvent::CompensationFailed { .. }
        )));
        fixture.drain_command(EngineCommand::Play);
        assert!(fixture.engine.transport.is_playing());
        assert!(!fixture.engine.project_processing_muted());
        assert_eq!(
            fixture
                .drain_events()
                .iter()
                .filter(|event| matches!(event, EngineEvent::PlaybackStarted))
                .count(),
            1
        );
        let resumed = render(&mut fixture, partitions);
        for (frame, pair) in resumed.as_chunks::<2>().0.iter().enumerate() {
            for channel in 0..2 {
                let expected = level[channel] * frame.min(64) as f32 / 64.0;
                assert!(
                    (pair[channel] - expected).abs() < 1e-7,
                    "recovered {partitions:?}, frame={frame}, channel={channel}: {} != {expected}",
                    pair[channel]
                );
            }
        }
        assert_eq!(fixture.engine.transport.position(), stopped_at + 128);
        assert_eq!(fixture.engine.output_gate.remaining_resume_frames(), 0);
        assert!(fixture.engine.compensation_valid);
        assert!(!fixture.engine.graph_edit_pending);
        assert!(!fixture.engine.presentation_state.is_faulted());
        assert!(!fixture.engine.project_processing_muted());
    }
}

#[test]
fn rejected_structural_plan_survives_play_and_automation_until_full_acceptance() {
    for partitions in [&[128][..], &[1, 7, 56, 64][..], &[16, 16, 16, 16, 64][..]] {
        let mut fixture = Fixture::new(StartingCondition::Running);
        fixture.engine.process(&mut [], 2);
        fixture.drain_events();
        let level = fixture.engine.output_gate.last_frame();
        assert!(level.into_iter().all(|sample| sample > 0.1));
        let capture_stop = fixture.engine.compensation_callback_heard_start;
        assert_eq!(capture_stop, 64);
        let original_plan = std::ptr::from_ref(fixture.engine.routing.as_ref());
        let effect_identity = reconfiguration::effect_owner_identity(
            &*fixture.engine.tracks[0].effects[0].effect,
        );
        let running = Readiness {
            valid: true,
            awaiting_plan: false,
            suspended: false,
            handoff: false,
            restart_requested: false,
            presentation_fault: false,
            playing: true,
            muted: false,
        };
        let observed_running = Readiness::observe(&fixture.engine);
        assert_eq!(observed_running, running);
        fixture.drain_command(EngineCommand::RemoveEffect(
            fixture.tracks[0],
            fixture.effect,
        ));
        let waiting = Readiness {
            awaiting_plan: true,
            muted: true,
            ..running
        };
        let observed_waiting = Readiness::observe(&fixture.engine);
        assert_eq!(observed_waiting, waiting);
        assert_eq!(std::ptr::from_ref(fixture.engine.routing.as_ref()), original_plan);
        let reason = String::from("plan overlap retains a rejected structural expectation");
        let reason_identity = reason.as_ptr();
        fixture.drain_command(EngineCommand::RejectRoutingUpdate { reason });
        let invalid = Readiness {
            valid: false,
            playing: false,
            ..waiting
        };
        let observed_invalid = Readiness::observe(&fixture.engine);
        assert_eq!(observed_invalid, invalid);
        assert_eq!(std::ptr::from_ref(fixture.engine.routing.as_ref()), original_plan);
        let mut order = Vec::new();
        for event in fixture.drain_events() {
            match event {
                EngineEvent::DisposeEffect(owner) => {
                    let effect = owner.take();
                    assert_eq!(reconfiguration::effect_owner_identity(&*effect), effect_identity);
                    order.push("effect-return");
                }
                EngineEvent::PerformanceCaptureStopped { effective_at_samples } => {
                    assert_eq!(effective_at_samples, capture_stop);
                    order.push("capture-stop");
                }
                EngineEvent::PlaybackStopped => order.push("playback-stop"),
                EngineEvent::CompensationFailed { reason } => {
                    assert_eq!(reason, "plan overlap retains a rejected structural expectation");
                    assert_eq!(reason.as_ptr(), reason_identity);
                    order.push("rejection");
                }
                event => panic!("unexpected rejection event: {event:?}"),
            }
        }
        assert_eq!(order, ["effect-return", "capture-stop", "playback-stop", "rejection"]);
        let loss = render(&mut fixture, partitions);
        for (frame, pair) in loss.as_chunks::<2>().0.iter().enumerate() {
            for channel in 0..2 {
                let expected = level[channel] * 64usize.saturating_sub(frame) as f32 / 64.0;
                assert!((pair[channel] - expected).abs() < 1e-7);
            }
        }
        assert_eq!(Readiness::observe(&fixture.engine), invalid);
        assert_eq!(fixture.engine.output_gate.remaining_loss_frames(), 0);
        assert_eq!(fixture.engine.output_gate.remaining_resume_frames(), 64);
        fixture.drain_command(EngineCommand::Play);
        let invalid_playing = Readiness { playing: true, ..invalid };
        let observed_play = Readiness::observe(&fixture.engine);
        assert_eq!(observed_play, invalid_playing);
        assert_eq!(std::ptr::from_ref(fixture.engine.routing.as_ref()), original_plan);
        let events = fixture.drain_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], EngineEvent::PlaybackStarted));
        order.push("playback-start");
        let play_muted = render(&mut fixture, &[32]);
        assert_eq!(play_muted, [0.0; 64]);
        let automation = fixture.plan(0);
        let automation_identity = std::ptr::from_ref(automation.as_ref());
        fixture.drain_command(EngineCommand::UpdateAutomationRouting(automation));
        let observed_automation = Readiness::observe(&fixture.engine);
        assert_eq!(observed_automation, invalid_playing);
        assert_eq!(std::ptr::from_ref(fixture.engine.routing.as_ref()), automation_identity);
        let events = fixture.drain_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            EngineEvent::RoutingRetired(plan) if std::ptr::from_ref(plan.as_ref()) == original_plan
        ));
        drop(events);
        order.push("original-plan-return");
        let automation_muted = render(&mut fixture, &[32]);
        assert_eq!(automation_muted, [0.0; 64]);
        assert_eq!(Readiness::observe(&fixture.engine), invalid_playing);
        let accepted = fixture.plan(0);
        let accepted_identity = std::ptr::from_ref(accepted.as_ref());
        fixture.drain_command(EngineCommand::SetRouting(accepted));
        let observed_accepted = Readiness::observe(&fixture.engine);
        assert_eq!(observed_accepted, running);
        assert_eq!(std::ptr::from_ref(fixture.engine.routing.as_ref()), accepted_identity);
        let events = fixture.drain_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            EngineEvent::RoutingRetired(plan) if std::ptr::from_ref(plan.as_ref()) == automation_identity
        ));
        drop(events);
        order.push("automation-plan-return");
        assert_eq!(fixture.engine.output_gate.remaining_morph_frames(), 0);
        let resumed = render(&mut fixture, partitions);
        for (frame, pair) in resumed.as_chunks::<2>().0.iter().enumerate() {
            for channel in 0..2 {
                let expected = level[channel] * frame.min(64) as f32 / 64.0;
                assert!((pair[channel] - expected).abs() < 1e-7);
            }
        }
        assert_eq!(Readiness::observe(&fixture.engine), running);
        assert_eq!(fixture.engine.transport.position(), 256);
        assert_eq!(fixture.engine.output_gate.remaining_resume_frames(), 0);
        assert_eq!(
            order,
            ["effect-return", "capture-stop", "playback-stop", "rejection", "playback-start", "original-plan-return", "automation-plan-return"]
        );
        assert!(fixture.drain_events().is_empty());
        let readiness = [
            observed_running,
            observed_waiting,
            observed_invalid,
            observed_play,
            observed_automation,
            observed_accepted,
        ]
        .map(|state| [
            u8::from(state.valid),
            u8::from(state.awaiting_plan),
            u8::from(state.suspended),
            u8::from(state.handoff),
            u8::from(state.restart_requested),
            u8::from(state.presentation_fault),
            u8::from(state.playing),
            u8::from(state.muted),
        ]);
        eprintln!(
            "compensation-plan-overlap baseline: partitions={partitions:?}; capture_stop={capture_stop}; readiness={readiness:?}; order={order:?}; loss_bits={:?}; play_muted_bits={:?}; automation_muted_bits={:?}; resumed_bits={:?}",
            loss.iter().copied().map(f32::to_bits).collect::<Vec<_>>(),
            play_muted.iter().copied().map(f32::to_bits).collect::<Vec<_>>(),
            automation_muted.iter().copied().map(f32::to_bits).collect::<Vec<_>>(),
            resumed.iter().copied().map(f32::to_bits).collect::<Vec<_>>(),
        );
    }
}
