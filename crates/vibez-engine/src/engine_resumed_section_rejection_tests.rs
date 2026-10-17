//! Rejected Section sources survive boundary mutes and an older retained recording stop.

use super::*;
use crate::events::{
    AutomationOverrideChangedEvent, SectionRecordStoppedEvent, TrackMuteChangedEvent,
};
use crate::playback_source::{EngineClip, PreparedPlaybackSource, PreparedSectionPlaybackSource};
use graph_test_support::{graph_engine, routing_channel};
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::id::{ClipId, SectionId};
use vibez_core::perform::{SectionLaunchQuantization, TrackMuteQuantization};

fn activation_clip(id: ClipId, value: f32) -> EngineClip {
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

fn unmuted_lane() -> AutomationLane {
    let mut lane = AutomationLane::new(AutomationTarget::TrackMute);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 0.0,
        curve: 0.0,
    });
    lane
}

struct ReturningSectionFixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    tracks: [TrackId; 2],
    first_section: SectionId,
    old_section: SectionId,
    new_section: SectionId,
    middle_pointer: usize,
    incoming_pointer: usize,
    original_audio: std::sync::Weak<DecodedAudio>,
    incoming_audio: std::sync::Weak<DecodedAudio>,
    first_audio: std::sync::Weak<DecodedAudio>,
    arrange_audio: std::sync::Weak<DecodedAudio>,
    removed_owners: Vec<std::sync::Weak<DecodedAudio>>,
    acknowledgements: Vec<usize>,
    offsets: std::sync::Weak<[(TrackId, u32)]>,
    output: [f32; 32],
    heap: (usize, usize),
    after_len: usize,
    after_capacity: usize,
}

