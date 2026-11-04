//! A clip's loop region survives undo, redo, save and reload apart from its length.

use super::*;
use crate::domains::arrangement::ArrangementMsg;
use crate::domains::project::ProjectMsg;
use crate::state::{ArrangementSelection, ProjectTrack, UiNoteClip, UndoGestureId};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{NoteClipInfo, TrackKind};
use vibez_core::track::{ClipInfo, TrackInfo};
use vibez_project::Project;

fn looped_audio(track_id: TrackId, source: &Path) -> ClipInfo {
    ClipInfo {
        id: ClipId::new(),
        track_id,
        name: "Percs".into(),
        position: 0,
        source_offset: 0,
        start_marker: Some(6_000),
        duration: 4_000,
        file_path: None,
        source: Some(MediaSourceRef::LocalFile {
            path: source.to_path_buf(),
        }),
        loop_enabled: true,
        loop_start: 0,
        loop_end: 8_000,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
    }
}

fn looped_pattern(track_id: TrackId) -> NoteClipInfo {
    NoteClipInfo {
        id: ClipId::new(),
        track_id,
        name: "Break".into(),
        position_beats: 0.0,
        duration_beats: 12.0,
        notes: Vec::new(),
        start_marker_beats: Some(14.0),
        loop_enabled: true,
        loop_start_beats: 0.0,
        loop_end_beats: 16.0,
        groove_grid: Default::default(),
    }
}

async fn reopen(path: PathBuf, saved_before: Option<PathBuf>, project: Project) -> App {
    save_project_async(path.clone(), saved_before, project)
        .await
        .unwrap();
    let mut app = super::test_support::app();
    app.rebuild_from_loaded_project(load_project_async(path, None).await.unwrap());
    app
}

fn assert_loops_kept(app: &App, audio_track: TrackId, midi_track: TrackId) {
    let audio = &app.state.arrange_content(audio_track).unwrap().clips[0];
    assert_eq!(audio.duration, 4_000);
    assert_eq!(
        (audio.start_marker, audio.loop_start, audio.loop_end),
        (6_000, 0, 8_000)
    );
    let pattern = &app.state.arrange_content(midi_track).unwrap().note_clips[0];
    assert_eq!(pattern.duration_beats, 12.0);
    assert_eq!(
        (
            pattern.start_marker_beats,
            pattern.loop_start_beats,
            pattern.loop_end_beats
        ),
        (14.0, 0.0, 16.0)
    );
}

#[tokio::test]
async fn a_loop_longer_than_its_clip_survives_save_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("percs.wav");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vibez-audio-io/tests/fixtures/mono-44100-s16.wav"),
        &source,
    )
    .unwrap();
    let mut project = Project::default();
    let audio_track = TrackInfo::new("Percs");
    let mut midi_track = TrackInfo::new("Drums");
    midi_track.kind = TrackKind::Midi;
    project
        .arrange
        .clips
        .push(looped_audio(audio_track.id, &source));
    project
        .arrange
        .note_clips
        .push(looped_pattern(midi_track.id));
    let (audio_id, midi_id) = (audio_track.id, midi_track.id);
    project.tracks.extend([audio_track, midi_track]);

    let path = directory.path().join("Percs.vzp");
    let app = reopen(path.clone(), None, project).await;
    assert_loops_kept(&app, audio_id, midi_id);

    let app = reopen(path.clone(), Some(path), app.project_for_save()).await;
    assert_loops_kept(&app, audio_id, midi_id);
}

#[test]
fn undo_and_redo_of_a_shrink_keep_the_loop() {
    let mut app = super::test_support::app();
    let mut track = ProjectTrack::new(TrackId::new(), "Drums".into(), 0);
    track.kind = TrackKind::Midi;
    let track_id = track.id;
    let mut pattern = looped_pattern(track_id);
    pattern.duration_beats = 16.0;
    pattern.start_marker_beats = Some(0.0);
    let clip_id = pattern.id;
    Arc::make_mut(&mut app.state.arrangement.timeline)
        .ensure(track_id)
        .note_clips
        .push(UiNoteClip {
            id: clip_id,
            name: pattern.name,
            position_beats: 0.0,
            duration_beats: 16.0,
            notes: Vec::new(),
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: true,
            loop_start_beats: 0.0,
            loop_end_beats: 16.0,
            groove_grid: Default::default(),
        });
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    let selection = ArrangementSelection::NoteClip { track_id, clip_id };
    let _ = app.update_and_refresh_clips(Message::Arrangement(
        ArrangementMsg::SelectArrangementClip {
            selection,
            shift_held: false,
        },
    ));
    let _ = app.update_and_refresh_clips(Message::UndoGesture {
        id: UndoGestureId::new(),
        edit: Box::new(Message::Arrangement(ArrangementMsg::ResizeSelectedClips {
            anchor: selection,
            new_duration_beats: 12.0,
        })),
    });
    let clip = |app: &App| {
        let clip = &app.state.arrange_content(track_id).unwrap().note_clips[0];
        (
            clip.duration_beats,
            clip.loop_start_beats,
            clip.loop_end_beats,
        )
    };
    assert_eq!(clip(&app), (12.0, 0.0, 16.0));

    let _ = app.update_and_refresh_clips(Message::Project(ProjectMsg::Undo));
    assert_eq!(clip(&app), (16.0, 0.0, 16.0));

    let _ = app.update_and_refresh_clips(Message::Project(ProjectMsg::Redo));
    assert_eq!(clip(&app), (12.0, 0.0, 16.0));
}
