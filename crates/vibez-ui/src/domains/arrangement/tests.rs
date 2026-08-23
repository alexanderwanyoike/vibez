//! Arrangement domain unit tests.

use super::test_support::*;
use super::*;
use crate::domains::test_support::RecordingEngine;
use crate::state::{AudioClipInspectorField, UiClip};
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::midi::MidiNote;
use vibez_core::track::AudioInputRoute;

#[test]
fn add_track_selects_it_and_names_uniquely() {
    let a = arrangement_with_tracks(2);
    assert_eq!(a.tracks.len(), 2);
    assert_eq!(a.tracks[1].name, "Track 2");
    assert_eq!(a.selected_track, Some(a.tracks[1].id));
}

#[test]
fn track_removal_requires_confirmation_then_clears_its_state() {
    let mut a = arrangement_with_tracks(2);
    let victim = a.tracks[1].id;
    let survivor = a.tracks[0].id;
    a.selected_note_clip = Some((victim, ClipId::new()));
    let mut engine = RecordingEngine::default();
    let request = a.update(
        ArrangementMsg::RequestRemoveTrack(victim),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks.len(), 2);
    assert_eq!(a.pending_project_track_deletion, Some(victim));
    assert_eq!(request.close_track_guis, None);
    let action = a.update(
        ArrangementMsg::ConfirmRemoveTrack(victim),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks.len(), 1);
    assert_eq!(a.selected_track, Some(survivor));
    assert_eq!(a.selected_note_clip, None);
    assert_eq!(action.close_track_guis, Some(victim));
    assert!(a.arrangement.timeline.get(victim).is_none());
}

#[test]
fn cancelling_track_removal_preserves_the_project_track() {
    let mut a = arrangement_with_tracks(1);
    let victim = a.tracks[0].id;
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::RequestRemoveTrack(victim),
        &mut engine,
        ArrangementCtx::default(),
    );
    a.update(
        ArrangementMsg::CancelRemoveTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks.len(), 1);
    assert_eq!(a.pending_project_track_deletion, None);
    assert!(engine.0.is_empty());
    assert!(!ArrangementMsg::RequestRemoveTrack(victim).marks_dirty());
    assert!(!ArrangementMsg::CancelRemoveTrack.marks_dirty());
    assert!(ArrangementMsg::ConfirmRemoveTrack(victim).marks_dirty());
}

#[test]
fn immediate_track_removal_reuses_the_confirmed_deletion_operation() {
    let mut a = arrangement_with_tracks(2);
    let victim = a.tracks[1].id;
    let mut engine = RecordingEngine::default();

    let action = a.update(
        ArrangementMsg::RemoveTrack(victim),
        &mut engine,
        ArrangementCtx::default(),
    );

    assert_eq!(a.tracks.len(), 1);
    assert!(a.arrangement.timeline.get(victim).is_none());
    assert_eq!(action.close_track_guis, Some(victim));
    assert_eq!(action.remove_track_from_sections, Some(victim));
    assert!(ArrangementMsg::RemoveTrack(victim).marks_dirty());
}

#[test]
fn removing_a_resample_source_resets_dependent_audio_track_routes() {
    let mut a = arrangement_with_tracks(2);
    let source = a.tracks[0].id;
    let target = a.tracks[1].id;
    a.tracks[1].audio_input_route = AudioInputRoute::Resample { track_id: source };
    let mut engine = RecordingEngine::default();

    a.update(
        ArrangementMsg::RemoveTrack(source),
        &mut engine,
        ArrangementCtx::default(),
    );

    assert_eq!(
        a.tracks
            .iter()
            .find(|track| track.id == target)
            .unwrap()
            .audio_input_route,
        AudioInputRoute::default(),
    );
}

#[test]
fn remove_bus_clears_sends_and_their_automation_lanes() {
    let mut a = arrangement_with_tracks(1);
    let track_id = a.tracks[0].id;
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddBus,
        &mut engine,
        ArrangementCtx::default(),
    );
    let bus_id = a.buses[0].id;
    a.tracks[0].sends.push((bus_id, 0.5));
    a.tracks[0]
        .automation
        .push(AutomationLane::new(AutomationTarget::Send { bus_id }));

    a.update(
        ArrangementMsg::RemoveBus(bus_id),
        &mut engine,
        ArrangementCtx::default(),
    );

    let track = a.tracks.iter().find(|track| track.id == track_id).unwrap();
    assert!(track.sends.iter().all(|(id, _)| *id != bus_id));
    assert!(track
        .automation
        .iter()
        .all(|lane| lane.target != AutomationTarget::Send { bus_id }));
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
fn renames_audio_midi_and_bus_channels() {
    let mut a = arrangement_with_tracks(1);
    let audio_id = a.tracks[0].id;
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    a.update(
        ArrangementMsg::AddBus,
        &mut engine,
        ArrangementCtx::default(),
    );
    let midi_id = a.tracks[1].id;
    let bus_id = a.buses[0].id;

    for (id, name) in [
        (audio_id, "Vocals"),
        (midi_id, "Keys"),
        (bus_id, "Long Reverb"),
    ] {
        a.update(
            ArrangementMsg::RenameTrack(id, name.to_string()),
            &mut engine,
            ArrangementCtx::default(),
        );
    }

    assert_eq!(a.find_track(audio_id).unwrap().name, "Vocals");
    assert_eq!(a.find_track(midi_id).unwrap().name, "Keys");
    assert_eq!(a.find_track(bus_id).unwrap().name, "Long Reverb");
}

