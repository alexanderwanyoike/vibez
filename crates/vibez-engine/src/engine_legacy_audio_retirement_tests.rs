//! Legacy buffer edits preserve playback while returning unique audio owners off callback.

use super::*;

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

#[test]
fn replacing_unique_legacy_audio_preserves_playing_cursor_and_returns_old_owner_off_callback() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let old = stereo_ramp(256, 0.125, -0.25, 1000);
    let old_owner = Arc::downgrade(&old);
    commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
    commands.push(EngineCommand::LoadAudio(old)).unwrap();
    commands.push(EngineCommand::Seek(17)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    for (frame, &actual) in output.as_chunks::<2>().0.iter().enumerate() {
        let position = (frame + 17) as f32;
        assert_eq!(
            actual,
            [0.125 + position / 1024.0, -0.25 + position / 2048.0]
        );
    }
    assert_eq!(engine.transport().position(), 25);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().audio_length(), Some(256));
    assert_eq!(
        old_owner.strong_count(),
        1,
        "resident legacy buffer is uniquely owned"
    );
    while events.pop().is_ok() {}

    let replacement = stereo_ramp(512, 0.5, -0.75, 2000);
    let replacement_pointer = Arc::as_ptr(&replacement);
    commands
        .push(EngineCommand::LoadAudio(replacement))
        .unwrap();
    let measured = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
    });
    for (frame, &actual) in output.as_chunks::<2>().0.iter().enumerate() {
        let position = (frame + 25) as f32;
        assert_eq!(actual, [0.5 + position / 1024.0, -0.75 + position / 2048.0]);
    }
    assert_eq!(engine.audio().map(Arc::as_ptr), Some(replacement_pointer));
    assert_eq!(engine.transport().position(), 33);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().audio_length(), Some(512));
    assert_eq!(
        engine.sample_rate(),
        1000,
        "loading media does not adopt its sample rate"
    );
    assert_eq!(
        measured,
        (0, 0),
        "replacement cannot destroy its displaced unique owner"
    );
    assert_eq!(
        old_owner.strong_count(),
        1,
        "old owner survives until ordinary UI receipt"
    );
    while events.pop().is_ok() {}
    assert_eq!(
        old_owner.strong_count(),
        0,
        "owner-thread event consumption releases old audio"
    );
}

#[test]
fn unloading_unique_legacy_audio_preserves_cursor_and_stop_notifications_without_callback_destruction(
) {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let old = stereo_ramp(256, 0.125, -0.25, 1000);
    let old_owner = Arc::downgrade(&old);
    commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
    commands.push(EngineCommand::LoadAudio(old)).unwrap();
    commands.push(EngineCommand::Seek(23)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    for (frame, &actual) in output.as_chunks::<2>().0.iter().enumerate() {
        let position = (frame + 23) as f32;
        assert_eq!(
            actual,
            [0.125 + position / 1024.0, -0.25 + position / 2048.0]
        );
    }
    assert_eq!(engine.transport().position(), 31);
    assert_eq!(
        old_owner.strong_count(),
        1,
        "Unload receives a uniquely owned resident buffer"
    );
    while events.pop().is_ok() {}
    let expected_capture_boundary = engine.heard_capture_position();
    commands.push(EngineCommand::UnloadAudio).unwrap();
    let measured = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
    });
    assert_eq!(output, [0.0; 16]);
    assert!(engine.audio().is_none());
    assert!(!engine.transport().is_playing());
    assert_eq!(engine.transport().position(), 31, "Unload does not seek");
    assert_eq!(engine.transport().audio_length(), None);
    assert_eq!(engine.sample_rate(), 1000);
    let notifications: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert_eq!(
        notifications
            .iter()
            .filter(|event| matches!(event, EngineEvent::PlaybackStopped))
            .count(),
        1
    );
    let capture_stops: Vec<_> = notifications
        .iter()
        .filter_map(|event| match event {
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => Some(*effective_at_samples),
            _ => None,
        })
        .collect();
    assert_eq!(
        capture_stops,
        [expected_capture_boundary],
        "existing heard Capture closure remains exact"
    );
    assert_eq!(
        measured,
        (0, 0),
        "Unload must return its unique legacy owner instead of destroying it"
    );
    let retired: Vec<_> = notifications
        .iter()
        .filter_map(|event| match event {
            EngineEvent::RetiredAudioBuffer(audio) => Some(audio),
            _ => None,
        })
        .collect();
    assert_eq!(retired.len(), 1);
    assert_eq!(Arc::as_ptr(retired[0]), old_owner.as_ptr());
    assert_eq!(old_owner.strong_count(), 1);
    drop(retired);
    drop(notifications);
    assert_eq!(
        old_owner.strong_count(),
        0,
        "ordinary owner-thread receipt releases old audio"
    );
}

