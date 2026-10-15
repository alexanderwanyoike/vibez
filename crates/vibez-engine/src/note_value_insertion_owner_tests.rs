//! Missing note targets return complete prepared data, storage and mailbox owners off callback.

use super::*;
use crate::commands::EngineCommand;
use crate::engine::{AudioEngine, AudioProcessBlock};
use crate::events::EngineEvent;
use crate::mixer::EngineTrack;
use vibez_core::routing::RoutingChannel;

#[test]
fn missing_track_or_clip_returns_complete_prepared_note_value_without_callback_heap_work() {
    for outcome in [
        ClipInsertionOutcome::MissingTrack,
        ClipInsertionOutcome::MissingClip,
    ] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let resident_track = TrackId::new();
        let resident_clip = ClipId::new();
        let resident_note = MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 4.0,
        };
        let source = PreparedPlaybackSource::new(
            Vec::new(),
            vec![EngineNoteClip::new(
                resident_clip,
                0.0,
                8.0,
                vec![resident_note],
                0.0,
                false,
                0.0,
                8.0,
                GrooveGrid::Off,
            )],
            Vec::new(),
        );
        let notes = source.note_clips[0].notes.as_ptr();
        let mut track = EngineTrack::with_playback_source(resident_track, source);
        track.pan = 0.0;
        track.instrument = Some(Box::new(crate::test_support::DelayProbeInstrument::new(
            0, 0.25,
        )));
        commands
            .push(EngineCommand::AddTrack(Box::new(track)))
            .unwrap();
        let channels: Vec<_> = [resident_track, TrackId::MASTER]
            .into_iter()
            .map(|id| RoutingChannel {
                id,
                is_bus: id.is_master(),
                sends: Vec::new(),
                effects: Vec::new(),
            })
            .collect();
        commands
            .push(EngineCommand::SetRouting(
                crate::routing::PreparedRouting::prepare(&channels, 8).unwrap(),
            ))
            .unwrap();
        commands.push(EngineCommand::Play).unwrap();
        let mut output = [0.0; 16];
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
        while events.pop().is_ok() {}
        let track_id = if outcome == ClipInsertionOutcome::MissingTrack {
            TrackId::new()
        } else {
            resident_track
        };
        let clip_id = ClipId::new();
        let note = MidiNote {
            pitch: 67,
            velocity: 93,
            start_beat: 1.25,
            duration_beats: 0.75,
        };
        let insertion = PreparedNoteInsertion::new(note);
        let identity = Arc::downgrade(&insertion.0.id.0);
        let operation = insertion.id();
        let control = std::ptr::from_ref(insertion.0.as_ref());
        let ring = std::ptr::from_ref(insertion.0.receiver.buffer());
        commands
            .push(EngineCommand::AddNote {
                track_id,
                clip_id,
                insertion,
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                engine.process_block(AudioProcessBlock::new(&mut output, 2));
            }),
            (0, 0)
        );
        assert_eq!(engine.tracks().len(), 1);
        let source = &engine.tracks()[0].playback_source;
        assert_eq!(source.note_clips.len(), 1);
        assert_eq!(source.note_clips[0].id, resident_clip);
        assert_eq!(source.note_clips[0].notes, [resident_note]);
        assert_eq!(source.note_clips[0].notes.as_ptr(), notes);
        assert_eq!(engine.transport().position(), 16);
        assert!(engine.transport().is_playing());
        assert_eq!(engine.transport().audio_length(), Some(8 * 22_050));
        for &[left, right] in output.as_chunks::<2>().0 {
            assert_eq!((left, right), (0.25, 0.0));
        }
        let EngineEvent::ClipInsertionCompleted(completed) = events.pop().unwrap() else {
            panic!("a missing note target must return its complete prepared owner");
        };
        assert_eq!(completed.id(), operation);
        assert_eq!(
            (completed.track_id, completed.target),
            (
                track_id,
                crate::clip_insertion::ClipInsertionTarget::Clip(clip_id)
            )
        );
        assert_eq!(completed.outcome, outcome);
        assert_eq!(std::ptr::from_ref(completed.owner.as_ref()), control);
        assert_eq!(std::ptr::from_ref(completed.owner.receiver.buffer()), ring);
        assert!(completed.owner.sender.is_some());
        assert!(!completed.owner.receiver.is_abandoned());
        assert!(completed.owner.reply.is_none());
        assert!(matches!(
            completed.owner.storage,
            ClipInsertionStorage::NoteValue(_)
        ));
        let Some(ClipInsertionDelta::NoteValue(rejected)) = completed.owner.clip.as_ref() else {
            panic!("missing note target must keep the original note value");
        };
        assert_eq!(*rejected, note);
        assert_eq!(identity.strong_count(), 2);
        assert_eq!(
            crate::retirement::tests::allocations(|| drop(completed)),
            (0, 5)
        );
        assert_eq!(identity.strong_count(), 1);
        drop(operation);
        assert_eq!(identity.strong_count(), 0);
        while let Ok(event) = events.pop() {
            assert!(!matches!(
                event,
                EngineEvent::ClipInsertionStorageRequested(_)
                    | EngineEvent::ClipInsertionCompleted(_)
            ));
        }
    }
}