fn returning_section_fixture() -> ReturningSectionFixture {
    let tracks = [TrackId::new(), TrackId::new()];
    let first_section = SectionId::new();
    let old_section = SectionId::new();
    let new_section = SectionId::new();
    let removed_id = ClipId::new();
    let mut removed: Vec<_> = (0..32).map(|_| activation_clip(removed_id, 0.0)).collect();
    let removed_owners: Vec<_> = removed
        .iter()
        .map(|clip| Arc::downgrade(&clip.audio))
        .collect();
    let resident = activation_clip(ClipId::new(), 0.25);
    let arrange_audio = Arc::downgrade(&resident.audio);
    removed.push(resident);
    let first_clip = activation_clip(ClipId::new(), 0.25);
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
    let original_clip = activation_clip(ClipId::new(), 0.25);
    let original_audio = Arc::downgrade(&original_clip.audio);
    let incoming_clip = activation_clip(ClipId::new(), 0.5);
    let incoming_audio = Arc::downgrade(&incoming_clip.audio);
    let original = Box::new(PreparedSectionPlaybackSource::new(
        old_section,
        4.0,
        true,
        vec![(
            tracks[0],
            PreparedPlaybackSource::new(vec![original_clip], vec![], vec![]),
        )],
    ));
    let middle_pointer = std::ptr::from_ref(original.as_ref()) as usize;
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
        EngineCommand::ArmSectionRecord {
            section_id: first_section,
            track_id: tracks[0],
            prepared: Some(first_prepared),
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
    assert_eq!(engine.performance_position, 1);
    assert_eq!(engine.active_section.unwrap().position_samples, 1);
    assert!(engine.capture_lifecycle.is_active());
    while events.pop().is_ok() {}
    for position in 0..1024 {
        engine
            .event_tx
            .push(EngineEvent::PlaybackPosition(position))
            .unwrap();
    }
    let mut acknowledgements = Vec::new();
    for _ in 0..2008 {
        let owner = Box::new(PreparedSectionPlaybackSource::new(
            SectionId::new(),
            1.0,
            false,
            vec![],
        ));
        acknowledgements.push(std::ptr::from_ref(owner.as_ref()) as usize);
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
    for _ in 0..40 {
        {
            let mut retirement_allowance = engine
                .pending_retirements
                .reserve(1)
                .expect("fixture admits its original owner-return budget");
            engine.present_event_at(
                EngineEvent::PlaybackStarted,
                engine.output_position + 5000,
                &mut retirement_allowance,
            )
        };
    }
    assert_eq!(engine.scheduled_presentation.len(), 2048);
    let mut first_end = [0.0; 7];
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut first_end, 1));
        }),
        (0, 0)
    );
    assert_eq!(first_end, [0.25; 7]);
    assert!(
        matches!(engine.presentation_state.terminals.overflow_owner(), Some(
        EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent {
            section_id, effective_at_samples: 8, section_position_samples: 8,
            started: true, retired: None, ..
        })
    ) if *section_id == first_section)
    );
    assert_eq!(engine.pending_retirements.len(), 0);
    assert_eq!(engine.scheduled_presentation.len(), 2009);
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
    assert_eq!(engine.output_position_samples(), 136);
    assert!(!engine.presentation_state.is_faulted());
    assert!(engine
        .presentation_state
        .terminals
        .overflow_owner()
        .is_some());
    commands
        .push(EngineCommand::LaunchSection(original))
        .unwrap();
    let mut middle_first = [0.0];
    engine.process_block(AudioProcessBlock::new(&mut middle_first, 1));
    assert_eq!(middle_first, [0.25]);
    assert_eq!(engine.performance_position, 1);
    assert_eq!(engine.output_position_samples(), 137);
    assert_eq!(engine.active_section.unwrap().section_id, old_section);
    assert_eq!(engine.active_section.unwrap().length_samples, 32);
    assert_eq!(engine.output_gate.last_frame(), [0.25, 0.25]);
    assert!(!engine.presentation_state.is_faulted());
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
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
    assert_eq!(engine.performance_position, 1);
    assert_eq!(engine.output_position_samples(), 137);
    assert_eq!(engine.transport().position(), 168);
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
                effective_at_samples: 32,
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
    assert_eq!(engine.scheduled_presentation.capacity(), 2048);
    assert!(engine
        .presentation_state
        .terminals
        .overflow_owner()
        .is_some());
    assert_eq!(engine.presentation_state.terminals.capture_stop(), Some(1));
    assert!(!engine.presentation_state.is_faulted());
    assert!(engine.event_tx.is_full());
    assert!(engine.scheduled_presentation.len() < 2044);
    while engine.scheduled_presentation.len() < 2044 {
        let owner = Box::new(PreparedSectionPlaybackSource::new(
            SectionId::new(),
            1.0,
            false,
            vec![],
        ));
        acknowledgements.push(std::ptr::from_ref(owner.as_ref()) as usize);
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
    assert_eq!(engine.scheduled_presentation.len(), 2044);
    assert_eq!(engine.pending_retirements.len(), 0);
    assert!(engine
        .presentation_state
        .terminals
        .overflow_owner()
        .is_some());
    assert!(!engine.presentation_state.is_faulted());
    assert_eq!(original_audio.strong_count(), 1);
    assert_eq!(incoming_audio.strong_count(), 1);
    let mut output = [0.0; 32];
    let heap = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 1));
    });
    assert_eq!(engine.pending_retirements.reserved(), 0);
    assert!(engine.pending_retirements.len() <= 32);
    let after_len = engine.pending_retirements.len();
    let after_capacity = engine.pending_retirements.capacity();
    ReturningSectionFixture {
        engine,
        commands,
        events,
        tracks,
        first_section,
        old_section,
        new_section,
        middle_pointer,
        incoming_pointer,
        original_audio,
        incoming_audio,
        first_audio,
        arrange_audio,
        removed_owners,
        acknowledgements,
        offsets,
        output,
        heap,
        after_len,
        after_capacity,
    }
}

