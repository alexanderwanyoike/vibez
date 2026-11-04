use super::*;
use crate::timeline_geometry::{TimelineViewport, FRAME_MARGIN_PIXELS};
use iced::keyboard::{Event, Modifiers};
use iced::widget::canvas::Program;

/// At 120 BPM and 1 kHz a beat is 500 frames, so zoom 25 draws one frame
/// per pixel from the clip's source offset.
fn one_frame_per_pixel() -> Option<TimelineViewport> {
    Some(TimelineViewport::new(25.0, 0.0))
}

fn widget() -> AudioClipDetailWidget {
    AudioClipDetailWidget {
        location: vibez_project::TimelineLocation::Arrange,
        track_id: TrackId::new(),
        clip_id: ClipId::new(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![0.0; 1_000]],
            sample_rate: 1_000,
        }),
        duration_samples: 800,
        source_offset: 100,
        start_marker: 100,
        sample_rate: 1_000,
        bpm: 120.0,
        grid: GridConfig::new(crate::state::SnapGrid::QUARTER, true, false, 0),
        track_color: Color::WHITE,
        playhead_frame: None,
        loop_enabled: true,
        loop_start: 100,
        loop_end: 500,
        loop_content_end: 1_000,
        playback_direction: ClipPlaybackDirection::Forward,
        transient_markers: Default::default(),
        selected_transient_marker: None,
        warp_markers: Default::default(),
        selected_warp_marker: None,
        view: one_frame_per_pixel(),
    }
}

#[test]
fn right_clicking_waveform_opens_transient_context_at_source_frame() {
    let widget = widget();
    let bounds = Rectangle::new(Point::new(20.0, 40.0), iced::Size::new(800.0, 200.0));
    let cursor = Point::new(420.0, 120.0);
    let message = widget
        .update(
            &mut AudioClipDetailState::default(),
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
            bounds,
            mouse::Cursor::Available(cursor),
        )
        .1;

    assert!(matches!(
        message,
        Some(Message::View(ViewMsg::ShowContextMenu {
            x,
            y,
            target: ContextMenuTarget::AudioClipDetail {
                location: vibez_project::TimelineLocation::Arrange,
                track_id,
                clip_id,
                source_frame: 500,
                timeline_frame: 400,
                transient_marker: None,
                warp_marker: None,
            },
        })) if x == 420.0 && y == 120.0
            && track_id == widget.track_id && clip_id == widget.clip_id
    ));
}

fn arrangement_message(message: Option<Message>) -> Option<ArrangementMsg> {
    match message {
        Some(Message::UndoGesture { edit, .. }) => arrangement_message(Some(*edit)),
        Some(Message::Arrangement(message)) => Some(message),
        _ => None,
    }
}

#[test]
fn audio_loop_end_marker_drag_edits_source_frames() {
    let widget = widget();
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let end = Point::new(widget.timeline_to_x(widget.loop_end, &bounds), 5.0);
    let target = Point::new(600.0, 5.0);
    let mut state = AudioClipDetailState::default();

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
        arrangement_message(message),
        Some(ArrangementMsg::SetClipLoopRegion {
            track_id,
            clip_id,
            loop_start: 100,
            loop_end: 600,
        }) if track_id == widget.track_id && clip_id == widget.clip_id
    ));
}

#[test]
fn audio_loop_end_drags_past_the_clip_end_up_to_the_source_end() {
    let widget = widget();
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let end = Point::new(widget.timeline_to_x(widget.loop_end, &bounds), 5.0);
    let mut state = AudioClipDetailState::default();
    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(end),
    );

    let mut drag_to = |x: f32| {
        let target = Point::new(x, 5.0);
        arrangement_message(
            widget
                .update(
                    &mut state,
                    canvas::Event::Mouse(mouse::Event::CursorMoved { position: target }),
                    bounds,
                    mouse::Cursor::Available(target),
                )
                .1,
        )
    };
    assert!(matches!(
        drag_to(850.0),
        Some(ArrangementMsg::SetClipLoopRegion {
            loop_start: 100,
            loop_end: 1_000,
            ..
        })
    ));
    assert!(matches!(
        drag_to(2_000.0),
        Some(ArrangementMsg::SetClipLoopRegion {
            loop_end: 1_000,
            ..
        })
    ));
}

