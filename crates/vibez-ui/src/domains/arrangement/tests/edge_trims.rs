//! Dragging a Clip's edges changes its length while its content stays at
//! the same Arrangement time, and never past the content it plays.

use super::*;

const CTX: ArrangementCtx = ArrangementCtx {
    samples_per_beat: 100.0,
    playhead_samples: 0,
    playhead_beats: 0.0,
};

/// An Audio Clip at `position` playing `duration` frames of a
/// `sample_frames` long sample from `source_offset`.
fn sample_clip(
    a: &mut ArrangementFixture,
    sample_frames: usize,
    position: u64,
    source_offset: u64,
    duration: u64,
) -> ArrangementSelection {
    let (track_id, clip_id) = add_audio_clip(a, 0, position, duration);
    let clip = &mut a.tracks[0].clips[0];
    clip.audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
        channels: vec![vec![0.0; sample_frames]],
        sample_rate: 44100,
    });
    clip.source_offset = source_offset;
    clip.start_marker = source_offset;
    ArrangementSelection::AudioClip { track_id, clip_id }
}

fn resize(a: &mut ArrangementFixture, anchor: ArrangementSelection, beats: f64) {
    a.update(
        ArrangementMsg::ResizeSelectedClips {
            anchor,
            new_duration_beats: beats,
        },
        &mut RecordingEngine::default(),
        CTX,
    );
}

#[test]
fn an_unlooped_audio_clip_resized_past_its_sample_stops_at_the_sample_end() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 0, 200, 500);

    resize(&mut a, anchor, 12.0);

    assert_eq!(a.tracks[0].clips[0].duration, 800);
}

#[test]
fn a_reversed_audio_clip_resized_past_its_sample_stops_at_the_sample_end() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 0, 200, 500);
    a.tracks[0].clips[0].playback_direction = ClipPlaybackDirection::Reverse;

    resize(&mut a, anchor, 12.0);

    assert_eq!(a.tracks[0].clips[0].duration, 800);
}

#[test]
fn a_warped_audio_clip_resized_past_its_sample_stops_at_the_sample_end() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 0, 0, 500);
    assert!(a.tracks[0].clips[0].warp_markers.add(250, 400, 0, 500, 500));

    resize(&mut a, anchor, 12.0);

    let clip = &a.tracks[0].clips[0];
    assert_eq!(clip.duration, 1_000);
    assert_eq!(clip.source_end(), 1_000);
}

fn trim(
    a: &mut ArrangementFixture,
    anchor: ArrangementSelection,
    beats: f64,
    engine: &mut RecordingEngine,
) -> ArrangementAction {
    a.update(
        ArrangementMsg::TrimSelectedClipsStart {
            anchor,
            new_start_beats: beats,
        },
        engine,
        CTX,
    )
}

fn trim_audio(a: &mut ArrangementFixture, anchor: ArrangementSelection, beats: f64) -> UiClip {
    trim(a, anchor, beats, &mut RecordingEngine::default());
    a.tracks[0].clips[0].clone()
}

/// Every Arrangement frame both Clips cover plays the same source frame,
/// to within `tolerance` frames of rounding.
fn assert_audio_in_place(before: &UiClip, after: &UiClip, tolerance: u64) {
    let start = before.position.max(after.position);
    let end = (before.position + before.duration).min(after.position + after.duration);
    assert!(start < end, "the Clips share no Arrangement time");
    for frame in start..end {
        let was = before.source_frame_at(frame - before.position);
        let is = after.source_frame_at(frame - after.position);
        assert!(
            was.abs_diff(is) <= tolerance,
            "Arrangement frame {frame} played source {was}, now {is}"
        );
    }
}

#[test]
fn trimming_an_audio_clips_start_later_keeps_its_audio_in_place() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);
    let before = a.tracks[0].clips[0].clone();

    let after = trim_audio(&mut a, anchor, 4.0);

    assert_eq!((after.position, after.duration), (400, 400));
    assert_audio_in_place(&before, &after, 0);
}

#[test]
fn trimming_an_audio_clips_start_edits_it_in_the_engine_without_re_adding_it() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);
    let mut engine = RecordingEngine::default();

    trim(&mut a, anchor, 4.0, &mut engine);

    assert!(engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::MoveClip {
            new_position: 400,
            ..
        }
    )));
    assert!(engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::SetClipBounds {
            start_marker: 300,
            duration: 400,
            ..
        }
    )));
    assert!(!engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::RemoveClip(..) | EngineCommand::AddClip { .. }
    )));
}

