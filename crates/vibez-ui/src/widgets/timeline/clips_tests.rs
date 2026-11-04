//! Tests for the clip lane canvas.
//!
//! Split out of `clips.rs` to keep that file inside the project's
//! 1,000-line ceiling.

use std::collections::HashSet;

use iced::keyboard::key::{Code, Physical};
use iced::keyboard::{Event, Key, Location, Modifiers};
use iced::widget::canvas;
use iced::{mouse, Color, Point, Rectangle, Size};

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;
use crate::message::Message;
use crate::state::{
    ContextMenuTarget, GridConfig, ProjectTrack, TrackTimelineContent, UndoGestureId,
};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;

use super::*;
use crate::widgets::timeline::fade_drag;

fn track_canvas(content: &TrackTimelineContent, zoom_level: f32) -> TrackClipCanvas {
    track_canvas_at(content, zoom_level, 0.0, 800.0, 16.0)
}

fn track_canvas_at(
    content: &TrackTimelineContent,
    zoom_level: f32,
    scroll_offset_beats: f64,
    viewport_width: f32,
    total_beats: f64,
) -> TrackClipCanvas {
    let track_id = TrackId::new();
    let track = ProjectTrack::new(track_id, "Track 1".into(), 0);
    TrackClipCanvas::from_track(
        &track,
        content,
        0.0,
        zoom_level,
        GridConfig::new(crate::state::SnapGrid::EIGHTH, true, false, 0),
        scroll_offset_beats,
        viewport_width,
        total_beats,
        44_100,
        true,
        Color::BLACK,
        120.0,
        track_id,
        vec![track_id],
        HashSet::new(),
        false,
        0.0,
        0.0,
        false,
        0.0,
        0.0,
        None,
        false,
        None,
        None,
    )
}

fn empty_track_canvas() -> TrackClipCanvas {
    track_canvas(&TrackTimelineContent::default(), 1.0)
}

fn note_clip(position_beats: f64) -> crate::state::UiNoteClip {
    crate::state::UiNoteClip {
        id: ClipId::new(),
        name: "Dense capture".into(),
        position_beats,
        duration_beats: 32.0,
        notes: (0..64)
            .map(|index| MidiNote {
                pitch: 36 + (index % 12) as u8,
                velocity: 100,
                start_beat: index as f64 * 0.25,
                duration_beats: 0.2,
            })
            .collect(),
        selected_notes: HashSet::new(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: GrooveGrid::Off,
    }
}

#[test]
fn note_clips_keep_their_notes_at_one_pixel_per_beat() {
    let mut content = TrackTimelineContent::default();
    content.note_clips.push(note_clip(0.0));

    let canvas = track_canvas(&content, 0.05);

    assert_eq!(canvas.geometry().pixels_per_beat(), 1.0);
    assert_eq!(canvas.note_clips.len(), 1);
    assert_eq!(canvas.note_clips[0].notes.len(), 64);
}

#[test]
fn track_canvas_materialises_only_the_visible_note_clips() {
    let mut content = TrackTimelineContent::default();
    let visible = note_clip(0.0);
    let visible_id = visible.id;
    content.note_clips.push(visible);
    content.note_clips.push(note_clip(128.0));

    let canvas = track_canvas(&content, 1.0);

    assert_eq!(canvas.note_clips.len(), 1);
    assert_eq!(canvas.note_clips[0].clip_id, visible_id);
    assert_eq!(canvas.note_clips[0].notes.len(), 64);
}

#[test]
fn looped_note_previews_repeat_their_notes_on_every_pass() {
    let mut clip = note_clip(0.0);
    clip.duration_beats = 8.0;
    clip.notes = vec![MidiNote {
        pitch: 60,
        velocity: 100,
        start_beat: 2.0,
        duration_beats: 0.5,
    }];
    clip.start_marker_beats = 2.0;
    clip.loop_enabled = true;
    clip.loop_start_beats = 1.0;
    clip.loop_end_beats = 3.0;
    let mut content = TrackTimelineContent::default();
    content.note_clips.push(clip);

    let canvas = track_canvas(&content, 1.0);

    assert_eq!(
        canvas.note_clips[0].notes,
        vec![
            (60, 0.0, 0.5),
            (60, 2.0, 0.5),
            (60, 4.0, 0.5),
            (60, 6.0, 0.5),
        ]
    );
}

#[test]
fn scrolled_long_section_materializes_visible_split_midi_fragments() {
    let mut content = TrackTimelineContent::default();
    content.note_clips.push(note_clip(0.0));
    let mut left_fragment = note_clip(96.0);
    left_fragment.duration_beats = 4.0;
    let left_id = left_fragment.id;
    let mut right_fragment = note_clip(100.0);
    right_fragment.duration_beats = 4.0;
    let right_id = right_fragment.id;
    content.note_clips.push(left_fragment);
    content.note_clips.push(right_fragment);

    let canvas = track_canvas_at(&content, 2.0, 94.0, 480.0, 128.0);
    let materialized: HashSet<_> = canvas.note_clips.iter().map(|clip| clip.clip_id).collect();

    assert_eq!(materialized, HashSet::from([left_id, right_id]));
}

#[test]
fn independently_constructed_ruler_and_lane_align_from_the_same_viewport() {
    let mut lane = empty_track_canvas();
    lane.zoom_level = 3.0;
    lane.scroll_offset_beats = 9.5;
    let ruler = RulerWidget {
        playhead_beats: -1.0,
        play_start_beats: None,
        zoom_level: 3.0,
        grid: GridConfig::new(crate::state::SnapGrid::EIGHTH, true, false, 0),
        scroll_offset_beats: 9.5,
        loop_brace: false,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 64.0,
        time_selection_active: false,
        selection_start_beats: 0.0,
        selection_end_beats: 0.0,
        snap_targets: Default::default(),
    };

    assert_eq!(ruler.geometry().beat_to_x(13.25), lane.beat_to_x(13.25));
}

#[test]
fn section_lane_keeps_edit_cursor_and_playback_playhead_distinct() {
    let canvas = empty_track_canvas().with_playback_playhead(Some(7.5));

    assert_eq!(canvas.playhead_beats, 0.0);
    assert_eq!(canvas.playback_playhead_beats, Some(7.5));
}

#[test]
fn one_pixel_shift_wheel_event_requests_continuous_cursor_anchored_zoom() {
    let canvas = empty_track_canvas().with_vertical_track_scrolling();
    let mut state = ClipInteractionState {
        shift_held: true,
        ..ClipInteractionState::default()
    };
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
    let (status, message) = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: 1.0 },
        }),
        bounds,
        mouse::Cursor::Available(Point::new(320.0, 20.0)),
    );

    assert_eq!(status, canvas::event::Status::Captured);
    assert!(matches!(
        message,
        Some(Message::View(ViewMsg::ZoomAround { factor, anchor_x }))
            if factor > 1.0 && factor < 1.01 && anchor_x == 320.0
    ));
}

