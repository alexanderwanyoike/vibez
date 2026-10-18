use super::*;
use crate::state::UndoGestureId;
use iced::widget::canvas::Program;

fn piano_roll_message(message: Option<Message>) -> Option<PianoRollMsg> {
    match message {
        Some(Message::UndoGesture { edit, .. }) => piano_roll_message(Some(*edit)),
        Some(Message::PianoRoll(message)) => Some(message),
        _ => None,
    }
}

fn gesture_id(message: &Option<Message>) -> Option<UndoGestureId> {
    match message {
        Some(Message::UndoGesture { id, .. }) => Some(*id),
        _ => None,
    }
}

#[test]
fn piano_roll_uses_triplet_grid_and_preserves_free_positions_when_snap_is_off() {
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(852.0, 400.0));
    let mut widget = PianoRollWidget::empty(TrackId::new(), 0.0, Color::WHITE);
    widget.grid = GridConfig::new(SnapGrid::EIGHTH.triplet(), true, false, 0);
    assert!((widget.snapped_beat(0.31, &bounds) - 1.0 / 3.0).abs() < 1e-9);

    widget.grid = GridConfig::new(SnapGrid::SIXTEENTH, false, false, 0);
    assert_eq!(widget.snapped_beat(0.31, &bounds), 0.31);
}

#[test]
fn creating_a_note_keeps_it_in_the_cell_the_pointer_is_over() {
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(852.0, 400.0));
    let mut widget = PianoRollWidget::empty(TrackId::new(), 0.0, Color::WHITE);
    widget.grid = GridConfig::new(SnapGrid::SIXTEENTH, true, false, 0);

    assert_eq!(widget.snapped_beat(0.4, &bounds), 0.5);
    assert_eq!(widget.snapped_beat_floor(0.4, &bounds), 0.25);
    assert_eq!(widget.snapped_beat_floor(0.3, &bounds), 0.25);
    assert_eq!(widget.snapped_beat_floor(0.25, &bounds), 0.25);
}

#[test]
fn creation_snapping_is_free_when_snap_is_disabled() {
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(852.0, 400.0));
    let mut widget = PianoRollWidget::empty(TrackId::new(), 0.0, Color::WHITE);
    widget.grid = GridConfig::new(SnapGrid::SIXTEENTH, false, false, 0);

    assert_eq!(widget.snapped_beat_floor(0.4, &bounds), 0.4);
}

#[test]
fn overlapping_notes_hit_test_to_the_one_drawn_on_top() {
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(852.0, 400.0));
    let mut widget = PianoRollWidget::empty(TrackId::new(), 0.0, Color::WHITE);
    widget.total_beats = 16.0;
    widget.scroll_y = 0.0;

    let note = |start: f64| MidiNote {
        pitch: 60,
        start_beat: start,
        duration_beats: 2.0,
        velocity: 100,
    };
    widget.clip = Some(PianoRollClipData {
        clip_id: ClipId::new(),
        notes: vec![note(0.0), note(0.5)],
        selected_notes: HashSet::new(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
    });

    let x = widget.beat_to_x(1.0, &bounds);
    let y = widget.pitch_to_y(60) + 4.0;
    assert_eq!(
        widget.hit_test_note(Point::new(x, y), &bounds).map(|h| h.0),
        Some(1)
    );
}

#[test]
fn loop_end_marker_drag_snaps_and_keeps_the_start_fixed() {
    let track_id = TrackId::new();
    let clip = UiNoteClip {
        id: ClipId::new(),
        name: "Two bars".into(),
        position_beats: 0.0,
        duration_beats: 8.0,
        notes: Vec::new(),
        selected_notes: HashSet::new(),
        start_marker_beats: 0.0,
        loop_enabled: true,
        loop_start_beats: 2.0,
        loop_end_beats: 6.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    };
    let widget = PianoRollWidget::from_clip(
        track_id,
        &clip,
        0.0,
        8.0,
        Color::WHITE,
        GridConfig::new(SnapGrid::SIXTEENTH, true, false, 0),
        0.0,
        PianoRollEditMode::Select,
    );
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(852.0, 400.0));
    let end = Point::new(widget.beat_to_x(6.0, &bounds), 5.0);
    let target = Point::new(widget.beat_to_x(7.1, &bounds), 5.0);
    let mut state = PianoRollState::default();

    let pressed = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(end),
        )
        .0;
    assert_eq!(pressed, canvas::event::Status::Captured);

    let message = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: target }),
            bounds,
            mouse::Cursor::Available(target),
        )
        .1;
    assert!(matches!(
        piano_roll_message(message.clone()),
        Some(PianoRollMsg::SetNoteClipLoopRegion {
            track_id: actual_track,
            clip_id: actual_clip,
            loop_start_beats: 2.0,
            loop_end_beats: 7.0,
        }) if actual_track == track_id && actual_clip == clip.id
    ));
    let second_target = Point::new(widget.beat_to_x(7.4, &bounds), 5.0);
    let second = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: second_target,
            }),
            bounds,
            mouse::Cursor::Available(second_target),
        )
        .1;
    assert!(gesture_id(&message).is_some());
    assert_eq!(gesture_id(&message), gesture_id(&second));
}

