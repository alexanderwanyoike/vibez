//! Routed Unload preserves resident media, source cleanup and terminal ownership under pressure.

use super::*;
use crate::events::SectionRecordStoppedEvent;
use crate::playback_source::EngineClip;
use vibez_core::id::ClipId;
fn stereo_ramp(frames: usize, left: f32, right: f32, sample_rate: u32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![
            (0..frames)
                .map(|frame| left + frame as f32 / 1024.0)
                .collect(),
            (0..frames)
                .map(|frame| right + frame as f32 / 2048.0)
                .collect(),
        ],
        sample_rate,
    })
}

fn fill_ui_ring(engine: &mut AudioEngine) -> usize {
    let capacity = engine.event_tx.slots();
    assert!(capacity > 0);
    for _ in 0..capacity {
        engine.event_tx.push(EngineEvent::PlaybackStarted).unwrap();
    }
    assert!(engine.event_tx.is_full());
    capacity
}

fn drain_full_ui_ring(events: &mut rtrb::Consumer<EngineEvent>, expected: usize) {
    let mut observed = 0;
    while let Ok(event) = events.pop() {
        assert!(matches!(event, EngineEvent::PlaybackStarted));
        observed += 1;
    }
    assert_eq!(observed, expected);
}

struct QueuedClipOwner {
    request: u64,
    prepared: *const crate::playback_source::PreparedClipPlayback,
    audio: std::sync::Weak<DecodedAudio>,
}

