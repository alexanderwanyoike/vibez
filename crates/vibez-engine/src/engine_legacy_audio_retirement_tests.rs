//! Legacy buffer edits preserve playback while returning unique audio owners off callback.

use super::*;

fn stereo_ramp(frames: usize, left: f32, right: f32, sample_rate: u32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![
            (0..frames).map(|frame| left + frame as f32 / 1024.0).collect(),
            (0..frames).map(|frame| right + frame as f32 / 2048.0).collect(),
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
    for (frame, actual) in output.chunks_exact(2).enumerate() {
        let position = (frame + 17) as f32;
        assert_eq!(actual, [0.125 + position / 1024.0, -0.25 + position / 2048.0]);
    }
    assert_eq!(engine.transport().position(), 25);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().audio_length(), Some(256));
    assert_eq!(old_owner.strong_count(), 1, "resident legacy buffer is uniquely owned");
    while events.pop().is_ok() {}

    let replacement = stereo_ramp(512, 0.5, -0.75, 2000);
    let replacement_pointer = Arc::as_ptr(&replacement);
    commands.push(EngineCommand::LoadAudio(replacement)).unwrap();
    let measured = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
    });
    for (frame, actual) in output.chunks_exact(2).enumerate() {
        let position = (frame + 25) as f32;
        assert_eq!(actual, [0.5 + position / 1024.0, -0.75 + position / 2048.0]);
    }
    assert_eq!(engine.audio().map(Arc::as_ptr), Some(replacement_pointer));
    assert_eq!(engine.transport().position(), 33);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().audio_length(), Some(512));
    assert_eq!(engine.sample_rate(), 1000, "loading media does not adopt its sample rate");
    assert_eq!(measured, (0, 0), "replacement cannot destroy its displaced unique owner");
    assert_eq!(old_owner.strong_count(), 1, "old owner survives until ordinary UI receipt");
    while events.pop().is_ok() {}
    assert_eq!(old_owner.strong_count(), 0, "owner-thread event consumption releases old audio");
}

#[test]
fn unloading_unique_legacy_audio_preserves_cursor_and_stop_notifications_without_callback_destruction() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let old = stereo_ramp(256, 0.125, -0.25, 1000);
    let old_owner = Arc::downgrade(&old);
    commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
    commands.push(EngineCommand::LoadAudio(old)).unwrap();
    commands.push(EngineCommand::Seek(23)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    for (frame, actual) in output.chunks_exact(2).enumerate() {
        let position = (frame + 23) as f32;
        assert_eq!(actual, [0.125 + position / 1024.0, -0.25 + position / 2048.0]);
    }
    assert_eq!(engine.transport().position(), 31);
    assert_eq!(old_owner.strong_count(), 1, "Unload receives a uniquely owned resident buffer");
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
    assert_eq!(notifications.iter().filter(|event| matches!(event, EngineEvent::PlaybackStopped)).count(), 1);
    let capture_stops: Vec<_> = notifications.iter().filter_map(|event| match event {
        EngineEvent::PerformanceCaptureStopped { effective_at_samples } => Some(*effective_at_samples),
        _ => None,
    }).collect();
    assert_eq!(capture_stops, [expected_capture_boundary], "existing heard Capture closure remains exact");
    assert_eq!(measured, (0, 0), "Unload must return its unique legacy owner instead of destroying it");
    let retired: Vec<_> = notifications.iter().filter_map(|event| match event {
        EngineEvent::RetiredAudioBuffer(audio) => Some(audio),
        _ => None,
    }).collect();
    assert_eq!(retired.len(), 1);
    assert_eq!(Arc::as_ptr(retired[0]), old_owner.as_ptr());
    assert_eq!(old_owner.strong_count(), 1);
    drop(retired);
    drop(notifications);
    assert_eq!(old_owner.strong_count(), 0, "ordinary owner-thread receipt releases old audio");
}

