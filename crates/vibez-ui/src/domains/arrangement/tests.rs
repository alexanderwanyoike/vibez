//! Arrangement domain unit tests.

use std::sync::Arc;

use super::test_support::*;
use super::*;
use crate::domains::test_support::RecordingEngine;
use crate::state::{AudioClipInspectorField, UiClip};
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::midi::MidiNote;
use vibez_core::track::{AudioInputRoute, ClipPlaybackDirection};
use vibez_core::transient::{TransientMarker, TransientMarkerKind};

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
        start_marker: 0,
        duration,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
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
fn audio_loop_region_must_be_ordered_and_inside_the_visible_clip() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 1_000);
    arrangement.tracks[0].clips[0].duration = 400;
    let mut engine = RecordingEngine::default();

    arrangement.update(
        ArrangementMsg::SetClipLoopRegion {
            track_id,
            clip_id,
            loop_start: 200,
            loop_end: 800,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(
        (
            arrangement.tracks[0].clips[0].loop_start,
            arrangement.tracks[0].clips[0].loop_end
        ),
        (0, 0)
    );
    assert!(engine.0.is_empty());

    arrangement.update(
        ArrangementMsg::SetClipLoopRegion {
            track_id,
            clip_id,
            loop_start: 200,
            loop_end: 400,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(
        (
            arrangement.tracks[0].clips[0].loop_start,
            arrangement.tracks[0].clips[0].loop_end
        ),
        (200, 400)
    );
    assert!(matches!(
        engine.0.as_slice(),
        [EngineCommand::SetClipLoop { .. }]
    ));

    engine.0.clear();
    arrangement.update(
        ArrangementMsg::SetClipLoopRegion {
            track_id,
            clip_id,
            loop_start: 200,
            loop_end: 400,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(engine.0.is_empty());
}

#[test]
fn split_audio_clip_replaces_clip_with_two_halves() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1000);
    let mut engine = RecordingEngine::default();
    a.tracks[0].clips[0].fades =
        vibez_core::track::ClipFades::new(100, 200, 1000).linked_fade_out(200, ClipId::new(), 1000);
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
    assert_eq!(a.tracks[0].clips[0].fades.fade_in_frames(), 100);
    assert_eq!(a.tracks[0].clips[0].fades.fade_out_frames(), 0);
    assert_eq!(a.tracks[0].clips[1].fades.fade_in_frames(), 0);
    assert_eq!(a.tracks[0].clips[1].fades.fade_out_frames(), 200);
    assert!(a.tracks[0].clips[0].fades.crossfade_out_to().is_none());
    assert!(a.tracks[0].clips[1].fades.crossfade_out_to().is_none());
    assert!(engine
        .0
        .iter()
        .any(|command| matches!(command, EngineCommand::RemoveClip(..))));
}

#[test]
fn reverse_toggle_updates_the_clip_and_engine_without_touching_its_audio() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 1000);
    let original_audio = Arc::clone(&a.tracks[0].clips[0].audio);
    let mut engine = RecordingEngine::default();

    a.update(
        ArrangementMsg::ToggleClipReverse(track_id, clip_id),
        &mut engine,
        ArrangementCtx::default(),
    );

    assert_eq!(
        a.tracks[0].clips[0].playback_direction,
        ClipPlaybackDirection::Reverse
    );
    assert!(Arc::ptr_eq(&original_audio, &a.tracks[0].clips[0].audio));
    assert!(matches!(
        engine.0.last(),
        Some(EngineCommand::SetClipPlaybackDirection {
            track_id: command_track,
            clip_id: command_clip,
            direction: ClipPlaybackDirection::Reverse,
        }) if *command_track == track_id && *command_clip == clip_id
    ));

    a.update(
        ArrangementMsg::ToggleClipReverse(track_id, clip_id),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(
        a.tracks[0].clips[0].playback_direction,
        ClipPlaybackDirection::Forward
    );
}

#[test]
fn transient_marker_edits_are_explicit_clamped_and_do_not_touch_audio() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 1000);
    a.tracks[0].clips[0].source_offset = 100;
    let original_audio = Arc::clone(&a.tracks[0].clips[0].audio);
    let mut engine = RecordingEngine::default();

    let added = a.update(
        ArrangementMsg::AddTransientMarker {
            track_id,
            clip_id,
            source_frame: 10,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(added.mark_dirty);
    assert_eq!(
        a.tracks[0].clips[0].transient_markers.as_slice(),
        &[TransientMarker::new(100, TransientMarkerKind::Authored)]
    );
    assert_eq!(a.selected_transient_marker, Some((track_id, clip_id, 100)));
    assert!(Arc::ptr_eq(&original_audio, &a.tracks[0].clips[0].audio));
    assert!(engine.0.is_empty());

    let moved = a.update(
        ArrangementMsg::MoveTransientMarker {
            track_id,
            clip_id,
            from: 100,
            to: 240,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(moved.mark_dirty);
    assert_eq!(
        a.tracks[0].clips[0].transient_markers.as_slice(),
        &[TransientMarker::new(240, TransientMarkerKind::Authored)]
    );

    let removed = a.update(
        ArrangementMsg::RemoveTransientMarker {
            track_id,
            clip_id,
            source_frame: 240,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(removed.mark_dirty);
    assert!(a.tracks[0].clips[0].transient_markers.is_empty());
    assert_eq!(a.selected_transient_marker, None);
}

#[test]
fn detection_replaces_suggestions_but_preserves_authored_transient_markers() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 1000);
    let markers = &mut a.tracks[0].clips[0].transient_markers;
    markers.add_authored(250);
    markers.replace_suggestions([100, 400]);
    let mut engine = RecordingEngine::default();

    let action = a.update(
        ArrangementMsg::ReplaceDetectedTransientMarkers {
            track_id,
            clip_id,
            source_frames: vec![700, 250, 300, 300],
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(action.mark_dirty);
    assert_eq!(
        a.tracks[0].clips[0].transient_markers.as_slice(),
        &[
            TransientMarker::new(250, TransientMarkerKind::Authored),
            TransientMarker::new(300, TransientMarkerKind::Suggested),
            TransientMarker::new(700, TransientMarkerKind::Suggested),
        ]
    );
    assert!(engine.0.is_empty());
}

#[test]
fn reverse_changes_marker_display_order_without_changing_source_positions() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 1000);
    a.tracks[0].clips[0]
        .transient_markers
        .replace_suggestions([100, 900]);
    let before = a.tracks[0].clips[0].transient_markers.clone();
    let mut engine = RecordingEngine::default();

    a.update(
        ArrangementMsg::ToggleClipReverse(track_id, clip_id),
        &mut engine,
        ArrangementCtx::default(),
    );

    assert_eq!(a.tracks[0].clips[0].transient_markers, before);
}

#[test]
fn splitting_a_reversed_clip_preserves_both_audible_fragments() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 1000);
    let original = &mut a.tracks[0].clips[0];
    original.playback_direction = ClipPlaybackDirection::Reverse;
    original
        .transient_markers
        .replace_suggestions([100, 650, 900]);
    let expected: Vec<_> = (0..original.duration)
        .map(|frame| original.source_frame_at(frame))
        .collect();
    let mut engine = RecordingEngine::default();

    a.update(
        ArrangementMsg::SplitAudioClip {
            track_id,
            clip_id,
            split_position: 400,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    let mut halves: Vec<_> = a.tracks[0].clips.iter().collect();
    halves.sort_by_key(|clip| clip.position);
    let actual: Vec<_> = halves
        .iter()
        .flat_map(|clip| (0..clip.duration).map(|frame| clip.source_frame_at(frame)))
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(halves[0].source_offset, 600);
    assert_eq!(halves[1].source_offset, 0);
    assert_eq!(
        halves[0]
            .transient_markers
            .as_slice()
            .iter()
            .map(|marker| marker.source_frame())
            .collect::<Vec<_>>(),
        vec![650, 900]
    );
    assert_eq!(
        halves[1]
            .transient_markers
            .as_slice()
            .iter()
            .map(|marker| marker.source_frame())
            .collect::<Vec<_>>(),
        vec![100]
    );
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
fn two_selected_edge_overlaps_create_one_equal_power_crossfade() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, outgoing_id) = add_audio_clip(&mut a, 0, 0, 1_000);
    let (_, incoming_id) = add_audio_clip(&mut a, 0, 750, 1_000);
    a.selected_clips = HashSet::from([
        ArrangementSelection::AudioClip {
            track_id,
            clip_id: outgoing_id,
        },
        ArrangementSelection::AudioClip {
            track_id,
            clip_id: incoming_id,
        },
    ]);
    let mut engine = RecordingEngine::default();

    let action = a.update(
        ArrangementMsg::CrossfadeSelectedAudioClips,
        &mut engine,
        ArrangementCtx::default(),
    );

    let outgoing = a.tracks[0]
        .clips
        .iter()
        .find(|clip| clip.id == outgoing_id)
        .unwrap();
    let incoming = a.tracks[0]
        .clips
        .iter()
        .find(|clip| clip.id == incoming_id)
        .unwrap();
    assert_eq!(
        action.status.as_deref(),
        Some("Created equal-power crossfade")
    );
    assert_eq!(outgoing.fades.fade_out_frames(), 250);
    assert_eq!(outgoing.fades.crossfade_out_to(), Some(incoming_id));
    assert_eq!(incoming.fades.fade_in_frames(), 250);
    assert_eq!(incoming.fades.crossfade_in_from(), Some(outgoing_id));
    assert_eq!(
        engine
            .0
            .iter()
            .filter(|command| matches!(command, EngineCommand::SetClipFades { .. }))
            .count(),
        2
    );
}

#[test]
fn moving_one_crossfaded_clip_unlinks_both_edges_without_losing_fade_lengths() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, outgoing_id) = add_audio_clip(&mut a, 0, 0, 1_000);
    let (_, incoming_id) = add_audio_clip(&mut a, 0, 750, 1_000);
    a.selected_clips = HashSet::from([
        ArrangementSelection::AudioClip {
            track_id,
            clip_id: outgoing_id,
        },
        ArrangementSelection::AudioClip {
            track_id,
            clip_id: incoming_id,
        },
    ]);
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::CrossfadeSelectedAudioClips,
        &mut engine,
        ArrangementCtx::default(),
    );
    engine.0.clear();

    a.update(
        ArrangementMsg::MoveAudioClip {
            track_id,
            clip_id: incoming_id,
            new_position: 1_100,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    let outgoing = a.tracks[0]
        .clips
        .iter()
        .find(|clip| clip.id == outgoing_id)
        .unwrap();
    let incoming = a.tracks[0]
        .clips
        .iter()
        .find(|clip| clip.id == incoming_id)
        .unwrap();
    assert_eq!(outgoing.fades.fade_out_frames(), 250);
    assert_eq!(incoming.fades.fade_in_frames(), 250);
    assert!(outgoing.fades.crossfade_out_to().is_none());
    assert!(incoming.fades.crossfade_in_from().is_none());
    assert!(matches!(
        engine.0.last(),
        Some(EngineCommand::MoveClip { .. })
    ));
}

#[test]
fn one_clip_can_keep_independent_crossfades_on_both_edges() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, first_id) = add_audio_clip(&mut a, 0, 0, 1_000);
    let (_, middle_id) = add_audio_clip(&mut a, 0, 750, 1_000);
    let (_, last_id) = add_audio_clip(&mut a, 0, 1_500, 1_000);
    let mut engine = RecordingEngine::default();
    let selection = |left, right| {
        HashSet::from([
            ArrangementSelection::AudioClip {
                track_id,
                clip_id: left,
            },
            ArrangementSelection::AudioClip {
                track_id,
                clip_id: right,
            },
        ])
    };

    a.selected_clips = selection(first_id, middle_id);
    a.update(
        ArrangementMsg::CrossfadeSelectedAudioClips,
        &mut engine,
        ArrangementCtx::default(),
    );
    a.selected_clips = selection(middle_id, last_id);
    a.update(
        ArrangementMsg::CrossfadeSelectedAudioClips,
        &mut engine,
        ArrangementCtx::default(),
    );

    let middle = a.tracks[0]
        .clips
        .iter()
        .find(|clip| clip.id == middle_id)
        .unwrap();
    assert_eq!(middle.fades.crossfade_in_from(), Some(first_id));
    assert_eq!(middle.fades.crossfade_out_to(), Some(last_id));
}