#[test]
fn section_lane_bubbles_plain_vertical_wheel_to_its_track_scroller() {
    let canvas = empty_track_canvas().with_vertical_track_scrolling();
    let mut state = ClipInteractionState::default();
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
    let (status, message) = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -12.0 },
        }),
        bounds,
        mouse::Cursor::Available(Point::new(320.0, 20.0)),
    );

    assert_eq!(status, canvas::event::Status::Ignored);
    assert!(message.is_none());
}

#[test]
fn arrangement_lane_keeps_plain_vertical_wheel_timeline_panning() {
    let canvas = empty_track_canvas();
    let mut state = ClipInteractionState::default();
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
    let (status, message) = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -12.0 },
        }),
        bounds,
        mouse::Cursor::Available(Point::new(320.0, 20.0)),
    );

    assert_eq!(status, canvas::event::Status::Captured);
    assert!(matches!(
        message,
        Some(Message::View(ViewMsg::ScrollArrangement(delta))) if delta < 0.0
    ));
}

#[test]
fn middle_drag_pans_continuously_without_seeking_or_selecting() {
    let mut canvas = empty_track_canvas();
    canvas.scroll_offset_beats = 32.0;
    let mut state = ClipInteractionState::default();
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));

    let (press_status, press_message) = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Middle,
        )),
        bounds,
        mouse::Cursor::Available(Point::new(400.0, 20.0)),
    );
    assert_eq!(press_status, canvas::event::Status::Captured);
    assert!(press_message.is_none());

    let (drag_status, drag_message) = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: Point::new(420.0, 20.0),
        }),
        bounds,
        mouse::Cursor::Available(Point::new(420.0, 20.0)),
    );
    assert_eq!(drag_status, canvas::event::Status::Captured);
    assert!(matches!(
        drag_message,
        Some(Message::View(ViewMsg::ScrollArrangement(delta)))
            if (delta + 1.0).abs() < f64::EPSILON
    ));

    let (release_status, release_message) = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Middle,
        )),
        bounds,
        mouse::Cursor::Available(Point::new(420.0, 20.0)),
    );
    assert_eq!(release_status, canvas::event::Status::Captured);
    assert!(release_message.is_none());
    assert!(state.drag.is_none());
}

fn right_click(
    canvas: &TrackClipCanvas,
    position: Point,
) -> (canvas::event::Status, Option<Message>) {
    <TrackClipCanvas as canvas::Program<Message>>::update(
        canvas,
        &mut ClipInteractionState::default(),
        canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Right,
        )),
        Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0)),
        mouse::Cursor::Available(position),
    )
}

#[test]
fn physical_right_click_opens_clip_and_empty_arrange_context_menus() {
    let mut canvas = empty_track_canvas();
    let (status, message) = right_click(&canvas, Point::new(300.0, 20.0));
    assert_eq!(status, canvas::event::Status::Captured);
    assert!(matches!(
        message,
        Some(Message::View(ViewMsg::ShowContextMenu {
            target: ContextMenuTarget::ArrangementEmpty,
            ..
        }))
    ));

    let clip_id = ClipId::new();
    canvas.clips.push(TimelineClip {
        clip_id,
        position: 0,
        duration: 44_100,
        name: "Clip".into(),
        peaks: Arc::new(Vec::new()),
        peak_span_frames: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        fade_in_frames: 0,
        fade_out_frames: 0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        crossfade_in_from: None,
        crossfade_out_to: None,
        warp_stale: false,
    });
    let (status, message) = right_click(&canvas, Point::new(10.0, 10.0));
    assert_eq!(status, canvas::event::Status::Captured);
    assert!(matches!(
        message,
        Some(Message::View(ViewMsg::ShowContextMenu {
            target: ContextMenuTarget::Clip {
                clip_id: id,
                is_note_clip: false,
                ..
            },
            ..
        })) if id == clip_id
    ));
}

#[test]
fn per_track_canvas_ignores_global_track_creation_shortcuts() {
    let canvas = empty_track_canvas();
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));

    for modifiers in [Modifiers::CTRL, Modifiers::CTRL | Modifiers::SHIFT] {
        let event = canvas::Event::Keyboard(Event::KeyPressed {
            key: Key::Character("t".into()),
            modified_key: Key::Character("t".into()),
            physical_key: Physical::Code(Code::KeyT),
            location: Location::Standard,
            modifiers,
            text: None,
        });
        let mut state = ClipInteractionState::default();

        let (status, message) = <TrackClipCanvas as canvas::Program<Message>>::update(
            &canvas,
            &mut state,
            event,
            bounds,
            mouse::Cursor::Unavailable,
        );

        assert_eq!(status, canvas::event::Status::Ignored);
        assert!(message.is_none());
    }
}

#[test]
fn recording_preview_is_visible_but_not_hit_testable() {
    let preview_id = ClipId::new();
    let canvas = empty_track_canvas().with_recording_preview(TimelineNoteClip {
        clip_id: preview_id,
        position_beats: 0.0,
        duration_beats: 4.0,
        name: "● RECORDING LIVE".into(),
        notes: vec![(60, 0.0, 0.5)],
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
    });

    assert_eq!(
        canvas.recording_preview.as_ref().unwrap().clip_id,
        preview_id
    );
    assert!(canvas.hit_test(10.0).is_none());
}

#[test]
fn audio_recording_waveform_is_visible_but_not_hit_testable() {
    let preview_id = ClipId::new();
    let canvas = empty_track_canvas().with_audio_recording_preview(TimelineClip {
        clip_id: preview_id,
        position: 0,
        duration: 44_100,
        name: "● RECORDING INPUT".into(),
        peaks: Arc::new(vec![(-0.8, 0.6); 32]),
        peak_span_frames: Some(64),
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        fade_in_frames: 0,
        fade_out_frames: 0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        crossfade_in_from: None,
        crossfade_out_to: None,
        warp_stale: false,
    });

    assert_eq!(
        canvas.audio_recording_preview.as_ref().unwrap().clip_id,
        preview_id
    );
    assert!(canvas.hit_test(10.0).is_none());
}

