use super::*;

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
