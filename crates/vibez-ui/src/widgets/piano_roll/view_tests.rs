use super::*;
use crate::domains::view::ViewMsg;
use crate::timeline_geometry::{TimelineViewport, FRAME_MARGIN_PIXELS};
use iced::keyboard::{Event, Modifiers};
use iced::widget::canvas::Program;

const WIDTH: f32 = 852.0;

fn bounds() -> Rectangle {
    Rectangle::new(Point::ORIGIN, iced::Size::new(WIDTH, 400.0))
}

fn long_clip(loop_enabled: bool) -> UiNoteClip {
    UiNoteClip {
        id: ClipId::new(),
        name: "Sixteen bars".into(),
        position_beats: 0.0,
        duration_beats: 64.0,
        notes: Vec::new(),
        selected_notes: HashSet::new(),
        start_marker_beats: 0.0,
        loop_enabled,
        loop_start_beats: 8.0,
        loop_end_beats: 16.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    }
}

fn editor(clip: &UiNoteClip) -> PianoRollWidget {
    PianoRollWidget::from_clip(
        TrackId::new(),
        clip,
        0.0,
        clip.duration_beats,
        Color::WHITE,
        GridConfig::new(SnapGrid::SIXTEENTH, true, false, 0),
        default_scroll_y(400.0),
        PianoRollEditMode::Select,
    )
}

fn wheel(
    widget: &PianoRollWidget,
    state: &mut PianoRollState,
    modifiers: Modifiers,
    pointer: Point,
    delta: mouse::ScrollDelta,
) -> Option<Message> {
    widget.update(
        state,
        canvas::Event::Keyboard(Event::ModifiersChanged(modifiers)),
        bounds(),
        mouse::Cursor::Available(pointer),
    );
    widget
        .update(
            state,
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }),
            bounds(),
            mouse::Cursor::Available(pointer),
        )
        .1
}

fn requested_view(message: Option<Message>, clip: &UiNoteClip) -> TimelineViewport {
    match message {
        Some(Message::View(ViewMsg::SetClipEditorView { clip_id, viewport })) => {
            assert_eq!(clip_id, clip.id);
            viewport
        }
        other => panic!("expected a clip editor view, got {other:?}"),
    }
}

#[test]
fn a_looped_clip_opens_framed_on_its_loop() {
    let clip = long_clip(true);
    let widget = editor(&clip);

    assert!((widget.beat_to_x(8.0, &bounds()) - (KEY_WIDTH + FRAME_MARGIN_PIXELS)).abs() < 1e-3);
    assert!((widget.beat_to_x(16.0, &bounds()) - (WIDTH - FRAME_MARGIN_PIXELS)).abs() < 1e-3);
}

#[test]
fn an_unlooped_clip_opens_framed_on_its_length() {
    let clip = long_clip(false);
    let widget = editor(&clip);

    assert!((widget.beat_to_x(0.0, &bounds()) - KEY_WIDTH).abs() < 1e-3);
    let end = widget.beat_to_x(64.0, &bounds());
    assert!(end > WIDTH - 3.0 * FRAME_MARGIN_PIXELS && end <= WIDTH - FRAME_MARGIN_PIXELS);
}

#[test]
fn shift_wheel_zooms_in_around_the_pointer() {
    let clip = long_clip(true);
    let mut widget = editor(&clip);
    let pointer = Point::new(400.0, 200.0);
    let beat_under_pointer = widget.x_to_beat(pointer.x, &bounds());
    let pixels_per_beat = widget.pixels_per_beat(&bounds());

    let viewport = requested_view(
        wheel(
            &widget,
            &mut PianoRollState::default(),
            Modifiers::SHIFT,
            pointer,
            mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        ),
        &clip,
    );
    widget.view = Some(viewport);

    assert!(widget.pixels_per_beat(&bounds()) > pixels_per_beat);
    assert!((widget.x_to_beat(pointer.x, &bounds()) - beat_under_pointer).abs() < 1e-6);
}