#[test]
fn selected_audio_fade_handle_drag_emits_realtime_edits_in_one_undo_gesture() {
    let mut canvas = empty_track_canvas();
    let clip_id = ClipId::new();
    canvas.clips.push(TimelineClip {
        clip_id,
        position: 0,
        duration: 44_100,
        name: "Clip".into(),
        peaks: Arc::new(Vec::new()),
        peak_span_frames: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 44_100,
        fade_in_frames: 11_025,
        fade_out_frames: 0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        crossfade_in_from: None,
        crossfade_out_to: None,
        warp_stale: false,
    });
    canvas.selected_clips.insert(clip_id);
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
    let mut state = ClipInteractionState::default();
    let handle = Point::new(10.0, FADE_HANDLE_Y);

    let (status, message) = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(handle),
    );
    assert_eq!(status, canvas::event::Status::Captured);
    assert!(message.is_none());

    let drag = |state: &mut ClipInteractionState, x: f32| {
        <TrackClipCanvas as canvas::Program<Message>>::update(
            &canvas,
            state,
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved {
                position: Point::new(x, FADE_HANDLE_Y),
            }),
            bounds,
            mouse::Cursor::Available(Point::new(x, FADE_HANDLE_Y)),
        )
        .1
    };
    let first = drag(&mut state, 20.0);
    let second = drag(&mut state, 30.0);

    let extract = |message: Option<Message>| match message {
        Some(Message::UndoGesture { id, edit }) => match *edit {
            Message::Arrangement(ArrangementMsg::SetAudioClipFade {
                clip_id: edited,
                edge: crate::state::AudioClipFadeEdge::In,
                frames,
                ..
            }) => Some((id, edited, frames)),
            _ => None,
        },
        _ => None,
    };
    let (first_gesture, first_clip, first_frames) = extract(first).expect("first fade edit");
    let (second_gesture, second_clip, second_frames) = extract(second).expect("second fade edit");
    assert_eq!(first_gesture, second_gesture);
    assert_eq!((first_clip, second_clip), (clip_id, clip_id));
    assert_eq!((first_frames, second_frames), (22_050, 33_075));
}

#[test]
fn fade_drag_snaps_to_an_overlapping_neighbour_across_pointer_moves() {
    let mut canvas = empty_track_canvas();
    let outgoing_id = ClipId::new();
    let incoming_id = ClipId::new();
    let timeline_clip = |clip_id, position, name: &str| TimelineClip {
        clip_id,
        position,
        duration: 44_100,
        name: name.into(),
        peaks: Arc::new(Vec::new()),
        peak_span_frames: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 44_100,
        fade_in_frames: 0,
        fade_out_frames: 0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        crossfade_in_from: None,
        crossfade_out_to: None,
        warp_stale: false,
    };
    canvas.clips.push(timeline_clip(outgoing_id, 0, "Outgoing"));
    canvas
        .clips
        .push(timeline_clip(incoming_id, 33_075, "Incoming"));
    canvas.selected_clips.insert(outgoing_id);
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
    let outgoing_end_x = canvas.beat_to_x(2.0);
    let neighbour_x = canvas.beat_to_x(1.5);
    let mut state = ClipInteractionState::default();

    let pressed = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(Point::new(outgoing_end_x, FADE_HANDLE_Y)),
    )
    .0;
    assert_eq!(pressed, canvas::event::Status::Captured);

    for x in [
        neighbour_x - FADE_HANDLE_HIT_RADIUS,
        neighbour_x + FADE_HANDLE_HIT_RADIUS,
    ] {
        let message = <TrackClipCanvas as canvas::Program<Message>>::update(
            &canvas,
            &mut state,
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved {
                position: Point::new(x, FADE_HANDLE_Y),
            }),
            bounds,
            mouse::Cursor::Available(Point::new(x, FADE_HANDLE_Y)),
        )
        .1;
        assert!(matches!(
            message,
            Some(Message::UndoGesture { edit, .. }) if matches!(
                *edit,
                Message::Arrangement(ArrangementMsg::SetAudioClipFade {
                    clip_id,
                    edge: crate::state::AudioClipFadeEdge::Out,
                    frames: 11_025,
                    ..
                }) if clip_id == outgoing_id
            )
        ));
    }
}

/// The fade in length after dragging the fade in handle of a fresh 4 beat
/// clip to `to_beat` with `modifiers` held.
fn fade_in_frames_after_drag(to_beat: f64, modifiers: Modifiers) -> Option<u64> {
    let (canvas, _) = canvas_with_selected_audio_clip();
    let handle = Point::new(canvas.beat_to_x(0.0), FADE_HANDLE_Y);
    let target = Point::new(canvas.beat_to_x(to_beat), FADE_HANDLE_Y);
    let mut state = ClipInteractionState::default();
    <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Keyboard(Event::ModifiersChanged(modifiers)),
        Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0)),
        mouse::Cursor::Available(handle),
    );
    left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        handle,
    );
    match left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::CursorMoved { position: target },
        target,
    ) {
        Some(Message::UndoGesture { edit, .. }) => match *edit {
            Message::Arrangement(ArrangementMsg::SetAudioClipFade { frames, .. }) => Some(frames),
            _ => None,
        },
        _ => None,
    }
}

#[test]
fn a_fade_drag_ends_the_fade_on_the_grid_line_under_the_pointer() {
    let frames = fade_in_frames_after_drag(1.1, Modifiers::empty()).expect("a fade edit");

    assert_eq!(frames, 22_050);
}

#[test]
fn an_alt_fade_drag_follows_the_pointer_off_the_grid() {
    let frames = fade_in_frames_after_drag(1.1, Modifiers::ALT).expect("a fade edit");

    assert!(frames.abs_diff(24_255) <= 1, "the fade is {frames} frames");
}

#[test]
fn selected_audio_fade_curve_handle_drag_emits_vertical_curve_edits() {
    let mut canvas = empty_track_canvas();
    let clip_id = ClipId::new();
    canvas.clips.push(TimelineClip {
        clip_id,
        position: 0,
        duration: 44_100,
        name: "Clip".into(),
        peaks: Arc::new(Vec::new()),
        peak_span_frames: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 44_100,
        fade_in_frames: 22_050,
        fade_out_frames: 0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        crossfade_in_from: None,
        crossfade_out_to: None,
        warp_stale: false,
    });
    canvas.selected_clips.insert(clip_id);
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
    let clip_width = canvas.geometry().width_for_beats(2.0);
    let handle = fade_drag::fade_curve_handle(
        0.0,
        clip_width,
        bounds.height,
        &canvas.clips[0],
        crate::state::AudioClipFadeEdge::In,
    )
    .unwrap();
    let mut state = ClipInteractionState::default();

    let pressed = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(handle),
    )
    .0;
    assert_eq!(pressed, canvas::event::Status::Captured);

    let target = Point::new(handle.x, CLIP_Y + CLIP_TITLE_HEIGHT + 2.0);
    let message = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::CursorMoved { position: target }),
        bounds,
        mouse::Cursor::Available(target),
    )
    .1;

    assert!(matches!(
        message,
        Some(Message::UndoGesture { edit, .. }) if matches!(
            *edit,
            Message::Arrangement(ArrangementMsg::SetAudioClipFadeCurve {
                clip_id: edited,
                edge: crate::state::AudioClipFadeEdge::In,
                curve,
                ..
            }) if edited == clip_id && curve.percent() == 100
        )
    ));
}

