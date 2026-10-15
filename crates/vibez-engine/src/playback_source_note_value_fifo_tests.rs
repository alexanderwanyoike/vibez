//! Pending note growth preserves current first-match edits, audible clamps and later FIFO release.

use super::*;
use crate::clip_insertion::{ClipInsertionOutcome, PreparedNoteInsertion};
use crate::events::EngineEvent;

fn measured(engine: &mut AudioEngine, output: &mut [f32]) {
    assert_eq!(crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(output, 2));
    }), (0, 0));
}

#[test]
fn pending_notes_keep_current_first_match_edits_and_clamps_until_fifo_release() {
    let duplicate = ClipId::new();
    let notes = vec![note(60, 16, 2.0, 1.0), note(60, 32, 0.0, 4.0),
        note(60, 96, 2.0, 0.5), note(67, 64, 3.0, 1.0)].into_boxed_slice().into_vec();
    let first = EngineNoteClip::new(duplicate, 0.0, 8.0, notes, 0.0, false, 0.0, 8.0, GrooveGrid::Sixteenth);
    assert_eq!(first.notes.len(), first.notes.capacity());
    assert_eq!(first.notes.len(), first.next_same_pitch_start_beats.capacity());
    let second = EngineNoteClip::new(duplicate, 16.0, 8.0, vec![note(72, 32, 0.5, 2.0)],
        0.25, true, 0.25, 4.0, GrooveGrid::Eighth);
    let track_id = TrackId::new();
    let unrelated = TrackId::new();
    let target_calls = Arc::new(VoiceCalls::default());
    let live_calls = Arc::new(VoiceCalls::default());
    let mut target = EngineTrack::with_playback_source(track_id,
        PreparedPlaybackSource::new(Vec::new(), vec![first, second], Vec::new()));
    target.pan = 0.0;
    target.instrument = Some(Box::new(VelocityProbe {
        native: crate::test_support::DelayProbeInstrument::new(0, 0.25), scale: 1.0,
        calls: Arc::clone(&target_calls),
    }));
    let mut held = EngineTrack::new(unrelated);
    held.pan = 0.0;
    held.instrument = Some(Box::new(VelocityProbe {
        native: crate::test_support::DelayProbeInstrument::new(0, 0.125), scale: 1.0,
        calls: Arc::clone(&live_calls),
    }));
    let owners = [
        crate::engine::reconfiguration::instrument_owner_identity(target.instrument.as_deref().unwrap()),
        crate::engine::reconfiguration::instrument_owner_identity(held.instrument.as_deref().unwrap()),
    ];
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands.push(EngineCommand::AddTrack(Box::new(target))).unwrap();
    commands.push(EngineCommand::AddTrack(Box::new(held))).unwrap();
    let model: Vec<_> = [track_id, unrelated, TrackId::MASTER].into_iter().map(|id| RoutingChannel {
        id, is_bus: id.is_master(), sends: Vec::new(), effects: Vec::new(),
    }).collect();
    commands.push(EngineCommand::SetRouting(crate::routing::PreparedRouting::prepare(&model, 128).unwrap())).unwrap();
    let swing = SwingAmount::new(0.62);
    commands.push(EngineCommand::SetProjectSwing(swing)).unwrap();
    commands.push(EngineCommand::ExternalNoteOn { track_id: unrelated, pitch: 72, velocity: 32 }).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 256];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    assert_signal(&output, 0.375);
    while events.pop().is_ok() {}
    let first = ClipSnapshot::observe(&engine.tracks()[0].playback_source.note_clips[0]);
    let second = ClipSnapshot::observe(&engine.tracks()[0].playback_source.note_clips[1]);
    assert_eq!(first.latch, Some((0, swing)));
    let source_owner = std::ptr::from_ref(engine.tracks()[0].playback_source.as_ref());
    let outer_owner = engine.tracks()[0].playback_source.note_clips.as_ptr();
    let a_note = note(60, 64, 1.0, 0.5);
    let b_note = note(60, 16, 0.5, 0.25);
    let a = PreparedNoteInsertion::new(a_note);
    let b = PreparedNoteInsertion::new(b_note);
    let a_id = a.id();
    let b_id = b.id();
    commands.push(EngineCommand::AddNote { track_id, clip_id: duplicate, insertion: a }).unwrap();
    // Move preserves the sounding voice; duration/groove edits deliberately
    // flush it, so they cannot witness insertion's no-flush invariant here.
    commands.push(EngineCommand::MoveNoteClip { track_id, clip_id: duplicate, new_position_beats: 0.5 }).unwrap();
    commands.push(EngineCommand::AddNote { track_id, clip_id: duplicate, insertion: b }).unwrap();
    measured(&mut engine, &mut []);
    let mut request = None;
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::ClipInsertionStorageRequested(value) => {
                assert_eq!(value.id, a_id);
                assert_eq!((value.track_id, value.clip_id), (track_id, duplicate));
                assert!(request.replace(value).is_none());
            }
            EngineEvent::ClipInsertionCompleted(_) => panic!("first growth cannot complete before fulfillment"),
            _ => {}
        }
    }
    let request = request.expect("the actual full note/cache arrays require growth");
    assert_eq!(commands.slots(), vibez_core::constants::RING_BUFFER_CAPACITY - 2);
    for block in 0..2 {
        measured(&mut engine, &mut output[..16]);
        assert_signal(&output[..16], 0.375);
        first.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[0]);
        second.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[1]);
        assert_eq!(engine.transport().position(), 128 + (block + 1) * 8);
        assert_eq!(target_calls.observed(), (1, 0, 0));
        assert_eq!(live_calls.observed(), (1, 0, 0));
        assert_eq!(commands.slots(), vibez_core::constants::RING_BUFFER_CAPACITY - 2);
        while let Ok(event) = events.pop() {
            assert!(!matches!(event, EngineEvent::ClipInsertionCompleted(_) | EngineEvent::ClipInsertionStorageRequested(_)));
        }
    }
    request.fulfill();
    measured(&mut engine, &mut []);
    let mut applied = Vec::new();
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::ClipInsertionCompleted(value) => {
                assert_eq!(value.outcome, ClipInsertionOutcome::Applied);
                assert_eq!((value.track_id, value.clip_id), (track_id, duplicate));
                applied.push(value.id());
            }
            EngineEvent::ClipInsertionStorageRequested(_) => panic!("grown current storage must also accept the second stale hint"),
            _ => {}
        }
    }
    assert_eq!(applied, [a_id, b_id]);
    let mut expected = first.notes.clone();
    expected.extend([a_note, b_note]);
    let clip = &engine.tracks()[0].playback_source.note_clips[0];
    assert_eq!(clip.position_beats, 0.5);
    assert_eq!(clip.notes, expected);
    assert_eq!(clip.next_same_pitch_start_beats, expected_clamps(&expected));
    assert_eq!(clip.groove_latch.get(), first.latch);
    assert_eq!(commands.slots(), vibez_core::constants::RING_BUFFER_CAPACITY);
    let required_tail = clip.notes.capacity() - clip.notes.len() + 1;
    assert!(required_tail >= 1);
    let mut tail_ids = Vec::new();
    let mut tail_notes = Vec::new();
    // Derive the second exhaustion from the actual installed allocation.
    // Every new control still uses the same insufficient default hint.
    for index in 0..required_tail {
        let value = if index + 1 == required_tail { note(60, 48, 0.75, 0.25) }
            else { note(72 + (index % 12) as u8, 32, 5.0 + index as f64 * 0.0625, 0.125) };
        let insertion = PreparedNoteInsertion::new(value);
        tail_ids.push(insertion.id());
        tail_notes.push(value);
        commands.push(EngineCommand::AddNote { track_id, clip_id: duplicate, insertion }).unwrap();
    }
    commands.push(EngineCommand::ExternalNoteOff { track_id: unrelated, pitch: 72 }).unwrap();
    measured(&mut engine, &mut []);
    let mut request = None;
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::ClipInsertionCompleted(value) => {
                assert_eq!(value.id(), tail_ids[applied.len() - 2]);
                assert_eq!(value.outcome, ClipInsertionOutcome::Applied);
                assert_eq!((value.track_id, value.clip_id), (track_id, duplicate));
                applied.push(value.id());
            }
            EngineEvent::ClipInsertionStorageRequested(value) => {
                assert_eq!(value.id, *tail_ids.last().unwrap());
                assert_eq!((value.track_id, value.clip_id), (track_id, duplicate));
                assert!(request.replace(value).is_none());
            }
            EngineEvent::SourceNoteInput { on: false, .. } | EngineEvent::InstrumentNoteInput { on: false, .. } => {
                panic!("later note-off cannot overtake the final pending note");
            }
            _ => {}
        }
    }
    let request = request.expect("the actual installed capacity must be crossed a second time");
    expected.extend_from_slice(&tail_notes[..tail_notes.len() - 1]);
    let before_last = ClipSnapshot::observe(&engine.tracks()[0].playback_source.note_clips[0]);
    assert_eq!(before_last.notes, expected);
    assert_eq!(before_last.notes.len(), before_last.notes_capacity);
    assert_eq!(before_last.clamps.len(), before_last.clamps_capacity);
    assert_eq!(before_last.clamps, expected_clamps(&expected));
    assert_eq!(before_last.clamps[first.notes.len() + 1], Some(1.0));
    assert_eq!(before_last.geometry.0, 0.5);
    assert_eq!(before_last.latch, first.latch);
    for block in 0..3 {
        measured(&mut engine, &mut output[..16]);
        assert_signal(&output[..16], 0.375);
        before_last.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[0]);
        second.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[1]);
        assert_eq!(engine.transport().position(), 144 + (block + 1) * 8);
        assert!(engine.transport().is_playing());
        assert_eq!(engine.transport().audio_length(), Some(24 * 22_050));
        assert_eq!(target_calls.observed(), (1, 0, 0));
        assert_eq!(live_calls.observed(), (1, 0, 0));
        assert_eq!(commands.slots(), vibez_core::constants::RING_BUFFER_CAPACITY - 1);
        while let Ok(event) = events.pop() {
            assert!(!matches!(event, EngineEvent::ClipInsertionCompleted(_) | EngineEvent::ClipInsertionStorageRequested(_)
                | EngineEvent::SourceNoteInput { on: false, .. } | EngineEvent::InstrumentNoteInput { on: false, .. }));
        }
    }
    request.fulfill();
    measured(&mut engine, &mut []);
    let mut order = Vec::new();
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::ClipInsertionCompleted(value) => {
                assert_eq!(value.id(), *tail_ids.last().unwrap());
                assert_eq!(value.outcome, ClipInsertionOutcome::Applied);
                assert_eq!((value.track_id, value.clip_id), (track_id, duplicate));
                applied.push(value.id());
                order.push("complete");
            }
            EngineEvent::SourceNoteInput { track_id, pitch, on: false, position, .. } => {
                assert_eq!((track_id, pitch), (unrelated, 72));
                assert_eq!((position.effective_at_samples, position.canonical_at_samples), (168, 168));
                assert_eq!(position.section_id, None);
                assert_eq!(position.section_position_samples, None);
                assert_eq!(position.canonical_section_position_samples, None);
                order.push("source-off");
            }
            EngineEvent::InstrumentNoteInput { track_id, pitch, on: false, effective_at_samples, .. } => {
                assert_eq!((track_id, pitch, effective_at_samples), (unrelated, 72, 168));
                order.push("heard-off");
            }
            EngineEvent::ClipInsertionStorageRequested(_) => panic!("one sufficient reply must commit the original final delta"),
            _ => {}
        }
    }
    assert_eq!(order, ["complete", "source-off", "heard-off"]);
    assert_eq!(&applied[2..], tail_ids.as_slice());
    assert_eq!(commands.slots(), vibez_core::constants::RING_BUFFER_CAPACITY);
    expected.push(*tail_notes.last().unwrap());
    let clip = &engine.tracks()[0].playback_source.note_clips[0];
    assert_eq!(clip.notes, expected);
    assert_eq!(clip.next_same_pitch_start_beats, expected_clamps(&expected));
    assert_eq!(clip.next_same_pitch_start_beats[first.notes.len() + 1], Some(0.75));
    assert_eq!(clip.position_beats, 0.5);
    assert_eq!(clip.groove_latch.get(), first.latch);
    second.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[1]);
    assert_eq!(std::ptr::from_ref(engine.tracks()[0].playback_source.as_ref()), source_owner);
    assert_eq!(engine.tracks()[0].playback_source.note_clips.as_ptr(), outer_owner);
    for (track, owner) in engine.tracks().iter().zip(owners) {
        assert_eq!(crate::engine::reconfiguration::instrument_owner_identity(track.instrument.as_deref().unwrap()), owner);
    }
    assert_eq!(target_calls.observed(), (1, 0, 0));
    assert_eq!(live_calls.observed(), (1, 1, 0));
    assert_eq!(engine.transport().position(), 168);
    // Move's existing schedule retriggers its original note at global beat0.5.
    // The inserted starts remain local. MPC's48-tick Sixteenth pair rounds
    // swing0.62 to30/96 beat, mapping local0.75 to0.8125.
    let b_on = 22_050;
    let last_on = (1.3125_f64 * 22_050.0).ceil() as u64;
    let a_on = 33_075;
    let a_off = 44_100;
    let end = a_off + 16;
    let partitions = [1, 7, 31, 128];
    let mut block = 0;
    while engine.transport().position() < end {
        let start = engine.transport().position();
        let frames = partitions[block % partitions.len()].min((end - start) as usize);
        measured(&mut engine, &mut output[..frames * 2]);
        for (frame, &[left, right]) in output[..frames * 2].as_chunks::<2>().0.iter().enumerate() {
            let position = start + frame as u64;
            let expected = if position < b_on { 0.25 } else if position < last_on { 0.125 }
                else if position < a_on { 0.375 } else if position < a_off { 0.5 } else { 0.0 };
            assert_eq!(left, expected, "current-data clamp audio at sample {position}");
            assert_eq!(right, 0.0);
        }
        while let Ok(event) = events.pop() {
            assert!(!matches!(event, EngineEvent::ClipInsertionCompleted(_) | EngineEvent::ClipInsertionStorageRequested(_)));
        }
        block += 1;
    }
    assert_eq!(target_calls.observed(), (5, 4, 0));
    assert_eq!(live_calls.observed(), (1, 1, 0));
    assert_eq!(engine.transport().position(), end);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().bpm(), 120.0);
    assert_eq!(engine.transport().audio_length(), Some(24 * 22_050));
    let clip = &engine.tracks()[0].playback_source.note_clips[0];
    assert_eq!(clip.notes, expected);
    assert_eq!(clip.next_same_pitch_start_beats, expected_clamps(&expected));
    second.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[1]);
}
