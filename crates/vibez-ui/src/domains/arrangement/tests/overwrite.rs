//! The overwrite rule: what placing Clips over a range does to the Clips
//! already on that lane.

use super::*;
use crate::domains::arrangement::overwrite::{lane_overwrite, LaneOverwrite};

const SPB: f64 = 100.0;

/// An unlooped Audio Clip at `position` frames playing `duration` frames
/// from the start of a long enough sample.
fn audio(position: u64, duration: u64) -> UiClip {
    UiClip {
        id: ClipId::new(),
        name: "Drums".to_string(),
        audio: Arc::new(vibez_core::audio_buffer::DecodedAudio {
            channels: vec![vec![0.0; 10_000]],
            sample_rate: 44100,
        }),
        source: None,
        position,
        source_offset: 0,
        start_marker: 0,
        duration,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
        original_audio: None,
    }
}

/// An unlooped note Clip with one note on every beat of its length.
fn notes(position_beats: f64, duration_beats: f64) -> UiNoteClip {
    UiNoteClip {
        name: "Hats".to_string(),
        notes: (0..duration_beats as usize)
            .map(|beat| MidiNote {
                pitch: 42,
                velocity: 100,
                start_beat: beat as f64,
                duration_beats: 0.25,
            })
            .collect(),
        loop_end_beats: 0.0,
        ..UiNoteClip::test_clip(position_beats, duration_beats)
    }
}

fn audio_lane(clips: &[&UiClip]) -> TrackTimelineContent {
    TrackTimelineContent {
        clips: clips.iter().map(|clip| (*clip).clone()).collect(),
        ..Default::default()
    }
}

fn note_lane(clips: &[&UiNoteClip]) -> TrackTimelineContent {
    TrackTimelineContent {
        note_clips: clips.iter().map(|clip| (*clip).clone()).collect(),
        ..Default::default()
    }
}

fn place(lane: &TrackTimelineContent, start: f64, end: f64) -> LaneOverwrite {
    lane_overwrite(lane, (start, end), &HashSet::new(), SPB)
}

/// The pieces left of `clip`, or `None` when the overwrite left it alone.
fn audio_pieces<'a>(edits: &'a LaneOverwrite, clip: &UiClip) -> Option<&'a [UiClip]> {
    edits
        .audio
        .iter()
        .find(|(id, _)| *id == clip.id)
        .map(|(_, pieces)| pieces.as_slice())
}

fn note_pieces<'a>(edits: &'a LaneOverwrite, clip: &UiNoteClip) -> Option<&'a [UiNoteClip]> {
    edits
        .notes
        .iter()
        .find(|(id, _)| *id == clip.id)
        .map(|(_, pieces)| pieces.as_slice())
}

/// Every frame of `piece` plays what `original` played at that time.
fn assert_audio_in_place(original: &UiClip, piece: &UiClip) {
    let offset = piece.position - original.position;
    for frame in (0..piece.duration).step_by(7) {
        assert_eq!(
            piece.source_frame_at(frame),
            original.source_frame_at(offset + frame),
            "frame {frame} of the piece at {} moved",
            piece.position
        );
    }
}

/// Every beat of `piece` plays what `original` played at that time, with
/// the same notes.
fn assert_notes_in_place(original: &UiNoteClip, piece: &UiNoteClip) {
    assert_eq!(piece.notes, original.notes);
    let offset = piece.position_beats - original.position_beats;
    let mut beat = 0.0;
    while beat < piece.duration_beats {
        assert_eq!(
            piece.timeline().source_at(beat),
            original.timeline().source_at(offset + beat),
            "beat {beat} of the piece at {} moved",
            piece.position_beats
        );
        beat += 0.125;
    }
}

#[test]
fn a_fully_covered_clip_is_removed() {
    let covered = audio(200, 200);

    let edits = place(&audio_lane(&[&covered]), 1.0, 5.0);

    assert_eq!(audio_pieces(&edits, &covered).map(<[_]>::len), Some(0));
}

#[test]
fn a_clip_whose_tail_is_covered_keeps_its_head() {
    let clip = audio(0, 400);

    let edits = place(&audio_lane(&[&clip]), 2.0, 6.0);

    let pieces = audio_pieces(&edits, &clip).unwrap();
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].id, clip.id);
    assert_eq!((pieces[0].position, pieces[0].duration), (0, 200));
    assert_audio_in_place(&clip, &pieces[0]);
}

#[test]
fn a_clip_whose_head_is_covered_keeps_its_tail_in_place() {
    let clip = audio(200, 400);

    let edits = place(&audio_lane(&[&clip]), 0.0, 4.0);

    let pieces = audio_pieces(&edits, &clip).unwrap();
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].id, clip.id);
    assert_eq!((pieces[0].position, pieces[0].duration), (400, 200));
    assert_audio_in_place(&clip, &pieces[0]);
}

#[test]
fn a_range_inside_a_longer_clip_splits_it_around_the_range() {
    let clip = audio(0, 800);

    let edits = place(&audio_lane(&[&clip]), 2.0, 4.0);

    let pieces = audio_pieces(&edits, &clip).unwrap();
    let spans: Vec<_> = pieces
        .iter()
        .map(|piece| (piece.position, piece.duration))
        .collect();
    assert_eq!(spans, vec![(0, 200), (400, 400)]);
    assert_eq!(pieces[0].id, clip.id);
    assert_ne!(pieces[1].id, clip.id);
    for piece in pieces {
        assert_audio_in_place(&clip, piece);
    }
}

