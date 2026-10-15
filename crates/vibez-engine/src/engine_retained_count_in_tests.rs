//! Prepared count-in sources survive stopped-record backpressure, recovery and teardown.

use super::*;
use crate::events::SectionRecordStoppedEvent;
use crate::playback_source::{EngineClip, PreparedPlaybackSource, PreparedSectionPlaybackSource};
use graph_test_support::{graph_engine, routing_channel};
use std::sync::Weak;
use vibez_core::id::{ClipId, SectionId};

fn box_clip(id: ClipId, value: f32) -> EngineClip {
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

fn box_source(
    section: SectionId,
    track: TrackId,
    value: f32,
) -> (Box<PreparedSectionPlaybackSource>, Weak<DecodedAudio>) {
    let clip = box_clip(ClipId::new(), value);
    let weak = Arc::downgrade(&clip.audio);
    (
        Box::new(PreparedSectionPlaybackSource::new(
            section,
            1.0,
            false,
            vec![(
                track,
                PreparedPlaybackSource::new(vec![clip], vec![], vec![]),
            )],
        )),
        weak,
    )
}

struct PendingBoxFixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    track: TrackId,
    old_section: SectionId,
    future_section: SectionId,
    future_pointer: usize,
    middle_pointer: usize,
    future_audio: Weak<DecodedAudio>,
    old_audio: Weak<DecodedAudio>,
    middle_audio: Weak<DecodedAudio>,
    arrange_audio: Weak<DecodedAudio>,
    removed_audio: Vec<Weak<DecodedAudio>>,
    acknowledgements: Vec<usize>,
}

