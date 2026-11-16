//! A live note held while recording a Clip sounds until its key is released.
//!
//! The UI half of the loop is modelled here exactly as `vibez-ui` does it:
//! every UI tick republishes the take (`refresh_clip_record` ->
//! `ClipRecordState::preview` -> `LoopRecordState::snapshot` ->
//! `push_free_note` -> `apply_midi_take` -> `RefreshClip`).

use super::*;
use crate::playback_source::{EngineNoteClip, PreparedClipPlayback, PreparedPlaybackSource};
use graph_test_support::{graph_engine, routing_channel};
use std::sync::Mutex;
use vibez_core::id::ClipId;
use vibez_core::midi::{InstrumentKind, MidiNote};
use vibez_core::perform::GrooveGrid;

const RATE: u32 = 48_000;
const SPB: f64 = 24_000.0; // 120 BPM
const SIXTEENTH: f64 = 0.25;
const BLOCK: usize = 512;
const UI_TICK_BLOCKS: usize = 2;
const PITCH: u8 = 60;

type Log = Arc<Mutex<(u64, Vec<(u64, u8, bool)>)>>;

struct Probe {
    log: Log,
    batch: bool,
}

impl Probe {
    fn push(&self, offset: u32, pitch: u8, on: bool) {
        let mut log = self.log.lock().unwrap();
        let frame = log.0 + u64::from(offset);
        log.1.push((frame, pitch, on));
    }
}

impl vibez_instruments::Instrument for Probe {
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
        self.push(0, pitch, true);
    }
    fn note_off(&mut self, pitch: u8) {
        self.push(0, pitch, false);
    }
    fn note_on_at(&mut self, pitch: u8, _: u8, offset: u32) {
        self.push(offset, pitch, true);
    }
    fn note_off_at(&mut self, pitch: u8, offset: u32) {
        self.push(offset, pitch, false);
    }
    fn supports_batch_render(&self) -> bool {
        self.batch
    }
    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        self.log.lock().unwrap().0 += (buffer.len() / channels) as u64;
    }
    fn reset(&mut self) {}
}

/// `vibez-ui` `quantize_start`.
fn quantize_start(beat: f64, length_beats: f64) -> f64 {
    ((beat / SIXTEENTH).round() * SIXTEENTH).rem_euclid(length_beats)
}

/// `vibez-ui` `free_recording_beats`.
fn free_recording_beats(elapsed: u64) -> f64 {
    (((elapsed as f64 - 1.0).max(0.0) / SPB / 4.0).ceil() * 4.0).max(4.0)
}

struct Rig {
    engine: AudioEngine,
    commands: rtrb::Producer<EngineCommand>,
    events: rtrb::Consumer<EngineEvent>,
    track: TrackId,
    clip_id: ClipId,
    log: Log,
    /// UI model.
    start: Option<u64>,
    heard_now: u64,
    length: Option<u64>,
    note_on_at: Option<u64>,
    note_off_at: Option<u64>,
    committed: Vec<MidiNote>,
    blocks: usize,
    /// The UI thread runs beside the callback: a refresh built from the last
    /// heard position reaches the engine one callback later.
    in_flight: Option<Box<PreparedClipPlayback>>,
}

impl Rig {
    fn new(batch: bool) -> Self {
        let track = TrackId::new();
        let (mut engine, mut commands, events) =
            graph_engine(&[routing_channel(track), routing_channel(TrackId::MASTER)]);
        let log: Log = Arc::default();
        for command in [
            EngineCommand::SetSampleRate(RATE),
            EngineCommand::SetBpm(120.0),
            EngineCommand::add_track(track),
            EngineCommand::SetInstrument {
                track_id: track,
                instrument: Box::new(Probe {
                    log: Arc::clone(&log),
                    batch,
                }),
            },
        ] {
            commands.push(command).unwrap();
        }
        engine.process(&mut [], 1);
        Self {
            engine,
            commands,
            events,
            track,
            clip_id: ClipId::new(),
            log,
            start: None,
            heard_now: 0,
            length: None,
            note_on_at: None,
            note_off_at: None,
            committed: Vec::new(),
            blocks: 0,
            in_flight: None,
        }
    }

    fn source(
        &self,
        beats: f64,
        notes: Vec<MidiNote>,
        request_id: u64,
    ) -> Box<PreparedClipPlayback> {
        let mut source = PreparedPlaybackSource::default();
        source.push_note_clip(EngineNoteClip::new(
            self.clip_id,
            0.0,
            beats,
            notes,
            0.0,
            true,
            0.0,
            beats,
            GrooveGrid::Sixteenth,
        ));
        Box::new(PreparedClipPlayback {
            track_id: self.track,
            clip_id: Some(self.clip_id),
            request_id,
            length_samples: self.length.unwrap_or((beats * SPB) as u64),
            looping: true,
            launch_mode: Default::default(),
            start_position: 0,
            source: Box::new(source),
        })
    }

