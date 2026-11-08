use super::*;
use vibez_core::perform::GrooveGrid;

// Eight samples per second at 120 BPM is four samples per beat, so one
// 4-bar pass of a 16-beat Section is 64 samples.
const SAMPLE_RATE: u32 = 8;
const BPM: f64 = 120.0;
const PASS: u64 = 64;

fn midi_section(name: &str, track_id: TrackId, pitch: u8) -> Section {
    let mut section = Section::new(0);
    section.name = name.into();
    section.length_beats = 16.0;
    let content = Arc::make_mut(&mut section.timeline).ensure(track_id);
    content.note_clips.push(UiNoteClip {
        id: ClipId::new(),
        name: format!("{name} Notes"),
        position_beats: 0.0,
        duration_beats: 16.0,
        notes: vec![
            MidiNote {
                pitch,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 0.5,
            },
            MidiNote {
                pitch,
                velocity: 90,
                start_beat: 10.0,
                duration_beats: 0.5,
            },
        ],
        selected_notes: Default::default(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: GrooveGrid::Sixteenth,
    });
    let mut gain = AutomationLane::new(AutomationTarget::TrackGain);
    for (beat, value) in [(0.0, 0.0), (8.0, 1.0)] {
        gain.insert_point(AutomationPoint {
            beat,
            value,
            curve: 0.0,
        });
    }
    content.automation.push(gain);
    section
}

fn capture_from(arrange_start: u64) -> CaptureState {
    let mut capture = CaptureState {
        phase: CapturePhase::Starting,
        ..CaptureState::default()
    };
    capture.prepare(arrange_start, SAMPLE_RATE, BPM);
    capture
}

fn spans(clips: &[UiNoteClip]) -> Vec<(f64, f64)> {
    clips
        .iter()
        .map(|clip| (clip.position_beats, clip.duration_beats))
        .collect()
}

#[test]
fn four_passes_of_a_looping_midi_section_become_one_looped_clip() {
    let track_id = TrackId::new();
    let section = midi_section("Groove A", track_id, 36);
    let mut capture = capture_from(0);
    capture.start(0, Some((CapturedTimelineSource::from_section(&section), 0)));
    let take = capture.finish(4 * PASS).unwrap().materialize();
    let clips = &take.by_track[&track_id].note_clips;

    assert_eq!(spans(clips), [(0.0, 64.0)]);
    let clip = &clips[0];
    assert!(clip.loop_enabled);
    assert_eq!(
        (
            clip.start_marker_beats,
            clip.loop_start_beats,
            clip.loop_end_beats
        ),
        (0.0, 0.0, 16.0)
    );
    assert_eq!(
        clip.notes
            .iter()
            .map(|note| (note.start_beat, note.velocity))
            .collect::<Vec<_>>(),
        [(0.0, 100), (10.0, 90)]
    );
    assert_eq!(clip.name, "Capture · Groove A · Groove A Notes");
    assert_eq!(clip.groove_grid, GrooveGrid::Sixteenth);
    let onsets: Vec<_> = clip
        .notes
        .iter()
        .flat_map(|note| clip.note_occurrences(note.start_beat))
        .collect();
    assert_eq!(onsets.len(), 8, "every pass is heard: {onsets:?}");

    let gain = &take.by_track[&track_id].automation[0];
    for pass in 0..4 {
        let start = f64::from(pass) * 16.0;
        assert_eq!(gain.value_at(start), Some(0.0));
        assert_eq!(gain.value_at(start + 8.0), Some(1.0));
    }
}

#[test]
fn a_section_change_or_relaunch_starts_a_new_clip() {
    let track_id = TrackId::new();
    let first = midi_section("Groove A", track_id, 36);
    let second = midi_section("Groove B", track_id, 38);
    let mut capture = capture_from(0);
    capture.start(0, Some((CapturedTimelineSource::from_section(&first), 0)));
    capture.transition(CapturedTimelineSource::from_section(&second), 2 * PASS);
    capture.transition(CapturedTimelineSource::from_section(&second), 3 * PASS);
    let take = capture.finish(5 * PASS).unwrap().materialize();
    let clips = &take.by_track[&track_id].note_clips;

    assert_eq!(spans(clips), [(0.0, 32.0), (32.0, 16.0), (48.0, 32.0)]);
    assert!(clips.iter().all(|clip| clip.loop_enabled));
    assert_eq!(
        clips
            .iter()
            .map(|clip| clip.notes[0].pitch)
            .collect::<Vec<_>>(),
        [36, 38, 38]
    );
    assert_ne!(clips[1].id, clips[2].id);
}

#[test]
fn a_launch_that_joins_mid_pass_starts_its_looped_clip_at_that_phase() {
    let track_id = TrackId::new();
    let section = midi_section("Groove A", track_id, 36);
    let mut capture = capture_from(0);
    capture.start(
        0,
        Some((CapturedTimelineSource::from_section(&section), 40)),
    );
    let take = capture.finish(2 * PASS).unwrap().materialize();
    let clip = &take.by_track[&track_id].note_clips[0];

    assert_eq!(spans(std::slice::from_ref(clip)), [(0.0, 32.0)]);
    assert_eq!(clip.start_marker_beats, 10.0);
    assert_eq!(clip.timeline().source_at(6.0), 0.0);
    assert_eq!(take.by_track[&track_id].note_clips.len(), 1);
}

#[test]
fn a_track_with_several_section_clips_still_writes_one_clip_per_launch() {
    let track_id = TrackId::new();
    let mut section = midi_section("Groove A", track_id, 36);
    let content = Arc::make_mut(&mut section.timeline).ensure(track_id);
    content.note_clips[0].duration_beats = 8.0;
    let mut second = content.note_clips[0].clone();
    second.id = ClipId::new();
    second.position_beats = 8.0;
    second.notes = vec![MidiNote {
        pitch: 40,
        velocity: 80,
        start_beat: 1.0,
        duration_beats: 0.5,
    }];
    content.note_clips.push(second);
    let mut capture = capture_from(0);
    capture.start(0, Some((CapturedTimelineSource::from_section(&section), 0)));
    let take = capture.finish(3 * PASS).unwrap().materialize();
    let clips = &take.by_track[&track_id].note_clips;

    assert_eq!(spans(clips), [(0.0, 48.0)]);
    assert_eq!(
        clips[0]
            .notes
            .iter()
            .map(|note| (note.start_beat, note.pitch))
            .collect::<Vec<_>>(),
        [(0.0, 36), (9.0, 40)]
    );
    assert_eq!(clips[0].name, "Capture · Groove A");
}

fn audio_section(name: &str, track_id: TrackId, frames: u64) -> Section {
    let mut section = Section::new(0);
    section.name = name.into();
    section.length_beats = 16.0;
    Arc::make_mut(&mut section.timeline)
        .ensure(track_id)
        .clips
        .push(UiClip {
            id: ClipId::new(),
            name: name.into(),
            audio: Arc::new(vibez_core::audio_buffer::DecodedAudio {
                channels: vec![(0..frames).map(|frame| frame as f32).collect()],
                sample_rate: SAMPLE_RATE,
            }),
            source: crate::state::UiClip::test_source(),
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

fn audio_spans(clips: &[UiClip]) -> Vec<(u64, u64)> {
    clips
        .iter()
        .map(|clip| (clip.position, clip.duration))
        .collect()
}

#[test]
fn four_passes_of_a_looping_audio_section_become_one_looped_clip() {
    let track_id = TrackId::new();
    let section = audio_section("Break", track_id, PASS);
    let mut capture = capture_from(8);
    capture.start(0, Some((CapturedTimelineSource::from_section(&section), 0)));
    let take = capture.finish(4 * PASS).unwrap().materialize();
    let clips = &take.by_track[&track_id].clips;

    assert_eq!(audio_spans(clips), [(8, 4 * PASS)]);
    let clip = &clips[0];
    assert!(clip.loop_enabled);
    assert_eq!(
        (clip.start_marker, clip.loop_start, clip.loop_end),
        (0, 0, PASS)
    );
    assert_eq!(clip.name, "Capture · Break");
    for pass in 0..4 {
        assert_eq!(clip.source_frame_at(pass * PASS + 5), 5);
    }
}

#[test]
fn an_audio_clip_that_loops_inside_its_pass_keeps_its_own_loop() {
    let track_id = TrackId::new();
    let mut section = audio_section("Hats", track_id, PASS);
    let clip = &mut Arc::make_mut(&mut section.timeline).ensure(track_id).clips[0];
    clip.loop_enabled = true;
    clip.loop_end = PASS / 4;
    let mut capture = capture_from(0);
    capture.start(
        0,
        Some((CapturedTimelineSource::from_section(&section), 20)),
    );
    capture.transition(CapturedTimelineSource::from_section(&section), 3 * PASS);
    let take = capture.finish(4 * PASS).unwrap().materialize();
    let clips = &take.by_track[&track_id].clips;

    assert_eq!(audio_spans(clips), [(0, 3 * PASS), (3 * PASS, PASS)]);
    assert_eq!(
        (
            clips[0].start_marker,
            clips[0].loop_start,
            clips[0].loop_end
        ),
        (4, 0, PASS / 4)
    );
    assert_eq!(clips[1].start_marker, 0);
}

#[test]
fn audio_that_a_looped_clip_cannot_repeat_keeps_one_clip_per_pass() {
    let track_id = TrackId::new();
    let mut section = audio_section("Late", track_id, PASS / 2);
    Arc::make_mut(&mut section.timeline).ensure(track_id).clips[0].position = PASS / 2;
    let mut capture = capture_from(0);
    capture.start(0, Some((CapturedTimelineSource::from_section(&section), 0)));
    let take = capture.finish(2 * PASS).unwrap().materialize();

    assert_eq!(
        audio_spans(&take.by_track[&track_id].clips),
        [(PASS / 2, PASS / 2), (PASS + PASS / 2, PASS / 2)]
    );
}

#[test]
fn captured_midi_clips_sit_on_whole_ticks_at_any_tempo() {
    // 124 BPM at 44.1 kHz has no whole number of samples per beat, so every
    // boundary the engine reports is a fraction of a sample off the beat.
    let samples_per_beat = 44_100.0 * 60.0 / 124.0;
    let at_beat = |beat: f64| (beat * samples_per_beat).round() as u64;
    let track_id = TrackId::new();
    let looping = midi_section("Groove A", track_id, 36);
    let mut one_shot = midi_section("Fill", track_id, 38);
    one_shot.looping = false;
    let mut riff = midi_section("Riff", track_id, 40);
    Arc::make_mut(&mut riff.timeline)
        .ensure(track_id)
        .note_clips[0]
        .set_loop_enabled(true);
    let launcher = crate::domains::perform::LauncherClip {
        id: ClipId::new(),
        track_id,
        row: 0,
        timeline: riff.timeline,
        launch: Default::default(),
    };
    let mut capture = CaptureState {
        phase: CapturePhase::Starting,
        ..CaptureState::default()
    };
    capture.prepare(at_beat(4.0), 44_100, 124.0);
    capture.start(0, Some((CapturedTimelineSource::from_section(&looping), 0)));
    capture.transition(
        CapturedTimelineSource::from_section(&one_shot),
        at_beat(32.0),
    );
    capture.end_source(at_beat(40.0));
    capture.clip_transition(
        track_id,
        Some(CapturedTimelineSource::from_clip(
            &launcher,
            samples_per_beat,
        )),
        at_beat(40.0),
        0,
    );
    let take = capture.finish(at_beat(72.0)).unwrap().materialize();
    let clips = &take.by_track[&track_id].note_clips;

    assert_eq!(spans(clips), [(4.0, 32.0), (36.0, 8.0), (44.0, 32.0)]);
    assert_eq!(clips[2].loop_end_beats, 16.0);
}
