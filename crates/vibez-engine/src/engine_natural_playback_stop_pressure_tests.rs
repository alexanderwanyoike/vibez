//! Proves natural playback completion retains owners when its final notification discovers overflow.

use super::*;
use crate::events::SectionRecordStoppedEvent;
use crate::playback_source::{EngineClip, PreparedPlaybackSource, PreparedSectionPlaybackSource};
use graph_test_support::{graph_engine, routing_channel};
use vibez_core::id::{ClipId, SectionId};

fn terminal_clip(id: ClipId, value: f32) -> EngineClip {
    EngineClip {
        id, audio: Arc::new(DecodedAudio { channels: vec![vec![value; 1024]], sample_rate: 8 }),
        position: 0, source_offset: 0, start_marker: 0, duration: 1024,
        loop_enabled: false, loop_start: 0, loop_end: 0, linear_gain: 1.0,
        fades: Default::default(), playback_direction: Default::default(), warp_markers: Default::default(),
    }
}

fn terminal_section(section: SectionId, track: TrackId) -> (Box<PreparedSectionPlaybackSource>, std::sync::Weak<DecodedAudio>) {
    let clip = terminal_clip(ClipId::new(), 0.25);
    let owner = Arc::downgrade(&clip.audio);
    (Box::new(PreparedSectionPlaybackSource::new(section, 1.0, false,
        vec![(track, PreparedPlaybackSource::new(vec![clip], vec![], vec![]))])), owner)
}

struct OldStopFixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    track: TrackId,
    first_section: SectionId,
    first_audio: std::sync::Weak<DecodedAudio>,
    arrange_audio: std::sync::Weak<DecodedAudio>,
    removed_audio: Vec<std::sync::Weak<DecodedAudio>>,
    removed_id: ClipId,
    acknowledgements: Vec<usize>,
}

fn old_stop_fixture() -> OldStopFixture {
    let track = TrackId::new();
    let first_section = SectionId::new();
    let removed_id = ClipId::new();
    let mut clips: Vec<_> = (0..32).map(|_| terminal_clip(removed_id, 0.0)).collect();
    let removed_audio: Vec<_> = clips.iter().map(|clip| Arc::downgrade(&clip.audio)).collect();
    let resident = terminal_clip(ClipId::new(), 0.25);
    let arrange_audio = Arc::downgrade(&resident.audio);
    clips.push(resident);
    let (prepared, first_audio) = terminal_section(first_section, track);
    let (mut engine, mut commands, mut events) = graph_engine(&[routing_channel(track), routing_channel(TrackId::MASTER)]);
    for command in [EngineCommand::SetSampleRate(8), EngineCommand::SetBpm(60.0), EngineCommand::Seek(40),
        EngineCommand::AddTrack(Box::new(EngineTrack::with_playback_source(track,
            PreparedPlaybackSource::new(clips, vec![], vec![])))),
        EngineCommand::ArmSectionRecord { section_id: first_section, track_id: track,
            prepared: Some(prepared), count_in_bars: 0, replace_existing: false },
        EngineCommand::StartPerformanceCapture,
    ] { commands.push(command).unwrap(); }
    let mut first = [0.0];
    engine.process_block(AudioProcessBlock::new(&mut first, 1));
    assert_eq!(first, [0.25]);
    while events.pop().is_ok() {}
    for position in 0..1024 { engine.event_tx.push(EngineEvent::PlaybackPosition(position)).unwrap(); }
    let mut acknowledgements = Vec::new();
    for _ in 0..2008 {
        let owner = Box::new(PreparedSectionPlaybackSource::new(SectionId::new(), 1.0, false, vec![]));
        acknowledgements.push(std::ptr::from_ref(owner.as_ref()) as usize);
        engine.present_event(EngineEvent::SectionQueueCancelled { retired: owner });
    }
    for _ in 0..40 { engine.present_event_at(EngineEvent::PlaybackStarted, engine.output_position + 5000); }
    assert_eq!(engine.scheduled_presentation.len(), 2048);
    let mut ended = [0.0; 7];
    assert_eq!(crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut ended, 1));
    }), (0, 0));
    assert_eq!(ended, [0.25; 7]);
    assert!(matches!(engine.presentation_state.terminals.overflow_owner(), Some(
        EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent {
            section_id, effective_at_samples: 8, section_position_samples: 8,
            started: true, retired: None, ..
        })
    ) if *section_id == first_section));
    assert_eq!(engine.pending_retirements.len(), 0);
    assert_eq!(engine.scheduled_presentation.len(), 2009);
    commands.push(EngineCommand::Play).unwrap();
    let mut resumed = [0.0; 128];
    assert_eq!(crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut resumed, 1));
    }), (0, 0));
    assert_eq!(resumed, [0.25; 128]);
    assert_eq!(engine.transport().position(), 168);
    assert_eq!(engine.output_position_samples(), 136);
    assert!(!engine.presentation_state.is_faulted());
    assert!(!engine.presentation_state.terminals.playback_stop_pending());
    assert_eq!(engine.presentation_state.terminals.capture_stop(), Some(1));
    OldStopFixture { engine, commands, events, track, first_section, first_audio, arrange_audio,
        removed_audio, removed_id, acknowledgements }
}

