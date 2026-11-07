//! Project format v1 integration tests. Split from app/mod.rs.

use super::*;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::ClipId;
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::track::ClipInfo;
use vibez_core::track::{
    ClipFades, ClipPlaybackDirection, FadeCurve, InstrumentStateInfo, TrackInfo,
};
use vibez_core::transient::{TransientMarker, TransientMarkerKind, TransientMarkers};
use vibez_core::warp_marker::{WarpMarker, WarpMarkers};
use vibez_engine::commands::{AuditionStart, EngineCommand};
use vibez_engine::engine::AudioProcessBlock;
use vibez_project::Project;

fn one_second_audio() -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![0.25; 44_100]],
        sample_rate: 44_100,
    })
}

fn format_fixture(file_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../vibez-audio-io/tests/fixtures")
        .join(file_name)
}

fn assert_audible(audio: Arc<DecodedAudio>, label: &str) {
    let (mut engine, mut commands, _events) = vibez_engine::engine::AudioEngine::new();
    commands
        .push(EngineCommand::StartAudition {
            audio,
            start: AuditionStart::Immediate,
            looped: false,
        })
        .unwrap();
    let mut output = vec![0.0_f32; 8_192];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    assert!(
        output.iter().any(|sample| sample.abs() > 1.0e-5),
        "{label} produced no Audition output"
    );
}

#[tokio::test]
async fn supported_format_matrix_catalogs_auditions_imports_and_reopens() {
    let fixtures = [
        ("mono-44100-s16.wav", "WAV", 1, 44_100),
        ("stereo-48000-s24.aiff", "AIFF", 2, 48_000),
        ("mono-32000-s24.flac", "FLAC", 1, 32_000),
        ("stereo-44100.mp3", "MP3", 2, 44_100),
        ("mono-48000.ogg", "OGG", 1, 48_000),
        ("stereo-44100.m4a", "M4A", 2, 44_100),
    ];

    for (file_name, format, channels, sample_rate) in fixtures {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join(file_name);
        std::fs::copy(format_fixture(file_name), &source_path).unwrap();

        let catalog =
            crate::app::audio_tasks::scan_sample_root(&directory.path().to_path_buf()).unwrap();
        assert_eq!(catalog.entries.len(), 1, "{file_name}");
        assert_eq!(catalog.entries[0].format, format);
        let source = catalog.entries[0].source.clone();

        let audition = decode_local_for_preview_async(source_path.clone())
            .await
            .unwrap();
        assert_eq!(audition.audio.num_channels(), channels, "{file_name}");
        assert_eq!(audition.audio.sample_rate, sample_rate, "{file_name}");
        assert_audible(Arc::clone(&audition.audio), file_name);

        let mut browser = crate::state::BrowserState {
            entries: catalog.entries,
            ..crate::state::BrowserState::default()
        };
        browser.select_source(source.clone());
        let generation = browser.begin_audition_load(&source);
        assert!(browser.install_audition(
            generation,
            browser.selected_source.clone().unwrap(),
            crate::message::AnalysedBrowserAudio {
                audio: Arc::clone(&audition.audio),
                loop_fit: audition.loop_fit,
            },
        ));
        let metadata = &browser.entries[0];
        assert_eq!(metadata.channels, Some(channels));
        assert_eq!(metadata.sample_rate, Some(sample_rate));
        assert!(metadata
            .duration_seconds
            .is_some_and(|duration| duration > 0.0));

        let (imported, staged) = decode_and_stage_local_async(source_path.clone(), sample_rate)
            .await
            .unwrap();
        assert_eq!(
            imported.num_frames(),
            audition.audio.num_frames(),
            "{file_name}"
        );
        let track = TrackInfo::new("Audio");
        let mut transient_markers = TransientMarkers::default();
        transient_markers.add_authored(imported.num_frames() as u64 / 3);
        transient_markers.replace_suggestions([imported.num_frames() as u64 / 2]);
        let mut warp_markers = WarpMarkers::default();
        warp_markers.add(
            imported.num_frames() as u64 / 3,
            imported.num_frames() as u64 / 2,
            0,
            imported.num_frames() as u64,
            imported.num_frames() as u64,
        );
        let duration = imported.num_frames() as u64;
        let fades = ClipFades::new(duration / 10, duration / 8, duration)
            .with_fade_in_curve(FadeCurve::new(65))
            .with_fade_out_curve(FadeCurve::new(-45));
        let project_path = directory.path().join("format-roundtrip.vzp");
        let project = Project {
            reduced_latency_monitoring: false,
            sample_rate,
            tracks: vec![track.clone()],
            arrange: vibez_project::TimelineInfo {
                clips: vec![ClipInfo {
                    id: ClipId::new(),
                    track_id: track.id,
                    name: file_name.into(),
                    position: 0,
                    source_offset: 0,
                    start_marker: None,
                    duration,
                    source: Some(staged),
                    file_path: None,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: 0,
                    gain_db: Default::default(),
                    fades,
                    playback_direction: ClipPlaybackDirection::Reverse,
                    transient_markers,
                    warp_markers,
                    transpose: Default::default(),
                    original_bpm: None,
                    warped: false,
                    warped_to_bpm: None,
                }],
                ..vibez_project::TimelineInfo::default()
            },
            ..Project::default()
        };
        vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();
        std::fs::remove_file(source_path).unwrap();

        let reopened = load_project_async(project_path, None, sample_rate)
            .await
            .unwrap();
        let reopened_audio = Arc::clone(&reopened.clips[0].audio);
        assert_eq!(reopened_audio.num_channels(), channels, "{file_name}");
        assert_eq!(reopened_audio.sample_rate, sample_rate, "{file_name}");
        assert_eq!(
            reopened_audio.num_frames(),
            imported.num_frames(),
            "{file_name}"
        );
        assert_eq!(
            reopened.project.arrange.clips[0].warp_markers.as_slice(),
            &[
                WarpMarker::new(0, 0),
                WarpMarker::new(
                    imported.num_frames() as u64 / 3,
                    imported.num_frames() as u64 / 2,
                ),
                WarpMarker::new(imported.num_frames() as u64, imported.num_frames() as u64,),
            ],
            "{file_name}"
        );
        assert!(matches!(
            reopened.project.arrange.clips[0].source,
            Some(MediaSourceRef::ProjectMedia { .. })
        ));
        assert_eq!(
            reopened.project.arrange.clips[0].playback_direction,
            ClipPlaybackDirection::Reverse,
            "{file_name}"
        );
        assert_eq!(
            reopened.project.arrange.clips[0].fades, fades,
            "{file_name}"
        );
        assert_eq!(
            reopened.project.arrange.clips[0]
                .transient_markers
                .as_slice(),
            &[
                TransientMarker::new(
                    imported.num_frames() as u64 / 3,
                    TransientMarkerKind::Authored,
                ),
                TransientMarker::new(
                    imported.num_frames() as u64 / 2,
                    TransientMarkerKind::Suggested,
                ),
            ],
            "{file_name}"
        );
        assert_audible(reopened_audio, file_name);
    }
}

