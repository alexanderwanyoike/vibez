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
fn a_trim_drag_past_a_warp_marker_and_back_restores_the_clip() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);
    assert!(a.tracks[0].clips[0]
        .warp_markers
        .add(400, 300, 200, 700, 500));
    let before = a.tracks[0].clips[0].clone();

    let past = trim_audio(&mut a, anchor, 7.0);
    assert!(!past
        .warp_markers
        .as_slice()
        .contains(&WarpMarker::new(400, 300)));
    let back = trim_audio(&mut a, anchor, 3.0);

    let geometry = |clip: &UiClip| {
        (
            clip.position,
            clip.duration,
            clip.source_offset,
            clip.start_marker,
        )
    };
    assert_eq!(geometry(&back), geometry(&before));
    assert_eq!(back.warp_markers, before.warp_markers);
}

#[test]
fn a_trim_drag_past_a_transient_marker_and_back_restores_it() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);
    a.tracks[0].clips[0]
        .transient_markers
        .replace_suggestions([250, 450, 650]);
    let before = a.tracks[0].clips[0].clone();

    trim_audio(&mut a, anchor, 4.0);
    let back = trim_audio(&mut a, anchor, 3.0);

    assert_eq!(back.transient_markers, before.transient_markers);
}

#[test]
fn a_new_trim_drag_starts_from_the_clip_the_last_one_left() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 200, 500);
    trim_audio(&mut a, anchor, 4.0);
    a.drag_resize_active = false;

    let after = trim_audio(&mut a, anchor, 5.0);

    assert_eq!((after.position, after.duration), (500, 300));
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

/// A note Clip at `position_beats`, 8 beats long, with notes at beats 0.5,
/// 2 and 5 of its content.
fn note_clip(a: &mut ArrangementFixture, position_beats: f64) -> ArrangementSelection {
    let track_id = a.tracks[0].id;
    let clip_id = ClipId::new();
    a.tracks[0].note_clips.push(UiNoteClip {
        id: clip_id,
        name: "Notes".into(),
        position_beats,
        duration_beats: 8.0,
        notes: [0.5, 2.0, 5.0]
            .into_iter()
            .map(|start_beat| MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat,
                duration_beats: 0.5,
            })
            .collect(),
        selected_notes: Default::default(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 8.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    });
    ArrangementSelection::NoteClip { track_id, clip_id }
}

fn trim_notes(a: &mut ArrangementFixture, anchor: ArrangementSelection, beats: f64) -> UiNoteClip {
    trim(a, anchor, beats, &mut RecordingEngine::default());
    a.tracks[0].note_clips[0].clone()
}

/// Arrangement beats of every note onset the Clip plays, repeats included.
fn note_onsets(clip: &UiNoteClip) -> Vec<f64> {
    let mut onsets: Vec<_> = clip
        .notes
        .iter()
        .flat_map(|note| clip.note_occurrences(note.start_beat))
        .map(|occurrence| clip.position_beats + occurrence)
        .collect();
    onsets.sort_by(f64::total_cmp);
    onsets
}

#[test]
fn trimming_a_note_clips_start_later_keeps_its_notes_in_place() {
    let mut a = arrangement_with_tracks(1);
    let anchor = note_clip(&mut a, 4.0);

    let after = trim_notes(&mut a, anchor, 6.0);

    assert_eq!((after.position_beats, after.duration_beats), (6.0, 6.0));
    assert_eq!(note_onsets(&after), vec![6.0, 9.0]);
    assert_eq!(after.notes.len(), 3, "the hidden note is kept");
}

#[test]
fn trimming_a_note_clips_start_edits_it_in_the_engine_without_re_adding_its_notes() {
    let mut a = arrangement_with_tracks(1);
    let anchor = note_clip(&mut a, 4.0);
    let mut engine = RecordingEngine::default();

    trim(&mut a, anchor, 6.0, &mut engine);

    assert!(engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::MoveNoteClip {
            new_position_beats,
            ..
        } if *new_position_beats == 6.0
    )));
    assert!(engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::SetNoteClipBounds {
            duration_beats,
            start_marker_beats,
            ..
        } if (*duration_beats, *start_marker_beats) == (6.0, 2.0)
    )));
    assert!(!engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::RemoveNoteClip(..) | EngineCommand::AddNoteClip { .. }
    )));
}