#[test]
fn natural_last_playback_stop_keeps_all_owners_without_growing_ordinary_returns() {
    let mut fixture = old_stop_fixture();
    let middle = SectionId::new();
    let (prepared, middle_audio) = terminal_section(middle, fixture.track);
    let middle_pointer = std::ptr::from_ref(prepared.as_ref()) as usize;
    fixture.commands.push(EngineCommand::LaunchSection(prepared)).unwrap();
    let mut first = [0.0];
    fixture.engine.process_block(AudioProcessBlock::new(&mut first, 1));
    assert_eq!(first, [0.25]);
    assert_eq!(fixture.engine.performance_position, 1);
    assert_eq!(fixture.engine.output_position_samples(), 137);
    assert_eq!(fixture.engine.active_section.unwrap().section_id, middle);
    assert_eq!(fixture.engine.active_section.unwrap().length_samples, 8);
    assert!(!fixture.engine.active_section.unwrap().looping);
    fixture.commands.push(EngineCommand::StartPerformanceCapture).unwrap();
    fixture.engine.process_block(AudioProcessBlock::new(&mut [], 1));
    assert!(fixture.engine.capture_lifecycle.is_active());
    assert!(fixture.engine.pending_section_record.is_none());
    assert!(fixture.engine.active_section_record.is_none());
    assert!(fixture.engine.pending_section_record_stop.is_none());
    assert!(fixture.engine.queued_section.is_none());
    assert!(fixture.engine.tracks().iter().all(|track| track.queued_mute.is_none()));
    assert!(fixture.engine.pending_clip_insertion.is_none());
    assert!(fixture.engine.capture_replay.is_none());
    assert!(!fixture.engine.clip_batch_blocks_commands());
    assert!(!fixture.engine.pending_source_cleanup);
    assert!(fixture.engine.pending_bus_cleanup.is_none());
    assert!(fixture.engine.cmd_rx.peek().is_err());
    assert_eq!(fixture.engine.mix_latency(), 0);
    assert_eq!(fixture.engine.heard_capture_position(), 1);
    assert!(fixture.engine.event_tx.is_full());
    assert_eq!(fixture.engine.pending_retirements.len(), 0);
    while fixture.engine.scheduled_presentation.len() < 2046 {
        let owner = Box::new(PreparedSectionPlaybackSource::new(SectionId::new(), 1.0, false, vec![]));
        fixture.acknowledgements.push(std::ptr::from_ref(owner.as_ref()) as usize);
        fixture.engine.present_event(EngineEvent::SectionQueueCancelled { retired: owner });
    }
    assert_eq!(fixture.engine.scheduled_presentation.len(), 2046);
    assert_eq!(fixture.engine.scheduled_presentation.capacity(), 2048);
    assert!(!fixture.engine.presentation_state.is_faulted());
    assert!(!fixture.engine.presentation_state.terminals.playback_stop_pending());
    fixture.engine.tracks_mut_for_retirement()[0].playback_source.remove_audio_clips(fixture.removed_id);
    assert!(fixture.engine.tracks()[0].playback_source.has_removed_audio_clips());
    assert_eq!(fixture.engine.pending_retirements.len(), 0);
    let offsets = Arc::downgrade(&fixture.engine.routing.capture_offsets);
    let mut output = [0.0; 7];
    let heap = crate::retirement::tests::allocations(|| {
        fixture.engine.process_block(AudioProcessBlock::new(&mut output, 1));
    });
    let length = fixture.engine.pending_retirements.len();
    let capacity = fixture.engine.pending_retirements.capacity();
    eprintln!("natural last-stop proposal: heap={heap:?}, ordinary={length}/{capacity}");
    assert_eq!(output, [0.25; 7]);
    assert_eq!(fixture.engine.output_gate.remaining_loss_frames(), 0);
    assert_eq!(fixture.engine.output_gate.remaining_resume_frames(), 0);
    assert_eq!(fixture.engine.performance_position, 8);
    assert_eq!(fixture.engine.output_position_samples(), 144);
    assert_eq!(fixture.engine.transport().position(), 168);
    assert!(!fixture.engine.transport().is_playing());
    assert!(fixture.engine.active_section.is_none());
    assert!(!fixture.engine.capture_lifecycle.is_active());
    assert!(fixture.engine.presentation_state.is_faulted());
    assert_eq!(fixture.engine.presentation_state.terminals.capture_stop(), Some(1));
    assert!(fixture.engine.presentation_state.terminals.playback_stop_pending());
    assert!(matches!(fixture.engine.presentation_state.terminals.overflow_owner(), Some(
        EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent { section_id, effective_at_samples: 8, .. })
    ) if *section_id == fixture.first_section));
    assert_eq!(fixture.engine.pending_retirements.iter().filter(|event| matches!(event, EngineEvent::RetiredAudioClip(_))).count(), 32);
    assert!(fixture.removed_audio.iter().all(|owner| owner.strong_count() == 1));
    assert_eq!(fixture.first_audio.strong_count(), 1);
    assert_eq!(middle_audio.strong_count(), 1);
    assert_eq!(fixture.arrange_audio.strong_count(), 1);
    for position in 0..1024 {
        assert!(matches!(fixture.events.pop(), Ok(EngineEvent::PlaybackPosition(actual)) if actual == position));
    }
    let mut delivered = Vec::new();
    for _ in 0..8 {
        assert_eq!(crate::retirement::tests::allocations(|| {
            fixture.engine.process_block(AudioProcessBlock::new(&mut [], 1));
        }), (0, 0));
        while let Ok(event) = fixture.events.pop() { delivered.push(event); }
    }
    let returned: Vec<_> = delivered.iter().filter_map(|event| match event {
        EngineEvent::SectionQueueCancelled { retired } => Some(std::ptr::from_ref(retired.as_ref()) as usize), _ => None,
    }).collect();
    assert_eq!(returned.iter().filter(|pointer| **pointer == middle_pointer).count(), 1);
    assert_eq!(returned.into_iter().filter(|pointer| *pointer != middle_pointer).collect::<Vec<_>>(), fixture.acknowledgements);
    for owner in &fixture.removed_audio {
        assert_eq!(delivered.iter().filter(|event| matches!(event, EngineEvent::RetiredAudioClip(clip)
            if Arc::as_ptr(&clip.audio) == owner.as_ptr())).count(), 1);
    }
    let capture_stops: Vec<_> = delivered.iter().filter_map(|event| match event {
        EngineEvent::PerformanceCaptureStopped { effective_at_samples } => Some(*effective_at_samples), _ => None,
    }).collect();
    assert_eq!(capture_stops, [168, 1]);
    assert_eq!(delivered.iter().filter(|event| matches!(event, EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent {
        section_id, effective_at_samples: 8, section_position_samples: 8, started: true, retired: None, ..
    }) if *section_id == fixture.first_section)).count(), 1);
    // Natural completion and its overflow fault describe the same stopped state.
    assert_eq!(delivered.iter().filter(|event| matches!(event, EngineEvent::PlaybackStopped)).count(), 1);
    assert_eq!(delivered.iter().filter(|event| matches!(event, EngineEvent::CompensationInvalid {
        track_id, effect_id: None, reason: "Audio presentation event history is full"
    } if *track_id == TrackId::MASTER)).count(), 1);
    assert!(fixture.engine.pending_retirements.is_empty());
    assert!(fixture.engine.scheduled_presentation.is_empty());
    assert_eq!(fixture.engine.presentation_state.terminals.occupied_slots(), 0);
    drop(delivered);
    assert!(fixture.removed_audio.iter().all(|owner| owner.strong_count() == 0));
    assert_eq!(fixture.first_audio.strong_count(), 0);
    assert_eq!(middle_audio.strong_count(), 1);
    assert_eq!(fixture.arrange_audio.strong_count(), 1);
    assert_eq!(offsets.strong_count(), 1);
    drop(fixture.engine);
    assert_eq!(middle_audio.strong_count(), 0);
    assert_eq!(fixture.arrange_audio.strong_count(), 0);
    assert!(length <= 32, "ordinary grew to{length}; heap={heap:?}");
    assert_eq!(capacity, 32);
    assert_eq!(heap, (0, 0));
}