#[test]
fn a_sideways_swipe_scrolls_time_and_a_plain_wheel_scrolls_pitch() {
    let clip = long_clip(true);
    let mut widget = editor(&clip);
    let pointer = Point::new(400.0, 200.0);
    let first_beat = widget.x_to_beat(KEY_WIDTH, &bounds());
    let mut state = PianoRollState::default();

    let viewport = requested_view(
        wheel(
            &widget,
            &mut state,
            Modifiers::empty(),
            pointer,
            mouse::ScrollDelta::Lines { x: -1.0, y: 0.0 },
        ),
        &clip,
    );
    widget.view = Some(viewport);
    assert!(widget.x_to_beat(KEY_WIDTH, &bounds()) > first_beat);

    let plain = wheel(
        &widget,
        &mut state,
        Modifiers::empty(),
        pointer,
        mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
    );
    assert!(matches!(
        plain,
        Some(Message::PianoRoll(PianoRollMsg::ScrollY(_)))
    ));
}

#[test]
fn a_loop_end_drags_past_the_clip_end_in_a_zoomed_out_view() {
    let mut clip = long_clip(true);
    clip.duration_beats = 12.0;
    let mut widget = editor(&clip);
    widget.view = Some(TimelineViewport::new(1.0, 0.0));
    let end = Point::new(widget.beat_to_x(16.0, &bounds()), 5.0);
    let target = Point::new(widget.beat_to_x(24.0, &bounds()), 5.0);
    let mut state = PianoRollState::default();

    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds(),
        mouse::Cursor::Available(end),
    );
    let message = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: target }),
            bounds(),
            mouse::Cursor::Available(target),
        )
        .1;

    assert!(matches!(
        message,
        Some(Message::UndoGesture { edit, .. }) if matches!(
            *edit,
            Message::PianoRoll(PianoRollMsg::SetNoteClipLoopRegion {
                loop_start_beats,
                loop_end_beats,
                ..
            }) if loop_start_beats == 8.0 && loop_end_beats == 24.0
        )
    ));
}

#[test]
fn zoomed_in_a_resized_note_ends_on_the_finer_grid_the_zoom_shows() {
    let mut clip = long_clip(false);
    clip.notes = vec![MidiNote {
        pitch: 60,
        start_beat: 0.0,
        duration_beats: 1.0,
        velocity: 100,
    }];
    let mut widget = editor(&clip);
    widget.grid = GridConfig::new(SnapGrid::SIXTEENTH, true, true, 0);
    let fitted_step = widget.effective_grid(&bounds()).beat_size();
    widget.view = Some(TimelineViewport::new(8.0, 0.0));
    let step = widget.effective_grid(&bounds()).beat_size();
    assert!(step < fitted_step, "{step} vs {fitted_step}");

    let y = widget.pitch_to_y(60) + KEY_HEIGHT / 2.0;
    let edge = Point::new(widget.beat_to_x(1.0, &bounds()) - 2.0, y);
    let target = Point::new(widget.beat_to_x(1.0 + 3.0 * step, &bounds()) - 2.0, y);
    let mut state = PianoRollState::default();
    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds(),
        mouse::Cursor::Available(edge),
    );
    let moved = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: target }),
            bounds(),
            mouse::Cursor::Available(target),
        )
        .1;

    let Some(Message::UndoGesture { edit, .. }) = moved else {
        panic!("a note resize is one undo step: {moved:?}");
    };
    let Message::PianoRoll(PianoRollMsg::SetNoteDurations { durations, .. }) = *edit else {
        panic!("dragging the edge resizes the note: {edit:?}");
    };
    let [(0, duration)] = durations[..] else {
        panic!("only the grabbed note resizes: {durations:?}");
    };
    assert!((duration - (1.0 + 3.0 * step)).abs() < 1e-6, "{duration}");
}
