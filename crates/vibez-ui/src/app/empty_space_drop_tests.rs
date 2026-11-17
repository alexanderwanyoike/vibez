//! A file dropped anywhere empty in Arrange or Perform lands as it would on
//! the new-track row or column, pointed at through the real view.

use std::path::PathBuf;

use iced::{Point, Rectangle};
use vibez_core::id::{SectionId, TrackId};
use vibez_core::midi::{InstrumentKind, TrackKind};

use super::new_track_drop_tests::{
    browser_drag, drum_phrase, kick, launcher, section_clip_starts, sections, the_new_track,
};
use super::test_support::{app, run_until_idle};
use super::view_harness::{bounds_of, hover_at, pointer_target_at, release_at};
use super::*;
use crate::domains::browser::BrowserMsg;
use crate::domains::desktop_pointer::PointerTarget;
use crate::domains::drop_ghost::GhostPlace;
use crate::state::{BrowserDropTarget, ProjectTrack, Workspace};
use crate::timeline_geometry::TimelineGeometry;

/// Tall enough that every view leaves room below its tracks.
const WINDOW_HEIGHT: f32 = 1_100.0;

fn tall(mut app: App) -> App {
    app.state.view.window_height = WINDOW_HEIGHT;
    app
}

/// Arrange holding one audio track.
fn arrange() -> (App, TrackId) {
    let mut app = tall(app());
    app.state.view.workspace = Workspace::Arrange;
    let track = ProjectTrack::new(TrackId::new(), "Audio 1".into(), 0);
    let id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    (app, id)
}

/// The clip launcher holding one audio track.
fn tall_launcher() -> (App, TrackId) {
    let (app, existing) = launcher();
    (tall(app), existing)
}

/// A selected Section in a project with one audio track.
fn tall_sections() -> (App, TrackId, SectionId) {
    let (app, existing, section_id) = sections();
    (tall(app), existing, section_id)
}

/// A launcher with no tracks at all.
fn empty_launcher() -> App {
    let (mut app, existing) = tall_launcher();
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .retain(|track| track.id != existing);
    app.state
        .perform
        .sync_project_tracks(&app.state.project_tracks.tracks);
    app
}

/// Sends what the view sends for a Browser drag released at `point`.
async fn release_browser_drag(app: &mut App, point: Point) {
    for message in release_at(app, point) {
        run_until_idle(app, message).await;
    }
}

/// Polls the pointer at the point `place` finds during another app's drag
/// of `path`, checks the ghost waits at `ghost`, then drops it.
async fn drop_from_another_app(
    app: &mut App,
    path: PathBuf,
    place: impl Fn(&App) -> Point,
    ghost: GhostPlace,
) {
    run_until_idle(
        app,
        Message::Browser(BrowserMsg::DesktopFileHovered(path.clone())),
    )
    .await;
    let point = place(app);
    let under = pointer_target_at(app, point);
    assert!(under.is_some(), "no tagged target at {point:?}");
    let _ = app.update(Message::DesktopPointerProbed { x: point.x, under });
    let shown = app.state.browser.drop_ghost(app.state.transport.bpm);
    assert_eq!(shown.map(|shown| shown.place), Some(ghost));
    run_until_idle(app, Message::Browser(BrowserMsg::DesktopFileDropped(path))).await;
}

fn new_track_row(app: &App) -> Rectangle {
    bounds_of(app, PointerTarget::NewTrackRow).expect("the new-track row shows")
}

fn new_track_slot(app: &App, row: u32) -> Rectangle {
    bounds_of(app, PointerTarget::NewTrackSlot { row }).expect("the new-track column shows")
}

/// How the shown timeline maps beats to its lanes' pixels.
fn lane_geometry(app: &App) -> TimelineGeometry {
    match app.state.view.workspace {
        Workspace::Perform => app.state.perform.section_editor.viewport().geometry(),
        _ => app.state.view.viewport.geometry(),
    }
}

/// A point `below` pixels under the new-track row, `beats` into its lane.
fn point_below_new_track_row(app: &App, beats: f64, below: f32) -> Point {
    let row = new_track_row(app);
    let geometry = lane_geometry(app);
    Point::new(
        row.x + geometry.beat_to_x(beats) + 2.0,
        row.y + row.height + below,
    )
}

