use super::*;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::perform::GrooveGrid;

fn section(name: &str, early: TrackId, full: TrackId, lead: u32) -> CapturedTimelineSource {
    let mut source = Section::new(0);
    source.name = name.into();
    source.length_beats = 16.0;
    for track in [early, full] {
        Arc::make_mut(&mut source.timeline)
            .ensure(track)
            .clips
            .push(UiClip {
                id: ClipId::new(),
                name: name.into(),
                audio: Arc::new(DecodedAudio {
                    channels: vec![vec![0.5; 64]],
                    sample_rate: 8,
                }),
                source: None,
                position: 0,
                source_offset: 0,
                start_marker: 0,
                duration: 64,
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
    }
    CapturedTimelineSource::from_section_with_offsets(&source, Arc::from([(early, lead)]))
}
fn capture() -> CaptureState {
    let mut capture = CaptureState {
        phase: CapturePhase::Starting,
        ..Default::default()
    };
    capture.prepare(0, 8, 120.0);
    capture
}

#[test]
fn heard_window_trims_early_prefix_and_retains_its_tail_at_capture_stop() {
    let early = TrackId::new();
    let full = TrackId::new();
    let mut capture = capture();
    capture.start(0, Some((section("First", early, full, 4), 0)));
    let result = capture.finish(12).unwrap().materialize();
    let early_clip = &result.by_track[&early].clips[0];
    assert_eq!(
        (
            early_clip.position,
            early_clip.source_offset,
            early_clip.duration
        ),
        (0, 4, 12)
    );
    let full_clip = &result.by_track[&full].clips[0];
    assert_eq!(
        (
            full_clip.position,
            full_clip.source_offset,
            full_clip.duration
        ),
        (0, 0, 12)
    );
}

#[test]
fn section_transition_preserves_distinct_early_and_full_track_boundaries() {
    let early = TrackId::new();
    let full = TrackId::new();
    let mut capture = capture();
    capture.start(0, Some((section("First", early, full, 4), 0)));
    capture.transition(section("Second", early, full, 4), 8);
    let result = capture.finish(12).unwrap().materialize();
    let early_clips = &result.by_track[&early].clips;
    assert_eq!((early_clips[0].position, early_clips[0].duration), (0, 4));
    assert_eq!((early_clips[1].position, early_clips[1].duration), (4, 8));
    let full_clips = &result.by_track[&full].clips;
    assert_eq!((full_clips[0].position, full_clips[0].duration), (0, 8));
    assert_eq!((full_clips[1].position, full_clips[1].duration), (8, 4));
}

#[test]
fn early_source_before_future_mix_boundary_is_kept_when_capture_stops_first() {
    let early = TrackId::new();
    let full = TrackId::new();
    let mut capture = capture();
    capture.start(0, Some((section("First", early, full, 8), 0)));
    capture.transition(section("Second", early, full, 8), 16);
    let result = capture.finish(12).unwrap().materialize();
    assert_eq!(result.by_track[&early].clips.len(), 2);
    assert_eq!(
        (
            result.by_track[&early].clips[1].position,
            result.by_track[&early].clips[1].duration
        ),
        (8, 4)
    );
    assert_eq!(result.by_track[&full].clips.len(), 1);
    assert_eq!(result.by_track[&full].clips[0].duration, 12);
    assert_eq!(result.arrange_end_samples, 12);
}

#[test]
fn midi_offsets_preserve_note_duration_velocity_and_groove() {
    let early = TrackId::new();
    let full = TrackId::new();
    let mut source = Section::new(0);
    source.length_beats = 16.0;
    for track in [early, full] {
        Arc::make_mut(&mut source.timeline)
            .ensure(track)
            .note_clips
            .push(UiNoteClip {
                id: ClipId::new(),
                name: "Notes".into(),
                position_beats: 0.0,
                duration_beats: 16.0,
                notes: vec![MidiNote {
                    pitch: 60,
                    velocity: 98,
                    start_beat: 2.0,
                    duration_beats: 1.0,
                }],
                selected_notes: Default::default(),
                start_marker_beats: 0.0,
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
                groove_grid: GrooveGrid::Sixteenth,
            });
    }
    let mut capture = capture();
    capture.start(
        0,
        Some((
            CapturedTimelineSource::from_section_with_offsets(&source, Arc::from([(early, 4)])),
            0,
        )),
    );
    let result = capture.finish(16).unwrap().materialize();
    for (track, expected_start) in [(early, 1.0), (full, 2.0)] {
        let clip = &result.by_track[&track].note_clips[0];
        let note = &clip.notes[0];
        assert_eq!(clip.position_beats + note.start_beat, expected_start);
        assert_eq!(note.duration_beats, 1.0);
        assert_eq!(note.velocity, 98);
        assert_eq!(clip.groove_grid, GrooveGrid::Sixteenth);
    }
}

#[test]
fn stopping_a_looped_section_keeps_silence_after_each_actual_path_end() {
    let early = TrackId::new();
    let full = TrackId::new();
    let mut source = section("Loop", early, full, 4);
    source.looping = true;
    let mut capture = capture();
    capture.start(0, Some((source, 0)));
    capture.end_source(12);
    let result = capture.finish(16).unwrap().materialize();
    assert_eq!(result.by_track[&early].clips[0].duration, 8);
    assert_eq!(result.by_track[&full].clips[0].duration, 12);
    assert_eq!(result.arrange_end_samples, 16);
}
