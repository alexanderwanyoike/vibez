//! A file dragged in from another app is a Browser drag that the drop
//! releases on whatever target the pointer is over.

use std::path::PathBuf;

use super::*;
use crate::state::BrowserDropTarget;

fn path(file: &str) -> PathBuf {
    PathBuf::from("/tmp/Rebeater drags").join(file)
}

fn hovering(file: &str) -> BrowserState {
    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::DesktopFileHovered(path(file)));
    browser
}

fn lane(track_id: TrackId) -> BrowserMsg {
    BrowserMsg::DragHoverTrack {
        track_id,
        beat: 4.0,
        compatible: true,
    }
}

#[test]
fn a_file_hovering_the_window_becomes_the_drag() {
    let mut browser = BrowserState::default();

    let action = browser.update(BrowserMsg::DesktopFileHovered(path("Rebeater P3.mid")));

    assert_eq!(
        browser.drag_source,
        Some(MediaSourceRef::LocalFile {
            path: path("Rebeater P3.mid")
        })
    );
    assert_eq!(browser.drag_media(), Some(DragMedia::Midi));
    assert!(browser.drags_from_desktop());
    assert!(action
        .status
        .unwrap()
        .starts_with("Moving Rebeater P3.mid - choose a MIDI lane"));
}

#[test]
fn a_file_vibez_cannot_import_starts_no_drag() {
    let mut browser = BrowserState::default();

    let action = browser.update(BrowserMsg::DesktopFileHovered(path("notes.pdf")));

    assert_eq!(browser.drag_source, None);
    assert_eq!(
        action.status.as_deref(),
        Some("Vibez can't import notes.pdf")
    );
}

#[test]
fn only_the_first_of_several_hovered_files_is_dragged() {
    let mut browser = hovering("kick.wav");

    browser.update(BrowserMsg::DesktopFileHovered(path("snare.wav")));

    assert_eq!(browser.drag_label.as_deref(), Some("kick.wav"));
}

#[test]
fn a_drop_releases_on_the_target_already_under_the_pointer() {
    let mut browser = hovering("Rebeater P3.mid");
    let track_id = TrackId::new();
    browser.update(lane(track_id));

    let action = browser.update(BrowserMsg::DesktopFileDropped(path("Rebeater P3.mid")));

    assert_eq!(
        action.release,
        Some(BrowserDropTarget::ArrangementLane {
            track_id,
            beat: 4.0,
            compatible: true,
        })
    );
}

#[test]
fn a_drop_before_the_pointer_reaches_a_target_releases_on_the_next_one() {
    let mut browser = hovering("Rebeater P3.mid");

    let dropped = browser.update(BrowserMsg::DesktopFileDropped(path("Rebeater P3.mid")));
    assert_eq!(dropped.release, None);
    assert!(browser.desktop_drop_waits_for_target());

    let track_id = TrackId::new();
    let hovered = browser.update(lane(track_id));

    assert!(matches!(
        hovered.release,
        Some(BrowserDropTarget::ArrangementLane { track_id: lane, .. }) if lane == track_id
    ));
}

#[test]
fn a_waiting_drop_the_pointer_settles_away_from_every_target_is_cancelled() {
    let mut browser = hovering("Rebeater P3.mid");
    browser.update(BrowserMsg::DesktopFileDropped(path("Rebeater P3.mid")));

    let action = browser.update(BrowserMsg::DesktopPointerMoved);

    assert_eq!(browser.drag_source, None);
    assert!(!browser.drags_from_desktop());
    assert_eq!(
        action.status.as_deref(),
        Some("Nothing here takes Rebeater P3.mid")
    );
}

#[test]
fn a_miss_before_the_drop_or_after_a_target_was_found_changes_nothing() {
    let mut browser = hovering("kick.wav");
    browser.update(BrowserMsg::DesktopPointerMoved);
    assert!(browser.drag_source.is_some());

    browser.update(lane(TrackId::new()));
    browser.update(BrowserMsg::DesktopFileDropped(path("kick.wav")));
    let action = browser.update(BrowserMsg::DesktopPointerMoved);
    assert_eq!(action, BrowserAction::default());
}

#[test]
fn leaving_the_window_cancels_a_desktop_drag_but_not_a_browser_drag() {
    let mut desktop = hovering("kick.wav");
    desktop.update(BrowserMsg::DesktopDragLeft);
    assert_eq!(desktop.drag_source, None);
    assert!(!desktop.drags_from_desktop());

    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::BeginPendingDrag {
        source: MediaSourceRef::LocalFile {
            path: path("kick.wav"),
        },
        label: "kick.wav".into(),
        origin_x: 0.0,
        origin_y: 0.0,
    });
    browser.update(BrowserMsg::PendingDragMoved { x: 40.0, y: 0.0 });
    browser.update(BrowserMsg::DesktopDragLeft);
    assert!(browser.drag_source.is_some());
}

#[test]
fn a_browser_drag_never_releases_on_hover() {
    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::BeginPendingDrag {
        source: MediaSourceRef::LocalFile {
            path: path("kick.wav"),
        },
        label: "kick.wav".into(),
        origin_x: 0.0,
        origin_y: 0.0,
    });
    browser.update(BrowserMsg::PendingDragMoved { x: 40.0, y: 0.0 });

    assert_eq!(browser.update(lane(TrackId::new())).release, None);
    assert!(!browser.drags_from_desktop());
}

#[test]
fn an_empty_clip_slot_is_a_drop_target() {
    let mut browser = hovering("Rebeater P3.mid");
    let track_id = TrackId::new();

    let action = browser.update(BrowserMsg::DragHoverClipSlot { track_id, row: 2 });

    assert_eq!(
        browser.drag_target,
        Some(BrowserDropTarget::ClipSlot { track_id, row: 2 })
    );
    assert_eq!(action.status.as_deref(), Some("Clip slot in row 3"));
    assert_eq!(browser.drag_accepted(), Some(true));
}

#[test]
fn a_drop_after_the_pointer_found_no_target_is_cancelled() {
    let mut browser = hovering("Rebeater P3.mid");
    browser.update(BrowserMsg::DesktopPointerMoved);

    let action = browser.update(BrowserMsg::DesktopFileDropped(path("Rebeater P3.mid")));

    assert_eq!(action.release, None);
    assert_eq!(browser.drag_source, None);
    assert_eq!(
        action.status.as_deref(),
        Some("Nothing here takes Rebeater P3.mid")
    );
}

#[test]
fn a_new_file_replaces_a_drop_still_waiting_for_a_target() {
    let mut browser = hovering("Rebeater P3.mid");
    browser.update(BrowserMsg::DesktopFileDropped(path("Rebeater P3.mid")));

    browser.update(BrowserMsg::DesktopFileHovered(path("kick.wav")));

    assert_eq!(browser.drag_label.as_deref(), Some("kick.wav"));
    assert!(!browser.desktop_drop_waits_for_target());
}

#[test]
fn a_lane_of_the_other_kind_cancels_the_drop_with_its_reason() {
    let mut browser = hovering("Rebeater P3.mid");
    browser.update(BrowserMsg::DragHoverTrack {
        track_id: TrackId::new(),
        beat: 0.0,
        compatible: false,
    });

    let action = browser.update(BrowserMsg::DesktopFileDropped(path("Rebeater P3.mid")));

    assert_eq!(action.release, None);
    assert_eq!(browser.drag_source, None);
    assert_eq!(
        action.status.as_deref(),
        Some("MIDI can't go on an audio lane")
    );
}
