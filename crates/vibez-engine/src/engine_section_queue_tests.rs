//! Quantized Section queueing regressions at the public command/event seam.

use super::*;

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, SectionId, TrackId};
use vibez_core::midi::InstrumentKind;
use vibez_core::perform::{NoteRepeatRate, SectionLaunchQuantization, SwingAmount};

use crate::playback_source::{EngineClip, PreparedPlaybackSource, PreparedSectionPlaybackSource};

fn constant_audio(frames: usize, value: f32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![value; frames]],
        sample_rate: 8,
    })
}

fn source(
    section_id: SectionId,
    track_id: TrackId,
    length_beats: f64,
    value: f32,
) -> Box<PreparedSectionPlaybackSource> {
    Box::new(PreparedSectionPlaybackSource::new(
        section_id,
        length_beats,
        true,
        vec![(
            track_id,
            PreparedPlaybackSource::new(
                vec![EngineClip {
                    id: ClipId::new(),
                    audio: constant_audio(128, value),
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

fn playing_engine(
    value: f32,
    length_beats: f64,
) -> (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    rtrb::Consumer<EngineEvent>,
    TrackId,
) {
    let (mut engine, mut commands, events) = AudioEngine::new();
    let track_id = TrackId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track_id, "Audio".into()))
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(source(
            SectionId::new(),
            track_id,
            length_beats,
            value,
        )))
        .unwrap();
    let mut first_frame = [0.0];
    engine.process(&mut first_frame, 1);
    assert_eq!(first_frame, [value]);
    (engine, commands, events, track_id)
}

fn transition_event(events: &mut rtrb::Consumer<EngineEvent>) -> Option<(SectionId, u64)> {
    std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::SectionTransitioned {
            section_id,
            effective_at_samples,
            ..
        } => Some((section_id, effective_at_samples)),
        _ => None,
    })
}

#[test]
fn immediate_section_launch_reanchors_held_repeat_to_the_section_downbeat() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    commands.push(EngineCommand::SetSampleRate(96)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
    commands
        .push(EngineCommand::SetProjectSwing(SwingAmount::new(0.62)))
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
    commands.push(EngineCommand::Play).unwrap();
    engine.process(&mut vec![0.0; 35 * 2], 2);
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 0,
            track_id,
            pitch: 42,
            velocity: 100,
            rate: NoteRepeatRate::Sixteenth,
        })
        .unwrap();
    engine.process(&mut vec![0.0; 2], 2);
    while events.pop().is_ok() {}

    commands
        .push(EngineCommand::QueueSection {
            prepared: source(SectionId::new(), track_id, 8.0, 0.0),
            quantization: SectionLaunchQuantization::Immediate,
        })
        .unwrap();
    engine.process(&mut vec![0.0; 60 * 2], 2);

    let repeated: Vec<u64> = std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::NoteRepeated {
                effective_at_samples,
                ..
            } => Some(effective_at_samples),
            _ => None,
        })
        .collect();
    assert_eq!(repeated, vec![36, 66, 84]);
}

#[test]
fn one_beat_transition_lands_mid_buffer_with_engine_timestamp() {
    let (mut engine, mut commands, mut events, track_id) = playing_engine(0.25, 8.0);
    while events.pop().is_ok() {}
    let next = SectionId::new();
    commands
        .push(EngineCommand::QueueSection {
            prepared: source(next, track_id, 8.0, 0.75),
            quantization: SectionLaunchQuantization::OneBeat,
        })
        .unwrap();

    let mut output = [0.0; 6];
    engine.process(&mut output, 1);

    assert_eq!(output, [0.25, 0.25, 0.25, 0.75, 0.75, 0.75]);
    assert_eq!(transition_event(&mut events), Some((next, 4)));
}