fn unique_clip_request(
    track: TrackId,
    request: u64,
) -> (
    Box<crate::playback_source::PreparedClipPlayback>,
    QueuedClipOwner,
) {
    use crate::playback_source::{EngineClip, PreparedClipPlayback, PreparedPlaybackSource};
    let clip_id = vibez_core::id::ClipId::new();
    let sample = stereo_ramp(128, 0.125, 0.0, 8);
    let audio = Arc::downgrade(&sample);
    let prepared = Box::new(PreparedClipPlayback {
        track_id: track,
        clip_id: Some(clip_id),
        request_id: request,
        length_samples: 128,
        looping: true,
        source: Box::new(PreparedPlaybackSource::new(
            vec![EngineClip {
                id: clip_id,
                audio: sample,
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
        )),
    });
    let owner = QueuedClipOwner {
        request,
        prepared: &*prepared,
        audio,
    };
    (prepared, owner)
}

#[test]
fn routed_unload_combined_source_capture_and_presentation_pressure_returns_all_unique_owners() {
    assert_combined_routed_unload_pressure(false);
}

#[test]
fn routed_unload_last_two_credits_defer_third_delayed_history_without_destroying_owners() {
    assert_combined_routed_unload_pressure(true);
}

fn assert_combined_routed_unload_pressure(delayed_minimum_credits: bool) {
    use crate::playback_source::PreparedSectionPlaybackSource;
    use vibez_core::perform::MusicalBoundary;

    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let tracks: Vec<_> = (0..40).map(|_| TrackId::new()).collect();
    let arrange_id = ClipId::new();
    let resident = stereo_ramp(512, 0.5, -0.25, 8);
    let resident_owner = Arc::downgrade(&resident);
    let resident_clip = EngineClip {
        id: arrange_id, audio: resident, position: 0, source_offset: 0,
        start_marker: 0, duration: 512, loop_enabled: false,
        loop_start: 0, loop_end: 0, linear_gain: 1.0,
        fades: Default::default(), playback_direction: Default::default(),
        warp_markers: Default::default(),
    };
    commands.push(EngineCommand::AddTrack(Box::new(EngineTrack::with_playback_source(
        tracks[0], crate::playback_source::PreparedPlaybackSource::new(
            vec![resident_clip], vec![], vec![],
        ),
    )))).unwrap();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    for track in &tracks[1..] {
        commands.push(EngineCommand::add_track(*track)).unwrap();
    }
    let mut delayed_sections = Vec::new();
    if delayed_minimum_credits {
        use vibez_core::routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode};
        let effect = vibez_core::id::EffectId::new();
        let slow = tracks[1];
        commands
            .push(EngineCommand::InsertEffect {
                track_id: slow,
                effect_id: effect,
                effect: Box::new(crate::test_support::DelayProbe::new(521)),
                position: None,
            })
            .unwrap();
        let mut channels: Vec<_> = tracks
            .iter()
            .map(|track| RoutingChannel {
                id: *track,
                is_bus: false,
                sends: vec![],
                effects: if *track == slow {
                    vec![RoutingEffect {
                        id: effect,
                        inputs: vec![],
                        assignments: vec![],
                        inactive_inputs: vec![],
                    }]
                } else {
                    vec![]
                },
            })
            .collect();
        channels.push(RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            effects: vec![],
            sends: vec![],
        });
        let reports = [(
            RoutingNode {
                channel: slow,
                stage: NodeStage::Effect(effect),
            },
            521,
        )];
        commands
            .push(EngineCommand::SetRouting(
                crate::routing::PreparedRouting::prepare_compensated(
                    &channels,
                    64,
                    &reports,
                    &[tracks[0]],
                    1,
                )
                .unwrap(),
            ))
            .unwrap();
        let section = SectionId::new();
        for _ in 0..3 {
            let prepared = Box::new(PreparedSectionPlaybackSource::new(
                section,
                8.0,
                true,
                vec![],
            ));
            delayed_sections.push(&*prepared as *const PreparedSectionPlaybackSource);
            commands
                .push(EngineCommand::LaunchSection(prepared))
                .unwrap();
        }
    } else {
        let mut channels: Vec<_> = tracks.iter().copied()
            .map(graph_test_support::routing_channel).collect();
        channels.push(graph_test_support::routing_channel(TrackId::MASTER));
        commands.push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&channels, 64).unwrap(),
        )).unwrap();
    }
    commands.push(EngineCommand::BeginClipPerformance).unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [0.0; 1], 1));
    while events.pop().is_ok() {}
    let mut clips = Vec::new();
    let mut expected_clips = Vec::new();
    for (index, track) in tracks.iter().enumerate() {
        let (prepared, owner) = unique_clip_request(*track, index as u64 + 1);
        clips.push(prepared);
        expected_clips.push(owner);
    }
    commands
        .push(EngineCommand::QueueClips {
            clips,
            quantization: MusicalBoundary::OneBar,
        })
        .unwrap();
    let section_id = SectionId::new();
    let (mut recording_clip, recording_audio) = unique_clip_request(tracks[0], 9000);
    let recording = Box::new(PreparedSectionPlaybackSource::new(
        section_id,
        8.0,
        true,
        vec![(tracks[0], std::mem::take(&mut *recording_clip.source))],
    ));
    let recording_pointer = &*recording as *const PreparedSectionPlaybackSource;
    drop(recording_clip);
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id: tracks[0],
            prepared: Some(recording),
            count_in_bars: 4,
            replace_existing: false,
        })
        .unwrap();
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [0.0; 1], 1));
    assert!(engine.capture_lifecycle.is_active());
    assert!(engine.pending_section_record.is_some());
    assert!(engine
        .tracks()
        .iter()
        .all(|track| track.queued_clip.is_some()));
    assert!(expected_clips
        .iter()
        .all(|owner| owner.audio.strong_count() == 1));
    while events.pop().is_ok() {}
    let timing_history = if delayed_minimum_credits {
        assert_eq!(engine.mix_latency(), 521);
        assert_eq!(engine.earliest_source_latency(), 0);
        assert_eq!(
            engine.section_capture_timing.len(),
            3,
            "three public launches retain real early/full timing history"
        );
        let offsets = engine.capture_offsets();
        assert!(!offsets.is_empty());
        Some((
            engine.section_capture_timing.len(),
            engine.section_capture_timing.capacity(),
            Arc::downgrade(&offsets),
        ))
    } else {
        assert!(engine.scheduled_presentation.is_empty());
        None
    };
    let ring_capacity = fill_ui_ring(&mut engine);
    let presentation_capacity = engine.scheduled_presentation.capacity();
    let retirement_capacity = engine.pending_retirements.capacity();
    let mut displaced_audio = Vec::new();
    let mut admitted_resident = resident_owner;
    if delayed_minimum_credits {
        for generation in 0..retirement_capacity - 2 {
            displaced_audio.push(admitted_resident);
            let incoming = stereo_ramp(512, generation as f32 / 128.0, 0.0, 8);
            admitted_resident = Arc::downgrade(&incoming);
            commands.push(EngineCommand::ReplaceClipBuffer {
                track_id: tracks[0], clip_id: arrange_id, audio: incoming,
            }).unwrap();
        }
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut [], 1))
            ),
            (0, 0)
        );
        assert_eq!(engine.pending_retirements.len(), retirement_capacity - 2);
        assert_eq!(
            engine.pending_retirements.capacity() - engine.pending_retirements.len(),
            2
        );
        assert_eq!(
            engine.section_capture_timing.len(),
            timing_history.as_ref().unwrap().0
        );
        assert_eq!(Arc::as_ptr(&engine.tracks()[0].playback_source.clips[0].audio), admitted_resident.as_ptr());
    }
    let retained_owners = presentation_capacity
        - engine.scheduled_presentation.len()
        - presentation_queue::COMMAND_PRESENTATION_RESERVE;
    // Existing owner-bearing acknowledgements cannot be discarded to make cleanup room.
    for index in 0..retained_owners {
        let (prepared, owner) = unique_clip_request(tracks[0], 10000 + index as u64);
        expected_clips.push(owner);
        engine.present_event(EngineEvent::ClipRequestRetired(prepared));
    }
    assert_eq!(
        engine.scheduled_presentation.len(),
        presentation_capacity - presentation_queue::COMMAND_PRESENTATION_RESERVE
    );
    let capture_boundary = engine.heard_capture_position();
    let arrange_cursor = engine.transport().position();
    commands.push(EngineCommand::UnloadAudio).unwrap();
    for blocked_callback in 0..2 {
        let mut output = [0.0; 1];
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut output, 1))
            ),
            (0, 0)
        );
        assert_eq!(output, [0.0]);
        assert_eq!(engine.tracks()[0].playback_source.clips.len(), 1);
        assert_eq!(Arc::as_ptr(&engine.tracks()[0].playback_source.clips[0].audio), admitted_resident.as_ptr());
        assert!(!engine.transport().is_playing());
        assert_eq!(engine.transport().position(), arrange_cursor);
        assert_eq!(engine.transport().audio_length(), None);
        assert!(!engine.capture_lifecycle.is_active());
        assert!(engine.pending_section_record.is_none());
        assert!(
            engine.pending_source_cleanup,
            "queued source cleanup must remain resumable"
        );
        assert!(engine
            .tracks()
            .iter()
            .all(|track| track.queued_clip.is_some()));
        if blocked_callback == 0 {
            // Probe the existing cleanup continuation without also invoking presentation-overflow policy.
            commands.push(EngineCommand::SetBpm(137.0)).unwrap();
        }
        assert!(
            matches!(engine.cmd_rx.peek(), Ok(EngineCommand::SetBpm(value)) if *value == 137.0)
        );
        if let Some((history_length, history_capacity, _)) = &timing_history {
            assert_eq!(engine.pending_retirements.len(), retirement_capacity);
            assert_eq!(
                engine.section_capture_timing.len(),
                history_length - 2,
                "two available credits retire two histories and preserve the third until UI receipt"
            );
            assert_eq!(engine.section_capture_timing.capacity(), *history_capacity);
            assert!(displaced_audio
                .iter()
                .all(|owner| owner.strong_count() == 1));
        }
        assert_eq!(admitted_resident.strong_count(), 1);
        assert_eq!(recording_audio.audio.strong_count(), 1);
        assert!(expected_clips
            .iter()
            .all(|owner| owner.audio.strong_count() == 1));
        assert_eq!(engine.pending_retirements.capacity(), retirement_capacity);
        assert_eq!(
            engine.scheduled_presentation.capacity(),
            presentation_capacity
        );
    }
    drain_full_ui_ring(&mut events, ring_capacity);
    let mut delivered = Vec::new();
    let delivery_bound = expected_clips.len().div_ceil(ring_capacity) + 4;
    for _ in 0..delivery_bound {
        let mut output = [0.0; 1];
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut output, 1))
            ),
            (0, 0)
        );
        assert_eq!(output, [0.0]);
        while let Ok(event) = events.pop() {
            delivered.push(event);
        }
        if engine.cmd_rx.peek().is_err()
            && !engine.pending_source_cleanup
            && engine.scheduled_presentation.is_empty()
            && engine.pending_retirements.is_empty()
        {
            break;
        }
    }
    assert!(!engine.pending_source_cleanup);
    assert!(engine.cmd_rx.peek().is_err());
    assert!(engine
        .tracks()
        .iter()
        .all(|track| track.queued_clip.is_none() && track.active_clip.is_none()));
    assert_eq!(engine.transport().bpm(), 137.0);
    assert!(!engine.transport().is_playing());
    assert_eq!(engine.transport().position(), arrange_cursor);
    assert_eq!(engine.pending_retirements.capacity(), retirement_capacity);
    assert_eq!(
        engine.scheduled_presentation.capacity(),
        presentation_capacity
    );
    if let Some((history_length, history_capacity, offsets)) = &timing_history {
        assert!(engine.section_capture_timing.is_empty());
        assert_eq!(engine.section_capture_timing.capacity(), *history_capacity);
        assert_eq!(delivered.iter().filter(|event| matches!(event, EngineEvent::CaptureTimingRetired(owner) if std::ptr::eq(Arc::as_ptr(owner), offsets.as_ptr()))).count(), *history_length);
        for pointer in &delayed_sections {
            assert_eq!(
                delivered
                    .iter()
                    .filter(|event| match event {
                        EngineEvent::SectionQueueCancelled { retired } =>
                            std::ptr::eq(&**retired, *pointer),
                        _ => false,
                    })
                    .count(),
                1,
                "all three public delayed Section acknowledgements return exactly once"
            );
        }
        for owner in &displaced_audio {
            assert_eq!(delivered.iter().filter(|event| matches!(event, EngineEvent::RetiredAudioBuffer(audio) if std::ptr::eq(Arc::as_ptr(audio), owner.as_ptr()))).count(), 1);
        }
    }
    for owner in &expected_clips {
        let matching: Vec<_> = delivered
            .iter()
            .filter_map(|event| match event {
                EngineEvent::ClipRequestRetired(prepared)
                    if prepared.request_id == owner.request =>
                {
                    Some(prepared)
                }
                _ => None,
            })
            .collect();
        assert_eq!(matching.len(), 1, "request {} returned once", owner.request);
        assert!(std::ptr::eq(&**matching[0], owner.prepared));
        assert_eq!(owner.audio.strong_count(), 1);
    }
    let stopped_recording: Vec<_> = delivered
        .iter()
        .filter_map(|event| match event {
            EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent {
                section_id: id,
                started: false,
                retired: Some(prepared),
                ..
            }) if *id == section_id => Some(prepared),
            _ => None,
        })
        .collect();
    assert_eq!(stopped_recording.len(), 1);
    assert!(std::ptr::eq(&**stopped_recording[0], recording_pointer));
    assert!(!delivered.iter().any(|event| matches!(event, EngineEvent::RetiredAudioBuffer(audio) if Arc::as_ptr(audio) == admitted_resident.as_ptr())), "Unload preserves Arrange media");
    assert_eq!(
        delivered
            .iter()
            .filter(|event| matches!(event, EngineEvent::PlaybackStopped))
            .count(),
        1
    );
    let capture_stops: Vec<_> = delivered
        .iter()
        .filter_map(|event| match event {
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => Some(*effective_at_samples),
            _ => None,
        })
        .collect();
    assert_eq!(capture_stops, [capture_boundary]);
    assert!(!delivered.iter().any(|event| matches!(
        event,
        EngineEvent::ClipTransitioned { .. }
            | EngineEvent::SectionTransitioned(_)
            | EngineEvent::SectionRecordStarted(_)
            | EngineEvent::PerformanceCaptureStarted(_)
    )));
    drop(stopped_recording);
    drop(delivered);
    assert_eq!(admitted_resident.strong_count(), 1, "Arrange media remains resident after Unload");
    assert!(displaced_audio
        .iter()
        .all(|owner| owner.strong_count() == 0));
    if let Some((_, _, offsets)) = timing_history {
        assert_eq!(
            offsets.strong_count(),
            1,
            "only the current routing plan retains its capture offsets"
        );
    }
    assert_eq!(recording_audio.audio.strong_count(), 0);
    assert!(expected_clips
        .iter()
        .all(|owner| owner.audio.strong_count() == 0));
}


