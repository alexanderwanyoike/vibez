//! Tests for the ruler's pointer gestures.

use iced::mouse::{Button, Cursor, Event};
use iced::widget::canvas;
use iced::{Point, Rectangle, Size};

use super::*;
use crate::state::{SnapGrid, UndoGestureId};
use crate::timeline_geometry::TimelineViewport;

/// The loop strip and the number strip of an Arrange ruler.
const STRIP_Y: f32 = 6.0;
const NUMBERS_Y: f32 = 25.0;

/// 20 px per beat with the loop over beats 16 to 32 (x 320 to 640).
fn ruler() -> RulerWidget {
    RulerWidget {
        playhead_beats: 0.0,
        play_start_beats: Some(0.0),
        zoom_level: 1.0,
        grid: GridConfig::new(SnapGrid::QUARTER, true, false, 0),
        scroll_offset_beats: 0.0,
        loop_brace: true,
        loop_enabled: true,
        loop_start_beats: 16.0,
        loop_end_beats: 32.0,
        time_selection_active: false,
        selection_start_beats: 0.0,
        selection_end_beats: 0.0,
    }
}

fn send(
    ruler: &RulerWidget,
    state: &mut RulerInteractionState,
    event: Event,
    at: Point,
) -> Option<Message> {
    <RulerWidget as canvas::Program<Message>>::update(
        ruler,
        state,
        canvas::Event::Mouse(event),
        Rectangle::new(Point::ORIGIN, Size::new(800.0, 36.0)),
        Cursor::Available(at),
    )
    .1
}

fn press(ruler: &RulerWidget, state: &mut RulerInteractionState, at: Point) -> Option<Message> {
    send(ruler, state, Event::ButtonPressed(Button::Left), at)
}

fn move_to(ruler: &RulerWidget, state: &mut RulerInteractionState, at: Point) -> Option<Message> {
    send(ruler, state, Event::CursorMoved { position: at }, at)
}

fn release(ruler: &RulerWidget, state: &mut RulerInteractionState, at: Point) -> Option<Message> {
    send(ruler, state, Event::ButtonReleased(Button::Left), at)
}

/// The loop a message sets, and the undo gesture it belongs to.
fn loop_edit(message: Option<Message>) -> Option<(UndoGestureId, f64, f64)> {
    match message? {
        Message::UndoGesture { id, edit } => match *edit {
            Message::Transport(TransportMsg::SetArrangementLoopRegion {
                start_beats,
                end_beats,
            }) => Some((id, start_beats, end_beats)),
            _ => None,
        },
        _ => None,
    }
}

#[test]
fn dragging_a_brace_end_moves_that_end_in_one_undo_gesture() {
    let ruler = ruler();
    let mut state = RulerInteractionState::default();

    assert!(press(&ruler, &mut state, Point::new(322.0, STRIP_Y)).is_none());
    assert!(
        move_to(&ruler, &mut state, Point::new(324.0, STRIP_Y)).is_none(),
        "a move inside the same grid step changes nothing"
    );
    let (first_id, first_start, first_end) =
        loop_edit(move_to(&ruler, &mut state, Point::new(402.0, STRIP_Y))).expect("start edit");
    let (second_id, second_start, _) =
        loop_edit(move_to(&ruler, &mut state, Point::new(442.0, STRIP_Y))).expect("start edit");

    assert_eq!((first_start, first_end), (20.0, 32.0));
    assert_eq!(second_start, 22.0);
    assert_eq!(first_id, second_id);
    assert!(release(&ruler, &mut state, Point::new(442.0, STRIP_Y)).is_none());
}

#[test]
fn dragging_the_brace_end_past_its_start_stops_one_grid_step_short() {
    let ruler = ruler();
    let mut state = RulerInteractionState::default();

    press(&ruler, &mut state, Point::new(638.0, STRIP_Y));
    let edit = loop_edit(move_to(&ruler, &mut state, Point::new(100.0, STRIP_Y)));

    assert_eq!(edit.map(|(_, start, end)| (start, end)), Some((16.0, 17.0)));
}

#[test]
fn dragging_the_brace_middle_moves_the_whole_loop() {
    let ruler = ruler();
    let mut state = RulerInteractionState::default();

    press(&ruler, &mut state, Point::new(480.0, STRIP_Y));
    let edit = loop_edit(move_to(&ruler, &mut state, Point::new(520.0, STRIP_Y)));

    assert_eq!(edit.map(|(_, start, end)| (start, end)), Some((18.0, 34.0)));
}

#[test]
fn dragging_in_the_empty_strip_draws_a_new_loop() {
    let ruler = ruler();
    let mut state = RulerInteractionState::default();

    press(&ruler, &mut state, Point::new(100.0, STRIP_Y));
    let edit = loop_edit(move_to(&ruler, &mut state, Point::new(200.0, STRIP_Y)));

    assert_eq!(edit.map(|(_, start, end)| (start, end)), Some((5.0, 10.0)));
}

#[test]
fn a_click_in_the_loop_strip_leaves_the_playhead_alone() {
    let ruler = ruler();
    let mut state = RulerInteractionState::default();

    press(&ruler, &mut state, Point::new(100.0, STRIP_Y));

    assert!(release(&ruler, &mut state, Point::new(100.0, STRIP_Y)).is_none());
}

#[test]
fn a_click_on_the_numbers_still_sets_the_play_start() {
    let ruler = ruler();
    let mut state = RulerInteractionState::default();

    press(&ruler, &mut state, Point::new(100.0, NUMBERS_Y));

    assert!(matches!(
        release(&ruler, &mut state, Point::new(100.0, NUMBERS_Y)),
        Some(Message::Transport(TransportMsg::SeekToBeat(beat))) if beat == 5.0
    ));
}

