use super::*;
use iced::widget::canvas::Program;

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
        playhead_normalized: -1.0,
        loop_enabled: true,
        loop_start: 100,
        loop_end: 500,
        playback_direction: ClipPlaybackDirection::Forward,
        transient_markers: Default::default(),
        selected_transient_marker: None,
        warp_markers: Default::default(),
        selected_warp_marker: None,
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
                marker: None,
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
fn audio_ruler_maps_measures_from_project_tempo() {
    let mut widget = widget();
    widget.duration_samples = 4_000;
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));

    assert_eq!(widget.total_beats(), 8.0);
    assert_eq!(widget.beat_to_x(4.0, &bounds), 400.0);
    assert_eq!(widget.beat_to_x(8.0, &bounds), 800.0);
}

#[test]
fn reverse_mirrors_source_markers_and_pointer_mapping() {
    let mut widget = widget();
    widget.playback_direction = ClipPlaybackDirection::Reverse;
    widget.grid.snap_enabled = false;
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));

    assert_eq!(widget.timeline_to_x(100, &bounds), 800.0);
    assert_eq!(widget.timeline_to_x(300, &bounds), 600.0);
    assert_eq!(widget.x_to_local_frame(600.0, &bounds), 200);
    assert_eq!(widget.x_to_local_frame(800.0, &bounds), 0);
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
fn dragging_repairs_a_legacy_region_past_the_visible_clip() {
    let mut widget = widget();
    widget.duration_samples = 400;
    widget.loop_end = 900;
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
    let end = Point::new(bounds.width - 1.0, 5.0);
    let target = Point::new(600.0, 5.0);
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
        Some(ArrangementMsg::SetClipLoopRegion { loop_end: 500, .. })
    ));
}