/// A point near the workspace's right edge, well right of the new-track
/// column beside one track, on `row`.
fn point_right_of_new_track_column(app: &App, row: u32) -> Point {
    let span = app.workspace_span();
    let slot = new_track_slot(app, row);
    Point::new(span.left + span.width - 48.0, slot.center_y())
}

/// A point `below` pixels under the new-track column's last slot.
fn point_below_new_track_column(app: &App, below: f32) -> Point {
    let last = new_track_slot(app, LAST_ROW);
    Point::new(last.center_x(), last.y + last.height + below)
}

/// The launcher's last row shown, the nearest to the space below it.
const LAST_ROW: u32 = super::views_clip_launcher::LAUNCHER_ROWS - 1;

fn launcher_clip_row(app: &App, track_id: TrackId) -> Option<u32> {
    (0..super::views_clip_launcher::LAUNCHER_ROWS)
        .find(|row| app.state.perform.clips.at(track_id, *row).is_some())
}

#[tokio::test]
async fn a_browser_drag_released_below_arranges_last_track_adds_a_track_at_the_pointers_beat() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, existing) = arrange();
    browser_drag(&mut app, drum_phrase(directory.path()));
    let point = point_below_new_track_row(&app, 8.0, 120.0);

    release_browser_drag(&mut app, point).await;

    let track = the_new_track(&app, existing);
    assert_eq!(track.instrument_kind, Some(InstrumentKind::DrumRack));
    let clips = &app.state.arrange_content(track.id).unwrap().note_clips;
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].position_beats, 8.0);
}

#[test]
fn hovering_below_arranges_last_track_shows_the_ghost_in_the_new_track_row() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, _) = arrange();
    browser_drag(&mut app, drum_phrase(directory.path()));
    let point = point_below_new_track_row(&app, 4.0, 120.0);

    for message in hover_at(&app, point) {
        let _ = app.update(message);
    }

    assert_eq!(
        app.state.browser.drag_target,
        Some(BrowserDropTarget::EmptyArrangement { beat: 4.0 })
    );
}

#[tokio::test]
async fn audio_from_another_app_dropped_below_arranges_last_track_gets_an_audio_track() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, existing) = arrange();

    drop_from_another_app(
        &mut app,
        kick(directory.path()),
        |app| point_below_new_track_row(app, 2.0, 120.0),
        GhostPlace::NewTrack { beat: 2.0 },
    )
    .await;

    let track = the_new_track(&app, existing);
    assert_eq!(track.kind, TrackKind::Audio);
    let clips = &app.state.arrange_content(track.id).unwrap().clips;
    assert_eq!(clips.len(), 1);
    let start = clips[0]
        .span(app.state.transport.samples_per_beat())
        .start_beats;
    assert!((start - 2.0).abs() < 1e-6, "{start}");
}

#[tokio::test]
async fn a_browser_drag_released_below_a_sections_tracks_adds_a_track_in_that_section() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, existing, section_id) = tall_sections();
    browser_drag(&mut app, drum_phrase(directory.path()));
    let point = point_below_new_track_row(&app, 4.0, 80.0);

    release_browser_drag(&mut app, point).await;

    let track = the_new_track(&app, existing);
    assert_eq!(track.instrument_kind, Some(InstrumentKind::DrumRack));
    assert_eq!(section_clip_starts(&app, section_id, track.id), vec![4.0]);
}

#[tokio::test]
async fn audio_from_another_app_dropped_below_a_sections_tracks_lands_in_that_section() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, existing, section_id) = tall_sections();

    drop_from_another_app(
        &mut app,
        kick(directory.path()),
        |app| point_below_new_track_row(app, 2.0, 80.0),
        GhostPlace::NewTrack { beat: 2.0 },
    )
    .await;

    let track = the_new_track(&app, existing);
    assert_eq!(track.kind, TrackKind::Audio);
    let starts = section_clip_starts(&app, section_id, track.id);
    assert_eq!(starts.len(), 1);
    assert!((starts[0] - 2.0).abs() < 1e-6, "{starts:?}");
}