fn pending_box_fixture() -> PendingBoxFixture {
    let track = TrackId::new();
    let old_section = SectionId::new();
    let middle_section = SectionId::new();
    let future_section = SectionId::new();
    let removed_id = ClipId::new();
    let mut clips: Vec<_> = (0..32).map(|_| box_clip(removed_id, 0.0)).collect();
    let removed_audio: Vec<_> = clips
        .iter()
        .map(|clip| Arc::downgrade(&clip.audio))
        .collect();
    let resident = box_clip(ClipId::new(), 0.25);
    let arrange_audio = Arc::downgrade(&resident.audio);
    clips.push(resident);
    let (old, old_audio) = box_source(old_section, track, 0.25);
    let (middle, middle_audio) = box_source(middle_section, track, 0.5);
    let middle_pointer = std::ptr::from_ref(middle.as_ref()) as usize;
    let (future, future_audio) = box_source(future_section, track, 0.75);
    let future_pointer = std::ptr::from_ref(future.as_ref()) as usize;
    let (mut engine, mut commands, mut events) =
        graph_engine(&[routing_channel(track), routing_channel(TrackId::MASTER)]);
    for command in [
        EngineCommand::SetSampleRate(8),
        EngineCommand::SetBpm(60.0),
        EngineCommand::Seek(40),
        EngineCommand::AddTrack(Box::new(EngineTrack::with_playback_source(
            track,
            PreparedPlaybackSource::new(clips, vec![], vec![]),
        ))),
        EngineCommand::SetTrackPan(track, 0.0),
        EngineCommand::ArmSectionRecord {
            section_id: old_section,
            track_id: track,
            prepared: Some(old),
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
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event(EngineEvent::SectionQueueCancelled { retired: owner }, &mut retirement_allowance)
        };
    }
    for _ in 0..40 {
        {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event_at(EngineEvent::PlaybackStarted, engine.output_position + 5000, &mut retirement_allowance)
        };
    }
    let mut old_end = [0.0; 7];
    assert_eq!(
        crate::retirement::tests::allocations(
            || engine.process_block(AudioProcessBlock::new(&mut old_end, 1))
        ),
        (0, 0)
    );
    assert_eq!(old_end, [0.25; 7]);
    assert_eq!(engine.scheduled_presentation.len(), 2009);
    assert!(
        matches!(engine.presentation_state.terminals.overflow_owner(), Some(
        EngineEvent::SectionRecordStopped(SectionRecordStoppedEvent { section_id, effective_at_samples: 8, started: true, retired: None, .. })
    ) if *section_id == old_section)
    );
    commands.push(EngineCommand::Play).unwrap();
    let mut resumed = [0.0; 128];
    assert_eq!(
        crate::retirement::tests::allocations(
            || engine.process_block(AudioProcessBlock::new(&mut resumed, 1))
        ),
        (0, 0)
    );
    assert_eq!(resumed, [0.25; 128]);
    assert!(!engine.presentation_state.is_faulted());
    assert_eq!(engine.transport().position(), 168);
    assert_eq!(engine.output_position_samples(), 136);
    assert_eq!(engine.scheduled_presentation.len(), 2010);
    commands.push(EngineCommand::LaunchSection(middle)).unwrap();
    let mut middle_first = [0.0];
    engine.process_block(AudioProcessBlock::new(&mut middle_first, 1));
    assert_eq!(middle_first, [0.5]);
    assert_eq!(engine.active_section.unwrap().section_id, middle_section);
    assert_eq!(engine.active_section.unwrap().position_samples, 1);
    assert!(engine.active_section_record.is_none());
    assert_eq!(engine.scheduled_presentation.len(), 2013);
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id: future_section,
            track_id: track,
            prepared: Some(future),
            count_in_bars: 1,
            replace_existing: false,
        })
        .unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [], 1));
    let pending = engine.pending_section_record.as_ref().unwrap();
    assert_eq!(
        (
            pending.effective_at_samples,
            pending.count_in_start_samples,
            pending.count_in_beat_samples
        ),
        (33, 1, 8)
    );
    assert_eq!(
        std::ptr::from_ref(pending.prepared.as_ref().unwrap().as_ref()) as usize,
        future_pointer
    );
    assert_eq!(engine.scheduled_presentation.len(), 2014);
    while engine.scheduled_presentation.len() < 2048 {
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
    assert!(!engine.presentation_state.is_faulted());
    assert!(engine
        .presentation_state
        .terminals
        .overflow_owner()
        .is_some());
    assert_eq!(engine.pending_retirements.len(), 0);
    engine.tracks_mut_for_retirement()[0]
        .playback_source
        .remove_audio_clips(removed_id);
    assert!(engine.tracks()[0].playback_source.has_removed_audio_clips());
    let mut count_in = [0.0; 7];
    assert_eq!(
        crate::retirement::tests::allocations(
            || engine.process_block(AudioProcessBlock::new(&mut count_in, 1))
        ),
        (0, 0)
    );
    assert_eq!(count_in, [0.32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(engine.performance_position, 8);
    assert_eq!(engine.output_position_samples(), 144);
    assert_eq!(engine.transport().position(), 168);
    assert!(engine.active_section.is_none());
    assert!(engine.pending_section_record.is_none());
    assert!(engine.active_section_record.is_none());
    let stop = engine.pending_section_record_stop.as_ref().unwrap();
    assert_eq!(
        (
            stop.section_id,
            stop.track_id,
            stop.effective_at_samples,
            stop.section_position_samples,
            stop.started
        ),
        (future_section, track, 8, 8, false)
    );
    assert_eq!(
        std::ptr::from_ref(&**stop.retired.as_ref().unwrap()) as usize,
        future_pointer
    );
    assert_eq!(future_audio.strong_count(), 1);
    assert_eq!(old_audio.strong_count(), 1);
    assert_eq!(middle_audio.strong_count(), 1);
    assert_eq!(arrange_audio.strong_count(), 1);
    assert!(removed_audio.iter().all(|owner| owner.strong_count() == 1));
    assert_eq!(engine.pending_retirements.len(), 32);
    assert_eq!(engine.pending_retirements.capacity(), 32);
    assert!(engine.event_tx.is_full());
    assert_eq!(engine.scheduled_presentation.len(), 2048);
    assert_eq!(engine.presentation_state.terminals.capture_stop(), Some(1));
    assert_eq!(engine.tracks()[0].pan, 0.0);
    PendingBoxFixture {
        engine,
        commands,
        events,
        track,
        old_section,
        future_section,
        future_pointer,
        middle_pointer,
        future_audio,
        old_audio,
        middle_audio,
        arrange_audio,
        removed_audio,
        acknowledgements,
    }
}

fn queue_next_arm(fixture: &mut PendingBoxFixture) -> (SectionId, usize, Weak<DecodedAudio>) {
    let next = SectionId::new();
    let (prepared, audio) = box_source(next, fixture.track, 0.125);
    let pointer = std::ptr::from_ref(prepared.as_ref()) as usize;
    fixture
        .commands
        .push(EngineCommand::ArmSectionRecord {
            section_id: next,
            track_id: fixture.track,
            prepared: Some(prepared),
            count_in_bars: 1,
            replace_existing: false,
        })
        .unwrap();
    fixture
        .commands
        .push(EngineCommand::SetTrackPan(fixture.track, 0.375))
        .unwrap();
    (next, pointer, audio)
}

#[test]
fn retained_count_in_box_precedes_next_arm_and_fifo_marker_when_ui_recovers() {
    let mut fixture = pending_box_fixture();
    let (next, next_pointer, next_audio) = queue_next_arm(&mut fixture);
    for _ in 0..2 {
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture
                .engine
                .process_block(AudioProcessBlock::new(&mut [], 1))),
            (0, 0)
        );
        assert_eq!(fixture.engine.cmd_rx.slots(), 2);
        assert!(
            matches!(fixture.engine.cmd_rx.peek(), Ok(EngineCommand::ArmSectionRecord { section_id, prepared: Some(prepared), .. })
            if *section_id == next && std::ptr::from_ref(prepared.as_ref()) as usize == next_pointer)
        );
        assert_eq!(fixture.engine.tracks()[0].pan, 0.0);
        assert_eq!(fixture.future_audio.strong_count(), 1);
        assert_eq!(next_audio.strong_count(), 1);
        let stopped = fixture.engine.pending_section_record_stop.as_ref().unwrap();
        assert_eq!(
            (
                stopped.effective_at_samples,
                stopped.section_position_samples
            ),
            (8, 8)
        );
        assert_eq!(
            std::ptr::from_ref(&**stopped.retired.as_ref().unwrap()) as usize,
            fixture.future_pointer
        );
    }
    for position in 0..1024 {
        assert!(
            matches!(fixture.events.pop(), Ok(EngineEvent::PlaybackPosition(actual)) if actual == position)
        );
    }
    let mut delivered = Vec::new();
    for _ in 0..8 {
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture
                .engine
                .process_block(AudioProcessBlock::new(&mut [], 1))),
            (0, 0)
        );
        while let Ok(event) = fixture.events.pop() {
            delivered.push(event);
        }
    }
    assert!(fixture.engine.cmd_rx.peek().is_err());
    assert!(fixture.engine.pending_section_record_stop.is_none());
    assert_eq!(fixture.engine.tracks()[0].pan, 0.375);
    let pending = fixture.engine.pending_section_record.as_ref().unwrap();
    assert_eq!(pending.section_id, next);
    assert_eq!(
        std::ptr::from_ref(pending.prepared.as_ref().unwrap().as_ref()) as usize,
        next_pointer
    );
    let records: Vec<_> = delivered
        .iter()
        .filter_map(|event| match event {
            EngineEvent::SectionRecordStopped(stopped) => Some(stopped),
            _ => None,
        })
        .collect();
    assert_eq!(records.len(), 2);
    assert_eq!(
        (
            records[0].section_id,
            records[0].effective_at_samples,
            records[0].started
        ),
        (fixture.old_section, 8, true)
    );
    assert_eq!(
        (
            records[1].section_id,
            records[1].effective_at_samples,
            records[1].section_position_samples,
            records[1].started
        ),
        (fixture.future_section, 8, 8, false)
    );
    assert_eq!(
        std::ptr::from_ref(&**records[1].retired.as_ref().unwrap()) as usize,
        fixture.future_pointer
    );
    let stop_index = delivered.iter().position(|event| matches!(event, EngineEvent::SectionRecordStopped(stop) if stop.section_id == fixture.future_section)).unwrap();
    let arm_index = delivered.iter().position(|event| matches!(event, EngineEvent::SectionRecordArmed(armed) if armed.section_id == next)).unwrap();
    assert!(stop_index < arm_index);
    let returned: Vec<_> = delivered
        .iter()
        .filter_map(|event| match event {
            EngineEvent::SectionQueueCancelled { retired } => {
                Some(std::ptr::from_ref(retired.as_ref()) as usize)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        returned
            .iter()
            .filter(|pointer| **pointer == fixture.middle_pointer)
            .count(),
        1
    );
    assert_eq!(
        returned
            .into_iter()
            .filter(|pointer| *pointer != fixture.middle_pointer)
            .collect::<Vec<_>>(),
        fixture.acknowledgements
    );
    for owner in &fixture.removed_audio {
        assert_eq!(delivered.iter().filter(|event| matches!(event, EngineEvent::RetiredAudioClip(clip) if Arc::as_ptr(&clip.audio) == owner.as_ptr())).count(), 1);
    }
    assert_eq!(fixture.future_audio.strong_count(), 1);
    assert!(fixture.engine.pending_retirements.is_empty());
    assert_eq!(fixture.engine.pending_retirements.capacity(), 32);
    assert!(fixture.engine.scheduled_presentation.is_empty());
    assert_eq!(
        fixture.engine.presentation_state.terminals.occupied_slots(),
        0
    );
    drop(records);
    drop(delivered);
    assert_eq!(fixture.future_audio.strong_count(), 0);
    assert_eq!(fixture.old_audio.strong_count(), 0);
    assert!(fixture
        .removed_audio
        .iter()
        .all(|owner| owner.strong_count() == 0));
    assert_eq!(next_audio.strong_count(), 1);
    drop(fixture.engine);
    assert_eq!(next_audio.strong_count(), 0);
    assert_eq!(fixture.middle_audio.strong_count(), 0);
    assert_eq!(fixture.arrange_audio.strong_count(), 0);
}

#[test]
fn retained_count_in_box_and_queued_arm_release_in_both_teardown_orders() {
    for engine_first in [false, true] {
        let mut fixture = pending_box_fixture();
        let (_, _, next_audio) = queue_next_arm(&mut fixture);
        if engine_first {
            drop(fixture.engine);
            assert_eq!(fixture.future_audio.strong_count(), 0);
            assert_eq!(fixture.old_audio.strong_count(), 0);
            assert_eq!(fixture.middle_audio.strong_count(), 0);
            assert_eq!(fixture.arrange_audio.strong_count(), 0);
            assert!(fixture
                .removed_audio
                .iter()
                .all(|owner| owner.strong_count() == 0));
            assert_eq!(next_audio.strong_count(), 1);
            drop(fixture.events);
            drop(fixture.commands);
        } else {
            drop(fixture.events);
            drop(fixture.commands);
            assert_eq!(fixture.future_audio.strong_count(), 1);
            assert_eq!(fixture.old_audio.strong_count(), 1);
            assert_eq!(fixture.middle_audio.strong_count(), 1);
            assert_eq!(fixture.arrange_audio.strong_count(), 1);
            assert!(fixture
                .removed_audio
                .iter()
                .all(|owner| owner.strong_count() == 1));
            assert_eq!(next_audio.strong_count(), 1);
            drop(fixture.engine);
        }
        assert_eq!(fixture.future_audio.strong_count(), 0);
        assert_eq!(next_audio.strong_count(), 0);
    }
}