#[test]
fn bus_solo_toggles_state_and_sends_the_engine_command() {
    let mut a = arrangement_with_tracks(1);
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddBus,
        &mut engine,
        ArrangementCtx::default(),
    );
    let bus_id = a.buses[0].id;
    engine.0.clear();

    a.update(
        ArrangementMsg::SetTrackSolo(bus_id),
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(a.buses[0].solo);
    assert!(matches!(
        engine.0.as_slice(),
        [EngineCommand::SetTrackSolo(id, true)] if *id == bus_id
    ));
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
    a: &mut ArrangementFixture,
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
    let clip = UiClip {
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
        gain_db: Default::default(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
        original_audio: None,
    };
    a.tracks[track_idx].clips.push(clip.clone());
    Arc::make_mut(&mut a.arrangement.timeline)
        .ensure(tid)
        .clips
        .push(clip);
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
    let clip = &a.arrangement.timeline.get(tid).unwrap().clips[0];
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

    let mut action = a.apply_clear_clip_warp(&mut engine, tid, cid);
    let request = action
        .transpose_render
        .take()
        .expect("clearing Warp renders the raw-timing buffer off-thread");
    a.apply_clip_transpose_success(
        &mut engine,
        tid,
        cid,
        crate::message::ClipTransposeSuccess {
            audio: Arc::clone(&request.source_audio),
            source_audio: request.source_audio,
            transpose: request.transpose,
            expected_warped: request.expected_warped,
            geometry: request.geometry,
        },
    );
    let clip = &a.arrangement.timeline.get(tid).unwrap().clips[0];
    assert!(!clip.warped);
    assert_eq!(clip.duration, 1000);
    assert!(clip.original_audio.is_none());
    assert!(Arc::ptr_eq(&clip.audio, &original));
    assert!(action.mark_dirty);
}

#[test]
fn inspector_gain_and_source_bounds_reach_the_resident_clip() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 44_100);
    let mut engine = RecordingEngine::default();

    a.audio_clip_inspector_edits
        .insert((cid, AudioClipInspectorField::Gain), "6.0".into());
    let action = a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::Gain,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].gain_db.db(), 6.0);
    assert!(matches!(
        engine.0.last(),
        Some(EngineCommand::SetClipGain { linear_gain, .. })
            if (*linear_gain - 1.995_262).abs() < 0.001
    ));

    a.audio_clip_inspector_edits
        .insert((cid, AudioClipInspectorField::SourceStart), "0.250".into());
    a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::SourceStart,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    let clip = &a.tracks[0].clips[0];
    assert_eq!(clip.source_offset, 11_025);
    assert_eq!(clip.duration, 33_075);
    assert!(engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::SetClipBounds {
            source_offset: 11_025,
            duration: 33_075,
            ..
        }
    )));

    a.update(
        ArrangementMsg::ToggleClipLoop(tid, cid),
        &mut engine,
        ArrangementCtx::default(),
    );
    let clip = &a.tracks[0].clips[0];
    assert!(clip.loop_enabled);
    assert_eq!(clip.loop_start, 11_025);
    assert_eq!(clip.loop_end, 44_100);
}

#[test]
fn inspector_knobs_commit_gain_and_rounded_transpose_values() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 44_100);
    let mut engine = RecordingEngine::default();

    let gain_action = a.update(
        ArrangementMsg::SetAudioClipInspectorValue {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::Gain,
            value: -6.25,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(gain_action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].gain_db.db(), -6.2);
    assert!(matches!(
        engine.0.last(),
        Some(EngineCommand::SetClipGain { track_id, clip_id, .. })
            if *track_id == tid && *clip_id == cid
    ));

    let transpose_action = a.update(
        ArrangementMsg::SetAudioClipInspectorValue {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::Transpose,
            value: 7.6,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    let request = transpose_action
        .transpose_render
        .expect("knob transpose render request");
    assert_eq!(request.transpose.semitones(), 8);
    assert_eq!(a.tracks[0].clips[0].transpose.semitones(), 8);
}

#[test]
fn invalid_inspector_boundary_leaves_the_clip_unchanged() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 44_100);
    let mut engine = RecordingEngine::default();
    a.audio_clip_inspector_edits
        .insert((cid, AudioClipInspectorField::SourceEnd), "2.0".into());

    let action = a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::SourceEnd,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(!action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].duration, 44_100);
    assert!(engine.0.is_empty());
}

#[test]
fn source_start_uses_the_rendered_source_end_for_a_looped_clip() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 44_100);
    let clip = &mut a.tracks[0].clips[0];
    clip.duration = 88_200;
    clip.loop_enabled = true;
    clip.loop_start = 0;
    clip.loop_end = 44_100;
    let mut engine = RecordingEngine::default();
    a.audio_clip_inspector_edits
        .insert((cid, AudioClipInspectorField::SourceStart), "0.250".into());

    let action = a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::SourceStart,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(action.mark_dirty);
    let clip = &a.tracks[0].clips[0];
    assert_eq!(clip.source_offset, 11_025);
    assert_eq!(clip.duration, 33_075);
    assert_eq!(clip.loop_start, 11_025);
    assert_eq!(clip.loop_end, 44_100);
}

