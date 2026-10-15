//! End-of-Section ownership and terminal delivery survive simultaneous queue backpressure.

use super::*;
use crate::events::SectionRecordStoppedEvent;
use crate::playback_source::{EngineClip, PreparedPlaybackSource, PreparedSectionPlaybackSource};
use graph_test_support::{graph_engine, routing_channel};
use vibez_core::id::{ClipId, SectionId};

fn pressure_clip(id: ClipId, value: f32) -> EngineClip {
    EngineClip {
        id,
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![value; 128]],
            sample_rate: 8,
        }),
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
    }
}

#[test]
fn one_shot_recording_end_preserves_owners_and_terminals_without_growing_retirements() {
    let track = TrackId::new();
    let section = SectionId::new();
    let removed_id = ClipId::new();
    let removed_clips: Vec<_> = (0..32).map(|_| pressure_clip(removed_id, 0.001)).collect();
    let removed_owners: Vec<_> = removed_clips
        .iter()
        .map(|clip| Arc::downgrade(&clip.audio))
        .collect();
    let audible_clip = pressure_clip(ClipId::new(), 0.25);
    let audible_owner = Arc::downgrade(&audible_clip.audio);
    let prepared = Box::new(PreparedSectionPlaybackSource::new(
        section,
        1.0,
        false,
        vec![(
            track,
            PreparedPlaybackSource::new(vec![audible_clip], vec![], vec![]),
        )],
    ));
    let (mut engine, mut commands, mut events) =
        graph_engine(&[routing_channel(track), routing_channel(TrackId::MASTER)]);
    for command in [
        EngineCommand::SetSampleRate(8),
        EngineCommand::SetBpm(60.0),
        EngineCommand::Seek(40),
        EngineCommand::AddTrack(Box::new(EngineTrack::with_playback_source(
            track,
            PreparedPlaybackSource::new(removed_clips, vec![], vec![]),
        ))),
        EngineCommand::ArmSectionRecord {
            section_id: section,
            track_id: track,
            prepared: Some(prepared),
            count_in_bars: 0,
            replace_existing: false,
        },
        EngineCommand::StartPerformanceCapture,
    ] {
        commands.push(command).unwrap();
    }
    let mut first = [0.0];
    engine.process_block(AudioProcessBlock::new(&mut first, 1));
    assert_eq!(first, [0.25]);
    assert_eq!(engine.active_section.unwrap().position_samples, 1);
    assert_eq!(
        engine.active_section_record.unwrap().effective_at_samples,
        0
    );
    assert!(engine.capture_lifecycle.is_active());
    while events.pop().is_ok() {}

    engine.tracks_mut_for_retirement()[0]
        .playback_source
        .remove_audio_clips(removed_id);
    assert!(engine.tracks()[0].playback_source.has_removed_audio_clips());
    assert_eq!(engine.pending_retirements.len(), 0);
    assert_eq!(engine.pending_retirements.capacity(), 32);
    assert_eq!(engine.scheduled_presentation.len(), 0);
    assert_eq!(engine.scheduled_presentation.capacity(), 2048);
    assert_eq!(engine.presentation_state.terminals.occupied_slots(), 0);
    assert!(!engine.presentation_state.is_faulted());
    assert!(engine.cmd_rx.peek().is_err());
    assert!(!engine.clip_batch_blocks_commands());
    assert!(engine.capture_replay.is_none());
    assert!(engine.pending_section_record.is_none());
    assert!(engine.queued_section.is_none());
    assert!(engine.pending_clip_insertion.is_none());
    assert!(!engine.pending_source_cleanup);
    assert!(engine.pending_bus_cleanup.is_none());
    assert_eq!(engine.mix_latency(), 0);
    assert_eq!(engine.heard_capture_position(), 1);
    for position in 0..1024 {
        engine
            .event_tx
            .push(EngineEvent::PlaybackPosition(position))
            .unwrap();
    }
    assert!(engine.event_tx.is_full());
    let mut acknowledgement_owners = Vec::new();
    for _ in 0..2048 {
        let owner = Box::new(PreparedSectionPlaybackSource::new(
            SectionId::new(),
            1.0,
            false,
            vec![],
        ));
        acknowledgement_owners.push(std::ptr::from_ref(owner.as_ref()) as usize);
        {
            let mut retirement_allowance = engine
                .pending_retirements
                .reserve(1)
                .expect("fixture admits its original owner-return budget");
            engine.present_event(
                EngineEvent::SectionQueueCancelled { retired: owner },
                &mut retirement_allowance,
            )
        };
    }
    assert_eq!(engine.scheduled_presentation.len(), 2048);
    assert_eq!(engine.pending_retirements.len(), 0);
    assert_eq!(engine.presentation_state.terminals.occupied_slots(), 0);
    assert!(!engine.presentation_state.is_faulted());
    let capacity = engine.pending_retirements.capacity();
    let mut output = [0.0; 7];
    let heap = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 1));
    });
    let after_len = engine.pending_retirements.len();
    let after_capacity = engine.pending_retirements.capacity();
    assert_eq!(output, [0.25; 7]);
    assert_eq!(engine.performance_position, 8);
    assert_eq!(engine.output_position_samples(), 8);
    assert_eq!(engine.transport().position(), 40);
    assert!(!engine.transport().is_playing());
    assert!(engine.active_section.is_none());
    assert!(engine.active_section_record.is_none());
    assert!(!engine.capture_lifecycle.is_active());
    assert!(engine.presentation_state.is_faulted());
    assert_eq!(engine.presentation_state.terminals.capture_stop(), Some(1));
    assert_eq!(
        engine.presentation_state.terminals.failure(),
        Some((
            TrackId::MASTER,
            None,
            "Audio presentation event history is full"
        ))
    );
    assert!(
        matches!(engine.presentation_state.terminals.overflow_owner(),
        Some(EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent {
            section_id, track_id, effective_at_samples: 8, section_position_samples: 8,
            started: true, retired: None,
        })) if *section_id == section && *track_id == track)
    );
    assert_eq!(
        engine
            .pending_retirements
            .as_slice()
            .iter()
            .filter(|event| matches!(event, EngineEvent::RetiredAudioClip(_)))
            .count(),
        32
    );
    assert!(removed_owners.iter().all(|owner| owner.strong_count() == 1));
    assert_eq!(audible_owner.strong_count(), 1);
    for position in 0..1024 {
        assert!(
            matches!(events.pop(), Ok(EngineEvent::PlaybackPosition(actual)) if actual == position)
        );
    }
    assert!(events.pop().is_err());
    let mut returned_acknowledgements = Vec::new();
    let mut returned_audio = Vec::new();
    let mut terminal_order = Vec::new();
    for _ in 0..8 {
        engine.flush_presentation();
        engine.flush_retirements();
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples: 1,
                } => terminal_order.push("capture"),
                EngineEvent::CompensationInvalid {
                    track_id,
                    effect_id: None,
                    reason: "Audio presentation event history is full",
                } if track_id == TrackId::MASTER => terminal_order.push("failure"),
                EngineEvent::PlaybackStopped => terminal_order.push("playback"),
                EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent {
                    section_id,
                    track_id,
                    effective_at_samples: 8,
                    section_position_samples: 8,
                    started: true,
                    retired: None,
                }) if section_id == section && track_id == track => terminal_order.push("record"),
                EngineEvent::SectionQueueCancelled { retired } => {
                    returned_acknowledgements.push(std::ptr::from_ref(retired.as_ref()) as usize)
                }
                EngineEvent::RetiredAudioClip(clip) => {
                    returned_audio.push(Arc::as_ptr(&clip.audio))
                }
                EngineEvent::PresentationCancelled => {}
                event => panic!("unexpected recovery event: {event:?}"),
            }
        }
    }
    assert_eq!(terminal_order, ["capture", "failure", "playback", "record"]);
    assert_eq!(returned_acknowledgements, acknowledgement_owners);
    assert_eq!(returned_audio.len(), 32);
    for owner in &removed_owners {
        assert_eq!(
            returned_audio
                .iter()
                .filter(|pointer| **pointer == owner.as_ptr())
                .count(),
            1
        );
        assert_eq!(owner.strong_count(), 0);
    }
    assert_eq!(audible_owner.strong_count(), 1);
    assert!(engine.pending_retirements.is_empty());
    assert!(engine.scheduled_presentation.is_empty());
    assert_eq!(engine.presentation_state.terminals.occupied_slots(), 0);
    assert!(after_len <= 32, "ordinary fallback grew to {after_len}; heap {heap:?}, capacity {capacity} -> {after_capacity}");
    assert_eq!(after_capacity, capacity);
    assert_eq!(heap, (0, 0));
}
