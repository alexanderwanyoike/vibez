//! A clip's loop region is content: resizing the clip never edits it.

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
