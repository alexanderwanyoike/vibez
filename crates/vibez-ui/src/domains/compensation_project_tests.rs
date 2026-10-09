use super::*;
use crate::state::{AppState, UiEffect};
use std::sync::Arc;
use vibez_core::{effect::EffectType, id::EffectId, midi::TrackKind, track::AudioInputRoute};

fn input(name: &str) -> ProjectTrack {
    ProjectTrack::new(TrackId::new(), name.into(), 0)
}
fn instrument(name: &str) -> ProjectTrack {
    let mut track = ProjectTrack::new_instrument(TrackId::new(), name.into(), TrackKind::Midi, 0);
    track.has_instrument = true;
    track
}

#[test]
fn project_and_application_defaults_use_full_compensation() {
    assert!(!vibez_project::Project::default().reduced_latency_monitoring);
    assert!(!ProjectTracksState::default().reduced_latency_monitoring);
    let state = AppState::default();
    assert!(!state.project_tracks.reduced_latency_monitoring);
}

#[test]
fn on_auto_and_off_require_actual_monitored_or_armed_hardware_routes() {
    let mut track = input("Audio");
    let id = track.id;
    let unrelated = TrackId::new();
    for (mode, armed, monitored, expected) in [
        (InputMonitoring::Off, None, None, false),
        (InputMonitoring::Off, Some(id), Some(id), false),
        (InputMonitoring::Auto, None, Some(id), false),
        (InputMonitoring::Auto, Some(unrelated), Some(id), false),
        (InputMonitoring::Auto, Some(id), None, true),
        (InputMonitoring::On, None, None, false),
        (InputMonitoring::On, Some(unrelated), Some(unrelated), false),
        (InputMonitoring::On, Some(id), None, true),
        (InputMonitoring::On, None, Some(id), true),
    ] {
        track.input_monitoring = mode;
        assert_eq!(
            eligible(&track, None, armed, monitored),
            expected,
            "{mode:?} armed{armed:?} monitored{monitored:?}"
        );
    }
    for route in [
        AudioInputRoute::Mono { channel: 0 },
        AudioInputRoute::Stereo { left: 2 },
    ] {
        track.audio_input_route = route;
        track.input_monitoring = InputMonitoring::On;
        assert!(eligible(&track, None, None, Some(id)));
    }
    track.audio_input_route = AudioInputRoute::Resample {
        track_id: unrelated,
    };
    assert!(!eligible(&track, None, Some(id), Some(id)));
}

#[test]
fn remembered_perform_target_survives_unrelated_selection_and_remains_eligible() {
    let mut state = AppState::default();
    let playable = instrument("Playable");
    let audio = input("Selected audio");
    let target = playable.id;
    let audio_id = audio.id;
    let project = Arc::make_mut(&mut state.project_tracks);
    project.reduced_latency_monitoring = true;
    project.tracks = vec![playable, audio];
    state
        .perform
        .sync_instrument_target_from_selection(Some(target), &state.project_tracks.tracks);
    state.arrangement.selected_track = Some(audio_id);
    state.perform.sync_instrument_target_from_selection(
        state.arrangement.selected_track,
        &state.project_tracks.tracks,
    );
    assert_eq!(state.perform.instrument_target(), Some(target));
    assert_eq!(signature_for_state(&state).reduced_tracks, vec![target]);
    assert!(signature(
        &state.project_tracks,
        state.arrangement.selected_track,
        None,
        None,
        48000
    )
    .reduced_tracks
    .is_empty());
}

#[test]
fn only_playable_instruments_and_project_tracks_receive_monitoring_exceptions() {
    let mut project = ProjectTracksState {
        reduced_latency_monitoring: true,
        ..Default::default()
    };
    let mut playable = instrument("Instrument");
    let id = playable.id;
    let mut unloaded = playable.clone();
    unloaded.id = TrackId::new();
    unloaded.has_instrument = false;
    assert!(!eligible(
        &unloaded,
        Some(unloaded.id),
        Some(unloaded.id),
        Some(unloaded.id)
    ));
    playable.solo = false;
    playable.mute = true;
    project.tracks.push(playable);
    project.master.input_monitoring = InputMonitoring::On;
    let master = project.master.id;
    let mut bus = input("Bus");
    bus.input_monitoring = InputMonitoring::On;
    let bus_id = bus.id;
    project.buses.push(bus);
    assert_eq!(
        signature(&project, Some(id), Some(master), Some(bus_id), 48000).reduced_tracks,
        vec![id]
    );
    project.reduced_latency_monitoring = false;
    assert!(
        signature(&project, Some(id), Some(master), Some(bus_id), 48000)
            .reduced_tracks
            .is_empty()
    );
}

#[test]
fn bypass_keeps_cached_device_reports_and_readouts_follow_current_sample_rate() {
    let mut project = ProjectTracksState::default();
    let mut track = instrument("Instrument");
    let id = track.id;
    let effect_id = EffectId::new();
    track.instrument_latency_samples = Some(137);
    track.effects.push(UiEffect {
        latency_samples: Some(480),
        id: effect_id,
        effect_type: EffectType::Gain,
        bypass: false,
        params: vec![1.0],
        descriptors: &[],
        plugin_name: None,
        has_plugin_gui: false,
        plugin_ref: None,
        sidechains: vec![],
        inactive_sidechains: vec![],
        external_inputs: vec![],
    });
    project.tracks.push(track);
    let before = signature(&project, Some(id), None, None, 48000);
    project.tracks[0].effects[0].bypass = true;
    let bypassed = signature(&project, Some(id), None, None, 48000);
    assert_eq!(bypassed, before);
    assert!(bypassed.reports.contains(&(
        RoutingNode {
            channel: id,
            stage: NodeStage::Source
        },
        137
    )));
    assert!(bypassed.reports.contains(&(
        RoutingNode {
            channel: id,
            stage: NodeStage::Effect(effect_id)
        },
        480
    )));
    assert_eq!(
        latency_label(project.tracks[0].effects[0].latency_samples, 48000),
        "480 samples / 10.00 ms"
    );
    assert_eq!(
        latency_label(project.tracks[0].effects[0].latency_samples, 96000),
        "480 samples / 5.00 ms"
    );
    assert_eq!(latency_label(Some(0), 48000), "0 samples / 0.00 ms");
    assert_eq!(latency_label(None, 48000), "Latency unavailable");
    assert_eq!(latency_label(Some(480), 0), "Latency unavailable");
}

#[test]
fn configured_usb_midi_route_and_remembered_perform_route_are_independent() {
    let mut state = AppState::default();
    let first = instrument("Perform");
    let second = instrument("USB MIDI");
    let audio = input("Audio");
    let first_id = first.id;
    let second_id = second.id;
    let audio_id = audio.id;
    let project = Arc::make_mut(&mut state.project_tracks);
    project.reduced_latency_monitoring = true;
    project.tracks = vec![first, second, audio];
    state.perform.sync_instrument_target(Some(first_id));
    state.arrangement.selected_track = Some(second_id);
    assert_eq!(signature_for_state(&state).reduced_tracks, vec![first_id]);
    assert_eq!(
        signature_for_live_routes(&state, Some(second_id)).reduced_tracks,
        vec![first_id, second_id]
    );
    assert_eq!(
        signature_for_live_routes(&state, Some(first_id)).reduced_tracks,
        vec![first_id]
    );
    assert_eq!(
        signature_for_live_routes(&state, Some(audio_id)).reduced_tracks,
        vec![first_id]
    );
}
