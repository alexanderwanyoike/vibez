use super::super::test_support::RecordingEngine;
use super::*;
use crate::state::PianoRollEditMode;

fn midi_track_with_clip() -> (TimelineEditorState, TrackId, ClipId) {
    let track_id = TrackId::new();
    let clip_id = ClipId::new();
    let mut timeline = TimelineEditorState::default();
    let track = timeline.ensure(track_id);
    track.note_clips.push(UiNoteClip {
        id: clip_id,
        name: "Pattern 1".to_string(),
        position_beats: 0.0,
        duration_beats: 4.0,
        notes: vec![
            MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.1,
                duration_beats: 0.5,
            },
            MidiNote {
                pitch: 64,
                velocity: 100,
                start_beat: 1.9,
                duration_beats: 0.5,
            },
        ],
        selected_notes: HashSet::new(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: GrooveGrid::Off,
    });
    (timeline, track_id, clip_id)
}

/// The fixture holds two notes: pitch 60 at beats 0.1..0.6, and
/// pitch 64 at beats 1.9..2.4.
#[allow(clippy::too_many_arguments)]
fn region(
    tracks: &mut TimelineEditorState,
    tid: TrackId,
    cid: ClipId,
    start_beat: f64,
    end_beat: f64,
    low_pitch: u8,
    high_pitch: u8,
    additive: bool,
) {
    let mut pr = PianoRollState::default();
    let mut engine = RecordingEngine::default();
    pr.update(
        PianoRollMsg::SelectNotesInRegion {
            track_id: tid,
            clip_id: cid,
            start_beat,
            end_beat,
            low_pitch,
            high_pitch,
            additive,
        },
        &mut engine,
        tracks,
        PianoRollCtx::default(),
    );
}

fn selected(tracks: &TimelineEditorState, tid: TrackId) -> HashSet<usize> {
    tracks.get(tid).unwrap().note_clips[0]
        .selected_notes
        .clone()
}

#[test]
fn a_rubber_band_catches_notes_inside_both_its_beat_and_pitch_range() {
    let (mut tracks, tid, cid) = midi_track_with_clip();

    region(&mut tracks, tid, cid, 0.0, 4.0, 0, 127, false);
    assert_eq!(selected(&tracks, tid), HashSet::from([0, 1]));

    // Same beats, pitch range excluding the second note.
    region(&mut tracks, tid, cid, 0.0, 4.0, 55, 62, false);
    assert_eq!(selected(&tracks, tid), HashSet::from([0]));

    // Same pitches, beat range excluding the first note.
    region(&mut tracks, tid, cid, 1.0, 4.0, 0, 127, false);
    assert_eq!(selected(&tracks, tid), HashSet::from([1]));
}

#[test]
fn a_note_only_clipped_by_the_edge_of_the_band_still_counts() {
    let (mut tracks, tid, cid) = midi_track_with_clip();

    // Ends at 0.3, partway through the first note's 0.1..0.6 span.
    region(&mut tracks, tid, cid, 0.0, 0.3, 0, 127, false);

    assert_eq!(selected(&tracks, tid), HashSet::from([0]));
}

#[test]
fn a_non_additive_band_replaces_the_selection_while_shift_extends_it() {
    let (mut tracks, tid, cid) = midi_track_with_clip();

    region(&mut tracks, tid, cid, 0.0, 1.0, 0, 127, false);
    assert_eq!(selected(&tracks, tid), HashSet::from([0]));

    // Non-additive band elsewhere drops the earlier note.
    region(&mut tracks, tid, cid, 1.5, 4.0, 0, 127, false);
    assert_eq!(selected(&tracks, tid), HashSet::from([1]));

    // Additive band brings it back alongside.
    region(&mut tracks, tid, cid, 0.0, 1.0, 0, 127, true);
    assert_eq!(selected(&tracks, tid), HashSet::from([0, 1]));
}

#[test]
fn add_note_updates_clip_and_engine() {
    let (mut tracks, tid, cid) = midi_track_with_clip();
    let mut pr = PianoRollState::default();
    let mut engine = RecordingEngine::default();
    pr.update(
        PianoRollMsg::AddNote {
            track_id: tid,
            clip_id: cid,
            pitch: 67,
            start_beat: 2.0,
            duration_beats: 1.0,
        },
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );
    assert_eq!(tracks.get(tid).unwrap().note_clips[0].notes.len(), 3);
    assert!(matches!(engine.0[0], EngineCommand::AddNote { .. }));
}

#[test]
fn batch_velocity_edit_updates_valid_notes_and_the_engine() {
    let (mut tracks, tid, cid) = midi_track_with_clip();
    let mut piano_roll = PianoRollState::default();
    let mut engine = RecordingEngine::default();

    piano_roll.update(
        PianoRollMsg::SetNoteVelocities {
            track_id: tid,
            clip_id: cid,
            velocities: vec![(0, 72), (1, 118), (99, 64)],
        },
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );

    let clip = &tracks.get(tid).unwrap().note_clips[0];
    assert_eq!(clip.notes[0].velocity, 72);
    assert_eq!(clip.notes[1].velocity, 118);
    assert_eq!(engine.0.len(), 2);
    assert!(matches!(
        engine.0[0],
        EngineCommand::EditNote {
            note_index: 0,
            note: MidiNote { velocity: 72, .. },
            ..
        }
    ));
    assert!(matches!(
        engine.0[1],
        EngineCommand::EditNote {
            note_index: 1,
            note: MidiNote { velocity: 118, .. },
            ..
        }
    ));
}

