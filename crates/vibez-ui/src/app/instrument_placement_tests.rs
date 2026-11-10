//! Which Project Tracks take an instrument, as the running app enforces it.

use super::{test_support::app, *};
use crate::state::ProjectTrack;
use vibez_core::id::TrackId;
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::track::TrackInfo;
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

#[tokio::test]
async fn a_saved_instrument_on_an_audio_track_still_loads_and_can_be_removed() {
    let directory = tempfile::Builder::new()
        .prefix(".instrument-placement-test-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    for extension in ["vibez", "vzp"] {
        let mut project = vibez_project::Project::default();
        let mut track = TrackInfo::new("Audio 1");
        track.kind = TrackKind::Audio;
        track.instrument = Some(InstrumentKind::Sampler);
        let track_id = track.id;
        project.tracks.push(track);
        let path = directory.path().join(format!("legacy.{extension}"));
        save_project_async(path.clone(), None, project)
            .await
            .unwrap();
        let loaded = load_project_async(path, None, vibez_core::constants::DEFAULT_SAMPLE_RATE)
            .await
            .unwrap();
        let mut app = app();
        app.rebuild_from_loaded_project(loaded);

        let track = app.state.find_track(track_id).unwrap();
        assert_eq!(track.kind, TrackKind::Audio);
        assert_eq!(track.instrument_kind, Some(InstrumentKind::Sampler));
        assert!(track.has_instrument);

        let _ = app.update(Message::remove_track_instrument(track_id));

        assert!(!app.state.find_track(track_id).unwrap().has_instrument);
    }
}