    /// `begin_clip_record` with Count-in 1 Bar, Overdub.
    fn arm(&mut self, length: Option<u64>) {
        self.length = length;
        let beats = length.map_or(4.0, |samples| samples as f64 / SPB);
        let mut prepared = self.source(beats, vec![], 1);
        prepared.length_samples = length.unwrap_or(u64::MAX / 4);
        prepared.looping = length.is_some();
        self.commands
            .push(EngineCommand::ArmClipRecord {
                free_length: length.is_none(),
                prepared,
                count_in_bars: 1,
            })
            .unwrap();
    }

    fn local(&self, at: u64) -> u64 {
        let elapsed = at - self.start.unwrap();
        self.length.map_or(elapsed, |length| elapsed % length)
    }

    fn pass_start(&self, at: u64) -> u64 {
        let start = self.start.unwrap();
        self.length
            .map_or(start, |length| start + (at - start) / length * length)
    }

    /// `push_free_note`: quantized start, unquantized duration.
    fn note(&self, on: u64, off: u64) -> MidiNote {
        let length_beats = self.length.map_or(1_000_000.0, |l| l as f64 / SPB);
        MidiNote {
            pitch: PITCH,
            velocity: 100,
            start_beat: quantize_start(self.local(on) as f64 / SPB, length_beats),
            duration_beats: ((off - on) as f64 / SPB).max(1.0 / 960.0),
        }
    }

    /// One UI tick: drain events, then `refresh_clip_record(heard now)`.
    fn ui_tick(&mut self) {
        while let Ok(event) = self.events.pop() {
            match event {
                EngineEvent::ClipRecordStarted { at, .. } => self.start = Some(at),
                EngineEvent::PerformancePosition(position) => self.heard_now = position,
                // `ClipRecordState::note_input` ignores input before the take starts.
                EngineEvent::SourceNoteInput(input)
                    if self
                        .start
                        .is_none_or(|start| input.position.effective_at_samples < start) => {}
                EngineEvent::SourceNoteInput(input) if input.on => {
                    self.note_on_at = Some(input.position.effective_at_samples)
                }
                EngineEvent::SourceNoteInput(input) => {
                    self.note_off_at = Some(input.position.effective_at_samples)
                }
                _ => {}
            }
        }
        let Some(start) = self.start else {
            return;
        };
        let now = self.heard_now;
        if now < start {
            return;
        }
        // `advance_pass`: a fixed-length take commits held notes at each wrap
        // and reopens them at local 0.
        if let (Some(length), Some(on)) = (self.length, self.note_on_at) {
            let pass = self.pass_start(now);
            if self.note_off_at.is_none() && self.pass_start(on) < pass {
                let boundary = pass;
                self.committed.push(self.note(on, boundary));
                self.note_on_at = Some(boundary);
                let _ = length;
            }
        }
        let mut notes = self.committed.clone();
        if let Some(on) = self.note_on_at.filter(|on| *on < now) {
            let off = self.note_off_at.unwrap_or(now);
            notes.push(self.note(on, off));
        }
        let beats = self
            .length
            .map_or_else(|| free_recording_beats(now - start), |l| l as f64 / SPB);
        if std::env::var_os("HOLD_TRACE").is_some() {
            eprintln!(
                "ui tick heard {now} engine {} notes {:?}",
                self.engine.performance_position,
                notes
                    .iter()
                    .map(|n| (n.start_beat * SPB, (n.start_beat + n.duration_beats) * SPB))
                    .collect::<Vec<_>>()
            );
        }
        self.in_flight = Some(self.source(beats, notes, 0));
    }

    fn block(&mut self) {
        let mut output = [0.0; BLOCK];
        self.engine.process(&mut output, 1);
        if let Some(refreshed) = self.in_flight.take() {
            self.commands
                .push(EngineCommand::RefreshClip(refreshed))
                .unwrap();
        }
        self.blocks += 1;
        if self.blocks.is_multiple_of(UI_TICK_BLOCKS) {
            self.ui_tick();
        }
    }

    fn run_until(&mut self, mut done: impl FnMut(&Self) -> bool) {
        for _ in 0..10_000 {
            if done(self) {
                return;
            }
            self.block();
        }
        panic!("condition never reached");
    }

    fn key(&mut self, on: bool) -> u64 {
        let at = self.engine.performance_position;
        let command = if on {
            EngineCommand::ExternalNoteOn {
                track_id: self.track,
                pitch: PITCH,
                velocity: 100,
            }
        } else {
            EngineCommand::ExternalNoteOff {
                track_id: self.track,
                pitch: PITCH,
            }
        };
        self.commands.push(command).unwrap();
        // Map Perform samples onto the probe's output frame counter.
        let frame = self.log.lock().unwrap().0;
        eprintln!(
            "key {} at perform {at} (local {}), output frame {frame}",
            if on { "down" } else { "up" },
            self.start.map_or(0, |_| self.local(at)),
        );
        frame
    }