fn assert_ramp_block(output: &[f32], position: usize, left: f32, right: f32) {
    for (frame, &actual) in output.as_chunks::<2>().0.iter().enumerate() {
        let position = (position + frame) as f32;
        assert_eq!(
            actual,
            [left + position / 1024.0, right + position / 2048.0]
        );
    }
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

fn returned_legacy_audio(events: &mut rtrb::Consumer<EngineEvent>) -> Vec<Arc<DecodedAudio>> {
    std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::RetiredAudioBuffer(audio) => Some(audio),
            _ => None,
        })
        .collect()
}

struct LegacyPressureFixture {
    engine: AudioEngine,
    commands: rtrb::Producer<EngineCommand>,
    events: rtrb::Consumer<EngineEvent>,
    owners: Vec<std::sync::Weak<DecodedAudio>>,
    ring_capacity: usize,
}

fn legacy_pressure_fixture() -> LegacyPressureFixture {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let original = stereo_ramp(512, 0.0, 0.0, 1000);
    let owners = vec![Arc::downgrade(&original)];
    commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
    commands.push(EngineCommand::LoadAudio(original)).unwrap();
    commands.push(EngineCommand::Seek(17)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [0.0; 16], 2));
    while events.pop().is_ok() {}
    let ring_capacity = fill_ui_ring(&mut engine);
    LegacyPressureFixture { engine, commands, events, owners, ring_capacity }
}

fn queue_legacy_replacements(
    commands: &mut rtrb::Producer<EngineCommand>,
    owners: &mut Vec<std::sync::Weak<DecodedAudio>>,
    count: usize,
) {
    for generation in 1..=count {
        let audio = stereo_ramp(
            512,
            generation as f32 / 128.0,
            -(generation as f32) / 256.0,
            1000,
        );
        owners.push(Arc::downgrade(&audio));
        commands.push(EngineCommand::LoadAudio(audio)).unwrap();
    }
}

#[test]
fn legacy_load_pressure_retains_thirty_two_owners_and_blocks_next_replacement_in_fifo_order() {
    let LegacyPressureFixture { mut engine, mut commands, mut events, mut owners, ring_capacity } = legacy_pressure_fixture();
    let mut output = [0.0; 16];
    let retirement_capacity = engine.pending_retirements.capacity();
    assert_eq!(retirement_capacity, 32);
    queue_legacy_replacements(&mut commands, &mut owners, 33);
    commands.push(EngineCommand::SetBpm(137.0)).unwrap();
    for position in [25, 33] {
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut output, 2))
            ),
            (0, 0)
        );
        assert_ramp_block(&output, position, 32.0 / 128.0, -32.0 / 256.0);
        assert_eq!(engine.pending_retirements.len(), 32);
        assert_eq!(engine.pending_retirements.capacity(), retirement_capacity);
        assert_eq!(engine.audio().map(Arc::as_ptr), Some(owners[32].as_ptr()));
        let EngineCommand::LoadAudio(incoming) = engine.cmd_rx.peek().unwrap() else {
            panic!("33rd replacement must remain queued")
        };
        assert_eq!(Arc::as_ptr(incoming), owners[33].as_ptr());
        assert_eq!(engine.transport().position(), (position + 8) as u64);
        assert_eq!(engine.transport().audio_length(), Some(512));
        assert!(engine.transport().is_playing());
        assert_ne!(engine.transport().bpm(), 137.0);
        assert!(owners.iter().all(|owner| owner.strong_count() == 1));
    }
    drain_full_ui_ring(&mut events, ring_capacity);
    assert_eq!(
        crate::retirement::tests::allocations(
            || engine.process_block(AudioProcessBlock::new(&mut output, 2))
        ),
        (0, 0)
    );
    assert_ramp_block(&output, 41, 33.0 / 128.0, -33.0 / 256.0);
    assert_eq!(engine.audio().map(Arc::as_ptr), Some(owners[33].as_ptr()));
    assert_eq!(engine.transport().position(), 49);
    assert_eq!(engine.transport().bpm(), 137.0);
    assert!(engine.cmd_rx.peek().is_err());
    assert!(engine.pending_retirements.is_empty());
    let returned = returned_legacy_audio(&mut events);
    assert_eq!(returned.len(), 33);
    for owner in &owners[..33] {
        assert_eq!(
            returned
                .iter()
                .filter(|audio| std::ptr::eq(Arc::as_ptr(audio), owner.as_ptr()))
                .count(),
            1
        );
    }
    drop(returned);
    assert!(owners[..33].iter().all(|owner| owner.strong_count() == 0));
    assert_eq!(
        owners[33].strong_count(),
        1,
        "only admitted final replacement remains resident"
    );
}

