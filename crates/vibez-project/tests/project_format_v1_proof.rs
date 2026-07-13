use std::fs;
use std::path::PathBuf;
use std::process::Command;

use vibez_project::project_format_v1_proof::{
    hex_sha256, representative_document, ProofContainer, Provenance, StagedMedia, APPLICATION_ID,
    FORMAT_VERSION,
};

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
        ProofContainer::create_from_staged(&path, &mut document, &[staged]).unwrap();
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
    let copied = ProofContainer::open(&save_as_path).unwrap();
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
    let (container, _) = ProofContainer::create_from_staged(&path, &mut document, &[]).unwrap();
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
        ProofContainer::create_from_staged(&path, &mut document, &[staged]).unwrap();
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

    let recovered = ProofContainer::open(&path).unwrap();
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