#[test]
fn overlapping_start_and_loop_start_have_separate_hit_rows() {
    let mut widget = widget();
    widget.grid.snap_enabled = false;
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let overlap_x = widget.timeline_to_x(widget.start_marker, &bounds);
    let target = Point::new(300.0, 15.0);
    let mut state = AudioClipDetailState::default();

    let pressed = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(Point::new(overlap_x, 15.0)),
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
        arrangement_message(message),
        Some(ArrangementMsg::SetClipStartMarker {
            track_id,
            clip_id,
            start_marker: 400,
        }) if track_id == widget.track_id && clip_id == widget.clip_id
    ));
}

#[test]
fn an_audio_clip_opens_framed_on_its_loop() {
    let mut widget = widget();
    widget.bpm = 1_200.0;
    widget.view = None;
    widget.loop_start = 300;
    widget.loop_end = 700;
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));

    assert!((widget.timeline_to_x(300, &bounds) - FRAME_MARGIN_PIXELS).abs() < 1e-3);
    assert!((widget.timeline_to_x(700, &bounds) - (800.0 - FRAME_MARGIN_PIXELS)).abs() < 1e-3);
}

fn wheel(
    widget: &AudioClipDetailWidget,
    modifiers: Modifiers,
    pointer: Point,
    bounds: Rectangle,
) -> Option<TimelineViewport> {
    let mut state = AudioClipDetailState::default();
    widget.update(
        &mut state,
        canvas::Event::Keyboard(Event::ModifiersChanged(modifiers)),
        bounds,
        mouse::Cursor::Available(pointer),
    );
    match widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
            }),
            bounds,
            mouse::Cursor::Available(pointer),
        )
        .1
    {
        Some(Message::View(ViewMsg::SetClipEditorView { clip_id, viewport }))
            if clip_id == widget.clip_id =>
        {
            Some(viewport)
        }
        _ => None,
    }
}

#[test]
fn shift_wheel_zooms_the_audio_editor_around_the_pointer() {
    let mut widget = widget();
    widget.bpm = 1_200.0;
    widget.view = Some(TimelineViewport::new(2.0, 0.0));
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let pointer = Point::new(100.0, 100.0);
    let frame_under_pointer = widget.x_to_timeline_frame(pointer.x, &bounds);

    widget.view = wheel(&widget, Modifiers::SHIFT, pointer, bounds);

    assert!(widget.view.is_some_and(|view| view.zoom_level > 2.0));
    assert!((widget.x_to_timeline_frame(pointer.x, &bounds) - frame_under_pointer).abs() < 1e-3);
}

#[test]
fn a_plain_wheel_scrolls_the_audio_editor_in_time() {
    let mut widget = widget();
    widget.bpm = 1_200.0;
    widget.view = Some(TimelineViewport::new(2.0, 0.0));
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(400.0, 200.0));
    let first_frame = widget.x_to_timeline_frame(0.0, &bounds);

    widget.view = wheel(
        &widget,
        Modifiers::empty(),
        Point::new(200.0, 100.0),
        bounds,
    );

    assert!(widget.x_to_timeline_frame(0.0, &bounds) > first_frame);
}

#[test]
fn the_audio_editor_scrolls_no_further_than_the_source() {
    let mut widget = widget();
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(400.0, 200.0));

    for _ in 0..50 {
        widget.view = wheel(
            &widget,
            Modifiers::empty(),
            Point::new(200.0, 100.0),
            bounds,
        );
    }

    let right_edge = widget.x_to_timeline_frame(400.0 - FRAME_MARGIN_PIXELS, &bounds);
    assert!((right_edge - 1_000.0).abs() < 1e-3);
}

