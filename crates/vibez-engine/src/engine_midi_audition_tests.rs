//! A Browser MIDI audition plays through its target track's instrument or the preview voice, on exact samples, and never records.

use super::*;
use crate::midi_audition::MidiAuditionPattern;
use graph_test_support::{graph_engine, routing_channel};
use note_listener_test_support::{Heard, Listener};
use vibez_core::midi::{InstrumentKind, MidiNote};

const BLOCK: usize = 256;
/// 120 BPM at the engine's default 44.1 kHz.
const SAMPLES_PER_BEAT: f64 = 22_050.0;
const BAR: u64 = 88_200;

fn at(beat: f64) -> u64 {
    (beat * SAMPLES_PER_BEAT).ceil() as u64
}

fn note(pitch: u8, start_beat: f64, duration_beats: f64) -> MidiNote {
    MidiNote {
        pitch,
        velocity: 100,
        start_beat,
        duration_beats,
    }
}

fn pattern(notes: Vec<MidiNote>, length_beats: f64) -> Arc<MidiAuditionPattern> {
    Arc::new(MidiAuditionPattern::new(notes, length_beats))
}

/// Sounds 1.0 on every channel while it holds a note, rendered frame by frame.
struct Tone {
    held: u32,
    heard: Heard,
    rendered: u64,
}

impl vibez_instruments::Instrument for Tone {
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn note_on(&mut self, pitch: u8, _: u8) {
        self.held += 1;
        self.heard
            .lock()
            .unwrap()
            .push((self.rendered, true, pitch));
    }
    fn note_off(&mut self, pitch: u8) {
        self.held = self.held.saturating_sub(1);
        self.heard
            .lock()
            .unwrap()
            .push((self.rendered, false, pitch));
    }
    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        let level = if self.held > 0 { 1.0 } else { 0.0 };
        for sample in buffer.iter_mut() {
            *sample += level;
        }
        self.rendered += (buffer.len() / channels) as u64;
    }
    fn reset(&mut self) {}
}

struct Rig {
    engine: AudioEngine,
    commands: rtrb::Producer<EngineCommand>,
    events: rtrb::Consumer<EngineEvent>,
    heard: Heard,
    voice_heard: Heard,
    track: TrackId,
    /// Every event popped so far, in order.
    seen: Vec<EngineEvent>,
}

impl Rig {
    /// One track; it plays a recording instrument unless `instrument` is false.
    fn new(batch: bool, instrument: bool) -> Self {
        let track = TrackId::new();
        let (engine, mut commands, events) =
            graph_engine(&[routing_channel(track), routing_channel(TrackId::MASTER)]);
        let heard = Heard::default();
        commands.push(EngineCommand::add_track(track)).unwrap();
        if instrument {
            commands
                .push(EngineCommand::SetInstrument {
                    track_id: track,
                    instrument: Box::new(Listener::new(Arc::clone(&heard), batch)),
                })
                .unwrap();
        }
        Self {
            engine,
            commands,
            events,
            heard,
            voice_heard: Heard::default(),
            track,
            seen: Vec::new(),
        }
    }

    fn with_voice(mut self) -> Self {
        self.commands
            .push(EngineCommand::SetAuditionVoice(Box::new(Tone {
                held: 0,
                heard: Arc::clone(&self.voice_heard),
                rendered: 0,
            })))
            .unwrap();
        self
    }

    fn start(&mut self, pattern: Arc<MidiAuditionPattern>, target: Option<TrackId>) {
        self.commands
            .push(EngineCommand::StartMidiAudition {
                pattern,
                target,
                start: AuditionStart::Immediate,
                looped: false,
            })
            .unwrap();
    }

    /// Renders `blocks` blocks and returns the left channel.
    fn run(&mut self, blocks: usize) -> Vec<f32> {
        let mut left = Vec::new();
        for _ in 0..blocks {
            let mut buffer = [0.0; BLOCK * 2];
            self.engine.process(&mut buffer, 2);
            left.extend(buffer.iter().step_by(2));
            while let Ok(event) = self.events.pop() {
                self.seen.push(event);
            }
        }
        left
    }

    fn heard(&self) -> Vec<(u64, bool, u8)> {
        self.heard.lock().unwrap().clone()
    }

