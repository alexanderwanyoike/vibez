//! Verifies Gate sound files preserve the destination chain and undo as one edit.

use super::*;
use crate::domains::{device_presets, devices::DevicesMsg};
use crate::state::ProjectTrack;
use vibez_core::automation::{AutomationLane, AutomationTarget};
use vibez_core::effect::EffectType;
use vibez_core::routing::{ExternalInputId, SidechainAssignment, SourceTap};

#[tokio::test]
async fn gate_factory_files_preserve_identity_routing_and_automation_with_one_undo() {
    let presets = device_presets::factory_presets(EffectType::Gate);
    assert!(presets.len() >= 6, "Gate needs useful factory presets");
    let mut app = super::super::test_support::app();
    let source = ProjectTrack::new(TrackId::new(), "Detector".into(), 6);
    let source_id = source.id;
    let target = ProjectTrack::new(TrackId::new(), "Gated".into(), 3);
    let track = target.id;
    Arc::make_mut(&mut app.state.project_tracks).tracks.extend([source, target]);
    for kind in [EffectType::Gain, EffectType::Gate, EffectType::Gain] {
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(track, kind)));
    }
    let gate = app.state.find_track_mut(track).unwrap().effects.iter_mut()
        .find(|effect| effect.effect_type == EffectType::Gate).unwrap();
    let effect = gate.id;
    gate.params[0] = -17.3;
    gate.bypass = true;
    gate.sidechains.push(SidechainAssignment {
        input_id: ExternalInputId(0), input_name: "Sidechain".into(), source: source_id,
        source_name: "Detector".into(), tap: SourceTap::AfterFader,
    });
    app.state.arrange_content_mut(track).automation.push(AutomationLane::new(
        AutomationTarget::EffectParam { effect_id: effect, param_index: 0 }
    ));
    let folder = tempfile::tempdir().unwrap();
    for preset in presets {
        app.state.project.history = Default::default();
        let before = app.state.find_track(track).unwrap().clone();
        let automation = app.state.arrange_content(track).unwrap().automation.clone();
        let path = folder.path().join("Gate.vzpreset");
        let saved = tasks::save_device_preset_at(path.clone(), preset).await.unwrap();
        let loaded = vibez_project::device_preset::load_device_preset(&path).unwrap();
        assert_eq!(saved, loaded);
        let request = app.begin_device_preset_load(track, effect).expect("native Gate load");
        let _ = app.finish_device_preset_load(request, Ok(loaded.clone()));
        let after = app.state.find_track(track).unwrap();
        assert_eq!(after.effects.iter().map(|effect| effect.id).collect::<Vec<_>>(),
            before.effects.iter().map(|effect| effect.id).collect::<Vec<_>>());
        let current = after.effects.iter().find(|candidate| candidate.id == effect).unwrap();
        assert_eq!(current.sidechains, before.effects[1].sidechains);
        assert_eq!(current.bypass, before.effects[1].bypass);
        assert_eq!(app.state.arrange_content(track).unwrap().automation, automation);
        assert_eq!(device_presets::capture(current, loaded.name()).unwrap(), loaded);
        let request = app.begin_device_preset_load(track, effect).unwrap();
        let _ = app.finish_device_preset_load(request, Ok(loaded));
        let snapshot = app.state.project.history.pop_undo().expect("one preset edit");
        assert!(!app.state.project.history.can_undo(), "same load added history");
        app.apply_snapshot(snapshot);
        assert_eq!(app.state.find_track(track).unwrap().effects[1].params, before.effects[1].params);
    }
}
