//! Recording starts retain their reliable fact when a nested Clip packet exhausts presentation.

use super::*;
use crate::events::SectionRecordStoppedEvent;
use crate::playback_source::{
    EngineClip, PreparedClipPlayback, PreparedPlaybackSource, PreparedSectionPlaybackSource,
};
use graph_test_support::{graph_engine, routing_channel};
use vibez_core::id::{ClipId, SectionId};
use vibez_core::perform::MusicalBoundary;

fn tail_clip(id: ClipId, value: f32) -> EngineClip {
    EngineClip {
        id,
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![value; 1024]],
            sample_rate: 8,
        }),
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 1024,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        linear_gain: 1.0,
        fades: Default::default(),
        playback_direction: Default::default(),
        warp_markers: Default::default(),
    }
}

fn tail_request(
    track: TrackId,
    request_id: u64,
    value: f32,
) -> (Box<PreparedClipPlayback>, std::sync::Weak<DecodedAudio>) {
    let clip = tail_clip(ClipId::new(), value);
    let owner = Arc::downgrade(&clip.audio);
    (
        Box::new(PreparedClipPlayback {
            track_id: track,
            clip_id: Some(clip.id),
            request_id,
            length_samples: 1024,
            looping: true,
            source: Box::new(PreparedPlaybackSource::new(vec![clip], vec![], vec![])),
        }),
        owner,
    )
}