    fn voice_heard(&self) -> Vec<(u64, bool, u8)> {
        self.voice_heard.lock().unwrap().clone()
    }

    fn saw(&self, wanted: impl Fn(&EngineEvent) -> bool) -> bool {
        self.seen.iter().any(wanted)
    }
}

#[test]
fn the_target_track_plays_the_audition_on_its_samples_on_either_render_path() {
    for batch in [true, false] {
        let mut rig = Rig::new(batch, true);
        let track = rig.track;
        rig.start(
            pattern(vec![note(60, 0.0, 0.02), note(62, 0.03, 0.01)], 4.0),
            Some(track),
        );

        rig.run(4);

        assert_eq!(
            rig.heard(),
            [
                (0, true, 60),
                (at(0.02), false, 60),
                (at(0.03), true, 62),
                (at(0.04), false, 62),
            ],
            "batch {batch}"
        );
    }
}

#[test]
fn the_target_track_plays_the_audition_while_the_transport_runs() {
    let mut rig = Rig::new(true, true);
    let track = rig.track;
    rig.commands.push(EngineCommand::Play).unwrap();
    rig.start(pattern(vec![note(60, 0.0, 0.02)], 4.0), Some(track));

    rig.run(4);

    assert_eq!(rig.heard(), [(0, true, 60), (at(0.02), false, 60)]);
}

#[test]
fn audition_notes_never_reach_recording() {
    let mut rig = Rig::new(true, true);
    let track = rig.track;
    rig.commands.push(EngineCommand::Play).unwrap();
    rig.start(pattern(vec![note(60, 0.0, 0.02)], 4.0), Some(track));

    rig.run(4);

    assert!(!rig.saw(|event| matches!(
        event,
        EngineEvent::SourceNoteInput(_) | EngineEvent::InstrumentNoteInput(_)
    )));
}

#[test]
fn stop_ends_a_held_audition_note_at_the_next_block_start_once() {
    let mut rig = Rig::new(true, true);
    let track = rig.track;
    rig.start(pattern(vec![note(60, 0.0, 2.0)], 4.0), Some(track));
    rig.run(1);
    rig.commands.push(EngineCommand::StopAudition).unwrap();

    rig.run(2);

    assert_eq!(rig.heard(), [(0, true, 60), (BLOCK as u64, false, 60)]);
    assert!(rig.saw(|event| *event == EngineEvent::AuditionStopped));
}

#[test]
fn a_new_audition_releases_the_notes_the_last_one_held() {
    let mut rig = Rig::new(true, true);
    let track = rig.track;
    rig.start(pattern(vec![note(60, 0.0, 2.0)], 4.0), Some(track));
    rig.run(1);
    rig.start(pattern(vec![note(64, 0.0, 2.0)], 4.0), Some(track));

    rig.run(1);

    assert_eq!(
        rig.heard(),
        [
            (0, true, 60),
            (BLOCK as u64, false, 60),
            (BLOCK as u64, true, 64)
        ]
    );
}

#[test]
fn played_once_the_audition_stops_after_its_length() {
    let mut rig = Rig::new(true, true);
    let track = rig.track;
    rig.start(pattern(vec![note(60, 0.0, 0.02)], 0.04), Some(track));

    rig.run(2);
    assert!(!rig.saw(|event| *event == EngineEvent::AuditionStopped));
    rig.run(3);

    assert!(rig.saw(|event| *event == EngineEvent::AuditionStopped));
    assert_eq!(rig.heard(), [(0, true, 60), (at(0.02), false, 60)]);
}

#[test]
fn the_audition_reports_how_far_it_has_played() {
    let mut rig = Rig::new(true, true);
    let track = rig.track;
    rig.start(pattern(vec![note(60, 0.0, 0.02)], 4.0), Some(track));

    rig.run(2);

    assert!(rig.saw(|event| *event == EngineEvent::AuditionPosition(BLOCK as u64 * 2)));
}

