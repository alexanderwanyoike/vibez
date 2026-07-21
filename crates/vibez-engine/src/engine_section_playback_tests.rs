//! Section playback regression tests. These exercise the public command/event
//! boundary so the source-switch implementation remains replaceable.

use super::*;

use std::sync::{Arc, Mutex};

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, SectionId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};

use crate::playback_source::{
    EngineClip, EngineNoteClip, PreparedPlaybackSource, PreparedSectionPlaybackSource,
};

struct NoteLogInstrument(Arc<Mutex<Vec<(bool, u8)>>>);

struct TailEffect {
    memory: f32,
}

impl vibez_dsp::effect::AudioEffect for TailEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }

    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }

    fn set_param(&mut self, _index: usize, _value: f32) -> bool {
        false
    }

    fn get_param(&self, _index: usize) -> f32 {
        0.0
    }

    fn process(&mut self, buffer: &mut [f32], _channels: usize) {
        for sample in buffer {
            if *sample != 0.0 {
                self.memory = *sample;
            } else {
                self.memory *= 0.5;
                *sample = self.memory;
            }
        }
    }

    fn reset(&mut self) {
        self.memory = 0.0;
    }
}

impl vibez_instruments::Instrument for NoteLogInstrument {
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }

    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }

    fn set_param(&mut self, _index: usize, _value: f32) -> bool {
        false
    }

    fn get_param(&self, _index: usize) -> f32 {
        0.0
    }

    fn note_on(&mut self, pitch: u8, _velocity: u8) {
        self.0.lock().unwrap().push((true, pitch));
    }

    fn note_off(&mut self, pitch: u8) {
        self.0.lock().unwrap().push((false, pitch));
    }

    fn render(&mut self, _buffer: &mut [f32], _channels: usize) {}

    fn reset(&mut self) {}
}

fn constant_audio(frames: usize, value: f32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![value; frames], vec![value; frames]],
        sample_rate: 44_100,
    })
}

#[test]
fn resident_section_switches_immediately_and_returns_the_displaced_source() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                section_id,
                4.0,
                true,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        vec![EngineClip {
                            id: ClipId::new(),
                            audio: constant_audio(16, 0.25),
                            position: 0,
                            source_offset: 0,
                            duration: 16,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                        }],
                        Vec::new(),
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();

    let mut output = [0.0; 8];
    engine.process(&mut output, 2);

    assert!(engine.transport().is_playing());
    assert!(output.iter().all(|sample| *sample > 0.0));

    let transition = std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::SectionTransitioned {
            section_id: actual,
            effective_at_samples,
            retired,
        } => Some((actual, effective_at_samples, retired)),
        _ => None,
    });
    let (actual, effective_at_samples, retired) = transition.expect("transition event");
    assert_eq!(actual, section_id);
    assert_eq!(effective_at_samples, 0);
    assert_eq!(retired.tracks().len(), 1);
    assert!(retired.tracks()[0].source.clips.is_empty());
}

#[test]
fn refreshing_the_active_section_changes_content_without_restarting_its_playhead() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();

    let source = |value| {
        Box::new(PreparedSectionPlaybackSource::new(
            section_id,
            4.0,
            true,
            vec![(
                track_id,
                PreparedPlaybackSource::new(
                    vec![EngineClip {
                        id: ClipId::new(),
                        audio: constant_audio(32, value),
                        position: 0,
                        source_offset: 0,
                        duration: 32,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: 0,
                    }],
                    Vec::new(),
                    Vec::new(),
                ),
            )],
        ))
    };

    commands
        .push(EngineCommand::LaunchSection(source(0.25)))
        .unwrap();
    let mut before = [0.0; 4];
    engine.process(&mut before, 1);
    assert_eq!(before, [0.25; 4]);

    commands
        .push(EngineCommand::RefreshSection(source(0.75)))
        .unwrap();
    let mut after = [0.0; 4];
    engine.process(&mut after, 1);

    assert_eq!(after, [0.75; 4]);
    let position = engine
        .active_section
        .expect("Section remains active")
        .position_samples;
    assert_eq!(position, 8, "refresh preserves the four elapsed samples");
}

#[test]
fn section_wraps_at_its_shortened_boundary_inside_one_callback() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    let audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.1, 0.2, 0.3, 0.4, 0.9, 0.9]],
        sample_rate: 8,
    });
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                1.0,
                true,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        vec![EngineClip {
                            id: ClipId::new(),
                            audio,
                            position: 0,
                            source_offset: 0,
                            duration: 6,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                        }],
                        Vec::new(),
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();

    // 8 Hz at 120 BPM is four samples per beat, so this callback crosses the
    // shortened one-beat boundary after frame four.
    let mut output = [0.0; 6];
    engine.process(&mut output, 1);

    assert!((output[0] - output[4]).abs() < f32::EPSILON);
    assert!((output[1] - output[5]).abs() < f32::EPSILON);
    assert_ne!(output[2], output[4]);
}

