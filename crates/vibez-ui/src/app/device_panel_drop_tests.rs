//! Audio dropped from the Browser on a MIDI track's device panel lands in a
//! Sampler, through the router and the real import pipeline.

use std::path::{Path, PathBuf};

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::TrackId;
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::track::MediaSourceRef;

use super::test_support::{app, run_until_idle};
use super::*;
use crate::domains::devices::DevicesMsg;
use crate::domains::project::ProjectMsg;
use crate::state::{AuditionMode, ProjectTrack};

fn kick(directory: &Path) -> PathBuf {
    let path = directory.join("kick.wav");
    let audio = DecodedAudio {
        channels: vec![vec![0.5; 4_410]],
        sample_rate: 44_100,
    };
    vibez_audio_io::file_io::write_wav_file(&path, &audio).unwrap();
    path
}

fn app_with_midi_track() -> (App, TrackId) {
    let mut app = app();
    let track = ProjectTrack::new_instrument(TrackId::new(), "MIDI 1".into(), TrackKind::Midi, 0);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    app.state.browser.audition_mode = AuditionMode::Raw;
    (app, track_id)
}

async fn drop_on_panel(app: &mut App, track_id: TrackId, path: PathBuf) {
    app.state.browser.drag_source = Some(MediaSourceRef::LocalFile { path });
    app.state.browser.drag_label = Some("kick.wav".into());
    run_until_idle(app, Message::DropSampleOnDevicePanel { track_id }).await;
}

fn assert_holds_kick(app: &App, track_id: TrackId) {
    let track = app.state.find_track(track_id).unwrap();
    assert!(crate::domains::sampler::is_native_sampler(track));
    assert_eq!(track.sample_name.as_deref(), Some("kick.wav"));
    assert!(track.sample_audio.is_some());
    assert!(track.sample_source.is_some());
}

#[tokio::test]
async fn a_drop_on_an_empty_midi_tracks_panel_adds_a_loaded_sampler_in_one_undo_step() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_midi_track();

    drop_on_panel(&mut app, track_id, kick(directory.path())).await;

    assert_holds_kick(&app, track_id);
    assert_eq!(app.state.status_text, "Added a Sampler holding 'kick.wav'");
    assert!(app.state.browser.drag_source.is_none());
    let _ = app.update(Message::Project(ProjectMsg::Undo));
    let track = app.state.find_track(track_id).unwrap();
    assert!(!track.has_instrument);
    assert_eq!(track.instrument_kind, None);
    assert!(app.state.project.history.undo.is_empty());
}

#[tokio::test]
async fn a_drop_on_the_panel_replaces_a_built_in_instrument_and_one_undo_restores_it() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_midi_track();
    let _ = app.update(Message::Devices(DevicesMsg::SetTrackInstrument(
        track_id,
        InstrumentKind::FmSynth,
    )));
    let _ = app.update(Message::Devices(DevicesMsg::SetInstrumentParam(
        track_id, 0, 0.25,
    )));
    let fm_params = app
        .state
        .find_track(track_id)
        .unwrap()
        .instrument_params
        .clone();
    app.state.project.history = Default::default();

    drop_on_panel(&mut app, track_id, kick(directory.path())).await;

    assert_holds_kick(&app, track_id);
    assert_eq!(
        app.state.status_text,
        "Replaced FM Synth with a Sampler holding 'kick.wav'"
    );
    let _ = app.update(Message::Project(ProjectMsg::Undo));
    let track = app.state.find_track(track_id).unwrap();
    assert_eq!(track.instrument_kind, Some(InstrumentKind::FmSynth));
    assert_eq!(track.instrument_params, fm_params);
    assert!(track.sample_audio.is_none());
    assert!(app.state.project.history.undo.is_empty());
}

#[tokio::test]
async fn a_drop_on_an_audio_tracks_panel_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app();
    let track = ProjectTrack::new(TrackId::new(), "Audio 1".into(), 0);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);

    drop_on_panel(&mut app, track_id, kick(directory.path())).await;

    assert!(!app.state.find_track(track_id).unwrap().has_instrument);
    assert!(app.state.project.history.undo.is_empty());
}

#[tokio::test]
async fn the_implied_sampler_and_its_sample_survive_save_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let project_path = directory.path().join("implied-sampler.vzp");
    let (mut app, track_id) = app_with_midi_track();
    drop_on_panel(&mut app, track_id, kick(directory.path())).await;
    let heard = app
        .state
        .find_track(track_id)
        .unwrap()
        .sample_audio
        .clone()
        .unwrap();

    save_project_async(project_path.clone(), None, app.project_for_save())
        .await
        .unwrap();
    let loaded = load_project_async(project_path, None, app.state.transport.sample_rate)
        .await
        .unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let mut reopened = test_support::app();
    reopened.rebuild_from_loaded_project(loaded);

    assert_holds_kick(&reopened, track_id);
    let reopened_audio = reopened
        .state
        .find_track(track_id)
        .unwrap()
        .sample_audio
        .clone()
        .unwrap();
    assert_eq!(reopened_audio.num_frames(), heard.num_frames());
    let worst = heard.channels[0]
        .iter()
        .zip(&reopened_audio.channels[0])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 1e-6, "reopened sample differs by {worst}");
}
