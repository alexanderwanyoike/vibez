use super::*;
use crate::state::UiEffect;
use vibez_core::routing::RoutingGraph;
use vibez_core::{
    effect::EffectType,
    routing::{ExternalInputDescriptor, ExternalInputId},
};

fn effect(id: EffectId) -> UiEffect {
    let device = vibez_dsp::factory::create_effect(EffectType::Compressor, 48000.0);
    UiEffect {
        latency_samples: Some(0),
        id,
        effect_type: EffectType::Compressor,
        bypass: false,
        params: device
            .param_descriptors()
            .iter()
            .map(|parameter| parameter.default)
            .collect(),
        descriptors: device.param_descriptors(),
        plugin_name: None,
        has_plugin_gui: false,
        plugin_ref: None,
        external_inputs: device.external_inputs().to_vec(),
        inactive_sidechains: Default::default(),
        sidechains: Vec::new(),
    }
}
fn setup() -> (Vec<ProjectTrack>, ProjectTrack, Vec<ProjectTrack>, EffectId) {
    let id = EffectId::new();
    let mut receiver = ProjectTrack::new(TrackId::new(), "Bass".into(), 0);
    receiver.effects.push(effect(id));
    (
        vec![
            receiver,
            ProjectTrack::new(TrackId::new(), "Kick".into(), 1),
        ],
        ProjectTrack::new(TrackId::MASTER, "Master".into(), 0),
        vec![],
        id,
    )
}

#[test]
fn each_effect_and_declared_input_keeps_an_independent_assignment() {
    let (mut tracks, mut master, mut buses, first) = setup();
    let second = EffectId::new();
    let receiver = tracks[0].id;
    let source = tracks[1].id;
    tracks[0].effects.push(effect(second));
    tracks[0].effects[0]
        .external_inputs
        .push(ExternalInputDescriptor {
            id: ExternalInputId(7),
            name: "Transient detector".into(),
            channels: 1,
        });
    assert!(edit_source(
        &mut tracks,
        &mut master,
        &mut buses,
        receiver,
        first,
        ExternalInputId(0),
        Some(source)
    ));
    assert!(edit_source(
        &mut tracks,
        &mut master,
        &mut buses,
        receiver,
        first,
        ExternalInputId(7),
        Some(receiver)
    ));
    assert!(edit_source(
        &mut tracks,
        &mut master,
        &mut buses,
        receiver,
        second,
        ExternalInputId(0),
        Some(source)
    ));
    assert!(edit_tap(
        &mut tracks,
        &mut master,
        &mut buses,
        receiver,
        second,
        ExternalInputId(0),
        SourceTap::AfterFader
    ));
    assert_eq!(
        tracks[0].effects[0].sidechains[0].tap,
        SourceTap::AfterEffects
    );
    assert_eq!(
        tracks[0].effects[0].sidechains[1].tap,
        SourceTap::BeforeEffects
    );
    assert_eq!(
        tracks[0].effects[1].sidechains[0].tap,
        SourceTap::AfterFader
    );
    assert!(!edit_source(
        &mut tracks,
        &mut master,
        &mut buses,
        receiver,
        first,
        ExternalInputId(0),
        Some(source)
    ));
}

#[test]
fn selectors_hide_actual_feedback_taps_including_existing_send_paths() {
    let (tracks, master, mut buses, effect_id) = setup();
    let receiver = tracks[0].id;
    let source = tracks[1].id;
    let bus = ProjectTrack::new(TrackId::new(), "Return".into(), 2);
    let bus_id = bus.id;
    buses.push(bus);
    let graph = routing_channels(&tracks, &master, &buses);
    assert_eq!(
        valid_taps(&graph, receiver, effect_id, ExternalInputId(0), receiver),
        vec![SourceTap::BeforeEffects]
    );
    assert_eq!(
        valid_taps(&graph, receiver, effect_id, ExternalInputId(0), source).len(),
        3
    );
    assert!(valid_taps(
        &graph,
        receiver,
        effect_id,
        ExternalInputId(0),
        TrackId::MASTER
    )
    .is_empty());
    let mut tracks = tracks;
    tracks[0].sends.push((bus_id, 0.5));
    let graph = routing_channels(&tracks, &master, &buses);
    assert!(valid_taps(&graph, receiver, effect_id, ExternalInputId(0), bus_id).is_empty());
}

