//! A pointer polled during another app's drag shows the ghost and lands the drop.

use std::path::{Path, PathBuf};

use iced::{Point, Rectangle, Size};
use vibez_core::id::TrackId;
use vibez_core::midi::TrackKind;

use super::test_support::{app, run_until_idle};
use super::*;
use crate::domains::browser::BrowserMsg;
use crate::domains::desktop_pointer::PointerTarget;
use crate::domains::drop_ghost::GhostPlace;
use crate::state::ProjectTrack;

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

fn app_with_midi_track() -> (App, TrackId) {
    let mut app = app();
    let track = ProjectTrack::new_instrument(TrackId::new(), "MIDI 1".into(), TrackKind::Midi, 0);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    (app, track_id)
}

fn lane_bounds() -> Rectangle {
    Rectangle::new(Point::new(400.0, 200.0), Size::new(900.0, 70.0))
}

#[tokio::test]
async fn a_polled_pointer_over_a_lane_shows_the_ghost_where_the_drop_then_lands() {
    let directory = tempfile::tempdir().unwrap();
    let path = drum_phrase(directory.path());
    let (mut app, track_id) = app_with_midi_track();
    run_until_idle(
        &mut app,
        Message::Browser(BrowserMsg::DesktopFileHovered(path.clone())),
    )
    .await;
    let pixels_per_beat = app.state.view.viewport.geometry().pixels_per_beat();
    let point = Point::new(400.0 + 8.0 * pixels_per_beat, 230.0);

    let _ = app.update(Message::DesktopPointerProbed {
        point,
        under: Some((PointerTarget::Lane(track_id), lane_bounds())),
    });

    let ghost = app
        .state
        .browser
        .drop_ghost(app.state.transport.bpm)
        .expect("a ghost");
    let GhostPlace::Lane {
        track_id: lane,
        beat,
    } = ghost.place
    else {
        panic!("a lane ghost");
    };
    assert_eq!(lane, track_id);
    run_until_idle(
        &mut app,
        Message::Browser(BrowserMsg::DesktopFileDropped(path)),
    )
    .await;
    let clips = &app.state.arrange_content(track_id).unwrap().note_clips;
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].position_beats, beat);
}

#[tokio::test]
async fn a_polled_pointer_off_every_target_takes_the_ghost_away() {
    let directory = tempfile::tempdir().unwrap();
    let path = drum_phrase(directory.path());
    let (mut app, track_id) = app_with_midi_track();
    run_until_idle(
        &mut app,
        Message::Browser(BrowserMsg::DesktopFileHovered(path)),
    )
    .await;
    let _ = app.update(Message::DesktopPointerProbed {
        point: Point::new(500.0, 230.0),
        under: Some((PointerTarget::Lane(track_id), lane_bounds())),
    });

    let _ = app.update(Message::DesktopPointerProbed {
        point: Point::new(10.0, 10.0),
        under: None,
    });

    assert!(app.state.browser.drag_target.is_none());
    assert!(app
        .state
        .browser
        .drop_ghost(app.state.transport.bpm)
        .is_none());
}

#[test]
fn a_probe_after_the_drag_ended_changes_nothing() {
    let (mut app, track_id) = app_with_midi_track();

    let _ = app.update(Message::DesktopPointerProbed {
        point: Point::new(500.0, 230.0),
        under: Some((PointerTarget::Lane(track_id), lane_bounds())),
    });

    assert!(app.state.browser.drag_target.is_none());
}
