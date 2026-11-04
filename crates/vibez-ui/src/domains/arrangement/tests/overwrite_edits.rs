//! Every edit that places Clips overwrites what they land on, in the
//! engine as well as on screen.

use super::*;
use vibez_engine::commands::EngineCommand;

const CTX: ArrangementCtx = ArrangementCtx {
    samples_per_beat: 100.0,
    playhead_samples: 0,
    playhead_beats: 0.0,
};

fn select_audio(a: &mut ArrangementFixture, (track_id, clip_id): (TrackId, ClipId)) {
    a.selected_clips
        .insert(ArrangementSelection::AudioClip { track_id, clip_id });
}

fn add_note_clip(a: &mut ArrangementFixture, position_beats: f64, duration_beats: f64) -> ClipId {
    let clip_id = ClipId::new();
    a.tracks.last_mut().unwrap().note_clips.push(UiNoteClip {
        id: clip_id,
        name: "Hats".to_string(),
        position_beats,
        duration_beats,
        notes: Vec::new(),
        selected_notes: HashSet::new(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    });
    clip_id
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
fn a_duplicate_trims_the_clip_it_lands_on() {
    let mut a = arrangement_with_tracks(1);
    let original = add_audio_clip(&mut a, 0, 0, 400);
    add_audio_clip(&mut a, 0, 400, 800);
    select_audio(&mut a, original);

    a.update(
        ArrangementMsg::DuplicateSelectedClip,
        &mut RecordingEngine::default(),
        CTX,
    );

    assert_eq!(audio_spans(&a, 0), vec![(0, 400), (400, 400), (800, 400)]);
}

#[test]
fn a_duplicated_block_overwrites_on_every_lane_it_spans() {
    let mut a = arrangement_with_tracks(1);
    let kick = add_audio_clip(&mut a, 0, 0, 400);
    add_audio_clip(&mut a, 0, 400, 800);
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut RecordingEngine::default(),
        CTX,
    );
    let hat = add_note_clip(&mut a, 0.0, 4.0);
    add_note_clip(&mut a, 5.0, 2.0);
    let hat_track = a.tracks[1].id;
    select_audio(&mut a, kick);
    a.selected_clips.insert(ArrangementSelection::NoteClip {
        track_id: hat_track,
        clip_id: hat,
    });

    a.update(
        ArrangementMsg::DuplicateSelectedClip,
        &mut RecordingEngine::default(),
        CTX,
    );

    assert_eq!(audio_spans(&a, 0), vec![(0, 400), (400, 400), (800, 400)]);
    assert_eq!(note_spans(&a, 1), vec![(0.0, 4.0), (4.0, 4.0)]);
}

#[test]
fn a_paste_into_a_longer_clip_splits_it_in_the_engine_too() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, long) = add_audio_clip(&mut a, 0, 0, 1200);
    let short = add_audio_clip(&mut a, 0, 1600, 200);
    select_audio(&mut a, short);
    a.update(
        ArrangementMsg::CopySelectedClips,
        &mut RecordingEngine::default(),
        CTX,
    );
    a.selected_track = Some(track_id);

    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::PasteClips,
        &mut engine,
        ArrangementCtx {
            playhead_beats: 4.0,
            ..CTX
        },
    );

    assert_eq!(
        audio_spans(&a, 0),
        vec![(0, 400), (400, 200), (600, 600), (1600, 200)]
    );
    let index_of = |wanted: &dyn Fn(&EngineCommand) -> bool| {
        engine
            .0
            .iter()
            .position(wanted)
            .expect("the command was sent")
    };
    let begin = index_of(&|command| matches!(command, EngineCommand::BeginContentRebuild));
    let shrink = index_of(
        &|command| matches!(command, EngineCommand::SetClipBounds { clip_id, duration: 400, .. } if *clip_id == long),
    );
    let tail = index_of(
        &|command| matches!(command, EngineCommand::AddClip { clip_id, position: 600, .. } if *clip_id != long),
    );
    let end = index_of(&|command| matches!(command, EngineCommand::EndContentRebuild));
    assert!(begin < shrink && shrink < tail && tail < end);
    assert!(!engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::RemoveClip(track, clip) if (*track, *clip) == (track_id, long)
    )));
}

