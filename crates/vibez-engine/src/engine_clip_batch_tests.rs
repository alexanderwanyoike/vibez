//! Clip packets retain every source owner across bounded event backpressure.

use super::*;
use crate::playback_source::{EngineClip, PreparedClipPlayback, PreparedPlaybackSource};
use vibez_core::{id::ClipId, perform::MusicalBoundary};

fn clip(track_id: TrackId, request_id: u64) -> Box<PreparedClipPlayback> {
    let id = ClipId::new();
    Box::new(PreparedClipPlayback {
        track_id,
        clip_id: Some(id),
        request_id,
        length_samples: 100,
        looping: true,
        source: Box::new(PreparedPlaybackSource::new(
            vec![EngineClip {
                id,
                audio: Arc::new(DecodedAudio {
                    channels: vec![vec![0.1; 100]],
                    sample_rate: 8,
                }),
                position: 0,
                source_offset: 0,
                start_marker: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                linear_gain: 1.0,
                fades: Default::default(),
                playback_direction: Default::default(),
                warp_markers: Default::default(),
            }],
            vec![],
            vec![],
        )),
    })
}

#[allow(clippy::vec_box)]
fn queue(
    engine: &mut AudioEngine,
    commands: &mut Producer<EngineCommand>,
    clips: Vec<Box<PreparedClipPlayback>>,
    quantization: MusicalBoundary,
) {
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    commands
        .push(EngineCommand::QueueClips {
            clips,
            quantization,
        })
        .unwrap();
}

#[test]
fn oversized_missing_target_packet_never_grows_retirement_storage_in_the_callback() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let target = TrackId::new();
    let clips = (0..2400)
        .map(|request| PreparedClipPlayback::stop(target, request))
        .collect();
    queue(
        &mut engine,
        &mut commands,
        clips,
        MusicalBoundary::Immediate,
    );
    let capacity = engine.pending_retirements.capacity();
    let mut retired = 0;
    let mut packets = 0;
    for _ in 0..12 {
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
        assert_eq!(engine.pending_retirements.capacity(), capacity);
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::ClipRequestRetired(owner) => {
                    retired += 1;
                    drop(owner);
                }
                EngineEvent::ClipBatchRetired(packet) => {
                    assert!(packet.is_empty());
                    packets += 1;
                }
                _ => {}
            }
        }
    }
    assert_eq!((retired, packets), (2400, 1));
    assert!(engine.pending_clip_batch.is_none());
    assert!(!engine.presentation_fault);
}

#[test]
fn repeated_target_packet_keeps_reverse_pop_order_and_retires_all_displaced_sources() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let target = TrackId::new();
    engine.tracks.push(EngineTrack::new(target));
    let clips = (0..2400).map(|request| clip(target, request)).collect();
    queue(
        &mut engine,
        &mut commands,
        clips,
        MusicalBoundary::Immediate,
    );
    let mut queued = vec![];
    let mut owners = 0;
    for _ in 0..20 {
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::ClipQueued { request_id, .. } => queued.push(request_id),
                EngineEvent::ClipRequestRetired(owner)
                | EngineEvent::ClipTransitioned {
                    retired: Some(owner),
                    ..
                } => {
                    owners += 1;
                    drop(owner);
                }
                EngineEvent::ClipBatchRetired(packet) => assert!(packet.is_empty()),
                _ => {}
            }
        }
    }
    assert_eq!(queued, (0..2400).rev().collect::<Vec<_>>());
    assert_eq!(owners, 2400);
    assert_eq!(engine.tracks[0].active_clip.unwrap().request_id, 0);
    assert!(engine.pending_clip_batch.is_none());
    assert!(!engine.presentation_fault);
}

