use super::*;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::perform::GrooveGrid;

fn audio_section(name: &str, length_beats: f64, frames: u64) -> Section {
    let mut section = Section::new(0);
    section.name = name.into();
    section.length_beats = length_beats;
    let track_id = TrackId::new();
    Arc::make_mut(&mut section.timeline)
        .ensure(track_id)
        .clips
        .push(UiClip {
            id: ClipId::new(),
            name: format!("{name} Audio"),
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![0.5; frames as usize]],
                sample_rate: 8,
            }),
            source: None,
            position: 0,
            source_offset: 0,
            start_marker: 0,
            duration: frames,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
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
    section
}

fn midi_section(name: &str, track_id: TrackId, pitch: u8) -> Section {
    let mut section = Section::new(0);
    section.name = name.into();
    section.length_beats = 4.0;
    Arc::make_mut(&mut section.timeline)
        .ensure(track_id)
        .note_clips
        .push(UiNoteClip {
            id: ClipId::new(),
            name: format!("{name} Notes"),
            position_beats: 0.0,
            duration_beats: 4.0,
            notes: vec![MidiNote {
                pitch,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 0.25,
            }],
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: GrooveGrid::Sixteenth,
        });
    section
}

fn starting_capture() -> CaptureState {
    CaptureState {
        phase: CapturePhase::Starting,
        ..CaptureState::default()
    }
}

#[test]
fn effective_mid_buffer_boundaries_become_exact_arrange_positions() {
    let first = audio_section("Groove A", 4.0, 16);
    let second = audio_section("Groove B", 4.0, 16);
    let track_id = *first.timeline.by_track.keys().next().unwrap();
    let mut second = second;
    let second_track_id = *second.timeline.by_track.keys().next().unwrap();
    let second_content = Arc::make_mut(&mut second.timeline)
        .by_track
        .remove(&second_track_id)
        .unwrap();
    Arc::make_mut(&mut second.timeline)
        .by_track
        .insert(track_id, second_content);

    let mut capture = starting_capture();
    capture.prepare(40, 8, 120.0);
    capture.start(3, Some((CapturedTimelineSource::from_section(&first), 0)));
    assert_eq!(capture.arrange_start_samples(), Some(40));
    capture.transition(CapturedTimelineSource::from_section(&second), 10);
    let completed = capture.finish(15).unwrap();
    assert_eq!(capture.arrange_start_samples(), None);
    let materialized = completed.materialize();
    let clips = &materialized.by_track[&track_id].clips;

    assert_eq!(clips.len(), 2);
    assert_eq!((clips[0].position, clips[0].duration), (40, 7));
    assert_eq!((clips[1].position, clips[1].duration), (47, 5));
    assert_eq!(materialized.arrange_end_samples, 52);
}

#[test]
fn source_refresh_resnapshots_at_the_exact_local_playhead() {
    let first = audio_section("Before edit", 4.0, 16);
    let track_id = *first.timeline.by_track.keys().next().unwrap();
    let mut refreshed = audio_section("After edit", 4.0, 16);
    let refreshed_track = *refreshed.timeline.by_track.keys().next().unwrap();
    let content = Arc::make_mut(&mut refreshed.timeline)
        .by_track
        .remove(&refreshed_track)
        .unwrap();
    Arc::make_mut(&mut refreshed.timeline)
        .by_track
        .insert(track_id, content);

    let mut capture = starting_capture();
    capture.prepare(0, 8, 120.0);
    capture.start(0, Some((CapturedTimelineSource::from_section(&first), 0)));
    capture.refresh(CapturedTimelineSource::from_section(&refreshed), 4, 4);
    let materialized = capture.finish(8).unwrap().materialize();
    let clips = &materialized.by_track[&track_id].clips;

    assert_eq!(clips.len(), 2);
    assert_eq!(
        (clips[0].position, clips[0].source_offset, clips[0].duration),
        (0, 0, 4)
    );
    assert_eq!(
        (clips[1].position, clips[1].source_offset, clips[1].duration),
        (4, 4, 4)
    );
    assert!(clips[0].name.contains("Before edit"));
    assert!(clips[1].name.contains("After edit"));
}

#[test]
fn looping_section_is_flattened_into_independent_linear_passes() {
    let mut section = audio_section("Groove A", 1.0, 4);
    section.looping = true;
    let track_id = *section.timeline.by_track.keys().next().unwrap();
    let mut capture = starting_capture();
    capture.prepare(0, 8, 120.0);
    capture.start(0, Some((CapturedTimelineSource::from_section(&section), 0)));
    let clips = &capture.finish(10).unwrap().materialize().by_track[&track_id].clips;

    assert_eq!(clips.len(), 3);
    assert_eq!(
        clips.iter().map(|clip| clip.position).collect::<Vec<_>>(),
        [0, 4, 8]
    );
    assert_eq!(
        clips.iter().map(|clip| clip.duration).collect::<Vec<_>>(),
        [4, 4, 2]
    );
    assert!(clips.windows(2).all(|pair| pair[0].id != pair[1].id));
}