#[test]
fn unchanged_fade_drag_keeps_its_crossfade_link() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, outgoing_id) = add_audio_clip(&mut a, 0, 0, 1_000);
    let (_, incoming_id) = add_audio_clip(&mut a, 0, 750, 1_000);
    let mut engine = RecordingEngine::default();
    a.selected_clips = HashSet::from([
        ArrangementSelection::AudioClip {
            track_id,
            clip_id: outgoing_id,
        },
        ArrangementSelection::AudioClip {
            track_id,
            clip_id: incoming_id,
        },
    ]);
    a.update(
        ArrangementMsg::CrossfadeSelectedAudioClips,
        &mut engine,
        ArrangementCtx::default(),
    );
    engine.0.clear();

    let action = a.update(
        ArrangementMsg::SetAudioClipFade {
            track_id,
            clip_id: outgoing_id,
            edge: crate::state::AudioClipFadeEdge::Out,
            frames: 250,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(!action.mark_dirty);
    assert!(engine.0.is_empty());
    assert_eq!(
        a.tracks[0].clips[0].fades.crossfade_out_to(),
        Some(incoming_id)
    );
    assert_eq!(
        a.tracks[0].clips[1].fades.crossfade_in_from(),
        Some(outgoing_id)
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
        new_start_marker: 0,
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
    Arc::make_mut(&mut a.arrangement.timeline)
        .get_mut(tid)
        .unwrap()
        .clips[0]
        .transient_markers
        .replace_suggestions([250, 750]);
    let original = Arc::clone(&a.arrangement.timeline.get(tid).unwrap().clips[0].audio);
    let mut engine = RecordingEngine::default();

    let action =
        a.apply_clip_warp_success(&mut engine, tid, cid, warp_success(Arc::clone(&original)));
    let clip = &a.arrangement.timeline.get(tid).unwrap().clips[0];
    assert!(clip.warped);
    assert_eq!(clip.duration, 2000);
    assert_eq!(clip.warped_to_bpm, Some(120.0));
    assert_eq!(clip.original_bpm, Some(128.0));
    assert!(clip.original_audio.is_some());
    assert_eq!(
        clip.transient_markers
            .as_slice()
            .iter()
            .map(|marker| marker.source_frame())
            .collect::<Vec<_>>(),
        vec![500, 1500]
    );
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
            expected_audio: request.expected_audio,
            expected_geometry: request.expected_geometry,
            geometry: request.geometry,
            warning: None,
        },
    );
    let clip = &a.arrangement.timeline.get(tid).unwrap().clips[0];
    assert!(!clip.warped);
    assert_eq!(clip.duration, 1000);
    assert!(clip.original_audio.is_none());
    assert!(Arc::ptr_eq(&clip.audio, &original));
    assert_eq!(
        clip.transient_markers
            .as_slice()
            .iter()
            .map(|marker| marker.source_frame())
            .collect::<Vec<_>>(),
        vec![250, 750]
    );
    assert!(action.mark_dirty);
}

