use super::*;

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