fn assert_ramp_block(output: &[f32], position: usize, left: f32, right: f32) {
    for (frame, actual) in output.chunks_exact(2).enumerate() {
        let position = (position + frame) as f32;
        assert_eq!(actual, [left + position / 1024.0, right + position / 2048.0]);
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
    std::iter::from_fn(|| events.pop().ok()).filter_map(|event| match event {
        EngineEvent::RetiredAudioBuffer(audio) => Some(audio),
        _ => None,
    }).collect()
}

#[test]
fn legacy_load_pressure_retains_thirty_two_owners_and_blocks_next_replacement_in_fifo_order() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let original = stereo_ramp(512, 0.0, 0.0, 1000);
    let mut owners = vec![Arc::downgrade(&original)];
    commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
    commands.push(EngineCommand::LoadAudio(original)).unwrap();
    commands.push(EngineCommand::Seek(17)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    while events.pop().is_ok() {}
    let ring_capacity = fill_ui_ring(&mut engine);
    let retirement_capacity = engine.pending_retirements.capacity();
    assert_eq!(retirement_capacity, 32);
    for generation in 1..=33 {
        let audio = stereo_ramp(512, generation as f32 / 128.0, -(generation as f32) / 256.0, 1000);
        owners.push(Arc::downgrade(&audio));
        commands.push(EngineCommand::LoadAudio(audio)).unwrap();
    }
    commands.push(EngineCommand::SetBpm(137.0)).unwrap();
    for position in [25, 33] {
        assert_eq!(crate::retirement::tests::allocations(|| engine.process_block(AudioProcessBlock::new(&mut output, 2))), (0, 0));
        assert_ramp_block(&output, position, 32.0 / 128.0, -32.0 / 256.0);
        assert_eq!(engine.pending_retirements.len(), 32);
        assert_eq!(engine.pending_retirements.capacity(), retirement_capacity);
        assert_eq!(engine.audio().map(Arc::as_ptr), Some(owners[32].as_ptr()));
        let EngineCommand::LoadAudio(incoming) = engine.cmd_rx.peek().unwrap() else { panic!("33rd replacement must remain queued") };
        assert_eq!(Arc::as_ptr(incoming), owners[33].as_ptr());
        assert_eq!(engine.transport().position(), (position + 8) as u64);
        assert_eq!(engine.transport().audio_length(), Some(512));
        assert!(engine.transport().is_playing());
        assert_ne!(engine.transport().bpm(), 137.0);
        assert!(owners.iter().all(|owner| owner.strong_count() == 1));
    }
    drain_full_ui_ring(&mut events, ring_capacity);
    assert_eq!(crate::retirement::tests::allocations(|| engine.process_block(AudioProcessBlock::new(&mut output, 2))), (0, 0));
    assert_ramp_block(&output, 41, 33.0 / 128.0, -33.0 / 256.0);
    assert_eq!(engine.audio().map(Arc::as_ptr), Some(owners[33].as_ptr()));
    assert_eq!(engine.transport().position(), 49);
    assert_eq!(engine.transport().bpm(), 137.0);
    assert!(engine.cmd_rx.peek().is_err());
    assert!(engine.pending_retirements.is_empty());
    let returned = returned_legacy_audio(&mut events);
    assert_eq!(returned.len(), 33);
    for owner in &owners[..33] {
        assert_eq!(returned.iter().filter(|audio| std::ptr::eq(Arc::as_ptr(audio), owner.as_ptr())).count(), 1);
    }
    drop(returned);
    assert!(owners[..33].iter().all(|owner| owner.strong_count() == 0));
    assert_eq!(owners[33].strong_count(), 1, "only admitted final replacement remains resident");
}

#[test]
fn legacy_unload_pressure_requires_two_free_credits_before_clearing_audio_and_stopping() {
    for free_credits in [1, 2] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let original = stereo_ramp(512, 0.0, 0.0, 1000);
        let mut owners = vec![Arc::downgrade(&original)];
        commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
        commands.push(EngineCommand::LoadAudio(original)).unwrap();
        commands.push(EngineCommand::Seek(17)).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        let mut output = [0.0; 16];
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
        while events.pop().is_ok() {}
        let ring_capacity = fill_ui_ring(&mut engine);
        let retirement_capacity = engine.pending_retirements.capacity();
        let replacements = retirement_capacity - free_credits;
        for generation in 1..=replacements {
            let audio = stereo_ramp(512, generation as f32 / 128.0, -(generation as f32) / 256.0, 1000);
            owners.push(Arc::downgrade(&audio));
            commands.push(EngineCommand::LoadAudio(audio)).unwrap();
        }
        assert_eq!(crate::retirement::tests::allocations(|| engine.process_block(AudioProcessBlock::new(&mut [], 2))), (0, 0));
        assert_eq!(engine.pending_retirements.len(), replacements);
        assert_eq!(engine.pending_retirements.capacity() - engine.pending_retirements.len(), free_credits);
        assert_eq!(engine.audio().map(Arc::as_ptr), Some(owners[replacements].as_ptr()));
        commands.push(EngineCommand::UnloadAudio).unwrap();
        commands.push(EngineCommand::SetBpm(137.0)).unwrap();
        assert_eq!(crate::retirement::tests::allocations(|| engine.process_block(AudioProcessBlock::new(&mut output, 2))), (0, 0));
        let stop_position = if free_credits == 1 {
            assert_ramp_block(&output, 25, replacements as f32 / 128.0, -(replacements as f32) / 256.0);
            assert!(matches!(engine.cmd_rx.peek(), Ok(EngineCommand::UnloadAudio)));
            assert_eq!(engine.audio().map(Arc::as_ptr), Some(owners[replacements].as_ptr()));
            assert!(engine.transport().is_playing());
            assert_eq!(engine.transport().audio_length(), Some(512));
            33
        } else {
            assert_eq!(output, [0.0; 16]);
            assert!(matches!(engine.cmd_rx.peek(), Ok(EngineCommand::SetBpm(value)) if *value == 137.0));
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
        assert_eq!(crate::retirement::tests::allocations(|| engine.process_block(AudioProcessBlock::new(&mut output, 2))), (0, 0));
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
            assert_eq!(returned.iter().filter(|audio| std::ptr::eq(Arc::as_ptr(audio), owner.as_ptr())).count(), 1);
        }
        drop(returned);
        assert!(owners.iter().all(|owner| owner.strong_count() == 0));
    }
}