#[test]
fn a_looped_clip_longer_than_its_source_scrolls_only_to_the_source_end() {
    let mut widget = widget();
    widget.duration_samples = 5_000;
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(400.0, 200.0));

    for _ in 0..50 {
        widget.view = wheel(
            &widget,
            Modifiers::empty(),
            Point::new(200.0, 100.0),
            bounds,
        );
    }

    let right_edge = widget.x_to_timeline_frame(400.0 - FRAME_MARGIN_PIXELS, &bounds);
    assert!((right_edge - 1_000.0).abs() < 1e-3);
}

#[test]
fn the_playhead_follows_the_loop_through_the_source() {
    let mut widget = widget();
    widget.playhead_frame = Some(450);
    assert_eq!(widget.playhead_timeline_frame(), Some(150));

    widget.playback_direction = ClipPlaybackDirection::Reverse;
    widget.playhead_frame = Some(0);
    assert_eq!(widget.playhead_timeline_frame(), Some(499));
}

#[test]
fn transient_marker_press_selects_and_drag_authors_a_new_source_position() {
    let mut widget = widget();
    widget.transient_markers.replace_suggestions([300]);
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let marker = Point::new(widget.source_to_x(300, &bounds), 80.0);
    let target = Point::new(widget.source_to_x(450, &bounds), 80.0);
    let mut state = AudioClipDetailState::default();

    let selected = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(marker),
        )
        .1;
    assert!(matches!(
        arrangement_message(selected),
        Some(ArrangementMsg::SelectTransientMarker {
            source_frame: Some(300),
            ..
        })
    ));

    let moved = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: target }),
            bounds,
            mouse::Cursor::Available(target),
        )
        .1;
    assert!(matches!(
        arrangement_message(moved),
        Some(ArrangementMsg::MoveTransientMarker {
            from: 300,
            to: 450,
            ..
        })
    ));
}

#[test]
fn double_clicking_a_transient_promotes_it_to_a_warp_marker() {
    let mut widget = widget();
    widget.transient_markers.replace_suggestions([300]);
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let marker = Point::new(widget.source_to_x(300, &bounds), 80.0);
    let mut state = AudioClipDetailState::default();

    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(marker),
    );
    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(marker),
    );
    let promoted = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(marker),
        )
        .1;

    assert!(matches!(
        arrangement_message(promoted),
        Some(ArrangementMsg::AddWarpMarker {
            source_frame: 300,
            timeline_frame: 200,
            ..
        })
    ));
}

#[test]
fn dragging_a_transient_handle_promotes_and_warps_in_one_gesture() {
    let mut widget = widget();
    widget.grid.snap_enabled = false;
    widget.transient_markers.replace_suggestions([300]);
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let handle = Point::new(widget.source_to_x(300, &bounds), AUDIO_RULER_HEIGHT + 3.0);
    let target = Point::new(400.0, AUDIO_RULER_HEIGHT + 3.0);
    let mut state = AudioClipDetailState::default();

    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(handle),
    );
    let promoted = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: target }),
            bounds,
            mouse::Cursor::Available(target),
        )
        .1;

    assert!(matches!(
        arrangement_message(promoted),
        Some(ArrangementMsg::AddWarpMarker {
            source_frame: 300,
            timeline_frame: 400,
            ..
        })
    ));
}

#[test]
fn double_clicking_empty_waveform_adds_a_transient_marker() {
    let widget = widget();
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let pointer = Point::new(200.0, 80.0);
    let mut state = AudioClipDetailState::default();

    let first = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(pointer),
        )
        .1;
    assert!(matches!(
        arrangement_message(first),
        Some(ArrangementMsg::SelectTransientMarker {
            source_frame: None,
            ..
        })
    ));
    let second = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(pointer),
        )
        .1;
    assert!(matches!(
        arrangement_message(second),
        Some(ArrangementMsg::AddTransientMarker {
            source_frame: 300,
            ..
        })
    ));
}