#[tokio::test]
async fn corrupt_advertised_local_source_never_reaches_staging() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("corrupt.wav");
    std::fs::write(&source_path, b"RIFF not decodable audio").unwrap();

    let catalog =
        crate::app::audio_tasks::scan_sample_root(&directory.path().to_path_buf()).unwrap();
    assert_eq!(catalog.entries.len(), 1, "extension is only eligibility");
    let preview_error = decode_local_for_preview_async(source_path.clone())
        .await
        .unwrap_err();
    let import_error = decode_and_stage_local_async(source_path, 44_100)
        .await
        .unwrap_err();
    assert!(!preview_error.is_empty());
    assert!(!import_error.is_empty());
}

#[tokio::test]
async fn warp_arrangement_import_reopens_from_project_media_without_local_source() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("loop.wav");
    let project_path = directory.path().join("warp-import.vzp");
    let raw = one_second_audio();
    vibez_audio_io::file_io::write_wav_file(&source_path, &raw).unwrap();
    let source = vibez_project::project_format_v1::stage_local_file(&source_path).unwrap();
    let treatment = crate::state::AuditionImportInput {
        mode: crate::state::AuditionMode::Warp,
        source_bpm: Some(120.0),
    };
    let (warped, original, staged) = prepare_browser_import_audio_async(
        crate::message::BrowserImportTarget::ArrangementNewTrackAt {
            position_samples: 0,
        },
        treatment,
        Arc::clone(&raw),
        source,
        60.0,
        44_100,
    )
    .await
    .unwrap();
    assert_eq!(warped.num_frames(), 88_200);
    assert_eq!(original.unwrap().num_frames(), raw.num_frames());

    let track = TrackInfo::new("Audio");
    let project = Project {
        reduced_latency_monitoring: false,
        tracks: vec![track.clone()],
        arrange: vibez_project::TimelineInfo {
            clips: vec![ClipInfo {
                id: ClipId::new(),
                track_id: track.id,
                name: "loop.wav".into(),
                position: 0,
                source_offset: 0,
                start_marker: None,
                duration: warped.num_frames() as u64,
                source: Some(staged),
                file_path: None,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                gain_db: Default::default(),
                fades: Default::default(),
                playback_direction: Default::default(),
                transient_markers: Default::default(),
                warp_markers: Default::default(),
                transpose: Default::default(),
                original_bpm: Some(120.0),
                warped: true,
                warped_to_bpm: Some(60.0),
            }],
            ..vibez_project::TimelineInfo::default()
        },
        ..Project::default()
    };
    vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();
    std::fs::remove_file(source_path).unwrap();

    let loaded = load_project_async(
        project_path,
        None,
        vibez_core::constants::DEFAULT_SAMPLE_RATE,
    )
    .await
    .unwrap();
    assert_eq!(loaded.clips[0].audio.num_frames(), warped.num_frames());
    assert!(matches!(
        loaded.project.arrange.clips[0].source,
        Some(MediaSourceRef::ProjectMedia { .. })
    ));
}

