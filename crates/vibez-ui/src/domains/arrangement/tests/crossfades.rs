use super::*;

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
    assert_eq!(action.status.as_deref(), Some("Created crossfade"));
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
fn dragging_a_fade_handle_to_the_neighbour_edge_creates_the_crossfade() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, outgoing_id) = add_audio_clip(&mut a, 0, 0, 1_000);
    let (_, incoming_id) = add_audio_clip(&mut a, 0, 750, 1_000);
    a.selected_clips = HashSet::from([ArrangementSelection::AudioClip {
        track_id,
        clip_id: outgoing_id,
    }]);
    let mut engine = RecordingEngine::default();

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

    assert!(action.mark_dirty);
    assert_eq!(
        a.tracks[0].clips[0].fades.crossfade_out_to(),
        Some(incoming_id)
    );
    assert_eq!(
        a.tracks[0].clips[1].fades.crossfade_in_from(),
        Some(outgoing_id)
    );
    assert_eq!(a.tracks[0].clips[0].fades.fade_out_frames(), 250);
    assert_eq!(a.tracks[0].clips[1].fades.fade_in_frames(), 250);
}

#[test]
fn one_crossfade_curve_edit_updates_both_reciprocal_edges() {
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
    let curve = vibez_core::track::FadeCurve::new(60);

    let action = a.update(
        ArrangementMsg::SetAudioClipCrossfadeCurve {
            track_id,
            outgoing_id,
            incoming_id,
            curve,
        },
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(action.mark_dirty);
    assert_eq!(a.tracks[0].clips[0].fades.fade_out_curve(), curve);
    assert_eq!(a.tracks[0].clips[1].fades.fade_in_curve(), curve);
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
    a.update(
        ArrangementMsg::SetAudioClipCrossfadeCurve {
            track_id,
            outgoing_id,
            incoming_id,
            curve: vibez_core::track::FadeCurve::new(70),
        },
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
    assert_eq!(
        outgoing.fades.fade_out_curve(),
        vibez_core::track::FadeCurve::default()
    );
    assert_eq!(
        incoming.fades.fade_in_curve(),
        vibez_core::track::FadeCurve::default()
    );
    assert!(matches!(
        engine.0.last(),
        Some(EngineCommand::MoveClip { .. })
    ));

    a.update(
        ArrangementMsg::MoveAudioClip {
            track_id,
            clip_id: incoming_id,
            new_position: 750,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    a.update(
        ArrangementMsg::CrossfadeSelectedAudioClips,
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(
        a.tracks[0].clips[0].fades.fade_out_curve(),
        vibez_core::track::FadeCurve::new(70)
    );
    assert_eq!(
        a.tracks[0].clips[1].fades.fade_in_curve(),
        vibez_core::track::FadeCurve::new(70)
    );
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
    let curve = vibez_core::track::FadeCurve::new(55);
    a.update(
        ArrangementMsg::SetAudioClipCrossfadeCurve {
            track_id,
            outgoing_id,
            incoming_id,
            curve,
        },
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
    assert_eq!(a.tracks[0].clips[0].fades.fade_out_curve(), curve);
    assert_eq!(a.tracks[0].clips[1].fades.fade_in_curve(), curve);
}
