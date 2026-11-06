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

#[tokio::test]
async fn a_raw_or_warp_audition_plays_audio_at_the_device_rate() {
    let raw = prepare_browser_audition_async(
        one_second_at_file_rate(),
        crate::state::BrowserAuditionPlan::Raw,
        120.0,
        DEVICE_RATE,
    )
    .await
    .unwrap();
    let warped = prepare_browser_audition_async(
        one_second_at_file_rate(),
        crate::state::BrowserAuditionPlan::Warp { source_bpm: 120.0 },
        60.0,
        DEVICE_RATE,
    )
    .await
    .unwrap();

    assert_eq!(
        (raw.sample_rate, raw.num_frames()),
        (DEVICE_RATE, DEVICE_RATE as usize)
    );
    assert_eq!(
        (warped.sample_rate, warped.num_frames()),
        (DEVICE_RATE, 2 * DEVICE_RATE as usize)
    );
}

#[test]
fn a_raw_audition_at_another_rate_is_prepared_before_it_plays() {
    let mut app = super::test_support::app();
    app.state.transport.sample_rate = DEVICE_RATE;
    let source = MediaSourceRef::LocalFile {
        path: PathBuf::from("/samples/loop.wav"),
    };
    app.state.browser.select_source(source.clone());

    let _ = app.play_browser_mode(source, one_second_at_file_rate());

    assert!(app.state.browser.audition_audio.is_none());
    assert!(app.state.browser.audition_loading);
}
