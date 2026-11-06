//! Audio imported into a session arrives at the device's sample rate.

use super::*;
use vibez_core::audio_buffer::DecodedAudio;

const FILE_RATE: u32 = 44_100;
const DEVICE_RATE: u32 = 48_000;

fn one_second_at_file_rate() -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![0.25; FILE_RATE as usize]],
        sample_rate: FILE_RATE,
    })
}

fn staged(directory: &Path, audio: &DecodedAudio) -> (PathBuf, MediaSourceRef) {
    let path = directory.join("loop.wav");
    vibez_audio_io::file_io::write_wav_file(&path, audio).unwrap();
    let source = vibez_project::project_format_v1::stage_local_file(&path).unwrap();
    (path, source)
}

#[tokio::test]
async fn a_raw_browser_import_arrives_at_the_device_rate() {
    let directory = tempfile::tempdir().unwrap();
    let raw = one_second_at_file_rate();
    let (_, source) = staged(directory.path(), &raw);
    let treatment = crate::state::AuditionImportInput {
        mode: crate::state::AuditionMode::Raw,
        source_bpm: None,
    };

    let (audio, _, _) = prepare_browser_import_audio_async(
        crate::message::BrowserImportTarget::ArrangementNewTrackAt {
            position_samples: 0,
        },
        treatment,
        raw,
        source,
        120.0,
        DEVICE_RATE,
    )
    .await
    .unwrap();

    assert_eq!(audio.sample_rate, DEVICE_RATE);
    assert_eq!(audio.num_frames(), DEVICE_RATE as usize);
}

#[tokio::test]
async fn a_warp_browser_import_and_its_original_arrive_at_the_device_rate() {
    let directory = tempfile::tempdir().unwrap();
    let raw = one_second_at_file_rate();
    let (_, source) = staged(directory.path(), &raw);
    let treatment = crate::state::AuditionImportInput {
        mode: crate::state::AuditionMode::Warp,
        source_bpm: Some(120.0),
    };

    let (warped, original, _) = prepare_browser_import_audio_async(
        crate::message::BrowserImportTarget::ArrangementNewTrackAt {
            position_samples: 0,
        },
        treatment,
        raw,
        source,
        60.0,
        DEVICE_RATE,
    )
    .await
    .unwrap();

    assert_eq!(warped.sample_rate, DEVICE_RATE);
    assert_eq!(warped.num_frames(), 2 * DEVICE_RATE as usize);
    let original = original.unwrap();
    assert_eq!(original.sample_rate, DEVICE_RATE);
    assert_eq!(original.num_frames(), DEVICE_RATE as usize);
}

#[tokio::test]
async fn a_file_picked_for_a_clip_or_device_decodes_at_the_device_rate() {
    let directory = tempfile::tempdir().unwrap();
    let (path, _) = staged(directory.path(), &one_second_at_file_rate());

    let (audio, _) = decode_and_stage_local_async(path, DEVICE_RATE)
        .await
        .unwrap();

    assert_eq!(audio.sample_rate, DEVICE_RATE);
    assert_eq!(audio.num_frames(), DEVICE_RATE as usize);
}
