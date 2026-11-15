//! Files dragged in from other apps land through the router like Browser
//! drags, whichever comes first of the drop and the pointer's target.

use std::path::{Path, PathBuf};

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::TrackId;
use vibez_core::midi::TrackKind;

use super::test_support::{app, run_until_idle};
use super::*;
use crate::domains::browser::BrowserMsg;
use crate::domains::view::ViewMsg;
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

fn app_with_midi_track() -> (App, TrackId) {
    let mut app = app();
    let track = ProjectTrack::new_instrument(TrackId::new(), "MIDI 1".into(), TrackKind::Midi, 0);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    (app, track_id)
}

fn browser(message: BrowserMsg) -> Message {
    Message::Browser(message)
}

fn note_clip_starts(app: &App, track_id: TrackId) -> Vec<f64> {
    app.state
        .arrange_content(track_id)
        .map(|content| {
            content
                .note_clips
                .iter()
                .map(|clip| clip.position_beats)
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn a_midi_file_from_another_app_lands_on_the_lane_under_the_pointer() {
    let directory = tempfile::tempdir().unwrap();
    let path = drum_phrase(directory.path());
    let (mut app, track_id) = app_with_midi_track();
    let _ = app.update(browser(BrowserMsg::DesktopFileHovered(path.clone())));
    let _ = app.update(browser(BrowserMsg::DragHoverTrack {
        track_id,
        beat: 8.0,
        compatible: true,
    }));

    run_until_idle(&mut app, browser(BrowserMsg::DesktopFileDropped(path))).await;

    assert_eq!(note_clip_starts(&app, track_id), vec![8.0]);
    assert!(app.state.browser.drag_source.is_none());
    assert!(!app.state.browser.drags_from_desktop());
}

#[tokio::test]
async fn a_file_dropped_before_the_pointer_reports_a_target_lands_on_the_next_one() {
    let directory = tempfile::tempdir().unwrap();
    let path = drum_phrase(directory.path());
    let (mut app, track_id) = app_with_midi_track();
    app.state.perform.layout = vibez_project::PerformLayout::Clips;
    let _ = app.update(browser(BrowserMsg::DesktopFileHovered(path.clone())));
    let _ = app.update(browser(BrowserMsg::DesktopFileDropped(path)));

    run_until_idle(
        &mut app,
        browser(BrowserMsg::DragHoverClipSlot { track_id, row: 0 }),
    )
    .await;

    assert!(app.state.perform.clips.at(track_id, 0).is_some());
}

#[tokio::test]
async fn audio_from_another_app_dropped_on_empty_arrange_gets_a_new_audio_track() {
    let directory = tempfile::tempdir().unwrap();
    let path = kick(directory.path());
    let mut app = app();
    app.state.browser.audition_mode = AuditionMode::Raw;
    let _ = app.update(browser(BrowserMsg::DesktopFileHovered(path.clone())));
    let _ = app.update(browser(BrowserMsg::DragHoverEmptyArrangement { beat: 4.0 }));

    run_until_idle(&mut app, browser(BrowserMsg::DesktopFileDropped(path))).await;

    let track = &app.state.project_tracks.tracks[0];
    assert_eq!(track.kind, TrackKind::Audio);
    let clips = &app.state.arrange_content(track.id).unwrap().clips;
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].position, app.state.transport.beats_to_samples(4.0));
}

#[tokio::test]
async fn a_waiting_drop_the_pointer_settles_away_from_is_cancelled() {
    let directory = tempfile::tempdir().unwrap();
    let path = drum_phrase(directory.path());
    let mut app = app();
    let _ = app.update(browser(BrowserMsg::DesktopFileHovered(path.clone())));
    let _ = app.update(browser(BrowserMsg::DesktopFileDropped(path)));

    let _ = app.update(Message::View(ViewMsg::CursorMoved(10.0, 10.0)));

    assert!(app.state.browser.drag_source.is_none());
    assert_eq!(app.state.status_text, "Nothing here takes Rebeater P3.mid");
}

#[test]
fn window_file_events_become_desktop_drag_messages() {
    use iced::window::Event;
    let path = PathBuf::from("/tmp/Rebeater drags/Rebeater P3.mid");

    assert!(matches!(
        desktop_drop::desktop_drop_message(&Event::FileHovered(path.clone())),
        Some(Message::Browser(BrowserMsg::DesktopFileHovered(hovered))) if hovered == path
    ));
    assert!(matches!(
        desktop_drop::desktop_drop_message(&Event::FileDropped(path.clone())),
        Some(Message::Browser(BrowserMsg::DesktopFileDropped(dropped))) if dropped == path
    ));
    assert!(matches!(
        desktop_drop::desktop_drop_message(&Event::FilesHoveredLeft),
        Some(Message::Browser(BrowserMsg::DesktopDragLeft))
    ));
    assert!(desktop_drop::desktop_drop_message(&Event::Focused).is_none());
}
