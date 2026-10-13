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

/// One audio track with a built-in device, already playing a steady clip.
fn playing_project(app: &mut App, engine: &mut AudioEngine) -> (TrackId, EffectId) {
    let mut track = ProjectTrack::new(TrackId::new(), "Pad".into(), 0);
    let effect = gain_effect();
    let effect_id = effect.id;
    track.effects.push(effect);
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
    assert!(settled_level(engine) > 0.1, "project plays before the edit");
    (track.id, effect_id)
}

#[test]
fn undo_while_playing_keeps_a_project_with_built_in_devices_audible() {
    let mut app = super::test_support::app();
    let (mut engine, commands, _events) = AudioEngine::new();
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    playing_project(&mut app, &mut engine);

    app.push_undo_snapshot(None);
    Arc::make_mut(&mut app.state.project_tracks).tracks[0].gain = 0.5;
    let _ = app.update_and_refresh_clips(Message::Project(ProjectMsg::Undo));

    assert!(
        settled_level(&mut engine) > 0.1,
        "Undo must leave the replayed project audible"
    );
}

#[test]
fn a_plugin_reloading_into_its_retained_slot_keeps_the_project_audible() {
    let mut app = super::test_support::app();
    let (mut engine, commands, _events) = AudioEngine::new();
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    let (track_id, effect_id) = playing_project(&mut app, &mut engine);

    // Undo and project open retain the slot, so the routing model is already
    // final when the loaded instance arrives.
    app.send_command(EngineCommand::AddPluginEffect {
        track_id,
        effect_id,
        effect: vibez_dsp::factory::create_effect(EffectType::Gain, 44_100.0),
        position: None,
    });
    let _ = app.update_and_refresh_clips(Message::Project(ProjectMsg::ToggleFileMenu));

    assert!(
        settled_level(&mut engine) > 0.1,
        "a reloaded plugin must not leave the project muted"
    );
}
