use super::*;

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
fn warp_marker_edits_keep_fixed_boundaries_and_sync_the_engine() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 1_000);
    let mut engine = RecordingEngine::default();

    let added = arrangement.update(
        ArrangementMsg::AddWarpMarker {
            track_id,
            clip_id,
            source_frame: 250,
            timeline_frame: 250,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(added.mark_dirty);
    assert_eq!(
        arrangement.tracks[0].clips[0].warp_markers.as_slice(),
        &[
            WarpMarker::new(0, 0),
            WarpMarker::new(250, 250),
            WarpMarker::new(1_000, 1_000),
        ]
    );

    arrangement.update(
        ArrangementMsg::AddWarpMarker {
            track_id,
            clip_id,
            source_frame: 750,
            timeline_frame: 750,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    let moved = arrangement.update(
        ArrangementMsg::MoveWarpMarker {
            track_id,
            clip_id,
            source_frame: 250,
            timeline_frame: 900,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(moved.mark_dirty);
    assert_eq!(
        arrangement.tracks[0].clips[0].warp_markers.as_slice()[1],
        WarpMarker::new(250, 749)
    );
    assert!(matches!(
        engine.0.last(),
        Some(EngineCommand::SetClipWarpMarkers { track_id: tid, clip_id: cid, .. })
            if *tid == track_id && *cid == clip_id
    ));

    let removed = arrangement.update(
        ArrangementMsg::RemoveWarpMarker {
            track_id,
            clip_id,
            source_frame: 250,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(removed.mark_dirty);
    assert_eq!(
        arrangement.tracks[0].clips[0].warp_markers.interior(),
        &[WarpMarker::new(750, 750)]
    );
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
fn splitting_a_piecewise_warped_clip_preserves_the_complete_audible_map() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 1_000);
    assert!(arrangement.tracks[0].clips[0]
        .warp_markers
        .add(250, 500, 0, 1_000, 1_000));
    let expected: Vec<_> = (0..1_000)
        .map(|frame| arrangement.tracks[0].clips[0].source_frame_at(frame))
        .collect();
    let mut engine = RecordingEngine::default();

    arrangement.update(
        ArrangementMsg::SplitAudioClip {
            track_id,
            clip_id,
            split_position: 400,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    let mut fragments: Vec<_> = arrangement.tracks[0].clips.iter().collect();
    fragments.sort_by_key(|clip| clip.position);
    let actual: Vec<_> = fragments
        .iter()
        .flat_map(|clip| (0..clip.duration).map(|frame| clip.source_frame_at(frame)))
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(fragments[0].source_offset, 0);
    assert_eq!(fragments[0].source_end(), 200);
    assert_eq!(fragments[1].source_offset, 200);
    assert_eq!(fragments[1].source_end(), 1_000);
}

#[test]
fn splitting_a_looped_piecewise_warp_preserves_repetitions_and_phase() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 2_000);
    let original = &mut arrangement.tracks[0].clips[0];
    original.loop_enabled = true;
    original.loop_start = 0;
    original.loop_end = 1_000;
    assert!(original.warp_markers.add(250, 500, 0, 1_000, 1_000));
    let expected: Vec<_> = (0..original.duration)
        .map(|frame| original.source_frame_at(frame))
        .collect();
    let mut engine = RecordingEngine::default();

    arrangement.update(
        ArrangementMsg::SplitAudioClip {
            track_id,
            clip_id,
            split_position: 750,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    let mut fragments: Vec<_> = arrangement.tracks[0].clips.iter().collect();
    fragments.sort_by_key(|clip| clip.position);
    let actual: Vec<_> = fragments
        .iter()
        .flat_map(|clip| (0..clip.duration).map(|frame| clip.source_frame_at(frame)))
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(fragments[1].source_offset, 0);
    assert_eq!(fragments[1].start_marker, 750);
    assert_eq!(fragments[1].warp_markers, fragments[0].warp_markers);
}

#[test]
fn slicing_at_transients_creates_selected_shared_media_clips_with_exact_playback() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 100, 1_000);
    let (_, unrelated_clip_id) = add_audio_clip(&mut arrangement, 0, 2_000, 100);
    let visible_note_clip = (TrackId::new(), ClipId::new());
    arrangement
        .selected_clips
        .insert(ArrangementSelection::AudioClip {
            track_id,
            clip_id: unrelated_clip_id,
        });
    arrangement.selected_note_clip = Some(visible_note_clip);
    let original = &mut arrangement.tracks[0].clips[0];
    original
        .transient_markers
        .replace_suggestions([0, 250, 600, 1_000]);
    original.fades = vibez_core::track::ClipFades::new(100, 200, original.duration);
    let shared_audio = Arc::clone(&original.audio);
    let shared_source = original.source.clone();
    let expected: Vec<_> = (0..original.duration)
        .map(|frame| original.source_frame_at(frame))
        .collect();
    let mut engine = RecordingEngine::default();

    let action = arrangement.update(
        ArrangementMsg::SliceAudioClipAtMarkers {
            track_id,
            clip_id,
            markers: AudioSliceMarkers::Transients,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    let mut slices: Vec<_> = arrangement.tracks[0]
        .clips
        .iter()
        .filter(|clip| clip.id != unrelated_clip_id)
        .collect();
    slices.sort_by_key(|clip| clip.position);
    assert!(action.mark_dirty);
    assert_eq!(slices.len(), 3);
    assert_eq!(
        slices
            .iter()
            .map(|clip| (clip.position, clip.duration))
            .collect::<Vec<_>>(),
        vec![(100, 250), (350, 350), (700, 400)]
    );
    assert!(slices
        .iter()
        .all(|clip| Arc::ptr_eq(&clip.audio, &shared_audio) && clip.source == shared_source));
    assert_eq!(arrangement.arrangement.selected_clips.len(), 4);
    assert!(arrangement
        .arrangement
        .selected_clips
        .contains(&ArrangementSelection::AudioClip {
            track_id,
            clip_id: unrelated_clip_id,
        }));
    assert_eq!(arrangement.selected_note_clip, Some(visible_note_clip));
    assert_eq!(slices[0].fades.fade_in_frames(), 100);
    assert_eq!(slices[0].fades.fade_out_frames(), 0);
    assert_eq!(slices[1].fades, Default::default());
    assert_eq!(slices[2].fades.fade_in_frames(), 0);
    assert_eq!(slices[2].fades.fade_out_frames(), 200);
    let actual: Vec<_> = slices
        .iter()
        .flat_map(|clip| (0..clip.duration).map(|frame| clip.source_frame_at(frame)))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn reversed_marker_starts_the_second_slice_at_the_marker_source_frame() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 1_000);
    let original = &mut arrangement.tracks[0].clips[0];
    original.playback_direction = ClipPlaybackDirection::Reverse;
    original.transient_markers.replace_suggestions([300]);
    let mut engine = RecordingEngine::default();

    arrangement.update(
        ArrangementMsg::SliceAudioClipAtMarkers {
            track_id,
            clip_id,
            markers: AudioSliceMarkers::Transients,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    let mut slices: Vec<_> = arrangement.tracks[0].clips.iter().collect();
    slices.sort_by_key(|clip| clip.position);
    assert_eq!(slices[0].duration, 699);
    assert_eq!(slices[1].duration, 301);
    assert_eq!(slices[1].source_frame_at(0), 300);
}

#[test]
fn slicing_a_reversed_piecewise_warp_uses_audible_marker_positions() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 1_000);
    let original = &mut arrangement.tracks[0].clips[0];
    original.playback_direction = ClipPlaybackDirection::Reverse;
    assert!(original.warp_markers.add(250, 400, 0, 1_000, 1_000));
    assert!(original.warp_markers.add(700, 800, 0, 1_000, 1_000));
    let expected: Vec<_> = (0..original.duration)
        .map(|frame| original.source_frame_at(frame))
        .collect();
    let mut engine = RecordingEngine::default();

    arrangement.update(
        ArrangementMsg::SliceAudioClipAtMarkers {
            track_id,
            clip_id,
            markers: AudioSliceMarkers::Warp,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    let mut slices: Vec<_> = arrangement.tracks[0].clips.iter().collect();
    slices.sort_by_key(|clip| clip.position);
    assert_eq!(
        slices.iter().map(|clip| clip.duration).collect::<Vec<_>>(),
        vec![199, 400, 401]
    );
    assert_eq!(slices[1].source_frame_at(0), 700);
    let actual: Vec<_> = slices
        .iter()
        .flat_map(|clip| (0..clip.duration).map(|frame| clip.source_frame_at(frame)))
        .collect();
    let maximum_source_frame_error = actual
        .iter()
        .zip(&expected)
        .map(|(actual, expected)| actual.abs_diff(*expected))
        .max()
        .unwrap_or(0);
    assert!(
        maximum_source_frame_error <= 1,
        "fragment Warp interpolation moved by {maximum_source_frame_error} source frames"
    );
}

#[test]
fn slicing_at_a_repeated_loop_marker_cuts_every_audible_occurrence() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 300);
    let original = &mut arrangement.tracks[0].clips[0];
    original.loop_enabled = true;
    original.loop_start = 0;
    original.loop_end = 100;
    original.transient_markers.replace_suggestions([25]);
    let expected: Vec<_> = (0..original.duration)
        .map(|frame| original.source_frame_at(frame))
        .collect();
    let mut engine = RecordingEngine::default();

    arrangement.update(
        ArrangementMsg::SliceAudioClipAtMarkers {
            track_id,
            clip_id,
            markers: AudioSliceMarkers::Transients,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    let mut slices: Vec<_> = arrangement.tracks[0].clips.iter().collect();
    slices.sort_by_key(|clip| clip.position);
    assert_eq!(
        slices.iter().map(|clip| clip.duration).collect::<Vec<_>>(),
        vec![25, 75, 25, 75, 25, 75]
    );
    assert!(slices.iter().all(|clip| !clip.loop_enabled));
    let actual: Vec<_> = slices
        .iter()
        .flat_map(|clip| (0..clip.duration).map(|frame| clip.source_frame_at(frame)))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn slicing_a_reversed_loop_creates_one_shots_without_changing_playback() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 300);
    let original = &mut arrangement.tracks[0].clips[0];
    original.playback_direction = ClipPlaybackDirection::Reverse;
    original.loop_enabled = true;
    original.loop_start = 0;
    original.loop_end = 100;
    original.transient_markers.replace_suggestions([25]);
    let expected: Vec<_> = (0..original.duration)
        .map(|frame| original.source_frame_at(frame))
        .collect();
    let mut engine = RecordingEngine::default();

    arrangement.update(
        ArrangementMsg::SliceAudioClipAtMarkers {
            track_id,
            clip_id,
            markers: AudioSliceMarkers::Transients,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    let mut slices: Vec<_> = arrangement.tracks[0].clips.iter().collect();
    slices.sort_by_key(|clip| clip.position);
    assert_eq!(
        slices.iter().map(|clip| clip.duration).collect::<Vec<_>>(),
        vec![74, 26, 74, 26, 74, 26]
    );
    assert!(slices.iter().all(|clip| !clip.loop_enabled));
    let actual: Vec<_> = slices
        .iter()
        .flat_map(|clip| (0..clip.duration).map(|frame| clip.source_frame_at(frame)))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn slicing_rejects_more_regions_than_the_timeline_limit() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 1_000);
    arrangement.tracks[0].clips[0]
        .transient_markers
        .replace_suggestions(1..=MAX_TIMELINE_SLICES as u64);
    let mut engine = RecordingEngine::default();

    let action = arrangement.update(
        ArrangementMsg::SliceAudioClipAtMarkers {
            track_id,
            clip_id,
            markers: AudioSliceMarkers::Transients,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(!action.mark_dirty);
    assert_eq!(arrangement.tracks[0].clips.len(), 1);
    assert!(engine.0.is_empty());
    assert_eq!(
        action.status.as_deref(),
        Some("257 slice regions exceed the maximum of 256")
    );
}

#[test]
fn slicing_without_an_interior_marker_is_a_clean_noop() {
    let mut arrangement = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut arrangement, 0, 0, 1_000);
    arrangement.tracks[0].clips[0]
        .transient_markers
        .replace_suggestions([0, 1_000]);
    let mut engine = RecordingEngine::default();

    let action = arrangement.update(
        ArrangementMsg::SliceAudioClipAtMarkers {
            track_id,
            clip_id,
            markers: AudioSliceMarkers::Transients,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(!action.mark_dirty);
    assert_eq!(arrangement.tracks[0].clips.len(), 1);
    assert!(engine.0.is_empty());
    assert!(ArrangementMsg::SliceAudioClipAtMarkers {
        track_id,
        clip_id,
        markers: AudioSliceMarkers::Transients,
    }
    .defers_project_edit());
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
