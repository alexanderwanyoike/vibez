//! Output gate loss, resume and plan morph retain their exact sample envelopes.

use super::*;
use compensation_state_test_support::{Fixture, StartingCondition};
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
        let level = fixture.engine.last_mix_frame;
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
        assert_eq!(fixture.engine.compensation_fade_out, 0);
        assert_eq!(fixture.engine.compensation_fade_in, 64);
        let plan = fixture.plan(0);
        fixture.drain_command(EngineCommand::SetRouting(plan));
        assert_eq!(fixture.engine.compensation_plan_fade, 0);
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
        assert_eq!(fixture.engine.compensation_fade_in, 0);
    }
}

#[test]
fn latency_restart_gates_the_healthy_branch_until_main_returns_the_owner() {
    let mut fixture = Fixture::new(StartingCondition::Running);
    let level = fixture.engine.last_mix_frame;
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
    let level = fixture.engine.last_mix_frame;
    let plan = fixture.plan(0);
    fixture.drain_command(EngineCommand::SetRouting(plan));
    assert_eq!(fixture.engine.compensation_plan_fade, 0);
    for pair in render(&mut fixture, &[1, 7, 56, 64]).as_chunks::<2>().0 {
        assert_eq!(*pair, level);
    }
}

#[test]
fn changed_plan_morph_uses_the_last_frame_without_a_global_processing_gate() {
    let mut fixture = Fixture::new(StartingCondition::Running);
    let old = fixture.engine.last_mix_frame;
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
    assert_eq!(fixture.engine.compensation_plan_fade, 64);
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
    assert_eq!(fixture.engine.compensation_plan_fade, 0);
}

#[test]
fn play_during_an_unfinished_loss_preserves_the_remaining_loss_and_resume_envelopes() {
    let mut fixture = Fixture::new(StartingCondition::AwaitingPlan);
    let level = fixture.engine.last_mix_frame;
    render(&mut fixture, &[8]);
    assert_eq!(fixture.engine.compensation_fade_out, 56);
    let plan = fixture.plan(0);
    fixture.drain_command(EngineCommand::SetRouting(plan));
    fixture.drain_command(EngineCommand::Play);
    assert_eq!(fixture.engine.compensation_fade_out, 56);
    assert_eq!(fixture.engine.compensation_fade_in, 64);
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