#[test]
fn arrangement_loop_does_not_wrap_section_engine_time() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    commands
        .push(EngineCommand::SetArrangementLoopRegion { start: 0, end: 2 })
        .unwrap();
    commands
        .push(EngineCommand::SetArrangementLoop(true))
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                section_id,
                4.0,
                true,
                vec![(track_id, PreparedPlaybackSource::default())],
            ),
        )))
        .unwrap();

    engine.process(&mut [0.0; 3], 1);

    assert_eq!(engine.transport().position(), 3);
    assert!(
        std::iter::from_fn(|| events.pop().ok()).any(|event| matches!(
            event,
            EngineEvent::SectionPlaybackPosition {
                section_id: actual,
                position_samples: 3,
            } if actual == section_id
        ))
    );
}

#[test]
fn non_looping_section_returns_to_arrangement_source_after_ending() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    engine.process(&mut [], 1);
    engine.tracks[0].playback_source = Box::new(PreparedPlaybackSource::new(
        vec![EngineClip {
            id: ClipId::new(),
            audio: constant_audio(16, 0.75),
            position: 0,
            source_offset: 0,
            duration: 16,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        }],
        Vec::new(),
        Vec::new(),
    ));
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                1.0,
                false,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        vec![EngineClip {
                            id: ClipId::new(),
                            audio: constant_audio(4, 0.25),
                            position: 0,
                            source_offset: 0,
                            duration: 4,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                        }],
                        Vec::new(),
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();

    let mut section_output = [0.0; 4];
    engine.process(&mut section_output, 1);
    assert!(section_output.iter().all(|sample| *sample == 0.25));
    assert!(!engine.transport().is_playing());

    commands.push(EngineCommand::Seek(0)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut arrangement_output = [0.0; 1];
    engine.process(&mut arrangement_output, 1);

    assert_eq!(arrangement_output, [0.75]);
}

#[test]
fn switching_sections_releases_notes_from_the_previous_source() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let note_events = Arc::new(Mutex::new(Vec::new()));
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands
        .push(EngineCommand::AddMidiTrack(track_id, "MIDI".into()))
        .unwrap();
    commands
        .push(EngineCommand::SetPluginInstrument {
            track_id,
            instrument: Box::new(NoteLogInstrument(Arc::clone(&note_events))),
        })
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        Vec::new(),
                        vec![EngineNoteClip::new(
                            ClipId::new(),
                            0.0,
                            4.0,
                            vec![MidiNote {
                                pitch: 60,
                                velocity: 100,
                                start_beat: 0.0,
                                duration_beats: 4.0,
                            }],
                            false,
                            0.0,
                            0.0,
                            vibez_core::perform::GrooveGrid::Off,
                        )],
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();
    let mut output = [0.0; 1];
    engine.process(&mut output, 1);
    assert!(note_events.lock().unwrap().contains(&(true, 60)));

    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(track_id, PreparedPlaybackSource::default())],
            ),
        )))
        .unwrap();
    engine.process(&mut output, 1);

    assert!(note_events.lock().unwrap().contains(&(false, 60)));
}

#[test]
fn live_instrument_note_releases_after_a_section_transition() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let note_events = Arc::new(Mutex::new(Vec::new()));
    commands
        .push(EngineCommand::AddMidiTrack(track_id, "MIDI".into()))
        .unwrap();
    commands
        .push(EngineCommand::SetPluginInstrument {
            track_id,
            instrument: Box::new(NoteLogInstrument(Arc::clone(&note_events))),
        })
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(track_id, PreparedPlaybackSource::default())],
            ),
        )))
        .unwrap();
    commands
        .push(EngineCommand::ExternalNoteOn {
            track_id,
            pitch: 48,
            velocity: 100,
        })
        .unwrap();
    let mut output = [0.0; 1];
    engine.process(&mut output, 1);

    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(track_id, PreparedPlaybackSource::default())],
            ),
        )))
        .unwrap();
    engine.process(&mut output, 1);
    commands
        .push(EngineCommand::ExternalNoteOff {
            track_id,
            pitch: 48,
        })
        .unwrap();
    engine.process(&mut output, 1);

    assert_eq!(*note_events.lock().unwrap(), vec![(true, 48), (false, 48)]);
}

#[test]
fn absent_track_stops_source_content_without_resetting_effect_tail() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id,
            effect_id: EffectId::new(),
            effect: Box::new(TailEffect { memory: 0.0 }),
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        vec![EngineClip {
                            id: ClipId::new(),
                            audio: constant_audio(1, 1.0),
                            position: 0,
                            source_offset: 0,
                            duration: 1,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                        }],
                        Vec::new(),
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();
    let mut source = [0.0; 1];
    engine.process(&mut source, 1);
    assert!(source[0] > 0.0);

    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(track_id, PreparedPlaybackSource::default())],
            ),
        )))
        .unwrap();
    let mut silence_with_tail = [0.0; 2];
    engine.process(&mut silence_with_tail, 1);

    assert!(silence_with_tail.iter().all(|sample| *sample > 0.0));
    assert!(silence_with_tail[0] < source[0]);
    assert!(silence_with_tail[1] < silence_with_tail[0]);
}
