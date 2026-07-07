//! Arrangement domain unit tests.

use super::*;
use crate::domains::test_support::RecordingEngine;

fn arrangement_with_tracks(n: usize) -> ArrangementState {
    let mut a = ArrangementState {
        next_track_number: 1,
        ..Default::default()
    };
    let mut engine = RecordingEngine::default();
    for _ in 0..n {
        a.update(
            ArrangementMsg::AddTrack,
            &mut engine,
            ArrangementCtx::default(),
        );
    }
    a
}

#[test]
fn add_track_selects_it_and_names_uniquely() {
    let a = arrangement_with_tracks(2);
    assert_eq!(a.tracks.len(), 2);
    assert_eq!(a.tracks[1].name, "Track 2");
    assert_eq!(a.selected_track, Some(a.tracks[1].id));
}

#[test]
fn remove_track_clears_its_selections_and_requests_gui_teardown() {
    let mut a = arrangement_with_tracks(2);
    let victim = a.tracks[1].id;
    let survivor = a.tracks[0].id;
    a.selected_note_clip = Some((victim, ClipId::new()));
    let mut engine = RecordingEngine::default();
    let action = a.update(
        ArrangementMsg::RemoveTrack(victim),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks.len(), 1);
    assert_eq!(a.selected_track, Some(survivor));
    assert_eq!(a.selected_note_clip, None);
    assert_eq!(action.close_track_guis, Some(victim));
}

#[test]
fn reorder_sends_full_order_and_respects_bounds() {
    let mut a = arrangement_with_tracks(2);
    let first = a.tracks[0].id;
    let mut engine = RecordingEngine::default();
    // Top track cannot move further up: no command.
    a.update(
        ArrangementMsg::MoveTrackUp(first),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(engine.0.is_empty());
    a.update(
        ArrangementMsg::MoveTrackDown(first),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks[1].id, first);
    assert!(matches!(engine.0[0], EngineCommand::ReorderTracks(_)));
}

#[test]
fn gain_and_pan_clamp() {
    let mut a = arrangement_with_tracks(1);
    let id = a.tracks[0].id;
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::SetTrackGain(id, 99.0),
        &mut engine,
        ArrangementCtx::default(),
    );
    a.update(
        ArrangementMsg::SetTrackPan(id, -5.0),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks[0].gain, 2.0);
    assert_eq!(a.tracks[0].pan, 0.0);
}

#[test]
fn meter_decays_instead_of_snapping() {
    let mut a = arrangement_with_tracks(1);
    let id = a.tracks[0].id;
    a.tracks[0].peak_l = 1.0;
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::EngineTrackMeter {
            track_id: id,
            peak_l: 0.0,
            peak_r: 0.0,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!((a.tracks[0].peak_l - 0.85).abs() < 1e-6);
}

fn add_audio_clip(
    a: &mut ArrangementState,
    track_idx: usize,
    position: u64,
    duration: u64,
) -> (TrackId, ClipId) {
    let audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
        channels: vec![vec![0.0; (position + duration) as usize]],
        sample_rate: 44100,
    });
    let id = ClipId::new();
    let tid = a.tracks[track_idx].id;
    a.tracks[track_idx].clips.push(UiClip {
        id,
        name: "Clip".to_string(),
        audio,
        source: None,
        position,
        source_offset: 0,
        duration,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
        original_audio: None,
    });
    (tid, id)
}

#[test]
fn split_audio_clip_replaces_clip_with_two_halves() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1000);
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::SplitAudioClip {
            track_id: tid,
            clip_id: cid,
            split_position: 400,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks[0].clips.len(), 2);
    assert_eq!(a.tracks[0].clips[0].duration, 400);
    assert_eq!(a.tracks[0].clips[1].duration, 600);
    assert_eq!(a.tracks[0].clips[1].position, 400);
    assert_eq!(a.tracks[0].clips[1].source_offset, 400);
    assert!(matches!(engine.0[0], EngineCommand::RemoveClip(..)));
}

#[test]
fn split_outside_clip_bounds_is_a_noop() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 100, 500);
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::SplitAudioClip {
            track_id: tid,
            clip_id: cid,
            split_position: 50,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks[0].clips.len(), 1);
    assert!(engine.0.is_empty());
}