#[test]
fn legacy_unload_pressure_requires_two_free_credits_before_clearing_audio_and_stopping() {
    for free_credits in [1, 2] {
        let LegacyPressureFixture { mut engine, mut commands, mut events, mut owners, ring_capacity } = legacy_pressure_fixture();
        let mut output = [0.0; 16];
        let retirement_capacity = engine.pending_retirements.capacity();
        let replacements = retirement_capacity - free_credits;
        queue_legacy_replacements(&mut commands, &mut owners, replacements);
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut [], 2))
            ),
            (0, 0)
        );
        assert_eq!(engine.pending_retirements.len(), replacements);
        assert_eq!(
            engine.pending_retirements.capacity() - engine.pending_retirements.len(),
            free_credits
        );
        assert_eq!(
            engine.audio().map(Arc::as_ptr),
            Some(owners[replacements].as_ptr())
        );
        commands.push(EngineCommand::UnloadAudio).unwrap();
        commands.push(EngineCommand::SetBpm(137.0)).unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut output, 2))
            ),
            (0, 0)
        );
        let stop_position = if free_credits == 1 {
            assert_ramp_block(
                &output,
                25,
                replacements as f32 / 128.0,
                -(replacements as f32) / 256.0,
            );
            assert!(matches!(
                engine.cmd_rx.peek(),
                Ok(EngineCommand::UnloadAudio)
            ));
            assert_eq!(
                engine.audio().map(Arc::as_ptr),
                Some(owners[replacements].as_ptr())
            );
            assert!(engine.transport().is_playing());
            assert_eq!(engine.transport().audio_length(), Some(512));
            33
        } else {
            assert_eq!(output, [0.0; 16]);
            assert!(
                matches!(engine.cmd_rx.peek(), Ok(EngineCommand::SetBpm(value)) if *value == 137.0)
            );
            assert!(engine.audio().is_none());
            assert!(!engine.transport().is_playing());
            assert_eq!(engine.transport().audio_length(), None);
            25
        };
        assert_eq!(engine.transport().position(), stop_position);
        assert_ne!(engine.transport().bpm(), 137.0);
        assert_eq!(engine.pending_retirements.len(), retirement_capacity - 1);
        assert!(owners.iter().all(|owner| owner.strong_count() == 1));
        drain_full_ui_ring(&mut events, ring_capacity);
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut output, 2))
            ),
            (0, 0)
        );
        assert_eq!(output, [0.0; 16]);
        assert_eq!(engine.transport().position(), stop_position);
        assert_eq!(engine.transport().bpm(), 137.0);
        assert!(engine.audio().is_none());
        assert!(!engine.transport().is_playing());
        assert_eq!(engine.transport().audio_length(), None);
        assert!(engine.cmd_rx.peek().is_err());
        assert!(engine.pending_retirements.is_empty());
        assert_eq!(engine.pending_retirements.capacity(), retirement_capacity);
        let returned = returned_legacy_audio(&mut events);
        assert_eq!(returned.len(), owners.len());
        for owner in &owners {
            assert_eq!(
                returned
                    .iter()
                    .filter(|audio| std::ptr::eq(Arc::as_ptr(audio), owner.as_ptr()))
                    .count(),
                1
            );
        }
        drop(returned);
        assert!(owners.iter().all(|owner| owner.strong_count() == 0));
    }
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
fn legacy_unload_combined_source_capture_and_presentation_pressure_returns_all_unique_owners() {
    assert_combined_legacy_unload_pressure(false);
}