#[tokio::test]
async fn a_browser_drag_released_right_of_the_launchers_last_column_takes_that_rows_slot() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, existing) = tall_launcher();
    browser_drag(&mut app, drum_phrase(directory.path()));
    let point = point_right_of_new_track_column(&app, 2);

    release_browser_drag(&mut app, point).await;

    let track = the_new_track(&app, existing);
    assert_eq!(track.instrument_kind, Some(InstrumentKind::DrumRack));
    assert_eq!(launcher_clip_row(&app, track.id), Some(2));
}

#[test]
fn hovering_right_of_the_launchers_last_column_shows_the_ghost_in_that_rows_slot() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, _) = tall_launcher();
    browser_drag(&mut app, drum_phrase(directory.path()));
    let point = point_right_of_new_track_column(&app, 3);

    for message in hover_at(&app, point) {
        let _ = app.update(message);
    }

    assert_eq!(
        app.state.browser.drag_target,
        Some(BrowserDropTarget::NewTrackSlot { row: 3 })
    );
}

#[tokio::test]
async fn a_browser_drag_released_below_the_launchers_rows_takes_the_nearest_row() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, existing) = tall_launcher();
    browser_drag(&mut app, kick(directory.path()));
    let point = point_below_new_track_column(&app, 40.0);

    release_browser_drag(&mut app, point).await;

    let track = the_new_track(&app, existing);
    assert_eq!(track.kind, TrackKind::Audio);
    assert_eq!(launcher_clip_row(&app, track.id), Some(LAST_ROW));
}

#[tokio::test]
async fn files_from_another_app_dropped_right_of_or_below_the_launchers_columns_land() {
    let directory = tempfile::tempdir().unwrap();
    for (row, place) in [
        (1, point_right_of_new_track_column as fn(&App, u32) -> Point),
        (LAST_ROW, |app: &App, _| {
            point_below_new_track_column(app, 40.0)
        }),
    ] {
        let (mut app, existing) = tall_launcher();

        drop_from_another_app(
            &mut app,
            drum_phrase(directory.path()),
            |app| place(app, row),
            GhostPlace::NewTrackSlot { row },
        )
        .await;

        let track = the_new_track(&app, existing);
        assert_eq!(launcher_clip_row(&app, track.id), Some(row));
    }
}

#[tokio::test]
async fn an_empty_launcher_takes_a_drop_anywhere_beside_or_below_its_new_column() {
    let directory = tempfile::tempdir().unwrap();
    for (row, place) in [
        (2, point_right_of_new_track_column as fn(&App, u32) -> Point),
        (LAST_ROW, |app: &App, _| {
            point_below_new_track_column(app, 40.0)
        }),
    ] {
        let mut app = empty_launcher();
        browser_drag(&mut app, drum_phrase(directory.path()));
        let point = place(&app, row);

        release_browser_drag(&mut app, point).await;

        let [track] = &app.state.project_tracks.tracks[..] else {
            panic!("one new track");
        };
        assert_eq!(track.instrument_kind, Some(InstrumentKind::DrumRack));
        assert_eq!(launcher_clip_row(&app, track.id), Some(row));
    }
}

#[tokio::test]
async fn an_empty_launcher_takes_another_apps_drop_beside_or_below_its_new_column() {
    let directory = tempfile::tempdir().unwrap();
    for (row, place) in [
        (2, point_right_of_new_track_column as fn(&App, u32) -> Point),
        (LAST_ROW, |app: &App, _| {
            point_below_new_track_column(app, 40.0)
        }),
    ] {
        let mut app = empty_launcher();

        drop_from_another_app(
            &mut app,
            kick(directory.path()),
            |app| place(app, row),
            GhostPlace::NewTrackSlot { row },
        )
        .await;

        let [track] = &app.state.project_tracks.tracks[..] else {
            panic!("one new track");
        };
        assert_eq!(track.kind, TrackKind::Audio);
        assert_eq!(launcher_clip_row(&app, track.id), Some(row));
    }
}