#[test]
fn join_merges_adjacent_audio_clips_into_one() {
    let mut a = arrangement_with_tracks(1);
    let (tid, c1) = add_audio_clip(&mut a, 0, 0, 100);
    let (_, c2) = add_audio_clip(&mut a, 0, 200, 100);
    a.selected_clips.insert(ArrangementSelection::AudioClip {
        track_id: tid,
        clip_id: c1,
    });
    a.selected_clips.insert(ArrangementSelection::AudioClip {
        track_id: tid,
        clip_id: c2,
    });
    let mut engine = RecordingEngine::default();
    let action = a.update(
        ArrangementMsg::JoinSelectedClips,
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks[0].clips.len(), 1);
    assert_eq!(a.tracks[0].clips[0].position, 0);
    assert_eq!(a.tracks[0].clips[0].duration, 300);
    assert_eq!(action.status.as_deref(), Some("Joined audio clips"));
}

#[test]
fn join_rejects_mixed_selection_types() {
    let mut a = arrangement_with_tracks(1);
    let (tid, c1) = add_audio_clip(&mut a, 0, 0, 100);
    a.selected_clips.insert(ArrangementSelection::AudioClip {
        track_id: tid,
        clip_id: c1,
    });
    a.selected_clips.insert(ArrangementSelection::NoteClip {
        track_id: tid,
        clip_id: ClipId::new(),
    });
    let mut engine = RecordingEngine::default();
    let action = a.update(
        ArrangementMsg::JoinSelectedClips,
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks[0].clips.len(), 1);
    assert_eq!(
        action.status.as_deref(),
        Some("Join requires same type and track")
    );
}

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

fn warp_success(
    audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
) -> crate::message::ClipWarpSuccess {
    crate::message::ClipWarpSuccess {
        original_audio: Arc::clone(&audio),
        audio: Arc::new(vibez_core::audio_buffer::DecodedAudio {
            channels: vec![vec![0.0; 2000]],
            sample_rate: 44100,
        }),
        new_duration: 2000,
        new_source_offset: 0,
        new_loop_start: 0,
        new_loop_end: 0,
        detected_bpm: 128.0,
        warped_to_bpm: 120.0,
    }
}

#[test]
fn warp_then_clear_roundtrips_clip_geometry() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1000);
    let original = Arc::clone(&a.tracks[0].clips[0].audio);
    let mut engine = RecordingEngine::default();

    let action =
        a.apply_clip_warp_success(&mut engine, tid, cid, warp_success(Arc::clone(&original)));
    let clip = &a.tracks[0].clips[0];
    assert!(clip.warped);
    assert_eq!(clip.duration, 2000);
    assert_eq!(clip.warped_to_bpm, Some(120.0));
    assert_eq!(clip.original_bpm, Some(128.0));
    assert!(clip.original_audio.is_some());
    assert!(action.mark_dirty);
    assert!(matches!(
        engine.0[0],
        EngineCommand::ReplaceClipAudio { .. }
    ));

    let action = a.apply_clear_clip_warp(&mut engine, tid, cid);
    let clip = &a.tracks[0].clips[0];
    assert!(!clip.warped);
    assert_eq!(clip.duration, 1000);
    assert!(clip.original_audio.is_none());
    assert!(Arc::ptr_eq(&clip.audio, &original));
    assert!(action.mark_dirty);
}

#[test]
fn bpm_detected_commits_and_clears_pending_edit() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1000);
    a.clip_bpm_edit.insert(cid, "999".to_string());
    let action = a.apply_clip_bpm_detected(tid, cid, Some(174.0), 0.9);
    assert_eq!(a.tracks[0].clips[0].original_bpm, Some(174.0));
    assert!(a.clip_bpm_edit.is_empty());
    assert!(action.mark_dirty);

    let action = a.apply_clip_bpm_detected(tid, cid, None, 0.0);
    assert!(!action.mark_dirty);
    assert!(action.status.unwrap().contains("Could not detect BPM"));
}

#[test]
fn submit_clip_bpm_parses_and_rejects_garbage() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1000);
    let mut engine = RecordingEngine::default();
    a.clip_bpm_edit.insert(cid, "140.5".to_string());
    let action = a.update(
        ArrangementMsg::SubmitClipBpm {
            track_id: tid,
            clip_id: cid,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks[0].clips[0].original_bpm, Some(140.5));
    assert!(action.mark_dirty);

    a.clip_bpm_edit.insert(cid, "not a number".to_string());
    let action = a.update(
        ArrangementMsg::SubmitClipBpm {
            track_id: tid,
            clip_id: cid,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(!action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].original_bpm, Some(140.5));
}