#[test]
fn legacy_unload_minimum_credits_defers_real_delayed_capture_history_without_destroying_owners() {
    assert_combined_legacy_unload_pressure(true);
}

fn assert_combined_legacy_unload_pressure(delayed_minimum_credits: bool) {
    use crate::playback_source::PreparedSectionPlaybackSource;
    use vibez_core::perform::MusicalBoundary;

    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let legacy = stereo_ramp(512, 0.5, -0.25, 8);
    let mut legacy_owner = Arc::downgrade(&legacy);
    let tracks: Vec<_> = (0..40).map(|_| TrackId::new()).collect();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands.push(EngineCommand::LoadAudio(legacy)).unwrap();
    for track in &tracks {
        commands.push(EngineCommand::add_track(*track)).unwrap();
    }
    let mut delayed_sections = Vec::new();
    if delayed_minimum_credits {
        use vibez_core::routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode};
        let effect = vibez_core::id::EffectId::new();
        let slow = tracks[1];
        commands.push(EngineCommand::InsertEffect {
            track_id: slow,
            effect_id: effect,
            effect: Box::new(crate::test_support::DelayProbe::new(521)),
            position: None,
        }).unwrap();
        let mut channels: Vec<_> = tracks.iter().map(|track| RoutingChannel {
            id: *track,
            is_bus: false,
            sends: vec![],
            effects: if *track == slow { vec![RoutingEffect {
                id: effect, inputs: vec![], assignments: vec![], inactive_inputs: vec![],
            }] } else { vec![] },
        }).collect();
        channels.push(RoutingChannel { id: TrackId::MASTER, is_bus: true, effects: vec![], sends: vec![] });
        let reports = [(RoutingNode { channel: slow, stage: NodeStage::Effect(effect) }, 521)];
        commands.push(EngineCommand::SetRouting(crate::routing::PreparedRouting::prepare_compensated(
            &channels, 64, &reports, &[], 1,
        ).unwrap())).unwrap();
        let section = SectionId::new();
        for _ in 0..2 {
            let prepared = Box::new(PreparedSectionPlaybackSource::new(section, 8.0, true, vec![]));
            delayed_sections.push(&*prepared as *const PreparedSectionPlaybackSource);
            commands.push(EngineCommand::LaunchSection(prepared)).unwrap();
        }
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
    assert!(engine.capture_active);
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
        assert_eq!(engine.section_capture_timing.len(), 2, "two public launches retain real early/full timing history");
        let offsets = engine.capture_offsets();
        assert!(!offsets.is_empty());
        Some((engine.section_capture_timing.len(), engine.section_capture_timing.capacity(), Arc::downgrade(&offsets)))
    } else {
        assert!(engine.scheduled_presentation.is_empty());
        None
    };
    let ring_capacity = fill_ui_ring(&mut engine);
    let presentation_capacity = engine.scheduled_presentation.capacity();
    let retirement_capacity = engine.pending_retirements.capacity();
    let mut displaced_legacy = Vec::new();
    if delayed_minimum_credits {
        displaced_legacy.push(legacy_owner.clone());
        queue_legacy_replacements(&mut commands, &mut displaced_legacy, retirement_capacity - 2);
        legacy_owner = displaced_legacy.pop().unwrap();
        assert_eq!(crate::retirement::tests::allocations(|| engine.process_block(AudioProcessBlock::new(&mut [], 1))), (0, 0));
        assert_eq!(engine.pending_retirements.len(), retirement_capacity - 2);
        assert_eq!(engine.pending_retirements.capacity() - engine.pending_retirements.len(), 2);
        assert_eq!(engine.section_capture_timing.len(), timing_history.as_ref().unwrap().0);
        assert_eq!(engine.audio().map(Arc::as_ptr), Some(legacy_owner.as_ptr()));
    }
    let retained_owners = presentation_capacity - engine.scheduled_presentation.len() - presentation_queue::COMMAND_PRESENTATION_RESERVE;
    // Existing owner-bearing acknowledgements cannot be discarded to make cleanup room.
    for index in 0..retained_owners {
        let (prepared, owner) = unique_clip_request(tracks[0], 10000 + index as u64);
        expected_clips.push(owner);
        engine.present_event(EngineEvent::ClipRequestRetired(prepared));
    }
    assert_eq!(engine.scheduled_presentation.len(), presentation_capacity - presentation_queue::COMMAND_PRESENTATION_RESERVE);
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
        assert!(engine.audio().is_none());
        assert!(!engine.transport().is_playing());
        assert_eq!(engine.transport().position(), arrange_cursor);
        assert_eq!(engine.transport().audio_length(), None);
        assert!(!engine.capture_active);
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
            assert_eq!(engine.section_capture_timing.len(), history_length - 1, "one history owner remains deferred after legacy retirement consumes its credit");
            assert_eq!(engine.section_capture_timing.capacity(), *history_capacity);
            assert!(displaced_legacy.iter().all(|owner| owner.strong_count() == 1));
        }
        assert_eq!(legacy_owner.strong_count(), 1);
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
            assert_eq!(delivered.iter().filter(|event| match event {
                EngineEvent::SectionQueueCancelled { retired } => std::ptr::eq(&**retired, *pointer),
                _ => false,
            }).count(), 1, "both public delayed Section acknowledgements return exactly once");
        }
        for owner in &displaced_legacy {
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
            EngineEvent::SectionRecordStopped {
                section_id: id,
                started: false,
                retired: Some(prepared),
                ..
            } if *id == section_id => Some(prepared),
            _ => None,
        })
        .collect();
    assert_eq!(stopped_recording.len(), 1);
    assert!(std::ptr::eq(&**stopped_recording[0], recording_pointer));
    assert_eq!(delivered.iter().filter(|event| matches!(event, EngineEvent::RetiredAudioBuffer(audio) if std::ptr::eq(Arc::as_ptr(audio), legacy_owner.as_ptr()))).count(), 1);
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
            | EngineEvent::SectionTransitioned { .. }
            | EngineEvent::SectionRecordStarted { .. }
            | EngineEvent::PerformanceCaptureStarted { .. }
    )));
    drop(stopped_recording);
    drop(delivered);
    assert_eq!(legacy_owner.strong_count(), 0);
    assert!(displaced_legacy.iter().all(|owner| owner.strong_count() == 0));
    if let Some((_, _, offsets)) = timing_history {
        assert_eq!(offsets.strong_count(), 1, "only the current routing plan retains its capture offsets");
    }
    assert_eq!(recording_audio.audio.strong_count(), 0);
    assert!(expected_clips
        .iter()
        .all(|owner| owner.audio.strong_count() == 0));
}

