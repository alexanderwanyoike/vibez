//! Arrange track heights follow a header-edge drag and travel with the
//! project file.

use super::*;
use crate::domains::track_heights::{TrackHeightMsg, DEFAULT_TRACK_HEIGHT};
use crate::domains::view::ViewMsg;
use crate::state::ProjectTrack;
use vibez_core::id::TrackId;
use vibez_core::track::TrackInfo;
use vibez_project::Project;

fn app_with_tracks(count: usize) -> (App, Vec<TrackId>) {
    let mut app = super::test_support::app();
    let ids: Vec<TrackId> = (0..count).map(|_| TrackId::new()).collect();
    Arc::make_mut(&mut app.state.project_tracks).tracks.extend(
        ids.iter()
            .enumerate()
            .map(|(index, &id)| ProjectTrack::new(id, format!("Track {index}"), 0)),
    );
    (app, ids)
}

fn drag_header_edge(app: &mut App, track_id: TrackId, (from_y, to_y): (f32, f32)) {
    let _ = app.update(Message::View(ViewMsg::CursorMoved(100.0, from_y)));
    let _ = app.update(Message::TrackHeight(TrackHeightMsg::BeginResize(track_id)));
    let _ = app.update(Message::View(ViewMsg::CursorMoved(100.0, to_y)));
    let _ = app.update(Message::View(ViewMsg::MouseReleased));
}

#[test]
fn dragging_a_header_edge_resizes_that_track_and_dirties_the_project() {
    let (mut app, ids) = app_with_tracks(2);

    drag_header_edge(&mut app, ids[0], (300.0, 440.0));

    assert_eq!(
        app.state.track_heights.height(ids[0]),
        DEFAULT_TRACK_HEIGHT + 140.0
    );
    assert_eq!(app.state.track_heights.height(ids[1]), DEFAULT_TRACK_HEIGHT);
    assert_eq!(app.state.track_heights.resizing(), None);
    assert!(app.state.project.dirty);
}

#[test]
fn holding_alt_when_the_drag_starts_resizes_every_track() {
    let (mut app, ids) = app_with_tracks(3);
    app.state.view.held_modifiers = iced::keyboard::Modifiers::ALT;

    drag_header_edge(&mut app, ids[1], (300.0, 340.0));

    assert!(ids
        .iter()
        .all(|&id| app.state.track_heights.height(id) == DEFAULT_TRACK_HEIGHT + 40.0));
}

async fn reopen(path: PathBuf, project: Project) -> App {
    save_project_async(path.clone(), None, project)
        .await
        .unwrap();
    let mut app = super::test_support::app();
    app.rebuild_from_loaded_project(load_project_async(path, None).await.unwrap());
    app
}

#[tokio::test]
async fn track_heights_survive_save_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, ids) = app_with_tracks(2);
    drag_header_edge(&mut app, ids[0], (300.0, 440.0));

    let reopened = reopen(directory.path().join("Tall.vzp"), app.project_for_save()).await;

    assert_eq!(
        reopened.state.track_heights.height(ids[0]),
        DEFAULT_TRACK_HEIGHT + 140.0
    );
    assert_eq!(
        reopened.state.track_heights.height(ids[1]),
        DEFAULT_TRACK_HEIGHT
    );
}

#[tokio::test]
async fn opening_another_project_forgets_the_last_ones_heights() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, ids) = app_with_tracks(1);
    drag_header_edge(&mut app, ids[0], (300.0, 440.0));
    let mut project = Project::default();
    project.tracks.push(TrackInfo::new("Old"));
    let path = directory.path().join("Old.vzp");
    save_project_async(path.clone(), None, project)
        .await
        .unwrap();

    app.rebuild_from_loaded_project(load_project_async(path, None).await.unwrap());

    assert_eq!(app.state.track_heights.height(ids[0]), DEFAULT_TRACK_HEIGHT);
}
