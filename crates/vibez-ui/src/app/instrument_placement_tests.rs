//! Instrument placement on Project Tracks, through the router.

use super::{test_support::app, *};
use crate::state::ProjectTrack;
use vibez_core::id::TrackId;
use vibez_plugin_host::{PluginCategory, PluginFormat, PluginId, PluginInfo};

#[test]
fn an_audio_track_refuses_an_instrument_plugin_and_says_why() {
    let mut app = app();
    let track_id = TrackId::new();
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(ProjectTrack::new(track_id, "Audio 1".into(), 0));
    let plugin_id = PluginId {
        format: PluginFormat::Clap,
        uid: "test.synth".into(),
    };
    app.state.plugin_settings.cache.push(PluginInfo {
        id: plugin_id.clone(),
        name: "Test Synth".into(),
        vendor: "Test".into(),
        category: PluginCategory::Instrument,
        format: PluginFormat::Clap,
        path: "/test-synth.clap".into(),
    });

    let _ = app.update(Message::AddPluginToTrack(track_id, plugin_id));

    assert_eq!(
        app.state.status_text,
        "Test Synth is an instrument; only MIDI tracks take instruments"
    );
    assert!(!app.state.find_track(track_id).unwrap().has_instrument);
}