#[test]
fn a_click_on_the_numbers_wobbles_as_far_as_a_lane_click() {
    let ruler = ruler();
    let mut state = RulerInteractionState::default();
    let wobble = Point::new(100.0 + crate::widgets::drag_phase::CLICK_SLOP_PX, NUMBERS_Y);

    press(&ruler, &mut state, Point::new(100.0, NUMBERS_Y));

    assert!(move_to(&ruler, &mut state, wobble).is_none());
    assert!(matches!(
        release(&ruler, &mut state, wobble),
        Some(Message::Transport(TransportMsg::SeekToBeat(beat))) if beat == 5.0
    ));
}

#[test]
fn section_rulers_have_no_loop_strip() {
    let ruler = RulerWidget {
        loop_brace: false,
        ..ruler()
    };
    let mut state = RulerInteractionState::default();

    press(&ruler, &mut state, Point::new(100.0, STRIP_Y));

    assert!(matches!(
        release(&ruler, &mut state, Point::new(100.0, STRIP_Y)),
        Some(Message::Transport(TransportMsg::SeekToBeat(_)))
    ));
}

fn viewport_of(message: Option<Message>) -> Option<TimelineViewport> {
    match message? {
        Message::View(ViewMsg::SetViewport(viewport)) => Some(viewport),
        _ => None,
    }
}

#[test]
fn dragging_down_on_the_numbers_zooms_in_around_the_grabbed_beat() {
    let ruler = ruler();
    let mut state = RulerInteractionState::default();

    press(&ruler, &mut state, Point::new(400.0, NUMBERS_Y));
    let view = viewport_of(move_to(
        &ruler,
        &mut state,
        Point::new(400.0, NUMBERS_Y + 50.0),
    ));

    assert_eq!(
        view,
        Some(TimelineViewport::new(1.0, 0.0).ruler_dragged(20.0, 400.0, 50.0))
    );
    assert!(view.is_some_and(|view| view.zoom_level > 1.0));
    assert!(
        release(&ruler, &mut state, Point::new(400.0, NUMBERS_Y + 50.0)).is_none(),
        "a zoom drag never moves the play start"
    );
}

#[test]
fn dragging_sideways_on_the_numbers_pans_with_the_pointer() {
    let ruler = RulerWidget {
        scroll_offset_beats: 10.0,
        ..ruler()
    };
    let mut state = RulerInteractionState::default();

    press(&ruler, &mut state, Point::new(400.0, NUMBERS_Y));
    let view = viewport_of(move_to(&ruler, &mut state, Point::new(300.0, NUMBERS_Y)));

    assert_eq!(view, Some(TimelineViewport::new(1.0, 15.0)));
}

#[test]
fn double_clicking_the_numbers_fits_the_song() {
    let ruler = ruler();
    let mut state = RulerInteractionState::default();
    let at = Point::new(400.0, NUMBERS_Y);

    press(&ruler, &mut state, at);
    release(&ruler, &mut state, at);

    assert!(matches!(
        press(&ruler, &mut state, at),
        Some(Message::View(ViewMsg::ZoomToFit))
    ));
    assert!(release(&ruler, &mut state, at).is_none());
}

#[test]
fn double_clicking_the_loop_strip_does_not_fit() {
    let ruler = ruler();
    let mut state = RulerInteractionState::default();
    let at = Point::new(100.0, STRIP_Y);

    press(&ruler, &mut state, at);
    release(&ruler, &mut state, at);

    assert!(press(&ruler, &mut state, at).is_none());
}

#[test]
fn the_pointer_over_the_ruler_numbers_shows_it_zooms() {
    let ruler = ruler();
    let interaction = |state: &RulerInteractionState, at: Point| {
        <RulerWidget as canvas::Program<Message>>::mouse_interaction(
            &ruler,
            state,
            Rectangle::new(Point::ORIGIN, Size::new(800.0, 36.0)),
            Cursor::Available(at),
        )
    };
    let mut state = RulerInteractionState::default();
    let numbers = Point::new(100.0, NUMBERS_Y);

    assert_eq!(
        interaction(&state, numbers),
        iced::mouse::Interaction::ZoomIn
    );
    press(&ruler, &mut state, numbers);
    assert_eq!(
        interaction(&state, numbers),
        iced::mouse::Interaction::ZoomIn
    );
}

/// Clicks the numbers of `ruler` at `beat` with `modifiers` held, returning
/// the beat the release seeks to.
fn seek_beat_after_click(
    ruler: &RulerWidget,
    beat: f64,
    modifiers: iced::keyboard::Modifiers,
) -> Option<f64> {
    let at = Point::new(ruler.geometry().beat_to_x(beat), NUMBERS_Y);
    let mut state = RulerInteractionState::default();
    <RulerWidget as canvas::Program<Message>>::update(
        ruler,
        &mut state,
        canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)),
        Rectangle::new(Point::ORIGIN, Size::new(800.0, 36.0)),
        Cursor::Available(at),
    );
    press(ruler, &mut state, at);
    match release(ruler, &mut state, at) {
        Some(Message::Transport(TransportMsg::SeekToBeat(beat))) => Some(beat),
        _ => None,
    }
}

#[test]
fn a_ruler_click_seeks_to_the_nearest_grid_line() {
    assert_eq!(
        seek_beat_after_click(&ruler(), 9.2, iced::keyboard::Modifiers::empty()),
        Some(9.0)
    );
}

#[test]
fn an_alt_click_on_the_ruler_seeks_exactly_where_clicked() {
    let beat =
        seek_beat_after_click(&ruler(), 9.2, iced::keyboard::Modifiers::ALT).expect("a seek");

    assert!((beat - 9.2).abs() < 1e-3, "seeked to {beat}");
}