#[test]
fn velocity_edit_clamps_zero_without_changing_shared_selection() {
    let (mut tracks, tid, cid) = midi_track_with_clip();
    tracks.get_mut(tid).unwrap().note_clips[0].selected_notes = HashSet::from([0, 1]);
    let mut piano_roll = PianoRollState::default();
    let mut engine = RecordingEngine::default();

    piano_roll.update(
        PianoRollMsg::SetNoteVelocities {
            track_id: tid,
            clip_id: cid,
            velocities: vec![(0, 0), (1, u8::MAX)],
        },
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );

    let clip = &tracks.get(tid).unwrap().note_clips[0];
    assert_eq!(clip.notes[0].velocity, 1);
    assert_eq!(clip.notes[1].velocity, 127);
    assert_eq!(clip.selected_notes, HashSet::from([0, 1]));
}

#[test]
fn add_note_clip_creates_missing_timeline_lane_for_a_shared_track() {
    let track_id = TrackId::new();
    let mut editor = TimelineEditorState::default();
    let mut piano_roll = PianoRollState::default();
    let mut engine = RecordingEngine::default();

    let action = piano_roll.update(
        PianoRollMsg::AddNoteClipToTrack(track_id),
        &mut engine,
        &mut editor,
        PianoRollCtx::default(),
    );

    assert_eq!(editor.timeline.get(track_id).unwrap().note_clips.len(), 1);
    assert_eq!(action.select_note_clip.unwrap().0, track_id);
    assert!(matches!(engine.0[0], EngineCommand::AddNoteClip { .. }));
}

#[test]
fn remove_note_reindexes_selection() {
    let (mut tracks, tid, cid) = midi_track_with_clip();
    tracks.get_mut(tid).unwrap().note_clips[0].selected_notes = [0, 1].into_iter().collect();
    let mut pr = PianoRollState::default();
    let mut engine = RecordingEngine::default();
    pr.update(
        PianoRollMsg::RemoveNote(tid, cid, 0),
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );
    let clip = &tracks.get(tid).unwrap().note_clips[0];
    assert_eq!(clip.notes.len(), 1);
    assert_eq!(
        clip.selected_notes.iter().copied().collect::<Vec<_>>(),
        vec![0]
    );
}

#[test]
fn quantize_snaps_notes_and_reports_count() {
    let (mut tracks, tid, cid) = midi_track_with_clip();
    let mut pr = PianoRollState::default();
    let mut engine = RecordingEngine::default();
    let action = pr.update(
        PianoRollMsg::QuantizeNoteClip {
            track_id: tid,
            clip_id: cid,
        },
        &mut engine,
        &mut tracks,
        PianoRollCtx {
            snap_grid: SnapGrid::QUARTER,
        },
    );
    let clip = &tracks.get(tid).unwrap().note_clips[0];
    assert_eq!(clip.notes[0].start_beat, 0.0);
    assert_eq!(clip.notes[1].start_beat, 2.0);
    assert_eq!(action.status.as_deref(), Some("Quantized 2 note(s) to 1/4"));
}

#[test]
fn double_clip_clones_notes_and_grows_engine_clip() {
    let (mut tracks, tid, cid) = midi_track_with_clip();
    let mut pr = PianoRollState::default();
    let mut engine = RecordingEngine::default();
    pr.update(
        PianoRollMsg::DoubleNoteClip(tid, cid),
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );
    let clip = &tracks.get(tid).unwrap().note_clips[0];
    assert_eq!(clip.duration_beats, 8.0);
    assert_eq!(clip.notes.len(), 4);
    assert_eq!(clip.notes[2].start_beat, 4.1);
    assert!(engine
        .0
        .iter()
        .any(|c| matches!(c, EngineCommand::SetNoteClipDuration { .. })));
}

#[test]
fn enabling_loop_replaces_stale_bounds_with_the_current_clip_length() {
    let (mut tracks, tid, cid) = midi_track_with_clip();
    let clip = &mut tracks.get_mut(tid).unwrap().note_clips[0];
    clip.duration_beats = 8.0;
    clip.loop_enabled = false;
    clip.loop_start_beats = 0.0;
    clip.loop_end_beats = 4.0;
    clip.notes.push(MidiNote {
        pitch: 67,
        velocity: 100,
        start_beat: 6.0,
        duration_beats: 0.5,
    });

    let mut piano_roll = PianoRollState::default();
    let mut engine = RecordingEngine::default();
    piano_roll.update(
        PianoRollMsg::ToggleNoteClipLoop(tid, cid),
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );

    let clip = &tracks.get(tid).unwrap().note_clips[0];
    assert!(clip.loop_enabled);
    assert_eq!(clip.loop_start_beats, 0.0);
    assert_eq!(clip.loop_end_beats, 8.0);
    assert!(matches!(
        engine.0.as_slice(),
        [EngineCommand::SetNoteClipLoop {
            enabled: true,
            loop_start_beats: 0.0,
            loop_end_beats: 8.0,
            ..
        }]
    ));
}