fn drop(a: &mut ArrangementFixture, anchor: ClipId) -> ArrangementAction {
    a.update(
        ArrangementMsg::DropClips { anchor },
        &mut RecordingEngine::default(),
        CTX,
    )
}

#[test]
fn a_clip_dragged_over_others_cuts_them_only_when_dropped() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, dragged) = add_audio_clip(&mut a, 0, 0, 400);
    add_audio_clip(&mut a, 0, 800, 1200);

    a.update(
        ArrangementMsg::MoveSelectedClips {
            anchor: dragged,
            new_position_beats: 10.0,
            target_track: track_id,
        },
        &mut RecordingEngine::default(),
        CTX,
    );
    assert_eq!(audio_spans(&a, 0), vec![(800, 1200), (1000, 400)]);
    let action = drop(&mut a, dragged);

    assert!(action.mark_dirty);
    assert_eq!(
        audio_spans(&a, 0),
        vec![(800, 200), (1000, 400), (1400, 600)]
    );
}

#[test]
fn a_clip_dropped_on_another_lane_overwrites_that_lane() {
    let mut a = arrangement_with_tracks(2);
    let (_, dragged) = add_audio_clip(&mut a, 0, 0, 400);
    add_audio_clip(&mut a, 1, 200, 400);
    let destination = a.tracks[1].id;

    a.update(
        ArrangementMsg::MoveSelectedClips {
            anchor: dragged,
            new_position_beats: 0.0,
            target_track: destination,
        },
        &mut RecordingEngine::default(),
        CTX,
    );
    drop(&mut a, dragged);

    assert_eq!(audio_spans(&a, 0), vec![]);
    assert_eq!(audio_spans(&a, 1), vec![(0, 400), (400, 200)]);
}

#[test]
fn a_selection_dropped_together_overwrites_others_but_not_itself() {
    let mut a = arrangement_with_tracks(1);
    let first = add_audio_clip(&mut a, 0, 0, 400);
    let second = add_audio_clip(&mut a, 0, 300, 400);
    add_audio_clip(&mut a, 0, 600, 400);
    select_audio(&mut a, first);
    select_audio(&mut a, second);

    drop(&mut a, first.1);

    assert_eq!(audio_spans(&a, 0), vec![(0, 400), (300, 400), (700, 300)]);
}

#[test]
fn a_note_clip_lengthened_over_its_neighbour_trims_it_when_dropped() {
    let mut a = arrangement_with_tracks(0);
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut RecordingEngine::default(),
        CTX,
    );
    let grown = add_note_clip(&mut a, 0.0, 4.0);
    add_note_clip(&mut a, 4.0, 4.0);
    let track_id = a.tracks[0].id;
    let anchor = ArrangementSelection::NoteClip {
        track_id,
        clip_id: grown,
    };

    a.update(
        ArrangementMsg::ResizeSelectedClips {
            anchor,
            new_duration_beats: 6.0,
        },
        &mut RecordingEngine::default(),
        CTX,
    );
    drop(&mut a, grown);

    assert_eq!(note_spans(&a, 0), vec![(0.0, 6.0), (6.0, 2.0)]);
}

#[test]
fn a_clip_start_dragged_over_its_neighbour_trims_it_when_dropped() {
    let mut a = arrangement_with_tracks(1);
    add_audio_clip(&mut a, 0, 0, 400);
    let (track_id, trimmed) = add_audio_clip(&mut a, 0, 400, 400);
    a.tracks[0].clips[1].source_offset = 400;
    a.tracks[0].clips[1].start_marker = 400;

    a.update(
        ArrangementMsg::TrimSelectedClipsStart {
            anchor: ArrangementSelection::AudioClip {
                track_id,
                clip_id: trimmed,
            },
            new_start_beats: 2.0,
        },
        &mut RecordingEngine::default(),
        CTX,
    );
    drop(&mut a, trimmed);

    assert_eq!(audio_spans(&a, 0), vec![(0, 200), (200, 600)]);
}
