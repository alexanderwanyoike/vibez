//! Deterministic render-read fixtures for opt-in MIDI clip Groove.

use super::*;
use std::sync::{Arc, Mutex};
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};
use vibez_core::perform::{GrooveGrid, SwingAmount, SwingOffset};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimedEvent {
    On(u32, u8),
    Off(u32, u8),
}

struct TimedSpy {
    events: Arc<Mutex<Vec<TimedEvent>>>,
}

impl vibez_instruments::Instrument for TimedSpy {
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
        self.events.lock().unwrap().push(TimedEvent::On(0, pitch));
    }

    fn note_off(&mut self, pitch: u8) {
        self.events.lock().unwrap().push(TimedEvent::Off(0, pitch));
    }

    fn note_on_at(&mut self, pitch: u8, _velocity: u8, frame_offset: u32) {
        self.events
            .lock()
            .unwrap()
            .push(TimedEvent::On(frame_offset, pitch));
    }

    fn note_off_at(&mut self, pitch: u8, frame_offset: u32) {
        self.events
            .lock()
            .unwrap()
            .push(TimedEvent::Off(frame_offset, pitch));
    }

    fn render(&mut self, _buffer: &mut [f32], _channels: usize) {}

    fn reset(&mut self) {}

    fn supports_batch_render(&self) -> bool {
        true
    }
}

#[allow(clippy::too_many_arguments)]
fn render_events(
    grid: GrooveGrid,
    swing: SwingAmount,
    track_offset: Option<SwingOffset>,
    automation_offset: Option<SwingOffset>,
    duration_beats: f64,
    looping: bool,
    loop_end_beats: f64,
    notes: &[MidiNote],
    frames: usize,
) -> Vec<TimedEvent> {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    let clip_id = ClipId::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    commands.push(EngineCommand::SetSampleRate(96)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
    commands
        .push(EngineCommand::SetProjectSwing(swing))
        .unwrap();
    commands
        .push(EngineCommand::AddMidiTrack(track_id, "Groove probe".into()))
        .unwrap();
    commands
        .push(EngineCommand::SetPluginInstrument {
            track_id,
            instrument: Box::new(TimedSpy {
                events: Arc::clone(&events),
            }),
        })
        .unwrap();
    if let Some(offset) = track_offset {
        commands
            .push(EngineCommand::SetTrackSwingOffset(track_id, Some(offset)))
            .unwrap();
    }
    if let Some(offset) = automation_offset {
        let mut lane = AutomationLane::new(AutomationTarget::TrackSwingOffset);
        lane.insert_point(AutomationPoint {
            beat: 0.0,
            value: offset.normalized(),
            curve: 0.0,
        });
        commands
            .push(EngineCommand::SetAutomationLane { track_id, lane })
            .unwrap();
    }
    commands
        .push(EngineCommand::AddNoteClip {
            track_id,
            clip_id,
            position_beats: 0.0,
            duration_beats,
            loop_enabled: looping,
            loop_start_beats: 0.0,
            loop_end_beats,
            groove_grid: grid,
        })
        .unwrap();
    for note in notes {
        commands
            .push(EngineCommand::AddNote {
                track_id,
                clip_id,
                note: *note,
            })
            .unwrap();
    }
    commands.push(EngineCommand::Play).unwrap();
    engine.process(&mut vec![0.0; frames * 2], 2);
    let rendered = events.lock().unwrap().clone();
    rendered
}

fn midpoint_note(start_beat: f64) -> MidiNote {
    MidiNote {
        pitch: 42,
        velocity: 100,
        start_beat,
        duration_beats: 0.05,
    }
}

#[test]
fn opted_in_clip_midpoints_match_the_mpc_96_ppqn_ticks() {
    for (grid, midpoint, fixtures) in [
        (
            GrooveGrid::Sixteenth,
            0.25,
            [(0.50, 24), (0.56, 27), (0.66, 32), (0.75, 36)],
        ),
        (
            GrooveGrid::Eighth,
            0.5,
            [(0.50, 48), (0.56, 54), (0.66, 63), (0.75, 72)],
        ),
    ] {
        for (swing, expected_tick) in fixtures {
            let events = render_events(
                grid,
                SwingAmount::new(swing),
                None,
                None,
                1.0,
                false,
                0.0,
                &[midpoint_note(midpoint)],
                96,
            );
            assert!(
                events.contains(&TimedEvent::On(expected_tick, 42)),
                "{events:?}"
            );
        }
    }
}

#[test]
fn looped_clip_reuses_its_local_grid_without_baking_absolute_positions() {
    let events = render_events(
        GrooveGrid::Sixteenth,
        SwingAmount::new(0.66),
        None,
        None,
        1.0,
        true,
        0.5,
        &[midpoint_note(0.25)],
        96,
    );
    let onsets: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            TimedEvent::On(frame, 42) => Some(*frame),
            _ => None,
        })
        .collect();
    assert_eq!(onsets, vec![32, 80]);
}

#[test]
fn shortened_clip_drops_an_onset_mapped_past_its_boundary() {
    let events = render_events(
        GrooveGrid::Sixteenth,
        SwingAmount::new(0.75),
        None,
        None,
        0.3,
        false,
        0.0,
        &[midpoint_note(0.25)],
        96,
    );
    assert!(!events
        .iter()
        .any(|event| matches!(event, TimedEvent::On(..))));
}

#[test]
fn mapped_note_off_is_clamped_to_the_next_same_pitch_onset() {
    let events = render_events(
        GrooveGrid::Sixteenth,
        SwingAmount::new(0.66),
        None,
        None,
        1.0,
        false,
        0.0,
        &[
            MidiNote {
                pitch: 42,
                velocity: 100,
                start_beat: 0.25,
                duration_beats: 0.4,
            },
            MidiNote {
                pitch: 42,
                velocity: 100,
                start_beat: 0.5,
                duration_beats: 0.25,
            },
        ],
        96,
    );
    assert!(events.contains(&TimedEvent::Off(48, 42)), "{events:?}");
    assert!(events.contains(&TimedEvent::On(48, 42)), "{events:?}");
    assert!(!events
        .iter()
        .any(|event| matches!(event, TimedEvent::Off(frame, 42) if (49..80).contains(frame))));
}

#[test]
fn project_track_offset_shapes_opted_in_clip_playback() {
    let events = render_events(
        GrooveGrid::Sixteenth,
        SwingAmount::new(0.56),
        Some(SwingOffset::new(0.04)),
        None,
        1.0,
        false,
        0.0,
        &[midpoint_note(0.25)],
        96,
    );
    assert!(events.contains(&TimedEvent::On(29, 42)), "{events:?}");
}

#[test]
fn track_swing_automation_shapes_opted_in_clip_playback() {
    let events = render_events(
        GrooveGrid::Sixteenth,
        SwingAmount::new(0.56),
        None,
        Some(SwingOffset::new(0.04)),
        1.0,
        false,
        0.0,
        &[midpoint_note(0.25)],
        96,
    );
    assert!(events.contains(&TimedEvent::On(29, 42)), "{events:?}");
}
