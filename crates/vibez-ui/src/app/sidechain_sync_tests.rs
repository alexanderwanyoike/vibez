//! Routing invalidation during idle messages, metering and capability restoration.
use super::{test_support::app, *};
use crate::state::{ProjectTrack, UiEffect};
use vibez_core::{
    effect::EffectType,
    id::{EffectId, TrackId},
    routing::{ExternalInputId, RoutingGraph, SidechainAssignment, SourceTap},
};

#[test]
fn dormant_send_keeps_its_graph_connection_without_republishing_for_level_edits() {
    let mut app = app();
    let mut track = ProjectTrack::new(TrackId::new(), "Kick".into(), 0);
    let bus = ProjectTrack::new(TrackId::new(), "Drums".into(), 1);
    let track_id = track.id;
    let bus_id = bus.id;
    track.sends.push((bus_id, 0.0));
    let project = Arc::make_mut(&mut app.state.project_tracks);
    project.tracks.push(track);
    project.buses.push(bus);
    let (commands, mut received) = rtrb::RingBuffer::new(16);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    app.sync_sidechain_routing();
    assert!(matches!(
        received.pop().unwrap(),
        EngineCommand::SetRouting(_)
    ));
    let routing = app.state.devices.last_routing.clone().unwrap();
    let graph = RoutingGraph::prepare(&routing).unwrap();
    let from = graph
        .index(track_id, vibez_core::routing::NodeStage::AfterFader)
        .unwrap();
    let to = graph
        .index(bus_id, vibez_core::routing::NodeStage::Sum)
        .unwrap();
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.from == from && edge.to == to));
    for amount in [0.5, 0.0, 1.0, 0.0] {
        let _ = app.update_and_refresh_clips(Message::set_send(track_id, bus_id, amount));
        assert_eq!(app.state.devices.last_routing.as_ref(), Some(&routing));
        assert_eq!(app.state.project_tracks.tracks[0].sends, [(bus_id, amount)]);
        let mut level_changes = 0;
        while let Ok(command) = received.pop() {
            assert!(!matches!(command, EngineCommand::SetRouting(_)));
            if matches!(command, EngineCommand::SetSend { .. }) {
                level_changes += 1;
            }
        }
        assert_eq!(level_changes, 1);
    }
}

#[test]
fn creating_a_zero_level_send_cannot_hide_feedback_from_validation() {
    let mut app = app();
    let bus = ProjectTrack::new(TrackId::new(), "Detector".into(), 1);
    let mut track = ProjectTrack::new(TrackId::new(), "Bass".into(), 0);
    track.effects.push(effect(bus.id));
    let track_id = track.id;
    let bus_id = bus.id;
    let project = Arc::make_mut(&mut app.state.project_tracks);
    project.tracks.push(track);
    project.buses.push(bus);
    app.sync_sidechain_routing();
    let routing = app.state.devices.last_routing.clone();
    let _ = app.update_and_refresh_clips(Message::set_send(track_id, bus_id, 0.0));
    assert!(app.state.project_tracks.tracks[0].sends.is_empty());
    assert_eq!(app.state.devices.last_routing, routing);
    assert!(app.state.status_text.contains("Routing change rejected"));
}

#[test]
fn configured_send_automation_reserves_topology_even_without_audible_points() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    let mut app = app();
    let track = ProjectTrack::new(TrackId::new(), "Kick".into(), 0);
    let bus = ProjectTrack::new(TrackId::new(), "Drums".into(), 1);
    let track_id = track.id;
    let bus_id = bus.id;
    let project = Arc::make_mut(&mut app.state.project_tracks);
    project.tracks.push(track);
    project.buses.push(bus);
    Arc::make_mut(&mut app.state.arrangement.timeline)
        .ensure(track_id)
        .automation
        .push(AutomationLane::new(AutomationTarget::Send { bus_id }));
    app.sync_sidechain_routing();
    let routing = app.state.devices.last_routing.clone().unwrap();
    let graph = RoutingGraph::prepare(&routing).unwrap();
    let from = graph
        .index(track_id, vibez_core::routing::NodeStage::AfterFader)
        .unwrap();
    let to = graph
        .index(bus_id, vibez_core::routing::NodeStage::Sum)
        .unwrap();
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.from == from && edge.to == to));
    for value in [0.0, 0.5, 0.0] {
        Arc::make_mut(&mut app.state.arrangement.timeline)
            .ensure(track_id)
            .automation[0]
            .insert_point(AutomationPoint {
                beat: 0.0,
                value,
                curve: 0.0,
            });
        app.sync_sidechain_routing();
        assert_eq!(app.state.devices.last_routing.as_ref(), Some(&routing));
    }
}

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
        reconfiguration_failed: false,
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
