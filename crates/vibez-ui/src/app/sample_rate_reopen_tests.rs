//! A project saved at one sample rate keeps its timing when the audio device
//! runs at another, whether opened that way or switched while open.

use super::*;
use vibez_core::id::ClipId;
use vibez_core::track::{ClipInfo, TrackInfo};
use vibez_project::Project;

const SAVED_RATE: u32 = 44_100;
const DEVICE_RATE: u32 = 48_000;

fn percs_at_two_seconds(track_id: vibez_core::id::TrackId, source: &Path) -> ClipInfo {
    ClipInfo {
        id: ClipId::new(),
        track_id,
        name: "Percs".into(),
        position: 2 * SAVED_RATE as u64,
        source_offset: 0,
        start_marker: None,
        duration: 1_000,
        file_path: None,
        source: Some(MediaSourceRef::LocalFile {
            path: source.to_path_buf(),
        }),
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
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

async fn saved_percs_project(directory: &Path) -> (PathBuf, vibez_core::id::TrackId) {
    let source = directory.join("percs.wav");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vibez-audio-io/tests/fixtures/mono-44100-s16.wav"),
        &source,
    )
    .unwrap();
    let track = TrackInfo::new("Percs");
    let track_id = track.id;
    let mut project = Project {
        sample_rate: SAVED_RATE,
        tracks: vec![track],
        ..Project::default()
    };
    project
        .arrange
        .clips
        .push(percs_at_two_seconds(track_id, &source));
    let path = directory.join("Percs.vzp");
    save_project_async(path.clone(), None, project)
        .await
        .unwrap();
    (path, track_id)
}

#[tokio::test]
async fn an_audio_clip_stays_on_its_bar_when_the_device_rate_differs() {
    let directory = tempfile::tempdir().unwrap();
    let (path, track_id) = saved_percs_project(directory.path()).await;

    let mut app = super::test_support::app();
    app.state.transport.sample_rate = DEVICE_RATE;
    app.rebuild_from_loaded_project(load_project_async(path, None, DEVICE_RATE).await.unwrap());

    let clip = &app.state.arrange_content(track_id).unwrap().clips[0];
    assert_eq!(clip.position, 2 * DEVICE_RATE as u64);
    assert_eq!(clip.audio.sample_rate, DEVICE_RATE);
}

#[tokio::test]
async fn switching_the_device_rate_keeps_the_open_project_on_its_bars_with_its_unsaved_edits() {
    let directory = tempfile::tempdir().unwrap();
    let (path, track_id) = saved_percs_project(directory.path()).await;
    let mut app = super::test_support::app();
    app.state.transport.sample_rate = SAVED_RATE;
    app.rebuild_from_loaded_project(
        load_project_async(path.clone(), None, SAVED_RATE)
            .await
            .unwrap(),
    );
    app.state.project.dirty = true;
    let at_saved_rate = app.project_for_save();

    app.state.transport.sample_rate = DEVICE_RATE;
    let conversion = app.sample_rate_conversion(at_saved_rate).unwrap();
    let converted = convert_project_sample_rate_async(conversion, None).await;
    app.finish_sample_rate_conversion(converted);

    let clip = &app.state.arrange_content(track_id).unwrap().clips[0];
    assert_eq!(clip.position, 2 * DEVICE_RATE as u64);
    assert_eq!(clip.audio.sample_rate, DEVICE_RATE);
    assert_eq!(app.state.project.current_path, Some(path));
    assert!(app.state.project.dirty);
}

#[test]
fn switching_the_device_rate_of_a_project_without_audio_converts_nothing() {
    let mut app = super::test_support::app();
    let at_saved_rate = app.project_for_save();

    app.state.transport.sample_rate = DEVICE_RATE;

    assert!(app.sample_rate_conversion(at_saved_rate).is_none());
}
