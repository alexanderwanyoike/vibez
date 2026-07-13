use std::fs;
use std::path::PathBuf;
use std::process::Command;

use vibez_core::id::ClipId;
use vibez_core::track::{ClipInfo, MediaSourceRef, TrackInfo};
use vibez_project::project_format_v1::{
    detect_project_format, hex_sha256, import_legacy_json_file, import_legacy_project_v1,
    representative_document, save_project_v1, stage_local_file, ProjectContainer,
    ProjectFileFormat, Provenance, StagedMedia, APPLICATION_ID, FORMAT_VERSION,
};
use vibez_project::Project;

fn stage(path: PathBuf, id: &str, seed: u8) -> (StagedMedia, Vec<u8>) {
    let bytes: Vec<u8> = (0..2 * 1024 * 1024)
        .map(|index| seed.wrapping_add((index % 251) as u8))
        .collect();
    fs::write(&path, &bytes).unwrap();
    (
        StagedMedia {
            id: id.into(),
            file_name: format!("{id}.wav"),
            path,
            provenance: Provenance::Remote {
                provider: "dropbox".into(),
                connection_id: "proof-connection".into(),
                source_id: format!("id:{id}"),
                source_path: format!("/Megalodon/{id}.wav"),
                revision: Some("proof-rev".into()),
            },
        },
        bytes,
    )
}

#[test]
fn representative_container_roundtrips_incrementally_and_save_as() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("proof.vzp");
    let save_as_path = directory.path().join("proof-copy.vzp");
    let (staged, media_bytes) = stage(directory.path().join("proof.staged"), "proof-media", 17);
    let staging_path = staged.path.clone();
    let mut document = representative_document();

    let (mut container, full_save) =
        ProjectContainer::create_from_staged(&path, &mut document, &[staged]).unwrap();
    assert_eq!(full_save.media_rows_written, 1);
    assert_eq!(full_save.media_bytes_written, media_bytes.len() as u64);
    assert!(
        !staging_path.exists(),
        "first save should consume Staged Media"
    );

    let loaded = container.load_document().unwrap();
    assert_eq!(
        serde_json::to_vec(&loaded).unwrap(),
        serde_json::to_vec(&document).unwrap()
    );
    assert_eq!(container.read_media("proof-media").unwrap(), media_bytes);
    assert_eq!(loaded.project.note_clips[0].notes.len(), 2);
    assert_eq!(loaded.project.tracks[0].automation[0].points.len(), 2);
    assert!(loaded.project.tracks[0].native_instrument.is_some());
    assert!(loaded.project.tracks[0].effects[0]
        .plugin
        .as_ref()
        .unwrap()
        .state_b64
        .is_some());
    assert!(matches!(
        loaded.project_media[0].provenance,
        Provenance::Remote { .. }
    ));

    let file_size_before = fs::metadata(&path).unwrap().len();
    let media_before = container.media_fingerprint("proof-media").unwrap();
    document.project.bpm = 131.0;
    document.project.name = "Incremental document edit".into();
    let incremental = container.save_document(&document).unwrap();
    let media_after = container.media_fingerprint("proof-media").unwrap();
    assert_eq!(incremental.media_rows_written, 0);
    assert_eq!(incremental.media_bytes_written, 0);
    assert_eq!(media_before, media_after);
    assert!(fs::metadata(&path).unwrap().len() - file_size_before < 128 * 1024);

    let save_as = container.save_as(&save_as_path).unwrap();
    assert!(save_as.container_bytes > media_bytes.len() as u64);
    let copied = ProjectContainer::open(&save_as_path).unwrap();
    assert_eq!(
        serde_json::to_vec(&copied.load_document().unwrap()).unwrap(),
        serde_json::to_vec(&document).unwrap()
    );
    assert_eq!(copied.read_media("proof-media").unwrap(), media_bytes);
}

