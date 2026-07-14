use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use vibez_core::id::ClipId;
use vibez_core::track::{ClipInfo, MediaSourceRef, TrackInfo};
use vibez_project::project_format_v1::{
    detect_project_format, hex_sha256, representative_document, save_project_v1, stage_local_file,
    stage_remote_file, strip_staged_sources, sweep_staging_root, ProjectContainer,
    ProjectFileFormat, ProjectFormatError, Provenance, StagedMedia, APPLICATION_ID, FORMAT_VERSION,
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
                connection_name: None,
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
        staging_path.exists(),
        "shared staging copies survive commit for other referencing projects"
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
        staging_path.exists(),
        "content-addressed staging copies stay shared after commit"
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
fn remote_import_becomes_self_contained_project_media_with_safe_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let materialized = directory.path().join("remote-cache.wav");
    let project_path = directory.path().join("remote-owned.vzp");
    let bytes = b"complete-remote-media".repeat(512);
    fs::write(&materialized, &bytes).unwrap();
    let staged = stage_remote_file(
        &materialized,
        "Kick.wav",
        vibez_core::track::MediaProvenance::Remote {
            provider: "dropbox".into(),
            connection_id: "dropbox-primary".into(),
            connection_name: Some("Alex's Dropbox".into()),
            source_id: "id:megalodon-kick".into(),
            source_path: "/Megalodon/Kick.wav".into(),
            revision: Some("rev-7".into()),
        },
    )
    .unwrap();
    let MediaSourceRef::StagedRemoteProjectMedia { staging_path, .. } = &staged else {
        panic!("Remote import must leave disposable cache for managed staging");
    };
    let staging_path = staging_path.clone();
    fs::remove_file(&materialized).unwrap();

    let saved = save_project_v1(&project_path, None, project_with_source(staged)).unwrap();
    assert!(
        staging_path.exists(),
        "Remote staging copies stay shared after commit"
    );
    let MediaSourceRef::ProjectMedia {
        id,
        provenance: Some(provenance),
        ..
    } = saved.project.clips[0].source.as_ref().unwrap()
    else {
        panic!("saved Remote import must resolve only to Project Media");
    };
    assert_eq!(
        ProjectContainer::open(&project_path)
            .unwrap()
            .read_media(id)
            .unwrap(),
        bytes
    );
    assert!(matches!(
        provenance.as_ref(),
        vibez_core::track::MediaProvenance::Remote {
            provider,
            connection_id,
            source_path,
            revision: Some(revision),
            ..
        } if provider == "dropbox"
            && connection_id == "dropbox-primary"
            && source_path == "/Megalodon/Kick.wav"
            && revision == "rev-7"
    ));

    let document = ProjectContainer::open(project_path)
        .unwrap()
        .load_document()
        .unwrap();
    let json = serde_json::to_string(&document).unwrap();
    assert!(json.contains("/Megalodon/Kick.wav"));
    assert!(!json.contains("access_token"));
    assert!(!json.contains("refresh_token"));
    assert!(!json.contains("secret"));
}

#[test]
fn failed_first_save_leaves_no_container_and_retry_succeeds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("retry.vzp");
    let missing = StagedMedia {
        id: "missing".into(),
        file_name: "missing.wav".into(),
        path: directory.path().join("not-there.staged"),
        provenance: Provenance::Local {
            source_path: directory.path().join("not-there.wav"),
        },
    };
    let mut document = representative_document();
    let error = match ProjectContainer::create_from_staged(&path, &mut document, &[missing]) {
        Ok(_) => panic!("saving with a missing staged file must fail"),
        Err(error) => error,
    };
    assert!(matches!(error, ProjectFormatError::Io(_)));
    assert!(
        !path.exists(),
        "a failed first save must not leave a partial container"
    );
    assert!(
        fs::read_dir(directory.path()).unwrap().next().is_none(),
        "a failed first save must clean up its temporary sibling"
    );

    let (staged, media_bytes) = stage(directory.path().join("retry.staged"), "retry-media", 5);
    let mut document = representative_document();
    let (container, _) =
        ProjectContainer::create_from_staged(&path, &mut document, &[staged]).unwrap();
    assert_eq!(container.read_media("retry-media").unwrap(), media_bytes);
}