fn completed_transpose_request(
    request: ClipTransposeRenderRequest,
) -> crate::message::ClipTransposeSuccess {
    crate::message::ClipTransposeSuccess {
        audio: Arc::clone(&request.source_audio),
        source_audio: request.source_audio,
        transpose: request.transpose,
        expected_warped: request.expected_warped,
        expected_audio: request.expected_audio,
        expected_geometry: request.expected_geometry,
        geometry: request.geometry,
        warning: None,
    }
}

#[test]
fn transpose_result_requeues_after_warp_wins_the_race() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1_000);
    let original = Arc::clone(&a.tracks[0].clips[0].audio);
    let mut engine = RecordingEngine::default();

    let transpose = a.set_audio_clip_rotary_value(
        &mut engine,
        tid,
        cid,
        crate::state::AudioClipRotaryField::Transpose,
        5.0,
    );
    let old_request = transpose
        .transpose_render
        .expect("initial Transpose render");
    a.apply_clip_warp_success(&mut engine, tid, cid, warp_success(original));
    let warped_audio = Arc::clone(&a.arrangement.timeline.get(tid).unwrap().clips[0].audio);

    let stale = a.apply_clip_transpose_success(
        &mut engine,
        tid,
        cid,
        completed_transpose_request(old_request),
    );

    assert!(Arc::ptr_eq(
        &a.arrangement.timeline.get(tid).unwrap().clips[0].audio,
        &warped_audio
    ));
    let refreshed = stale
        .transpose_render
        .expect("stale result should rebuild from current Warp state");
    assert!(refreshed.expected_warped);
    assert_eq!(refreshed.transpose.semitones(), 5);
    assert_eq!(refreshed.target_frames, 2_000);
}

