use super::*;

#[test]
fn add_move_and_delete_transient_markers_each_restore_through_undo() {
    let mut state = AppState::default();
    let track_id = TrackId::new();
    let clip_id = ClipId::new();
    Arc::make_mut(&mut state.project_tracks)
        .tracks
        .push(ProjectTrack::new(track_id, "Audio".into(), 0));
    Arc::make_mut(&mut state.arrangement.timeline)
        .ensure(track_id)
        .clips
        .push(UiClip {
            id: clip_id,
            name: "Loop".into(),
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![0.0; 1_000]],
                sample_rate: 48_000,
            }),
            source: None,
            position: 0,
            source_offset: 0,
            start_marker: 0,
            duration: 1_000,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 1_000,
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
        });
    let mut engine = RecordingEngine::default();

    let mut edit = |state: &mut AppState, message| {
        let before = snapshot(state);
        let action = state.arrangement.update(
            Arc::make_mut(&mut state.project_tracks),
            message,
            &mut engine,
            ArrangementCtx::default(),
        );
        assert!(action.mark_dirty);
        state.project.history.push_edit(before, None);
    };

    edit(
        &mut state,
        ArrangementMsg::AddTransientMarker {
            track_id,
            clip_id,
            source_frame: 100,
        },
    );
    undo_once(&mut state);
    assert!(state.arrangement.timeline.get(track_id).unwrap().clips[0]
        .transient_markers
        .is_empty());

    edit(
        &mut state,
        ArrangementMsg::AddTransientMarker {
            track_id,
            clip_id,
            source_frame: 100,
        },
    );
    edit(
        &mut state,
        ArrangementMsg::MoveTransientMarker {
            track_id,
            clip_id,
            from: 100,
            to: 250,
        },
    );
    undo_once(&mut state);
    assert_eq!(
        state.arrangement.timeline.get(track_id).unwrap().clips[0]
            .transient_markers
            .as_slice()[0]
            .source_frame(),
        100
    );

    edit(
        &mut state,
        ArrangementMsg::RemoveTransientMarker {
            track_id,
            clip_id,
            source_frame: 100,
        },
    );
    undo_once(&mut state);
    assert_eq!(
        state.arrangement.timeline.get(track_id).unwrap().clips[0]
            .transient_markers
            .as_slice()[0]
            .source_frame(),
        100
    );

    edit(
        &mut state,
        ArrangementMsg::AddWarpMarker {
            track_id,
            clip_id,
            source_frame: 250,
            timeline_frame: 250,
        },
    );
    undo_once(&mut state);
    assert!(state.arrangement.timeline.get(track_id).unwrap().clips[0]
        .warp_markers
        .is_empty());

    edit(
        &mut state,
        ArrangementMsg::AddWarpMarker {
            track_id,
            clip_id,
            source_frame: 250,
            timeline_frame: 250,
        },
    );
    edit(
        &mut state,
        ArrangementMsg::MoveWarpMarker {
            track_id,
            clip_id,
            source_frame: 250,
            timeline_frame: 500,
        },
    );
    undo_once(&mut state);
    assert_eq!(
        state.arrangement.timeline.get(track_id).unwrap().clips[0]
            .warp_markers
            .interior()[0]
            .timeline_frame(),
        250
    );
}

#[test]
fn one_marker_slice_action_undoes_back_to_the_original_audio_clip() {
    let mut state = AppState::default();
    let track_id = TrackId::new();
    let clip_id = ClipId::new();
    Arc::make_mut(&mut state.project_tracks)
        .tracks
        .push(ProjectTrack::new(track_id, "Audio".into(), 0));
    let mut clip = UiClip {
        id: clip_id,
        name: "Loop".into(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![0.0; 1_000]],
            sample_rate: 48_000,
        }),
        source: None,
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 1_000,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 1_000,
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
    };
    clip.transient_markers.replace_suggestions([250, 750]);
    Arc::make_mut(&mut state.arrangement.timeline)
        .ensure(track_id)
        .clips
        .push(clip);
    let before = snapshot(&state);
    let mut engine = RecordingEngine::default();

    let action = state.arrangement.update(
        Arc::make_mut(&mut state.project_tracks),
        ArrangementMsg::SliceAudioClipAtMarkers {
            track_id,
            clip_id,
            markers: AudioSliceMarkers::Transients,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(action.mark_dirty);
    state.project.history.push_edit(before, None);
    assert_eq!(
        state
            .arrangement
            .timeline
            .get(track_id)
            .unwrap()
            .clips
            .len(),
        3
    );

    undo_once(&mut state);
    let clips = &state.arrangement.timeline.get(track_id).unwrap().clips;
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].id, clip_id);
}

#[test]
fn slice_to_drum_rack_undo_removes_the_track_instrument_and_midi_clip_together() {
    let mut state = AppState::default();
    let source_track_id = TrackId::new();
    let source_clip_id = ClipId::new();
    Arc::make_mut(&mut state.project_tracks)
        .tracks
        .push(ProjectTrack::new(source_track_id, "Audio".into(), 0));
    let mut clip = UiClip {
        id: source_clip_id,
        name: "Loop".into(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![0.0; 1_000]],
            sample_rate: 48_000,
        }),
        source: Some(vibez_core::track::MediaSourceRef::LocalFile {
            path: "loop.wav".into(),
        }),
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 1_000,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 1_000,
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
    };
    clip.transient_markers.replace_suggestions([500]);
    let prepared_audio = Arc::clone(&clip.audio);
    let prepared_source = clip.source.clone().unwrap();
    Arc::make_mut(&mut state.arrangement.timeline)
        .ensure(source_track_id)
        .clips
        .push(clip);
    let before = snapshot(&state);
    let mut engine = RecordingEngine::default();

    let action = state.arrangement.update(
        Arc::make_mut(&mut state.project_tracks),
        ArrangementMsg::SliceAudioClipToDrumRack {
            track_id: source_track_id,
            clip_id: source_clip_id,
            markers: AudioSliceMarkers::Transients,
            source: prepared_source,
            audio: prepared_audio,
        },
        &mut engine,
        ArrangementCtx {
            samples_per_beat: 100.0,
            ..ArrangementCtx::default()
        },
    );
    let generated_track_id = action.replay_project_track.unwrap();
    assert!(action.mark_dirty);
    state.project.history.push_edit(before, None);
    assert!(state
        .project_tracks
        .find(generated_track_id)
        .is_some_and(
            |track| track.instrument_kind == Some(vibez_core::midi::InstrumentKind::DrumRack)
        ));
    assert_eq!(
        state
            .arrangement
            .timeline
            .get(generated_track_id)
            .unwrap()
            .note_clips
            .len(),
        1
    );

    undo_once(&mut state);
    assert!(state.project_tracks.find(generated_track_id).is_none());
    assert!(state.arrangement.timeline.get(generated_track_id).is_none());
    assert!(state.project_tracks.find(source_track_id).is_some());
}
