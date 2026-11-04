use iced::Point;

use super::*;
use crate::state::{ClipDragKind, ClipDragStep, ClipSpan, UndoGestureId};
use crate::timeline_snap::{SnapTarget, SnapTargetKind};

const CTX: ArrangementCtx = ArrangementCtx {
    samples_per_beat: 100.0,
    playhead_samples: 0,
    playhead_beats: 0.0,
};

const GUIDE: SnapTarget = SnapTarget {
    beat: 8.0,
    kind: SnapTargetKind::ClipEdge,
};

/// A step of gesture `gesture` dragging `anchor`, which began at beats
/// 1..9 on `origin_track`.
fn step(
    gesture: UndoGestureId,
    kind: ClipDragKind,
    anchor: ClipId,
    origin_track: TrackId,
) -> ClipDragStep {
    ClipDragStep {
        gesture,
        kind,
        anchor,
        origin_track,
        origin: ClipSpan {
            start_beats: 1.0,
            length_beats: 8.0,
        },
        guide: Some(GUIDE),
        pointer: Point::ORIGIN,
    }
}

fn drag(a: &mut ArrangementFixture, step: ClipDragStep, edit: ArrangementMsg) {
    a.update(
        ArrangementMsg::DragClips {
            step,
            edit: Box::new(edit),
        },
        &mut RecordingEngine::default(),
        CTX,
    );
}

fn readout(a: &ArrangementFixture) -> &str {
    &a.clip_drag
        .as_ref()
        .expect("a drag in flight")
        .preview
        .readout
}

#[test]
fn a_move_step_reads_where_the_clip_lands_and_the_lane_it_changes_to() {
    let mut a = arrangement_with_tracks(2);
    let (t0, clip) = add_audio_clip(&mut a, 0, 100, 800);
    let t1 = a.tracks[1].id;
    let lane = a.tracks[1].name.clone();
    let gesture = UndoGestureId::new();
    let moved = |target_track| ArrangementMsg::MoveSelectedClips {
        anchor: clip,
        new_position_beats: 8.0,
        target_track,
    };

    drag(
        &mut a,
        step(gesture, ClipDragKind::Move, clip, t0),
        moved(t0),
    );
    assert_eq!(readout(&a), "3.1.1");
    assert_eq!(
        a.clip_drag.as_ref().unwrap().preview.step.guide,
        Some(GUIDE),
        "the preview reports the line the drag locked onto"
    );

    drag(
        &mut a,
        step(gesture, ClipDragKind::Move, clip, t0),
        moved(t1),
    );
    assert_eq!(readout(&a), format!("3.1.1 to {lane}"));
}

#[test]
fn edge_steps_read_the_new_start_or_the_new_length() {
    let mut a = arrangement_with_tracks(1);
    let (t0, clip) = add_audio_clip(&mut a, 0, 100, 800);
    let anchor = ArrangementSelection::AudioClip {
        track_id: t0,
        clip_id: clip,
    };

    drag(
        &mut a,
        step(UndoGestureId::new(), ClipDragKind::Start, clip, t0),
        ArrangementMsg::TrimSelectedClipsStart {
            anchor,
            new_start_beats: 1.25,
        },
    );
    assert_eq!(readout(&a), "Start 1.2.2");

    drag(
        &mut a,
        step(UndoGestureId::new(), ClipDragKind::End, clip, t0),
        ArrangementMsg::ResizeSelectedClips {
            anchor,
            new_duration_beats: 6.5,
        },
    );
    assert_eq!(readout(&a), "Length 1.2.2");
}

#[test]
fn dropping_the_clips_ends_the_preview() {
    let mut a = arrangement_with_tracks(1);
    let (t0, clip) = add_audio_clip(&mut a, 0, 100, 800);
    drag(
        &mut a,
        step(UndoGestureId::new(), ClipDragKind::Move, clip, t0),
        ArrangementMsg::MoveSelectedClips {
            anchor: clip,
            new_position_beats: 4.0,
            target_track: t0,
        },
    );

    a.update(
        ArrangementMsg::DropClips { anchor: clip },
        &mut RecordingEngine::default(),
        CTX,
    );

    assert!(a.clip_drag.is_none());
}

#[test]
fn cancelling_a_drag_puts_back_the_selection_it_began_with() {
    let mut a = arrangement_with_tracks(2);
    let (t0, clip) = add_audio_clip(&mut a, 0, 100, 800);
    let t1 = a.tracks[1].id;
    let before = ArrangementSelection::AudioClip {
        track_id: t0,
        clip_id: clip,
    };
    a.selected_clips.insert(before);
    a.selected_track = Some(t0);
    let gesture = UndoGestureId::new();
    drag(
        &mut a,
        step(gesture, ClipDragKind::Move, clip, t0),
        ArrangementMsg::MoveSelectedClips {
            anchor: clip,
            new_position_beats: 1.0,
            target_track: t1,
        },
    );
    assert_eq!(a.selected_track, Some(t1));

    assert_eq!(a.arrangement.editor.cancel_clip_drag(), Some(gesture));

    assert!(a.clip_drag.is_none());
    assert_eq!(a.selected_clips, HashSet::from([before]));
    assert_eq!(a.selected_track, Some(t0));
}