#[test]
fn clear_warp_result_requeues_without_overwriting_new_geometry() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1_000);
    let original = Arc::clone(&a.arrangement.timeline.get(tid).unwrap().clips[0].audio);
    let mut engine = RecordingEngine::default();
    a.apply_clip_warp_success(&mut engine, tid, cid, warp_success(original));
    let clear_request = a
        .apply_clear_clip_warp(&mut engine, tid, cid)
        .transpose_render
        .expect("clear Warp render");
    Arc::make_mut(&mut a.arrangement.timeline)
        .get_mut(tid)
        .unwrap()
        .clips[0]
        .duration = 1_250;

    let stale = a.apply_clip_transpose_success(
        &mut engine,
        tid,
        cid,
        completed_transpose_request(clear_request),
    );

    assert_eq!(
        a.arrangement.timeline.get(tid).unwrap().clips[0].duration,
        1_250
    );
    let refreshed = stale
        .transpose_render
        .expect("geometry edit should rebuild clear-Warp render");
    assert_eq!(refreshed.expected_geometry.unwrap().duration, 1_250);
    assert_eq!(refreshed.geometry.unwrap().duration, 625);
}

#[test]
fn inspector_gain_and_source_bounds_reach_the_resident_clip() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 44_100);
    a.tracks[0].clips[0]
        .transient_markers
        .replace_suggestions([5_000, 20_000, 44_100]);
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
    assert_eq!(
        clip.transient_markers
            .as_slice()
            .iter()
            .map(|marker| marker.source_frame())
            .collect::<Vec<_>>(),
        vec![20_000, 44_100]
    );
    assert!(engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::SetClipBounds {
            source_offset: 11_025,
            start_marker: 11_025,
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
fn inspector_fades_commit_in_frames_and_clamp_as_one_pair() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 44_100);
    let mut engine = RecordingEngine::default();

    a.audio_clip_inspector_edits
        .insert((cid, AudioClipInspectorField::FadeIn), "0.750".into());
    let action = a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::FadeIn,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].fades.fade_in_frames(), 33_075);
    assert!(matches!(
        engine.0.last(),
        Some(EngineCommand::SetClipFades { fades, .. })
            if fades.fade_in_frames() == 33_075 && fades.fade_out_frames() == 0
    ));

    a.audio_clip_inspector_edits
        .insert((cid, AudioClipInspectorField::FadeOut), "0.750".into());
    a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::FadeOut,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    let fades = a.tracks[0].clips[0].fades;
    assert_eq!(fades.fade_in_frames(), 11_025);
    assert_eq!(fades.fade_out_frames(), 33_075);
    assert_eq!(fades.fade_in_frames() + fades.fade_out_frames(), 44_100);
}