#[test]
fn linked_crossfade_has_one_shared_curve_handle_and_edit() {
    let mut canvas = empty_track_canvas();
    let outgoing_id = ClipId::new();
    let incoming_id = ClipId::new();
    canvas.crossfades.push(TimelineCrossfade {
        outgoing_id,
        incoming_id,
        overlap_start: 33_075,
        overlap_end: 44_100,
        curve: Default::default(),
    });
    canvas.selected_clips.insert(outgoing_id);
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
    let handles: Vec<_> = canvas.crossfade_curve_handles(bounds.height).collect();
    assert_eq!(handles.len(), 1);
    let handle = handles[0].2;
    let mut state = ClipInteractionState::default();

    let pressed = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(handle),
    )
    .0;
    assert_eq!(pressed, canvas::event::Status::Captured);

    let target = Point::new(handle.x, CLIP_Y + CLIP_TITLE_HEIGHT + 2.0);
    let message = <TrackClipCanvas as canvas::Program<Message>>::update(
        &canvas,
        &mut state,
        canvas::Event::Mouse(iced::mouse::Event::CursorMoved { position: target }),
        bounds,
        mouse::Cursor::Available(target),
    )
    .1;
    assert!(matches!(
        message,
        Some(Message::UndoGesture { edit, .. }) if matches!(
            *edit,
            Message::Arrangement(ArrangementMsg::SetAudioClipCrossfadeCurve {
                outgoing_id: outgoing,
                incoming_id: incoming,
                curve,
                ..
            }) if outgoing == outgoing_id && incoming == incoming_id && curve.percent() == 100
        )
    ));
}

#[test]
fn overlapping_fade_controls_choose_the_handle_nearest_the_pointer() {
    let mut canvas = empty_track_canvas();
    let clip_id = ClipId::new();
    canvas.clips.push(TimelineClip {
        clip_id,
        position: 0,
        duration: 44_100,
        name: "Short steep fade".into(),
        peaks: Arc::new(Vec::new()),
        peak_span_frames: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 44_100,
        fade_in_frames: 11_025,
        fade_out_frames: 0,
        fade_in_curve: vibez_core::track::FadeCurve::new(100),
        fade_out_curve: Default::default(),
        crossfade_in_from: None,
        crossfade_out_to: None,
        warp_stale: false,
    });
    canvas.selected_clips.insert(clip_id);
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
    let clip_width = canvas.geometry().width_for_beats(2.0);
    let (length_x, _) = fade_drag::fade_handle_xs(0.0, clip_width, &canvas.clips[0]);
    let curve = fade_drag::fade_curve_handle(
        0.0,
        clip_width,
        bounds.height,
        &canvas.clips[0],
        crate::state::AudioClipFadeEdge::In,
    )
    .unwrap();
    let length = Point::new(length_x, FADE_HANDLE_Y);

    assert!(matches!(
        canvas.fade_control_hit(length, bounds.height),
        Some(fade_drag::FadeControlDrag::Length(_))
    ));
    assert!(matches!(
        canvas.fade_control_hit(curve, bounds.height),
        Some(fade_drag::FadeControlDrag::Curve(_))
    ));
}

fn canvas_with_selected_audio_clip() -> (TrackClipCanvas, ClipId) {
    let mut canvas = empty_track_canvas();
    let clip_id = ClipId::new();
    canvas.clips.push(TimelineClip {
        clip_id,
        position: 0,
        duration: 88_200,
        name: "Clip".into(),
        peaks: Arc::new(Vec::new()),
        peak_span_frames: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        fade_in_frames: 0,
        fade_out_frames: 0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        crossfade_in_from: None,
        crossfade_out_to: None,
        warp_stale: false,
    });
    canvas.selected_clips.insert(clip_id);
    (canvas, clip_id)
}

fn left_event(
    canvas: &TrackClipCanvas,
    state: &mut ClipInteractionState,
    event: iced::mouse::Event,
    position: Point,
) -> Option<Message> {
    <TrackClipCanvas as canvas::Program<Message>>::update(
        canvas,
        state,
        canvas::Event::Mouse(event),
        Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0)),
        mouse::Cursor::Available(position),
    )
    .1
}

#[test]
fn pressing_a_selected_clip_keeps_the_selection_and_dragging_moves_it_as_a_group() {
    let (canvas, clip_id) = canvas_with_selected_audio_clip();
    let start = Point::new(canvas.beat_to_x(1.0), CLIP_Y + 4.0);
    let mut state = ClipInteractionState::default();

    let pressed = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        start,
    );
    assert!(pressed.is_none(), "the press must not narrow the selection");

    let target = Point::new(canvas.beat_to_x(2.0), start.y);
    let moved = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::CursorMoved { position: target },
        target,
    );
    let Some(Message::UndoGesture { id: gesture, .. }) = moved else {
        panic!("expected a clip move, got {moved:?}");
    };
    assert!(matches!(
        drag_edit(moved.as_ref()),
        Some(ArrangementMsg::MoveSelectedClips {
            anchor,
            new_position_beats,
            target_track,
        }) if *anchor == clip_id
            && (new_position_beats - 1.0).abs() < 1e-9
            && *target_track == canvas.track_ids[0]
    ));

    let released = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
        target,
    );
    assert!(
        is_drop_of(released.as_ref(), clip_id, gesture),
        "a drag keeps the whole selection and drops it: {released:?}"
    );
}

/// `message` drops `clip_id` as the last step of undo gesture `gesture`.
fn is_drop_of(message: Option<&Message>, clip_id: ClipId, gesture: UndoGestureId) -> bool {
    matches!(
        message,
        Some(Message::UndoGesture { id, edit }) if *id == gesture && matches!(
            **edit,
            Message::Arrangement(ArrangementMsg::DropClips { anchor }) if anchor == clip_id
        )
    )
}

#[test]
fn releasing_a_clip_edge_drag_drops_the_clip_in_the_same_undo_gesture() {
    let (canvas, clip_id) = canvas_with_off_grid_audio_clip();
    for (edge_beat, inset, to_beat) in [(5.1, -2.0, 7.0), (1.1, 2.0, 0.5)] {
        let start = Point::new(canvas.beat_to_x(edge_beat) + inset, CLIP_FOOT_Y);
        let target = Point::new(canvas.beat_to_x(to_beat), start.y);
        let mut state = ClipInteractionState::default();

        left_event(
            &canvas,
            &mut state,
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            start,
        );
        let dragged = left_event(
            &canvas,
            &mut state,
            iced::mouse::Event::CursorMoved { position: target },
            target,
        );
        let Some(Message::UndoGesture { id: gesture, .. }) = dragged else {
            panic!("expected an edge drag, got {dragged:?}");
        };
        let released = left_event(
            &canvas,
            &mut state,
            iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
            target,
        );

        assert!(
            is_drop_of(released.as_ref(), clip_id, gesture),
            "edge at beat {edge_beat}: {released:?}"
        );
    }
}