#[test]
fn nested_packet_publication_preserves_record_started_without_growing_retirements() {
    let tracks: [TrackId; 4] = std::array::from_fn(|_| TrackId::new());
    let section = SectionId::new();
    let removed_id = ClipId::new();
    let mut arrange: Vec<_> = (0..32).map(|_| tail_clip(removed_id, 0.0)).collect();
    let removed_owners: Vec<_> = arrange
        .iter()
        .map(|clip| Arc::downgrade(&clip.audio))
        .collect();
    let resident = tail_clip(ClipId::new(), 0.25);
    let arrange_owner = Arc::downgrade(&resident.audio);
    arrange.push(resident);
    let first_clip = tail_clip(ClipId::new(), 0.25);
    let first_owner = Arc::downgrade(&first_clip.audio);
    let first_section = Box::new(PreparedSectionPlaybackSource::new(
        section,
        1.0,
        false,
        vec![(
            tracks[0],
            PreparedPlaybackSource::new(vec![first_clip], vec![], vec![]),
        )],
    ));
    let mut routes: Vec<_> = tracks.iter().copied().map(routing_channel).collect();
    routes.push(routing_channel(TrackId::MASTER));
    let (mut engine, mut commands, mut events) = graph_engine(&routes);
    for command in [
        EngineCommand::SetSampleRate(8),
        EngineCommand::SetBpm(60.0),
        EngineCommand::Seek(40),
    ] {
        commands.push(command).unwrap();
    }
    commands
        .push(EngineCommand::AddTrack(Box::new(
            EngineTrack::with_playback_source(
                tracks[0],
                PreparedPlaybackSource::new(arrange, vec![], vec![]),
            ),
        )))
        .unwrap();
    for track in &tracks[1..] {
        commands.push(EngineCommand::add_track(*track)).unwrap();
    }
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id: section,
            track_id: tracks[0],
            prepared: Some(first_section),
            count_in_bars: 0,
            replace_existing: false,
        })
        .unwrap();
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    let mut first = [0.0];
    engine.process_block(AudioProcessBlock::new(&mut first, 1));
    assert_eq!(first, [0.25]);
    while events.pop().is_ok() {}
    for position in 0..1024 {
        engine
            .event_tx
            .push(EngineEvent::PlaybackPosition(position))
            .unwrap();
    }
    let mut acknowledgement_owners = Vec::new();
    for _ in 0..2008 {
        let owner = Box::new(PreparedSectionPlaybackSource::new(
            SectionId::new(),
            1.0,
            false,
            vec![],
        ));
        acknowledgement_owners.push(std::ptr::from_ref(owner.as_ref()) as usize);
        engine.present_event(EngineEvent::SectionQueueCancelled { retired: owner });
    }
    for _ in 0..40 {
        engine.present_event_at(EngineEvent::PlaybackStarted, engine.output_position + 5000);
    }
    assert_eq!(engine.scheduled_presentation.len(), 2048);
    assert!(engine.event_tx.is_full());
    let mut ending = [0.0; 7];
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut ending, 1));
        }),
        (0, 0)
    );
    assert_eq!(ending, [0.25; 7]);
    assert!(
        matches!(engine.presentation_state.terminals.overflow_owner(), Some(
        EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent {
            section_id, effective_at_samples: 8, section_position_samples: 8,
            started: true, retired: None, ..
        })
    ) if *section_id == section)
    );
    assert_eq!(engine.pending_retirements.len(), 0);
    assert_eq!(engine.scheduled_presentation.len(), 2009);
    assert_eq!(engine.presentation_state.terminals.capture_stop(), Some(1));
    commands.push(EngineCommand::Play).unwrap();
    let mut resumed = [0.0; 128];
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut resumed, 1));
        }),
        (0, 0)
    );
    assert_eq!(resumed, [0.25; 128]);
    assert_eq!(engine.transport().position(), 168);
    assert!(!engine.presentation_state.is_faulted());
    assert!(engine
        .presentation_state
        .terminals
        .overflow_owner()
        .is_some());

    let (base, base_audio) = tail_request(tracks[0], 10, 0.25);
    let base_pointer = std::ptr::from_ref(base.as_ref()) as usize;
    commands
        .push(EngineCommand::QueueClips {
            clips: vec![base],
            quantization: MusicalBoundary::Immediate,
        })
        .unwrap();
    let mut base_first = [0.0];
    engine.process_block(AudioProcessBlock::new(&mut base_first, 1));
    assert_eq!(base_first, [0.25]);
    assert_eq!(engine.performance_position, 1);
    assert_eq!(engine.output_position_samples(), 137);
    assert_eq!(engine.transport().position(), 168);
    assert!(engine.clip_performance);
    assert!(!engine.presentation_state.is_faulted());
    assert_eq!(engine.output_gate.last_frame(), [0.25, 0.25]);
    assert!(engine.pending_clip_batch.is_none());

    let (recording, recording_audio) = tail_request(tracks[0], 20, 0.5);
    let recording_id = recording.clip_id.unwrap();
    let recording_pointer = std::ptr::from_ref(recording.as_ref()) as usize;
    commands
        .push(EngineCommand::ArmClipRecord {
            prepared: recording,
            count_in_bars: 0,
            free_length: false,
        })
        .unwrap();
    let mut packet = Vec::new();
    let mut packet_pointers = Vec::new();
    let mut packet_audio = Vec::new();
    for (index, track) in tracks[1..].iter().enumerate() {
        let (prepared, owner) = tail_request(*track, 30 + index as u64, 0.125);
        packet_pointers.push(std::ptr::from_ref(prepared.as_ref()) as usize);
        packet_audio.push(owner);
        packet.push(prepared);
    }
    let packet_storage = packet.as_ptr() as usize;
    commands
        .push(EngineCommand::QueueClips {
            clips: packet,
            quantization: MusicalBoundary::OneBar,
        })
        .unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [], 1));
    assert_eq!(engine.next_clip_record_boundary(), Some(32));
    assert!(engine.clip_batch_waiting_boundary());
    assert!(!engine.clip_batch_blocks_commands());
    assert_eq!(engine.pending_clip_batch.as_ref().unwrap().clips.len(), 0);
    assert!(engine.tracks()[0].queued_clip.is_none());
    for track in &engine.tracks()[1..] {
        assert!(
            matches!(track.queued_clip.as_ref(), Some(queued) if queued.pending_batch && queued.effective_at == 32)
        );
    }
    assert_eq!(engine.performance_position, 1);
    assert_eq!(engine.output_position_samples(), 137);
    assert_eq!(engine.transport().position(), 168);
    assert!(engine.cmd_rx.peek().is_err());
    assert!(engine.capture_replay.is_none());
    assert!(engine.pending_clip_insertion.is_none());
    assert!(engine.pending_bus_cleanup.is_none());
    assert!(!engine.pending_source_cleanup);
    assert!(engine.pending_section_record.is_none());
    assert!(engine.active_section_record.is_none());
    assert_eq!(engine.mix_latency(), 0);
    assert!(!engine.presentation_state.is_faulted());
    assert!(engine
        .presentation_state
        .terminals
        .overflow_owner()
        .is_some());
    assert_eq!(engine.pending_retirements.len(), 0);
    assert!(engine.scheduled_presentation.len() < 2044);
    while engine.scheduled_presentation.len() < 2044 {
        let owner = Box::new(PreparedSectionPlaybackSource::new(
            SectionId::new(),
            1.0,
            false,
            vec![],
        ));
        acknowledgement_owners.push(std::ptr::from_ref(owner.as_ref()) as usize);
        engine.present_event(EngineEvent::SectionQueueCancelled { retired: owner });
    }
    assert_eq!(engine.scheduled_presentation.capacity(), 2048);
    engine.tracks_mut_for_retirement()[0]
        .playback_source
        .remove_audio_clips(removed_id);
    assert!(engine.tracks()[0].playback_source.has_removed_audio_clips());
    assert_eq!(engine.pending_retirements.len(), 0);
    assert!(removed_owners.iter().all(|owner| owner.strong_count() == 1));
    let mut crossing = [0.0; 32];
    let heap = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut crossing, 1));
    });
    let ordinary_length = engine.pending_retirements.len();
    let ordinary_capacity = engine.pending_retirements.capacity();
    for (index, sample) in crossing.iter().enumerate() {
        assert_eq!(*sample, 0.25 * (64 - index) as f32 / 64.0, "frame{index}");
    }
    assert_eq!(engine.performance_position, 33);
    assert_eq!(engine.output_position_samples(), 169);
    assert_eq!(engine.transport().position(), 168);
    assert!(engine.presentation_state.is_faulted());
    assert!(!engine.transport().is_playing());
    let held_packet = engine.pending_clip_batch.as_ref().is_some_and(|batch| {
        batch.clips.is_empty() && batch.clips.as_ptr() as usize == packet_storage
    });
    let published_packets = engine
        .scheduled_presentation
        .iter()
        .filter(|pending| {
            matches!(&pending.event, EngineEvent::ClipBatchRetired(packet)
            if packet.is_empty() && packet.as_ptr() as usize == packet_storage)
        })
        .count();
    assert_eq!(usize::from(held_packet) + published_packets, 1);
    assert_eq!(engine.next_clip_record_boundary(), None);
    assert!(engine
        .tracks()
        .iter()
        .all(|track| track.active_clip.is_none()));
    assert!(
        matches!(engine.tracks()[0].queued_clip.as_ref(), Some(queued)
        if std::ptr::from_ref(queued.prepared.as_ref()) as usize == recording_pointer && queued.effective_at == 32)
    );
    assert!(engine.tracks()[1..]
        .iter()
        .all(|track| track.queued_clip.is_none()));
    assert_eq!(
        engine
            .pending_retirements
            .iter()
            .filter(|event| matches!(event, EngineEvent::RetiredAudioClip(_)))
            .count(),
        32
    );
    assert_eq!(base_audio.strong_count(), 1);
    assert_eq!(recording_audio.strong_count(), 1);
    assert!(packet_audio.iter().all(|owner| owner.strong_count() == 1));
    for position in 0..1024 {
        assert!(
            matches!(events.pop(), Ok(EngineEvent::PlaybackPosition(actual)) if actual == position)
        );
    }
    let mut delivered = Vec::new();
    for _ in 0..8 {
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                engine.process_block(AudioProcessBlock::new(&mut [], 1));
            }),
            (0, 0)
        );
        while let Ok(event) = events.pop() {
            delivered.push(event);
        }
    }
    let actual_acknowledgements: Vec<_> = delivered
        .iter()
        .filter_map(|event| match event {
            EngineEvent::SectionQueueCancelled { retired } => {
                Some(std::ptr::from_ref(retired.as_ref()) as usize)
            }
            _ => None,
        })
        .collect();
    assert_eq!(actual_acknowledgements, acknowledgement_owners);
    let returned_clips: Vec<_> = delivered
        .iter()
        .filter_map(|event| match event {
            EngineEvent::ClipRequestRetired(prepared) => {
                Some(std::ptr::from_ref(prepared.as_ref()) as usize)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        returned_clips
            .iter()
            .filter(|pointer| **pointer == base_pointer)
            .count(),
        1
    );
    for pointer in &packet_pointers {
        assert_eq!(
            returned_clips
                .iter()
                .filter(|actual| *actual == pointer)
                .count(),
            1
        );
    }
    assert!(!returned_clips.contains(&recording_pointer));
    assert_eq!(
        delivered
            .iter()
            .filter(
                |event| matches!(event, EngineEvent::ClipBatchRetired(packet)
        if packet.as_ptr() as usize == packet_storage && packet.is_empty())
            )
            .count(),
        1
    );
    assert_eq!(
        delivered
            .iter()
            .filter(
                |event| matches!(event, EngineEvent::ClipRecordStarted { clip_id, at: 32 }
        if *clip_id == recording_id)
            )
            .count(),
        1
    );
    assert_eq!(delivered.iter().filter(|event| matches!(event, EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent {
        section_id, effective_at_samples: 8, section_position_samples: 8, started: true, retired: None, ..
    }) if *section_id == section)).count(), 1);
    let capture_stops: Vec<_> = delivered
        .iter()
        .filter_map(|event| match event {
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => Some(*effective_at_samples),
            _ => None,
        })
        .collect();
    assert_eq!(capture_stops, [168, 1]);
    for owner in &removed_owners {
        assert_eq!(
            delivered
                .iter()
                .filter(|event| matches!(event, EngineEvent::RetiredAudioClip(clip)
            if Arc::as_ptr(&clip.audio) == owner.as_ptr()))
                .count(),
            1
        );
    }
    assert!(engine.pending_retirements.is_empty());
    assert!(engine.scheduled_presentation.is_empty());
    assert_eq!(engine.presentation_state.terminals.occupied_slots(), 0);
    drop(delivered);
    assert!(removed_owners.iter().all(|owner| owner.strong_count() == 0));
    assert_eq!(arrange_owner.strong_count(), 1);
    assert_eq!(first_owner.strong_count(), 1);
    assert_eq!(base_audio.strong_count(), 1);
    assert_eq!(recording_audio.strong_count(), 1);
    assert!(packet_audio.iter().all(|owner| owner.strong_count() == 1));
    drop(engine);
    for owner in [arrange_owner, first_owner, base_audio, recording_audio] {
        assert_eq!(owner.strong_count(), 0);
    }
    assert!(packet_audio.iter().all(|owner| owner.strong_count() == 0));
    assert!(
        ordinary_length <= 32,
        "ordinary={ordinary_length}, heap={heap:?}"
    );
    assert_eq!(ordinary_capacity, 32);
    assert_eq!(heap, (0, 0));
}