#[derive(Clone, Copy, Debug)]
enum LegacyOwnership {
    Empty,
    Unique,
    Shared,
    SameAllocation,
}

#[test]
fn legacy_owner_state_and_format_matrix_preserves_existing_render_and_transport_rules() {
    struct Case {
        name: &'static str,
        ownership: LegacyOwnership,
        channels: usize,
        frames: usize,
        playing: bool,
    }
    let cases = [
        Case {
            name: "initial empty",
            ownership: LegacyOwnership::Empty,
            channels: 2,
            frames: 32,
            playing: true,
        },
        Case {
            name: "unique stopped",
            ownership: LegacyOwnership::Unique,
            channels: 2,
            frames: 32,
            playing: false,
        },
        Case {
            name: "shared playing",
            ownership: LegacyOwnership::Shared,
            channels: 2,
            frames: 32,
            playing: true,
        },
        Case {
            name: "same allocation playing",
            ownership: LegacyOwnership::SameAllocation,
            channels: 2,
            frames: 128,
            playing: true,
        },
        Case {
            name: "mono fallback",
            ownership: LegacyOwnership::Unique,
            channels: 1,
            frames: 32,
            playing: true,
        },
        Case {
            name: "stereo fallback",
            ownership: LegacyOwnership::Unique,
            channels: 2,
            frames: 32,
            playing: true,
        },
        Case {
            name: "zero channels",
            ownership: LegacyOwnership::Unique,
            channels: 0,
            frames: 0,
            playing: true,
        },
        Case {
            name: "zero frames",
            ownership: LegacyOwnership::Unique,
            channels: 2,
            frames: 0,
            playing: true,
        },
        Case {
            name: "shorter than cursor",
            ownership: LegacyOwnership::Unique,
            channels: 2,
            frames: 3,
            playing: true,
        },
        Case {
            name: "ends within callback",
            ownership: LegacyOwnership::Unique,
            channels: 2,
            frames: 7,
            playing: true,
        },
    ];
    for case in cases {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let old = Arc::new(DecodedAudio {
            channels: vec![vec![0.125; 128]; 2],
            sample_rate: 1000,
        });
        let old_owner = Arc::downgrade(&old);
        let external = match case.ownership {
            LegacyOwnership::Shared | LegacyOwnership::SameAllocation => Some(old.clone()),
            _ => None,
        };
        if matches!(case.ownership, LegacyOwnership::Empty) {
            drop(old);
        } else {
            commands.push(EngineCommand::LoadAudio(old)).unwrap();
        }
        commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
        commands.push(EngineCommand::Seek(5)).unwrap();
        engine.process_block(AudioProcessBlock::new(&mut [0.0; 3], 3));
        while events.pop().is_ok() {}
        let incoming = if matches!(case.ownership, LegacyOwnership::SameAllocation) {
            external.as_ref().unwrap().clone()
        } else {
            Arc::new(DecodedAudio {
                channels: (0..case.channels)
                    .map(|channel| vec![0.25 + channel as f32 / 8.0; case.frames])
                    .collect(),
                sample_rate: 2000,
            })
        };
        let incoming_owner = Arc::downgrade(&incoming);
        commands.push(EngineCommand::LoadAudio(incoming)).unwrap();
        if case.playing {
            commands.push(EngineCommand::Play).unwrap();
        }
        let mut output = [9.0; 12];
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut output, 3))
            ),
            (0, 0),
            "{}",
            case.name
        );
        for (frame, samples) in output.as_chunks::<3>().0.iter().enumerate() {
            for (channel, sample) in samples.iter().enumerate() {
                let expected = if !case.playing || frame + 5 >= case.frames || case.channels == 0 {
                    0.0
                } else if matches!(case.ownership, LegacyOwnership::SameAllocation) {
                    0.125
                } else {
                    0.25 + channel.min(case.channels - 1) as f32 / 8.0
                };
                assert_eq!(
                    *sample, expected,
                    "{} frame {frame} channel {channel}",
                    case.name
                );
            }
        }
        assert_eq!(
            engine.audio().map(Arc::as_ptr),
            Some(incoming_owner.as_ptr()),
            "{}",
            case.name
        );
        assert_eq!(engine.sample_rate(), 1000, "{}", case.name);
        assert_eq!(
            engine.transport().audio_length(),
            Some(case.frames as u64),
            "{}",
            case.name
        );
        let expected_position = if case.playing {
            9.min(case.frames as u64)
        } else {
            5
        };
        assert_eq!(
            engine.transport().position(),
            expected_position,
            "{}",
            case.name
        );
        assert_eq!(
            engine.transport().is_playing(),
            case.playing && case.frames > 9,
            "{}",
            case.name
        );
        let retired = returned_legacy_audio(&mut events);
        assert_eq!(
            retired.len(),
            usize::from(!matches!(case.ownership, LegacyOwnership::Empty)),
            "{}",
            case.name
        );
        if let Some(audio) = retired.first() {
            assert_eq!(Arc::as_ptr(audio), old_owner.as_ptr(), "{}", case.name);
        }
        drop(retired);
        assert_eq!(
            old_owner.strong_count(),
            match case.ownership {
                LegacyOwnership::Empty | LegacyOwnership::Unique => 0,
                LegacyOwnership::Shared => 1,
                LegacyOwnership::SameAllocation => 2,
            },
            "{}",
            case.name
        );
        commands.push(EngineCommand::UnloadAudio).unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut output, 3))
            ),
            (0, 0),
            "{} unload",
            case.name
        );
        assert_eq!(output, [0.0; 12], "{} unload", case.name);
        assert_eq!(
            engine.transport().position(),
            expected_position,
            "{} unload",
            case.name
        );
        let retired = returned_legacy_audio(&mut events);
        assert_eq!(retired.len(), 1, "{} unload", case.name);
        assert_eq!(Arc::as_ptr(&retired[0]), incoming_owner.as_ptr());
        drop(retired);
        drop(external);
        assert_eq!(old_owner.strong_count(), 0, "{}", case.name);
        assert_eq!(incoming_owner.strong_count(), 0, "{}", case.name);
        commands.push(EngineCommand::UnloadAudio).unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut output, 3))
            ),
            (0, 0),
            "{} repeated unload",
            case.name
        );
        assert!(returned_legacy_audio(&mut events).is_empty());
        assert_eq!(engine.transport().position(), expected_position);
        assert_eq!(engine.transport().audio_length(), None);
        assert!(!engine.transport().is_playing());
    }
}

