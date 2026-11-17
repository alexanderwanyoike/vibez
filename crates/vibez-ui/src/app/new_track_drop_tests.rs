//! A file dropped on Perform's empty space makes the track it needs: a new
//! column's Clip slot in the clip launcher, or a new lane in a Section.

use std::path::{Path, PathBuf};

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::TrackId;
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::track::MediaSourceRef;

use super::test_support::{app, run_until_idle};
use super::*;
use crate::domains::browser::BrowserMsg;
use crate::domains::drop_ghost::{GhostPlace, NewTrack};
use crate::state::{AuditionMode, ProjectTrack};

/// A one-bar type 0 drum phrase at 96 PPQ: kick on 1, snare on 2.
fn drum_phrase(directory: &Path) -> PathBuf {
    let bytes: &[u8] = &[
        b'M', b'T', b'h', b'd', 0, 0, 0, 6, 0, 0, 0, 1, 0, 96, //
        b'M', b'T', b'r', b'k', 0, 0, 0, 20, //
        0, 0x99, 36, 110, 24, 0x89, 36, 0, //
        72, 0x99, 38, 110, 24, 0x89, 38, 0, //
        0x82, 8, 0xff, 0x2f, 0x00,
    ];
    let path = directory.join("Rebeater P3.mid");
    std::fs::write(&path, bytes).unwrap();
    path
}

fn kick(directory: &Path) -> PathBuf {
    let path = directory.join("kick.wav");
    let audio = DecodedAudio {
        channels: vec![vec![0.5; 4_410]],
        sample_rate: 44_100,
    };
    vibez_audio_io::file_io::write_wav_file(&path, &audio).unwrap();
    path
}

fn browser(message: BrowserMsg) -> Message {
    Message::Browser(message)
}

/// A clip launcher holding one audio track, which is not where drops on
/// empty space land.
fn launcher() -> (App, TrackId) {
    let mut app = app();
    app.state.browser.audition_mode = AuditionMode::Raw;
    app.state.view.workspace = crate::state::Workspace::Perform;
    app.state.perform.layout = vibez_project::PerformLayout::Clips;
    let existing = ProjectTrack::new(TrackId::new(), "Audio 1".into(), 0);
    let id = existing.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(existing);
    app.state
        .perform
        .sync_project_tracks(&app.state.project_tracks.tracks);
    (app, id)
}

fn browser_drag(app: &mut App, path: PathBuf) {
    app.state.browser.drag_label = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    app.state.browser.drag_source = Some(MediaSourceRef::LocalFile { path });
}

fn the_new_track(app: &App, existing: TrackId) -> &ProjectTrack {
    let added: Vec<_> = app
        .state
        .project_tracks
        .tracks
        .iter()
        .filter(|track| track.id != existing)
        .collect();
    let [track] = added[..] else {
        panic!("one new track, found {}", added.len());
    };
    track
}

#[tokio::test]
async fn drums_released_beside_the_last_launcher_column_get_a_drum_rack_track_and_that_slot() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, existing) = launcher();
    browser_drag(&mut app, drum_phrase(directory.path()));
    let _ = app.update(browser(BrowserMsg::DragHoverNewTrackSlot { row: 2 }));

    run_until_idle(&mut app, Message::ImportLauncherClipOnNewTrack { row: 2 }).await;

    let track = the_new_track(&app, existing);
    assert!(track.kind.is_midi());
    assert_eq!(track.instrument_kind, Some(InstrumentKind::DrumRack));
    let clip = app.state.perform.clips.at(track.id, 2).expect("slot clip");
    assert_eq!(clip.name(), "Rebeater P3");
    assert!(app.state.browser.drag_source.is_none());
}

#[tokio::test]
async fn audio_from_another_app_beside_the_last_launcher_column_gets_an_audio_track() {
    let directory = tempfile::tempdir().unwrap();
    let path = kick(directory.path());
    let (mut app, existing) = launcher();
    let _ = app.update(browser(BrowserMsg::DesktopFileHovered(path.clone())));
    let _ = app.update(browser(BrowserMsg::DragHoverNewTrackSlot { row: 1 }));

    run_until_idle(&mut app, browser(BrowserMsg::DesktopFileDropped(path))).await;

    let track = the_new_track(&app, existing);
    assert_eq!(track.kind, TrackKind::Audio);
    assert!(app.state.perform.clips.at(track.id, 1).is_some());
}

#[tokio::test]
async fn the_ghost_waits_in_the_new_columns_slot_under_the_pointer() {
    let directory = tempfile::tempdir().unwrap();
    let path = drum_phrase(directory.path());
    let (mut app, _) = launcher();
    run_until_idle(&mut app, browser(BrowserMsg::DesktopFileHovered(path))).await;

    let _ = app.update(browser(BrowserMsg::DragHoverNewTrackSlot { row: 3 }));

    let ghost = app
        .state
        .browser
        .drop_ghost(app.state.transport.bpm)
        .unwrap();
    assert_eq!(ghost.place, GhostPlace::NewTrackSlot { row: 3 });
    assert_eq!(ghost.media.new_track(), NewTrack::Midi { drum_rack: true });
    assert_eq!(
        app.state.browser.drop_ghost_slot().unwrap().name(),
        "Rebeater P3"
    );
}