#[test]
fn save_replaces_existing_destinations() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("source.wav");
    fs::write(&source_path, b"media".repeat(256)).unwrap();

    // In-place Ctrl+S over an existing legacy JSON .vzp converts it to V1.
    let legacy_path = directory.path().join("legacy.vzp");
    let project = project_with_source(MediaSourceRef::LocalFile {
        path: source_path.clone(),
    });
    project.save_to_file(&legacy_path).unwrap();
    assert_eq!(
        detect_project_format(&legacy_path).unwrap(),
        ProjectFileFormat::LegacyJson
    );
    let saved = save_project_v1(&legacy_path, None, project).unwrap();
    assert_eq!(
        detect_project_format(&legacy_path).unwrap(),
        ProjectFileFormat::V1
    );

    // Save As onto another existing file replaces it via the copy branch;
    // the native dialog already confirmed the overwrite.
    let copy_path = directory.path().join("copy.vzp");
    fs::write(&copy_path, b"previous contents").unwrap();
    save_project_v1(&copy_path, Some(&legacy_path), saved.project).unwrap();
    assert_eq!(
        detect_project_format(&copy_path).unwrap(),
        ProjectFileFormat::V1
    );
}

#[test]
fn vanished_source_container_fails_save_instead_of_dangling_references() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("source.wav");
    let container_path = directory.path().join("original.vzp");
    fs::write(&source_path, b"bytes".repeat(128)).unwrap();
    let saved = save_project_v1(
        &container_path,
        None,
        project_with_source(MediaSourceRef::LocalFile { path: source_path }),
    )
    .unwrap();

    fs::remove_file(&container_path).unwrap();
    let rescue_path = directory.path().join("rescue.vzp");
    let error = save_project_v1(&rescue_path, None, saved.project).unwrap_err();
    assert!(matches!(error, ProjectFormatError::MissingMedia(_)));
    assert!(
        !rescue_path.exists(),
        "a refused save must not leave a container behind"
    );
}

#[test]
fn missing_local_source_survives_save_for_relink() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("relink.vzp");
    let missing = directory.path().join("gone.wav");
    let saved = save_project_v1(
        &path,
        None,
        project_with_source(MediaSourceRef::LocalFile {
            path: missing.clone(),
        }),
    )
    .unwrap();
    assert_eq!(saved.observation.media_rows_written, 0);
    let document = ProjectContainer::open(&path)
        .unwrap()
        .load_document()
        .unwrap();
    assert!(matches!(
        document.project.clips[0].source.as_ref().unwrap(),
        MediaSourceRef::LocalFile { path } if path == &missing
    ));
}

#[test]
fn strip_staged_sources_rewrites_transient_staging_references() {
    let directory = tempfile::tempdir().unwrap();
    let local_source = directory.path().join("kick.wav");
    fs::write(&local_source, b"kick").unwrap();
    let staged_local = stage_local_file(&local_source).unwrap();
    let mut local_project = project_with_source(staged_local);
    strip_staged_sources(&mut local_project);
    assert!(matches!(
        local_project.clips[0].source.as_ref().unwrap(),
        MediaSourceRef::LocalFile { path } if path == &local_source
    ));

    let materialized = directory.path().join("cache.wav");
    fs::write(&materialized, b"vocal").unwrap();
    let staged_remote = stage_remote_file(
        &materialized,
        "Vocal.wav",
        vibez_core::track::MediaProvenance::Remote {
            provider: "dropbox".into(),
            connection_id: "primary".into(),
            connection_name: None,
            source_id: "/megalodon/vocal.wav".into(),
            source_path: "/Megalodon/Vocal.wav".into(),
            revision: Some("rev-3".into()),
        },
    )
    .unwrap();
    let mut remote_project = project_with_source(staged_remote);
    strip_staged_sources(&mut remote_project);
    assert!(matches!(
        remote_project.clips[0].source.as_ref().unwrap(),
        MediaSourceRef::DropboxFile { path_lower, rev, .. }
            if path_lower == "/megalodon/vocal.wav" && rev.as_deref() == Some("rev-3")
    ));
}

#[test]
fn staging_sweep_removes_only_stale_entries() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("staging");
    fs::create_dir_all(&root).unwrap();
    let first = root.join("aaaa-first.wav");
    let second = root.join(".bbbb-second.partial");
    fs::write(&first, b"first").unwrap();
    fs::write(&second, b"second").unwrap();

    assert_eq!(sweep_staging_root(&root, Duration::from_secs(3600)), 0);
    assert!(first.exists() && second.exists());

    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(sweep_staging_root(&root, Duration::from_millis(100)), 2);
    assert!(!first.exists() && !second.exists());

    assert_eq!(
        sweep_staging_root(&directory.path().join("absent"), Duration::ZERO),
        0
    );
}