#[test]
fn legacy_replacement_remains_inaudible_with_live_tracks_or_a_prepared_graph() {
    for graph in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        if graph {
            let master = vibez_core::routing::RoutingChannel {
                id: TrackId::MASTER,
                is_bus: false,
                effects: vec![],
                sends: vec![],
            };
            commands
                .push(EngineCommand::SetRouting(
                    crate::routing::PreparedRouting::prepare(&[master], 8).unwrap(),
                ))
                .unwrap();
        } else {
            commands
                .push(EngineCommand::add_track(TrackId::new()))
                .unwrap();
        }
        let old = stereo_ramp(128, 0.5, -0.5, 1000);
        let old_owner = Arc::downgrade(&old);
        commands.push(EngineCommand::LoadAudio(old)).unwrap();
        commands.push(EngineCommand::Seek(5)).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        let mut output = [0.0; 8];
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
        assert_eq!(output, [0.0; 8]);
        while events.pop().is_ok() {}
        let incoming = stereo_ramp(256, 0.75, -0.75, 2000);
        let incoming_pointer = Arc::as_ptr(&incoming);
        commands.push(EngineCommand::LoadAudio(incoming)).unwrap();
        let position = engine.transport().position();
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut output, 2))
            ),
            (0, 0)
        );
        assert_eq!(output, [0.0; 8], "legacy cannot bypass graph={graph}");
        assert_eq!(engine.audio().map(Arc::as_ptr), Some(incoming_pointer));
        assert_eq!(engine.transport().position(), position + 4);
        assert!(engine.transport().is_playing());
        assert_eq!(engine.transport().audio_length(), Some(256));
        let retired = returned_legacy_audio(&mut events);
        assert_eq!(retired.len(), 1);
        assert_eq!(Arc::as_ptr(&retired[0]), old_owner.as_ptr());
        drop(retired);
        assert_eq!(old_owner.strong_count(), 0);
    }
}

