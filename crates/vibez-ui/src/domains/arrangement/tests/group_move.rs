use super::*;

const CTX: ArrangementCtx = ArrangementCtx {
    samples_per_beat: 100.0,
    playhead_samples: 0,
    playhead_beats: 0.0,
};

fn select(a: &mut ArrangementFixture, track_id: TrackId, clip_id: ClipId) {
    a.selected_clips
        .insert(ArrangementSelection::AudioClip { track_id, clip_id });
}

fn drag(
    a: &mut ArrangementFixture,
    anchor: ClipId,
    new_position_beats: f64,
    target_track: TrackId,
) {
    a.update(
        ArrangementMsg::MoveSelectedClips {
            anchor,
            new_position_beats,
            target_track,
        },
        &mut RecordingEngine::default(),
        CTX,
    );
}

fn audio_position(a: &ArrangementFixture, track_idx: usize, clip_id: ClipId) -> Option<u64> {
    a.tracks[track_idx]
        .clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .map(|clip| clip.position)
}

#[test]
fn dragging_one_selected_clip_moves_the_whole_selection_by_the_same_offset() {
    let mut a = arrangement_with_tracks(2);
    let (t0, first) = add_audio_clip(&mut a, 0, 100, 100);
    let (t1, second) = add_audio_clip(&mut a, 1, 300, 100);
    select(&mut a, t0, first);
    select(&mut a, t1, second);

    drag(&mut a, first, 2.0, t0);

    assert_eq!(audio_position(&a, 0, first), Some(200));
    assert_eq!(audio_position(&a, 1, second), Some(400));
}

#[test]
fn a_selection_dragged_down_a_lane_keeps_its_shape_and_keeps_following_the_cursor() {
    let mut a = arrangement_with_tracks(3);
    let (t0, first) = add_audio_clip(&mut a, 0, 100, 100);
    let (t1, second) = add_audio_clip(&mut a, 1, 300, 100);
    let t2 = a.tracks[2].id;
    select(&mut a, t0, first);
    select(&mut a, t1, second);

    drag(&mut a, first, 1.0, t1);
    drag(&mut a, first, 3.0, t1);

    assert_eq!(audio_position(&a, 1, first), Some(300));
    assert_eq!(audio_position(&a, 2, second), Some(500));
    assert!(a.tracks[0].clips.is_empty());
    assert!(a.selected_clips.contains(&ArrangementSelection::AudioClip {
        track_id: t1,
        clip_id: first,
    }));
    assert!(a.selected_clips.contains(&ArrangementSelection::AudioClip {
        track_id: t2,
        clip_id: second,
    }));
    assert_eq!(a.selected_clips.len(), 2);
}

#[test]
fn a_lane_change_that_does_not_fit_every_clip_keeps_the_lanes_but_still_moves_in_time() {
    let mut a = arrangement_with_tracks(2);
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut RecordingEngine::default(),
        ArrangementCtx::default(),
    );
    let (t0, first) = add_audio_clip(&mut a, 0, 100, 100);
    let (t1, second) = add_audio_clip(&mut a, 1, 300, 100);
    select(&mut a, t0, first);
    select(&mut a, t1, second);

    // Lane 2 is MIDI, so the second clip has nowhere to go.
    drag(&mut a, first, 2.0, t1);

    assert_eq!(audio_position(&a, 0, first), Some(200));
    assert_eq!(audio_position(&a, 1, second), Some(400));
}

#[test]
fn the_selection_stops_at_the_arrangement_start_without_changing_shape() {
    let mut a = arrangement_with_tracks(2);
    let (t0, first) = add_audio_clip(&mut a, 0, 100, 100);
    let (t1, second) = add_audio_clip(&mut a, 1, 300, 100);
    select(&mut a, t0, first);
    select(&mut a, t1, second);

    drag(&mut a, second, 1.0, t1);

    assert_eq!(audio_position(&a, 0, first), Some(0));
    assert_eq!(audio_position(&a, 1, second), Some(200));
}

#[test]
fn dragging_a_clip_outside_the_selection_moves_only_that_clip() {
    let mut a = arrangement_with_tracks(2);
    let (t0, first) = add_audio_clip(&mut a, 0, 100, 100);
    let (t1, second) = add_audio_clip(&mut a, 1, 300, 100);
    select(&mut a, t0, first);

    drag(&mut a, second, 5.0, t1);

    assert_eq!(audio_position(&a, 0, first), Some(100));
    assert_eq!(audio_position(&a, 1, second), Some(500));
}