#[test]
fn resumed_section_rejection_preserves_complete_owner_with_an_old_inline_stop() {
    let ReturningSectionFixture {
        mut engine,
        commands: _commands,
        mut events,
        tracks,
        first_section,
        old_section,
        new_section: _new_section,
        middle_pointer,
        incoming_pointer,
        original_audio,
        incoming_audio,
        first_audio,
        arrange_audio,
        removed_owners,
        acknowledgements,
        offsets,
        output,
        heap,
        after_len,
        after_capacity,
    } = returning_section_fixture();
    let active_after = engine.active_section.map(|section| section.section_id);
    let expected_fade = std::array::from_fn(|frame| 0.25 * (1.0 - frame as f32 / 64.0));
    assert_eq!(output, expected_fade);
    assert_eq!(active_after, Some(old_section));
    assert_eq!(
        Arc::as_ptr(&engine.tracks()[0].section_playback_source.clips[0].audio),
        original_audio.as_ptr(),
    );
    assert_eq!(engine.performance_position, 33);
    assert_eq!(engine.output_position_samples(), 169);
    assert_eq!(engine.transport().position(), 168);
    assert!(!engine.transport().is_playing());
    assert!(!engine.capture_lifecycle.is_active());
    assert!(engine.presentation_state.is_faulted());
    let is_incoming = |event: &EngineEvent| {
        matches!(event,
        EngineEvent::SectionQueueCancelled { retired }
            if std::ptr::from_ref(retired.as_ref()) as usize == incoming_pointer)
    };
    let held = engine.queued_section.as_ref().is_some_and(|queued| {
        std::ptr::from_ref(queued.prepared.as_ref()) as usize == incoming_pointer
    });
    let published = engine
        .pending_retirements
        .as_slice()
        .iter()
        .filter(|event| is_incoming(event))
        .count()
        + engine
            .scheduled_presentation
            .iter()
            .filter(|pending| is_incoming(&pending.event))
            .count()
        + usize::from(
            engine
                .presentation_state
                .terminals
                .overflow_owner()
                .is_some_and(is_incoming),
        );
    assert_eq!(usize::from(held) + published, 1);
    assert_eq!(engine.presentation_state.terminals.capture_stop(), Some(1));
    assert!(
        matches!(engine.presentation_state.terminals.overflow_owner(), Some(
        EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent { section_id, effective_at_samples: 8, .. })
    ) if *section_id == first_section)
    );
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
            .pending_retirements
            .as_slice()
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
    assert_eq!(
        returned_sections
            .iter()
            .filter(|pointer| **pointer == middle_pointer)
            .count(),
        1
    );
    let prior: Vec<_> = returned_sections
        .iter()
        .copied()
        .filter(|pointer| *pointer != incoming_pointer && *pointer != middle_pointer)
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
    assert_eq!(terminals, ["capture", "capture", "failure", "playback"]);
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
    assert_eq!(
        delivered
            .iter()
            .filter(|event| matches!(event,
                EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent {
                    section_id, effective_at_samples: 8, section_position_samples: 8,
                    started: true, retired: None, ..
                }) if *section_id == first_section
            ))
            .count(),
        1
    );
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
    assert_eq!(first_audio.strong_count(), 0);
    assert_eq!(arrange_audio.strong_count(), 1);
    drop(engine);
    assert_eq!(arrange_audio.strong_count(), 0);
    assert_eq!(original_audio.strong_count(), 0);
    assert_eq!(incoming_audio.strong_count(), 0);
    assert!(
        after_len <= 32,
        "ordinary owner storage grew to {after_len}; measured heap={heap:?}"
    );
    assert_eq!(after_capacity, 32);
    assert_eq!(heap, (0, 0));
}

fn assert_rejected_source_is_returning(fixture: &ReturningSectionFixture) {
    let queued = fixture
        .engine
        .queued_section
        .as_ref()
        .expect("retained rejected source");
    assert!(queued.phase == SectionQueuePhase::Returning);
    assert_eq!(queued.effective_at_samples, 32);
    assert_eq!(
        std::ptr::from_ref(queued.prepared.as_ref()) as usize,
        fixture.incoming_pointer
    );
    assert_eq!(queued.prepared.section_id, fixture.new_section);
    assert_eq!(
        fixture.engine.active_section.unwrap().section_id,
        fixture.old_section
    );
    assert_eq!(
        Arc::as_ptr(&fixture.engine.tracks()[0].section_playback_source.clips[0].audio),
        fixture.original_audio.as_ptr()
    );
    assert_eq!(fixture.incoming_audio.strong_count(), 1);
    assert_eq!(fixture.heap, (0, 0));
    assert_eq!(fixture.after_len, 32);
    assert_eq!(fixture.after_capacity, 32);
    assert_eq!(fixture.engine.pending_retirements.len(), 32);
    assert_eq!(fixture.engine.pending_retirements.capacity(), 32);
}

