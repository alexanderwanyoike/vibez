use super::*;

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
