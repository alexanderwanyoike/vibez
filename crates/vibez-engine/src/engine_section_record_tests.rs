//! Section Record boundary regressions at the public command/event seam.

use super::*;

use crate::playback_source::{
    EngineClip, EngineNoteClip, PreparedPlaybackSource, PreparedSectionPlaybackSource,
};
use std::sync::{Arc, Mutex};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, SectionId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};
use vibez_core::perform::{GrooveGrid, NoteRepeatRate, SwingAmount};

struct NoteLogInstrument(Arc<Mutex<Vec<(bool, u8)>>>);

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

fn source(
    section_id: SectionId,
    track_id: TrackId,
    value: f32,
) -> Box<PreparedSectionPlaybackSource> {
    let audio = Arc::new(DecodedAudio {
        channels: vec![vec![value; 128]],
        sample_rate: 8,
    });
    Box::new(PreparedSectionPlaybackSource::new(
        section_id,
        8.0,
        true,
        vec![(
            track_id,
            PreparedPlaybackSource::new(
                vec![EngineClip {
                    id: ClipId::new(),
                    audio,
                    position: 0,
                    source_offset: 0,
                    duration: 128,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: 0,
                }],
                Vec::new(),
                Vec::new(),
            ),
        )],
    ))
}

fn record_events(events: &mut rtrb::Consumer<EngineEvent>) -> Vec<(bool, u64, u64)> {
    std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::SectionRecordArmed {
                effective_at_samples,
                section_position_samples,
                ..
            } => Some((false, effective_at_samples, section_position_samples)),
            EngineEvent::SectionRecordStarted {
                effective_at_samples,
                section_position_samples,
                ..
            } => Some((true, effective_at_samples, section_position_samples)),
            _ => None,
        })
        .collect()
}

#[test]
fn stopped_count_in_variants_activate_section_at_exact_boundary() {
    for (count_in_bars, boundary) in [(0, 0), (1, 16), (2, 32)] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track_id = TrackId::new();
        let section_id = SectionId::new();
        commands.push(EngineCommand::SetSampleRate(8)).unwrap();
        commands.push(EngineCommand::SetBpm(120.0)).unwrap();
        commands
            .push(EngineCommand::AddTrack(track_id, "Audio".into()))
            .unwrap();
        commands
            .push(EngineCommand::ArmSectionRecord {
                section_id,
                track_id,
                prepared: Some(source(section_id, track_id, 0.75)),
                count_in_bars,
                replace_existing: false,
            })
            .unwrap();

        let frames = boundary as usize + 4;
        let mut output = vec![0.0; frames];
        engine.process(&mut output, 1);

        if boundary > 0 {
            assert!((output[0] - 0.32).abs() < f32::EPSILON);
            assert!(output[..boundary as usize]
                .iter()
                .all(|sample| sample.abs() <= 0.32));
        }
        assert!(output[boundary as usize..]
            .iter()
            .all(|sample| (*sample - 0.75).abs() < f32::EPSILON));
        assert_eq!(
            record_events(&mut events),
            vec![(false, boundary, 0), (true, boundary, 0)]
        );
    }
}

#[test]
fn stopped_count_in_ignores_the_idle_performance_clock_before_transport_reset() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();

    engine.process(&mut [0.0; 40], 1);
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id,
            prepared: Some(source(section_id, track_id, 0.75)),
            count_in_bars: 1,
            replace_existing: false,
        })
        .unwrap();

    let mut count_in_prefix = [0.0; 4];
    engine.process(&mut count_in_prefix, 1);
    assert_eq!(record_events(&mut events), vec![(false, 16, 0)]);
    assert!((count_in_prefix[0] - 0.32).abs() < f32::EPSILON);
    assert!(count_in_prefix.iter().all(|sample| sample.abs() <= 0.32));

    let mut boundary_block = [0.0; 13];
    engine.process(&mut boundary_block, 1);
    assert!(boundary_block[..12]
        .iter()
        .all(|sample| sample.abs() <= 0.32));
    assert!((boundary_block[12] - 0.75).abs() < f32::EPSILON);
    assert_eq!(record_events(&mut events), vec![(true, 16, 0)]);
}

#[test]
fn stopped_count_in_does_not_play_arrangement_content() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    let arrangement_audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.5; 128]],
        sample_rate: 8,
    });
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddClip {
            track_id,
            clip_id: ClipId::new(),
            audio: arrangement_audio,
            position: 0,
            source_offset: 0,
            duration: 128,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        })
        .unwrap();
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id,
            prepared: Some(source(section_id, track_id, 0.75)),
            count_in_bars: 1,
            replace_existing: false,
        })
        .unwrap();

    let mut output = [0.0; 17];
    engine.process(&mut output, 1);

    assert!(
        (output[0] - 0.32).abs() < f32::EPSILON,
        "count-in downbeat must contain only the click, got {}",
        output[0]
    );
    assert!(output[1..4]
        .iter()
        .all(|sample| sample.abs() < f32::EPSILON));
    assert!((output[4] - 0.22).abs() < f32::EPSILON);
    assert!((output[8] - 0.22).abs() < f32::EPSILON);
    assert!((output[12] - 0.22).abs() < f32::EPSILON);
    for sample in [5, 6, 7, 9, 10, 11, 13, 14, 15] {
        assert!(output[sample].abs() < f32::EPSILON);
    }
    assert!((output[16] - 0.75).abs() < f32::EPSILON);
}