#[tokio::test]
async fn warp_sampler_import_bakes_heard_audio_into_project_media() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("device-loop.wav");
    let project_path = directory.path().join("warp-device.vzp");
    let raw = one_second_audio();
    vibez_audio_io::file_io::write_wav_file(&source_path, &raw).unwrap();
    let source = vibez_project::project_format_v1::stage_local_file(&source_path).unwrap();
    let mut track = TrackInfo::new("Sampler");
    track.kind = TrackKind::Midi;
    track.instrument = Some(InstrumentKind::Sampler);
    let (warped, original, staged) = prepare_browser_import_audio_async(
        crate::message::BrowserImportTarget::Sampler(track.id),
        crate::state::AuditionImportInput {
            mode: crate::state::AuditionMode::Warp,
            source_bpm: Some(120.0),
        },
        raw,
        source,
        60.0,
        44_100,
    )
    .await
    .unwrap();
    assert!(original.is_none(), "device media is the baked WARP buffer");
    track.native_instrument = Some(InstrumentStateInfo::Sampler {
        params: Vec::new(),
        source: Some(staged),
    });
    let project = Project {
        reduced_latency_monitoring: false,
        tracks: vec![track],
        ..Project::default()
    };
    vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();
    std::fs::remove_file(source_path).unwrap();

    let loaded = load_project_async(
        project_path,
        None,
        vibez_core::constants::DEFAULT_SAMPLE_RATE,
    )
    .await
    .unwrap();
    assert_eq!(loaded.sampler_samples.len(), 1);
    assert_eq!(
        loaded.sampler_samples[0].audio.num_frames(),
        warped.num_frames()
    );
    assert!(matches!(
        loaded.project.tracks[0]
            .native_instrument
            .as_ref()
            .and_then(|state| match state {
                InstrumentStateInfo::Sampler { source, .. } => source.as_ref(),
                _ => None,
            }),
        Some(MediaSourceRef::ProjectMedia { .. })
    ));
}

#[tokio::test]
async fn v1_reopen_decodes_embedded_audio_after_source_removal() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("source.wav");
    let project_path = directory.path().join("self-contained.vzp");
    let audio = DecodedAudio {
        channels: vec![vec![0.0, 0.25, -0.5, 0.75, -1.0]],
        sample_rate: 44_100,
    };
    vibez_audio_io::file_io::write_wav_file(&source_path, &audio).unwrap();
    let track = TrackInfo::new("Audio");
    let project = Project {
        reduced_latency_monitoring: false,
        tracks: vec![track.clone()],
        arrange: vibez_project::TimelineInfo {
            clips: vec![ClipInfo {
                id: ClipId::new(),
                track_id: track.id,
                name: "source.wav".into(),
                position: 0,
                source_offset: 0,
                start_marker: None,
                duration: audio.num_frames() as u64,
                source: Some(MediaSourceRef::LocalFile {
                    path: source_path.clone(),
                }),
                file_path: Some(source_path.clone()),
                loop_enabled: false,
                loop_start: 0,
                loop_end: audio.num_frames() as u64,
                gain_db: Default::default(),
                fades: Default::default(),
                playback_direction: Default::default(),
                transient_markers: Default::default(),
                warp_markers: Default::default(),
                transpose: Default::default(),
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
            }],
            ..vibez_project::TimelineInfo::default()
        },
        ..Project::default()
    };
    vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();
    std::fs::remove_file(source_path).unwrap();

    let loaded = load_project_async(
        project_path,
        None,
        vibez_core::constants::DEFAULT_SAMPLE_RATE,
    )
    .await
    .unwrap();
    assert_eq!(loaded.clips.len(), 1);
    assert_eq!(loaded.clips[0].audio.num_frames(), audio.num_frames());
    assert!(matches!(
        loaded.project.arrange.clips[0].source,
        Some(MediaSourceRef::ProjectMedia { .. })
    ));
    assert_eq!(loaded.project.tracks[0].id, track.id);
}

