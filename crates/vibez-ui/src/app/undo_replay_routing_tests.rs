//! Undo replays the project into the running engine, which must stay audible.

use super::*;
use crate::domains::project::ProjectMsg;
use crate::state::{ProjectTrack, UiClip, UiEffect};
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_engine::engine::{AudioEngine, AudioProcessBlock};

fn gain_effect() -> UiEffect {
    let device = vibez_dsp::factory::create_effect(EffectType::Gain, 44_100.0);
    UiEffect {
        reconfiguration_failed: false,
        latency_samples: Some(0),
        id: EffectId::new(),
        effect_type: EffectType::Gain,
        bypass: false,
        params: device
            .param_descriptors()
            .iter()
            .map(|p| p.default)
            .collect(),
        descriptors: device.param_descriptors(),
        plugin_name: None,
        has_plugin_gui: false,
        plugin_ref: None,
        external_inputs: Vec::new(),
        inactive_sidechains: Vec::new(),
        sidechains: Vec::new(),
    }
}

fn steady_clip(frames: u64) -> UiClip {
    UiClip {
        id: ClipId::new(),
        name: "Pad".into(),
        audio: Arc::new(vibez_core::audio_buffer::DecodedAudio {
            channels: vec![vec![0.5; frames as usize]],
            sample_rate: 44_100,
        }),
        source: None,
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: frames,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
        original_audio: None,
    }
}

/// Settled level once any compensation fade has had time to finish.
fn settled_level(engine: &mut AudioEngine) -> f32 {
    let mut output = [0.0_f32; 1024];
    for _ in 0..8 {
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
    }
    output
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

#[test]
fn undo_while_playing_keeps_a_project_with_built_in_devices_audible() {
    let mut app = super::test_support::app();
    let (mut engine, commands, _events) = AudioEngine::new();
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    let mut track = ProjectTrack::new(TrackId::new(), "Pad".into(), 0);
    track.effects.push(gain_effect());
    Arc::make_mut(&mut app.state.arrangement.timeline)
        .ensure(track.id)
        .clips
        .push(steady_clip(1 << 20));
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track.clone());
    app.replay_track_to_engine(&track);
    app.sync_sidechain_routing();
    app.send_command(EngineCommand::Play);
    assert!(
        settled_level(&mut engine) > 0.1,
        "project plays before the edit"
    );

    app.push_undo_snapshot(None);
    Arc::make_mut(&mut app.state.project_tracks).tracks[0].gain = 0.5;
    let _ = app.update_and_refresh_clips(Message::Project(ProjectMsg::Undo));

    assert!(
        settled_level(&mut engine) > 0.1,
        "Undo must leave the replayed project audible"
    );
}
