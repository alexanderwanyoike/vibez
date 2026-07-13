use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use vibez_project::project_format_v1::{
    abort_mid_save, hex_sha256, representative_document, ProjectContainer, Provenance, StagedMedia,
};

const ASSET_BYTES: usize = 32 * 1024 * 1024;

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.get(1).map(String::as_str) == Some("interrupt") {
        abort_mid_save(
            arguments
                .get(2)
                .expect("interrupt requires a container path"),
        );
    }

    let output_dir = arguments
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_output_dir);
    run_measurement(&output_dir)
}

fn run_measurement(output_dir: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(output_dir)?;
    let container_path = output_dir.join("project-format-v1-proof.vzp");
    let save_as_path = output_dir.join("project-format-v1-proof-save-as.vzp");
    for path in [&container_path, &save_as_path] {
        if path.exists() {
            fs::remove_file(path)?;
        }
    }

    let local_bytes = pcm_wav_bytes(0x31);
    let remote_bytes = pcm_wav_bytes(0xa7);
    let local_stage = output_dir.join("local.staged");
    let remote_stage = output_dir.join("remote.staged");
    fs::write(&local_stage, &local_bytes)?;
    fs::write(&remote_stage, &remote_bytes)?;

    let staged = vec![
        StagedMedia {
            id: "local-break".into(),
            file_name: "local-break.wav".into(),
            path: local_stage.clone(),
            provenance: Provenance::Local {
                source_path: PathBuf::from("/library/Breaks/local-break.wav"),
            },
        },
        StagedMedia {
            id: "remote-vocal".into(),
            file_name: "remote-vocal.wav".into(),
            path: remote_stage.clone(),
            provenance: Provenance::Remote {
                provider: "dropbox".into(),
                connection_id: "alex-dropbox".into(),
                source_id: "id:proof-remote-vocal".into(),
                source_path: "/Megalodon/Vocals/remote-vocal.wav".into(),
                revision: Some("proof-rev-1".into()),
            },
        },
    ];

    let mut document = representative_document();
    let (mut container, full_save) =
        ProjectContainer::create_from_staged(&container_path, &mut document, &staged)?;
    assert!(!local_stage.exists() && !remote_stage.exists());
    assert_eq!(container.read_media("local-break")?, local_bytes);
    assert_eq!(container.read_media("remote-vocal")?, remote_bytes);
    let document_json = serde_json::to_vec(&document)?;
    assert_eq!(
        serde_json::to_vec(&container.load_document()?)?,
        document_json
    );

    let original_size = fs::metadata(&container_path)?.len();
    let fingerprint_before = container.media_fingerprint("local-break")?;
    document.project.name = "Document-only incremental change".into();
    document.project.bpm = 127.0;
    let committed_json = serde_json::to_vec(&document)?;
    let incremental = container.save_document(&document)?;
    let fingerprint_after = container.media_fingerprint("local-break")?;
    assert_eq!(fingerprint_after, fingerprint_before);
    assert_eq!(incremental.media_rows_written, 0);

    let save_as = container.save_as(&save_as_path)?;
    drop(container);

    let reopen_started = Instant::now();
    let reopened = ProjectContainer::open(&container_path)?;
    let reopened_document = reopened.load_document()?;
    let reopen_elapsed = reopen_started.elapsed();
    assert_eq!(serde_json::to_vec(&reopened_document)?, committed_json);
    assert_eq!(
        hex_sha256(&reopened.read_media("local-break")?),
        fingerprint_before.1
    );
    drop(reopened);

    let child = Command::new(std::env::current_exe()?)
        .arg("interrupt")
        .arg(&container_path)
        .status()?;
    assert!(
        !child.success(),
        "the interrupted child must terminate before commit"
    );

    let recovered_started = Instant::now();
    let recovered = ProjectContainer::open(&container_path)?;
    let recovered_document = recovered.load_document()?;
    let recovery_elapsed = recovered_started.elapsed();
    assert_eq!(serde_json::to_vec(&recovered_document)?, committed_json);
    assert_eq!(
        recovered.media_fingerprint("local-break")?,
        fingerprint_before
    );
    assert_eq!(recovered.read_media("local-break")?, local_bytes);

    let save_as_container = ProjectContainer::open(&save_as_path)?;
    assert_eq!(
        serde_json::to_vec(&save_as_container.load_document()?)?,
        committed_json
    );
    assert_eq!(save_as_container.read_media("remote-vocal")?, remote_bytes);

    let final_size = fs::metadata(&container_path)?.len();
    println!("# Project Format V1 SQLite proof measurement");
    println!("fixture_media_bytes={}", ASSET_BYTES * 2);
    println!(
        "full_save_ms={:.3}",
        full_save.elapsed.as_secs_f64() * 1000.0
    );
    println!(
        "incremental_save_ms={:.3}",
        incremental.elapsed.as_secs_f64() * 1000.0
    );
    println!("reopen_ms={:.3}", reopen_elapsed.as_secs_f64() * 1000.0);
    println!("save_as_ms={:.3}", save_as.elapsed.as_secs_f64() * 1000.0);
    println!(
        "recovery_reopen_ms={:.3}",
        recovery_elapsed.as_secs_f64() * 1000.0
    );
    println!("container_bytes_after_full_save={original_size}");
    println!("container_bytes_after_incremental_and_recovery={final_size}");
    println!(
        "incremental_media_rows_written={}",
        incremental.media_rows_written
    );
    println!(
        "incremental_media_bytes_written={}",
        incremental.media_bytes_written
    );
    println!("media_rowid_before={}", fingerprint_before.0);
    println!("media_rowid_after={}", fingerprint_after.0);
    println!("media_sha256_before={}", fingerprint_before.1);
    println!("media_sha256_after={}", fingerprint_after.1);
    println!("interrupted_child_status={child}");
    println!("artifacts={}", output_dir.display());
    Ok(())
}

fn pcm_wav_bytes(seed: u8) -> Vec<u8> {
    let mut bytes = vec![0; ASSET_BYTES];
    let data_bytes = (ASSET_BYTES - 44) as u32;
    bytes[0..4].copy_from_slice(b"RIFF");
    bytes[4..8].copy_from_slice(&((ASSET_BYTES as u32) - 8).to_le_bytes());
    bytes[8..12].copy_from_slice(b"WAVE");
    bytes[12..16].copy_from_slice(b"fmt ");
    bytes[16..20].copy_from_slice(&16_u32.to_le_bytes());
    bytes[20..22].copy_from_slice(&1_u16.to_le_bytes());
    bytes[22..24].copy_from_slice(&2_u16.to_le_bytes());
    bytes[24..28].copy_from_slice(&48_000_u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&(48_000_u32 * 4).to_le_bytes());
    bytes[32..34].copy_from_slice(&4_u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&16_u16.to_le_bytes());
    bytes[36..40].copy_from_slice(b"data");
    bytes[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    for (index, byte) in bytes[44..].iter_mut().enumerate() {
        *byte = seed.wrapping_add((index % 251) as u8);
    }
    bytes
}

fn default_output_dir() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after Unix epoch")
        .as_millis();
    std::env::temp_dir().join(format!("vibez-project-format-v1-proof-{timestamp}"))
}
