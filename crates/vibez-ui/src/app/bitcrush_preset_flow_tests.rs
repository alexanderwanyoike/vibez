//! Verifies Bitcrush sound files retain exact stored controls through the shared App flow.

use super::*;
use crate::domains::devices::DevicesMsg;
use crate::state::ProjectTrack;
use vibez_core::effect::EffectType;

#[test]
fn bitcrush_fresh_insert_selects_crunchy_twelve_without_rewriting_legacy_defaults() {
    let mut app = super::super::test_support::app();
    let channel = ProjectTrack::new(TrackId::new(), "New Bitcrush".into(), 3);
    let track = channel.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(channel);
    let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
        track,
        EffectType::Bitcrush,
    )));
    let effect = &app.state.find_track(track).unwrap().effects[0];
    assert_eq!(effect.descriptors[0].default, 10.0);
    assert_eq!(effect.descriptors[1].default, 1.0);
    assert_eq!(effect.descriptors[2].default, 1.0);
    let presets = crate::domains::device_presets::presentation(effect, None);
    assert_eq!(
        presets.selected.as_ref().map(DevicePreset::name),
        Some("Crunchy 12")
    );
    assert!(!presets.modified);
}

#[path = "bitcrush_acceptance_flow_tests.rs"]
mod acceptance;