#[test]
fn a_warped_audition_waits_for_the_next_bar_while_the_project_plays() {
    let mut rig = Rig::new(true, true);
    let track = rig.track;
    rig.commands.push(EngineCommand::Seek(BAR - 300)).unwrap();
    rig.commands.push(EngineCommand::Play).unwrap();
    rig.commands
        .push(EngineCommand::StartMidiAudition {
            pattern: pattern(vec![note(60, 0.0, 0.02)], 4.0),
            target: Some(track),
            start: AuditionStart::NextBar,
            looped: true,
        })
        .unwrap();

    rig.run(4);

    assert_eq!(rig.heard(), [(300, true, 60), (300 + at(0.02), false, 60)]);
    let queued = rig
        .seen
        .iter()
        .position(|event| *event == EngineEvent::AuditionQueued);
    let started = rig
        .seen
        .iter()
        .position(|event| *event == EngineEvent::AuditionStarted);
    assert!(queued.is_some() && started > queued);
}

#[test]
fn a_warped_audition_started_while_stopped_moves_to_the_next_bar_when_play_starts() {
    let mut rig = Rig::new(true, true);
    let track = rig.track;
    rig.commands
        .push(EngineCommand::StartMidiAudition {
            pattern: pattern(vec![note(60, 0.0, 2.0)], 4.0),
            target: Some(track),
            start: AuditionStart::NextBar,
            looped: true,
        })
        .unwrap();
    rig.run(1);
    rig.commands.push(EngineCommand::Seek(BAR - 300)).unwrap();
    rig.commands.push(EngineCommand::Play).unwrap();

    rig.run(3);

    assert_eq!(
        rig.heard(),
        [
            (0, true, 60),
            (BLOCK as u64, false, 60),
            (BLOCK as u64 + 300, true, 60)
        ]
    );
}

#[test]
fn without_a_target_the_preview_voice_plays_into_the_output_at_the_audition_gain() {
    let mut rig = Rig::new(true, true).with_voice();
    rig.commands
        .push(EngineCommand::SetAuditionGain(0.5))
        .unwrap();
    rig.start(pattern(vec![note(60, 0.0, 0.01)], 4.0), None);

    let left = rig.run(2);

    let end = at(0.01) as usize;
    assert_eq!(rig.voice_heard(), [(0, true, 60), (end as u64, false, 60)]);
    assert!(left[..end].iter().all(|sample| *sample == 0.5));
    assert!(left[end..].iter().all(|sample| *sample == 0.0));
    assert!(rig.heard().is_empty());
}

#[test]
fn a_target_without_an_instrument_hands_the_audition_to_the_preview_voice() {
    let mut rig = Rig::new(true, false).with_voice();
    let track = rig.track;
    rig.start(pattern(vec![note(60, 0.0, 0.01)], 4.0), Some(track));

    rig.run(2);

    assert_eq!(rig.voice_heard(), [(0, true, 60), (at(0.01), false, 60)]);
}

#[test]
fn a_new_preview_voice_hands_the_old_one_back_for_disposal() {
    let mut rig = Rig::new(true, true).with_voice().with_voice();

    rig.run(1);

    assert!(rig.saw(|event| matches!(event, EngineEvent::DisposeInstrument(_))));
}

#[test]
fn an_audio_audition_ends_a_midi_one() {
    let mut rig = Rig::new(true, true);
    let track = rig.track;
    rig.start(pattern(vec![note(60, 0.0, 2.0)], 4.0), Some(track));
    rig.run(1);
    rig.commands
        .push(EngineCommand::StartAudition {
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![0.25; 4_096]; 2],
                sample_rate: DEFAULT_SAMPLE_RATE,
            }),
            start: AuditionStart::Immediate,
            looped: false,
        })
        .unwrap();

    rig.run(1);

    assert_eq!(rig.heard(), [(0, true, 60), (BLOCK as u64, false, 60)]);
}

#[test]
fn a_midi_audition_fades_out_an_audio_one_without_reporting_a_stop() {
    let mut rig = Rig::new(true, true);
    let track = rig.track;
    rig.commands
        .push(EngineCommand::StartAudition {
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![0.25; 44_100]; 2],
                sample_rate: DEFAULT_SAMPLE_RATE,
            }),
            start: AuditionStart::Immediate,
            looped: false,
        })
        .unwrap();
    rig.run(1);
    rig.start(pattern(vec![note(60, 0.0, 2.0)], 4.0), Some(track));

    let left = rig.run(4);

    assert!(left[BLOCK * 3..].iter().all(|sample| *sample == 0.0));
    assert!(!rig.saw(|event| *event == EngineEvent::AuditionStopped));
}
