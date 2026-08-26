use super::*;

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
    Arc::make_mut(&mut a.arrangement.timeline)
        .get_mut(tid)
        .unwrap()
        .clips[0]
        .warp_markers
        .add(250, 500, 0, 1_000, 1_000);
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
    assert_eq!(clip.warp_markers.interior(), &[WarpMarker::new(500, 1_000)]);
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
    assert!(clip.warp_markers.is_empty());
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

    assert!(a.tracks[0].clips[0]
        .warp_markers
        .add(22_050, 30_000, 0, 44_100, 44_100));

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
    assert!(clip.warp_markers.is_empty());
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
    assert!(engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::SetClipWarpMarkers { warp_markers, .. }
            if warp_markers.is_empty()
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
