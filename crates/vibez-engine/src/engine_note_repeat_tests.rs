use super::*;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};
use vibez_core::perform::{NoteRepeatRate, SwingAmount, SwingOffset};

fn repeat_engine() -> (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    rtrb::Consumer<EngineEvent>,
    TrackId,
) {
    let (mut engine, mut commands, events) = AudioEngine::new();
    let track_id = TrackId::new();
    commands.push(EngineCommand::SetSampleRate(100)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
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
    let mut bootstrap = [0.0; 2];
    engine.process(&mut bootstrap, 2);
    (engine, commands, events, track_id)
}

fn repeat_timestamps(events: &mut rtrb::Consumer<EngineEvent>) -> Vec<u64> {
    let mut timestamps = Vec::new();
    while let Ok(event) = events.pop() {
        if let EngineEvent::NoteRepeated {
            effective_at_samples,
            ..
        } = event
        {
            timestamps.push(effective_at_samples);
        }
    }
    timestamps
}

#[test]
fn project_and_track_swing_shift_straight_repeat_timestamps() {
    let (mut engine, mut commands, mut events, track_id) = repeat_engine();
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::SetProjectSwing(SwingAmount::new(0.5)))
        .unwrap();
    commands
        .push(EngineCommand::SetTrackSwingOffset(
            track_id,
            Some(SwingOffset::new(0.5)),
        ))
        .unwrap();
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 0,
            track_id,
            pitch: 42,
            velocity: 91,
            rate: NoteRepeatRate::Eighth,
        })
        .unwrap();

    let mut output = vec![0.0; 120 * 2];
    engine.process(&mut output, 2);

    assert_eq!(repeat_timestamps(&mut events), vec![67, 100]);
}

#[test]
fn triplet_repeat_timestamps_ignore_swing() {
    let (mut engine, mut commands, mut events, track_id) = repeat_engine();
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::SetProjectSwing(SwingAmount::new(1.0)))
        .unwrap();
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 0,
            track_id,
            pitch: 42,
            velocity: 100,
            rate: NoteRepeatRate::EighthTriplet,
        })
        .unwrap();

    let mut output = vec![0.0; 110 * 2];
    engine.process(&mut output, 2);

    assert_eq!(repeat_timestamps(&mut events), vec![33, 67, 100]);
}

#[test]
fn rate_change_retimes_future_repeats_without_immediate_retrigger() {
    let (mut engine, mut commands, mut events, track_id) = repeat_engine();
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 0,
            track_id,
            pitch: 42,
            velocity: 100,
            rate: NoteRepeatRate::Quarter,
        })
        .unwrap();
    let mut first = vec![0.0; 109 * 2];
    engine.process(&mut first, 2);
    assert_eq!(repeat_timestamps(&mut events), vec![100]);

    commands
        .push(EngineCommand::UpdateNoteRepeatRate {
            id: 0,
            track_id,
            rate: NoteRepeatRate::Eighth,
        })
        .unwrap();
    let mut second = vec![0.0; 101 * 2];
    engine.process(&mut second, 2);

    assert_eq!(repeat_timestamps(&mut events), vec![150, 200]);
}

#[test]
fn stopping_repeat_prevents_every_future_generated_note() {
    let (mut engine, mut commands, mut events, track_id) = repeat_engine();
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 0,
            track_id,
            pitch: 42,
            velocity: 100,
            rate: NoteRepeatRate::Eighth,
        })
        .unwrap();
    let mut first = vec![0.0; 59 * 2];
    engine.process(&mut first, 2);
    assert_eq!(repeat_timestamps(&mut events), vec![50]);

    commands
        .push(EngineCommand::StopNoteRepeat { id: 0, track_id })
        .unwrap();
    let mut second = vec![0.0; 200 * 2];
    engine.process(&mut second, 2);
    assert!(repeat_timestamps(&mut events).is_empty());
}

fn render_clip_with_swing(swing: SwingAmount) -> Vec<f32> {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let clip_id = ClipId::new();
    commands.push(EngineCommand::SetSampleRate(8_000)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::SetProjectSwing(swing))
        .unwrap();
    commands
        .push(EngineCommand::AddMidiTrack(track_id, "Clip".into()))
        .unwrap();
    commands
        .push(EngineCommand::SetTrackInstrument(
            track_id,
            InstrumentKind::SubtractiveSynth,
        ))
        .unwrap();
    commands
        .push(EngineCommand::AddNoteClip {
            track_id,
            clip_id,
            position_beats: 0.0,
            duration_beats: 1.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
        })
        .unwrap();
    commands
        .push(EngineCommand::AddNote {
            track_id,
            clip_id,
            note: MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 0.5,
            },
        })
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = vec![0.0; 4_000 * 2];
    engine.process(&mut output, 2);
    output
}

#[test]
fn swing_does_not_change_existing_clip_playback() {
    assert_eq!(
        render_clip_with_swing(SwingAmount::STRAIGHT),
        render_clip_with_swing(SwingAmount::new(1.0))
    );
}