#[test]
fn legacy_audio_owners_are_released_when_engine_and_undrained_queues_teardown_in_either_order() {
    for engine_first in [false, true] {
        let (mut engine, mut commands, events) = AudioEngine::new();
        let retired = stereo_ramp(128, 0.125, -0.25, 1000);
        let resident = stereo_ramp(128, 0.25, -0.5, 1000);
        let queued = stereo_ramp(128, 0.5, -0.75, 1000);
        let owners = [
            Arc::downgrade(&retired),
            Arc::downgrade(&resident),
            Arc::downgrade(&queued),
        ];
        commands.push(EngineCommand::LoadAudio(retired)).unwrap();
        commands.push(EngineCommand::LoadAudio(resident)).unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(
                || engine.process_block(AudioProcessBlock::new(&mut [0.0; 2], 2))
            ),
            (0, 0)
        );
        commands.push(EngineCommand::LoadAudio(queued)).unwrap();
        assert!(owners.iter().all(|owner| owner.strong_count() == 1));
        if engine_first {
            drop(engine);
            assert_eq!(
                owners[0].strong_count(),
                1,
                "undrained UI event owns its retired audio"
            );
            assert_eq!(
                owners[1].strong_count(),
                0,
                "resident owner ends with the engine"
            );
            assert_eq!(
                owners[2].strong_count(),
                1,
                "command producer retains unconsumed payload"
            );
            drop(events);
            drop(commands);
        } else {
            drop(events);
            drop(commands);
            assert!(
                owners.iter().all(|owner| owner.strong_count() == 1),
                "engine-side endpoints retain undrained payloads"
            );
            drop(engine);
        }
        assert!(owners.iter().all(|owner| owner.strong_count() == 0));
    }
}

