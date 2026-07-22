//! Section Record boundary regressions at the public command/event seam.

use super::*;

use crate::playback_source::{EngineClip, PreparedPlaybackSource, PreparedSectionPlaybackSource};
use std::sync::Arc;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, SectionId, TrackId};
use vibez_core::midi::InstrumentKind;
use vibez_core::perform::{NoteRepeatRate, SwingAmount};

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
            })
            .unwrap();

        let frames = boundary as usize + 4;
        let mut output = vec![0.0; frames];
        engine.process(&mut output, 1);

        assert!(output[..boundary as usize]
            .iter()
            .all(|sample| *sample == 0.0));
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