#[tokio::test]
async fn shortened_section_audio_and_automation_survive_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("shared.wav");
    let project_path = directory.path().join("sections.vzp");
    let audio = DecodedAudio {
        channels: vec![vec![0.0, 0.25, -0.5, 0.75, -1.0]],
        sample_rate: 44_100,
    };
    vibez_audio_io::file_io::write_wav_file(&source_path, &audio).unwrap();
    let track = TrackInfo::new("Audio");
    let track_id = track.id;
    let section_id = vibez_core::id::SectionId::new();
    let clip = |id, position| ClipInfo {
        id,
        track_id,
        name: "shared.wav".into(),
        position,
        source_offset: 0,
        start_marker: None,
        duration: audio.num_frames() as u64,
        source: Some(MediaSourceRef::LocalFile {
            path: source_path.clone(),
        }),
        file_path: Some(source_path.clone()),
        loop_enabled: false,
        loop_start: 0,
        loop_end: audio.num_frames() as u64,
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
    let mut section_lane = vibez_core::automation::AutomationLane::new(
        vibez_core::automation::AutomationTarget::TrackGain,
    );
    section_lane.insert_point(vibez_core::automation::AutomationPoint {
        beat: 12.0,
        value: 0.25,
        curve: 0.4,
    });
    let beyond_boundary_position = 132_300;
    let project = Project {
        reduced_latency_monitoring: false,
        tracks: vec![track],
        arrange: vibez_project::TimelineInfo {
            clips: vec![clip(ClipId::new(), 0)],
            ..vibez_project::TimelineInfo::default()
        },
        sections: vec![vibez_project::SectionInfo {
            id: section_id,
            slot: 7,
            name: "Breakdown".into(),
            length_beats: 4.0,
            launch_quantization: vibez_project::SectionLaunchQuantization::EndOfSection,
            looping: false,
            timeline: vibez_project::TimelineInfo {
                clips: vec![clip(ClipId::new(), beyond_boundary_position)],
                automation: vec![vibez_project::TimelineAutomationInfo {
                    track_id,
                    lanes: vec![section_lane.clone()],
                }],
                ..vibez_project::TimelineInfo::default()
            },
        }],
        ..Project::default()
    };
    let saved =
        vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();
    assert_eq!(saved.observation.media_rows_written, 1);
    std::fs::remove_file(source_path).unwrap();

    let loaded = load_project_async(
        project_path,
        None,
        vibez_core::constants::DEFAULT_SAMPLE_RATE,
    )
    .await
    .unwrap();
    assert!(loaded.warnings.is_empty());
    assert_eq!(loaded.clips.len(), 2);
    assert!(loaded
        .clips
        .iter()
        .any(|clip| clip.location == vibez_project::TimelineLocation::Arrange));
    assert!(loaded
        .clips
        .iter()
        .any(|clip| { clip.location == vibez_project::TimelineLocation::Section(section_id) }));
    assert!(loaded
        .clips
        .iter()
        .all(|clip| clip.audio.num_frames() == audio.num_frames()));
    let section = &loaded.project.sections[0];
    assert_eq!(section.id, section_id);
    assert_eq!(section.slot, 7);
    assert_eq!(section.name, "Breakdown");
    assert_eq!(section.length_beats, 4.0);
    assert_eq!(
        section.launch_quantization,
        vibez_project::SectionLaunchQuantization::EndOfSection
    );
    assert!(!section.looping);
    assert_eq!(section.timeline.clips[0].position, beyond_boundary_position);
    assert_eq!(section.timeline.automation[0].lanes, vec![section_lane]);
}

