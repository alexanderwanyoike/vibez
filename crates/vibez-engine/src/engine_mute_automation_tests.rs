use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::id::{ClipId, TrackId};

use super::*;

fn constant_audio(frames: usize) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![0.8; frames]],
        sample_rate: 100,
    })
}

fn add_constant_track(commands: &mut rtrb::Producer<EngineCommand>, track_id: TrackId) {
    commands.push(EngineCommand::SetSampleRate(100)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Mute automation".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddClip {
            track_id,
            clip_id: ClipId::new(),
            audio: constant_audio(256),
            position: 0,
            source_offset: 0,
            duration: 256,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
}

#[test]
fn mute_step_inside_buffer_starts_the_shared_ramp_at_its_exact_sample() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    add_constant_track(&mut commands, track_id);
    let mut lane = AutomationLane::new(AutomationTarget::TrackMute);
    lane.insert_point(AutomationPoint {
        beat: 0.04,
        value: 1.0,
        curve: 0.0,
    });
    commands
        .push(EngineCommand::SetAutomationLane { track_id, lane })
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();

    let mut output = vec![0.0; 80 * 2];
    engine.process(&mut output, 2);

    let frame = |index: usize| output[index * 2].abs();
    assert!(frame(3) > 0.5, "signal must remain open before the step");
    assert!(
        frame(4) < frame(3),
        "the anti-click ramp must begin on the point's exact sample"
    );
    assert_eq!(frame(67), 0.0, "the 64-frame ramp must reach silence");
    assert_eq!(frame(79), 0.0);
}

#[test]
fn manual_mute_override_holds_until_automation_is_reenabled() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    add_constant_track(&mut commands, track_id);
    let mut lane = AutomationLane::new(AutomationTarget::TrackMute);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 1.0,
        curve: 0.0,
    });
    commands
        .push(EngineCommand::SetAutomationLane { track_id, lane })
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = vec![0.0; 80 * 2];
    engine.process(&mut output, 2);
    assert!(output.iter().all(|sample| *sample == 0.0));

    commands
        .push(EngineCommand::SetTrackMute(track_id, false))
        .unwrap();
    output.fill(0.0);
    engine.process(&mut output, 2);
    assert!(
        output[79 * 2].abs() > 0.5,
        "manual unmute must override the mute lane"
    );

    commands
        .push(EngineCommand::SetAutomationOverride {
            track_id,
            target: AutomationTarget::TrackMute,
            overridden: false,
        })
        .unwrap();
    output.fill(0.0);
    engine.process(&mut output, 2);
    assert_eq!(
        output[79 * 2],
        0.0,
        "re-enable must return control to the lane"
    );
}
