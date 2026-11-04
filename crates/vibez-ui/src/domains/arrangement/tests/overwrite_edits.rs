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