#[test]
fn audio_quantize_replaces_and_selects_the_clip_in_its_timeline() {
    let mut a = arrangement_with_tracks(1);
    let (tid, old_clip_id) = add_audio_clip(&mut a, 0, 120, 44_100);
    Arc::make_mut(&mut a.arrangement.timeline).ensure(tid).clips[0].gain_db =
        vibez_core::track::ClipGainDb::new(-4.0).unwrap();
    let new_clip_id = ClipId::new();
    let new_audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
        channels: vec![vec![0.0; 22_050]],
        sample_rate: 44_100,
    });
    let mut engine = RecordingEngine::default();

    let action = a.apply_audio_quantize_success(
        &mut engine,
        tid,
        old_clip_id,
        crate::message::AudioQuantizeSuccess {
            new_clip_id,
            new_audio,
            new_name: "Quantized Clip".into(),
            new_position: 0,
            new_duration: 22_050,
            slice_count: 4,
            grid_label: "1/16".into(),
        },
        44_100,
    );

    assert!(action.mark_dirty);
    let clips = &a.arrangement.timeline.get(tid).unwrap().clips;
    assert_eq!(clips.len(), 1);
    let clip = &clips[0];
    assert_eq!(clip.id, new_clip_id);
    assert_eq!(clip.name, "Quantized Clip");
    assert_eq!(clip.gain_db.db(), -4.0);
    assert!(a.selected_clips.contains(&ArrangementSelection::AudioClip {
        track_id: tid,
        clip_id: new_clip_id,
    }));
    assert!(matches!(
        engine.0.as_slice(),
        [EngineCommand::RemoveClip(track_id, clip_id), EngineCommand::AddClip { clip_id: added, .. }]
            if *track_id == tid && *clip_id == old_clip_id && *added == new_clip_id
    ));
}

#[test]
fn inspector_transpose_requests_one_duration_preserving_background_render() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 44_100);
    let mut engine = RecordingEngine::default();
    a.audio_clip_inspector_edits
        .insert((cid, AudioClipInspectorField::Transpose), "7".into());

    let action = a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::Transpose,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    let request = action.transpose_render.expect("transpose render request");
    assert_eq!(request.transpose.semitones(), 7);
    assert_eq!(request.target_frames, 44_100);
    assert!(request.geometry.is_none());
    assert_eq!(a.tracks[0].clips[0].transpose.semitones(), 7);
    assert!(engine.0.is_empty(), "DSP never runs on the audio thread");
}

#[test]
fn bpm_detected_commits_and_clears_pending_edit() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1000);
    a.audio_clip_inspector_edits
        .insert((cid, AudioClipInspectorField::SourceBpm), "999".to_string());
    let action = a.apply_clip_bpm_detected(tid, cid, Some(174.0), 0.9);
    assert_eq!(
        a.arrangement.timeline.get(tid).unwrap().clips[0].original_bpm,
        Some(174.0)
    );
    assert!(a.audio_clip_inspector_edits.is_empty());
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
    a.audio_clip_inspector_edits.insert(
        (cid, AudioClipInspectorField::SourceBpm),
        "140.5".to_string(),
    );
    let action = a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::SourceBpm,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks[0].clips[0].original_bpm, Some(140.5));
    assert!(action.mark_dirty);

    a.audio_clip_inspector_edits.insert(
        (cid, AudioClipInspectorField::SourceBpm),
        "not a number".to_string(),
    );
    let action = a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::SourceBpm,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(!action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].original_bpm, Some(140.5));
}

#[test]
fn copy_and_paste_multiple_clips_anchors_at_playhead_and_preserves_offsets_and_loops() {
    let mut a = arrangement_with_tracks(1);
    let (tid, first) = add_audio_clip(&mut a, 0, 0, 100);
    let (_, second) = add_audio_clip(&mut a, 0, 200, 100);
    a.tracks[0].clips[0].loop_enabled = true;
    a.tracks[0].clips[0].loop_end = 100;
    for clip_id in [first, second] {
        a.selected_clips.insert(ArrangementSelection::AudioClip {
            track_id: tid,
            clip_id,
        });
    }
    let mut engine = RecordingEngine::default();
    let ctx = ArrangementCtx {
        samples_per_beat: 100.0,
        playhead_samples: 1_000,
        playhead_beats: 10.0,
    };

    a.update(ArrangementMsg::CopySelectedClips, &mut engine, ctx);
    a.update(ArrangementMsg::PasteClips, &mut engine, ctx);

    assert_eq!(a.tracks[0].clips.len(), 4);
    let mut pasted: Vec<_> = a.tracks[0].clips[2..].iter().collect();
    pasted.sort_by_key(|clip| clip.position);
    assert_eq!(pasted[0].position, 1_000);
    assert_eq!(pasted[1].position, 1_200);
    assert!(pasted[0].loop_enabled);
    assert!(pasted.iter().all(|clip| clip.name == "Clip"));
    assert_eq!(a.selected_clips.len(), 2);
}