#[test]
fn legacy_load_and_unload_preserve_loop_region_and_existing_short_buffer_playback() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let old = stereo_ramp(32, 0.125, -0.25, 1000);
    let old_owner = Arc::downgrade(&old);
    commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
    commands.push(EngineCommand::LoadAudio(old)).unwrap();
    commands.push(EngineCommand::SetArrangementLoopRegion { start: 2, end: 8 }).unwrap();
    commands.push(EngineCommand::SetArrangementLoop(true)).unwrap();
    commands.push(EngineCommand::Seek(6)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 8];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    assert_ramp_block(&output, 6, 0.125, -0.25);
    assert_eq!(engine.transport().position(), 4);
    while events.pop().is_ok() {}
    assert_eq!(old_owner.strong_count(), 1);

    let incoming = stereo_ramp(3, 0.5, -0.75, 2000);
    let incoming_owner = Arc::downgrade(&incoming);
    commands.push(EngineCommand::LoadAudio(incoming)).unwrap();
    assert_eq!(crate::retirement::tests::allocations(|| engine.process_block(AudioProcessBlock::new(&mut output, 2))), (0, 0));
    assert_eq!(output, [0.0; 8], "legacy reads beyond its shorter media remain silent");
    assert_eq!(engine.audio().map(Arc::as_ptr), Some(incoming_owner.as_ptr()));
    assert_eq!(engine.transport().audio_length(), Some(3));
    assert_eq!(engine.transport().position(), 2);
    assert!(engine.transport().is_playing(), "loop wrapping retains priority over content-end auto-stop");
    assert_eq!(engine.transport().active_loop_region(), Some((2, 8)));
    assert_eq!(engine.sample_rate(), 1000);
    let retired = returned_legacy_audio(&mut events);
    assert_eq!(retired.len(), 1);
    assert_eq!(Arc::as_ptr(&retired[0]), old_owner.as_ptr());
    drop(retired);
    assert_eq!(old_owner.strong_count(), 0);

    assert_eq!(crate::retirement::tests::allocations(|| engine.process_block(AudioProcessBlock::new(&mut output, 2))), (0, 0));
    assert_eq!(&output[..2], &[0.5 + 2.0 / 1024.0, -0.75 + 2.0 / 2048.0]);
    assert_eq!(&output[2..], &[0.0; 6]);
    assert_eq!(engine.transport().position(), 3);
    assert!(!engine.transport().is_playing(), "a callback that does not reach the loop end retains ordinary media-end auto-stop");
    assert!(returned_legacy_audio(&mut events).is_empty());
    commands.push(EngineCommand::UnloadAudio).unwrap();
    assert_eq!(crate::retirement::tests::allocations(|| engine.process_block(AudioProcessBlock::new(&mut output, 2))), (0, 0));
    assert_eq!(output, [0.0; 8]);
    assert_eq!(engine.transport().position(), 3);
    assert_eq!(engine.transport().audio_length(), None);
    assert!(!engine.transport().is_playing());
    assert_eq!(engine.transport().active_loop_region(), Some((2, 8)));
    let retired = returned_legacy_audio(&mut events);
    assert_eq!(retired.len(), 1);
    assert_eq!(Arc::as_ptr(&retired[0]), incoming_owner.as_ptr());
    drop(retired);
    assert_eq!(incoming_owner.strong_count(), 0);
}
