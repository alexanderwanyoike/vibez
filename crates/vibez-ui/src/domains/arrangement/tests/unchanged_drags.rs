//! Clip moves and resizes that end where they began change nothing.

use super::*;

const CTX: ArrangementCtx = ArrangementCtx {
    samples_per_beat: 100.0,
    playhead_samples: 0,
    playhead_beats: 0.0,
};

#[test]
fn moving_a_clip_to_where_it_already_is_does_nothing() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 150, 100);
    let mut engine = RecordingEngine::default();

    let action = a.update(
        ArrangementMsg::MoveSelectedClips {
            anchor: clip_id,
            new_position_beats: 1.5,
            target_track: track_id,
        },
        &mut engine,
        CTX,
    );

    assert_eq!(action, ArrangementAction::default());
    assert!(engine.0.is_empty(), "{} engine commands", engine.0.len());
}

#[test]
fn moving_a_clip_reports_the_project_edit() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 150, 100);

    let action = a.update(
        ArrangementMsg::MoveSelectedClips {
            anchor: clip_id,
            new_position_beats: 2.5,
            target_track: track_id,
        },
        &mut RecordingEngine::default(),
        CTX,
    );

    assert!(action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].position, 250);
}

#[test]
fn resizing_a_clip_to_its_own_length_does_nothing() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 150, 100);
    let mut engine = RecordingEngine::default();

    let action = a.update(
        ArrangementMsg::ResizeSelectedClips {
            anchor: ArrangementSelection::AudioClip { track_id, clip_id },
            new_duration_beats: 1.0,
        },
        &mut engine,
        CTX,
    );

    assert_eq!(action, ArrangementAction::default());
    assert!(engine.0.is_empty(), "{} engine commands", engine.0.len());
}

#[test]
fn resizing_a_clip_reports_the_project_edit() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 150, 300);

    let action = a.update(
        ArrangementMsg::ResizeSelectedClips {
            anchor: ArrangementSelection::AudioClip { track_id, clip_id },
            new_duration_beats: 2.0,
        },
        &mut RecordingEngine::default(),
        CTX,
    );

    assert!(action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].duration, 200);
}
