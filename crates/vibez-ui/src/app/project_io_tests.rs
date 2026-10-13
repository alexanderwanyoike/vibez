use super::*;
use vibez_core::midi::InstrumentKind;

fn surge_device() -> vibez_core::effect::PluginDeviceInfo {
    vibez_core::effect::PluginDeviceInfo {
        format: "clap".to_string(),
        uid: "org.surge-synth-team.surge-xt".to_string(),
        path: "/usr/lib/clap/Surge XT.clap".into(),
        name: "Surge XT".to_string(),
        state_b64: Some("plugin-state".to_string()),
    }
}

#[test]
fn legacy_dual_instrument_record_replays_the_native_sampler() {
    let mut track = TrackInfo::new("MIDI 1");
    track.instrument = Some(InstrumentKind::Sampler);
    track.native_instrument = Some(InstrumentStateInfo::Sampler {
        params: Vec::new(),
        source: None,
    });
    track.plugin_instrument = Some(surge_device());

    assert!(plugin_instrument_for_replay(&track).is_none());
}

#[test]
fn plugin_only_record_still_replays_its_plugin() {
    let mut track = TrackInfo::new("Bass");
    track.plugin_instrument = Some(surge_device());

    assert_eq!(
        plugin_instrument_for_replay(&track)
            .as_ref()
            .map(|plugin| plugin.name.as_str()),
        Some("Surge XT")
    );
}

#[test]
fn legacy_one_bank_drum_racks_expand_without_moving_saved_pads() {
    let first = crate::state::UiDrumPad {
        name: Some("Kick".into()),
        ..Default::default()
    };
    let mut saved = vec![first.to_state()];
    saved.extend((1..16).map(|_| crate::state::UiDrumPad::default().to_state()));

    let expanded = expand_drum_rack_pads(&saved);

    assert_eq!(expanded.len(), vibez_core::track::DRUM_RACK_PAD_COUNT);
    assert_eq!(expanded[0].name.as_deref(), Some("Kick"));
    assert!(expanded[16..].iter().all(|pad| pad.source.is_none()));
}

#[test]
fn saving_a_multi_bank_rack_keeps_used_banks_without_serializing_empty_tail_banks() {
    let mut pads = crate::state::default_drum_rack_pads();
    pads[19].name = Some("Slice 20".into());

    let saved = drum_rack_pads_for_save(&pads);

    assert_eq!(saved.len(), 20);
    assert_eq!(saved[19].name.as_deref(), Some("Slice 20"));
}