#[test]
fn format_markers_are_explicit_and_file_is_not_zip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("proof.vzp");
    let mut document = representative_document();
    let (container, _) = ProjectContainer::create_from_staged(&path, &mut document, &[]).unwrap();
    drop(container);

    let bytes = fs::read(&path).unwrap();
    assert_eq!(&bytes[..16], b"SQLite format 3\0");
    assert_ne!(&bytes[..2], b"PK");
    assert_eq!(document.format_version, FORMAT_VERSION);

    let connection = rusqlite::Connection::open(&path).unwrap();
    let application_id: u32 = connection
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .unwrap();
    let user_version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(application_id, APPLICATION_ID);
    assert_eq!(user_version, FORMAT_VERSION);
}

#[test]
fn process_interruption_preserves_last_committed_document_and_media() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("proof.vzp");
    let (staged, media_bytes) = stage(directory.path().join("proof.staged"), "proof-media", 83);
    let mut document = representative_document();
    let (container, _) =
        ProjectContainer::create_from_staged(&path, &mut document, &[staged]).unwrap();
    let committed_json = serde_json::to_vec(&container.load_document().unwrap()).unwrap();
    let committed_fingerprint = container.media_fingerprint("proof-media").unwrap();
    drop(container);

    let binary = env!("CARGO_BIN_EXE_project-format-v1-proof");
    let status = Command::new(binary)
        .arg("interrupt")
        .arg(&path)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(86));

    let recovered = ProjectContainer::open(&path).unwrap();
    assert_eq!(
        serde_json::to_vec(&recovered.load_document().unwrap()).unwrap(),
        committed_json
    );
    assert_eq!(
        recovered.media_fingerprint("proof-media").unwrap(),
        committed_fingerprint
    );
    assert_eq!(
        hex_sha256(&recovered.read_media("proof-media").unwrap()),
        hex_sha256(&media_bytes)
    );
}

fn project_with_source(source: MediaSourceRef) -> Project {
    let track = TrackInfo::new("Audio");
    Project {
        tracks: vec![track.clone()],
        clips: vec![ClipInfo {
            id: ClipId::new(),
            track_id: track.id,
            name: source.display_name(),
            position: 0,
            source_offset: 0,
            duration: 128,
            source: Some(source),
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 128,
            original_bpm: Some(120.0),
            warped: false,
            warped_to_bpm: None,
        }],
        ..Project::default()
    }
}

#[test]
fn production_save_is_self_contained_incremental_and_save_as_reuses_media() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("source.wav");
    let path = directory.path().join("song.vzp");
    let save_as_path = directory.path().join("song-copy.vzp");
    let bytes = b"playback-critical-media".repeat(1024);
    fs::write(&source_path, &bytes).unwrap();
    let project = project_with_source(MediaSourceRef::LocalFile {
        path: source_path.clone(),
    });

    let first = save_project_v1(&path, None, project).unwrap();
    assert_eq!(first.observation.media_rows_written, 1);
    assert_eq!(detect_project_format(&path).unwrap(), ProjectFileFormat::V1);
    fs::remove_file(&source_path).unwrap();

    let container = ProjectContainer::open(&path).unwrap();
    let document = container.load_document().unwrap();
    let MediaSourceRef::ProjectMedia { id, .. } =
        document.project.clips[0].source.as_ref().unwrap()
    else {
        panic!("committed project must reference Project Media");
    };
    assert_eq!(container.read_media(id).unwrap(), bytes);
    let fingerprint = container.media_fingerprint(id).unwrap();
    drop(container);

    let incremental = save_project_v1(&path, Some(&path), first.project.clone()).unwrap();
    assert_eq!(incremental.observation.media_rows_written, 0);
    let reopened = ProjectContainer::open(&path).unwrap();
    assert_eq!(reopened.media_fingerprint(id).unwrap(), fingerprint);
    drop(reopened);

    let save_as = save_project_v1(&save_as_path, Some(&path), incremental.project).unwrap();
    assert_eq!(save_as.observation.media_rows_written, 0);
    let copied = ProjectContainer::open(save_as_path).unwrap();
    assert_eq!(copied.read_media(id).unwrap(), bytes);
}