#[test]
fn clicking_a_selected_clip_without_dragging_narrows_the_selection_to_it() {
    let (canvas, clip_id) = canvas_with_selected_audio_clip();
    let start = Point::new(canvas.beat_to_x(1.0), CLIP_Y + 4.0);
    let mut state = ClipInteractionState::default();

    left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        start,
    );
    let released = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
        start,
    );

    assert!(matches!(
        released,
        Some(Message::Arrangement(ArrangementMsg::SelectArrangementClip {
            selection: crate::state::ArrangementSelection::AudioClip { clip_id: id, .. },
            shift_held: false,
        })) if id == clip_id
    ));
}

/// The selected audio Clip moved off the grid, starting at beat 1.1.
fn canvas_with_off_grid_audio_clip() -> (TrackClipCanvas, ClipId) {
    let (mut canvas, clip_id) = canvas_with_selected_audio_clip();
    canvas.clips[0].position = (1.1 * canvas.spb()).round() as u64;
    (canvas, clip_id)
}

fn is_clip_edit(message: &Message) -> bool {
    matches!(
        drag_edit(Some(message)),
        Some(ArrangementMsg::MoveSelectedClips { .. } | ArrangementMsg::ResizeSelectedClips { .. })
    )
}

/// The Clip edit a pointer move carries, under its undo gesture and drag
/// preview.
fn drag_edit(message: Option<&Message>) -> Option<&ArrangementMsg> {
    match message? {
        Message::UndoGesture { edit, .. } => drag_edit(Some(edit)),
        Message::Arrangement(ArrangementMsg::DragClips { edit, .. }) => Some(edit),
        Message::Arrangement(edit) => Some(edit),
        _ => None,
    }
}

#[test]
fn a_click_with_a_shaky_hand_never_moves_an_off_grid_clip() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();
    let start = Point::new(canvas.beat_to_x(1.5), CLIP_Y + 4.0);
    let jitter = Point::new(start.x + 3.0, start.y + 1.0);
    let mut state = ClipInteractionState::default();

    left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        start,
    );
    let moved = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::CursorMoved { position: jitter },
        jitter,
    );
    let released = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
        jitter,
    );

    assert!(
        !moved.as_ref().is_some_and(is_clip_edit),
        "3 px of travel is a click: {moved:?}"
    );
    assert!(!released.as_ref().is_some_and(is_clip_edit), "{released:?}");
}

#[test]
fn dragging_an_off_grid_clip_moves_it_by_whole_grid_steps_keeping_its_offset() {
    let (canvas, clip_id) = canvas_with_off_grid_audio_clip();
    let start = Point::new(canvas.beat_to_x(1.5), CLIP_Y + 4.0);
    let mut state = ClipInteractionState::default();

    left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        start,
    );
    // 1.1 beats of travel on the 1/8 grid is two grid steps.
    let target = Point::new(canvas.beat_to_x(2.6), start.y);
    let moved = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::CursorMoved { position: target },
        target,
    );

    match drag_edit(moved.as_ref()) {
        Some(ArrangementMsg::MoveSelectedClips {
            anchor,
            new_position_beats,
            ..
        }) => {
            assert_eq!(*anchor, clip_id);
            assert!(
                (new_position_beats - 2.1).abs() < 1e-4,
                "moved to {new_position_beats}"
            );
        }
        other => panic!("expected a clip move, got {other:?}"),
    }
}

/// Presses `canvas`'s first Clip on its right edge and drags the pointer to
/// `to_beat`, returning the length the last pointer move asked for.
fn resize_length_after_drag(canvas: &TrackClipCanvas, to_beat: f64) -> Option<f64> {
    resize_length_after_modified_drag(canvas, to_beat, Modifiers::empty())
}

fn resize_length_after_modified_drag(
    canvas: &TrackClipCanvas,
    to_beat: f64,
    modifiers: Modifiers,
) -> Option<f64> {
    match drag_edit(end_drag_message(canvas, to_beat, modifiers).as_ref()) {
        Some(ArrangementMsg::ResizeSelectedClips {
            new_duration_beats, ..
        }) => Some(*new_duration_beats),
        _ => None,
    }
}

/// Presses `canvas`'s first Clip on its right edge with `modifiers` held and
/// drags the pointer to `to_beat`, returning what the pointer move sent.
fn end_drag_message(
    canvas: &TrackClipCanvas,
    to_beat: f64,
    modifiers: Modifiers,
) -> Option<Message> {
    let clip = &canvas.clips[0];
    let end_beat = (clip.position + clip.duration) as f64 / canvas.spb();
    let start = Point::new(canvas.beat_to_x(end_beat) - 2.0, CLIP_Y + 4.0);
    let target = Point::new(canvas.beat_to_x(to_beat), start.y);
    let mut state = ClipInteractionState::default();
    <TrackClipCanvas as canvas::Program<Message>>::update(
        canvas,
        &mut state,
        canvas::Event::Keyboard(Event::ModifiersChanged(modifiers)),
        Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0)),
        mouse::Cursor::Available(start),
    );

    left_event(
        canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        start,
    );
    left_event(
        canvas,
        &mut state,
        iced::mouse::Event::CursorMoved { position: target },
        target,
    )
}

#[test]
fn resizing_an_off_grid_clip_ends_it_on_the_grid_line_under_the_pointer() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();

    let length = resize_length_after_drag(&canvas, 6.0).expect("a resize");

    assert!(
        (1.1 + length - 6.0).abs() < 1e-4,
        "ended at beat {}",
        1.1 + length
    );
}

#[test]
fn a_clip_resized_at_fit_zoom_can_shrink_below_the_coarse_adaptive_grid() {
    let (mut canvas, _) = canvas_with_off_grid_audio_clip();
    canvas.clips[0].duration = (64.0 * canvas.spb()) as u64;
    canvas.zoom_level = 0.02;
    canvas.grid = GridConfig::new(crate::state::SnapGrid::EIGHTH, true, true, 0);
    assert_eq!(
        canvas.grid.effective_grid(canvas.pixels_per_beat()),
        crate::state::SnapGrid::EIGHT_BARS
    );

    let length = resize_length_after_drag(&canvas, 10.0).expect("a resize");

    assert!(
        (length - crate::state::SHORTEST_LENGTH_BEATS).abs() < 1e-9,
        "shrank to {length} beats"
    );
}