#[test]
fn partial_time_selection_copies_audio_and_trimmed_midi() {
    let mut a = arrangement_with_tracks(1);
    let (audio_tid, _) = add_audio_clip(&mut a, 0, 100, 600);
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    let midi_tid = a.tracks[1].id;
    let note_id = ClipId::new();
    a.tracks[1].note_clips.push(UiNoteClip {
        id: note_id,
        name: "Notes".to_string(),
        position_beats: 1.0,
        duration_beats: 6.0,
        notes: vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 3.0,
        }],
        selected_notes: HashSet::new(),
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    });
    a.time_selection_active = true;
    a.selection_start_beats = 2.0;
    a.selection_end_beats = 5.0;
    a.time_selection_track = None;
    let ctx = ArrangementCtx {
        samples_per_beat: 100.0,
        playhead_samples: 1_000,
        playhead_beats: 10.0,
    };

    a.update(ArrangementMsg::CopySelectedClips, &mut engine, ctx);
    a.selected_track = Some(audio_tid);
    a.update(ArrangementMsg::PasteClips, &mut engine, ctx);

    let audio = a.find_track(audio_tid).unwrap().clips.last().unwrap();
    assert_eq!(audio.position, 1_000);
    assert_eq!(audio.duration, 300);
    assert_eq!(audio.source_offset, 100);
    let notes = a.find_track(midi_tid).unwrap().note_clips.last().unwrap();
    assert_eq!(notes.position_beats, 10.0);
    assert_eq!(notes.duration_beats, 3.0);
    assert_eq!(notes.notes[0].start_beat, 0.0);
    assert_eq!(notes.notes[0].duration_beats, 2.0);
}

#[test]
fn cut_time_selection_preserves_material_outside_the_range() {
    let mut a = arrangement_with_tracks(1);
    add_audio_clip(&mut a, 0, 0, 800);
    a.time_selection_active = true;
    a.selection_start_beats = 2.0;
    a.selection_end_beats = 5.0;
    a.time_selection_track = Some(a.tracks[0].id);
    let mut engine = RecordingEngine::default();
    let ctx = ArrangementCtx {
        samples_per_beat: 100.0,
        ..Default::default()
    };

    a.update(ArrangementMsg::CutSelectedClips, &mut engine, ctx);

    let mut remaining: Vec<_> = a.tracks[0].clips.iter().collect();
    remaining.sort_by_key(|clip| clip.position);
    assert_eq!(remaining.len(), 2);
    assert_eq!((remaining[0].position, remaining[0].duration), (0, 200));
    assert_eq!((remaining[1].position, remaining[1].duration), (500, 300));
    assert_eq!(a.clipboard.clips.len(), 1);
}

#[test]
fn loop_toggle_and_resize_apply_to_the_whole_clip_selection() {
    let mut a = arrangement_with_tracks(2);
    let (tid, first) = add_audio_clip(&mut a, 0, 0, 200);
    let (second_tid, second) = add_audio_clip(&mut a, 1, 300, 300);
    a.selected_clips.insert(ArrangementSelection::AudioClip {
        track_id: tid,
        clip_id: first,
    });
    a.selected_clips.insert(ArrangementSelection::AudioClip {
        track_id: second_tid,
        clip_id: second,
    });
    let mut engine = RecordingEngine::default();
    let ctx = ArrangementCtx {
        samples_per_beat: 100.0,
        ..Default::default()
    };

    a.update(ArrangementMsg::ToggleSelectedClipLoop, &mut engine, ctx);
    assert!(a.tracks.iter().all(|track| track.clips[0].loop_enabled));
    a.update(
        ArrangementMsg::ResizeSelectedClips {
            anchor: ArrangementSelection::AudioClip {
                track_id: tid,
                clip_id: first,
            },
            new_duration_beats: 4.0,
        },
        &mut engine,
        ctx,
    );

    assert_eq!(a.tracks[0].clips[0].duration, 400);
    assert_eq!(a.tracks[1].clips[0].duration, 500);
}

#[test]
fn duplicate_preserves_audio_and_midi_loop_settings() {
    let mut a = arrangement_with_tracks(1);
    let (audio_tid, audio_id) = add_audio_clip(&mut a, 0, 0, 300);
    let audio = &mut a.tracks[0].clips[0];
    audio.loop_enabled = true;
    audio.loop_start = 10;
    audio.loop_end = 110;
    audio.gain_db = vibez_core::track::ClipGainDb::new(-4.0).unwrap();
    audio.transpose = vibez_core::track::ClipTranspose::new(5);

    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    let midi_tid = a.tracks[1].id;
    let midi_id = ClipId::new();
    a.tracks[1].note_clips.push(UiNoteClip {
        id: midi_id,
        name: "Loop".to_string(),
        position_beats: 0.0,
        duration_beats: 8.0,
        notes: vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 1.0,
        }],
        selected_notes: HashSet::new(),
        loop_enabled: true,
        loop_start_beats: 0.0,
        loop_end_beats: 4.0,
        groove_grid: vibez_core::perform::GrooveGrid::Sixteenth,
    });
    a.selected_clips.insert(ArrangementSelection::AudioClip {
        track_id: audio_tid,
        clip_id: audio_id,
    });
    a.selected_clips.insert(ArrangementSelection::NoteClip {
        track_id: midi_tid,
        clip_id: midi_id,
    });
    engine.0.clear();

    a.update(
        ArrangementMsg::DuplicateSelectedClip,
        &mut engine,
        ArrangementCtx::default(),
    );

    let audio_copy = a.tracks[0].clips.last().unwrap();
    assert!(audio_copy.loop_enabled);
    assert_eq!((audio_copy.loop_start, audio_copy.loop_end), (10, 110));
    assert_eq!(audio_copy.gain_db.db(), -4.0);
    assert_eq!(audio_copy.transpose.semitones(), 5);
    let midi_copy = a.tracks[1].note_clips.last().unwrap();
    assert!(midi_copy.loop_enabled);
    assert_eq!(
        (midi_copy.loop_start_beats, midi_copy.loop_end_beats),
        (0.0, 4.0)
    );
    assert!(engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::AddClip {
            loop_enabled: true,
            loop_start: 10,
            loop_end: 110,
            linear_gain,
            ..
        } if (*linear_gain - vibez_core::track::ClipGainDb::new(-4.0).unwrap().linear()).abs()
            < f32::EPSILON
    )));
    assert!(engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::AddNoteClip {
            loop_enabled: true,
            loop_start_beats: 0.0,
            loop_end_beats: 4.0,
            ..
        }
    )));
}

