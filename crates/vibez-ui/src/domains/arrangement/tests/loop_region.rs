//! A clip's loop region is content: resizing the clip or turning its loop
//! off and on never edits it.

use super::*;

fn resize(a: &mut ArrangementFixture, anchor: ArrangementSelection, beats: f64) {
    a.update(
        ArrangementMsg::ResizeSelectedClips {
            anchor,
            new_duration_beats: beats,
        },
        &mut RecordingEngine::default(),
        ArrangementCtx {
            samples_per_beat: 100.0,
            ..Default::default()
        },
    );
}

#[test]
fn shrinking_and_regrowing_a_looped_audio_clip_keeps_its_loop() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 400);
    let clip = &mut a.tracks[0].clips[0];
    clip.loop_enabled = true;
    clip.loop_end = 400;
    clip.start_marker = 350;
    let anchor = ArrangementSelection::AudioClip { track_id, clip_id };
    a.selected_clips.insert(anchor);

    resize(&mut a, anchor, 3.0);
    let clip = &a.tracks[0].clips[0];
    assert_eq!(clip.duration, 300);
    assert_eq!(
        (clip.start_marker, clip.loop_start, clip.loop_end),
        (350, 0, 400)
    );

    resize(&mut a, anchor, 16.0);
    let clip = &a.tracks[0].clips[0];
    assert_eq!(clip.duration, 1_600);
    assert_eq!(
        (clip.start_marker, clip.loop_start, clip.loop_end),
        (350, 0, 400)
    );
}

#[test]
fn shrinking_and_regrowing_a_looped_midi_clip_keeps_its_loop() {
    let mut a = arrangement_with_tracks(1);
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut RecordingEngine::default(),
        ArrangementCtx::default(),
    );
    let track_id = a.tracks[1].id;
    let clip_id = ClipId::new();
    a.tracks[1].note_clips.push(UiNoteClip {
        id: clip_id,
        name: "Break".to_string(),
        position_beats: 0.0,
        duration_beats: 16.0,
        notes: Vec::new(),
        selected_notes: HashSet::new(),
        start_marker_beats: 14.0,
        loop_enabled: true,
        loop_start_beats: 0.0,
        loop_end_beats: 16.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    });
    let anchor = ArrangementSelection::NoteClip { track_id, clip_id };
    a.selected_clips.insert(anchor);

    resize(&mut a, anchor, 12.0);
    let clip = &a.tracks[1].note_clips[0];
    assert_eq!(clip.duration_beats, 12.0);
    assert_eq!(
        (
            clip.start_marker_beats,
            clip.loop_start_beats,
            clip.loop_end_beats
        ),
        (14.0, 0.0, 16.0)
    );

    resize(&mut a, anchor, 64.0);
    let clip = &a.tracks[1].note_clips[0];
    assert_eq!(clip.duration_beats, 64.0);
    assert_eq!(
        (
            clip.start_marker_beats,
            clip.loop_start_beats,
            clip.loop_end_beats
        ),
        (14.0, 0.0, 16.0)
    );
}

fn looped_note_clip(
    a: &mut ArrangementFixture,
    loop_beats: (f64, f64),
    start: f64,
) -> (TrackId, ClipId) {
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut RecordingEngine::default(),
        ArrangementCtx::default(),
    );
    let track = a.tracks.last_mut().unwrap();
    let clip_id = ClipId::new();
    track.note_clips.push(UiNoteClip {
        id: clip_id,
        name: "Break".to_string(),
        position_beats: 0.0,
        duration_beats: 4.0,
        notes: Vec::new(),
        selected_notes: HashSet::new(),
        start_marker_beats: start,
        loop_enabled: true,
        loop_start_beats: loop_beats.0,
        loop_end_beats: loop_beats.1,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    });
    (track.id, clip_id)
}

fn toggle_audio_loop_twice(
    a: &mut ArrangementFixture,
    track_id: TrackId,
    clip_id: ClipId,
) -> RecordingEngine {
    let mut engine = RecordingEngine::default();
    for _ in 0..2 {
        a.update(
            ArrangementMsg::ToggleClipLoop(track_id, clip_id),
            &mut engine,
            ArrangementCtx::default(),
        );
    }
    engine
}

#[test]
fn turning_an_audio_clip_loop_off_and_on_keeps_its_loop() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 400);
    let clip = &mut a.tracks[0].clips[0];
    clip.loop_enabled = true;
    clip.loop_start = 100;
    clip.loop_end = 300;
    clip.start_marker = 150;

    let engine = toggle_audio_loop_twice(&mut a, track_id, clip_id);

    let clip = &a.tracks[0].clips[0];
    assert!(clip.loop_enabled);
    assert_eq!((clip.loop_start, clip.loop_end), (100, 300));
    assert!(matches!(
        engine.0.last(),
        Some(EngineCommand::SetClipLoop {
            enabled: true,
            loop_start: 100,
            loop_end: 300,
            ..
        })
    ));
}

#[test]
fn turning_a_loop_off_and_on_never_inverts_a_loop_reaching_past_the_clip() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 300);
    let clip = &mut a.tracks[0].clips[0];
    clip.audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
        channels: vec![vec![0.0; 1_000]],
        sample_rate: 44100,
    });
    clip.loop_enabled = true;
    clip.loop_end = 400;
    clip.start_marker = 350;

    toggle_audio_loop_twice(&mut a, track_id, clip_id);

    let clip = &a.tracks[0].clips[0];
    assert_eq!((clip.loop_start, clip.loop_end), (0, 400));
}

#[test]
fn turning_selected_clip_loops_off_and_on_keeps_their_loops() {
    let mut a = arrangement_with_tracks(1);
    let (audio_track, audio_clip) = add_audio_clip(&mut a, 0, 0, 400);
    let clip = &mut a.tracks[0].clips[0];
    clip.loop_enabled = true;
    clip.loop_start = 100;
    clip.loop_end = 300;
    let (note_track, note_clip) = looped_note_clip(&mut a, (4.0, 8.0), 6.0);
    a.selected_clips.insert(ArrangementSelection::AudioClip {
        track_id: audio_track,
        clip_id: audio_clip,
    });
    a.selected_clips.insert(ArrangementSelection::NoteClip {
        track_id: note_track,
        clip_id: note_clip,
    });

    for _ in 0..2 {
        a.update(
            ArrangementMsg::ToggleSelectedClipLoop,
            &mut RecordingEngine::default(),
            ArrangementCtx::default(),
        );
    }

    let clip = &a.tracks[0].clips[0];
    assert!(clip.loop_enabled);
    assert_eq!((clip.loop_start, clip.loop_end), (100, 300));
    let clip = &a.tracks[1].note_clips[0];
    assert!(clip.loop_enabled);
    assert_eq!((clip.loop_start_beats, clip.loop_end_beats), (4.0, 8.0));
}

#[test]
fn resetting_a_loop_region_with_the_start_marker_past_the_clip_end_stays_ordered() {
    let mut a = arrangement_with_tracks(1);
    add_audio_clip(&mut a, 0, 0, 88_200);
    let audio = &mut a.tracks[0].clips[0];
    audio.duration = 44_100;
    audio.start_marker = 60_000;
    audio.reset_loop_region_to_clip();
    assert!(audio.loop_start < audio.loop_end);
    assert!(audio.start_marker < audio.loop_end);

    let mut midi = UiNoteClip::test_clip(0.0, 4.0);
    midi.start_marker_beats = 6.0;
    midi.reset_loop_region_to_clip();
    assert!(midi.loop_start_beats < midi.loop_end_beats);
    assert!(midi.start_marker_beats < midi.loop_end_beats);
}