#[test]
fn a_shaky_click_on_an_off_grid_clip_end_never_resizes_it() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();
    let start = Point::new(canvas.beat_to_x(5.1) - 2.0, CLIP_Y + 4.0);
    let jitter = Point::new(start.x + 3.0, start.y + 1.0);
    let mut state = ClipInteractionState::default();

    left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        start,
    );
    let moved = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::CursorMoved { position: jitter },
        jitter,
    );
    let released = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
        jitter,
    );

    assert!(
        !moved.as_ref().is_some_and(is_clip_edit),
        "3 px of travel is a click: {moved:?}"
    );
    assert!(!released.as_ref().is_some_and(is_clip_edit), "{released:?}");
}

#[test]
fn one_clip_edit_key_press_emits_one_message_whatever_the_lane_count() {
    let lanes: Vec<_> = (0..3)
        .map(|_| canvas_with_selected_audio_clip().0)
        .collect();
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
    let command = if crate::app::ON_MACOS {
        Modifiers::LOGO
    } else {
        Modifiers::CTRL
    };

    for (letter, code) in [("d", Code::KeyD), ("e", Code::KeyE), ("j", Code::KeyJ)] {
        let event = canvas::Event::Keyboard(Event::KeyPressed {
            key: Key::Character(letter.into()),
            modified_key: Key::Character(letter.into()),
            physical_key: Physical::Code(code),
            location: Location::Standard,
            modifiers: command,
            text: None,
        });
        let from_lanes = lanes
            .iter()
            .filter_map(|lane| {
                <TrackClipCanvas as canvas::Program<Message>>::update(
                    lane,
                    &mut ClipInteractionState::default(),
                    event.clone(),
                    bounds,
                    mouse::Cursor::Unavailable,
                )
                .1
            })
            .count();
        let from_global =
            crate::app::global_key_handler(Key::Character(letter.into()), command).is_some();

        assert_eq!(
            from_lanes + usize::from(from_global),
            1,
            "Command+{letter} must reach the timeline exactly once"
        );
    }
}

/// Clicks `canvas` at `beat` and `y` with `modifiers` held, returning the
/// beat the release seeks to.
fn seek_beat_after_click(
    canvas: &TrackClipCanvas,
    beat: f64,
    y: f32,
    modifiers: Modifiers,
) -> Option<f64> {
    let position = Point::new(canvas.beat_to_x(beat), y);
    let mut state = ClipInteractionState::default();
    <TrackClipCanvas as canvas::Program<Message>>::update(
        canvas,
        &mut state,
        canvas::Event::Keyboard(Event::ModifiersChanged(modifiers)),
        Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0)),
        mouse::Cursor::Available(position),
    );
    left_event(
        canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        position,
    );
    match left_event(
        canvas,
        &mut state,
        iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
        position,
    ) {
        Some(Message::Transport(TransportMsg::SeekToBeat(beat))) => Some(beat),
        _ => None,
    }
}

const CLIP_BODY_Y: f32 = CLIP_Y + CLIP_TITLE_HEIGHT + 10.0;

#[test]
fn clicking_a_clip_body_seeks_to_the_nearest_grid_line() {
    let (canvas, _) = canvas_with_selected_audio_clip();

    let beat = seek_beat_after_click(&canvas, 1.3, CLIP_BODY_Y, Modifiers::empty());

    assert_eq!(beat, Some(1.5));
}

#[test]
fn clicking_an_empty_lane_seeks_to_the_nearest_grid_line() {
    let canvas = empty_track_canvas();

    let beat = seek_beat_after_click(&canvas, 6.8, CLIP_BODY_Y, Modifiers::empty());

    assert_eq!(beat, Some(7.0));
}

#[test]
fn an_alt_click_on_a_clip_body_seeks_exactly_where_clicked() {
    let (canvas, _) = canvas_with_selected_audio_clip();

    let beat = seek_beat_after_click(&canvas, 1.3, CLIP_BODY_Y, Modifiers::ALT).expect("a seek");

    assert!((beat - 1.3).abs() < 1e-3, "seeked to {beat}");
}

/// The off-grid Clip at 1.1..5.1, with another lane holding a note Clip
/// whose start sits off the grid at beat 9.3.
fn canvas_beside_an_off_grid_clip() -> (TrackClipCanvas, ClipId) {
    let (canvas, clip_id) = canvas_with_off_grid_audio_clip();
    let mut other = note_clip(9.3);
    other.duration_beats = 4.0;
    let mut editor = crate::state::TimelineEditorState::default();
    Arc::make_mut(&mut editor.timeline)
        .ensure(TrackId::new())
        .note_clips
        .push(other);
    let targets = crate::timeline_snap::SnapTargets::for_editor(&editor, canvas.spb());
    (canvas.with_snap_targets(Arc::new(targets)), clip_id)
}

/// Presses `canvas`'s first Clip title at beat 1.5 with `modifiers` held
/// and drags it `travel` beats, returning where the move puts its start.
fn moved_start_after_drag(
    canvas: &TrackClipCanvas,
    travel: f64,
    modifiers: Modifiers,
) -> Option<f64> {
    match drag_edit(title_drag_message(canvas, travel, modifiers).as_ref()) {
        Some(ArrangementMsg::MoveSelectedClips {
            new_position_beats, ..
        }) => Some(*new_position_beats),
        _ => None,
    }
}

/// Presses `canvas`'s first Clip title at beat 1.5 with `modifiers` held
/// and drags it `travel` beats, returning what the pointer move sent.
fn title_drag_message(
    canvas: &TrackClipCanvas,
    travel: f64,
    modifiers: Modifiers,
) -> Option<Message> {
    let start = Point::new(canvas.beat_to_x(1.5), CLIP_Y + 4.0);
    let target = Point::new(canvas.beat_to_x(1.5 + travel), start.y);
    let mut state = ClipInteractionState::default();
    <TrackClipCanvas as canvas::Program<Message>>::update(
        canvas,
        &mut state,
        canvas::Event::Keyboard(Event::ModifiersChanged(modifiers)),
        Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0)),
        mouse::Cursor::Available(start),
    );
    left_event(
        canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        start,
    );
    left_event(
        canvas,
        &mut state,
        iced::mouse::Event::CursorMoved { position: target },
        target,
    )
}

/// The preview step a drag's pointer move carries.
fn drag_step(message: Option<Message>) -> Option<crate::state::ClipDragStep> {
    match message? {
        Message::UndoGesture { edit, .. } => drag_step(Some(*edit)),
        Message::Arrangement(ArrangementMsg::DragClips { step, .. }) => Some(step),
        _ => None,
    }
}

#[test]
fn a_clip_move_step_reports_the_clip_edge_it_locked_onto() {
    let (canvas, clip_id) = canvas_beside_an_off_grid_clip();

    let step = drag_step(title_drag_message(&canvas, 4.15, Modifiers::empty())).expect("a step");

    assert_eq!(step.kind, crate::state::ClipDragKind::Move);
    assert_eq!((step.anchor, step.origin_track), (clip_id, canvas.track_id));
    assert!((step.origin.start_beats - 1.1).abs() < 1e-4);
    let guide = step.guide.expect("a snapped move locks onto a line");
    assert_eq!(guide.kind, crate::timeline_snap::SnapTargetKind::ClipEdge);
    assert!((guide.beat - 9.3).abs() < 1e-9, "guide at {}", guide.beat);
}