#[test]
fn repeated_duplicate_keeps_the_source_clip_name_readable() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 100);
    a.tracks[0].clips[0].name = "OTH_128_Hub_Full.wav".to_string();
    a.selected_clips
        .insert(ArrangementSelection::AudioClip { track_id, clip_id });
    let mut engine = RecordingEngine::default();

    for _ in 0..4 {
        a.update(
            ArrangementMsg::DuplicateSelectedClip,
            &mut engine,
            ArrangementCtx::default(),
        );
    }

    assert!(a.tracks[0]
        .clips
        .iter()
        .all(|clip| clip.name == "OTH_128_Hub_Full.wav"));
}

#[test]
fn midi_duplicate_keeps_the_source_clip_name_readable() {
    let mut a = arrangement_with_tracks(1);
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    let track_id = a.tracks[1].id;
    let clip_id = ClipId::new();
    a.tracks[1].note_clips.push(UiNoteClip {
        id: clip_id,
        name: "Pattern 1".to_string(),
        position_beats: 0.0,
        duration_beats: 4.0,
        notes: Vec::new(),
        selected_notes: HashSet::new(),
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    });
    a.selected_clips
        .insert(ArrangementSelection::NoteClip { track_id, clip_id });

    a.update(
        ArrangementMsg::DuplicateSelectedClip,
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(a.tracks[1]
        .note_clips
        .iter()
        .all(|clip| clip.name == "Pattern 1"));
}

#[test]
fn split_looped_audio_preserves_source_phase() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 300);
    let clip = &mut a.tracks[0].clips[0];
    clip.loop_enabled = true;
    clip.loop_start = 0;
    clip.loop_end = 100;
    let mut engine = RecordingEngine::default();

    a.update(
        ArrangementMsg::SplitAudioClip {
            track_id,
            clip_id,
            split_position: 150,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    let mut halves: Vec<_> = a.tracks[0].clips.iter().collect();
    halves.sort_by_key(|clip| clip.position);
    assert!(halves.iter().all(|clip| clip.loop_enabled));
    assert_eq!(halves[1].source_offset, 50);
    assert_eq!((halves[1].loop_start, halves[1].loop_end), (0, 100));
}

#[test]
fn trim_track_mutes_replaces_audio_with_unmuted_fragments_and_preserves_loop_phase() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 800);
    let clip = &mut a.tracks[0].clips[0];
    clip.loop_enabled = true;
    clip.loop_start = 0;
    clip.loop_end = 100;
    let mut lane = AutomationLane::new(AutomationTarget::TrackMute);
    for (beat, value) in [(2.5, 1.0), (4.5, 0.0)] {
        lane.insert_point(AutomationPoint {
            beat,
            value,
            curve: 0.0,
        });
    }
    a.tracks[0].automation.push(lane);
    a.selected_clips
        .insert(ArrangementSelection::AudioClip { track_id, clip_id });
    let mut engine = RecordingEngine::default();

    let action = a.update(
        ArrangementMsg::TrimSelectedByTrackMutes,
        &mut engine,
        ArrangementCtx {
            samples_per_beat: 100.0,
            ..Default::default()
        },
    );

    let mut fragments: Vec<_> = a.tracks[0].clips.iter().collect();
    fragments.sort_by_key(|clip| clip.position);
    assert_eq!(fragments.len(), 2);
    assert_eq!((fragments[0].position, fragments[0].duration), (0, 250));
    assert_eq!((fragments[1].position, fragments[1].duration), (450, 350));
    assert_eq!(fragments[1].source_offset, 50);
    assert!(fragments.iter().all(|clip| clip.loop_enabled));
    assert_eq!(a.selected_clips.len(), 2);
    assert_eq!(
        engine
            .0
            .iter()
            .filter(|command| matches!(command, EngineCommand::RemoveClip(..)))
            .count(),
        1
    );
    assert_eq!(
        engine
            .0
            .iter()
            .filter(|command| matches!(command, EngineCommand::AddClip { .. }))
            .count(),
        2
    );
    assert_eq!(
        action.status.as_deref(),
        Some("Trimmed 1 clip by Track Mutes · kept 2 fragments")
    );
}

