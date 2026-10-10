//! Full compensation consumes reports and automation independently of live selection.

use super::*;
use crate::state::{AppState, ProjectTrack, UiEffect};
use std::sync::Arc;
use vibez_core::{automation::AutomationTarget, effect::EffectType, id::EffectId};

fn effect(samples: Option<u32>, bypass: bool) -> UiEffect {
    UiEffect {
        reconfiguration_failed: false,
        latency_samples: samples,
        id: EffectId::new(),
        effect_type: EffectType::Gain,
        bypass,
        params: vec![],
        descriptors: &[],
        plugin_name: None,
        has_plugin_gui: false,
        plugin_ref: None,
        sidechains: vec![],
        external_inputs: vec![],
        inactive_sidechains: vec![],
    }
}

#[test]
fn cached_source_bus_master_and_bypass_reports_define_full_timing() {
    let mut state = AppState::default();
    let mut source = ProjectTrack::new(TrackId::new(), "Source".into(), 0);
    source.instrument_latency_samples = Some(137);
    source.effects = vec![effect(Some(521), true), effect(None, false)];
    let source_id = source.id;
    let effect_id = source.effects[0].id;
    let mut bus = ProjectTrack::new(TrackId::new(), "Return".into(), 1);
    bus.effects.push(effect(Some(17), false));
    let project = Arc::make_mut(&mut state.project_tracks);
    project.tracks.push(source);
    project.buses.push(bus);
    project.master.effects.push(effect(Some(19), false));
    let timing = signature_for_state(&state);
    assert!(timing.reports.contains(&(
        RoutingNode {
            channel: source_id,
            stage: NodeStage::Source
        },
        137
    )));
    assert!(timing.reports.contains(&(
        RoutingNode {
            channel: source_id,
            stage: NodeStage::Effect(effect_id)
        },
        521
    )));
    assert_eq!(
        timing
            .reports
            .iter()
            .filter(|(_, report)| *report > 0)
            .count(),
        4
    );
    assert!(timing.reduced_tracks.is_empty());
    state.arrangement.selected_track = Some(source_id);
    state.perform.sync_instrument_target(Some(source_id));
    state.audio_recording.armed_track = Some(source_id);
    state.audio_recording.monitor_track = Some(source_id);
    assert_eq!(signature_for_state(&state), timing);
}

#[test]
fn automation_control_metadata_is_deduplicated_and_retained_only_for_the_same_alignment() {
    let mut state = AppState::default();
    let track = TrackId::new();
    let content = Arc::make_mut(&mut state.arrangement.timeline)
        .by_track
        .entry(track)
        .or_default();
    content
        .automation
        .push(vibez_core::automation::AutomationLane::new(
            AutomationTarget::TrackGain,
        ));
    let mut first = signature_for_state(&state);
    first.controls.push((track, AutomationTarget::TrackPan));
    state.devices.last_timing = Some(first);
    let signature = signature_for_state(&state);
    assert_eq!(signature.controls.len(), 2);
    assert!(signature
        .controls
        .contains(&(track, AutomationTarget::TrackPan)));
    state.transport.sample_rate = 96000;
    let changed = signature_for_state(&state);
    assert_eq!(changed.controls, vec![(track, AutomationTarget::TrackGain)]);
    assert!(changed.reduced_tracks.is_empty());
}