fn next_section_launch(
    fixture: &mut ReturningSectionFixture,
) -> (SectionId, usize, std::sync::Weak<DecodedAudio>) {
    let section = SectionId::new();
    let clip = activation_clip(ClipId::new(), 0.125);
    let audio = Arc::downgrade(&clip.audio);
    let prepared = Box::new(PreparedSectionPlaybackSource::new(
        section,
        4.0,
        true,
        vec![(
            fixture.tracks[0],
            PreparedPlaybackSource::new(vec![clip], vec![], vec![]),
        )],
    ));
    let pointer = std::ptr::from_ref(prepared.as_ref()) as usize;
    fixture.commands.push(EngineCommand::Play).unwrap();
    fixture
        .commands
        .push(EngineCommand::LaunchSection(prepared))
        .unwrap();
    fixture
        .commands
        .push(EngineCommand::SetTrackPan(fixture.tracks[0], 0.375))
        .unwrap();
    (section, pointer, audio)
}

fn remove_original_ring_filler(events: &mut Consumer<EngineEvent>) {
    for position in 0..1024 {
        assert!(
            matches!(events.pop(), Ok(EngineEvent::PlaybackPosition(actual)) if actual == position)
        );
    }
    assert!(events.pop().is_err());
}

fn retry_empty_callbacks(fixture: &mut ReturningSectionFixture) -> Vec<EngineEvent> {
    let mut delivered = Vec::new();
    for _ in 0..8 {
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                fixture
                    .engine
                    .process_block(AudioProcessBlock::new(&mut [], 1));
            }),
            (0, 0)
        );
        assert_eq!(fixture.engine.pending_retirements.reserved(), 0);
        assert!(fixture.engine.pending_retirements.len() <= 32);
        while let Ok(event) = fixture.events.pop() {
            delivered.push(event);
        }
    }
    delivered
}

#[test]
fn returning_section_rejection_precedes_next_launch_and_its_fifo_marker() {
    let mut fixture = returning_section_fixture();
    assert_rejected_source_is_returning(&fixture);
    let before_pan = fixture.engine.tracks()[0].pan;
    assert_ne!(before_pan, 0.375);
    let (next, next_pointer, next_audio) = next_section_launch(&mut fixture);
    for _ in 0..2 {
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                fixture
                    .engine
                    .process_block(AudioProcessBlock::new(&mut [], 1));
            }),
            (0, 0)
        );
        assert_rejected_source_is_returning(&fixture);
        assert_eq!(fixture.engine.cmd_rx.slots(), 3);
        assert!(matches!(
            fixture.engine.cmd_rx.peek(),
            Ok(EngineCommand::Play)
        ));
        assert_eq!(fixture.engine.tracks()[0].pan, before_pan);
        assert_eq!(next_audio.strong_count(), 1);
    }
    remove_original_ring_filler(&mut fixture.events);
    let delivered = retry_empty_callbacks(&mut fixture);
    assert!(fixture.engine.cmd_rx.peek().is_err());
    assert!(fixture.engine.queued_section.is_none());
    assert_eq!(fixture.engine.active_section.unwrap().section_id, next);
    assert_eq!(
        Arc::as_ptr(&fixture.engine.tracks()[0].section_playback_source.clips[0].audio),
        next_audio.as_ptr()
    );
    assert_eq!(fixture.engine.tracks()[0].pan, 0.375);
    let rejected: Vec<_> = delivered
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            matches!(event,
        EngineEvent::SectionQueueCancelled { retired }
        if std::ptr::from_ref(retired.as_ref()) as usize == fixture.incoming_pointer)
        })
        .collect();
    let launched: Vec<_> = delivered.iter().enumerate().filter(|(_, event)| matches!(event,
        EngineEvent::SectionTransitioned(transition)
        if transition.section_id == next && std::ptr::from_ref(&*transition.retired) as usize == next_pointer)).collect();
    assert_eq!(rejected.len(), 1);
    assert_eq!(launched.len(), 1);
    assert!(rejected[0].0 < launched[0].0);
    assert!(!delivered.iter().any(|event| matches!(event,
        EngineEvent::SectionTransitioned(transition) if transition.section_id == fixture.new_section)));
    for owner in &fixture.removed_owners {
        assert_eq!(
            delivered
                .iter()
                .filter(|event| matches!(event,
            EngineEvent::RetiredAudioClip(clip) if Arc::as_ptr(&clip.audio) == owner.as_ptr()))
                .count(),
            1
        );
    }
    assert_eq!(fixture.incoming_audio.strong_count(), 1);
    assert_eq!(next_audio.strong_count(), 1);
    drop(rejected);
    drop(launched);
    drop(delivered);
    assert_eq!(fixture.incoming_audio.strong_count(), 0);
    assert_eq!(fixture.original_audio.strong_count(), 0);
    assert_eq!(fixture.first_audio.strong_count(), 0);
    assert!(fixture
        .removed_owners
        .iter()
        .all(|owner| owner.strong_count() == 0));
    assert_eq!(next_audio.strong_count(), 1);
    assert_eq!(fixture.arrange_audio.strong_count(), 1);
    drop(fixture.engine);
    assert_eq!(next_audio.strong_count(), 0);
    assert_eq!(fixture.arrange_audio.strong_count(), 0);
}