#[test]
fn trim_track_mutes_materializes_midi_notes_across_unmuted_fragments() {
    let mut a = arrangement_with_tracks(1);
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    engine.0.clear();
    let track_id = a.tracks[1].id;
    let clip_id = ClipId::new();
    a.tracks[1].note_clips.push(UiNoteClip {
        id: clip_id,
        name: "Held note".to_string(),
        position_beats: 0.0,
        duration_beats: 8.0,
        notes: vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 1.0,
            duration_beats: 5.0,
        }],
        selected_notes: HashSet::new(),
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    });
    let mut lane = AutomationLane::new(AutomationTarget::TrackMute);
    for (beat, value) in [(2.0, 1.0), (4.0, 0.0)] {
        lane.insert_point(AutomationPoint {
            beat,
            value,
            curve: 0.0,
        });
    }
    a.tracks[1].automation.push(lane);
    a.selected_clips
        .insert(ArrangementSelection::NoteClip { track_id, clip_id });

    a.update(
        ArrangementMsg::TrimSelectedByTrackMutes,
        &mut engine,
        ArrangementCtx {
            samples_per_beat: 100.0,
            ..Default::default()
        },
    );

    let mut fragments: Vec<_> = a.tracks[1].note_clips.iter().collect();
    fragments.sort_by(|a, b| a.position_beats.partial_cmp(&b.position_beats).unwrap());
    assert_eq!(fragments.len(), 2);
    assert_eq!(
        (fragments[0].position_beats, fragments[0].duration_beats),
        (0.0, 2.0)
    );
    assert_eq!(
        (fragments[1].position_beats, fragments[1].duration_beats),
        (4.0, 4.0)
    );
    assert_eq!(
        (
            fragments[0].notes[0].start_beat,
            fragments[0].notes[0].duration_beats
        ),
        (1.0, 1.0)
    );
    assert_eq!(
        (
            fragments[1].notes[0].start_beat,
            fragments[1].notes[0].duration_beats
        ),
        (0.0, 2.0)
    );
}

#[test]
fn trim_track_mutes_leaves_selected_clip_without_mute_automation_untouched() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 800);
    a.selected_clips
        .insert(ArrangementSelection::AudioClip { track_id, clip_id });
    let mut engine = RecordingEngine::default();

    let action = a.update(
        ArrangementMsg::TrimSelectedByTrackMutes,
        &mut engine,
        ArrangementCtx {
            samples_per_beat: 100.0,
            ..Default::default()
        },
    );

    assert_eq!(a.tracks[0].clips[0].id, clip_id);
    assert!(engine.0.is_empty());
    assert_eq!(
        action.status.as_deref(),
        Some("No selected clip material overlaps Track Mutes")
    );
}

#[test]
fn trim_track_mutes_ignores_redundant_unmuted_automation_points() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 800);
    let mut lane = AutomationLane::new(AutomationTarget::TrackMute);
    for beat in [2.5, 6.0] {
        lane.insert_point(AutomationPoint {
            beat,
            value: 0.0,
            curve: 0.0,
        });
    }
    a.tracks[0].automation.push(lane);
    a.selected_clips
        .insert(ArrangementSelection::AudioClip { track_id, clip_id });
    let mut engine = RecordingEngine::default();

    let action = a.update(
        ArrangementMsg::TrimSelectedByTrackMutes,
        &mut engine,
        ArrangementCtx {
            samples_per_beat: 100.0,
            ..Default::default()
        },
    );

    assert_eq!(a.tracks[0].clips.len(), 1);
    assert_eq!(a.tracks[0].clips[0].id, clip_id);
    assert!(engine.0.is_empty());
    assert_eq!(
        action.status.as_deref(),
        Some("No selected clip material overlaps Track Mutes")
    );
}

#[test]
fn split_looped_midi_materializes_both_looped_halves() {
    let mut a = arrangement_with_tracks(1);
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    let track_id = a.tracks[1].id;
    let clip_id = ClipId::new();
    a.tracks[1].note_clips.push(UiNoteClip {
        id: clip_id,
        name: "Pattern".to_string(),
        position_beats: 0.0,
        duration_beats: 8.0,
        notes: vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 2.0,
            duration_beats: 1.0,
        }],
        selected_notes: HashSet::new(),
        loop_enabled: true,
        loop_start_beats: 0.0,
        loop_end_beats: 4.0,
        groove_grid: vibez_core::perform::GrooveGrid::Sixteenth,
    });

    a.update(
        ArrangementMsg::SplitNoteClip {
            track_id,
            clip_id,
            split_beat: 5.0,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    let mut halves: Vec<_> = a.tracks[1].note_clips.iter().collect();
    halves.sort_by(|a, b| a.position_beats.partial_cmp(&b.position_beats).unwrap());
    assert!(halves.iter().all(|clip| clip.loop_enabled));
    assert_eq!(halves[0].loop_end_beats, 5.0);
    assert_eq!(halves[1].loop_end_beats, 3.0);
    assert_eq!(halves[1].notes[0].start_beat, 1.0);
}

#[test]
fn join_looped_audio_consolidates_wrapped_samples_and_remains_looped() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, first_id) = add_audio_clip(&mut a, 0, 0, 200);
    let (_, second_id) = add_audio_clip(&mut a, 0, 200, 100);
    let source = Arc::new(vibez_core::audio_buffer::DecodedAudio {
        channels: vec![(0..100).map(|frame| frame as f32).collect()],
        sample_rate: 44_100,
    });
    for clip in &mut a.tracks[0].clips {
        clip.audio = Arc::clone(&source);
        clip.loop_enabled = true;
        clip.loop_start = 0;
        clip.loop_end = 100;
    }
    for clip_id in [first_id, second_id] {
        a.selected_clips
            .insert(ArrangementSelection::AudioClip { track_id, clip_id });
    }
    let mut engine = RecordingEngine::default();

    a.update(
        ArrangementMsg::JoinSelectedClips,
        &mut engine,
        ArrangementCtx::default(),
    );

    let joined = &a.tracks[0].clips[0];
    assert!(joined.loop_enabled);
    assert_eq!((joined.loop_start, joined.loop_end), (0, 300));
    assert_eq!(joined.audio.channels[0][150], 50.0);
}