#[tokio::test]
async fn legacy_json_reopens_with_an_empty_section_store_and_no_warnings() {
    let directory = tempfile::tempdir().unwrap();
    let project_path = directory.path().join("legacy.json");
    std::fs::write(
        &project_path,
        r#"{
            "name": "Legacy",
            "bpm": 120.0,
            "sample_rate": 44100,
            "tracks": [],
            "clips": [],
            "note_clips": [],
            "master": null,
            "buses": []
        }"#,
    )
    .unwrap();

    let loaded = load_project_async(
        project_path,
        None,
        vibez_core::constants::DEFAULT_SAMPLE_RATE,
    )
    .await
    .unwrap();
    assert!(loaded.project.sections.is_empty());
    assert!(loaded.warnings.is_empty());
    assert!(loaded.clips.is_empty());
}

#[tokio::test]
async fn unavailable_media_clip_is_kept_for_relink_on_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let project_path = directory.path().join("relink.vzp");
    let track = TrackInfo::new("Audio");
    let clip_id = ClipId::new();
    let project = Project {
        reduced_latency_monitoring: false,
        tracks: vec![track.clone()],
        arrange: vibez_project::TimelineInfo {
            clips: vec![ClipInfo {
                id: clip_id,
                track_id: track.id,
                name: "Remote clip".into(),
                position: 0,
                source_offset: 0,
                start_marker: None,
                duration: 128,
                source: Some(MediaSourceRef::DropboxFile {
                    path_lower: "/megalodon/pad.wav".into(),
                    display_path: "/Megalodon/Pad.wav".into(),
                    rev: Some("rev-1".into()),
                }),
                file_path: None,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 128,
                gain_db: Default::default(),
                fades: Default::default(),
                playback_direction: Default::default(),
                transient_markers: Default::default(),
                warp_markers: Default::default(),
                transpose: Default::default(),
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
            }],
            ..vibez_project::TimelineInfo::default()
        },
        ..Project::default()
    };
    vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();

    // Without a Dropbox connection the clip cannot hydrate, but its
    // source reference must survive for relink instead of being
    // silently dropped from the next save.
    let loaded = load_project_async(
        project_path,
        None,
        vibez_core::constants::DEFAULT_SAMPLE_RATE,
    )
    .await
    .unwrap();
    assert!(loaded.clips.is_empty());
    assert_eq!(loaded.unresolved_clips.len(), 1);
    assert_eq!(loaded.unresolved_clips[0].id, clip_id);
    assert!(matches!(
        loaded.unresolved_clips[0].source,
        Some(MediaSourceRef::DropboxFile { .. })
    ));
    assert!(loaded
        .warnings
        .iter()
        .any(|warning| warning.contains("kept for relink")));
}

#[tokio::test]
async fn cached_remote_media_materializes_without_a_client_and_persists_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("source.wav");
    let audio = DecodedAudio {
        channels: vec![vec![0.0, 0.5, -0.5, 0.25]],
        sample_rate: 44_100,
    };
    vibez_audio_io::file_io::write_wav_file(&source_path, &audio).unwrap();
    let cache = DropboxCache::with_root(directory.path().join("media-cache"));
    cache
        .write(
            "/megalodon/source.wav",
            Some("rev-1"),
            &std::fs::read(source_path).unwrap(),
        )
        .unwrap();
    let entry = DropboxEntry {
        path_lower: "/megalodon/source.wav".into(),
        path_display: "/Megalodon/Source.wav".into(),
        name: "Source.wav".into(),
        is_folder: false,
        rev: Some("rev-1".into()),
        size: None,
    };
    let lease = cache.protect(&entry.path_lower, entry.rev.as_deref());
    let materialized = materialize_remote_sample_async(None, cache.clone(), entry, lease, false)
        .await
        .unwrap();
    assert_eq!(materialized.audio.num_frames(), audio.num_frames());
    assert_eq!(
        materialized.metadata.provider_revision.as_deref(),
        Some("rev-1")
    );
    assert_eq!(materialized.metadata.channels, 1);
    assert_eq!(materialized.metadata.sample_rate, 44_100);
    assert!(cache
        .derived_metadata("/megalodon/source.wav", Some("rev-1"))
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn remote_import_reuses_the_cached_audition_tempo() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("fast-loop.wav");
    let sample_rate = 44_100_u32;
    let source_bpm = 190.0;
    let frames = (4.0 * 4.0 * 60.0 / source_bpm * sample_rate as f64).round() as usize;
    let beat_frames = (60.0 / source_bpm * sample_rate as f64).round() as usize;
    let mut channel = vec![0.0_f32; frames];
    for beat in (0..frames).step_by(beat_frames) {
        for offset in 0..256.min(frames - beat) {
            channel[beat + offset] = 0.9 * (1.0 - offset as f32 / 256.0);
        }
    }
    let audio = DecodedAudio {
        channels: vec![channel],
        sample_rate,
    };
    vibez_audio_io::file_io::write_wav_file(&source_path, &audio).unwrap();
    let cache = DropboxCache::with_root(directory.path().join("media-cache"));
    cache
        .write(
            "/megalodon/fast-loop.wav",
            Some("rev-old-analysis"),
            &std::fs::read(&source_path).unwrap(),
        )
        .unwrap();
    cache
        .store_derived_metadata(
            "/megalodon/fast-loop.wav",
            Some("rev-old-analysis"),
            vibez_dropbox::DerivedMetadata {
                provider_revision: Some("rev-old-analysis".into()),
                duration_seconds: audio.duration_seconds(),
                channels: 1,
                sample_rate,
                bpm: Some(95.0),
                bpm_confidence: Some(0.9),
                waveform_peaks: vec![],
            },
        )
        .unwrap();
    let entry = DropboxEntry {
        path_lower: "/megalodon/fast-loop.wav".into(),
        path_display: "/Megalodon/Fast Loop.wav".into(),
        name: "Fast Loop.wav".into(),
        is_folder: false,
        rev: Some("rev-old-analysis".into()),
        size: None,
    };

    let (analysed, _, _) = fetch_dropbox_sample_async(None, cache, entry)
        .await
        .unwrap();

    assert!((analysed.loop_fit.unwrap().source_bpm - 95.0).abs() < 0.01);
}

