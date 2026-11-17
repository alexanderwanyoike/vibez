//! A polled pointer reports the target under it as that target's widget would.

use std::path::PathBuf;

use super::*;

fn hovering() -> BrowserState {
    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::DesktopFileHovered(PathBuf::from(
        "/tmp/Rebeater drags/Rebeater P3.mid",
    )));
    browser
}

/// One beat per 100 pixels from beat 0, snapping to whole beats.
fn timing() -> LaneTiming {
    let geometry = TimelineGeometry::from_zoom(1.0, 0.0);
    LaneTiming {
        geometry,
        grid: GridConfig::default(),
    }
}

#[test]
fn a_lane_under_the_pointer_is_hovered_at_the_beat_its_widget_would_report() {
    let browser = hovering();
    let track_id = TrackId::new();
    let x = 300.0 + 4.0 * timing().geometry.pixels_per_beat();

    let messages = browser.pointer_messages(
        Some((PointerTarget::Lane(track_id), 300.0)),
        x,
        timing(),
        |_| true,
    );

    let expected_beat = timing().beat_at(4.0 * timing().geometry.pixels_per_beat());
    assert!(matches!(
        messages[..],
        [
            BrowserMsg::DragHoverTrack { track_id: lane, beat, compatible: true },
            BrowserMsg::DesktopPointerMoved,
        ] if lane == track_id && beat == expected_beat
    ));
}

#[test]
fn a_lane_that_turns_the_drag_away_says_so() {
    let browser = hovering();

    let messages = browser.pointer_messages(
        Some((PointerTarget::Lane(TrackId::new()), 0.0)),
        10.0,
        timing(),
        |_| false,
    );

    assert!(matches!(
        messages[0],
        BrowserMsg::DragHoverTrack {
            compatible: false,
            ..
        }
    ));
}

#[test]
fn the_new_track_row_and_slots_are_hovered_as_their_widgets_would() {
    let browser = hovering();
    let track_id = TrackId::new();
    let hover = |target| browser.pointer_messages(Some((target, 0.0)), 200.0, timing(), |_| true);

    assert!(matches!(
        hover(PointerTarget::NewTrackRow)[0],
        BrowserMsg::DragHoverEmptyArrangement { beat } if beat == timing().beat_at(200.0)
    ));
    assert!(matches!(
        hover(PointerTarget::NewTrackSlot { row: 3 })[0],
        BrowserMsg::DragHoverNewTrackSlot { row: 3 }
    ));
    assert!(matches!(
        hover(PointerTarget::ClipSlot { track_id, row: 2 })[0],
        BrowserMsg::DragHoverClipSlot { track_id: slot, row: 2 } if slot == track_id
    ));
}

#[test]
fn leaving_every_target_clears_the_one_the_drag_had() {
    let mut browser = hovering();
    let nothing = browser.pointer_messages(None, 0.0, timing(), |_| true);
    assert!(nothing.is_empty());

    browser.update(BrowserMsg::DragHoverNewTrackSlot { row: 0 });
    let left = browser.pointer_messages(None, 0.0, timing(), |_| true);

    assert!(matches!(left[..], [BrowserMsg::ClearDragTarget]));
}

#[test]
fn only_a_desktop_drag_still_hovering_is_polled() {
    assert!(!BrowserState::default().polls_desktop_pointer());
    let mut browser = hovering();
    assert!(browser.polls_desktop_pointer());

    browser.update(BrowserMsg::DesktopPointerMoved);
    assert!(browser.polls_desktop_pointer());

    browser.update(BrowserMsg::DesktopDragLeft);
    assert!(!browser.polls_desktop_pointer());
}

#[test]
fn a_drop_where_no_tagged_target_was_found_waits_for_the_widgets_to_report_one() {
    // Device cards and pads are not tagged, so after the drop the widget
    // under the released pointer reports itself and takes the file.
    let mut browser = hovering();
    for message in browser.pointer_messages(None, 0.0, timing(), |_| true) {
        browser.update(message);
    }

    browser.update(BrowserMsg::DesktopFileDropped(PathBuf::from(
        "/tmp/Rebeater drags/Rebeater P3.mid",
    )));

    assert!(browser.desktop_drop_waits_for_target());
}

#[test]
fn a_drop_after_the_pointer_crossed_a_lane_to_an_untagged_target_waits_for_the_widgets() {
    let mut browser = hovering();
    let lane = Some((PointerTarget::Lane(TrackId::new()), 0.0));
    for under in [lane, None] {
        for message in browser.pointer_messages(under, 100.0, timing(), |_| true) {
            browser.update(message);
        }
    }

    browser.update(BrowserMsg::DesktopFileDropped(PathBuf::from(
        "/tmp/Rebeater drags/Rebeater P3.mid",
    )));

    assert!(browser.desktop_drop_waits_for_target());
}