#[test]
fn partial_packet_releases_tracks_atomically_at_one_source_timestamp_and_keeps_other_clips_running()
{
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let ids = [TrackId::new(), TrackId::new(), TrackId::new()];
    engine.sample_rate = 8;
    for id in ids {
        engine.tracks.push(EngineTrack::new(id));
    }
    let channels: Vec<_> = ids
        .into_iter()
        .chain([TrackId::MASTER])
        .map(|id| vibez_core::routing::RoutingChannel {
            id,
            is_bus: id.is_master(),
            sends: vec![],
            effects: vec![],
        })
        .collect();
    engine.routing = Some(crate::routing::PreparedRouting::prepare(&channels, 8).unwrap());
    engine.queue_clips(vec![clip(ids[2], 7)], MusicalBoundary::Immediate);
    while events.pop().is_ok() {}
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY - 2 {
        engine.present_event_at(
            EngineEvent::PlaybackStarted,
            engine.output_position.saturating_add(5000_u64),
        );
    }
    queue(
        &mut engine,
        &mut commands,
        vec![clip(ids[0], 1), clip(ids[1], 2)],
        MusicalBoundary::Immediate,
    );
    // Direct delivery exercises packet progress without the command guard's reserve.
    let EngineCommand::QueueClips {
        clips,
        quantization,
    } = engine.cmd_rx.pop().unwrap()
    else {
        panic!("packet");
    };
    engine.queue_clips(clips, quantization);
    assert!(engine.tracks[..2]
        .iter()
        .all(|track| track.active_clip.is_none()));
    assert_eq!(engine.tracks[2].active_clip.unwrap().request_id, 7);
    let mut output = [0.0; 8];
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 1)),
        (0, 0)
    );
    assert!(output.iter().all(|sample| (*sample - 0.1).abs() < 1e-6));
    engine.performance_position = 17;
    engine.output_position = 6000;
    while events.pop().is_ok() {}
    engine.flush_presentation();
    while events.pop().is_ok() {}
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.continue_clip_batch(17)),
        (0, 0)
    );
    assert!(engine.tracks[..2]
        .iter()
        .all(|track| track.active_clip.is_some()));
    let mut transitions = Vec::new();
    for _ in 0..4 {
        while let Ok(event) = events.pop() {
            if let EngineEvent::ClipTransitioned {
                track_id,
                effective_at_samples,
                ..
            } = event
            {
                transitions.push((track_id, effective_at_samples));
            }
        }
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.flush_presentation()),
            (0, 0)
        );
    }
    assert_eq!(transitions, vec![(ids[0], 17), (ids[1], 17)]);
    assert_eq!(engine.tracks[2].active_clip.unwrap().request_id, 7);
    assert!(!engine.presentation_fault);
}

#[test]
fn larger_than_event_capacity_row_swaps_all_sources_before_resumable_publication() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let count = presentation_queue::PRESENTATION_EVENT_CAPACITY + 10;
    let ids: Vec<_> = (0..count).map(|_| TrackId::new()).collect();
    engine
        .tracks
        .extend(ids.iter().map(|&id| EngineTrack::new(id)));
    let clips = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| clip(id, i as u64))
        .collect();
    queue(
        &mut engine,
        &mut commands,
        clips,
        MusicalBoundary::Immediate,
    );
    let mut retired = 0;
    let mut swapped = false;
    let mut timestamps = vec![];
    for step in 0..24 {
        engine.performance_position = step;
        engine.output_position = step;
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
        let active = engine
            .tracks
            .iter()
            .filter(|track| track.active_clip.is_some())
            .count();
        assert!(
            active == 0 || active == count,
            "row partially swapped {active}/{count}"
        );
        swapped |= active == count;
        while let Ok(event) = events.pop() {
            if let EngineEvent::ClipTransitioned {
                effective_at_samples,
                retired: Some(owner),
                ..
            } = event
            {
                retired += 1;
                timestamps.push(effective_at_samples);
                drop(owner);
            }
        }
    }
    assert!(swapped);
    assert_eq!(retired, count);
    assert!(timestamps.iter().all(|at| *at == timestamps[0]));
    assert!(engine.pending_clip_batch.is_none());
    assert!(!engine.presentation_fault);
}

#[test]
fn newer_quantized_packet_supersedes_only_its_targets_and_keeps_unrelated_deadline() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let ids = [TrackId::new(), TrackId::new()];
    engine.sample_rate = 8;
    engine.tracks.extend(ids.map(EngineTrack::new));
    engine.begin_clip_performance();
    engine.performance_position = 1;
    engine.queue_clips(
        vec![clip(ids[0], 1), clip(ids[1], 2)],
        MusicalBoundary::OneBar,
    );
    assert_eq!(
        engine.tracks[0].queued_clip.as_ref().unwrap().effective_at,
        16
    );
    engine.performance_position = 2;
    engine.queue_clips(vec![clip(ids[1], 3)], MusicalBoundary::OneBeat);
    assert_eq!(
        engine.tracks[0].queued_clip.as_ref().unwrap().effective_at,
        16
    );
    assert_eq!(
        engine.tracks[1].queued_clip.as_ref().unwrap().effective_at,
        4
    );
    engine.performance_position = 4;
    engine.apply_clip_boundaries(4);
    assert!(engine.tracks[0].active_clip.is_none());
    assert_eq!(engine.tracks[1].active_clip.unwrap().request_id, 3);
    engine.performance_position = 16;
    engine.apply_clip_boundaries(16);
    assert_eq!(engine.tracks[0].active_clip.unwrap().request_id, 1);
    while events.pop().is_ok() {}
    assert!(engine.pending_clip_batch.is_none());
}