#[tokio::test]
async fn remote_warp_import_reopens_after_cache_clear_without_dropbox() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("remote-loop.wav");
    let project_path = directory.path().join("remote-owned.vzp");
    let raw = one_second_audio();
    vibez_audio_io::file_io::write_wav_file(&source_path, &raw).unwrap();
    let cache = DropboxCache::with_root(directory.path().join("media-cache"));
    cache
        .write(
            "/megalodon/remote-loop.wav",
            Some("rev-9"),
            &std::fs::read(&source_path).unwrap(),
        )
        .unwrap();
    let entry = DropboxEntry {
        path_lower: "/megalodon/remote-loop.wav".into(),
        path_display: "/Megalodon/Remote Loop.wav".into(),
        name: "Remote Loop.wav".into(),
        is_folder: false,
        rev: Some("rev-9".into()),
        size: None,
    };
    let (analysed, name, staged) = fetch_dropbox_sample_async(None, cache.clone(), entry)
        .await
        .unwrap();
    assert!(matches!(
        staged,
        MediaSourceRef::StagedRemoteProjectMedia { .. }
    ));
    let treatment = crate::state::AuditionImportInput {
        mode: crate::state::AuditionMode::Warp,
        source_bpm: Some(120.0),
    };
    let (warped, original, staged) = prepare_browser_import_audio_async(
        crate::message::BrowserImportTarget::ArrangementNewTrackAt {
            position_samples: 0,
        },
        treatment,
        analysed.audio,
        staged,
        60.0,
        44_100,
    )
    .await
    .unwrap();
    assert_eq!(warped.num_frames(), 88_200);
    assert_eq!(original.unwrap().num_frames(), raw.num_frames());

    cache.clear().unwrap();
    std::fs::remove_file(source_path).unwrap();
    let track = TrackInfo::new("Audio");
    let project = Project {
        reduced_latency_monitoring: false,
        tracks: vec![track.clone()],
        arrange: vibez_project::TimelineInfo {
            clips: vec![ClipInfo {
                id: ClipId::new(),
                track_id: track.id,
                name,
                position: 0,
                source_offset: 0,
                start_marker: None,
                duration: warped.num_frames() as u64,
                source: Some(staged),
                file_path: None,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                gain_db: Default::default(),
                fades: Default::default(),
                playback_direction: Default::default(),
                transient_markers: Default::default(),
                warp_markers: Default::default(),
                transpose: Default::default(),
                original_bpm: Some(120.0),
                warped: true,
                warped_to_bpm: Some(60.0),
            }],
            ..vibez_project::TimelineInfo::default()
        },
        ..Project::default()
    };
    vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();

    let reopened = load_project_async(
        project_path.clone(),
        None,
        vibez_core::constants::DEFAULT_SAMPLE_RATE,
    )
    .await
    .unwrap();
    assert_eq!(reopened.clips[0].audio.num_frames(), warped.num_frames());
    assert_audible(Arc::clone(&reopened.clips[0].audio), "Remote WARP reopen");
    let Some(MediaSourceRef::ProjectMedia {
        provenance: Some(provenance),
        ..
    }) = reopened.project.arrange.clips[0].source.as_ref()
    else {
        panic!("reopened clip must carry Remote provenance on Project Media");
    };
    let vibez_core::track::MediaProvenance::Remote {
        provider,
        connection_id,
        connection_name,
        source_path,
        revision,
        ..
    } = provenance.as_ref()
    else {
        panic!("reopened clip provenance must remain Remote");
    };
    assert_eq!(provider, "dropbox");
    assert_eq!(connection_id, "dropbox-primary");
    assert_eq!(connection_name.as_deref(), Some("Dropbox"));
    assert_eq!(source_path, "/Megalodon/Remote Loop.wav");
    assert_eq!(revision.as_deref(), Some("rev-9"));
    let serialized = serde_json::to_string(
        &vibez_project::project_format_v1::ProjectContainer::open(project_path)
            .unwrap()
            .load_document()
            .unwrap(),
    )
    .unwrap();
    assert!(!serialized.contains("access_token"));
    assert!(!serialized.contains("refresh_token"));
}