#[test]
fn stopped_count_in_keeps_live_instrument_monitoring() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8_000)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::AddMidiTrack(track_id, "MIDI".into()))
        .unwrap();
    commands
        .push(EngineCommand::SetTrackInstrument(
            track_id,
            InstrumentKind::SubtractiveSynth,
        ))
        .unwrap();
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id,
            prepared: Some(source(section_id, track_id, 0.0)),
            count_in_bars: 1,
            replace_existing: false,
        })
        .unwrap();
    commands
        .push(EngineCommand::ExternalNoteOn {
            track_id,
            pitch: 60,
            velocity: 100,
        })
        .unwrap();

    let mut output = vec![0.0; 2_000];
    engine.process(&mut output, 2);

    assert!(
        output[600..].iter().any(|sample| sample.abs() > 0.001),
        "live instrument input must remain audible between count-in clicks"
    );
}

#[test]
fn stopped_count_in_click_accents_each_bar_and_clicks_each_beat() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8_000)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id,
            prepared: Some(source(section_id, track_id, 0.75)),
            count_in_bars: 2,
            replace_existing: false,
        })
        .unwrap();

    let mut output = vec![0.0; 32_001];
    engine.process(&mut output, 1);

    assert!((output[0] - 0.32).abs() < f32::EPSILON);
    assert!((output[4_000] - 0.22).abs() < f32::EPSILON);
    assert!((output[8_000] - 0.22).abs() < f32::EPSILON);
    assert!((output[12_000] - 0.22).abs() < f32::EPSILON);
    assert!((output[16_000] - 0.32).abs() < f32::EPSILON);
    assert!((output[20_000] - 0.22).abs() < f32::EPSILON);
    assert!(output[1_000].abs() < f32::EPSILON);
    assert!((output[32_000] - 0.75).abs() < f32::EPSILON);
}

#[test]
fn playing_arm_uses_next_section_bar_without_restart() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(source(
            section_id, track_id, 0.5,
        )))
        .unwrap();
    engine.process(&mut [0.0], 1);
    while events.pop().is_ok() {}

    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id,
            prepared: None,
            count_in_bars: 0,
            replace_existing: false,
        })
        .unwrap();
    let mut output = [0.0; 18];
    engine.process(&mut output, 1);

    assert!(output
        .iter()
        .all(|sample| (*sample - 0.5).abs() < f32::EPSILON));
    assert_eq!(
        record_events(&mut events),
        vec![(false, 16, 16), (true, 16, 16)]
    );
}

#[test]
fn note_at_block_start_boundary_is_reported_after_recording_starts() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::AddMidiTrack(track_id, "MIDI".into()))
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(source(
            section_id, track_id, 0.0,
        )))
        .unwrap();
    engine.process(&mut [0.0], 1);
    while events.pop().is_ok() {}

    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id,
            prepared: None,
            count_in_bars: 0,
            replace_existing: false,
        })
        .unwrap();
    engine.process(&mut [0.0; 15], 1);
    while events.pop().is_ok() {}

    commands
        .push(EngineCommand::ExternalNoteOn {
            track_id,
            pitch: 42,
            velocity: 100,
        })
        .unwrap();
    engine.process(&mut [0.0], 1);

    let boundary_events: Vec<_> = std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::SectionRecordStarted {
                effective_at_samples,
                ..
            } => Some(("record-started", effective_at_samples)),
            EngineEvent::InstrumentNoteInput {
                pitch,
                on: true,
                effective_at_samples,
                ..
            } if pitch == 42 => Some(("note-on", effective_at_samples)),
            _ => None,
        })
        .collect();

    assert_eq!(
        boundary_events,
        vec![("record-started", 16), ("note-on", 16)]
    );
}

#[test]
fn stop_reports_the_engine_timestamp_and_local_playhead() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id,
            prepared: Some(source(section_id, track_id, 0.5)),
            count_in_bars: 0,
            replace_existing: false,
        })
        .unwrap();
    engine.process(&mut [0.0; 5], 1);
    while events.pop().is_ok() {}
    commands.push(EngineCommand::StopSectionRecord).unwrap();
    engine.process(&mut [0.0], 1);

    let stopped = std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::SectionRecordStopped {
            effective_at_samples,
            section_position_samples,
            started,
            ..
        } => Some((effective_at_samples, section_position_samples, started)),
        _ => None,
    });
    assert_eq!(stopped, Some((5, 5, true)));
}