#[test]
fn stalled_quantized_packet_moves_only_the_missed_boundary_to_the_next_achievable_grid() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let id = TrackId::new();
    engine.sample_rate = 8;
    engine.tracks.push(EngineTrack::new(id));
    engine.begin_clip_performance();
    engine.performance_position = 1;
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY - 1 {
        engine.present_event_at(
            EngineEvent::PlaybackStarted,
            engine.output_position.saturating_add(1000_u64),
        );
    }
    engine.queue_clips(vec![clip(id, 8)], MusicalBoundary::OneBar);
    assert!(engine.tracks[0].queued_clip.is_none());
    while events.pop().is_ok() {}
    engine.performance_position = 17;
    engine.continue_clip_batch(17);
    assert_eq!(
        engine.tracks[0].queued_clip.as_ref().unwrap().effective_at,
        32
    );
    engine.performance_position = 32;
    engine.apply_clip_boundaries(32);
    assert_eq!(engine.tracks[0].active_clip.unwrap().request_id, 8);
}

#[test]
fn late_publication_keeps_the_original_source_timestamp_and_physical_release() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let id = TrackId::new();
    engine.tracks.push(EngineTrack::new(id));
    engine.sample_rate = 8;
    engine.begin_clip_performance();
    engine.performance_position = 1;
    engine.queue_clips(vec![clip(id, 1)], MusicalBoundary::OneBar);
    while events.pop().is_ok() {}
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY - 3 {
        engine.present_event_at(
            EngineEvent::PlaybackStarted,
            engine.output_position.saturating_add(5000_u64),
        );
    }
    engine.queue_clips(vec![clip(id, 2)], MusicalBoundary::Immediate);
    assert_eq!(engine.tracks[0].active_clip.unwrap().request_id, 2);
    assert!(
        engine.clip_batch_blocks_commands(),
        "publication must retain the source owner"
    );
    while events.pop().is_ok() {}
    engine.output_position = 1000;
    engine.flush_presentation();
    while events.pop().is_ok() {}
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.continue_clip_batch(1000)),
        (0, 0)
    );
    assert!(!engine
        .scheduled_presentation
        .iter()
        .any(|pending| matches!(pending.event, EngineEvent::ClipTransitioned { .. })));
    let event = events.pop().unwrap();
    assert!(matches!(
        event,
        EngineEvent::ClipTransitioned {
            request_id: 2,
            effective_at_samples: 1,
            retired: Some(_),
            ..
        }
    ));
    assert!(engine.pending_clip_batch.is_none());
}

#[test]
fn in_segment_release_retains_the_actual_output_clock_prefix() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let id = TrackId::new();
    engine.sample_rate = 8;
    engine.tracks.push(EngineTrack::new(id));
    let channels = [
        vibez_core::routing::RoutingChannel {
            id,
            is_bus: false,
            sends: vec![],
            effects: vec![],
        },
        vibez_core::routing::RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            sends: vec![],
            effects: vec![],
        },
    ];
    engine.routing = Some(crate::routing::PreparedRouting::prepare(&channels, 8).unwrap());
    engine.queue_clips(vec![clip(id, 1)], MusicalBoundary::Immediate);
    while events.pop().is_ok() {}
    engine.performance_position = 1;
    engine.output_position = 100;
    let mut next = clip(id, 2);
    next.source.clips[0].linear_gain = 2.0;
    engine.queue_clips(vec![next], MusicalBoundary::OneBeat);
    while events.pop().is_ok() {}
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY {
        engine.present_event_at(
            EngineEvent::PlaybackStarted,
            engine.output_position.saturating_add(5000_u64),
        );
    }
    let mut output = [0.0; 8];
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 1)),
        (0, 0)
    );
    assert!(output[..3]
        .iter()
        .all(|sample| (*sample - 0.1).abs() < 1e-6));
    assert!(output[3..]
        .iter()
        .all(|sample| (*sample - 0.2).abs() < 1e-6));
    assert!(matches!(
        engine.pending_clip_batch.as_ref().unwrap().phase,
        BatchPhase::Publishing {
            source: 4,
            physical: 103
        }
    ));
    assert_eq!(engine.output_position, 108);
}