#[test]
fn routed_unload_needs_two_free_credits_without_retiring_the_arrange_buffer() {
    for free_credits in [0, 1, 2, 3] {
        let track = TrackId::new();
        let clip_id = ClipId::new();
        let (mut engine, mut commands, mut events) = graph_test_support::graph_engine(&[
            graph_test_support::routing_channel(track),
            graph_test_support::routing_channel(TrackId::MASTER),
        ]);
        let make_audio = || Arc::new(DecodedAudio {
            channels: vec![vec![0.25; 256]], sample_rate: 1000,
        });
        let original = make_audio();
        let mut owners = vec![Arc::downgrade(&original)];
        let clip = EngineClip {
            id: clip_id, audio: original, position: 0, source_offset: 0,
            start_marker: 0, duration: 256, loop_enabled: false,
            loop_start: 0, loop_end: 0, linear_gain: 1.0,
            fades: Default::default(), playback_direction: Default::default(),
            warp_markers: Default::default(),
        };
        for command in [
            EngineCommand::SetSampleRate(1000),
            EngineCommand::AddTrack(Box::new(EngineTrack::with_playback_source(
                track, crate::playback_source::PreparedPlaybackSource::new(
                    vec![clip], vec![], vec![],
                ),
            ))),
            EngineCommand::SetArrangementLoopRegion { start: 2, end: 128 },
            EngineCommand::SetArrangementLoop(true),
            EngineCommand::Seek(17), EngineCommand::Play,
        ] { commands.push(command).unwrap(); }
        let mut output = [0.0; 8];
        engine.process_block(AudioProcessBlock::new(&mut output, 1));
        assert_eq!(output, [0.25; 8]);
        assert_eq!(engine.transport().position(), 25);
        while events.pop().is_ok() {}
        let ring_capacity = fill_ui_ring(&mut engine);
        let capacity = engine.pending_retirements.capacity();
        assert_eq!(capacity, 32);
        let replacements = capacity - free_credits;
        for _ in 0..replacements {
            let incoming = make_audio();
            owners.push(Arc::downgrade(&incoming));
            commands.push(EngineCommand::ReplaceClipBuffer {
                track_id: track, clip_id, audio: incoming,
            }).unwrap();
        }
        assert_eq!(crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut [], 1));
        }), (0, 0));
        assert_eq!(engine.pending_retirements.len(), replacements);
        assert_eq!(engine.pending_retirements.capacity(), capacity);
        assert_eq!(engine.transport().position(), 25);
        commands.push(EngineCommand::UnloadAudio).unwrap();
        commands.push(EngineCommand::SetBpm(137.0)).unwrap();
        assert_eq!(crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut output, 1));
        }), (0, 0));
        let position = if free_credits < 2 {
            assert_eq!(output, [0.25; 8]);
            assert!(matches!(engine.cmd_rx.peek(), Ok(EngineCommand::UnloadAudio)));
            assert!(engine.transport().is_playing());
            assert_eq!(engine.transport().audio_length(), Some(256));
            assert_ne!(engine.transport().bpm(), 137.0);
            33
        } else {
            assert_eq!(output, [0.0; 8]);
            assert!(engine.cmd_rx.peek().is_err());
            assert!(!engine.transport().is_playing());
            assert_eq!(engine.transport().audio_length(), None);
            assert_eq!(engine.transport().bpm(), 137.0);
            25
        };
        assert_eq!(engine.transport().position(), position);
        assert_eq!(engine.sample_rate(), 1000);
        assert_eq!(engine.transport().active_loop_region(), Some((2, 128)));
        assert_eq!(engine.pending_retirements.len(), replacements);
        assert_eq!(engine.pending_retirements.capacity(), capacity);
        assert!(owners.iter().all(|owner| owner.strong_count() == 1));
        assert_eq!(engine.tracks()[0].playback_source.clips.len(), 1);
        assert_eq!(Arc::as_ptr(&engine.tracks()[0].playback_source.clips[0].audio), owners[replacements].as_ptr());
        drain_full_ui_ring(&mut events, ring_capacity);
        let mut delivered = Vec::new();
        for _ in 0..3 {
            assert_eq!(crate::retirement::tests::allocations(|| {
                engine.process_block(AudioProcessBlock::new(&mut output, 1));
            }), (0, 0));
            assert_eq!(output, [0.0; 8]);
            while let Ok(event) = events.pop() { delivered.push(event); }
        }
        assert_eq!(engine.transport().position(), position);
        assert!(!engine.transport().is_playing());
        assert_eq!(engine.transport().bpm(), 137.0);
        assert!(engine.cmd_rx.peek().is_err());
        assert!(engine.pending_retirements.is_empty());
        assert_eq!(engine.transport().active_loop_region(), Some((2, 128)));
        for owner in &owners[..replacements] {
            assert_eq!(delivered.iter().filter(|event| matches!(event,
                EngineEvent::RetiredAudioBuffer(audio) if Arc::as_ptr(audio) == owner.as_ptr()
            )).count(), 1);
        }
        assert!(!delivered.iter().any(|event| matches!(event,
            EngineEvent::RetiredAudioBuffer(audio) if Arc::as_ptr(audio) == owners[replacements].as_ptr()
        )));
        assert_eq!(delivered.iter().filter(|event| matches!(event, EngineEvent::PlaybackStopped)).count(), 1);
        drop(delivered);
        assert!(owners[..replacements].iter().all(|owner| owner.strong_count() == 0));
        assert_eq!(owners[replacements].strong_count(), 1);
        drop(engine);
        assert_eq!(owners[replacements].strong_count(), 0);
    }
}

