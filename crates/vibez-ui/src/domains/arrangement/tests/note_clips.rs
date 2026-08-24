use super::*;

#[test]
fn create_note_clip_needs_midi_track_and_active_selection() {
    let mut a = arrangement_with_tracks(1);
    let audio_tid = a.tracks[0].id;
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    let midi_tid = a.tracks[1].id;

    // No selection yet: refused.
    let action = a.update(
        ArrangementMsg::CreateNoteClipFromSelection(midi_tid),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(action.status.as_deref(), Some("No time selection active"));

    a.time_selection_active = true;
    a.selection_start_beats = 4.0;
    a.selection_end_beats = 8.0;

    // Audio track: refused.
    let action = a.update(
        ArrangementMsg::CreateNoteClipFromSelection(audio_tid),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(
        action.status.as_deref(),
        Some("Can only create note clips on MIDI tracks")
    );

    // MIDI track: creates and selects the clip.
    let action = a.update(
        ArrangementMsg::CreateNoteClipFromSelection(midi_tid),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(
        action.status.as_deref(),
        Some("Created note clip from selection")
    );
    let clip = &a.tracks[1].note_clips[0];
    assert_eq!(clip.position_beats, 4.0);
    assert_eq!(clip.duration_beats, 4.0);
    assert_eq!(a.selected_note_clip, Some((midi_tid, clip.id)));
}
