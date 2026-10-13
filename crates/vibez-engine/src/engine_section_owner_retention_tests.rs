//! Section owners survive a blocked UI until their original allocation can be retired there.

use super::*;
use crate::playback_source::{EngineClip, PreparedPlaybackSource, PreparedSectionPlaybackSource};
use std::sync::{Arc, Weak};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::ClipId;

fn source(
    section_id: SectionId,
    track_id: TrackId,
) -> (Box<PreparedSectionPlaybackSource>, Weak<DecodedAudio>) {
    let audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.5; 128]],
        sample_rate: 8,
    });
    let weak = Arc::downgrade(&audio);
    let prepared = Box::new(PreparedSectionPlaybackSource::new(
        section_id,
        8.0,
        true,
        vec![(
            track_id,
            PreparedPlaybackSource::new(
                vec![EngineClip {
                    id: ClipId::new(),
                    audio,
                    position: 0,
                    source_offset: 0,
                    start_marker: 0,
                    duration: 128,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: 0,
                    linear_gain: 1.0,
                    fades: Default::default(),
                    playback_direction: Default::default(),
                    warp_markers: Default::default(),
                }],
                Vec::new(),
                Vec::new(),
            ),
        )],
    ));
    (prepared, weak)
}

fn setup() -> (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    rtrb::Consumer<EngineEvent>,
    TrackId,
) {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    engine.process(&mut [0.0; 8], 1);
    while events.pop().is_ok() {}
    (engine, commands, events, track_id)
}

fn process_without_memory_operations(engine: &mut AudioEngine) {
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 4], 1)),
        (0, 0)
    );
}

#[test]
fn rejected_section_launch_retains_original_owner_until_ui_delivery() {
    let (mut engine, mut commands, mut events, track_id) = setup();
    let (recording, _) = source(SectionId::new(), track_id);
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id: recording.section_id,
            track_id,
            prepared: Some(recording),
            count_in_bars: 4,
            replace_existing: false,
        })
        .unwrap();
    engine.process(&mut [0.0; 1], 1);
    while events.pop().is_ok() {}

    let section_id = SectionId::new();
    let (rejected, audio) = source(section_id, track_id);
    let original = &*rejected as *const PreparedSectionPlaybackSource;
    commands
        .push(EngineCommand::LaunchSection(rejected))
        .unwrap();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    process_without_memory_operations(&mut engine);
    assert!(audio.upgrade().is_some());
    while events.pop().is_ok() {}
    process_without_memory_operations(&mut engine);

    let retired = std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::SectionQueueCancelled { retired } if retired.section_id == section_id => {
            Some(retired)
        }
        _ => None,
    });
    let retired = retired.expect("the rejected Section owner must eventually reach the UI");
    assert_eq!(&*retired as *const PreparedSectionPlaybackSource, original);
    assert!(audio.upgrade().is_some());
    drop(retired);
    assert!(audio.upgrade().is_none());
}

#[test]
fn stopped_section_record_retains_original_count_in_owner_until_ui_delivery() {
    let (mut engine, mut commands, mut events, track_id) = setup();
    let section_id = SectionId::new();
    let (recording, audio) = source(section_id, track_id);
    let original = &*recording as *const PreparedSectionPlaybackSource;
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id,
            prepared: Some(recording),
            count_in_bars: 4,
            replace_existing: false,
        })
        .unwrap();
    engine.process(&mut [0.0; 1], 1);
    while events.pop().is_ok() {}

    commands.push(EngineCommand::StopSectionRecord).unwrap();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    process_without_memory_operations(&mut engine);
    assert!(audio.upgrade().is_some());
    while events.pop().is_ok() {}
    process_without_memory_operations(&mut engine);

    let retired = std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::SectionRecordStopped {
            section_id: actual,
            track_id: actual_track,
            started,
            retired,
            ..
        } if actual == section_id => {
            assert_eq!(actual_track, track_id);
            assert!(!started);
            retired
        }
        _ => None,
    });
    let retired = retired.expect("the stopped count-in owner must eventually reach the UI");
    assert_eq!(&*retired as *const PreparedSectionPlaybackSource, original);
    assert!(audio.upgrade().is_some());
    drop(retired);
    assert!(audio.upgrade().is_none());
}