#[test]
fn unchanged_timeline_fade_is_not_an_edit() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 44_100);
    a.tracks[0].clips[0].fades = vibez_core::track::ClipFades::new(11_025, 0, 44_100);
    let mut engine = RecordingEngine::default();

    let action = a.update(
        ArrangementMsg::SetAudioClipFade {
            track_id,
            clip_id,
            edge: crate::state::AudioClipFadeEdge::In,
            frames: 11_025,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(!action.mark_dirty);
    assert!(engine.0.is_empty());
    assert!(ArrangementMsg::SetAudioClipFade {
        track_id,
        clip_id,
        edge: crate::state::AudioClipFadeEdge::In,
        frames: 11_025,
    }
    .defers_project_edit());
}

#[test]
fn inspector_knobs_commit_gain_and_rounded_transpose_values() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 44_100);
    let mut engine = RecordingEngine::default();

    let gain_action = a.update(
        ArrangementMsg::SetAudioClipRotaryValue {
            track_id: tid,
            clip_id: cid,
            field: crate::state::AudioClipRotaryField::Gain,
            value: -6.25,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(gain_action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].gain_db.db(), -6.25);
    assert!(matches!(
        engine.0.last(),
        Some(EngineCommand::SetClipGain { track_id, clip_id, .. })
            if *track_id == tid && *clip_id == cid
    ));

    let transpose_action = a.update(
        ArrangementMsg::SetAudioClipRotaryValue {
            track_id: tid,
            clip_id: cid,
            field: crate::state::AudioClipRotaryField::Transpose,
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
    assert_eq!(
        a.audio_clip_inspector_edits
            .get(&(cid, AudioClipInspectorField::SourceEnd))
            .map(String::as_str),
        Some("2.0"),
        "rejected text remains available for correction"
    );
}

#[test]
fn clip_start_moves_without_changing_the_loop_region() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 44_100);
    let clip = &mut arrangement.tracks[0].clips[0];
    clip.loop_enabled = true;
    clip.loop_start = 11_025;
    clip.loop_end = 44_100;
    let mut engine = RecordingEngine::default();
    arrangement
        .audio_clip_inspector_edits
        .insert((clip_id, AudioClipInspectorField::Start), "0.500".into());

    let action = arrangement.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id,
            clip_id,
            field: AudioClipInspectorField::Start,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(action.mark_dirty);
    let clip = &arrangement.tracks[0].clips[0];
    assert_eq!(clip.start_marker, 22_050);
    assert_eq!((clip.loop_start, clip.loop_end), (11_025, 44_100));
    assert!(matches!(
        engine.0.last(),
        Some(EngineCommand::SetClipStartMarker {
            track_id: sent_track,
            clip_id: sent_clip,
            start_marker: 22_050,
        }) if *sent_track == track_id && *sent_clip == clip_id
    ));
}