#[test]
fn warp_marker_drag_selects_its_source_anchor_and_snaps_timeline_position() {
    let mut widget = widget();
    assert!(widget.warp_markers.add(300, 200, 100, 900, 800));
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let marker = Point::new(widget.timeline_to_x(300, &bounds), 25.0);
    let target = Point::new(400.0, 25.0);
    let mut state = AudioClipDetailState::default();

    let selected = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(marker),
        )
        .1;
    assert!(matches!(
        arrangement_message(selected),
        Some(ArrangementMsg::SelectWarpMarker {
            source_frame: Some(300),
            ..
        })
    ));

    let moved = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: target }),
            bounds,
            mouse::Cursor::Available(target),
        )
        .1;
    assert!(matches!(
        arrangement_message(moved),
        Some(ArrangementMsg::MoveWarpMarker {
            source_frame: 300,
            timeline_frame: 500,
            ..
        })
    ));
}

#[test]
fn source_markers_follow_the_piecewise_timeline_map() {
    let mut widget = widget();
    assert!(widget.warp_markers.add(300, 400, 100, 900, 800));
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));

    assert_eq!(widget.source_to_x(300, &bounds), 400.0);
    assert_eq!(widget.transient_source_from_x(200.0, &bounds), 200);
}

#[test]
fn audio_loop_drag_skips_an_unchanged_snapped_region() {
    let mut widget = widget();
    widget.loop_end = 600;
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let end = Point::new(widget.timeline_to_x(widget.loop_end, &bounds), 5.0);
    let mut state = AudioClipDetailState::default();

    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(end),
    );
    let message = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: end }),
            bounds,
            mouse::Cursor::Available(end),
        )
        .1;

    assert!(message.is_none());
}

#[test]
fn audio_loop_overshoot_keeps_one_grid_cell_between_the_handles() {
    let mut widget = widget();
    widget.loop_end = 900;
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let start = Point::new(widget.timeline_to_x(widget.loop_start, &bounds), 5.0);
    let past_end = Point::new(1_000.0, 5.0);
    let mut state = AudioClipDetailState::default();

    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(start),
    );
    let message = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: past_end }),
            bounds,
            mouse::Cursor::Available(past_end),
        )
        .1;

    assert!(matches!(
        arrangement_message(message),
        Some(ArrangementMsg::SetClipLoopRegion {
            loop_start: 400,
            loop_end: 900,
            ..
        })
    ));
}

#[test]
fn a_loop_end_past_the_clip_end_is_drawn_and_grabbed_where_it_is() {
    let mut widget = widget();
    widget.grid.snap_enabled = false;
    widget.duration_samples = 400;
    widget.loop_end = 700;
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let end = Point::new(widget.timeline_to_x(widget.loop_end, &bounds), 5.0);
    assert_eq!(end.x, 600.0);
    let target = Point::new(650.0, 5.0);
    let mut state = AudioClipDetailState::default();

    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(end),
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
        arrangement_message(message),
        Some(ArrangementMsg::SetClipLoopRegion { loop_end: 750, .. })
    ));
}

#[test]
fn a_looped_start_marker_drags_past_the_clip_end_inside_its_loop() {
    let mut widget = widget();
    widget.grid.snap_enabled = false;
    widget.duration_samples = 400;
    widget.loop_end = 900;
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let start = Point::new(widget.timeline_to_x(widget.start_marker, &bounds), 15.0);
    let target = Point::new(600.0, 15.0);
    let mut state = AudioClipDetailState::default();

    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(start),
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
        arrangement_message(message),
        Some(ArrangementMsg::SetClipStartMarker {
            start_marker: 700,
            ..
        })
    ));
}

