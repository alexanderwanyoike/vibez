//! Existing independent Clip Capture materialization regressions.

use super::*;
use crate::domains::perform::LauncherClip;

fn source(track_id: TrackId, pitch: u8, beats: f64) -> LauncherClip {
    let id = ClipId::new();
    let mut timeline = ArrangementTimeline::default();
    timeline.ensure(track_id).note_clips.push(UiNoteClip {
        id,
        name: "Part".into(),
        position_beats: 0.0,
        duration_beats: beats,
        notes: vec![MidiNote {
            pitch,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: beats,
        }],
        selected_notes: Default::default(),
        start_marker_beats: 0.0,
        loop_enabled: true,
        loop_start_beats: 0.0,
        loop_end_beats: beats,
        groove_grid: Default::default(),
    });
    LauncherClip {
        id,
        track_id,
        row: 0,
        timeline: Arc::new(timeline),
    }
}

#[test]
fn independent_combinations_capture_mid_loop_replacements_and_silence() {
    let bass = TrackId::new();
    let hat = TrackId::new();
    let mut bass_clip = source(bass, 36, 4.0);
    let hat_clip = source(hat, 42, 1.0);
    let replacement = source(bass, 40, 2.0);
    let mut capture = CaptureState::default();
    capture.update(CaptureMsg::Toggle);
    capture.prepare(40, 8, 120.0);
    capture.prepare_controlled_tracks([(bass, false), (hat, false)]);
    capture.start(6, None);
    capture.clip_transition(
        bass,
        Some(CapturedTimelineSource::from_clip(&bass_clip, 4.0)),
        6,
        6,
    );
    capture.clip_transition(
        hat,
        Some(CapturedTimelineSource::from_clip(&hat_clip, 4.0)),
        8,
        0,
    );
    capture.clip_transition(hat, None, 12, 0);
    capture.clip_transition(
        bass,
        Some(CapturedTimelineSource::from_clip(&replacement, 4.0)),
        16,
        0,
    );
    Arc::make_mut(&mut bass_clip.timeline)
        .ensure(bass)
        .note_clips[0]
        .notes[0]
        .pitch = 99;
    let take = capture.finish(20).unwrap().materialize();
    assert_eq!(
        (take.arrange_start_samples, take.arrange_end_samples),
        (40, 54)
    );
    let bass_notes = &take.by_track[&bass].note_clips;
    assert_eq!(bass_notes.len(), 2);
    assert_eq!(
        (bass_notes[0].position_beats, bass_notes[0].duration_beats),
        (10.0, 2.5)
    );
    assert_eq!(bass_notes[0].notes[0].pitch, 36);
    assert_eq!(
        (bass_notes[1].position_beats, bass_notes[1].duration_beats),
        (12.5, 1.0)
    );
    assert_eq!(bass_notes[1].notes[0].pitch, 40);
    let hats = &take.by_track[&hat].note_clips;
    assert_eq!(hats.len(), 1);
    assert_eq!(
        (hats[0].position_beats, hats[0].duration_beats),
        (10.5, 1.0)
    );
    assert_ne!(hats[0].id, hat_clip.id);
    assert!(take.controlled_track_ids.contains(&hat));
}

#[test]
fn independent_loops_flatten_without_moving_other_tracks_boundaries() {
    let a = TrackId::new();
    let b = TrackId::new();
    let mut capture = CaptureState::default();
    capture.update(CaptureMsg::Toggle);
    capture.prepare(0, 8, 120.0);
    capture.start(0, None);
    capture.clip_transition(
        a,
        Some(CapturedTimelineSource::from_clip(&source(a, 36, 1.0), 4.0)),
        0,
        0,
    );
    capture.clip_transition(
        b,
        Some(CapturedTimelineSource::from_clip(&source(b, 48, 3.0), 4.0)),
        2,
        0,
    );
    let take = capture.finish(14).unwrap().materialize();
    let a_clips = &take.by_track[&a].note_clips;
    assert_eq!(
        a_clips
            .iter()
            .map(|clip| (clip.position_beats, clip.duration_beats))
            .collect::<Vec<_>>(),
        [(0.0, 1.0), (1.0, 1.0), (2.0, 1.0), (3.0, 0.5)]
    );
    assert_eq!(take.by_track[&b].note_clips.len(), 1);
    assert_eq!(take.by_track[&b].note_clips[0].position_beats, 0.5);
}