#[test]
fn midi_sections_keep_effective_transition_alignment_and_groove_identity() {
    let track_id = TrackId::new();
    let first = midi_section("Groove A", track_id, 36);
    let second = midi_section("Groove B", track_id, 38);
    let source_ids = [
        first.timeline.get(track_id).unwrap().note_clips[0].id,
        second.timeline.get(track_id).unwrap().note_clips[0].id,
    ];
    let mut capture = starting_capture();
    capture.prepare(8, 8, 120.0);
    capture.start(0, Some((CapturedTimelineSource::from_section(&first), 0)));
    capture.transition(CapturedTimelineSource::from_section(&second), 5);
    let clips = &capture.finish(9).unwrap().materialize().by_track[&track_id].note_clips;

    assert_eq!(clips.len(), 2);
    assert!((clips[0].position_beats - 2.0).abs() < 1e-9);
    assert!((clips[0].duration_beats - 1.25).abs() < 1e-9);
    assert!((clips[1].position_beats - 3.25).abs() < 1e-9);
    assert_eq!([clips[0].notes[0].pitch, clips[1].notes[0].pitch], [36, 38]);
    assert!(clips
        .iter()
        .all(|clip| clip.groove_grid == GrooveGrid::Sixteenth));
    assert!(clips.iter().all(|clip| !source_ids.contains(&clip.id)));
}

#[test]
fn coincident_live_and_section_notes_are_both_preserved_without_source_mutation() {
    let track_id = TrackId::new();
    let section = midi_section("Layer", track_id, 36);
    let source_id = section.timeline.get(track_id).unwrap().note_clips[0].id;
    let mut capture = starting_capture();
    capture.prepare(0, 8, 120.0);
    capture.prepare_controlled_tracks([(track_id, false)]);
    capture.start(0, Some((CapturedTimelineSource::from_section(&section), 0)));
    capture.input_note(track_id, 36, 127, true, 0);
    capture.input_note(track_id, 36, 0, false, 1);

    let materialized = capture.finish(4).unwrap().materialize();
    let clips = &materialized.by_track[&track_id].note_clips;
    assert_eq!(
        clips
            .iter()
            .flat_map(|clip| &clip.notes)
            .filter(|note| note.pitch == 36)
            .count(),
        2
    );
    assert_eq!(
        section.timeline.get(track_id).unwrap().note_clips[0].id,
        source_id
    );
    assert!(clips.iter().all(|clip| clip.id != source_id));
}

#[test]
fn transition_reported_after_stop_cannot_extend_the_capture() {
    let first = audio_section("Groove A", 4.0, 16);
    let second = audio_section("Groove B", 4.0, 16);
    let track_id = *first.timeline.by_track.keys().next().unwrap();
    let mut capture = starting_capture();
    capture.prepare(0, 8, 120.0);
    capture.start(0, Some((CapturedTimelineSource::from_section(&first), 0)));
    let completed = capture.finish(4).unwrap();
    capture.transition(CapturedTimelineSource::from_section(&second), 6);

    let clips = &completed.materialize().by_track[&track_id].clips;
    assert_eq!(clips.len(), 1);
    assert_eq!((clips[0].position, clips[0].duration), (0, 4));
    assert_eq!(capture.phase, CapturePhase::Idle);
}

#[test]
fn source_edits_after_transition_cannot_rewrite_capture_snapshot() {
    let mut section = audio_section("Breakdown", 4.0, 16);
    let track_id = *section.timeline.by_track.keys().next().unwrap();
    let mut capture = starting_capture();
    capture.prepare(20, 8, 120.0);
    capture.start(
        100,
        Some((CapturedTimelineSource::from_section(&section), 0)),
    );
    Arc::make_mut(&mut section.timeline)
        .ensure(track_id)
        .clips
        .clear();

    let materialized = capture.finish(108).unwrap().materialize();
    assert_eq!(materialized.by_track[&track_id].clips.len(), 1);
    assert!(section.timeline.get(track_id).unwrap().clips.is_empty());
}

#[test]
fn effective_mute_event_materializes_as_steps_with_start_and_closing_state() {
    let track_id = TrackId::new();
    let section = midi_section("Groove A", track_id, 36);
    let mut capture = starting_capture();
    capture.prepare(40, 8, 120.0);
    capture.prepare_controlled_tracks([(track_id, false)]);
    capture.start(
        100,
        Some((CapturedTimelineSource::from_section(&section), 0)),
    );
    capture.track_mute_changed(track_id, true, 103);
    // A Section transition while held must not synthesize another gesture.
    capture.transition(CapturedTimelineSource::from_section(&section), 105);
    let materialized = capture.finish(110).unwrap().materialize();
    let lane = materialized.by_track[&track_id]
        .automation
        .iter()
        .find(|lane| lane.target == AutomationTarget::TrackMute)
        .unwrap();

    assert_eq!(
        lane.points
            .iter()
            .map(|point| (point.beat, point.value))
            .collect::<Vec<_>>(),
        [(10.0, 0.0), (10.75, 1.0), (12.5, 0.0)]
    );
    assert_eq!(lane.value_at(10.5), Some(0.0));
    assert_eq!(lane.value_at(11.0), Some(1.0));
    assert_eq!(lane.value_at(12.5), Some(0.0));
}