#[test]
fn trimming_an_audio_clips_start_earlier_reveals_the_audio_before_it_in_place() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);
    let before = a.tracks[0].clips[0].clone();

    let after = trim_audio(&mut a, anchor, 2.0);

    assert_eq!((after.position, after.duration), (200, 600));
    assert_eq!(after.source_frame_at(0), 100);
    assert_audio_in_place(&before, &after, 0);
}

#[test]
fn an_audio_clips_start_stops_at_the_start_of_its_sample() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);

    let after = trim_audio(&mut a, anchor, 0.0);

    assert_eq!((after.position, after.duration), (100, 700));
    assert_eq!(after.source_frame_at(0), 0);
}

#[test]
fn a_warped_clip_trimmed_later_then_back_earlier_plays_its_warp_in_place() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);
    assert!(a.tracks[0].clips[0]
        .warp_markers
        .add(400, 300, 200, 700, 500));
    let before = a.tracks[0].clips[0].clone();

    let later = trim_audio(&mut a, anchor, 4.0);
    assert_audio_in_place(&before, &later, 1);
    let back = trim_audio(&mut a, anchor, 2.0);

    assert_eq!((back.position, back.duration), (200, 600));
    assert_audio_in_place(&before, &back, 1);
    assert!(back
        .warp_markers
        .as_slice()
        .contains(&WarpMarker::new(400, 400)));
}

#[test]
fn a_reversed_clip_trimmed_later_and_earlier_keeps_its_audio_in_place() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);
    a.tracks[0].clips[0].playback_direction = ClipPlaybackDirection::Reverse;
    let before = a.tracks[0].clips[0].clone();

    let later = trim_audio(&mut a, anchor, 4.0);
    assert_audio_in_place(&before, &later, 0);
    let earlier = trim_audio(&mut a, anchor, 1.0);

    assert_eq!((earlier.position, earlier.duration), (100, 700));
    assert_audio_in_place(&before, &earlier, 0);
}

#[test]
fn a_looped_clip_trimmed_past_its_loop_end_keeps_every_repeat_in_place() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 0, 0, 1_000);
    let clip = &mut a.tracks[0].clips[0];
    clip.loop_enabled = true;
    clip.loop_start = 0;
    clip.loop_end = 250;
    let before = a.tracks[0].clips[0].clone();

    let after = trim_audio(&mut a, anchor, 3.0);

    assert_eq!((after.position, after.duration), (300, 700));
    assert_audio_in_place(&before, &after, 0);
}

#[test]
fn trimming_keeps_transient_markers_on_the_audio_still_in_the_clip() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);
    a.tracks[0].clips[0]
        .transient_markers
        .replace_suggestions([250, 450, 650]);

    let after = trim_audio(&mut a, anchor, 4.0);

    assert_eq!(
        after
            .transient_markers
            .as_slice()
            .iter()
            .map(|marker| marker.source_frame())
            .collect::<Vec<_>>(),
        vec![450, 650]
    );
}

#[test]
fn a_trim_later_than_the_clip_end_leaves_the_shortest_clip() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);

    let after = trim_audio(&mut a, anchor, 20.0);

    assert_eq!((after.position, after.duration), (787, 13));
}

#[test]
fn trimming_a_selection_moves_every_clip_start_by_the_same_offset() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, first) = add_audio_clip(&mut a, 0, 0, 400);
    let (_, second) = add_audio_clip(&mut a, 0, 500, 400);
    let anchor = ArrangementSelection::AudioClip {
        track_id,
        clip_id: first,
    };
    a.selected_clips.insert(anchor);
    a.selected_clips.insert(ArrangementSelection::AudioClip {
        track_id,
        clip_id: second,
    });

    trim(&mut a, anchor, 1.0, &mut RecordingEngine::default());

    let span = |id: ClipId| {
        let clip = a.tracks[0].clips.iter().find(|clip| clip.id == id).unwrap();
        (clip.position, clip.duration)
    };
    assert_eq!(span(first), (100, 300));
    assert_eq!(span(second), (600, 300));
}

#[test]
fn trimming_a_clip_to_its_own_start_does_nothing() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);
    let mut engine = RecordingEngine::default();

    let action = trim(&mut a, anchor, 3.0, &mut engine);

    assert_eq!(action, ArrangementAction::default());
    assert!(engine.0.is_empty(), "{} engine commands", engine.0.len());
}
