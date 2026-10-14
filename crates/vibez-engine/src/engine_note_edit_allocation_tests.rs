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
            instrument: Box::new(PitchProbe {
                active: [false; 256],
            }),
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
            let weight = intervals
                .iter()
                .find(|&&(start, end, _)| (start..end).contains(&frame))
                .map_or(0.0, |&(_, _, weight)| weight);
            let expected = unit * weight;
            assert!(
                pair.iter().all(|sample| (*sample - expected).abs() < 1e-7),
                "{stage}, frame {frame}: {pair:?} != {expected}"
            );
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
    assert_audio(
        "baseline",
        &rendered,
        &[
            (0, 96, 1.0),
            (96, 240, 3.0),
            (240, 288, 2.0),
            (288, 336, 3.0),
            (336, 384, 1.0),
        ],
    );

    // Equal-start notes must not clamp one another; their distinct lengths
    // make a zero-length clamp audible after editing the original index 0.
    for (stage, note_index, replacement, intervals) in [
        (
            "pitch edit",
            2,
            note(60, 1.0, 2.5),
            &[(0, 240, 1.0), (288, 384, 1.0)][..],
        ),
        ("start edit", 3, note(60, 0.5, 0.5), &[(0, 384, 1.0)][..]),
        ("length edit", 0, note(60, 3.0, 0.25), &[(0, 312, 1.0)][..]),
    ] {
        commands.push(EngineCommand::Seek(0)).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        engine.process_block(AudioProcessBlock::new(&mut sounding, 2));
        assert!(
            engine.transport().is_playing(),
            "{stage}: active voice probe"
        );
        assert!(
            sounding.iter().all(|sample| (*sample - unit).abs() < 1e-7),
            "{stage}: active voice probe"
        );
        while events.pop().is_ok() {}
        commands
            .push(EngineCommand::EditNote {
                track_id: track,
                clip_id: clip,
                note_index,
                note: replacement,
            })
            .unwrap();
        let edit_allocations = crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut [], 2));
        });
        assert_eq!(
            edit_allocations,
            (0, 0),
            "{stage}: editing original note {note_index}"
        );
        // Finish all note-offs before seeking, so a discontinuity flush cannot
        // affect the independently asserted replay timeline.
        engine.process_block(AudioProcessBlock::new(&mut rendered, 2));
        assert!(
            !engine.transport().is_playing(),
            "{stage}: edited voices finished"
        );
        commands.push(EngineCommand::Seek(0)).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
        assert!(engine.transport().is_playing(), "{stage}: replay started");
        let render_allocations = crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut rendered, 2));
        });
        assert_audio(stage, &rendered, intervals);
        assert_eq!(
            render_allocations,
            (0, 0),
            "{stage}: replaying original note {note_index}"
        );
    }
}

type PublicNoteFixture = (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    rtrb::Consumer<crate::events::EngineEvent>,
    TrackId,
    ClipId,
);

fn held_note(start_beat: f64, duration_beats: f64) -> MidiNote {
    MidiNote {
        pitch: 60,
        velocity: 100,
        start_beat,
        duration_beats,
    }
}

fn public_note_fixture(notes: &[MidiNote], duration_beats: f64) -> PublicNoteFixture {
    let (mut engine, mut commands, events) = AudioEngine::new();
    let track = TrackId::new();
    let clip = ClipId::new();
    for command in [
        EngineCommand::SetSampleRate(96),
        EngineCommand::SetBpm(60.0),
        EngineCommand::add_track(track),
        EngineCommand::SetInstrument {
            track_id: track,
            instrument: Box::new(PitchProbe {
                active: [false; 256],
            }),
        },
        EngineCommand::AddNoteClip {
            track_id: track,
            clip_id: clip,
            position_beats: 0.0,
            duration_beats,
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: duration_beats,
            groove_grid: GrooveGrid::Sixteenth,
        },
    ] {
        commands.push(command).unwrap();
    }
    for &note in notes {
        commands
            .push(EngineCommand::AddNote {
                track_id: track,
                clip_id: clip,
                note,
            })
            .unwrap();
    }
    engine.process_block(AudioProcessBlock::new(&mut [], 2));
    (engine, commands, events, track, clip)
}

fn begin_note_replay(engine: &mut AudioEngine, commands: &mut rtrb::Producer<EngineCommand>) {
    commands.push(EngineCommand::Seek(0)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [], 2));
    assert!(engine.transport().is_playing());
}

fn render_note_blocks(engine: &mut AudioEngine, output: &mut [f32], frames: usize) {
    for block in output.chunks_mut(frames * 2) {
        engine.process_block(AudioProcessBlock::new(block, 2));
    }
}