#[tokio::test]
async fn dropping_a_debounced_uncached_request_before_200ms_materializes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let cache = DropboxCache::with_root(directory.path().join("media-cache"));
    let entry = DropboxEntry {
        path_lower: "/megalodon/transient.wav".into(),
        path_display: "/Megalodon/Transient.wav".into(),
        name: "Transient.wav".into(),
        is_folder: false,
        rev: Some("rev-1".into()),
        size: None,
    };
    let lease = cache.protect(&entry.path_lower, entry.rev.as_deref());
    let future = materialize_remote_sample_async(None, cache.clone(), entry, lease, true);
    tokio::pin!(future);
    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {}
        result = &mut future => panic!("debounced request completed early: {result:?}"),
    }
    assert!(!cache.is_cached("/megalodon/transient.wav", Some("rev-1")));
    assert_eq!(cache.usage().unwrap(), vibez_dropbox::CacheUsage::default());
}

#[tokio::test]
async fn uncached_degraded_materialization_requires_explicit_reconnect() {
    let directory = tempfile::tempdir().unwrap();
    let cache = DropboxCache::with_root(directory.path().join("media-cache"));
    let entry = DropboxEntry {
        path_lower: "/megalodon/uncached.wav".into(),
        path_display: "/Megalodon/Uncached.wav".into(),
        name: "Uncached.wav".into(),
        is_folder: false,
        rev: Some("rev-1".into()),
        size: None,
    };
    let lease = cache.protect(&entry.path_lower, entry.rev.as_deref());
    let error = materialize_remote_sample_async(None, cache, entry, lease, false)
        .await
        .unwrap_err();
    assert!(error.contains("Reconnect Required"));
}

/// An App holding one Arrange Audio track with a break chopped into two
/// selected Clips backed by staged Project Media.
fn app_with_two_selected_break_clips(directory: &Path) -> (App, vibez_core::id::TrackId) {
    let source_path = directory.join("break.wav");
    vibez_audio_io::file_io::write_wav_file(&source_path, &one_second_audio()).unwrap();
    let source = vibez_project::project_format_v1::stage_local_file(&source_path).unwrap();
    let mut app = super::test_support::app();
    let track = crate::state::ProjectTrack::new(vibez_core::id::TrackId::new(), "Breaks".into(), 0);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    for position in [0, 22_050] {
        let clip = crate::state::UiClip {
            name: "break.wav".into(),
            audio: one_second_audio(),
            source: Some(source.clone()),
            ..crate::state::UiClip::test_clip(position, 22_050)
        };
        app.state.arrangement.selected_clips.insert(
            crate::state::ArrangementSelection::AudioClip {
                track_id,
                clip_id: clip.id,
            },
        );
        app.state.arrange_content_mut(track_id).clips.push(clip);
    }
    (app, track_id)
}