#[test]
fn publication_after_the_original_audible_due_time_does_not_add_device_latency_again() {
    let (mut engine, mut events, id) = delayed_engine();
    engine.sample_rate = 8;
    engine.begin_clip_performance();
    engine.performance_position = 1;
    engine.queue_clips(vec![clip(id, 1)], MusicalBoundary::OneBar);
    while events.pop().is_ok() {}
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY - 3 {
        engine.present_event(EngineEvent::PlaybackStarted, 5000);
    }
    engine.queue_clips(vec![clip(id, 2)], MusicalBoundary::Immediate);
    assert_eq!(engine.tracks[0].active_clip.unwrap().request_id, 2);
    assert!(
        engine.clip_batch_blocks_commands(),
        "publication must retain the source owner"
    );
    while events.pop().is_ok() {}
    engine.output_position = 1000;
    engine.flush_presentation();
    while events.pop().is_ok() {}
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.continue_clip_batch(1000)),
        (0, 0)
    );
    assert!(!engine
        .scheduled_presentation
        .iter()
        .any(|pending| matches!(pending.event, EngineEvent::ClipTransitioned { .. })));
    let event = events.pop().unwrap();
    assert!(matches!(
        event,
        EngineEvent::ClipTransitioned {
            request_id: 2,
            effective_at_samples: 1,
            retired: Some(_),
            ..
        }
    ));
    assert!(engine.pending_clip_batch.is_none());
}

#[test]
fn publishing_does_not_count_ui_slots_before_a_real_heard_deadline() {
    let (mut engine, mut events, id) = delayed_engine();
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY {
        engine.present_event(EngineEvent::PlaybackStarted, 137);
    }
    events.pop().unwrap();
    let prepared = clip(id, 1);
    let original = &*prepared as *const PreparedClipPlayback;
    engine.pending_clip_batch = Some(PendingClipBatch {
        clips: vec![prepared],
        quantization: MusicalBoundary::Immediate,
        boundary: 1,
        starting: false,
        phase: BatchPhase::Publishing {
            source: 1,
            physical: 20,
        },
    });
    engine.rendered_callback_frames = 200;
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.continue_clip_batch(200)),
        (0, 0)
    );
    assert!(!engine.presentation_fault);
    assert_eq!(engine.pending_clip_batch.as_ref().unwrap().clips.len(), 1);
    engine.rendered_callback_frames = 0;
    engine.output_position = 157;
    let mut retired = None;
    for _ in 0..4 {
        while let Ok(event) = events.pop() {
            if let EngineEvent::ClipTransitioned { retired: owner, .. } = event {
                retired = owner;
            }
        }
        engine.flush_presentation();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.continue_clip_batch(200)),
            (0, 0)
        );
    }
    assert_eq!(&*retired.unwrap() as *const PreparedClipPlayback, original);
    assert!(!engine.presentation_fault);
}