#[test]
fn routed_audio_retired_resident_and_unconsumed_owners_teardown_in_either_order() {
    for engine_first in [false, true] {
        let track = TrackId::new();
        let clip_id = ClipId::new();
        let (mut engine, mut commands, events) = graph_test_support::graph_engine(&[
            graph_test_support::routing_channel(track),
            graph_test_support::routing_channel(TrackId::MASTER),
        ]);
        let retired = stereo_ramp(128, 0.125, -0.25, 1000);
        let resident = stereo_ramp(128, 0.25, -0.5, 1000);
        let queued = stereo_ramp(128, 0.5, -0.75, 1000);
        let owners = [Arc::downgrade(&retired), Arc::downgrade(&resident), Arc::downgrade(&queued)];
        let clip = EngineClip {
            id: clip_id, audio: retired, position: 0, source_offset: 0,
            start_marker: 0, duration: 128, loop_enabled: false,
            loop_start: 0, loop_end: 0, linear_gain: 1.0,
            fades: Default::default(), playback_direction: Default::default(),
            warp_markers: Default::default(),
        };
        commands.push(EngineCommand::AddTrack(Box::new(EngineTrack::with_playback_source(
            track, crate::playback_source::PreparedPlaybackSource::new(vec![clip], vec![], vec![]),
        )))).unwrap();
        commands.push(EngineCommand::ReplaceClipBuffer { track_id: track, clip_id, audio: resident }).unwrap();
        assert_eq!(crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut [0.0; 2], 2));
        }), (0, 0));
        commands.push(EngineCommand::ReplaceClipBuffer { track_id: track, clip_id, audio: queued }).unwrap();
        assert!(owners.iter().all(|owner| owner.strong_count() == 1));
        if engine_first {
            drop(engine);
            assert_eq!(owners[0].strong_count(), 1, "UI queue retains retired audio");
            assert_eq!(owners[1].strong_count(), 0, "resident audio ends with engine");
            assert_eq!(owners[2].strong_count(), 1, "command queue retains unconsumed incoming audio");
            drop(events);
            drop(commands);
        } else {
            drop(events);
            drop(commands);
            assert!(owners.iter().all(|owner| owner.strong_count() == 1));
            drop(engine);
        }
        assert!(owners.iter().all(|owner| owner.strong_count() == 0));
    }
}