#[test]
fn unsaved_project_uses_staged_copy_before_first_save() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("fragile-source.wav");
    let path = directory.path().join("staged.vzp");
    let bytes = b"staged-before-save".repeat(512);
    fs::write(&source_path, &bytes).unwrap();
    let staged_source = stage_local_file(&source_path).unwrap();
    let MediaSourceRef::StagedProjectMedia { staging_path, .. } = &staged_source else {
        panic!("local import must become Staged Project Media");
    };
    let staging_path = staging_path.clone();
    fs::remove_file(source_path).unwrap();

    let saved = save_project_v1(&path, None, project_with_source(staged_source)).unwrap();
    assert!(
        !staging_path.exists(),
        "commit should consume the staging copy"
    );
    let MediaSourceRef::ProjectMedia { id, .. } = saved.project.clips[0].source.as_ref().unwrap()
    else {
        panic!("saved project must no longer reference staging");
    };
    assert_eq!(
        ProjectContainer::open(path)
            .unwrap()
            .read_media(id)
            .unwrap(),
        bytes
    );
}

#[test]
fn legacy_json_import_is_separate_self_contained_and_reports_missing_media() {
    let directory = tempfile::tempdir().unwrap();
    let source_media = directory.path().join("present.wav");
    let missing_media = directory.path().join("missing.wav");
    let legacy_path = directory.path().join("legacy.vzp");
    let imported_path = directory.path().join("legacy-imported.vzp");
    let bytes = b"legacy-local-media".repeat(256);
    fs::write(&source_media, &bytes).unwrap();
    let mut project = project_with_source(MediaSourceRef::LocalFile { path: source_media });
    let mut missing_clip = project.clips[0].clone();
    missing_clip.id = ClipId::new();
    missing_clip.name = "missing.wav".into();
    missing_clip.source = Some(MediaSourceRef::LocalFile {
        path: missing_media,
    });
    project.clips.push(missing_clip);
    project.save_to_file(&legacy_path).unwrap();
    let original = fs::read(&legacy_path).unwrap();

    let imported = import_legacy_json_file(&legacy_path, &imported_path).unwrap();
    assert_eq!(fs::read(&legacy_path).unwrap(), original);
    assert_eq!(imported.relink.len(), 1);
    assert!(imported.relink[0].source.contains("missing.wav"));
    assert_eq!(
        detect_project_format(&imported_path).unwrap(),
        ProjectFileFormat::V1
    );
    let container = ProjectContainer::open(imported_path).unwrap();
    let document = container.load_document().unwrap();
    let MediaSourceRef::ProjectMedia { id, .. } =
        document.project.clips[0].source.as_ref().unwrap()
    else {
        panic!("resolvable legacy media must become Project Media");
    };
    assert_eq!(container.read_media(id).unwrap(), bytes);
}

#[test]
fn malformed_legacy_import_never_creates_or_changes_a_destination() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("malformed.vzp");
    let destination = directory.path().join("should-not-exist.vzp");
    let bytes = b"{ definitely not valid project JSON";
    fs::write(&source, bytes).unwrap();

    assert!(import_legacy_json_file(&source, &destination).is_err());
    assert_eq!(fs::read(source).unwrap(), bytes);
    assert!(!destination.exists());
}

#[test]
fn materialized_legacy_remote_reference_keeps_remote_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let staged_path = directory.path().join("remote.staged");
    let destination = directory.path().join("remote-import.vzp");
    let bytes = b"remote-materialized-media".repeat(128);
    fs::write(&staged_path, &bytes).unwrap();
    let source = MediaSourceRef::StagedRemoteProjectMedia {
        id: hex_sha256(&bytes),
        file_name: "remote.wav".into(),
        staging_path: staged_path,
        provider: "dropbox".into(),
        connection_id: "legacy-connection".into(),
        source_id: "id:remote".into(),
        source_path: "/Samples/remote.wav".into(),
        rev: Some("rev-1".into()),
    };

    let imported = import_legacy_project_v1(&destination, project_with_source(source)).unwrap();
    assert!(imported.relink.is_empty());
    let document = ProjectContainer::open(destination)
        .unwrap()
        .load_document()
        .unwrap();
    assert!(matches!(
        document.project_media[0].provenance,
        Provenance::Remote { .. }
    ));
}