    fn heard(&self) -> Vec<(u64, u8, bool)> {
        self.log
            .lock()
            .unwrap()
            .1
            .iter()
            .copied()
            .filter(|(_, pitch, _)| *pitch == PITCH)
            .collect()
    }
}

fn assert_one_sustained_note(rig: &Rig, down: u64, up: u64) {
    let heard = rig.heard();
    eprintln!("probe pitch {PITCH} events (output frame, on): {heard:?}");
    assert_eq!(
        heard,
        vec![(down, PITCH, true), (up, PITCH, false)],
        "the held key must sound once from {down} until its release at {up}"
    );
}

/// Press inside a sixteenth so it rounds forward by `ahead` samples.
fn press_rounding_forward(rig: &mut Rig, ahead: u64) -> u64 {
    let step = (SIXTEENTH * SPB) as u64;
    rig.run_until(|rig| {
        rig.start.is_some() && {
            let local = rig.local(rig.engine.performance_position);
            local > step && (step - local % step) <= ahead && (step - local % step) > ahead / 2
        }
    });
    rig.key(true)
}

/// Press just after a sixteenth so quantizing moves it back, never ahead.
fn press_rounding_backward(rig: &mut Rig) -> u64 {
    let step = (SIXTEENTH * SPB) as u64;
    rig.run_until(|rig| {
        rig.start.is_some() && {
            let local = rig.local(rig.engine.performance_position);
            local > step && local % step > 0 && local % step <= 1_000
        }
    });
    rig.key(true)
}

#[test]
fn free_length_take_sustains_a_held_note_that_quantizes_forward() {
    for batch in [false, true] {
        let mut rig = Rig::new(batch);
        rig.arm(None);
        let down = press_rounding_forward(&mut rig, 1_400);
        for _ in 0..20 {
            rig.block();
        }
        let up = rig.key(false);
        for _ in 0..40 {
            rig.block();
        }
        assert_one_sustained_note(&rig, down, up);
    }
}

#[test]
fn free_length_take_sustains_a_held_note_across_a_bar_line() {
    for batch in [false, true] {
        let mut rig = Rig::new(batch);
        rig.arm(None);
        // Exactly on a sixteenth: quantizing does not move it.
        let down = press_rounding_backward(&mut rig);
        // Hold across the first bar line (start + 96000).
        rig.run_until(|rig| rig.engine.performance_position > rig.start.unwrap() + 96_000 + 4_096);
        let up = rig.key(false);
        for _ in 0..40 {
            rig.block();
        }
        assert_one_sustained_note(&rig, down, up);
    }
}

#[test]
fn fixed_length_overdub_sustains_a_held_note_across_the_loop_wrap() {
    for batch in [false, true] {
        let mut rig = Rig::new(batch);
        rig.arm(Some(96_000));
        let down = press_rounding_backward(&mut rig);
        // Hold across the wrap and well into the second pass, past where the
        // first pass pressed the key.
        rig.run_until(|rig| rig.engine.performance_position > rig.start.unwrap() + 96_000 + 30_000);
        let up = rig.key(false);
        for _ in 0..40 {
            rig.block();
        }
        assert_one_sustained_note(&rig, down, up);
    }
}

/// Control: many refreshes alone do not release the live voice (hypothesis 1).
#[test]
fn control_refreshes_within_a_bar_keep_a_note_that_quantizes_back() {
    for batch in [false, true] {
        let mut rig = Rig::new(batch);
        rig.arm(None);
        let down = press_rounding_backward(&mut rig);
        for _ in 0..40 {
            rig.block();
        }
        let up = rig.key(false);
        for _ in 0..40 {
            rig.block();
        }
        assert_one_sustained_note(&rig, down, up);
    }
}

/// Control: a key held through count-in and the record start boundary
/// (hypothesis 3).
#[test]
fn control_a_note_held_through_count_in_and_record_start_sustains() {
    for batch in [false, true] {
        let mut rig = Rig::new(batch);
        rig.arm(None);
        for _ in 0..100 {
            rig.block();
        }
        let down = rig.key(true);
        rig.run_until(|rig| {
            rig.start
                .is_some_and(|start| rig.engine.performance_position > start + 20_000)
        });
        let up = rig.key(false);
        for _ in 0..40 {
            rig.block();
        }
        assert_one_sustained_note(&rig, down, up);
    }
}

#[test]
fn free_length_take_sustains_a_held_note_that_quantizes_far_forward() {
    for batch in [false, true] {
        let mut rig = Rig::new(batch);
        rig.arm(None);
        let down = press_rounding_forward(&mut rig, 2_990);
        for _ in 0..20 {
            rig.block();
        }
        let up = rig.key(false);
        for _ in 0..40 {
            rig.block();
        }
        assert_one_sustained_note(&rig, down, up);
    }
}