#[test]
fn a_looped_audio_clip_keeps_its_loop_phase_on_both_sides() {
    let mut clip = audio(0, 800);
    clip.loop_enabled = true;
    clip.loop_start = 0;
    clip.loop_end = 150;

    let edits = place(&audio_lane(&[&clip]), 2.5, 4.5);

    let pieces = audio_pieces(&edits, &clip).unwrap();
    assert_eq!(pieces.len(), 2);
    for piece in pieces {
        assert_audio_in_place(&clip, piece);
    }
}

#[test]
fn a_reversed_audio_clip_keeps_its_content_in_place_on_both_sides() {
    let mut clip = audio(0, 800);
    clip.playback_direction = ClipPlaybackDirection::Reverse;

    let edits = place(&audio_lane(&[&clip]), 2.0, 4.0);

    let pieces = audio_pieces(&edits, &clip).unwrap();
    assert_eq!(pieces.len(), 2);
    for piece in pieces {
        assert_audio_in_place(&clip, piece);
    }
}

#[test]
fn a_range_over_several_clips_trims_both_ends_and_removes_the_middle() {
    let first = audio(0, 300);
    let middle = audio(300, 200);
    let last = audio(500, 300);
    let untouched = audio(800, 100);

    let edits = place(&audio_lane(&[&first, &middle, &last, &untouched]), 2.0, 6.0);

    let spans = |clip: &UiClip| -> Vec<(u64, u64)> {
        audio_pieces(&edits, clip)
            .unwrap()
            .iter()
            .map(|piece| (piece.position, piece.duration))
            .collect()
    };
    assert_eq!(spans(&first), vec![(0, 200)]);
    assert_eq!(spans(&middle), vec![]);
    assert_eq!(spans(&last), vec![(600, 200)]);
    assert!(audio_pieces(&edits, &untouched).is_none());
}

#[test]
fn clips_that_only_touch_the_range_are_left_alone() {
    let before = audio(0, 200);
    let after = audio(400, 200);

    let edits = place(&audio_lane(&[&before, &after]), 2.0, 4.0);

    assert!(edits.audio.is_empty());
}

#[test]
fn the_clips_being_placed_never_overwrite_each_other() {
    let placed = audio(0, 400);
    let other = audio(200, 400);
    let lane = audio_lane(&[&placed, &other]);

    let edits = lane_overwrite(&lane, (0.0, 4.0), &HashSet::from([placed.id]), SPB);

    assert!(audio_pieces(&edits, &placed).is_none());
    assert_eq!(audio_pieces(&edits, &other).unwrap()[0].position, 400);
}

#[test]
fn a_leftover_shorter_than_the_shortest_clip_is_removed_with_the_rest() {
    let clip = notes(0.0, 4.0);

    let edits = place(&note_lane(&[&clip]), 0.1, 4.0);

    assert_eq!(note_pieces(&edits, &clip).map(<[_]>::len), Some(0));
}

#[test]
fn a_note_clip_whose_tail_is_covered_keeps_its_head_and_notes() {
    let clip = notes(0.0, 8.0);

    let edits = place(&note_lane(&[&clip]), 4.0, 12.0);

    let pieces = note_pieces(&edits, &clip).unwrap();
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].id, clip.id);
    assert_eq!(
        (pieces[0].position_beats, pieces[0].duration_beats),
        (0.0, 4.0)
    );
    assert_notes_in_place(&clip, &pieces[0]);
}

#[test]
fn a_note_clip_whose_head_is_covered_moves_its_start_marker_over_its_notes() {
    let clip = notes(2.0, 8.0);

    let edits = place(&note_lane(&[&clip]), 0.0, 5.0);

    let pieces = note_pieces(&edits, &clip).unwrap();
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].id, clip.id);
    assert_eq!(
        (pieces[0].position_beats, pieces[0].duration_beats),
        (5.0, 5.0)
    );
    assert_eq!(pieces[0].start_marker_beats, 3.0);
    assert_notes_in_place(&clip, &pieces[0]);
}

#[test]
fn a_range_inside_a_note_clip_splits_it_and_a_covered_one_goes() {
    let long = notes(0.0, 8.0);
    let short = notes(8.0, 2.0);

    let edits = place(&note_lane(&[&long, &short]), 4.0, 10.0);
    let split = place(&note_lane(&[&long]), 2.0, 6.0);

    assert_eq!(note_pieces(&edits, &short).map(<[_]>::len), Some(0));
    let pieces = note_pieces(&split, &long).unwrap();
    let spans: Vec<_> = pieces
        .iter()
        .map(|piece| (piece.position_beats, piece.duration_beats))
        .collect();
    assert_eq!(spans, vec![(0.0, 2.0), (6.0, 2.0)]);
    assert_eq!(pieces[0].id, long.id);
    assert_ne!(pieces[1].id, long.id);
    for piece in pieces {
        assert_notes_in_place(&long, piece);
    }
}

#[test]
fn a_looped_note_clip_keeps_its_loop_phase_on_both_sides() {
    let mut clip = notes(0.0, 8.0);
    clip.loop_enabled = true;
    clip.loop_start_beats = 0.0;
    clip.loop_end_beats = 1.5;

    let edits = place(&note_lane(&[&clip]), 2.5, 4.0);

    let pieces = note_pieces(&edits, &clip).unwrap();
    assert_eq!(pieces.len(), 2);
    assert!(pieces.iter().all(|piece| piece.loop_enabled));
    for piece in pieces {
        assert_notes_in_place(&clip, piece);
    }
}