#[test]
fn repeated_note_reports_swung_truth_and_straight_canonical_position() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(96)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
    commands
        .push(EngineCommand::SetProjectSwing(SwingAmount::new(0.75)))
        .unwrap();
    commands
        .push(EngineCommand::AddMidiTrack(track_id, "Hats".into()))
        .unwrap();
    commands
        .push(EngineCommand::SetTrackInstrument(
            track_id,
            InstrumentKind::SubtractiveSynth,
        ))
        .unwrap();
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id,
            prepared: Some(source(section_id, track_id, 0.0)),
            count_in_bars: 0,
            replace_existing: false,
        })
        .unwrap();
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 0,
            track_id,
            pitch: 42,
            velocity: 100,
            rate: NoteRepeatRate::Sixteenth,
        })
        .unwrap();
    engine.process(&mut vec![0.0; 40 * 2], 2);

    let repeated = std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::NoteRepeated {
            effective_at_samples,
            canonical_at_samples,
            section_position_samples,
            canonical_section_position_samples,
            rate,
            ..
        } if effective_at_samples > 0 => Some((
            effective_at_samples,
            canonical_at_samples,
            section_position_samples,
            canonical_section_position_samples,
            rate,
        )),
        _ => None,
    });
    assert_eq!(
        repeated,
        Some((36, 24, Some(36), Some(24), NoteRepeatRate::Sixteenth))
    );
}

#[test]
fn replace_silences_resident_notes_only_until_the_first_section_wrap() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    let note_events = Arc::new(Mutex::new(Vec::new()));
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
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
                section_id,
                8.0,
                true,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        Vec::new(),
                        vec![EngineNoteClip::new(
                            ClipId::new(),
                            0.0,
                            8.0,
                            vec![
                                MidiNote {
                                    pitch: 62,
                                    velocity: 100,
                                    start_beat: 0.5,
                                    duration_beats: 0.25,
                                },
                                MidiNote {
                                    pitch: 61,
                                    velocity: 100,
                                    start_beat: 4.5,
                                    duration_beats: 0.25,
                                },
                            ],
                            false,
                            0.0,
                            0.0,
                            GrooveGrid::Off,
                        )],
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();

    engine.process(&mut [0.0], 1);
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id,
            prepared: None,
            count_in_bars: 0,
            replace_existing: true,
        })
        .unwrap();
    engine.process(&mut [0.0; 36], 1);
    let active_record = engine.active_section_record.unwrap();
    assert_eq!(active_record.effective_at_samples, 16);
    assert!(
        !active_record.replace_first_pass,
        "Replace first pass remained active at Section wrap: {active_record:?}"
    );
    let events = note_events.lock().unwrap();
    assert_eq!(
        events.iter().filter(|event| **event == (true, 62)).count(),
        2,
        "resident notes before Replace and after its first wrap remain audible: {events:?}"
    );
    assert!(
        !events.contains(&(true, 61)),
        "resident notes crossed during the Replace pass are silent"
    );
}

#[test]
fn replace_keeps_live_input_and_note_repeat_audible() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let section_id = SectionId::new();
    let note_events = Arc::new(Mutex::new(Vec::new()));
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
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
                section_id,
                8.0,
                true,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        Vec::new(),
                        vec![EngineNoteClip::new(
                            ClipId::new(),
                            0.0,
                            8.0,
                            vec![MidiNote {
                                pitch: 60,
                                velocity: 100,
                                start_beat: 0.0,
                                duration_beats: 8.0,
                            }],
                            false,
                            0.0,
                            0.0,
                            GrooveGrid::Off,
                        )],
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();
    engine.process(&mut [0.0], 1);
    commands
        .push(EngineCommand::ArmSectionRecord {
            section_id,
            track_id,
            prepared: None,
            count_in_bars: 0,
            replace_existing: true,
        })
        .unwrap();
    engine.process(&mut [0.0; 15], 1);
    note_events.lock().unwrap().clear();

    commands
        .push(EngineCommand::ExternalNoteOn {
            track_id,
            pitch: 65,
            velocity: 100,
        })
        .unwrap();
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 7,
            track_id,
            pitch: 66,
            velocity: 100,
            rate: NoteRepeatRate::Sixteenth,
        })
        .unwrap();
    engine.process(&mut [0.0], 1);

    let events = note_events.lock().unwrap();
    assert!(
        events.contains(&(false, 60)),
        "the resident note is stopped"
    );
    assert!(events.contains(&(true, 65)), "live input remains audible");
    assert!(
        events.contains(&(true, 66)),
        "Note Repeat remains audible during Replace"
    );
}