#[test]
fn join_looped_midi_expands_repetitions_and_remains_looped() {
    let mut a = arrangement_with_tracks(1);
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    let track_id = a.tracks[1].id;
    for position_beats in [0.0, 8.0] {
        let clip_id = ClipId::new();
        a.tracks[1].note_clips.push(UiNoteClip {
            id: clip_id,
            name: "Pattern".to_string(),
            position_beats,
            duration_beats: 8.0,
            notes: vec![MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 1.0,
            }],
            selected_notes: HashSet::new(),
            loop_enabled: true,
            loop_start_beats: 0.0,
            loop_end_beats: 4.0,
            groove_grid: vibez_core::perform::GrooveGrid::Sixteenth,
        });
        a.selected_clips
            .insert(ArrangementSelection::NoteClip { track_id, clip_id });
    }

    a.update(
        ArrangementMsg::JoinSelectedClips,
        &mut engine,
        ArrangementCtx::default(),
    );

    let joined = &a.tracks[1].note_clips[0];
    assert!(joined.loop_enabled);
    assert_eq!(joined.loop_end_beats, 16.0);
    let starts: Vec<_> = joined.notes.iter().map(|note| note.start_beat).collect();
    assert_eq!(starts, vec![0.0, 4.0, 8.0, 12.0]);
}

// ── Rubber-band (box) selection ──

/// Spans in beats at the fixture's 100 samples-per-beat.
fn marquee(
    a: &mut ArrangementFixture,
    engine: &mut RecordingEngine,
    start_beats: f64,
    end_beats: f64,
    tracks: &[usize],
    additive: bool,
) {
    let anchor = a.tracks[0].id;
    let track_ids: Vec<TrackId> = tracks.iter().map(|i| a.tracks[*i].id).collect();
    a.update(
        ArrangementMsg::MarqueeSelect {
            anchor_track: anchor,
            start_beats,
            end_beats,
            top_y: 0.0,
            bottom_y: 70.0 * tracks.len() as f32,
            track_ids,
            additive,
        },
        engine,
        ArrangementCtx {
            samples_per_beat: 100.0,
            playhead_samples: 0,
            playhead_beats: 0.0,
        },
    );
}

#[test]
fn a_box_selects_every_clip_it_overlaps_across_the_lanes_it_spans() {
    let mut a = arrangement_with_tracks(3);
    // Beats 0..1 on track 0, beats 2..3 on track 1, beats 0..1 on track 2.
    let (t0, first) = add_audio_clip(&mut a, 0, 0, 100);
    let (t1, second) = add_audio_clip(&mut a, 1, 200, 100);
    let (_, third) = add_audio_clip(&mut a, 2, 0, 100);
    let mut engine = RecordingEngine::default();

    // A box over beats 0..2.5 covering only the first two lanes.
    marquee(&mut a, &mut engine, 0.0, 2.5, &[0, 1], false);

    assert!(a.selected_clips.contains(&ArrangementSelection::AudioClip {
        track_id: t0,
        clip_id: first,
    }));
    assert!(a.selected_clips.contains(&ArrangementSelection::AudioClip {
        track_id: t1,
        clip_id: second,
    }));
    // Track 2 was outside the box vertically even though the clip's beats
    // fall inside it.
    assert!(!a
        .selected_clips
        .iter()
        .any(|selection| matches!(selection, ArrangementSelection::AudioClip { clip_id, .. } if *clip_id == third)));
}

#[test]
fn a_clip_only_clipped_by_the_edge_of_the_box_still_counts() {
    let mut a = arrangement_with_tracks(1);
    // Beats 2..6.
    let (tid, clip_id) = add_audio_clip(&mut a, 0, 200, 400);
    let mut engine = RecordingEngine::default();

    // Box ends inside the clip. Overlap is what reads as "caught" on
    // screen, unlike the region ops which demand full containment.
    marquee(&mut a, &mut engine, 0.0, 3.0, &[0], false);

    assert!(a.selected_clips.contains(&ArrangementSelection::AudioClip {
        track_id: tid,
        clip_id,
    }));
}

#[test]
fn shrinking_a_shift_drag_gives_back_clips_it_no_longer_covers() {
    let mut a = arrangement_with_tracks(1);
    let (tid, early) = add_audio_clip(&mut a, 0, 0, 100);
    let (_, late) = add_audio_clip(&mut a, 0, 500, 100);
    let mut engine = RecordingEngine::default();

    // Pre-select the late clip, then shift-drag a box over the early one.
    a.selected_clips.insert(ArrangementSelection::AudioClip {
        track_id: tid,
        clip_id: late,
    });
    marquee(&mut a, &mut engine, 0.0, 4.0, &[0], true);
    assert_eq!(a.selected_clips.len(), 2);

    // Same gesture, box now shrunk off the early clip. The additive base
    // is the pre-drag selection, so the early clip is released rather
    // than accumulating for the rest of the drag.
    marquee(&mut a, &mut engine, 3.0, 4.0, &[0], true);

    assert_eq!(
        a.selected_clips,
        [ArrangementSelection::AudioClip {
            track_id: tid,
            clip_id: late,
        }]
        .into_iter()
        .collect()
    );
    assert!(
        !a.selected_clips.contains(&ArrangementSelection::AudioClip {
            track_id: tid,
            clip_id: early,
        })
    );
}

