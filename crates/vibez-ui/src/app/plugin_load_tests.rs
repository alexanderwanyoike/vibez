//! Plugin completion ordering at the UI-to-engine publication boundary.

use super::{test_support::app, *};
use crate::state::{ProjectTrack, UiEffect};
use vibez_core::effect::{EffectType, PluginDeviceInfo};
use vibez_core::id::{EffectId, TrackId};

fn reload(track: TrackId, effect: EffectId, device: &PluginDeviceInfo) -> PluginLoadResult {
    PluginLoadResult {
        load_token: Default::default(),
        track_id: track,
        effect_id: effect,
        plugin_name: device.name.clone(),
        effect: Some(vibez_dsp::factory::create_effect(
            EffectType::Gain,
            44_100.0,
        )),
        gui_raw_ptr: None,
        clap_partial: None,
        vst3_partial: None,
        sample_rate: 44_100.0,
        device_ref: device.clone(),
        state_ptr: None,
        pending_state: None,
        position: Some(0),
    }
}

#[test]
fn repeated_restore_completion_publishes_only_one_native_effect() {
    let (mut app, track_id, effect_id, device, mut commands) = setup();
    let key = PluginGuiKey::Effect {
        track_id,
        effect_id,
    };
    let token = app.plugin_load_requests.begin(key);
    let mut first = reload(track_id, effect_id, &device);
    first.load_token = token;
    app.plugin_effect_tx.send(first).unwrap();
    let mut duplicate = reload(track_id, effect_id, &device);
    duplicate.load_token = token;
    app.plugin_effect_tx.send(duplicate).unwrap();

    app.poll_plugin_loads();
    let mut added = 0;
    while let Ok(command) = commands.pop() {
        if matches!(command, EngineCommand::AddPluginEffect { .. }) {
            added += 1;
        }
    }
    assert_eq!(
        added, 1,
        "duplicate asynchronous completions must not install two owners"
    );
}

fn setup() -> (
    App,
    TrackId,
    EffectId,
    PluginDeviceInfo,
    rtrb::Consumer<EngineCommand>,
) {
    let mut app = app();
    let track_id = TrackId::new();
    let effect_id = EffectId::new();
    let device = PluginDeviceInfo {
        format: "clap".into(),
        uid: "test.compressor".into(),
        path: "/test.clap".into(),
        name: "Compressor".into(),
        state_b64: None,
    };
    let mut track = ProjectTrack::new(track_id, "Bass".into(), 0);
    track.effects.push(UiEffect {
        latency_samples: None,
        id: effect_id,
        effect_type: EffectType::Gain,
        bypass: false,
        params: vec![],
        descriptors: &[],
        plugin_name: Some(device.name.clone()),
        has_plugin_gui: false,
        plugin_ref: Some(device.clone()),
        inactive_sidechains: Default::default(),
        sidechains: vec![],
        external_inputs: vec![],
    });
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    let (producer, commands) = rtrb::RingBuffer::new(16);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(producer);
    (app, track_id, effect_id, device, commands)
}

#[test]
fn only_the_latest_reload_is_published_in_both_completion_orders() {
    for latest_first in [false, true] {
        let (mut app, track, effect, device, mut commands) = setup();
        let key = PluginGuiKey::Effect {
            track_id: track,
            effect_id: effect,
        };
        let old = app.plugin_load_requests.begin(key);
        let current = app.plugin_load_requests.begin(key);
        let mut stale = reload(track, effect, &device);
        stale.load_token = old;
        stale.effect.as_mut().unwrap().set_param(0, 0.1);
        let mut latest = reload(track, effect, &device);
        latest.load_token = current;
        latest.effect.as_mut().unwrap().set_param(0, 0.9);
        for result in if latest_first {
            [latest, stale]
        } else {
            [stale, latest]
        } {
            app.plugin_effect_tx.send(result).unwrap();
        }
        app.poll_plugin_loads();
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params[0],
            0.9
        );
        let mut added = 0;
        while let Ok(command) = commands.pop() {
            if matches!(command, EngineCommand::AddPluginEffect { .. }) {
                added += 1;
            }
        }
        assert_eq!(added, 1);
    }
}

#[test]
fn project_reset_rejects_a_completion_even_if_the_same_slot_identity_returns() {
    let (mut app, track, effect, device, mut commands) = setup();
    let key = PluginGuiKey::Effect {
        track_id: track,
        effect_id: effect,
    };
    let old = app.plugin_load_requests.begin(key);
    let saved_tracks = Arc::clone(&app.state.project_tracks);
    app.clear_project_runtime();
    app.state.project_tracks = saved_tracks;
    let mut stale = reload(track, effect, &device);
    stale.load_token = old;
    app.plugin_effect_tx.send(stale).unwrap();
    app.poll_plugin_loads();
    while let Ok(command) = commands.pop() {
        assert!(!matches!(command, EngineCommand::AddPluginEffect { .. }));
    }
    assert!(app.state.find_track(track).unwrap().effects[0]
        .external_inputs
        .is_empty());
}
