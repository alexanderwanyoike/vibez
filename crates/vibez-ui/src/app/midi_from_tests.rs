//! A MIDI From choice is one undoable project edit that reaches the routing model and survives a reopen.

use std::sync::Arc;

use vibez_core::id::TrackId;
use vibez_core::midi_from::MidiFrom;

use crate::message::Message;

use crate::domains::midi_from::plugin_track;

fn routed_app() -> (super::App, TrackId, TrackId) {
    let mut app = super::test_support::app();
    let (source, receiver) = (plugin_track("Rebeater"), plugin_track("Lead"));
    let ids = (source.id, receiver.id);
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .extend([source, receiver]);
    (app, ids.0, ids.1)
}

fn midi_from(app: &super::App, track: TrackId) -> Option<TrackId> {
    app.state
        .find_track(track)
        .and_then(|track| track.midi_from)
        .map(MidiFrom::source)
}

#[test]
fn choosing_a_source_is_one_undoable_edit_that_reaches_the_routing_model() {
    let (mut app, source, receiver) = routed_app();

    let _ = app.update(Message::SetMidiFrom(receiver, Some(source)));

    assert_eq!(midi_from(&app, receiver), Some(source));
    assert!(app.state.project.dirty);
    let model = app.sidechain_model();
    let channel = model.iter().find(|channel| channel.id == receiver).unwrap();
    assert_eq!(channel.midi_from, Some(source));
    let _ = app.update(Message::Project(crate::domains::project::ProjectMsg::Undo));
    assert_eq!(midi_from(&app, receiver), None);
    assert!(!app.state.project.history.can_undo());
}

#[test]
fn choosing_the_current_source_is_not_an_edit() {
    let (mut app, _, receiver) = routed_app();

    let _ = app.update(Message::SetMidiFrom(receiver, None));

    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
}

#[tokio::test]
async fn midi_from_reopens_with_the_project() {
    let (mut app, source, receiver) = routed_app();
    let _ = app.update(Message::SetMidiFrom(receiver, Some(source)));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("routed.vzp");
    super::save_project_async(path.clone(), None, app.project_from_state())
        .await
        .unwrap();
    let loaded = super::load_project_async(path, None, vibez_core::constants::DEFAULT_SAMPLE_RATE)
        .await
        .unwrap();
    let mut reopened = super::test_support::app();

    reopened.rebuild_from_loaded_project(loaded);

    assert_eq!(midi_from(&reopened, receiver), Some(source));
    assert_eq!(midi_from(&reopened, source), None);
}