#[test]
fn a_horizontal_box_also_sets_the_time_selection_but_a_vertical_one_does_not() {
    let mut a = arrangement_with_tracks(2);
    add_audio_clip(&mut a, 0, 0, 100);
    let mut engine = RecordingEngine::default();

    marquee(&mut a, &mut engine, 1.0, 5.0, &[0, 1], false);
    assert!(a.time_selection_active);
    assert_eq!(a.selection_start_beats, 1.0);
    assert_eq!(a.selection_end_beats, 5.0);

    // A drag straight down has no range to select, so it must not wipe
    // the range the user already had.
    marquee(&mut a, &mut engine, 2.0, 2.0, &[0, 1], false);
    assert_eq!(a.selection_start_beats, 1.0);
    assert_eq!(a.selection_end_beats, 5.0);
}

#[test]
fn ending_the_drag_drops_the_box_but_keeps_the_selection() {
    let mut a = arrangement_with_tracks(1);
    let (tid, clip_id) = add_audio_clip(&mut a, 0, 0, 100);
    let mut engine = RecordingEngine::default();

    marquee(&mut a, &mut engine, 0.0, 4.0, &[0], false);
    assert!(a.marquee.is_some());

    a.update(
        ArrangementMsg::EndMarqueeSelect,
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(a.marquee.is_none());
    assert!(a.selected_clips.contains(&ArrangementSelection::AudioClip {
        track_id: tid,
        clip_id,
    }));
}

#[test]
fn selecting_a_clip_replaces_a_stale_time_range_before_split_and_delete() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 800);
    let mut engine = RecordingEngine::default();
    let ctx = ArrangementCtx {
        samples_per_beat: 100.0,
        playhead_samples: 300,
        playhead_beats: 3.0,
    };

    a.time_selection_active = true;
    a.selection_start_beats = 2.0;
    a.selection_end_beats = 5.0;
    a.time_selection_track = Some(track_id);

    a.update(
        ArrangementMsg::SelectArrangementClip {
            selection: ArrangementSelection::AudioClip { track_id, clip_id },
            shift_held: false,
        },
        &mut engine,
        ctx,
    );
    a.update(ArrangementMsg::SplitSelectedAtPlayhead, &mut engine, ctx);

    assert!(!a.time_selection_active);
    assert_eq!(a.time_selection_track, None);
    assert_eq!(a.tracks[0].clips.len(), 2);

    a.update(ArrangementMsg::DeleteSelectedClip, &mut engine, ctx);

    assert_eq!(a.tracks[0].clips.len(), 1);
    assert_eq!(a.tracks[0].clips[0].position, 300);
    assert!(a.selected_clips.is_empty());
}

#[test]
fn select_all_clips_takes_every_clip_on_every_track() {
    let mut a = arrangement_with_tracks(2);
    let (t0, first) = add_audio_clip(&mut a, 0, 0, 100);
    let (_, second) = add_audio_clip(&mut a, 0, 400, 100);
    let (t1, third) = add_audio_clip(&mut a, 1, 200, 100);
    let mut engine = RecordingEngine::default();

    a.update(
        ArrangementMsg::SelectAllClips,
        &mut engine,
        ArrangementCtx::default(),
    );

    assert_eq!(a.selected_clips.len(), 3);
    for (track_id, clip_id) in [(t0, first), (t0, second), (t1, third)] {
        assert!(a
            .selected_clips
            .contains(&ArrangementSelection::AudioClip { track_id, clip_id }));
    }
}

#[test]
fn select_all_replaces_a_stale_time_range_before_split() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, _) = add_audio_clip(&mut a, 0, 0, 800);
    let mut engine = RecordingEngine::default();
    let ctx = ArrangementCtx {
        samples_per_beat: 100.0,
        playhead_samples: 300,
        playhead_beats: 3.0,
    };
    a.time_selection_active = true;
    a.selection_start_beats = 2.0;
    a.selection_end_beats = 5.0;
    a.time_selection_track = Some(track_id);

    a.update(ArrangementMsg::SelectAllClips, &mut engine, ctx);
    a.update(ArrangementMsg::SplitSelectedAtPlayhead, &mut engine, ctx);

    assert!(!a.time_selection_active);
    assert_eq!(a.time_selection_track, None);
    assert_eq!(a.tracks[0].clips.len(), 2);
    assert_eq!(a.tracks[0].clips[0].duration, 300);
    assert_eq!(a.tracks[0].clips[1].position, 300);
}

#[test]
fn select_all_clips_on_an_empty_timeline_selects_nothing() {
    let mut a = arrangement_with_tracks(2);
    let stale_track = a.tracks[0].id;
    a.selected_clips.insert(ArrangementSelection::AudioClip {
        track_id: stale_track,
        clip_id: ClipId::new(),
    });
    let mut engine = RecordingEngine::default();

    let action = a.update(
        ArrangementMsg::SelectAllClips,
        &mut engine,
        ArrangementCtx::default(),
    );

    // The stale selection is replaced, not extended, and an empty result
    // must not pull focus to a clip tab with nothing to show.
    assert!(a.selected_clips.is_empty());
    assert!(!action.focus_clip_tab);
}