#[test]
fn extending_a_non_looping_clip_does_not_infer_a_loop_from_note_content() {
    let (mut tracks, tid, cid) = midi_track_with_clip();
    let mut piano_roll = PianoRollState::default();
    let mut engine = RecordingEngine::default();

    piano_roll.update(
        PianoRollMsg::ResizeNoteClipDuration {
            track_id: tid,
            clip_id: cid,
            new_duration_beats: 8.0,
        },
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );

    let clip = &tracks.get(tid).unwrap().note_clips[0];
    assert_eq!(clip.duration_beats, 8.0);
    assert!(!clip.loop_enabled);
    assert_eq!(clip.loop_start_beats, 0.0);
    assert_eq!(clip.loop_end_beats, 0.0);
}

#[test]
fn start_marker_moves_without_changing_the_midi_loop_region() {
    let (mut tracks, track_id, clip_id) = midi_track_with_clip();
    let clip = &mut tracks.get_mut(track_id).unwrap().note_clips[0];
    clip.loop_enabled = true;
    clip.loop_start_beats = 1.0;
    clip.loop_end_beats = 4.0;
    let mut piano_roll = PianoRollState::default();
    let mut engine = RecordingEngine::default();

    piano_roll.update(
        PianoRollMsg::SetNoteClipStartMarker {
            track_id,
            clip_id,
            start_marker_beats: 2.0,
        },
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );

    let clip = &tracks.get(track_id).unwrap().note_clips[0];
    assert_eq!(clip.start_marker_beats, 2.0);
    assert_eq!((clip.loop_start_beats, clip.loop_end_beats), (1.0, 4.0));
    assert!(matches!(
        engine.0.as_slice(),
        [EngineCommand::SetNoteClipStartMarker {
            start_marker_beats: 2.0,
            ..
        }]
    ));
}

#[test]
fn midi_loop_region_must_be_ordered_and_inside_the_clip() {
    let (mut tracks, track_id, clip_id) = midi_track_with_clip();
    let mut piano_roll = PianoRollState::default();
    let mut engine = RecordingEngine::default();

    piano_roll.update(
        PianoRollMsg::SetNoteClipLoopRegion {
            track_id,
            clip_id,
            loop_start_beats: 3.0,
            loop_end_beats: 5.0,
        },
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );
    let clip = &tracks.get(track_id).unwrap().note_clips[0];
    assert_eq!((clip.loop_start_beats, clip.loop_end_beats), (0.0, 0.0));
    assert!(engine.0.is_empty());

    piano_roll.update(
        PianoRollMsg::SetNoteClipLoopRegion {
            track_id,
            clip_id,
            loop_start_beats: 1.0,
            loop_end_beats: 3.0,
        },
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );
    let clip = &tracks.get(track_id).unwrap().note_clips[0];
    assert_eq!((clip.loop_start_beats, clip.loop_end_beats), (1.0, 3.0));
    assert!(matches!(
        engine.0.as_slice(),
        [EngineCommand::SetNoteClipLoop { .. }]
    ));

    engine.0.clear();
    piano_roll.update(
        PianoRollMsg::SetNoteClipLoopRegion {
            track_id,
            clip_id,
            loop_start_beats: 1.0,
            loop_end_beats: 3.0,
        },
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );
    assert!(engine.0.is_empty());
}

#[test]
fn groove_grid_updates_the_clip_and_live_engine_source() {
    let (mut tracks, tid, cid) = midi_track_with_clip();
    let mut piano_roll = PianoRollState::default();
    let mut engine = RecordingEngine::default();

    let action = piano_roll.update(
        PianoRollMsg::SetNoteClipGrooveGrid(tid, cid, GrooveGrid::Sixteenth),
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );

    assert_eq!(
        tracks.get(tid).unwrap().note_clips[0].groove_grid,
        GrooveGrid::Sixteenth
    );
    assert!(matches!(
        engine.0[0],
        EngineCommand::SetNoteClipGrooveGrid {
            track_id,
            clip_id,
            groove_grid: GrooveGrid::Sixteenth,
        } if track_id == tid && clip_id == cid
    ));
    assert_eq!(
        action.status.as_deref(),
        Some("Clip Swing follows Track on 1/16")
    );
}

#[test]
fn toggle_edit_mode_flips_and_reports() {
    let mut pr = PianoRollState::default();
    let mut engine = RecordingEngine::default();
    let mut tracks = TimelineEditorState::default();
    let action = pr.update(
        PianoRollMsg::ToggleEditMode,
        &mut engine,
        &mut tracks,
        PianoRollCtx::default(),
    );
    assert_eq!(pr.edit_mode, PianoRollEditMode::Draw);
    assert_eq!(action.status.as_deref(), Some("Piano roll: Draw mode"));
}
