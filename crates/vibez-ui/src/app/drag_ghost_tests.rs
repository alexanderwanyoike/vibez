//! A drag's ghost, read through the router, matches the Clip its drop lands.

use std::path::{Path, PathBuf};

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::TrackId;
use vibez_core::midi::TrackKind;
use vibez_core::track::MediaSourceRef;

use super::test_support::{app, run_until_idle};
use super::*;
use crate::domains::browser::BrowserMsg;
use crate::domains::drop_ghost::{GhostMedia, GhostPlace};
use crate::state::{AuditionMode, ProjectTrack};

/// A one-bar type 0 drum phrase at 96 PPQ whose End of Track lands a tick
/// past the bar: kick on 1, snare on 2.
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

/// Three seconds of a half-scale tone.
fn tone(directory: &Path) -> PathBuf {
    let path = directory.join("pad.wav");
    let audio = DecodedAudio {
        channels: vec![vec![0.5; 3 * 44_100]],
        sample_rate: 44_100,
    };
    vibez_audio_io::file_io::write_wav_file(&path, &audio).unwrap();
    path
}

fn app_with_track(kind: TrackKind) -> (App, TrackId) {
    let mut app = app();
    app.state.browser.audition_mode = AuditionMode::Raw;
    let track = ProjectTrack::new_instrument(TrackId::new(), "Lane".into(), kind, 0);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    (app, track_id)
}

fn browser(message: BrowserMsg) -> Message {
    Message::Browser(message)
}

fn hover(track_id: TrackId, beat: f64) -> Message {
    browser(BrowserMsg::DragHoverTrack {
        track_id,
        beat,
        compatible: true,
    })
}

/// The ghost's start and length in beats.
fn ghost_span(app: &App) -> (GhostPlace, f64) {
    let ghost = app
        .state
        .browser
        .drop_ghost(app.state.transport.bpm)
        .expect("a ghost");
    (ghost.place, ghost.length_beats)
}

/// Every Clip on `track_id`'s lane as its start and length in beats.
fn landed(app: &App, track_id: TrackId) -> Vec<(f64, f64)> {
    let content = app.state.arrange_content(track_id).unwrap();
    let samples_per_beat = app.state.transport.samples_per_beat();
    content
        .clips
        .iter()
        .map(|clip| {
            let span = clip.span(samples_per_beat);
            (span.start_beats, span.length_beats)
        })
        .chain(
            content
                .note_clips
                .iter()
                .map(|clip| (clip.position_beats, clip.duration_beats)),
        )
        .collect()
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-6
}

#[tokio::test]
async fn a_midi_file_from_another_app_shows_the_clip_its_drop_lands() {
    let directory = tempfile::tempdir().unwrap();
    let path = drum_phrase(directory.path());
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    run_until_idle(
        &mut app,
        browser(BrowserMsg::DesktopFileHovered(path.clone())),
    )
    .await;
    let _ = app.update(hover(track_id, 8.0));

    let (place, length) = ghost_span(&app);
    assert_eq!(
        place,
        GhostPlace::Lane {
            track_id,
            beat: 8.0
        }
    );
    assert_eq!(length, 4.0);

    run_until_idle(&mut app, browser(BrowserMsg::DesktopFileDropped(path))).await;
    assert_eq!(landed(&app, track_id), vec![(8.0, length)]);
    assert!(app
        .state
        .browser
        .drop_ghost(app.state.transport.bpm)
        .is_none());
}

#[tokio::test]
async fn a_wav_from_another_app_shows_its_waveform_as_long_as_the_clip_it_lands() {
    let directory = tempfile::tempdir().unwrap();
    let path = tone(directory.path());
    let (mut app, track_id) = app_with_track(TrackKind::Audio);
    run_until_idle(
        &mut app,
        browser(BrowserMsg::DesktopFileHovered(path.clone())),
    )
    .await;
    let _ = app.update(hover(track_id, 4.0));

    let (place, length) = ghost_span(&app);
    assert_eq!(
        place,
        GhostPlace::Lane {
            track_id,
            beat: 4.0
        }
    );
    assert!(close(length, 3.0 * app.state.transport.bpm / 60.0));
    let ghost = app
        .state
        .browser
        .drop_ghost(app.state.transport.bpm)
        .unwrap();
    let GhostMedia::Audio { peaks, .. } = ghost.media else {
        panic!("an audio ghost");
    };
    assert!(peaks.iter().any(|&(_, high)| close(f64::from(high), 0.5)));

    run_until_idle(&mut app, browser(BrowserMsg::DesktopFileDropped(path))).await;
    let [(start, landed_length)] = landed(&app, track_id)[..] else {
        panic!("one Clip");
    };
    assert!(close(start, 4.0));
    assert!(close(landed_length, length));
}

#[tokio::test]
async fn a_browser_drag_shows_the_clip_its_release_lands() {
    let directory = tempfile::tempdir().unwrap();
    let path = tone(directory.path());
    let source = MediaSourceRef::LocalFile { path };
    let (mut app, track_id) = app_with_track(TrackKind::Audio);
    let _ = app.update(browser(BrowserMsg::BeginPendingDrag {
        source,
        label: "pad.wav".into(),
        origin_x: 0.0,
        origin_y: 0.0,
    }));
    run_until_idle(
        &mut app,
        browser(BrowserMsg::PendingDragMoved { x: 60.0, y: 0.0 }),
    )
    .await;
    let _ = app.update(hover(track_id, 2.0));
    let (_, length) = ghost_span(&app);
    let position_samples = app.state.transport.beats_to_samples(2.0);

    run_until_idle(
        &mut app,
        Message::DropSampleOnArrangement {
            track_id,
            position_samples,
        },
    )
    .await;

    let [(start, landed_length)] = landed(&app, track_id)[..] else {
        panic!("one Clip");
    };
    assert!(close(start, 2.0));
    assert!(close(landed_length, length));
    assert!(app.state.browser.ghost_cache.is_none());
}

#[tokio::test]
async fn a_drag_over_empty_arrange_shows_its_ghost_on_the_new_track() {
    let directory = tempfile::tempdir().unwrap();
    let path = drum_phrase(directory.path());
    let (mut app, _) = app_with_track(TrackKind::Audio);
    run_until_idle(&mut app, browser(BrowserMsg::DesktopFileHovered(path))).await;

    let _ = app.update(browser(BrowserMsg::DragHoverEmptyArrangement { beat: 4.0 }));

    let ghost = app
        .state
        .browser
        .drop_ghost(app.state.transport.bpm)
        .unwrap();
    assert_eq!(ghost.place, GhostPlace::NewTrack { beat: 4.0 });
    assert_eq!(
        ghost.media.new_track().label(),
        "New MIDI track · Drum Rack"
    );
}

#[tokio::test]
async fn a_browser_entry_dragged_past_the_click_slop_reads_its_file_for_the_ghost() {
    let directory = tempfile::tempdir().unwrap();
    let source = MediaSourceRef::LocalFile {
        path: drum_phrase(directory.path()),
    };
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    let _ = app.update(Message::View(crate::domains::view::ViewMsg::CursorMoved(
        100.0, 100.0,
    )));
    let _ = app.update(Message::BeginPendingBrowserDrag(
        source,
        "Rebeater P3.mid".into(),
    ));

    run_until_idle(
        &mut app,
        Message::View(crate::domains::view::ViewMsg::CursorMoved(400.0, 120.0)),
    )
    .await;
    let _ = app.update(hover(track_id, 4.0));

    assert_eq!(
        ghost_span(&app),
        (
            GhostPlace::Lane {
                track_id,
                beat: 4.0
            },
            4.0
        )
    );
}
