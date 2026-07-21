use super::*;
use crate::playback_source::{
    EngineNoteClip, PreparedPlaybackSource, PreparedSectionPlaybackSource,
};
use vibez_core::id::{ClipId, SectionId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};
use vibez_core::perform::{GrooveGrid, GrooveProfile, NoteRepeatRate, SwingAmount, SwingOffset};

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

fn repeat_events(events: &mut rtrb::Consumer<EngineEvent>) -> Vec<(u8, u64)> {
    let mut repeated = Vec::new();
    while let Ok(event) = events.pop() {
        if let EngineEvent::NoteRepeated {
            pitch,
            effective_at_samples,
            ..
        } = event
        {
            repeated.push((pitch, effective_at_samples));
        }
    }
    repeated
}

fn stopped_repeat_engine_at(
    position: usize,
    swing: SwingAmount,
) -> (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    rtrb::Consumer<EngineEvent>,
    TrackId,
) {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    commands.push(EngineCommand::SetSampleRate(96)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
    commands
        .push(EngineCommand::SetProjectSwing(swing))
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
    engine.process(&mut [], 2);
    engine.process(&mut vec![0.0; position * 2], 2);
    while events.pop().is_ok() {}
    (engine, commands, events, track_id)
}

#[test]
fn stopped_repeat_press_anchors_a_long_then_short_pair_mid_grid() {
    // Start at an arbitrary point inside the old absolute Swing grid.
    let (mut engine, mut commands, mut events, track_id) =
        stopped_repeat_engine_at(29, SwingAmount::new(0.62));

    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 0,
            track_id,
            pitch: 42,
            velocity: 100,
            rate: NoteRepeatRate::Sixteenth,
        })
        .unwrap();
    engine.process(&mut vec![0.0; 60 * 2], 2);

    // The press at 29 is step zero. At 62% the 48-tick pair is split at
    // tick 30, so the audible gaps must be 30 then 18 ticks, never a flam
    // or the reversed short-then-long phase of the old absolute clock.
    assert_eq!(repeat_timestamps(&mut events), vec![29, 59, 77]);
}

#[test]
fn stopped_repeat_pads_share_one_anchor_until_the_last_pad_stops() {
    let (mut engine, mut commands, mut events, track_id) =
        stopped_repeat_engine_at(29, SwingAmount::new(0.62));
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 0,
            track_id,
            pitch: 42,
            velocity: 100,
            rate: NoteRepeatRate::Sixteenth,
        })
        .unwrap();
    engine.process(&mut vec![0.0; 10 * 2], 2);
    assert_eq!(repeat_events(&mut events), vec![(42, 29)]);

    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 1,
            track_id,
            pitch: 43,
            velocity: 100,
            rate: NoteRepeatRate::Sixteenth,
        })
        .unwrap();
    engine.process(&mut vec![0.0; 40 * 2], 2);
    assert_eq!(
        repeat_events(&mut events),
        vec![(42, 59), (43, 59), (42, 77), (43, 77)]
    );

    commands
        .push(EngineCommand::StopNoteRepeat { id: 0, track_id })
        .unwrap();
    commands
        .push(EngineCommand::StopNoteRepeat { id: 1, track_id })
        .unwrap();
    engine.process(&mut [], 2);
    engine.process(&mut vec![0.0; 7 * 2], 2);
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 2,
            track_id,
            pitch: 44,
            velocity: 100,
            rate: NoteRepeatRate::Sixteenth,
        })
        .unwrap();
    engine.process(&mut vec![0.0; 50 * 2], 2);
    assert_eq!(
        repeat_events(&mut events),
        vec![(44, 86), (44, 116), (44, 134)]
    );
}

#[test]
fn playing_repeat_waits_for_the_existing_musical_grid() {
    let (mut engine, mut commands, mut events, track_id) =
        stopped_repeat_engine_at(0, SwingAmount::new(0.62));
    commands.push(EngineCommand::Play).unwrap();
    engine.process(&mut vec![0.0; 29 * 2], 2);
    while events.pop().is_ok() {}

    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 0,
            track_id,
            pitch: 42,
            velocity: 100,
            rate: NoteRepeatRate::Sixteenth,
        })
        .unwrap();
    engine.process(&mut vec![0.0; 60 * 2], 2);

    assert_eq!(repeat_timestamps(&mut events), vec![30, 48, 78]);
}

#[test]
fn swing_changes_preserve_the_stopped_press_anchor() {
    let (mut engine, mut commands, mut events, track_id) =
        stopped_repeat_engine_at(29, SwingAmount::new(0.62));
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 0,
            track_id,
            pitch: 42,
            velocity: 100,
            rate: NoteRepeatRate::Sixteenth,
        })
        .unwrap();
    engine.process(&mut vec![0.0; 10 * 2], 2);
    assert_eq!(repeat_timestamps(&mut events), vec![29]);

    commands
        .push(EngineCommand::SetProjectSwing(SwingAmount::new(0.75)))
        .unwrap();
    engine.process(&mut vec![0.0; 40 * 2], 2);

    assert_eq!(repeat_timestamps(&mut events), vec![65, 77]);
}

