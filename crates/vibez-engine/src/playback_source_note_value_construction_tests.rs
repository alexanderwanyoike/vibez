//! Constructor and offline note arrays cross actual inner capacity with complete schedules intact.

use super::*;
use super::consumer_tests::measured;
use crate::clip_insertion::{ClipInsertionOutcome, PreparedNoteInsertion};
use crate::events::EngineEvent;

pub(crate) fn boundary_clip(count: usize, id: ClipId) -> EngineNoteClip {
    let notes = (0..count).map(|index| match index {
        0 => note(60, 32, 0.0, 4.0),
        1 => note(60, 16, 2.0, 1.0),
        2 => note(60, 96, 2.0, 0.5),
        _ => note(67 + (index % 8) as u8, 32, 6.0 + (count - index) as f64 * 0.125, 0.25),
    }).collect::<Vec<_>>().into_boxed_slice().into_vec();
    EngineNoteClip::new(id, 0.0, 64.0, notes, 0.0, false, 0.0, 64.0, GrooveGrid::Sixteenth)
}

pub(crate) fn verify_note_value_source_growth(source: PreparedPlaybackSource, count: usize) {
    assert_eq!(source.note_clips.len(), 2);
    let prepared = &source.note_clips[0];
    let expected = boundary_clip(count, prepared.id);
    assert_eq!(prepared.notes, expected.notes);
    assert_eq!(prepared.next_same_pitch_start_beats, expected_clamps(&expected.notes));
    assert_eq!(prepared.groove_latch.get(), None);
    assert_eq!(prepared.notes.len(), count);
    assert_eq!(prepared.notes.capacity(), count);
    assert_eq!(prepared.next_same_pitch_start_beats.len(), count);
    assert_eq!(prepared.next_same_pitch_start_beats.capacity(), count);
    let track_id = TrackId::new();
    let clip_id = prepared.id;
    let outer = source.note_clips.as_ptr();
    let outer_capacity = source.note_clips.capacity();
    let staging = source.removed_note_clips.as_ptr();
    let staging_capacity = source.removed_note_clips.capacity();
    assert!(source.removed_note_clips.is_empty());
    assert!(staging_capacity >= source.note_clips.len());
    let mut channel = EngineTrack::with_playback_source(track_id, source);
    channel.pan = 0.0;
    let calls = Arc::new(VoiceCalls::default());
    channel.instrument = Some(Box::new(VelocityProbe {
        native: crate::test_support::DelayProbeInstrument::new(0, 0.25), scale: 1.0,
        calls: Arc::clone(&calls),
    }));
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands.push(EngineCommand::AddTrack(Box::new(channel))).unwrap();
    let model: Vec<_> = [track_id, TrackId::MASTER].into_iter().map(|id| RoutingChannel {
        id, is_bus: id.is_master(), sends: Vec::new(), effects: Vec::new(),
    }).collect();
    commands.push(EngineCommand::SetRouting(crate::routing::PreparedRouting::prepare(&model, 128).unwrap())).unwrap();
    let swing = SwingAmount::new(0.62);
    commands.push(EngineCommand::SetProjectSwing(swing)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 256];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    assert_signal(&output, 0.25);
    while events.pop().is_ok() {}
    assert_eq!(engine.transport().position(), 128);
    let first = ClipSnapshot::observe(&engine.tracks()[0].playback_source.note_clips[0]);
    first.assert_full();
    assert_eq!(first.latch, Some((0, swing)));
    let other = ClipSnapshot::observe(&engine.tracks()[0].playback_source.note_clips[1]);
    let source_owner = std::ptr::from_ref(engine.tracks()[0].playback_source.as_ref());
    let length = engine.transport().audio_length();
    let inserted = note(60, 64, 1.0, 0.5);
    let insertion = PreparedNoteInsertion::new(inserted);
    let operation = insertion.id();
    commands.push(EngineCommand::AddNote { track_id, clip_id, insertion }).unwrap();
    measured(&mut engine, &mut []);
    let mut request = None;
    while let Ok(event) = events.pop() {
        if let EngineEvent::ClipInsertionStorageRequested(value) = event {
            assert_eq!(value.id, operation);
            assert_eq!((value.track_id, value.clip_id), (track_id, clip_id));
            assert!(request.replace(value).is_none());
        } else { assert!(!matches!(event, EngineEvent::ClipInsertionCompleted(_))); }
    }
    let request = request.expect("actual full notes and cache must exceed default1 storage together");
    measured(&mut engine, &mut output[..16]);
    assert_signal(&output[..16], 0.25);
    assert_eq!(engine.transport().position(), 136);
    first.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[0]);
    other.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[1]);
    while let Ok(event) = events.pop() {
        assert!(!matches!(event, EngineEvent::ClipInsertionStorageRequested(_) | EngineEvent::ClipInsertionCompleted(_)));
    }
    request.fulfill();
    measured(&mut engine, &mut []);
    let EngineEvent::ClipInsertionCompleted(owner) = events.pop().unwrap() else {
        panic!("fulfilled inner growth must return the original complete owner");
    };
    assert_eq!(owner.id(), operation);
    assert_eq!((owner.track_id, owner.clip_id), (track_id, clip_id));
    assert_eq!(owner.outcome, ClipInsertionOutcome::Applied);
    let source = &engine.tracks()[0].playback_source;
    assert_eq!(std::ptr::from_ref(source.as_ref()), source_owner);
    assert_eq!(source.note_clips.len(), 2);
    assert_eq!(source.note_clips.as_ptr(), outer);
    assert_eq!(source.note_clips.capacity(), outer_capacity);
    assert_eq!(source.removed_note_clips.as_ptr(), staging);
    assert_eq!(source.removed_note_clips.capacity(), staging_capacity);
    assert!(source.removed_note_clips.is_empty());
    first.assert_appended(&source.note_clips[0], inserted);
    other.assert_unchanged(&source.note_clips[1]);
    assert_eq!(engine.transport().audio_length(), length);
    assert_eq!(calls.observed(), (1, 0, 0));
    assert_eq!(crate::retirement::tests::allocations(|| drop(owner)), (0, 7));
    while events.pop().is_ok() {}
    let mut step = 0;
    let end = 33_075 + 8;
    while engine.transport().position() < end {
        let start = engine.transport().position();
        let frames = [1, 7, 31, 128][step % 4].min((end - start) as usize);
        measured(&mut engine, &mut output[..frames * 2]);
        for (offset, &[left, right]) in output[..frames * 2].as_chunks::<2>().0.iter().enumerate() {
            let sample = start + offset as u64;
            let expected = if sample < 22_050 { 0.25 } else if sample < 33_075 { 0.5 } else { 0.0 };
            assert_eq!((left, right), (expected, 0.0), "count {count} sample {sample}");
        }
        while let Ok(event) = events.pop() {
            assert!(!matches!(event, EngineEvent::ClipInsertionStorageRequested(_) | EngineEvent::ClipInsertionCompleted(_)));
        }
        step += 1;
    }
    assert_eq!(calls.observed(), (2, 2, 0));
    assert_eq!(engine.transport().audio_length(), length);
    other.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[1]);
}

#[test]
fn public_constructor_note_values_cross_full_notes_and_cache_at_seven_boundaries() {
    for count in [1, 2, 20, 21, 128, 129, 257] {
        let target = boundary_clip(count, ClipId::new());
        let first = ClipSnapshot::observe(&target);
        let mut other = boundary_clip(3, ClipId::new());
        other.position_beats = 128.0;
        other.duration_beats = 8.0;
        other.loop_enabled = true;
        other.loop_start_beats = 0.25;
        other.loop_end_beats = 4.0;
        other.start_marker_beats = 0.25;
        other.groove_grid = GrooveGrid::Eighth;
        let second = ClipSnapshot::observe(&other);
        let source = PreparedPlaybackSource::new(Vec::new(), vec![target, other], Vec::new());
        first.assert_unchanged(&source.note_clips[0]);
        second.assert_unchanged(&source.note_clips[1]);
        verify_note_value_source_growth(source, count);
    }
}
