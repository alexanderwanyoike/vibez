//! Public note edits retain sample-accurate note-off behavior without callback allocation.

use crate::{
    commands::EngineCommand,
    engine::{AudioEngine, AudioProcessBlock},
    mixer::EngineTrack,
    playback_source::{EngineNoteClip, PreparedPlaybackSource},
    test_support::DelayProbeInstrument,
};
use vibez_core::{
    effect::ParamDescriptor,
    id::{ClipId, TrackId},
    midi::{InstrumentKind, MidiNote},
    perform::GrooveGrid,
};

struct PitchProbe {
    active: [bool; 256],
}

impl vibez_instruments::Instrument for PitchProbe {
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }

    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }

    fn get_param(&self, _: usize) -> f32 {
        0.0
    }

    fn note_on(&mut self, pitch: u8, _: u8) {
        self.active[pitch as usize] = true;
    }

    fn note_off(&mut self, pitch: u8) {
        self.active[pitch as usize] = false;
    }

    fn render(&mut self, buffer: &mut [f32], _: usize) {
        let low = if self.active[60] { 0.125 } else { 0.0 };
        let high = if self.active[61] { 0.25 } else { 0.0 };
        buffer.fill(low + high);
    }

    fn reset(&mut self) {
        self.active.fill(false);
    }
}

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

#[test]
fn editing_pitch_start_and_length_preserves_note_indices_and_allocation_free_note_offs() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let clip = ClipId::new();
    let note = |pitch, start_beat, duration_beats| MidiNote {
        pitch,
        velocity: 100,
        start_beat,
        duration_beats,
    };
    let source = PreparedPlaybackSource::new(
        vec![],
        vec![EngineNoteClip::new(
            clip,
            0.0,
            5.0,
            vec![
                note(60, 3.0, 1.0),
                note(60, 0.0, 3.75),
                note(61, 1.0, 2.5),
                note(60, 2.0, 0.5),
                note(60, 3.0, 1.25),
            ],
            0.0,
            false,
            0.0,
            5.0,
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
            instrument: Box::new(PitchProbe { active: [false; 256] }),
        },
        EngineCommand::Play,
    ] {
        commands.push(command).unwrap();
    }
    let mut sounding = [0.0; 48 * 2];
    engine.process_block(AudioProcessBlock::new(&mut sounding, 2));
    let unit = sounding[0];
    assert!(unit > 0.01);
    assert!(sounding.iter().all(|sample| (*sample - unit).abs() < 1e-7));
    assert!(engine.transport().is_playing());

    let assert_audio = |stage: &str, audio: &[f32], intervals: &[(usize, usize, f32)]| {
        for (frame, pair) in audio.as_chunks::<2>().0.iter().enumerate() {
            let weight = intervals.iter().find(|&&(start, end, _)| (start..end).contains(&frame))
                .map_or(0.0, |&(_, _, weight)| weight);
            let expected = unit * weight;
            assert!(pair.iter().all(|sample| (*sample - expected).abs() < 1e-7),
                "{stage}, frame {frame}: {pair:?} != {expected}");
        }
    };
    let mut rendered = [0.0; 480 * 2];
    engine.process_block(AudioProcessBlock::new(&mut rendered, 2));
    assert!(!engine.transport().is_playing(), "initial voices finished");
    commands.push(EngineCommand::Seek(0)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [], 2));
    assert!(engine.transport().is_playing(), "baseline replay started");
    engine.process_block(AudioProcessBlock::new(&mut rendered, 2));
    assert_audio("baseline", &rendered, &[(0, 96, 1.0), (96, 240, 3.0), (240, 288, 2.0), (288, 336, 3.0), (336, 384, 1.0)]);

    // Equal-start notes must not clamp one another; their distinct lengths
    // make a zero-length clamp audible after editing the original index 0.
    for (stage, note_index, replacement, intervals) in [
        ("pitch edit", 2, note(60, 1.0, 2.5), &[(0, 240, 1.0), (288, 384, 1.0)][..]),
        ("start edit", 3, note(60, 0.5, 0.5), &[(0, 384, 1.0)][..]),
        ("length edit", 0, note(60, 3.0, 0.25), &[(0, 312, 1.0)][..]),
    ] {
        commands.push(EngineCommand::Seek(0)).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        engine.process_block(AudioProcessBlock::new(&mut sounding, 2));
        assert!(engine.transport().is_playing(), "{stage}: active voice probe");
        assert!(sounding.iter().all(|sample| (*sample - unit).abs() < 1e-7), "{stage}: active voice probe");
        while events.pop().is_ok() {}
        commands.push(EngineCommand::EditNote {
            track_id: track,
            clip_id: clip,
            note_index,
            note: replacement,
        }).unwrap();
        let edit_allocations = crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut [], 2));
        });
        assert_eq!(edit_allocations, (0, 0), "{stage}: editing original note {note_index}");
        // Finish all note-offs before seeking, so a discontinuity flush cannot
        // affect the independently asserted replay timeline.
        engine.process_block(AudioProcessBlock::new(&mut rendered, 2));
        assert!(!engine.transport().is_playing(), "{stage}: edited voices finished");
        commands.push(EngineCommand::Seek(0)).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
        assert!(engine.transport().is_playing(), "{stage}: replay started");
        let render_allocations = crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut rendered, 2));
        });
        assert_audio(stage, &rendered, intervals);
        assert_eq!(render_allocations, (0, 0), "{stage}: replaying original note {note_index}");
    }
}