#[test]
fn an_alt_clip_move_step_shows_no_guide() {
    let (canvas, _) = canvas_beside_an_off_grid_clip();

    let step = drag_step(title_drag_message(&canvas, 4.15, Modifiers::ALT)).expect("a step");

    assert_eq!(step.guide, None);
}

#[test]
fn a_clip_end_step_reports_the_grid_line_it_locked_onto() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();

    let step = drag_step(end_drag_message(&canvas, 7.05, Modifiers::empty())).expect("a step");

    assert_eq!(step.kind, crate::state::ClipDragKind::End);
    assert_eq!(
        step.guide,
        Some(crate::timeline_snap::SnapTarget {
            beat: 7.0,
            kind: crate::timeline_snap::SnapTargetKind::Grid,
        })
    );
}

#[test]
fn dragging_a_clip_locks_its_end_onto_another_lanes_off_grid_clip_start() {
    let (canvas, _) = canvas_beside_an_off_grid_clip();

    // 4.15 beats of travel ends the 4 beat Clip at 9.25, just short of the
    // other Clip's start, where the grid alone would end it at 9.1.
    let start = moved_start_after_drag(&canvas, 4.15, Modifiers::empty()).expect("a move");

    assert!((start + 4.0 - 9.3).abs() < 1e-4, "ended at {}", start + 4.0);
}

#[test]
fn resizing_a_clip_locks_its_end_onto_another_lanes_off_grid_clip_start() {
    let (canvas, _) = canvas_beside_an_off_grid_clip();

    let length = resize_length_after_drag(&canvas, 9.2).expect("a resize");

    assert!(
        (1.1 + length - 9.3).abs() < 1e-4,
        "ended at {}",
        1.1 + length
    );
}

#[test]
fn clicking_near_another_lanes_clip_start_seeks_onto_it() {
    let (canvas, _) = canvas_beside_an_off_grid_clip();

    let beat =
        seek_beat_after_click(&canvas, 9.2, CLIP_BODY_Y, Modifiers::empty()).expect("a seek");

    assert!((beat - 9.3).abs() < 1e-9, "seeked to {beat}");
}

#[test]
fn an_alt_drag_moves_a_clip_exactly_with_the_pointer() {
    let (canvas, _) = canvas_beside_an_off_grid_clip();

    let start = moved_start_after_drag(&canvas, 4.15, Modifiers::ALT).expect("a move");

    assert!((start - 5.25).abs() < 1e-3, "moved to {start}");
}

#[test]
fn an_alt_resize_ends_a_clip_exactly_under_the_pointer() {
    let (canvas, _) = canvas_beside_an_off_grid_clip();

    let length = resize_length_after_modified_drag(&canvas, 9.2, Modifiers::ALT).expect("a resize");

    assert!(
        (1.1 + length - 9.2).abs() < 1e-3,
        "ended at {}",
        1.1 + length
    );
}

/// Low in a Clip's body, clear of its fade handles.
const CLIP_FOOT_Y: f32 = 60.0;

/// Presses `canvas`'s first Clip on its start edge at `y`, drags the pointer
/// to `to_beat` and returns the start the last pointer move asked for.
fn trimmed_start_after_drag(
    canvas: &TrackClipCanvas,
    y: f32,
    to_beat: f64,
    modifiers: Modifiers,
) -> Option<f64> {
    let clip = &canvas.clips[0];
    let start = Point::new(
        canvas.beat_to_x(clip.position as f64 / canvas.spb()) + 2.0,
        y,
    );
    let target = Point::new(canvas.beat_to_x(to_beat), start.y);
    let mut state = ClipInteractionState::default();
    <TrackClipCanvas as canvas::Program<Message>>::update(
        canvas,
        &mut state,
        canvas::Event::Keyboard(Event::ModifiersChanged(modifiers)),
        Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0)),
        mouse::Cursor::Available(start),
    );
    left_event(
        canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        start,
    );
    let moved = left_event(
        canvas,
        &mut state,
        iced::mouse::Event::CursorMoved { position: target },
        target,
    );
    match drag_edit(moved.as_ref()) {
        Some(ArrangementMsg::TrimSelectedClipsStart {
            new_start_beats, ..
        }) => Some(*new_start_beats),
        _ => None,
    }
}

#[test]
fn trimming_an_off_grid_clip_starts_it_on_the_grid_line_under_the_pointer() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();

    let start =
        trimmed_start_after_drag(&canvas, CLIP_Y + 4.0, 2.05, Modifiers::empty()).expect("a trim");

    assert!((start - 2.0).abs() < 1e-9, "started at {start}");
}

#[test]
fn a_clip_start_trims_from_anywhere_down_its_edge() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();

    let start =
        trimmed_start_after_drag(&canvas, CLIP_FOOT_Y, 0.5, Modifiers::empty()).expect("a trim");

    assert!((start - 0.5).abs() < 1e-9, "started at {start}");
}

#[test]
fn a_clip_end_resizes_from_anywhere_down_its_edge() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();
    let end = Point::new(canvas.beat_to_x(5.1) - 2.0, CLIP_FOOT_Y);
    let mut state = ClipInteractionState::default();

    left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        end,
    );
    let target = Point::new(canvas.beat_to_x(6.0), end.y);
    let moved = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::CursorMoved { position: target },
        target,
    );

    assert!(
        matches!(
            drag_edit(moved.as_ref()),
            Some(ArrangementMsg::ResizeSelectedClips { .. })
        ),
        "{moved:?}"
    );
}

#[test]
fn an_alt_trim_starts_a_clip_exactly_under_the_pointer() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();

    let start =
        trimmed_start_after_drag(&canvas, CLIP_Y + 4.0, 2.05, Modifiers::ALT).expect("a trim");

    assert!((start - 2.05).abs() < 1e-3, "started at {start}");
}

#[test]
fn a_trim_past_a_clips_end_leaves_its_shortest_length() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();
    let shortest = crate::state::SHORTEST_LENGTH_BEATS;

    let start =
        trimmed_start_after_drag(&canvas, CLIP_Y + 4.0, 7.0, Modifiers::empty()).expect("a trim");

    assert!(
        (start - (5.1 - shortest)).abs() < 1e-6,
        "started at {start}"
    );
}

#[test]
fn a_shaky_click_on_a_clip_start_never_trims_it() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();
    let start = Point::new(canvas.beat_to_x(1.1) + 2.0, CLIP_Y + 4.0);
    let jitter = Point::new(start.x + 3.0, start.y + 1.0);
    let mut state = ClipInteractionState::default();

    left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        start,
    );
    let moved = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::CursorMoved { position: jitter },
        jitter,
    );

    assert!(
        !matches!(
            drag_edit(moved.as_ref()),
            Some(ArrangementMsg::TrimSelectedClipsStart { .. })
        ),
        "3 px of travel is a click: {moved:?}"
    );
}

