//! Clip Projects round-trip through the real container and media hydration path.

use super::*;
use vibez_core::id::{ClipId, TrackId};
use vibez_project::{LauncherClipInfo, PerformLayout, Project, TimelineLocation};

#[tokio::test]
async fn clip_project_reopens_audio_midi_and_independent_arrange_content() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("hats.wav");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../vibez-audio-io/tests/fixtures/mono-44100-s16.wav");
    std::fs::copy(fixture, &source).unwrap();
    let mut project = vibez_project::project_format_v1::representative_document().project;
    project.perform_layout = PerformLayout::Clips;
    let audio_track = vibez_core::track::TrackInfo::new("Hats");
    let audio_track_id = audio_track.id;
    project.tracks.push(audio_track);
    let mut midi = project.arrange.clone();
    let midi_id = ClipId::new();
    midi.note_clips[0].id = midi_id;
    midi.note_clips[0].name = "Bass alternative".into();
    project.launcher_clips.push(LauncherClipInfo {
        id: midi_id,
        track_id: project.tracks[0].id,
        row: 13,
        timeline: midi,
    });
    let audio_id = ClipId::new();
    let audio = vibez_core::track::ClipInfo {
        id: audio_id,
        track_id: audio_track_id,
        name: "Hats".into(),
        position: 0,
        source_offset: 0,
        start_marker: Some(0),
        duration: 1000,
        file_path: None,
        source: Some(MediaSourceRef::LocalFile {
            path: source.clone(),
        }),
        loop_enabled: true,
        loop_start: 0,
        loop_end: 1000,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
    };
    project.launcher_clips.push(LauncherClipInfo {
        id: audio_id,
        track_id: audio_track_id,
        row: 8,
        timeline: vibez_project::TimelineInfo {
            clips: vec![audio],
            ..Default::default()
        },
    });
    let destination = directory.path().join("Clip project.vzp");
    save_project_async(destination.clone(), None, project)
        .await
        .unwrap();
    std::fs::remove_file(source).unwrap();
    let loaded = load_project_async(destination.clone(), None).await.unwrap();
    assert_eq!(loaded.project.perform_layout, PerformLayout::Clips);
    assert!(loaded.project.sections.is_empty());
    assert_eq!(loaded.project.launcher_clips.len(), 2);
    assert_eq!(loaded.project.launcher_clips[0].row, 13);
    assert_eq!(loaded.project.arrange.note_clips[0].name, "Proof pattern");
    assert_eq!(
        loaded.project.launcher_clips[0].timeline.note_clips[0].name,
        "Bass alternative"
    );
    assert!(loaded.unresolved_clips.is_empty());
    assert_eq!(loaded.clips.len(), 1);
    assert_eq!(
        loaded.clips[0].location,
        TimelineLocation::LauncherClip(audio_id)
    );
    assert!(loaded.clips[0].clip.audio.num_frames() > 0);
    let container = vibez_project::project_format_v1::ProjectContainer::open(destination).unwrap();
    assert_eq!(container.load_document().unwrap().format_version, 2);
}

#[test]
fn legacy_documents_keep_sections_and_discover_launcher_ids() {
    let legacy =
        r#"{"name":"Legacy","bpm":120,"sample_rate":44100,"tracks":[],"clips":[],"note_clips":[]}"#;
    let mut project: Project = serde_json::from_str(legacy).unwrap();
    assert_eq!(project.perform_layout, PerformLayout::Sections);
    assert!(project.launcher_clips.is_empty());
    let id = ClipId::new();
    let track_id = TrackId::new();
    project.launcher_clips.push(LauncherClipInfo {
        id,
        track_id,
        row: 5,
        timeline: Default::default(),
    });
    assert!(project.max_persisted_id() >= id.raw().max(track_id.raw()));
    assert!(project
        .timeline(TimelineLocation::LauncherClip(id))
        .is_some());
}