#[test]
fn loop_fields_seed_an_uninitialised_pair_from_source_bounds() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 44_100);
    let clip = &mut a.tracks[0].clips[0];
    clip.source_offset = 4_410;
    clip.duration = 39_690;
    clip.loop_start = 0;
    clip.loop_end = 0;
    let mut engine = RecordingEngine::default();

    a.audio_clip_inspector_edits
        .insert((cid, AudioClipInspectorField::LoopStart), "0.250".into());
    let start_action = a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::LoopStart,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(start_action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].loop_start, 11_025);
    assert_eq!(a.tracks[0].clips[0].loop_end, 44_100);

    a.tracks[0].clips[0].loop_start = 0;
    a.tracks[0].clips[0].loop_end = 0;
    a.audio_clip_inspector_edits
        .insert((cid, AudioClipInspectorField::LoopEnd), "0.750".into());
    let end_action = a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::LoopEnd,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(end_action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].loop_start, 4_410);
    assert_eq!(a.tracks[0].clips[0].loop_end, 33_075);
}

#[test]
fn typed_transpose_rounds_fractional_semitones() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 44_100);
    let mut engine = RecordingEngine::default();
    a.audio_clip_inspector_edits
        .insert((cid, AudioClipInspectorField::Transpose), "-1.5".into());

    let action = a.update(
        ArrangementMsg::SubmitAudioClipInspectorField {
            track_id: tid,
            clip_id: cid,
            field: AudioClipInspectorField::Transpose,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    assert_eq!(a.tracks[0].clips[0].transpose.semitones(), -2);
    assert_eq!(
        action
            .transpose_render
            .expect("fractional Transpose should render")
            .transpose
            .semitones(),
        -2
    );
}

#[test]
fn transpose_wheel_preview_defers_render_until_settle() {
    let mut a = arrangement_with_tracks(1);
    let (tid, cid) = add_audio_clip(&mut a, 0, 0, 44_100);

    let action = a.preview_audio_clip_rotary_value(
        tid,
        cid,
        crate::state::AudioClipRotaryField::Transpose,
        7.6,
    );

    assert!(!action.mark_dirty);
    assert!(action.transpose_render.is_none());
    assert_eq!(action.transpose_debounce, Some((tid, cid, 8, 1)));
    assert_eq!(
        a.audio_clip_inspector_edits
            .get(&(cid, AudioClipInspectorField::Transpose))
            .map(String::as_str),
        Some("8")
    );

    let second = a.preview_audio_clip_rotary_value(
        tid,
        cid,
        crate::state::AudioClipRotaryField::Transpose,
        9.0,
    );
    let revisited = a.preview_audio_clip_rotary_value(
        tid,
        cid,
        crate::state::AudioClipRotaryField::Transpose,
        8.0,
    );
    assert_eq!(second.transpose_debounce, Some((tid, cid, 9, 2)));
    assert_eq!(revisited.transpose_debounce, Some((tid, cid, 8, 3)));
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
        start_marker_beats: 0.0,
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
fn selected_midi_loop_activation_replaces_stale_bounds_with_the_clip_length() {
    let mut arrangement = arrangement_with_tracks(1);
    let mut engine = RecordingEngine::default();
    arrangement.update(
        ArrangementMsg::AddMidiTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    let track_id = arrangement.tracks[1].id;
    let clip_id = ClipId::new();
    arrangement.tracks[1].note_clips.push(UiNoteClip {
        id: clip_id,
        name: "Two bars".to_string(),
        position_beats: 0.0,
        duration_beats: 8.0,
        notes: vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 6.0,
            duration_beats: 0.5,
        }],
        selected_notes: HashSet::new(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 4.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    });
    arrangement
        .selected_clips
        .insert(ArrangementSelection::NoteClip { track_id, clip_id });
    engine.0.clear();

    arrangement.update(
        ArrangementMsg::ToggleSelectedClipLoop,
        &mut engine,
        ArrangementCtx::default(),
    );

    let clip = &arrangement.tracks[1].note_clips[0];
    assert!(clip.loop_enabled);
    assert_eq!((clip.loop_start_beats, clip.loop_end_beats), (0.0, 8.0));
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
fn enabling_a_mixed_loop_selection_preserves_existing_regions() {
    let mut arrangement = arrangement_with_tracks(1);
    let mut engine = RecordingEngine::default();
    arrangement.update(
        ArrangementMsg::AddMidiTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    let track_id = arrangement.tracks[1].id;
    let existing_id = ClipId::new();
    let disabled_id = ClipId::new();
    for (id, loop_enabled, loop_start_beats, loop_end_beats) in [
        (existing_id, true, 2.0, 6.0),
        (disabled_id, false, 0.0, 0.0),
    ] {
        arrangement.tracks[1].note_clips.push(UiNoteClip {
            id,
            name: "Pattern".to_string(),
            position_beats: 0.0,
            duration_beats: 8.0,
            notes: Vec::new(),
            selected_notes: HashSet::new(),
            start_marker_beats: 0.0,
            loop_enabled,
            loop_start_beats,
            loop_end_beats,
            groove_grid: vibez_core::perform::GrooveGrid::Off,
        });
        arrangement
            .selected_clips
            .insert(ArrangementSelection::NoteClip {
                track_id,
                clip_id: id,
            });
    }

    arrangement.update(
        ArrangementMsg::ToggleSelectedClipLoop,
        &mut engine,
        ArrangementCtx::default(),
    );

    let content = &arrangement.tracks[1];
    let existing = content
        .note_clips
        .iter()
        .find(|clip| clip.id == existing_id)
        .unwrap();
    let enabled = content
        .note_clips
        .iter()
        .find(|clip| clip.id == disabled_id)
        .unwrap();
    assert_eq!(
        (existing.loop_start_beats, existing.loop_end_beats),
        (2.0, 6.0)
    );
    assert_eq!(
        (enabled.loop_start_beats, enabled.loop_end_beats),
        (0.0, 8.0)
    );
}

#[test]
fn extending_audio_does_not_enable_looping_implicitly() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 200);
    let mut engine = RecordingEngine::default();

    arrangement.update(
        ArrangementMsg::ResizeAudioClip {
            track_id,
            clip_id,
            new_duration: 400,
        },
        &mut engine,
        ArrangementCtx {
            samples_per_beat: 100.0,
            ..Default::default()
        },
    );

    let clip = &arrangement.tracks[0].clips[0];
    assert_eq!(clip.duration, 400);
    assert!(!clip.loop_enabled);
    assert_eq!((clip.loop_start, clip.loop_end), (0, 0));
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
        start_marker_beats: 0.0,
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
            start_marker_beats: 0.0,
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
        start_marker_beats: 0.0,
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
fn split_looped_reverse_audio_preserves_the_complete_audible_sequence() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 300);
    let clip = &mut a.tracks[0].clips[0];
    clip.loop_enabled = true;
    clip.loop_start = 0;
    clip.loop_end = 100;
    clip.playback_direction = ClipPlaybackDirection::Reverse;
    let expected: Vec<_> = (0..clip.duration)
        .map(|frame| clip.source_frame_at(frame))
        .collect();
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
    let actual: Vec<_> = halves
        .iter()
        .flat_map(|clip| (0..clip.duration).map(|frame| clip.source_frame_at(frame)))
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(halves[0].start_marker, 50);
    assert_eq!(halves[1].start_marker, 0);
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
        start_marker_beats: 0.0,
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
        start_marker_beats: 0.0,
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
            start_marker_beats: 0.0,
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