#[test]
fn trimming_a_note_clips_start_back_earlier_brings_its_notes_back_in_place() {
    let mut a = arrangement_with_tracks(1);
    let anchor = note_clip(&mut a, 4.0);
    assert_eq!(trim_notes(&mut a, anchor, 10.0).position_beats, 10.0);

    let after = trim_notes(&mut a, anchor, 4.0);

    assert_eq!((after.position_beats, after.duration_beats), (4.0, 8.0));
    assert_eq!(note_onsets(&after), vec![4.5, 6.0, 9.0]);
}

#[test]
fn a_note_clips_start_stops_at_the_start_of_its_content() {
    let mut a = arrangement_with_tracks(1);
    let anchor = note_clip(&mut a, 4.0);
    assert_eq!(trim_notes(&mut a, anchor, 5.0).position_beats, 5.0);

    let after = trim_notes(&mut a, anchor, 2.0);

    assert_eq!((after.position_beats, after.duration_beats), (4.0, 8.0));
    assert_eq!(after.start_marker_beats, 0.0);
}

#[test]
fn a_looped_note_clip_trimmed_past_its_loop_end_keeps_every_repeat_in_place() {
    let mut a = arrangement_with_tracks(1);
    let anchor = note_clip(&mut a, 4.0);
    let clip = &mut a.tracks[0].note_clips[0];
    clip.loop_enabled = true;
    clip.loop_end_beats = 3.0;
    let before = note_onsets(&a.tracks[0].note_clips[0]);

    let after = trim_notes(&mut a, anchor, 7.5);

    let kept: Vec<_> = before.into_iter().filter(|onset| *onset >= 7.5).collect();
    assert_eq!(note_onsets(&after), kept);
}

#[test]
fn trimming_a_mixed_selection_moves_audio_and_note_clip_starts_together() {
    let mut a = arrangement_with_tracks(1);
    let audio = sample_clip(&mut a, 1_000, 300, 200, 500);
    let notes = note_clip(&mut a, 4.0);
    a.selected_clips.insert(audio);
    a.selected_clips.insert(notes);

    trim(&mut a, notes, 5.0, &mut RecordingEngine::default());

    assert_eq!(a.tracks[0].clips[0].position, 400);
    assert_eq!(a.tracks[0].note_clips[0].position_beats, 5.0);
}

#[test]
fn a_looped_note_clips_start_comes_back_earlier_as_far_as_its_loop_start() {
    let mut a = arrangement_with_tracks(1);
    let anchor = note_clip(&mut a, 4.0);
    let clip = &mut a.tracks[0].note_clips[0];
    clip.loop_enabled = true;
    clip.start_marker_beats = 1.0;
    clip.loop_start_beats = 1.0;
    clip.loop_end_beats = 3.0;
    let before = note_onsets(&a.tracks[0].note_clips[0]);
    assert_eq!(trim_notes(&mut a, anchor, 5.5).position_beats, 5.5);

    let after = trim_notes(&mut a, anchor, 2.0);

    assert_eq!((after.position_beats, after.start_marker_beats), (4.0, 1.0));
    assert_eq!(note_onsets(&after), before);
}

#[test]
fn a_looped_audio_clips_start_comes_back_earlier_as_far_as_its_loop_start() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 300, 0, 600);
    let clip = &mut a.tracks[0].clips[0];
    clip.loop_enabled = true;
    clip.start_marker = 100;
    clip.loop_start = 100;
    clip.loop_end = 300;
    let before = a.tracks[0].clips[0].clone();
    assert_eq!(trim_audio(&mut a, anchor, 4.5).position, 450);

    let after = trim_audio(&mut a, anchor, 1.0);

    assert_eq!((after.position, after.start_marker), (300, 100));
    assert_audio_in_place(&before, &after, 0);
}

#[test]
fn a_trimmed_note_clip_resized_shorter_than_its_start_marker_keeps_its_notes_in_place() {
    let mut a = arrangement_with_tracks(1);
    let anchor = note_clip(&mut a, 4.0);
    assert_eq!(trim_notes(&mut a, anchor, 8.5).start_marker_beats, 4.5);

    resize(&mut a, anchor, 2.0);

    let clip = &a.tracks[0].note_clips[0];
    assert_eq!(clip.start_marker_beats, 4.5);
    assert_eq!(note_onsets(clip), vec![9.0]);
}