#[test]
fn start_marker_has_its_own_row_when_it_overlaps_loop_start() {
    let track_id = TrackId::new();
    let clip = UiNoteClip {
        id: ClipId::new(),
        name: "Pattern".into(),
        position_beats: 0.0,
        duration_beats: 8.0,
        notes: Vec::new(),
        selected_notes: HashSet::new(),
        start_marker_beats: 2.0,
        loop_enabled: true,
        loop_start_beats: 2.0,
        loop_end_beats: 6.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    };
    let widget = PianoRollWidget::from_clip(
        track_id,
        &clip,
        0.0,
        8.0,
        Color::WHITE,
        GridConfig::new(SnapGrid::SIXTEENTH, true, false, 0),
        0.0,
        PianoRollEditMode::Select,
    );
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(852.0, 400.0));
    let start = Point::new(widget.beat_to_x(2.0, &bounds), 15.0);
    let target = Point::new(widget.beat_to_x(3.0, &bounds), 15.0);
    let mut state = PianoRollState::default();

    assert_eq!(
        widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                bounds,
                mouse::Cursor::Available(start),
            )
            .0,
        canvas::event::Status::Captured
    );
    let message = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: target }),
            bounds,
            mouse::Cursor::Available(target),
        )
        .1;
    assert!(matches!(
        piano_roll_message(message),
        Some(PianoRollMsg::SetNoteClipStartMarker {
            track_id: actual_track,
            clip_id: actual_clip,
            start_marker_beats: 3.0,
        }) if actual_track == track_id && actual_clip == clip.id
    ));
}

#[test]
fn loop_drag_skips_moves_that_stay_in_the_same_grid_cell() {
    let track_id = TrackId::new();
    let clip = UiNoteClip {
        id: ClipId::new(),
        name: "Pattern".into(),
        position_beats: 0.0,
        duration_beats: 8.0,
        notes: Vec::new(),
        selected_notes: HashSet::new(),
        start_marker_beats: 0.0,
        loop_enabled: true,
        loop_start_beats: 2.0,
        loop_end_beats: 6.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    };
    let widget = PianoRollWidget::from_clip(
        track_id,
        &clip,
        0.0,
        8.0,
        Color::WHITE,
        GridConfig::new(SnapGrid::SIXTEENTH, true, false, 0),
        0.0,
        PianoRollEditMode::Select,
    );
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(852.0, 400.0));
    let end = Point::new(widget.beat_to_x(6.0, &bounds), 5.0);
    let same_cell = Point::new(widget.beat_to_x(6.05, &bounds), 5.0);
    let mut state = PianoRollState::default();

    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(end),
    );
    let message = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: same_cell,
            }),
            bounds,
            mouse::Cursor::Available(same_cell),
        )
        .1;

    assert!(message.is_none());
}

#[test]
fn loop_handle_keeps_its_resize_cursor_in_draw_mode() {
    let track_id = TrackId::new();
    let clip = UiNoteClip {
        id: ClipId::new(),
        name: "Pattern".into(),
        position_beats: 0.0,
        duration_beats: 8.0,
        notes: Vec::new(),
        selected_notes: HashSet::new(),
        start_marker_beats: 0.0,
        loop_enabled: true,
        loop_start_beats: 2.0,
        loop_end_beats: 6.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    };
    let widget = PianoRollWidget::from_clip(
        track_id,
        &clip,
        0.0,
        8.0,
        Color::WHITE,
        GridConfig::new(SnapGrid::SIXTEENTH, true, false, 0),
        0.0,
        PianoRollEditMode::Draw,
    );
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(852.0, 400.0));
    let end = Point::new(widget.beat_to_x(6.0, &bounds), 5.0);

    assert_eq!(
        widget.mouse_interaction(
            &PianoRollState::default(),
            bounds,
            mouse::Cursor::Available(end),
        ),
        mouse::Interaction::ResizingHorizontally
    );
}