#[test]
fn deletion_and_same_name_replacement_preserve_missing_identity_until_disconnect() {
    let (mut tracks, mut master, mut buses, effect_id) = setup();
    let receiver = tracks[0].id;
    let source = tracks[1].id;
    assert!(edit_source(
        &mut tracks,
        &mut master,
        &mut buses,
        receiver,
        effect_id,
        ExternalInputId(0),
        Some(source)
    ));
    let removed = tracks.pop().unwrap();
    tracks.push(ProjectTrack::new(TrackId::new(), "Kick".into(), 1));
    let route = &tracks[0].effects[0].sidechains[0];
    assert_eq!(route.source, source);
    assert_eq!(route.source_name, "Kick");
    assert_ne!(route.source, tracks[1].id);
    let graph = RoutingGraph::prepare(&routing_channels(&tracks, &master, &buses)).unwrap();
    assert!(!graph
        .edges
        .iter()
        .any(|edge| matches!(edge.kind, vibez_core::routing::EdgeKind::External(_))));
    tracks.push(removed);
    let graph = RoutingGraph::prepare(&routing_channels(&tracks, &master, &buses)).unwrap();
    assert!(graph
        .edges
        .iter()
        .any(|edge| matches!(edge.kind, vibez_core::routing::EdgeKind::External(_))));
    assert!(edit_source(
        &mut tracks,
        &mut master,
        &mut buses,
        receiver,
        effect_id,
        ExternalInputId(0),
        None
    ));
    assert!(tracks[0].effects[0].sidechains.is_empty());
}

#[test]
fn unavailable_input_assignments_never_move_to_reused_port_identity() {
    let (mut tracks, mut master, mut buses, effect_id) = setup();
    let receiver = tracks[0].id;
    let source = tracks[1].id;
    assert!(edit_source(
        &mut tracks,
        &mut master,
        &mut buses,
        receiver,
        effect_id,
        ExternalInputId(0),
        Some(source)
    ));
    tracks[0].effects[0].external_inputs[0].name = "Different bus".into();
    let graph = RoutingGraph::prepare(&routing_channels(&tracks, &master, &buses)).unwrap();
    assert!(!graph
        .edges
        .iter()
        .any(|edge| matches!(edge.kind, vibez_core::routing::EdgeKind::External(_))));
    assert_eq!(tracks[0].effects[0].sidechains[0].input_name, "Sidechain");
    tracks[0].effects[0].external_inputs.clear();
    assert!(edit_source(
        &mut tracks,
        &mut master,
        &mut buses,
        receiver,
        effect_id,
        ExternalInputId(0),
        None
    ));
    assert!(tracks[0].effects[0].sidechains.is_empty());
}

#[test]
fn unsupported_layouts_cannot_receive_new_routes() {
    let (mut tracks, mut master, mut buses, effect_id) = setup();
    let receiver = tracks[0].id;
    let source = tracks[1].id;
    tracks[0].effects[0].external_inputs[0].channels = 6;
    assert!(!edit_source(
        &mut tracks,
        &mut master,
        &mut buses,
        receiver,
        effect_id,
        ExternalInputId(0),
        Some(source)
    ));
    assert!(tracks[0].effects[0].sidechains.is_empty());
}

#[test]
fn potential_automated_send_prevents_feedback_before_its_first_nonzero_point() {
    let (mut tracks, mut master, mut buses, effect_id) = setup();
    let receiver = tracks[0].id;
    let bus_id = TrackId::new();
    buses.push(ProjectTrack::new(bus_id, "Return".into(), 2));
    let mut model = routing_channels(&tracks, &master, &buses);
    model
        .iter_mut()
        .find(|channel| channel.id == receiver)
        .unwrap()
        .sends
        .push(bus_id);
    assert!(valid_taps(&model, receiver, effect_id, ExternalInputId(0), bus_id).is_empty());
    assert!(!edit_source_with_model(
        &mut tracks,
        &mut master,
        &mut buses,
        (receiver, effect_id, ExternalInputId(0)),
        Some(bus_id),
        &model
    ));
    assert!(tracks[0].effects[0].sidechains.is_empty());
}

#[test]
fn cached_source_taps_match_command_validation_for_each_supported_input() {
    let (mut tracks, master, mut buses, effect_id) = setup();
    let source = tracks[1].id;
    let mut bus = ProjectTrack::new(TrackId::new(), "Bus".into(), 2);
    let bus_effect = EffectId::new();
    bus.effects.push(effect(bus_effect));
    tracks[1].sends.push((bus.id, 0.5));
    buses.push(bus);
    tracks[0].effects[0].sidechains.push(SidechainAssignment {
        input_id: ExternalInputId(0),
        input_name: "Sidechain".into(),
        source,
        source_name: "Kick".into(),
        tap: SourceTap::AfterEffects,
    });
    let graph = routing_channels(&tracks, &master, &buses);
    let cached = input_source_choices(&graph);
    for channel in &graph {
        for effect in &channel.effects {
            for input in effect.inputs.iter().filter(|input| input.supported()) {
                let choices = &cached[&(effect.id, input.id)];
                for source in &graph {
                    let expected = valid_taps(&graph, channel.id, effect.id, input.id, source.id);
                    let actual = choices
                        .iter()
                        .find(|choice| choice.source == source.id)
                        .map_or(&[][..], |choice| choice.taps.as_slice());
                    assert_eq!(actual, expected);
                }
            }
        }
    }
    assert!(cached.contains_key(&(effect_id, ExternalInputId(0))));
}