#[test]
fn resync_does_not_announce_a_clip_before_its_reported_delay_is_heard() {
    let (mut engine, mut events, id) = delayed_engine();
    engine.sample_rate = 8;
    engine.queue_clips(vec![clip(id, 1)], MusicalBoundary::Immediate);
    let mut output = [0.0; 16];
    engine.process(&mut output, 2);
    assert!(output.iter().all(|sample| *sample == 0.0));
    while events.pop().is_ok() {}
    let index = engine
        .scheduled_presentation
        .iter()
        .position(|pending| matches!(pending.event, EngineEvent::ClipTransitioned { .. }))
        .unwrap();
    let owner = match engine.scheduled_presentation.remove(index).event {
        EngineEvent::ClipTransitioned {
            retired: Some(owner),
            ..
        } => owner,
        _ => unreachable!(),
    };
    engine.pending_clip_batch = Some(PendingClipBatch {
        clips: vec![owner],
        quantization: MusicalBoundary::Immediate,
        boundary: 0,
        starting: true,
        phase: BatchPhase::Publishing {
            source: 0,
            physical: 0,
        },
    });
    engine.clip_resync_track = Some(0);
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.resync_clip_events()),
        (0, 0)
    );
    assert!(!std::iter::from_fn(|| events.pop().ok()).any(
        |event| matches!(event, EngineEvent::ClipStateResynced(state) if state.playing.is_some())
    ));
    engine.continue_clip_batch(8);
    engine.resync_clip_events();
    assert!(!std::iter::from_fn(|| events.pop().ok()).any(
        |event| matches!(event, EngineEvent::ClipStateResynced(state) if state.playing.is_some())
    ));
    engine.output_position = 137;
    engine.flush_presentation();
    engine.resync_clip_events();
    let received: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    let transition = received
        .iter()
        .position(|event| matches!(event, EngineEvent::ClipTransitioned { .. }));
    let snapshot = received.iter().position(
        |event| matches!(event, EngineEvent::ClipStateResynced(state) if state.playing.is_some()),
    );
    assert!(transition.is_some() && snapshot.is_some() && transition < snapshot);
    let state = match &received[snapshot.unwrap()] {
        EngineEvent::ClipStateResynced(state) => state,
        _ => unreachable!(),
    };
    assert_eq!(state.playing.unwrap().position, 0);
    assert_eq!(state.effective_at_samples, 0);
}

fn delayed_engine() -> (AudioEngine, Consumer<EngineEvent>, TrackId) {
    use vibez_core::routing::*;
    let (mut engine, _, events) = AudioEngine::new();
    let id = TrackId::new();
    let effect = vibez_core::id::EffectId::new();
    let mut track = EngineTrack::new(id);
    track.effects.push(EffectSlot {
        id: effect,
        effect: Box::new(crate::test_support::DelayProbe::new(137)),
        bypass: false,
    });
    engine.tracks.push(track);
    let channels = [
        RoutingChannel {
            id,
            is_bus: false,
            sends: vec![],
            effects: vec![RoutingEffect {
                id: effect,
                inputs: vec![],
                assignments: vec![],
                inactive_inputs: vec![],
            }],
        },
        RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            sends: vec![],
            effects: vec![],
        },
    ];
    let reports = [(
        RoutingNode {
            channel: id,
            stage: NodeStage::Effect(effect),
        },
        137,
    )];
    engine.routing = Some(
        crate::routing::PreparedRouting::prepare_compensated(&channels, 8, &reports, &[], 1)
            .unwrap(),
    );
    engine.sample_rate = 8;
    (engine, events, id)
}

#[test]
fn inline_capture_stop_does_not_consume_packet_owner_admission_capacity() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let id = TrackId::new();
    engine.tracks.push(EngineTrack::new(id));
    engine.begin_clip_performance();
    engine.performance_position = 1;
    engine.queue_clips(vec![clip(id, 92)], MusicalBoundary::OneBar);
    let boundary = engine.pending_clip_batch.as_ref().unwrap().boundary;
    let identity = &*engine.tracks[0].queued_clip.as_ref().unwrap().prepared as *const _;
    while events.pop().is_ok() {}
    engine.performance_position = boundary;
    engine.output_position = boundary;
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    for request in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY {
        let owner = PreparedClipPlayback::stop(TrackId::new(), request as u64);
        engine.present_event(EngineEvent::ClipRequestRetired(owner), 0);
    }
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.continue_clip_batch(boundary)),
        (0, 0)
    );
    assert!(matches!(
        engine.pending_clip_batch.as_ref().unwrap().phase,
        BatchPhase::Publishing { .. }
    ));
    engine.compensation_callback_heard_start = boundary;
    engine.close_capture_for_device_failure();
    events.pop().unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.continue_clip_batch(boundary)),
        (0, 0)
    );
    assert!(!engine.presentation_fault);
    assert_eq!(engine.pending_clip_batch.as_ref().unwrap().clips.len(), 1);
    let mut returned = false;
    for _ in 0..8 {
        while let Ok(event) = events.pop() {
            if let EngineEvent::ClipTransitioned {
                retired: Some(owner),
                ..
            } = event
            {
                assert_eq!(&*owner as *const _, identity);
                returned = true;
            }
        }
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                engine.flush_presentation();
                engine.continue_clip_batch(boundary);
            }),
            (0, 0)
        );
    }
    assert!(returned);
    assert!(engine.pending_clip_batch.is_none());
    assert!(!engine.presentation_fault);
}
