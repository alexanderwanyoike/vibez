//! Routing invalidation during idle messages, metering and capability restoration.
use super::{test_support::app, *};
use crate::state::{ProjectTrack, UiEffect};
use vibez_core::{
    effect::EffectType,
    id::{EffectId, TrackId},
    routing::{ExternalInputId, RoutingGraph, SidechainAssignment, SourceTap},
};

#[test]
fn metered_ticks_and_mouse_messages_do_not_rebuild_the_project_routing_model() {
    let mut app = app();
    let track = ProjectTrack::new(TrackId::new(), "Bass".into(), 0);
    let id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    app.sync_sidechain_routing();
    let project = Arc::clone(&app.state.project_tracks);
    super::sidechain::MODEL_BUILDS.with(|count| count.set(0));
    let (mut events, consumer) = rtrb::RingBuffer::new(8);
    app.event_rx = Some(consumer);
    for _ in 0..20 {
        events
            .push(EngineEvent::TrackMeter {
                track_id: id,
                peak_l: 0.8,
                peak_r: 0.5,
            })
            .unwrap();
        events
            .push(EngineEvent::Metering {
                peak_l: 0.6,
                peak_r: 0.4,
                rms_l: 0.3,
                rms_r: 0.2,
            })
            .unwrap();
        app.poll_engine_events();
        let _ = app.update_and_refresh_clips(Message::View(
            crate::domains::view::ViewMsg::CursorMoved(10.0, 20.0),
        ));
        let _ = app.update_and_refresh_clips(Message::Arrangement(
            crate::domains::arrangement::ArrangementMsg::EngineTrackMeter {
                track_id: id,
                peak_l: 0.8,
                peak_r: 0.5,
            },
        ));
    }
    super::sidechain::MODEL_BUILDS.with(|count| assert_eq!(count.get(), 0));
    assert!(Arc::ptr_eq(&project, &app.state.project_tracks));
    assert_eq!(app.track_meter_peaks[&id], (0.8, 0.5));
    assert_eq!(app.track_meter_peaks[&TrackId::MASTER], (0.6, 0.4));
    Arc::make_mut(&mut app.state.project_tracks).tracks[0].name = "Renamed".into();
    app.sync_sidechain_routing();
    super::sidechain::MODEL_BUILDS.with(|count| assert_eq!(count.get(), 1));
    Arc::make_mut(&mut app.state.arrangement.timeline);
    app.sync_sidechain_routing();
    Arc::make_mut(&mut app.state.perform.sections);
    app.sync_sidechain_routing();
    Arc::make_mut(&mut app.state.perform.clips);
    app.sync_sidechain_routing();
    super::sidechain::MODEL_BUILDS.with(|count| assert_eq!(count.get(), 4));
}

fn effect(source: TrackId) -> UiEffect {
    let device = vibez_dsp::factory::create_effect(EffectType::Compressor, 44_100.0);
    UiEffect {
        latency_samples: Some(0),
        id: EffectId::new(),
        effect_type: EffectType::Compressor,
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
        external_inputs: device.external_inputs().to_vec(),
        inactive_sidechains: vec![],
        sidechains: vec![SidechainAssignment {
            input_id: ExternalInputId(0),
            input_name: "Sidechain".into(),
            source,
            source_name: "Source".into(),
            tap: SourceTap::AfterEffects,
        }],
    }
}

#[test]
fn restored_cycle_keeps_established_route_and_persists_the_silent_assignment() {
    let mut app = app();
    let mut a = ProjectTrack::new(TrackId::new(), "A".into(), 0);
    let mut b = ProjectTrack::new(TrackId::new(), "B".into(), 1);
    a.effects.push(effect(b.id));
    b.effects.push(effect(a.id));
    let declared = std::mem::take(&mut a.effects[0].external_inputs);
    Arc::make_mut(&mut app.state.project_tracks).tracks = vec![a, b];
    app.sync_sidechain_routing();
    let snapshot = app.project_for_offline_render();
    assert_eq!(
        snapshot.tracks[0].effects[0].inactive_sidechains,
        [ExternalInputId(0)]
    );
    assert!(snapshot.tracks[1].effects[0].inactive_sidechains.is_empty());
    assert!(app.project_from_state().tracks[0].effects[0].inactive_sidechains.is_empty(),
        "offline capability snapshot must not write temporary unavailability into the saved project");
    Arc::make_mut(&mut app.state.project_tracks).tracks[0].effects[0].external_inputs = declared;
    app.sync_sidechain_routing();
    let tracks = &app.state.project_tracks.tracks;
    assert_eq!(
        tracks[0].effects[0].inactive_sidechains,
        [ExternalInputId(0)]
    );
    assert!(tracks[1].effects[0].inactive_sidechains.is_empty());
    assert_eq!(tracks[0].effects[0].sidechains[0].source, tracks[1].id);
    RoutingGraph::prepare(app.state.devices.last_routing.as_ref().unwrap()).unwrap();
    let project = app.project_from_state();
    let reopened: vibez_project::Project =
        serde_json::from_slice(&serde_json::to_vec(&project).unwrap()).unwrap();
    assert_eq!(
        reopened.tracks[0].effects[0].inactive_sidechains,
        [ExternalInputId(0)]
    );
    assert_eq!(
        reopened.tracks[0].effects[0].sidechains,
        project.tracks[0].effects[0].sidechains
    );
    let inactive_snapshot = app.take_snapshot();
    let receiver = app.state.project_tracks.tracks[0].id;
    let effect_id = app.state.project_tracks.tracks[0].effects[0].id;
    let _ = app.update_and_refresh_clips(Message::Devices(
        crate::domains::devices::DevicesMsg::SetSidechainTap {
            track_id: receiver,
            effect_id,
            input_id: ExternalInputId(0),
            tap: SourceTap::BeforeEffects,
        },
    ));
    assert!(app.state.project_tracks.tracks[0].effects[0]
        .inactive_sidechains
        .is_empty());
    assert_eq!(app.state.project.history.undo.len(), 1);
    app.apply_snapshot(inactive_snapshot);
    app.sync_sidechain_routing();
    assert_eq!(
        app.state.project_tracks.tracks[0].effects[0].inactive_sidechains,
        [ExternalInputId(0)]
    );
    RoutingGraph::prepare(app.state.devices.last_routing.as_ref().unwrap()).unwrap();
}

