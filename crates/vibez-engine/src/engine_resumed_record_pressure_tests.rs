//! Resumed recording preserves earlier stop notifications through a second full-queue ending.

use super::*;
use crate::events::{SectionRecordStoppedEvent, TrackMuteChangedEvent};
use crate::playback_source::{EngineClip, PreparedPlaybackSource, PreparedSectionPlaybackSource};
use graph_test_support::{graph_engine, routing_channel};
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::id::{ClipId, SectionId};
use vibez_core::perform::TrackMuteQuantization;

fn resumed_clip(id: ClipId, value: f32) -> EngineClip {
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

fn resumed_mute_lane() -> AutomationLane {
    let mut lane = AutomationLane::new(AutomationTarget::TrackMute);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 0.0,
        curve: 0.0,
    });
    lane
}

#[test]
fn resumed_record_end_retains_both_reliable_stops_without_growing_ordinary_owners() {
    let tracks: [TrackId; 8] = std::array::from_fn(|_| TrackId::new());
    let first_section = SectionId::new();
    let second_section = SectionId::new();
    let removed_id = ClipId::new();
    let mut arrange_clips: Vec<_> = (0..32).map(|_| resumed_clip(removed_id, 0.0)).collect();
    let removed_owners: Vec<_> = arrange_clips
        .iter()
        .map(|clip| Arc::downgrade(&clip.audio))
        .collect();
    let resident = resumed_clip(ClipId::new(), 0.25);
    let arrange_owner = Arc::downgrade(&resident.audio);
    arrange_clips.push(resident);
    let first_clip = resumed_clip(ClipId::new(), 0.25);
    let first_audio = Arc::downgrade(&first_clip.audio);
    let first_prepared = Box::new(PreparedSectionPlaybackSource::new(
        first_section,
        1.0,
        false,
        vec![(
            tracks[0],
            PreparedPlaybackSource::new(vec![first_clip], vec![], vec![]),
        )],
    ));
    let mut second_audio = Vec::new();
    let second_sources: Vec<_> = tracks
        .iter()
        .map(|track| {
            let clip = resumed_clip(ClipId::new(), 0.0625);
            second_audio.push(Arc::downgrade(&clip.audio));
            (
                *track,
                PreparedPlaybackSource::new(vec![clip], vec![], vec![resumed_mute_lane()]),
            )
        })
        .collect();
    let second_prepared = Box::new(PreparedSectionPlaybackSource::new(
        second_section,
        2.0,
        false,
        second_sources,
    ));
    let second_pointer = std::ptr::from_ref(second_prepared.as_ref()) as usize;
    let mut model: Vec<_> = tracks.iter().copied().map(routing_channel).collect();
    model.push(routing_channel(TrackId::MASTER));
    let (mut engine, mut commands, mut events) = graph_engine(&model);
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
                PreparedPlaybackSource::new(arrange_clips, vec![], vec![]),
            ),
        )))
        .unwrap();
    for track in &tracks[1..] {
        commands.push(EngineCommand::add_track(*track)).unwrap();
    }
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id: first_section,
            track_id: tracks[0],
            prepared: Some(first_prepared),
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
    assert_eq!(engine.performance_position, 1);
    assert!(engine.capture_lifecycle.is_active());
    while events.pop().is_ok() {}
    for position in 0..1024 {
        engine
            .event_tx
            .push(EngineEvent::PlaybackPosition(position))
            .unwrap();
    }
    assert!(engine.event_tx.is_full());
    let mut acknowledgements = Vec::new();
    for _ in 0..2008 {
        let prepared = Box::new(PreparedSectionPlaybackSource::new(
            SectionId::new(),
            1.0,
            false,
            vec![],
        ));
        acknowledgements.push(std::ptr::from_ref(prepared.as_ref()) as usize);
        {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event(EngineEvent::SectionQueueCancelled { retired: prepared }, &mut retirement_allowance)
        };
    }
    for _ in 0..40 {
        {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event_at(EngineEvent::PlaybackStarted, engine.output_position + 5000, &mut retirement_allowance)
        };
    }
    assert_eq!(engine.scheduled_presentation.len(), 2048);
    assert_eq!(engine.pending_retirements.len(), 0);
    assert_eq!(engine.presentation_state.terminals.occupied_slots(), 0);
    let mut first_loss = [0.0; 7];
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut first_loss, 1));
        }),
        (0, 0)
    );
    assert_eq!(first_loss, [0.25; 7]);
    assert_eq!(engine.output_gate.remaining_loss_frames(), 0);
    assert_eq!(engine.output_gate.remaining_resume_frames(), 0);
    assert_eq!(engine.output_position_samples(), 8);
    assert_eq!(engine.performance_position, 8);
    assert_eq!(engine.transport().position(), 40);
    assert_eq!(engine.scheduled_presentation.len(), 2009);
    assert_eq!(engine.pending_retirements.len(), 0);
    assert!(engine.presentation_state.is_faulted());
    assert!(
        matches!(engine.presentation_state.terminals.overflow_owner(), Some(
        EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent {
            section_id, track_id, effective_at_samples: 8, section_position_samples: 8,
            started: true, retired: None,
        })
    ) if *section_id == first_section && *track_id == tracks[0])
    );
    assert_eq!(engine.presentation_state.terminals.capture_stop(), Some(1));
    assert!(!engine.capture_lifecycle.is_active());
    commands.push(EngineCommand::Play).unwrap();
    let mut resumed = [0.0; 128];
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut resumed, 1));
        }),
        (0, 0)
    );
    assert_eq!(resumed, [0.25; 128]);
    assert_eq!(engine.output_gate.remaining_loss_frames(), 0);
    assert_eq!(engine.output_gate.remaining_resume_frames(), 0);
    assert!(!engine.presentation_state.is_faulted());
    assert!(!engine.project_processing_muted());
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().position(), 168);
    assert_eq!(engine.output_position_samples(), 136);
    assert_eq!(engine.scheduled_presentation.len(), 2010);
    assert!(engine
        .presentation_state
        .terminals
        .overflow_owner()
        .is_some());
    assert_eq!(engine.event_tx.slots(), 0);
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id: second_section,
            track_id: tracks[0],
            prepared: Some(second_prepared),
            count_in_bars: 0,
            replace_existing: false,
        })
        .unwrap();
    let mut new_first = [0.0];
    engine.process_block(AudioProcessBlock::new(&mut new_first, 1));
    assert_eq!(new_first, [0.5]);
    assert_eq!(engine.performance_position, 1);
    assert_eq!(engine.output_position_samples(), 137);
    assert_eq!(engine.scheduled_presentation.len(), 2015);
    assert_eq!(engine.output_gate.last_frame(), [0.5, 0.5]);
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    for track in tracks {
        commands
            .push(EngineCommand::QueueTrackMute {
                track_id: track,
                muted: true,
                quantization: TrackMuteQuantization::OneBeat,
            })
            .unwrap();
    }
    engine.process_block(AudioProcessBlock::new(&mut [], 1));
    assert_eq!(engine.scheduled_presentation.len(), 2032);
    assert_eq!(engine.scheduled_presentation.capacity(), 2048);
    assert!(engine.cmd_rx.peek().is_err());
    assert!(!engine.presentation_state.is_faulted());
    assert!(engine.capture_lifecycle.is_active());
    assert!(engine
        .presentation_state
        .terminals
        .overflow_owner()
        .is_some());
    assert_eq!(engine.presentation_state.terminals.capture_stop(), Some(1));
    assert_eq!(
        engine.active_section_record.unwrap().section_id,
        second_section
    );
    assert!(engine.pending_section_record.is_none());
    assert!(engine.queued_section.is_none());
    assert!(engine.capture_replay.is_none());
    assert!(engine.pending_clip_insertion.is_none());
    assert!(engine.pending_bus_cleanup.is_none());
    assert!(!engine.pending_source_cleanup);
    assert!(!engine.clip_batch_blocks_commands());
    assert_eq!(engine.mix_latency(), 0);
    assert_eq!(engine.pending_retirements.len(), 0);
    assert_eq!(engine.pending_retirements.capacity(), 32);
    for track in engine.tracks() {
        assert!(matches!(
            track.queued_mute,
            Some(crate::mixer::QueuedTrackMute {
                muted: true,
                effective_at_samples: 8,
                end_of_section: false,
            })
        ));
    }
    engine.tracks_mut_for_retirement()[0]
        .playback_source
        .remove_audio_clips(removed_id);
    assert!(engine.tracks()[0].playback_source.has_removed_audio_clips());
    assert_eq!(engine.pending_retirements.len(), 0);
    assert!(removed_owners.iter().all(|owner| owner.strong_count() == 1));
    let offsets = Arc::downgrade(&engine.routing.capture_offsets);
    let mut second_loss = [0.0; 15];
    let heap = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut second_loss, 1));
    });
    let ordinary_length = engine.pending_retirements.len();
    let ordinary_capacity = engine.pending_retirements.capacity();
    for (index, actual) in second_loss.iter().enumerate() {
        let expected = if index < 7 {
            0.5
        } else {
            0.5 * (70 - index) as f32 / 64.0
        };
        assert_eq!(*actual, expected, "recorded frame {index}");
    }
    assert_eq!(engine.output_gate.remaining_loss_frames(), 0);
    assert_eq!(engine.output_gate.remaining_resume_frames(), 0);
    assert_eq!(engine.performance_position, 16);
    assert_eq!(engine.output_position_samples(), 152);
    assert_eq!(engine.transport().position(), 168);
    assert!(!engine.transport().is_playing());
    assert!(engine.active_section.is_none());
    assert!(engine.active_section_record.is_none());
    assert!(!engine.capture_lifecycle.is_active());
    assert!(engine.presentation_state.is_faulted());
    assert_eq!(engine.presentation_state.terminals.capture_stop(), Some(1));
    assert!(
        matches!(engine.presentation_state.terminals.overflow_owner(), Some(
        EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent { section_id, effective_at_samples: 8, .. })
    ) if *section_id == first_section)
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
    assert_eq!(first_audio.strong_count(), 1);
    assert!(second_audio.iter().all(|owner| owner.strong_count() == 1));
    assert_eq!(arrange_owner.strong_count(), 1);
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
    let mut records = Vec::new();
    let mut returned_acknowledgements = Vec::new();
    let mut second_returns = 0;
    for event in &delivered {
        match event {
            EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent {
                section_id,
                track_id,
                effective_at_samples,
                section_position_samples,
                started: true,
                retired: None,
            }) => {
                assert_eq!(*track_id, tracks[0]);
                records.push((
                    *section_id,
                    *effective_at_samples,
                    *section_position_samples,
                ));
            }
            EngineEvent::SectionQueueCancelled { retired } => {
                let pointer = std::ptr::from_ref(retired.as_ref()) as usize;
                if pointer == second_pointer {
                    second_returns += 1;
                } else {
                    returned_acknowledgements.push(pointer);
                }
            }
            _ => {}
        }
    }
    assert_eq!(records, [(first_section, 8, 8), (second_section, 16, 16)]);
    assert_eq!(returned_acknowledgements, acknowledgements);
    assert_eq!(second_returns, 1);
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
    for track in tracks {
        assert_eq!(delivered.iter().filter(|event| matches!(event,
            EngineEvent::TrackMuteChanged(TrackMuteChangedEvent { track_id, muted: true, effective_at_samples: 8 }) if *track_id == track
        )).count(), 1);
        assert_eq!(delivered.iter().filter(|event| matches!(event,
            EngineEvent::AutomationOverrideChanged(crate::events::AutomationOverrideChangedEvent {
                track_id, target: AutomationTarget::TrackMute, overridden: true,
            }) if *track_id == track
        )).count(), 1);
    }
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
    assert_eq!(first_audio.strong_count(), 0);
    assert!(second_audio.iter().all(|owner| owner.strong_count() == 1));
    assert_eq!(arrange_owner.strong_count(), 1);
    assert_eq!(offsets.strong_count(), 1);
    drop(engine);
    assert!(second_audio.iter().all(|owner| owner.strong_count() == 0));
    assert_eq!(arrange_owner.strong_count(), 0);
    assert!(
        ordinary_length <= 32,
        "ordinary grew to {ordinary_length}, heap={heap:?}"
    );
    assert_eq!(ordinary_capacity, 32);
    assert_eq!(heap, (0, 0));
}