#[test]
fn project_and_track_swing_shift_straight_repeat_timestamps() {
    let (mut engine, mut commands, mut events, track_id) = repeat_engine();
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::SetProjectSwing(SwingAmount::new(0.56)))
        .unwrap();
    commands
        .push(EngineCommand::SetTrackSwingOffset(
            track_id,
            Some(SwingOffset::new(0.10)),
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

    // At 100 Hz and 60 BPM, MPC tick 63 lands at sample 66 and the next
    // 96-tick pair boundary lands at sample 100.
    assert_eq!(repeat_timestamps(&mut events), vec![66, 100]);
}

#[test]
fn triplet_repeat_timestamps_ignore_swing() {
    let (mut engine, mut commands, mut events, track_id) = repeat_engine();
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::SetProjectSwing(SwingAmount::new(0.75)))
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

fn clip_engine(
    swing: SwingAmount,
    groove_grid: GrooveGrid,
) -> (AudioEngine, rtrb::Producer<EngineCommand>) {
    let (engine, mut commands, _events) = AudioEngine::new();
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
            groove_grid,
        })
        .unwrap();
    commands
        .push(EngineCommand::AddNote {
            track_id,
            clip_id,
            note: MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.25,
                duration_beats: 0.25,
            },
        })
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    (engine, commands)
}

fn render_clip_with_swing(swing: SwingAmount, groove_grid: GrooveGrid) -> Vec<f32> {
    let (mut engine, _commands) = clip_engine(swing, groove_grid);
    let mut output = vec![0.0; 4_000 * 2];
    engine.process(&mut output, 2);
    output
}

#[test]
fn swing_does_not_change_clip_playback_when_its_grid_is_off() {
    assert_eq!(
        render_clip_with_swing(SwingAmount::STRAIGHT, GrooveGrid::Off),
        render_clip_with_swing(SwingAmount::new(0.75), GrooveGrid::Off)
    );
}

#[test]
fn swing_changes_opted_in_clip_playback_without_rewriting_notes() {
    assert_ne!(
        render_clip_with_swing(SwingAmount::STRAIGHT, GrooveGrid::Sixteenth),
        render_clip_with_swing(SwingAmount::new(0.75), GrooveGrid::Sixteenth)
    );
}

fn render_prepared_section_with_swing(swing: SwingAmount) -> Vec<f32> {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let prepared = PreparedSectionPlaybackSource::new(
        SectionId::new(),
        1.0,
        false,
        vec![(
            track_id,
            PreparedPlaybackSource::new(
                Vec::new(),
                vec![EngineNoteClip::new(
                    ClipId::new(),
                    0.0,
                    1.0,
                    vec![MidiNote {
                        pitch: 60,
                        velocity: 100,
                        start_beat: 0.25,
                        duration_beats: 0.25,
                    }],
                    false,
                    0.0,
                    0.0,
                    GrooveGrid::Sixteenth,
                )],
                Vec::new(),
            ),
        )],
    );
    commands.push(EngineCommand::SetSampleRate(8_000)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands
        .push(EngineCommand::AddMidiTrack(track_id, "Section clip".into()))
        .unwrap();
    commands
        .push(EngineCommand::SetTrackInstrument(
            track_id,
            InstrumentKind::SubtractiveSynth,
        ))
        .unwrap();
    commands
        .push(EngineCommand::SetProjectSwing(swing))
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(prepared)))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = vec![0.0; 4_000 * 2];
    engine.process(&mut output, 2);
    output
}

#[test]
fn prepared_sections_apply_live_swing_at_render_read_time() {
    assert_ne!(
        render_prepared_section_with_swing(SwingAmount::STRAIGHT),
        render_prepared_section_with_swing(SwingAmount::new(0.75))
    );
}

#[test]
fn lowering_swing_mid_pair_does_not_move_an_unsounded_clip_note_into_the_past() {
    let constant = render_clip_with_swing(SwingAmount::new(0.75), GrooveGrid::Sixteenth);
    let (mut engine, mut commands) = clip_engine(SwingAmount::new(0.75), GrooveGrid::Sixteenth);
    let mut before_change = vec![0.0; 1_200 * 2];
    engine.process(&mut before_change, 2);
    commands
        .push(EngineCommand::SetProjectSwing(SwingAmount::STRAIGHT))
        .unwrap();
    let mut after_change = vec![0.0; 2_800 * 2];
    engine.process(&mut after_change, 2);
    before_change.extend(after_change);

    assert_eq!(before_change, constant);
}

#[test]
fn engine_ships_the_versioned_mpc2000xl_profile() {
    assert_eq!(AudioEngine::groove_profile(), GrooveProfile::Mpc2000XlV1);
}