fn assert_note_intervals(stage: &str, output: &[f32], unit: f32, intervals: &[(usize, usize)]) {
    for (frame, pair) in output.as_chunks::<2>().0.iter().enumerate() {
        let expected = if intervals
            .iter()
            .any(|&(start, end)| (start..end).contains(&frame))
        {
            unit
        } else {
            0.0
        };
        assert!(
            pair.iter().all(|sample| (*sample - expected).abs() < 1e-7),
            "{stage}, frame {frame}: {pair:?} != {expected}"
        );
    }
}

#[test]
fn public_note_growth_keeps_edited_and_removed_clamps_correct_without_callback_allocation() {
    for count in [1, 2, 20, 21, 128] {
        let notes = (0..count)
            .rev()
            .map(|start| held_note(start as f64, 1.5))
            .collect::<Vec<_>>();
        let duration = (count + 3) as f64;
        let (mut engine, mut commands, mut events, track, clip) =
            public_note_fixture(&notes, duration);
        begin_note_replay(&mut engine, &mut commands);
        let mut sounding = [0.0; 24 * 2];
        render_note_blocks(&mut engine, &mut sounding, 96);
        let unit = sounding[0];
        assert!(unit > 0.01);
        assert_note_intervals("growth probe", &sounding, unit, &[(0, 24)]);
        let mut output = vec![0.0; (count + 3) * 96 * 2];
        render_note_blocks(&mut engine, &mut output, 96);
        assert!(!engine.transport().is_playing());
        while events.pop().is_ok() {}

        let note_index = count / 2;
        let original_start = count - 1 - note_index;
        commands
            .push(EngineCommand::EditNote {
                track_id: track,
                clip_id: clip,
                note_index,
                note: held_note(original_start as f64 + 0.25, 1.0),
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                engine.process_block(AudioProcessBlock::new(&mut [], 2));
            }),
            (0, 0),
            "EditNote after {count} public additions"
        );
        begin_note_replay(&mut engine, &mut commands);
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                render_note_blocks(&mut engine, &mut output, 96);
            }),
            (0, 0),
            "render edited {count}-note clip"
        );
        let edited_end = if count == 1 { 120 } else { count * 96 + 48 };
        let edited_start = if original_start == 0 { 24 } else { 0 };
        assert_note_intervals(
            &format!("edit after {count} additions"),
            &output,
            unit,
            &[(edited_start, edited_end)],
        );
        assert!(!engine.transport().is_playing());
        while events.pop().is_ok() {}

        commands
            .push(EngineCommand::RemoveNote {
                track_id: track,
                clip_id: clip,
                note_index,
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                engine.process_block(AudioProcessBlock::new(&mut [], 2));
            }),
            (0, 0),
            "RemoveNote after {count} public additions"
        );
        begin_note_replay(&mut engine, &mut commands);
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                render_note_blocks(&mut engine, &mut output, 96);
            }),
            (0, 0),
            "render removed {count}-note clip"
        );
        let intervals = if count == 1 {
            vec![]
        } else if original_start == 0 {
            vec![(96, count * 96 + 48)]
        } else {
            vec![
                (0, original_start * 96 + 48),
                ((original_start + 1) * 96, count * 96 + 48),
            ]
        };
        assert_note_intervals(
            &format!("remove after {count} additions"),
            &output,
            unit,
            &intervals,
        );
    }
}

