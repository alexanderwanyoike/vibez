//! Tests for the clip lane canvas.
//!
//! Split out of `clips.rs` to keep that file inside the project's
//! 1,000-line ceiling.

use std::collections::HashSet;

use iced::keyboard::key::{Code, Physical};
use iced::keyboard::{Event, Key, Location, Modifiers};
use iced::widget::canvas;
use iced::{mouse, Color, Point, Rectangle, Size};

use crate::domains::view::ViewMsg;
use crate::message::Message;
use crate::state::{ContextMenuTarget, GridConfig, ProjectTrack, TrackTimelineContent};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;

use super::*;

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
        0,
        1,
        vec![track_id],
        vec![false],
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
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: GrooveGrid::Off,
    }
}

#[test]
fn zoomed_out_note_clips_skip_individual_note_geometry() {
    let mut content = TrackTimelineContent::default();
    content.note_clips.push(note_clip(0.0));

    let canvas = track_canvas(&content, 0.01);

    assert_eq!(canvas.note_clips.len(), 1);
    assert!(canvas.note_clips[0].notes.is_empty());
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
        bpm: 120.0,
        zoom_level: 3.0,
        grid: GridConfig::new(crate::state::SnapGrid::EIGHTH, true, false, 0),
        scroll_offset_beats: 9.5,
        total_beats: 64.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 64.0,
        time_selection_active: false,
        selection_start_beats: 0.0,
        selection_end_beats: 0.0,
    };

    assert_eq!(ruler.beat_to_x(13.25, 800.0), lane.beat_to_x(13.25));
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
        warp_stale: false,
    });

    assert_eq!(
        canvas.audio_recording_preview.as_ref().unwrap().clip_id,
        preview_id
    );
    assert!(canvas.hit_test(10.0).is_none());
}