#[test]
fn immediate_quantization_switches_at_the_callback_start() {
    let (mut engine, mut commands, mut events, track_id) = playing_engine(0.25, 8.0);
    while events.pop().is_ok() {}
    let next = SectionId::new();
    commands
        .push(EngineCommand::QueueSection {
            prepared: source(next, track_id, 8.0, 0.75),
            quantization: SectionLaunchQuantization::Immediate,
        })
        .unwrap();

    let mut output = [0.0; 3];
    engine.process(&mut output, 1);

    assert_eq!(output, [0.75; 3]);
    assert_eq!(transition_event(&mut events), Some((next, 1)));
}

#[test]
fn one_bar_and_end_of_section_use_their_exact_boundaries() {
    for (quantization, expected_boundary) in [
        (SectionLaunchQuantization::OneBar, 16),
        (SectionLaunchQuantization::EndOfSection, 8),
    ] {
        let (mut engine, mut commands, mut events, track_id) = playing_engine(0.2, 2.0);
        while events.pop().is_ok() {}
        let next = SectionId::new();
        commands
            .push(EngineCommand::QueueSection {
                prepared: source(next, track_id, 4.0, 0.8),
                quantization,
            })
            .unwrap();

        let mut output = [0.0; 18];
        engine.process(&mut output, 1);

        let old_frames = (expected_boundary - 1) as usize;
        assert!(output[..old_frames]
            .iter()
            .all(|sample| (*sample - 0.2).abs() < f32::EPSILON));
        assert!(output[old_frames..]
            .iter()
            .all(|sample| (*sample - 0.8).abs() < f32::EPSILON));
        assert_eq!(
            transition_event(&mut events),
            Some((next, expected_boundary))
        );
    }
}

#[test]
fn requeue_returns_stale_residency_and_only_latest_section_transitions() {
    let (mut engine, mut commands, mut events, track_id) = playing_engine(0.1, 8.0);
    while events.pop().is_ok() {}
    let stale = SectionId::new();
    let latest = SectionId::new();
    commands
        .push(EngineCommand::QueueSection {
            prepared: source(stale, track_id, 8.0, 0.5),
            quantization: SectionLaunchQuantization::OneBar,
        })
        .unwrap();
    commands
        .push(EngineCommand::QueueSection {
            prepared: source(latest, track_id, 8.0, 0.9),
            quantization: SectionLaunchQuantization::OneBeat,
        })
        .unwrap();

    let mut output = [0.0; 6];
    engine.process(&mut output, 1);

    let queued: Vec<_> = std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::SectionQueued {
                section_id,
                effective_at_samples,
                retired,
            } => Some((
                section_id,
                effective_at_samples,
                retired.map(|item| item.section_id),
            )),
            EngineEvent::SectionTransitioned {
                section_id,
                effective_at_samples,
                ..
            } => Some((section_id, effective_at_samples, None)),
            _ => None,
        })
        .collect();
    assert_eq!(
        queued,
        vec![
            (stale, 16, None),
            (latest, 4, Some(stale)),
            (latest, 4, None),
        ]
    );
    assert!(output[3..]
        .iter()
        .all(|sample| (*sample - 0.9).abs() < f32::EPSILON));
}

#[test]
fn perform_bpm_is_locked_and_live_mute_survives_transition() {
    let (mut engine, mut commands, _events, track_id) = playing_engine(0.4, 8.0);
    commands
        .push(EngineCommand::SetTrackMute(track_id, true))
        .unwrap();
    commands.push(EngineCommand::SetBpm(140.0)).unwrap();
    commands
        .push(EngineCommand::QueueSection {
            prepared: source(SectionId::new(), track_id, 8.0, 0.7),
            quantization: SectionLaunchQuantization::OneBeat,
        })
        .unwrap();

    let mut output = [1.0; 6];
    engine.process(&mut output, 1);

    assert_eq!(engine.transport().bpm(), 120.0);
    assert!(engine.tracks()[0].mute);
    assert_eq!(output, [0.0; 6]);
}