#[test]
fn returning_section_rejection_and_queued_next_launch_release_in_both_teardown_orders() {
    for engine_first in [false, true] {
        let mut fixture = returning_section_fixture();
        assert_rejected_source_is_returning(&fixture);
        let (_, _, next_audio) = next_section_launch(&mut fixture);
        if engine_first {
            drop(fixture.engine);
            assert_eq!(fixture.incoming_audio.strong_count(), 0);
            assert_eq!(fixture.original_audio.strong_count(), 0);
            assert_eq!(fixture.first_audio.strong_count(), 0);
            assert_eq!(fixture.arrange_audio.strong_count(), 0);
            assert!(fixture
                .removed_owners
                .iter()
                .all(|owner| owner.strong_count() == 0));
            assert_eq!(next_audio.strong_count(), 1);
            drop(fixture.events);
            drop(fixture.commands);
        } else {
            drop(fixture.events);
            drop(fixture.commands);
            assert_eq!(fixture.incoming_audio.strong_count(), 1);
            assert_eq!(fixture.original_audio.strong_count(), 1);
            assert_eq!(fixture.first_audio.strong_count(), 1);
            assert_eq!(fixture.arrange_audio.strong_count(), 1);
            assert!(fixture
                .removed_owners
                .iter()
                .all(|owner| owner.strong_count() == 1));
            assert_eq!(next_audio.strong_count(), 1);
            drop(fixture.engine);
        }
        assert_eq!(fixture.incoming_audio.strong_count(), 0);
        assert_eq!(next_audio.strong_count(), 0);
    }
}

#[test]
fn play_after_returning_section_recovery_never_revives_the_rejected_source() {
    let mut fixture = returning_section_fixture();
    assert_rejected_source_is_returning(&fixture);
    fixture.commands.push(EngineCommand::Play).unwrap();
    remove_original_ring_filler(&mut fixture.events);
    let delivered = retry_empty_callbacks(&mut fixture);
    assert!(!fixture.engine.presentation_state.is_faulted());
    assert!(fixture.engine.transport().is_playing());
    assert!(fixture.engine.queued_section.is_none());
    assert!(fixture
        .engine
        .active_section
        .is_none_or(|active| active.section_id != fixture.new_section));
    assert_eq!(
        delivered
            .iter()
            .filter(|event| matches!(event,
        EngineEvent::SectionQueueCancelled { retired }
        if std::ptr::from_ref(retired.as_ref()) as usize == fixture.incoming_pointer))
            .count(),
        1
    );
    assert!(!delivered.iter().any(|event| matches!(event,
        EngineEvent::SectionTransitioned(transition) if transition.section_id == fixture.new_section)));
    drop(delivered);
    assert_eq!(fixture.incoming_audio.strong_count(), 0);
    fixture.commands.push(EngineCommand::Play).unwrap();
    let later = retry_empty_callbacks(&mut fixture);
    assert!(fixture.engine.queued_section.is_none());
    assert!(fixture
        .engine
        .active_section
        .is_none_or(|active| active.section_id != fixture.new_section));
    assert!(!later.iter().any(|event| matches!(event,
        EngineEvent::SectionTransitioned(transition) if transition.section_id == fixture.new_section)));
    assert_eq!(fixture.incoming_audio.strong_count(), 0);
}