#[test]
fn monitoring_target_changes_invalidate_timing_without_rebuilding_structural_routing() {
    let mut app = app();
    let mut first = ProjectTrack::new(TrackId::new(), "First".into(), 0);
    let mut second = ProjectTrack::new(TrackId::new(), "Second".into(), 1);
    for track in [&mut first, &mut second] {
        track.kind = vibez_core::midi::TrackKind::Midi;
        track.has_instrument = true;
    }
    let first_id = first.id;
    let second_id = second.id;
    let project = Arc::make_mut(&mut app.state.project_tracks);
    project.reduced_latency_monitoring = true;
    project.tracks = vec![first, second];
    app.state.perform.sync_instrument_target(Some(first_id));
    app.sync_sidechain_routing();
    assert_eq!(
        app.state
            .devices
            .last_timing
            .as_ref()
            .unwrap()
            .reduced_tracks,
        [first_id]
    );
    super::sidechain::MODEL_BUILDS.with(|count| count.set(0));
    app.state.perform.sync_instrument_target(Some(second_id));
    app.sync_sidechain_routing();
    assert_eq!(
        app.state
            .devices
            .last_timing
            .as_ref()
            .unwrap()
            .reduced_tracks,
        [second_id]
    );
    super::sidechain::MODEL_BUILDS.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn timing_reactivation_refreshes_choices_and_persists_restored_feedback_silence() {
    let mut app = app();
    let mut a = ProjectTrack::new(TrackId::new(), "A".into(), 0);
    let mut b = ProjectTrack::new(TrackId::new(), "B".into(), 1);
    a.effects.push(effect(b.id));
    b.effects.push(effect(a.id));
    a.effects[0].external_inputs.clear();
    let track_id = a.id;
    let effect_id = a.effects[0].id;
    Arc::make_mut(&mut app.state.project_tracks).tracks = vec![a, b];
    app.sync_sidechain_routing();
    assert!(!app
        .state
        .devices
        .sidechain_choices
        .contains_key(&(effect_id, ExternalInputId(0))));
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(producer);
    app.send_command(EngineCommand::AddPluginEffect {
        track_id,
        effect_id,
        effect: vibez_dsp::factory::create_effect(EffectType::Compressor, 44_100.0),
        position: None,
    });
    let EngineCommand::AddPluginEffect { effect, .. } = consumer.pop().unwrap() else {
        panic!("device creation")
    };
    app.reconfigure_device_timing(
        vibez_engine::engine::reconfiguration::DeviceReconfiguration::Effect {
            handoff_id: 1,
            reserved_effects: Vec::new(),
            track_id,
            position: 0,
            slot: vibez_engine::mixer::EffectSlot {
                id: effect_id,
                effect,
                bypass: false,
            },
        },
    );
    assert_eq!(
        app.state.project_tracks.tracks[0].effects[0].inactive_sidechains,
        [ExternalInputId(0)]
    );
    let channels = app.state.devices.last_routing.as_ref().unwrap();
    assert_eq!(
        app.state.devices.sidechain_choices,
        crate::domains::sidechain::input_source_choices(channels)
    );
    assert!(app
        .state
        .devices
        .sidechain_choices
        .contains_key(&(effect_id, ExternalInputId(0))));
    assert_eq!(
        app.project_from_state().tracks[0].effects[0].inactive_sidechains,
        [ExternalInputId(0)]
    );
}
