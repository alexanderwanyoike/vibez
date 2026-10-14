//! Public note edits retain sample-accurate note-off behavior without callback allocation.

use crate::{
    commands::EngineCommand,
    engine::{AudioEngine, AudioProcessBlock},
    mixer::EngineTrack,
    playback_source::{EngineNoteClip, PreparedPlaybackSource},
    test_support::DelayProbeInstrument,
};
use vibez_core::{
    id::{ClipId, TrackId},
    midi::MidiNote,
    perform::GrooveGrid,
};

#[test]
fn removing_a_note_rebuilds_its_surviving_note_offs_without_callback_allocation() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let clip = ClipId::new();
    let notes = vec![
        MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 2.5,
        },
        MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 1.0,
            duration_beats: 2.0,
        },
        MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 2.0,
            duration_beats: 1.0,
        },
    ];
    let source = PreparedPlaybackSource::new(
        vec![],
        vec![EngineNoteClip::new(
            clip,
            0.0,
            4.0,
            notes,
            0.0,
            false,
            0.0,
            4.0,
            GrooveGrid::Sixteenth,
        )],
        vec![],
    );
    for command in [
        EngineCommand::SetSampleRate(96),
        EngineCommand::SetBpm(60.0),
        EngineCommand::AddTrack(Box::new(EngineTrack::with_playback_source(track, source))),
        EngineCommand::SetInstrument {
            track_id: track,
            instrument: Box::new(DelayProbeInstrument::new(0, 1.0)),
        },
        EngineCommand::Play,
    ] {
        commands.push(command).unwrap();
    }
    let mut sounding = [0.0; 48 * 2];
    engine.process_block(AudioProcessBlock::new(&mut sounding, 2));
    let level = sounding[0];
    assert!(level > 0.1);
    assert!(sounding.iter().all(|sample| (*sample - level).abs() < 1e-7));
    while events.pop().is_ok() {}

    commands
        .push(EngineCommand::RemoveNote {
            track_id: track,
            clip_id: clip,
            note_index: 1,
        })
        .unwrap();
    let measured = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
    });
    let mut released = [0.0; 16 * 2];
    engine.process_block(AudioProcessBlock::new(&mut released, 2));
    assert_eq!(released, [0.0; 16 * 2]);

    commands.push(EngineCommand::Seek(0)).unwrap();
    let mut replayed = [0.0; 384 * 2];
    engine.process_block(AudioProcessBlock::new(&mut replayed, 2));
    for (frame, pair) in replayed.as_chunks::<2>().0.iter().enumerate() {
        let expected = if frame < 288 { level } else { 0.0 };
        assert!(
            pair.iter().all(|sample| (*sample - expected).abs() < 1e-7),
            "frame {frame}: {pair:?} != {expected}"
        );
    }
    assert_eq!(measured, (0, 0));
}