#[test]
fn controller_navigation_never_nudges_selected_notes() {
    use iced::keyboard::{
        key::{Code, Named, Physical},
        Event, Key, Location, Modifiers,
    };
    let mut widget = PianoRollWidget::empty(TrackId::new(), 0.0, Color::WHITE);
    widget.clip = Some(PianoRollClipData {
        clip_id: ClipId::new(),
        notes: Vec::new(),
        selected_notes: [0].into(),
        start_marker_beats: 0.0,
        loop_enabled: true,
        loop_start_beats: 0.0,
        loop_end_beats: 16.0,
    });
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 300.0));
    for (named, code) in [
        (Named::ArrowLeft, Code::ArrowLeft),
        (Named::ArrowRight, Code::ArrowRight),
        (Named::ArrowUp, Code::ArrowUp),
        (Named::ArrowDown, Code::ArrowDown),
    ] {
        for modifiers in [Modifiers::empty(), Modifiers::SHIFT] {
            let event = canvas::Event::Keyboard(Event::KeyPressed {
                key: Key::Named(named),
                modified_key: Key::Named(named),
                physical_key: Physical::Code(code),
                location: Location::Standard,
                modifiers,
                text: None,
            });
            widget.reserve_arrow_keys = true;
            let (status, message) = widget.update(
                &mut PianoRollState::default(),
                event.clone(),
                bounds,
                mouse::Cursor::Unavailable,
            );
            assert_eq!(status, canvas::event::Status::Ignored);
            assert!(message.is_none());
            widget.reserve_arrow_keys = false;
            let (status, message) = widget.update(
                &mut PianoRollState::default(),
                event,
                bounds,
                mouse::Cursor::Unavailable,
            );
            assert_eq!(status, canvas::event::Status::Captured);
            assert!(message.is_some());
        }
    }
}

#[test]
fn the_piano_roll_reaches_every_note_the_pads_can_play() {
    let height = 400.0;
    let mut widget = PianoRollWidget::empty(TrackId::new(), 0.0, Color::WHITE);
    widget.scroll_y = clamp_scroll_y(f32::MAX, height);
    assert_eq!(widget.y_to_pitch(height - 1.0), 0, "the bottom row is C-1");
    widget.scroll_y = clamp_scroll_y(0.0, height);
    assert_eq!(
        widget.y_to_pitch(RULER_HEIGHT + 1.0),
        127,
        "the top row is G9"
    );
}

#[test]
fn dragging_a_selection_moves_every_note_together_in_one_undo_step() {
    let track_id = TrackId::new();
    let note = |pitch: u8, start_beat: f64| MidiNote {
        pitch,
        start_beat,
        duration_beats: 1.0,
        velocity: 100,
    };
    let clip = UiNoteClip {
        id: ClipId::new(),
        name: "Chord".into(),
        position_beats: 0.0,
        duration_beats: 8.0,
        notes: vec![note(60, 0.0), note(64, 2.0)],
        selected_notes: HashSet::from([0, 1]),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 8.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    };
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(852.0, 400.0));
    let widget = PianoRollWidget::from_clip(
        track_id,
        &clip,
        0.0,
        8.0,
        Color::WHITE,
        GridConfig::new(SnapGrid::SIXTEENTH, true, false, 0),
        default_scroll_y(bounds.height),
        PianoRollEditMode::Select,
    );
    let grab = Point::new(
        widget.beat_to_x(0.5, &bounds),
        widget.pitch_to_y(60) + KEY_HEIGHT / 2.0,
    );
    let mut state = PianoRollState::default();
    let event = |position| canvas::Event::Mouse(mouse::Event::CursorMoved { position });
    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(grab),
    );

    let target = Point::new(widget.beat_to_x(1.5, &bounds), grab.y - 2.0 * KEY_HEIGHT);
    let first = widget
        .update(
            &mut state,
            event(target),
            bounds,
            mouse::Cursor::Available(target),
        )
        .1;
    let Some(PianoRollMsg::MoveNotesAbsolute { mut moves, .. }) = piano_roll_message(first.clone())
    else {
        panic!("a drag moves the selection as it goes: {first:?}");
    };
    moves.sort_by_key(|(index, ..)| *index);
    assert_eq!(moves, vec![(0, 1.0, 62), (1, 3.0, 66)]);

    let further = Point::new(target.x + 1.0, target.y);
    let second = widget
        .update(
            &mut state,
            event(further),
            bounds,
            mouse::Cursor::Available(further),
        )
        .1;
    assert!(gesture_id(&first).is_some());
    assert_eq!(gesture_id(&first), gesture_id(&second));

    let released = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(further),
        )
        .1;
    assert!(
        piano_roll_message(released.clone()).is_none(),
        "release adds no second edit: {released:?}"
    );
}
