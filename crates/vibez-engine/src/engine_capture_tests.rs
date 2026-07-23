//! Capture timestamp regressions at the public engine command/event seam.

use super::*;

use vibez_core::id::{SectionId, TrackId};

use crate::playback_source::{PreparedPlaybackSource, PreparedSectionPlaybackSource};

fn empty_section(section_id: SectionId, track_id: TrackId) -> Box<PreparedSectionPlaybackSource> {
    Box::new(PreparedSectionPlaybackSource::new(
        section_id,
        4.0,
        true,
        vec![(
            track_id,
            PreparedPlaybackSource::new(Vec::new(), Vec::new(), Vec::new()),
        )],
    ))
}

fn capture_started(events: &mut rtrb::Consumer<EngineEvent>) -> Option<(u64, SectionId, u64)> {
    std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::PerformanceCaptureStarted {
            effective_at_samples,
            section_id: Some(section_id),
            section_position_samples: Some(section_position_samples),
        } => Some((effective_at_samples, section_id, section_position_samples)),
        _ => None,
    })
}

#[test]
fn capture_start_reports_exact_transport_and_active_section_position() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(empty_section(
            section_id, track_id,
        )))
        .unwrap();
    engine.process(&mut [0.0; 3], 1);
    while events.pop().is_ok() {}

    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    engine.process(&mut [0.0; 5], 1);

    assert_eq!(capture_started(&mut events), Some((3, section_id, 3)));
}

#[test]
fn capture_stop_reports_callback_boundary_without_ui_tick_timing() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0; 7], 1);
    while events.pop().is_ok() {}

    commands
        .push(EngineCommand::StopPerformanceCapture)
        .unwrap();
    engine.process(&mut [0.0; 3], 1);

    let stopped = std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::PerformanceCaptureStopped {
            effective_at_samples,
        } => Some(effective_at_samples),
        _ => None,
    });
    assert_eq!(stopped, Some(7));
}

#[test]
fn transport_stop_ends_capture_and_section_playback_at_one_boundary() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(empty_section(
            section_id, track_id,
        )))
        .unwrap();
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    engine.process(&mut [0.0; 7], 1);
    while events.pop().is_ok() {}

    commands.push(EngineCommand::Stop).unwrap();
    engine.process(&mut [0.0; 3], 1);

    let terminal: Vec<_> = std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => Some(("capture", effective_at_samples)),
            EngineEvent::PlaybackStopped => Some(("playback", 7)),
            _ => None,
        })
        .collect();
    assert_eq!(terminal, [("capture", 7), ("playback", 7)]);
    assert!(!engine.transport().is_playing());
    assert!(engine.active_section.is_none());
}
