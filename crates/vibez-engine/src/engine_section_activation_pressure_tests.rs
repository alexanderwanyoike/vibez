//! Section transitions retain complete owners when end-boundary mutes exhaust presentation space.

use super::*;
use crate::events::{AutomationOverrideChangedEvent, TrackMuteChangedEvent};
use crate::playback_source::{EngineClip, PreparedPlaybackSource, PreparedSectionPlaybackSource};
use graph_test_support::{graph_engine, routing_channel};
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::id::{ClipId, SectionId};
use vibez_core::perform::{SectionLaunchQuantization, TrackMuteQuantization};

fn activation_clip(id: ClipId, value: f32) -> EngineClip {
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

fn unmuted_lane() -> AutomationLane {
    let mut lane = AutomationLane::new(AutomationTarget::TrackMute);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 0.0,
        curve: 0.0,
    });
    lane
}

#[test]
fn queued_section_end_mutes_leave_room_for_the_complete_transition_owner() {
    let tracks = [TrackId::new(), TrackId::new()];
    let old_section = SectionId::new();
    let new_section = SectionId::new();
    let removed_id = ClipId::new();
    let removed: Vec<_> = (0..32)
        .map(|_| activation_clip(removed_id, 0.001))
        .collect();
    let removed_owners: Vec<_> = removed
        .iter()
        .map(|clip| Arc::downgrade(&clip.audio))
        .collect();
    let original_clip = activation_clip(ClipId::new(), 0.25);
    let original_audio = Arc::downgrade(&original_clip.audio);
    let incoming_clip = activation_clip(ClipId::new(), 0.5);
    let incoming_audio = Arc::downgrade(&incoming_clip.audio);
    let original = Box::new(PreparedSectionPlaybackSource::new(
        old_section,
        8.0,
        true,
        vec![(
            tracks[0],
            PreparedPlaybackSource::new(vec![original_clip], vec![], vec![]),
        )],
    ));
    let incoming = Box::new(PreparedSectionPlaybackSource::new(
        new_section,
        8.0,
        true,
        vec![(
            tracks[0],
            PreparedPlaybackSource::new(vec![incoming_clip], vec![], vec![]),
        )],
    ));
    let incoming_pointer = std::ptr::from_ref(incoming.as_ref()) as usize;
    let (mut engine, mut commands, mut events) = graph_engine(&[
        routing_channel(tracks[0]),
        routing_channel(tracks[1]),
        routing_channel(TrackId::MASTER),
    ]);
    for command in [
        EngineCommand::SetSampleRate(8),
        EngineCommand::SetBpm(60.0),
        EngineCommand::Seek(40),
        EngineCommand::AddTrack(Box::new(EngineTrack::with_playback_source(
            tracks[0],
            PreparedPlaybackSource::new(removed, vec![], vec![unmuted_lane()]),
        ))),
        EngineCommand::AddTrack(Box::new(EngineTrack::with_playback_source(
            tracks[1],
            PreparedPlaybackSource::new(vec![], vec![], vec![unmuted_lane()]),
        ))),
        EngineCommand::LaunchSection(original),
        EngineCommand::StartPerformanceCapture,
    ] {
        commands.push(command).unwrap();
    }
    let mut first = [0.0];
    engine.process_block(AudioProcessBlock::new(&mut first, 1));
    assert_eq!(first, [0.25]);
    assert_eq!(engine.performance_position, 1);
    assert_eq!(engine.active_section.unwrap().position_samples, 1);
    assert!(engine.capture_lifecycle.is_active());
    while events.pop().is_ok() {}
    for track in tracks {
        commands
            .push(EngineCommand::QueueTrackMute {
                track_id: track,
                muted: true,
                quantization: TrackMuteQuantization::EndOfSection,
            })
            .unwrap();
    }
    commands
        .push(EngineCommand::QueueSection {
            prepared: incoming,
            quantization: SectionLaunchQuantization::OneBar,
        })
        .unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [], 1));
    while events.pop().is_ok() {}
    assert_eq!(engine.performance_position, 1);
    assert_eq!(engine.output_position_samples(), 1);
    assert_eq!(engine.transport().position(), 40);
    assert_eq!(
        engine.queued_section.as_ref().unwrap().effective_at_samples,
        32
    );
    for track in engine.tracks() {
        assert!(track.has_automation_target(AutomationTarget::TrackMute));
        assert!(matches!(
            track.queued_mute,
            Some(crate::mixer::QueuedTrackMute {
                muted: true,
                effective_at_samples: 64,
                end_of_section: true
            })
        ));
        assert!(!track.mute);
    }
    assert!(engine.pending_section_record.is_none());
    assert!(engine.active_section_record.is_none());
    assert!(engine.pending_clip_insertion.is_none());
    assert!(engine.capture_replay.is_none());
    assert!(!engine.clip_batch_blocks_commands());
    assert!(!engine.pending_source_cleanup);
    assert!(engine.pending_bus_cleanup.is_none());
    assert!(engine.cmd_rx.peek().is_err());
    assert_eq!(engine.mix_latency(), 0);
    assert_eq!(engine.heard_capture_position(), 1);
    let offsets = Arc::downgrade(&engine.routing.capture_offsets);
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
    for position in 0..1024 {
        engine
            .event_tx
            .push(EngineEvent::PlaybackPosition(position))
            .unwrap();
    }
    assert!(engine.event_tx.is_full());
    let mut acknowledgements = Vec::new();
    for _ in 0..2044 {
        let owner = Box::new(PreparedSectionPlaybackSource::new(
            SectionId::new(),
            1.0,
            false,
            vec![],
        ));
        acknowledgements.push(std::ptr::from_ref(owner.as_ref()) as usize);
        {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event(EngineEvent::SectionQueueCancelled { retired: owner }, &mut retirement_allowance)
        };
    }
    assert_eq!(engine.scheduled_presentation.len(), 2044);
    assert_eq!(engine.pending_retirements.len(), 0);
    assert_eq!(engine.presentation_state.terminals.occupied_slots(), 0);
    assert!(!engine.presentation_state.is_faulted());
    assert_eq!(original_audio.strong_count(), 1);
    assert_eq!(incoming_audio.strong_count(), 1);
    let mut output = [0.0; 32];
    let heap = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 1));
    });
    let after_len = engine.pending_retirements.len();
    let after_capacity = engine.pending_retirements.capacity();
    let active_after = engine.active_section.map(|section| section.section_id);
    let expected_fade = std::array::from_fn(|frame| 0.25 * (1.0 - frame as f32 / 64.0));
    assert_eq!(output, expected_fade);
    assert_eq!(active_after, Some(old_section));
    assert_eq!(
        Arc::as_ptr(&engine.tracks()[0].section_playback_source.clips[0].audio),
        original_audio.as_ptr(),
    );
    assert_eq!(engine.performance_position, 33);
    assert_eq!(engine.output_position_samples(), 33);
    assert_eq!(engine.transport().position(), 40);
    assert!(!engine.transport().is_playing());
    assert!(!engine.capture_lifecycle.is_active());
    assert!(engine.presentation_state.is_faulted());
    assert!(engine.queued_section.is_none());
    assert_eq!(engine.presentation_state.terminals.capture_stop(), Some(1));
    assert_eq!(
        engine.presentation_state.terminals.failure(),
        Some((
            TrackId::MASTER,
            None,
            "Audio presentation event history is full"
        ))
    );
    assert_eq!(
        engine
            .pending_retirements.as_slice()
            .iter()
            .filter(|event| matches!(event, EngineEvent::RetiredAudioClip(_)))
            .count(),
        32
    );
    assert!(removed_owners.iter().all(|owner| owner.strong_count() == 1));
    assert_eq!(original_audio.strong_count(), 1);
    assert_eq!(incoming_audio.strong_count(), 1);
    for position in 0..1024 {
        assert!(
            matches!(events.pop(), Ok(EngineEvent::PlaybackPosition(actual)) if actual == position)
        );
    }
    assert!(events.pop().is_err());
    let mut delivered = Vec::new();
    for _ in 0..8 {
        engine.flush_presentation();
        engine.flush_retirements();
        while let Ok(event) = events.pop() {
            delivered.push(event);
        }
    }
    let returned_sections: Vec<_> = delivered
        .iter()
        .filter_map(|event| match event {
            EngineEvent::SectionQueueCancelled { retired } => {
                Some(std::ptr::from_ref(retired.as_ref()) as usize)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        returned_sections
            .iter()
            .filter(|pointer| **pointer == incoming_pointer)
            .count(),
        1
    );
    let prior: Vec<_> = returned_sections
        .iter()
        .copied()
        .filter(|pointer| *pointer != incoming_pointer)
        .collect();
    assert_eq!(prior, acknowledgements);
    for track in tracks {
        assert_eq!(delivered.iter().filter(|event| matches!(event,
            EngineEvent::TrackMuteChanged(TrackMuteChangedEvent { track_id, muted: true, effective_at_samples: 32 }) if *track_id == track
        )).count(), 1);
        assert_eq!(delivered.iter().filter(|event| matches!(event,
            EngineEvent::AutomationOverrideChanged(AutomationOverrideChangedEvent { track_id, target: AutomationTarget::TrackMute, overridden: true }) if *track_id == track
        )).count(), 1);
    }
    assert_eq!(
        delivered
            .iter()
            .filter(|event| matches!(
                event,
                EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples: 1
                }
            ))
            .count(),
        1
    );
    let capture_index = delivered
        .iter()
        .position(|event| {
            matches!(
                event,
                EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples: 1
                }
            )
        })
        .unwrap();
    assert_eq!(
        delivered[..capture_index]
            .iter()
            .filter(|event| matches!(event, EngineEvent::TrackMuteChanged(_)))
            .count(),
        2,
        "accepted due mute Capture feed precedes closure"
    );
    let terminals: Vec<_> = delivered
        .iter()
        .filter_map(|event| match event {
            EngineEvent::PerformanceCaptureStopped { .. } => Some("capture"),
            EngineEvent::CompensationInvalid { .. } => Some("failure"),
            EngineEvent::PlaybackStopped => Some("playback"),
            _ => None,
        })
        .collect();
    assert_eq!(terminals, ["capture", "failure", "playback"]);
    assert_eq!(
        delivered
            .iter()
            .filter(|event| matches!(event, EngineEvent::PlaybackStopped))
            .count(),
        1
    );
    assert_eq!(delivered.iter().filter(|event| matches!(event,
        EngineEvent::CompensationInvalid { track_id, effect_id: None, reason: "Audio presentation event history is full" } if *track_id == TrackId::MASTER
    )).count(), 1);
    assert!(!delivered
        .iter()
        .any(|event| matches!(event, EngineEvent::SectionTransitioned(_))));
    for owner in &removed_owners {
        assert_eq!(delivered.iter().filter(|event| matches!(event,
            EngineEvent::RetiredAudioClip(clip) if Arc::as_ptr(&clip.audio) == owner.as_ptr()
        )).count(), 1);
    }
    assert!(engine.pending_retirements.is_empty());
    assert!(engine.scheduled_presentation.is_empty());
    assert_eq!(engine.presentation_state.terminals.occupied_slots(), 0);
    drop(delivered);
    assert!(removed_owners.iter().all(|owner| owner.strong_count() == 0));
    assert_eq!(original_audio.strong_count(), 1);
    assert_eq!(incoming_audio.strong_count(), 0);
    assert_eq!(offsets.strong_count(), 1);
    drop(engine);
    assert_eq!(original_audio.strong_count(), 0);
    assert_eq!(incoming_audio.strong_count(), 0);
    assert!(
        after_len <= 32,
        "ordinary owner storage grew to {after_len}; measured heap={heap:?}"
    );
    assert_eq!(after_capacity, 32);
    assert_eq!(heap, (0, 0));
}
