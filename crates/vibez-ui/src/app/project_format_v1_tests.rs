//! Project format v1 integration tests. Split from app/mod.rs.

use super::*;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::ClipId;
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::track::{InstrumentStateInfo, TrackInfo};
use vibez_engine::commands::{AuditionSync, EngineCommand};

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
            sync: AuditionSync::Off,
            looped: false,
        })
        .unwrap();
    let mut output = vec![0.0_f32; 8_192];
    engine.process(&mut output, 2);
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
        assert_eq!(audition.num_channels(), channels, "{file_name}");
        assert_eq!(audition.sample_rate, sample_rate, "{file_name}");
        assert_audible(Arc::clone(&audition), file_name);

        let mut browser = crate::state::BrowserState {
            entries: catalog.entries,
            ..crate::state::BrowserState::default()
        };
        browser.select_source(source.clone());
        let generation = browser.begin_audition_load(&source);
        assert!(browser.install_audition(
            generation,
            browser.selected_source.clone().unwrap(),
            Arc::clone(&audition)
        ));
        let metadata = &browser.entries[0];
        assert_eq!(metadata.channels, Some(channels));
        assert_eq!(metadata.sample_rate, Some(sample_rate));
        assert!(metadata
            .duration_seconds
            .is_some_and(|duration| duration > 0.0));

        let (imported, staged) = decode_and_stage_local_async(source_path.clone())
            .await
            .unwrap();
        assert_eq!(imported.num_frames(), audition.num_frames(), "{file_name}");
        let track = TrackInfo::new("Audio");
        let project_path = directory.path().join("format-roundtrip.vzp");
        let project = Project {
            tracks: vec![track.clone()],
            arrange: vibez_project::TimelineInfo {
                clips: vec![ClipInfo {
                    id: ClipId::new(),
                    track_id: track.id,
                    name: file_name.into(),
                    position: 0,
                    source_offset: 0,
                    duration: imported.num_frames() as u64,
                    source: Some(staged),
                    file_path: None,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: 0,
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

        let reopened = load_project_async(project_path, None).await.unwrap();
        let reopened_audio = Arc::clone(&reopened.clips[0].audio);
        assert_eq!(reopened_audio.num_channels(), channels, "{file_name}");
        assert_eq!(reopened_audio.sample_rate, sample_rate, "{file_name}");
        assert_eq!(
            reopened_audio.num_frames(),
            imported.num_frames(),
            "{file_name}"
        );
        assert!(matches!(
            reopened.project.arrange.clips[0].source,
            Some(MediaSourceRef::ProjectMedia { .. })
        ));
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
    let import_error = decode_and_stage_local_async(source_path).await.unwrap_err();
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
    )
    .await
    .unwrap();
    assert_eq!(warped.num_frames(), 88_200);
    assert_eq!(original.unwrap().num_frames(), raw.num_frames());

    let track = TrackInfo::new("Audio");
    let project = Project {
        tracks: vec![track.clone()],
        arrange: vibez_project::TimelineInfo {
            clips: vec![ClipInfo {
                id: ClipId::new(),
                track_id: track.id,
                name: "loop.wav".into(),
                position: 0,
                source_offset: 0,
                duration: warped.num_frames() as u64,
                source: Some(staged),
                file_path: None,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
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

    let loaded = load_project_async(project_path, None).await.unwrap();
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
    )
    .await
    .unwrap();
    assert!(original.is_none(), "device media is the baked WARP buffer");
    track.native_instrument = Some(InstrumentStateInfo::Sampler {
        params: Vec::new(),
        source: Some(staged),
    });
    let project = Project {
        tracks: vec![track],
        ..Project::default()
    };
    vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();
    std::fs::remove_file(source_path).unwrap();

    let loaded = load_project_async(project_path, None).await.unwrap();
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
        tracks: vec![track.clone()],
        arrange: vibez_project::TimelineInfo {
            clips: vec![ClipInfo {
                id: ClipId::new(),
                track_id: track.id,
                name: "source.wav".into(),
                position: 0,
                source_offset: 0,
                duration: audio.num_frames() as u64,
                source: Some(MediaSourceRef::LocalFile {
                    path: source_path.clone(),
                }),
                file_path: Some(source_path.clone()),
                loop_enabled: false,
                loop_start: 0,
                loop_end: audio.num_frames() as u64,
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

    let loaded = load_project_async(project_path, None).await.unwrap();
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
        duration: audio.num_frames() as u64,
        source: Some(MediaSourceRef::LocalFile {
            path: source_path.clone(),
        }),
        file_path: Some(source_path.clone()),
        loop_enabled: false,
        loop_start: 0,
        loop_end: audio.num_frames() as u64,
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

    let loaded = load_project_async(project_path, None).await.unwrap();
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

    let loaded = load_project_async(project_path, None).await.unwrap();
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
        tracks: vec![track.clone()],
        arrange: vibez_project::TimelineInfo {
            clips: vec![ClipInfo {
                id: clip_id,
                track_id: track.id,
                name: "Remote clip".into(),
                position: 0,
                source_offset: 0,
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
    let loaded = load_project_async(project_path, None).await.unwrap();
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
    let (decoded, name, staged) = fetch_dropbox_sample_async(None, cache.clone(), entry)
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
        decoded,
        staged,
        60.0,
    )
    .await
    .unwrap();
    assert_eq!(warped.num_frames(), 88_200);
    assert_eq!(original.unwrap().num_frames(), raw.num_frames());

    cache.clear().unwrap();
    std::fs::remove_file(source_path).unwrap();
    let track = TrackInfo::new("Audio");
    let project = Project {
        tracks: vec![track.clone()],
        arrange: vibez_project::TimelineInfo {
            clips: vec![ClipInfo {
                id: ClipId::new(),
                track_id: track.id,
                name,
                position: 0,
                source_offset: 0,
                duration: warped.num_frames() as u64,
                source: Some(staged),
                file_path: None,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: Some(120.0),
                warped: true,
                warped_to_bpm: Some(60.0),
            }],
            ..vibez_project::TimelineInfo::default()
        },
        ..Project::default()
    };
    vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();

    let reopened = load_project_async(project_path.clone(), None)
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
    assert_eq!(connection_name.as_deref(), Some("Alex's Dropbox"));
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