#[test]
fn the_pointer_shows_a_resize_cursor_on_either_clip_edge() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
    let cursor_at = |beat: f64, dx: f32| {
        <TrackClipCanvas as canvas::Program<Message>>::mouse_interaction(
            &canvas,
            &ClipInteractionState::default(),
            bounds,
            mouse::Cursor::Available(Point::new(canvas.beat_to_x(beat) + dx, CLIP_FOOT_Y)),
        )
    };

    assert_eq!(
        cursor_at(1.1, 2.0),
        mouse::Interaction::ResizingHorizontally
    );
    assert_eq!(
        cursor_at(5.1, -2.0),
        mouse::Interaction::ResizingHorizontally
    );
    assert_eq!(cursor_at(3.0, 0.0), mouse::Interaction::Text);
}

#[test]
fn the_pointer_is_an_i_beam_over_a_clip_body_and_an_arrow_on_an_empty_lane() {
    let (canvas, _) = canvas_with_off_grid_audio_clip();
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
    let cursor_at = |beat: f64, y: f32| {
        <TrackClipCanvas as canvas::Program<Message>>::mouse_interaction(
            &canvas,
            &ClipInteractionState::default(),
            bounds,
            mouse::Cursor::Available(Point::new(canvas.beat_to_x(beat), y)),
        )
    };

    assert_eq!(cursor_at(3.0, CLIP_BODY_Y), mouse::Interaction::Text);
    assert_eq!(cursor_at(3.0, CLIP_Y + 4.0), mouse::Interaction::Grab);
    assert_eq!(cursor_at(8.0, CLIP_BODY_Y), mouse::Interaction::Idle);
}

/// A three bar Clip at beat 4, unselected, at the zoom where it is 16 px
/// wide, as at fit zoom on a real song.
fn canvas_with_a_short_clip_at_fit_zoom() -> TrackClipCanvas {
    let (mut canvas, clip_id) = canvas_with_selected_audio_clip();
    canvas.selected_clips.remove(&clip_id);
    canvas.zoom_level = 1.0 / 15.0;
    canvas.clips[0].position = (4.0 * canvas.spb()) as u64;
    canvas.clips[0].duration = (12.0 * canvas.spb()) as u64;
    canvas
}

#[test]
fn a_short_clip_at_fit_zoom_moves_when_dragged_near_its_start() {
    let canvas = canvas_with_a_short_clip_at_fit_zoom();
    let start = Point::new(canvas.beat_to_x(4.0) + 5.0, CLIP_Y + 4.0);
    let target = Point::new(start.x + 20.0, start.y);
    let mut state = ClipInteractionState::default();

    left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        start,
    );
    let dragged = left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::CursorMoved { position: target },
        target,
    );

    assert!(
        matches!(
            drag_edit(dragged.as_ref()),
            Some(ArrangementMsg::MoveSelectedClips { .. })
        ),
        "{dragged:?}"
    );
}

#[test]
fn a_short_clip_at_fit_zoom_shows_the_grab_cursor_near_its_start() {
    let canvas = canvas_with_a_short_clip_at_fit_zoom();

    let cursor = <TrackClipCanvas as canvas::Program<Message>>::mouse_interaction(
        &canvas,
        &ClipInteractionState::default(),
        Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0)),
        mouse::Cursor::Available(Point::new(canvas.beat_to_x(4.0) + 5.0, CLIP_Y + 4.0)),
    );

    assert_eq!(cursor, mouse::Interaction::Grab);
}

#[test]
fn a_clips_end_zone_stops_at_its_drawn_right_edge() {
    let (canvas, clip_id) = canvas_with_selected_audio_clip();
    let end_x = canvas.beat_to_x(canvas.clips[0].duration as f64 / canvas.spb());

    let inside = canvas.hit_test(end_x - 1.5).expect("the clip");

    assert_eq!(
        (inside.clip_id, inside.edge),
        (
            clip_id,
            Some(crate::widgets::timeline::clip_drag::ClipEdge::End)
        )
    );
    assert!(
        canvas.hit_test(end_x - 0.5).is_none(),
        "the gap after the clip is lane"
    );
}

#[test]
fn pressing_where_clips_overlap_picks_the_one_drawn_on_top() {
    let (mut canvas, under_id) = canvas_with_selected_audio_clip();
    canvas.selected_clips.clear();
    canvas.clips[0].duration = (8.0 * canvas.spb()) as u64;
    let (other, on_top_id) = canvas_with_selected_audio_clip();
    let mut on_top = other.clips.into_iter().next().expect("a clip");
    on_top.position = (4.0 * canvas.spb()) as u64;
    canvas.clips.push(on_top);
    let press = Point::new(canvas.beat_to_x(6.0), CLIP_Y + 4.0);

    let pressed = left_event(
        &canvas,
        &mut ClipInteractionState::default(),
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        press,
    );

    match pressed {
        Some(Message::Arrangement(ArrangementMsg::SelectArrangementClip {
            selection: crate::state::ArrangementSelection::AudioClip { clip_id, .. },
            ..
        })) => assert_eq!(
            clip_id, on_top_id,
            "picked the clip underneath, {under_id:?}"
        ),
        other => panic!("expected a clip selection, got {other:?}"),
    }
}

#[test]
fn a_clip_dragged_down_a_tall_lane_changes_track_only_past_its_bottom_edge() {
    let (mut canvas, clip_id) = canvas_with_selected_audio_clip();
    let below = TrackId::new();
    canvas.track_ids = vec![canvas.track_id, below];
    canvas.row_spans = crate::widgets::timeline::build_row_spans([
        (canvas.track_id, 210.0, 0.0),
        (below, 52.0, 0.0),
    ]);
    let x = canvas.beat_to_x(1.0);
    let mut state = ClipInteractionState::default();
    left_event(
        &canvas,
        &mut state,
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        Point::new(x, CLIP_Y + 4.0),
    );

    let target_at = |state: &mut ClipInteractionState, y: f32| {
        let point = Point::new(x, y);
        let message = left_event(
            &canvas,
            state,
            iced::mouse::Event::CursorMoved { position: point },
            point,
        );
        match drag_edit(message.as_ref()) {
            Some(ArrangementMsg::MoveSelectedClips {
                anchor,
                target_track,
                ..
            }) if *anchor == clip_id => *target_track,
            other => panic!("expected a clip move, got {other:?}"),
        }
    };

    assert_eq!(target_at(&mut state, 209.0), canvas.track_id);
    assert_eq!(target_at(&mut state, 210.0), below);
}