#[test]
fn moving_the_whole_audio_loop_stops_at_the_source_end() {
    let widget = widget();
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let grab = Point::new(200.0, 5.0);
    let past_the_source = Point::new(2_000.0, 5.0);
    let mut state = AudioClipDetailState::default();

    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(grab),
    );
    let message = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: past_the_source,
            }),
            bounds,
            mouse::Cursor::Available(past_the_source),
        )
        .1;

    assert!(matches!(
        arrangement_message(message),
        Some(ArrangementMsg::SetClipLoopRegion {
            loop_start: 600,
            loop_end: 1_000,
            ..
        })
    ));
}

#[test]
fn dragging_down_the_audio_ruler_zooms_in_around_the_press() {
    let mut widget = widget();
    widget.bpm = 1_200.0;
    widget.view = Some(TimelineViewport::new(2.0, 0.0));
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let press = Point::new(100.0, 25.0);
    let frame_under_press = widget.x_to_timeline_frame(press.x, &bounds);
    let mut state = AudioClipDetailState::default();

    let pressed = widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(press),
    );
    assert_eq!(pressed.0, canvas::event::Status::Captured);
    let below = Point::new(press.x, press.y + 60.0);
    let message = widget
        .update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: below }),
            bounds,
            mouse::Cursor::Available(below),
        )
        .1;
    let Some(Message::View(ViewMsg::SetClipEditorView { viewport, .. })) = message else {
        panic!("expected a clip editor view, got {message:?}");
    };
    widget.view = Some(viewport);

    assert!(viewport.zoom_level > 2.0);
    assert!((widget.x_to_timeline_frame(press.x, &bounds) - frame_under_press).abs() < 1e-3);
}

#[test]
fn pressing_an_audio_loop_handle_pins_the_framing_so_the_drag_does_not_reframe() {
    let mut widget = widget();
    widget.bpm = 1_200.0;
    widget.view = None;
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let end_x = widget.timeline_to_x(widget.loop_end, &bounds);

    let pressed = widget.update(
        &mut AudioClipDetailState::default(),
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(Point::new(end_x, 5.0)),
    );
    let Some(Message::View(ViewMsg::SetClipEditorView { viewport, .. })) = pressed.1 else {
        panic!("expected the framing to be pinned, got {:?}", pressed.1);
    };
    widget.view = Some(viewport);
    widget.loop_end = 900;

    assert!((widget.timeline_to_x(500, &bounds) - end_x).abs() < 1e-3);
}

fn drag_with_alt(widget: &AudioClipDetailWidget, from: Point, to: Point) -> Option<ArrangementMsg> {
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let mut state = AudioClipDetailState::default();
    widget.update(
        &mut state,
        canvas::Event::Keyboard(Event::ModifiersChanged(Modifiers::ALT)),
        bounds,
        mouse::Cursor::Available(from),
    );
    widget.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(from),
    );
    arrangement_message(
        widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved { position: to }),
                bounds,
                mouse::Cursor::Available(to),
            )
            .1,
    )
}

#[test]
fn alt_drags_an_audio_loop_handle_off_the_grid() {
    let widget = widget();
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let end = Point::new(widget.timeline_to_x(widget.loop_end, &bounds), 5.0);

    let message = drag_with_alt(&widget, end, Point::new(613.0, 5.0));

    assert!(
        matches!(
            message,
            Some(ArrangementMsg::SetClipLoopRegion {
                loop_start: 100,
                loop_end: 713,
                ..
            })
        ),
        "{message:?}"
    );
}

#[test]
fn alt_drags_the_audio_start_marker_off_the_grid() {
    let widget = widget();
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let start = Point::new(widget.timeline_to_x(widget.start_marker, &bounds), 15.0);

    let message = drag_with_alt(&widget, start, Point::new(213.0, 15.0));

    assert!(
        matches!(
            message,
            Some(ArrangementMsg::SetClipStartMarker {
                start_marker: 313,
                ..
            })
        ),
        "{message:?}"
    );
}