#[tokio::test]
async fn joined_audio_clips_survive_save_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let project_path = directory.path().join("jungle.vzp");
    let (mut app, track_id) = app_with_two_selected_break_clips(directory.path());

    super::test_support::join_selected_audio_clips(&mut app, track_id).await;
    let _ = app.update(Message::Arrangement(
        crate::domains::arrangement::ArrangementMsg::DuplicateSelectedClip,
    ));
    let clips = &app.state.arrange_content(track_id).unwrap().clips;
    assert_eq!(clips.len(), 2);
    let Some(MediaSourceRef::StagedProjectMedia { staging_path, .. }) = clips[0].source.clone()
    else {
        panic!("joined clip has no staged media: {:?}", clips[0].source);
    };
    save_project_async(project_path.clone(), None, app.project_for_save())
        .await
        .unwrap();
    std::fs::remove_file(staging_path).unwrap();
    let reopened = load_project_async(project_path, None, 44_100)
        .await
        .unwrap();

    assert!(reopened.warnings.is_empty(), "{:?}", reopened.warnings);
    assert_eq!(reopened.clips.len(), 2, "joined clip audio lost on reopen");
    assert!(reopened
        .clips
        .iter()
        .all(|clip| clip.clip.audio.num_frames() == 44_100));
}

#[tokio::test]
async fn joined_audio_clip_survives_sample_rate_conversion() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_two_selected_break_clips(directory.path());

    super::test_support::join_selected_audio_clips(&mut app, track_id).await;
    let converted = super::tasks::convert_project_sample_rate_async(
        super::tasks::SampleRateConversion {
            project: app.project_for_save(),
            path: None,
            dirty: true,
            sample_rate: 48_000,
        },
        None,
    )
    .await;

    assert!(
        converted.loaded.warnings.is_empty(),
        "{:?}",
        converted.loaded.warnings
    );
    assert_eq!(converted.loaded.clips.len(), 1);
}

/// One second of decaying noise bursts, so Quantize finds transients.
fn drum_hits() -> Arc<DecodedAudio> {
    let mut samples = vec![0.0f32; 44_100];
    for hit in [3_000usize, 14_000, 25_000, 36_000] {
        for (offset, sample) in samples[hit..hit + 4_000].iter_mut().enumerate() {
            let noise = if (offset * 7919) % 13 < 6 { 1.0 } else { -1.0 };
            *sample = noise * 0.9 * (-(offset as f32) / 600.0).exp();
        }
    }
    Arc::new(DecodedAudio {
        channels: vec![samples],
        sample_rate: 44_100,
    })
}

#[tokio::test]
async fn quantized_audio_clip_survives_save_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let project_path = directory.path().join("quantized.vzp");
    let mut app = super::test_support::app();
    let track = crate::state::ProjectTrack::new(vibez_core::id::TrackId::new(), "Drums".into(), 0);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    let clip = crate::state::UiClip {
        name: "hits.wav".into(),
        audio: drum_hits(),
        ..crate::state::UiClip::test_clip(0, 44_100)
    };
    let clip_id = clip.id;
    app.state
        .arrange_content_mut(track_id)
        .clips
        .push(clip.clone());

    let result = super::tasks::quantize_audio_clip_async(super::audio_tasks::QuantizeInput {
        audio: Arc::clone(&clip.audio),
        bpm: 120.0,
        sample_rate: 44_100,
        grid: crate::state::SnapGrid::SIXTEENTH,
        clip_position: 0,
        clip_source_offset: 0,
        clip_duration: 44_100,
        original_name: clip.name,
        new_clip_id: ClipId::new(),
    })
    .await;
    assert!(result.is_ok(), "{result:?}");
    let quantized_frames = result
        .as_ref()
        .map_or(0, |success| success.new_audio.num_frames());
    let _ = app.update(Message::AudioQuantizeReady {
        location: vibez_project::TimelineLocation::Arrange,
        track_id,
        old_clip_id: clip_id,
        result,
    });
    save_project_async(project_path.clone(), None, app.project_for_save())
        .await
        .unwrap();
    let reopened = load_project_async(project_path, None, 44_100)
        .await
        .unwrap();

    assert!(reopened.warnings.is_empty(), "{:?}", reopened.warnings);
    assert_eq!(
        reopened.clips.len(),
        1,
        "quantized clip audio lost on reopen"
    );
    assert_eq!(reopened.clips[0].clip.audio.num_frames(), quantized_frames);
    assert!(matches!(
        reopened.project.arrange.clips[0].source,
        Some(MediaSourceRef::ProjectMedia { .. })
    ));
}