#[test]
fn public_note_edit_boundaries_release_voices_and_preserve_audio_across_callback_partitions() {
    let notes = [
        held_note(1.0, 0.75),
        held_note(0.0, 2.5),
        held_note(1.0, 1.0),
    ];
    let mut reference = None;
    for frames in [1, 13, 96, 127] {
        let (mut engine, mut commands, mut events, track, clip) = public_note_fixture(&notes, 4.0);
        let mut output = vec![0.0; 384 * 2];
        begin_note_replay(&mut engine, &mut commands);
        render_note_blocks(&mut engine, &mut output, frames);
        let unit = output[0];
        assert!(unit > 0.01);
        assert_note_intervals("partition baseline", &output, unit, &[(0, 168)]);
        if let Some(expected) = &reference {
            assert_eq!(&output, expected, "{frames}-frame callback baseline");
        } else {
            reference = Some(output.clone());
        }

        let missing_track = TrackId::new();
        let missing_clip = ClipId::new();
        let replacement = MidiNote {
            pitch: 61,
            ..held_note(0.5, 0.25)
        };
        for (stage, command, releases_voice) in [
            (
                "edit missing track",
                EngineCommand::EditNote {
                    track_id: missing_track,
                    clip_id: clip,
                    note_index: 0,
                    note: replacement,
                },
                false,
            ),
            (
                "remove missing track",
                EngineCommand::RemoveNote {
                    track_id: missing_track,
                    clip_id: clip,
                    note_index: 0,
                },
                false,
            ),
            (
                "edit missing clip",
                EngineCommand::EditNote {
                    track_id: track,
                    clip_id: missing_clip,
                    note_index: 0,
                    note: replacement,
                },
                true,
            ),
            (
                "remove missing clip",
                EngineCommand::RemoveNote {
                    track_id: track,
                    clip_id: missing_clip,
                    note_index: 0,
                },
                true,
            ),
            (
                "edit index at length",
                EngineCommand::EditNote {
                    track_id: track,
                    clip_id: clip,
                    note_index: 3,
                    note: replacement,
                },
                true,
            ),
            (
                "remove index at length",
                EngineCommand::RemoveNote {
                    track_id: track,
                    clip_id: clip,
                    note_index: 3,
                },
                true,
            ),
            (
                "edit maximum index",
                EngineCommand::EditNote {
                    track_id: track,
                    clip_id: clip,
                    note_index: usize::MAX,
                    note: replacement,
                },
                true,
            ),
            (
                "remove maximum index",
                EngineCommand::RemoveNote {
                    track_id: track,
                    clip_id: clip,
                    note_index: usize::MAX,
                },
                true,
            ),
        ] {
            begin_note_replay(&mut engine, &mut commands);
            let mut sounding = [0.0; 24 * 2];
            render_note_blocks(&mut engine, &mut sounding, frames);
            assert_note_intervals(stage, &sounding, unit, &[(0, 24)]);
            while events.pop().is_ok() {}
            commands.push(command).unwrap();
            let mut following = [0.0; 16 * 2];
            assert_eq!(
                crate::retirement::tests::allocations(|| {
                    engine.process_block(AudioProcessBlock::new(&mut [], 2));
                    render_note_blocks(&mut engine, &mut following, frames);
                }),
                (0, 0),
                "{stage}, {frames}-frame callbacks"
            );
            // Existing-track commands flush voices even when no note matches;
            // replay separately verifies their note-data no-op contract.
            let intervals = if releases_voice {
                &[][..]
            } else {
                &[(0, 16)][..]
            };
            assert_note_intervals(stage, &following, unit, intervals);
            render_note_blocks(&mut engine, &mut output, frames);
            assert!(!engine.transport().is_playing(), "{stage}: voices finished");
            while events.pop().is_ok() {}
            begin_note_replay(&mut engine, &mut commands);
            assert_eq!(
                crate::retirement::tests::allocations(|| {
                    render_note_blocks(&mut engine, &mut output, frames);
                }),
                (0, 0),
                "{stage}: replay with {frames}-frame callbacks"
            );
            assert_eq!(
                &output,
                reference.as_ref().unwrap(),
                "{stage}: note data changed with {frames}-frame callbacks"
            );
        }

        for note_index in [0, 1] {
            commands
                .push(EngineCommand::RemoveNote {
                    track_id: track,
                    clip_id: clip,
                    note_index,
                })
                .unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| {
                    engine.process_block(AudioProcessBlock::new(&mut [], 2));
                }),
                (0, 0)
            );
        }
        begin_note_replay(&mut engine, &mut commands);
        let mut sounding = [0.0; 24 * 2];
        render_note_blocks(&mut engine, &mut sounding, frames);
        assert_note_intervals("last note is sounding", &sounding, unit, &[(0, 24)]);
        while events.pop().is_ok() {}
        commands
            .push(EngineCommand::RemoveNote {
                track_id: track,
                clip_id: clip,
                note_index: 0,
            })
            .unwrap();
        let mut released = [0.0; 16 * 2];
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                engine.process_block(AudioProcessBlock::new(&mut [], 2));
                render_note_blocks(&mut engine, &mut released, frames);
            }),
            (0, 0),
            "remove last sounding note with {frames}-frame callbacks"
        );
        assert_eq!(released, [0.0; 16 * 2]);
        render_note_blocks(&mut engine, &mut output, frames);
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert!(!engine.transport().is_playing());
        while events.pop().is_ok() {}
        begin_note_replay(&mut engine, &mut commands);
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                render_note_blocks(&mut engine, &mut output, frames);
            }),
            (0, 0),
            "empty clip replay with {frames}-frame callbacks"
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
    }
}
