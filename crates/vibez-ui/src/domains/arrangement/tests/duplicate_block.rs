use super::*;

const CTX: ArrangementCtx = ArrangementCtx {
    samples_per_beat: 100.0,
    playhead_samples: 0,
    playhead_beats: 0.0,
};

fn select_audio(a: &mut ArrangementFixture, (track_id, clip_id): (TrackId, ClipId)) {
    a.selected_clips
        .insert(ArrangementSelection::AudioClip { track_id, clip_id });
}

fn add_midi_track_with_clip(
    a: &mut ArrangementFixture,
    position_beats: f64,
    duration_beats: f64,
) -> (TrackId, ClipId) {
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut RecordingEngine::default(),
        CTX,
    );
    let track = a.tracks.last_mut().unwrap();
    let clip_id = ClipId::new();
    track.note_clips.push(UiNoteClip {
        id: clip_id,
        name: "Pattern".to_string(),
        notes: vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 0.5,
        }],
        loop_end_beats: 0.0,
        ..UiNoteClip::test_clip(position_beats, duration_beats)
    });
    (track.id, clip_id)
}

fn duplicate(a: &mut ArrangementFixture) -> ArrangementAction {
    a.update(
        ArrangementMsg::DuplicateSelectedClip,
        &mut RecordingEngine::default(),
        CTX,
    )
}

fn audio_spans(a: &ArrangementFixture, track_idx: usize) -> Vec<(u64, u64)> {
    let mut spans: Vec<_> = a.tracks[track_idx]
        .clips
        .iter()
        .map(|clip| (clip.position, clip.duration))
        .collect();
    spans.sort_unstable();
    spans
}

fn note_spans(a: &ArrangementFixture, track_idx: usize) -> Vec<(f64, f64)> {
    let mut spans: Vec<_> = a.tracks[track_idx]
        .note_clips
        .iter()
        .map(|clip| (clip.position_beats, clip.duration_beats))
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    spans
}

#[test]
fn adjacent_clips_duplicate_as_one_block_after_the_selection() {
    let mut a = arrangement_with_tracks(1);
    let first = add_audio_clip(&mut a, 0, 0, 400);
    let second = add_audio_clip(&mut a, 0, 400, 400);
    select_audio(&mut a, first);
    select_audio(&mut a, second);

    let action = duplicate(&mut a);

    assert!(action.mark_dirty);
    assert_eq!(
        audio_spans(&a, 0),
        vec![(0, 400), (400, 400), (800, 400), (1200, 400)]
    );
}

#[test]
fn gapped_clips_keep_their_gap_and_land_after_the_last_clip_ends() {
    let mut a = arrangement_with_tracks(1);
    let first = add_audio_clip(&mut a, 0, 0, 400);
    let second = add_audio_clip(&mut a, 0, 600, 200);
    select_audio(&mut a, first);
    select_audio(&mut a, second);

    duplicate(&mut a);

    assert_eq!(
        audio_spans(&a, 0),
        vec![(0, 400), (600, 200), (800, 400), (1400, 200)]
    );
}

#[test]
fn clips_on_several_lanes_move_together_and_the_copies_become_the_selection() {
    let mut a = arrangement_with_tracks(1);
    let kick_a = add_audio_clip(&mut a, 0, 0, 400);
    let kick_b = add_audio_clip(&mut a, 0, 400, 400);
    let (hat_track, hat_clip) = add_midi_track_with_clip(&mut a, 2.0, 2.0);
    select_audio(&mut a, kick_a);
    select_audio(&mut a, kick_b);
    a.selected_clips.insert(ArrangementSelection::NoteClip {
        track_id: hat_track,
        clip_id: hat_clip,
    });

    duplicate(&mut a);
    duplicate(&mut a);

    assert_eq!(
        audio_spans(&a, 0),
        vec![
            (0, 400),
            (400, 400),
            (800, 400),
            (1200, 400),
            (1600, 400),
            (2000, 400)
        ]
    );
    assert_eq!(
        note_spans(&a, 1),
        vec![(2.0, 2.0), (10.0, 2.0), (18.0, 2.0)]
    );
    assert_eq!(a.tracks[1].note_clips[2].notes.len(), 1);
    let newest_note = a.tracks[1]
        .note_clips
        .iter()
        .find(|clip| clip.position_beats == 18.0)
        .unwrap()
        .id;
    assert_eq!(a.selected_clips.len(), 3);
    assert!(a.selected_clips.contains(&ArrangementSelection::NoteClip {
        track_id: hat_track,
        clip_id: newest_note,
    }));
}

#[test]
fn a_time_selection_duplicates_its_range_right_after_itself() {
    let mut a = arrangement_with_tracks(1);
    add_audio_clip(&mut a, 0, 0, 800);
    add_midi_track_with_clip(&mut a, 3.0, 2.0);
    a.time_selection_active = true;
    a.selection_start_beats = 2.0;
    a.selection_end_beats = 6.0;

    let action = duplicate(&mut a);

    assert!(action.mark_dirty);
    assert_eq!(audio_spans(&a, 0), vec![(0, 600), (600, 400)]);
    assert_eq!(note_spans(&a, 1), vec![(3.0, 2.0), (7.0, 2.0)]);
    assert!(a.time_selection_active);
    assert_eq!(
        (a.selection_start_beats, a.selection_end_beats),
        (6.0, 10.0)
    );
    assert_eq!(a.selected_clips.len(), 2);
}

#[test]
fn a_box_over_several_lanes_duplicates_its_range_on_every_lane_it_caught() {
    let mut a = arrangement_with_tracks(2);
    add_audio_clip(&mut a, 0, 0, 400);
    add_audio_clip(&mut a, 1, 0, 400);
    let lanes = vec![a.tracks[0].id, a.tracks[1].id];
    a.update(
        ArrangementMsg::MarqueeSelect {
            anchor_track: lanes[0],
            start_beats: 0.0,
            end_beats: 4.0,
            top_y: 0.0,
            bottom_y: 140.0,
            track_ids: lanes,
            additive: false,
        },
        &mut RecordingEngine::default(),
        CTX,
    );

    duplicate(&mut a);

    assert_eq!(audio_spans(&a, 0), vec![(0, 400), (400, 400)]);
    assert_eq!(audio_spans(&a, 1), vec![(0, 400), (400, 400)]);
}
