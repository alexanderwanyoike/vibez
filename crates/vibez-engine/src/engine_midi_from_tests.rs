//! MIDI From plays the notes one track's instrument outputs on another track's instrument, at their exact frames.

use super::*;
use graph_test_support::{graph_engine, routing_channel, routing_plan};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use vibez_core::midi::InstrumentKind;
use vibez_core::note_output::{BlockNote, NoteOutput};
use vibez_core::routing::RoutingChannel;

const BLOCK: usize = 256;

/// Plays like a phrase plugin: each render outputs the next block of notes.
struct NotePlayer {
    script: Arc<Mutex<VecDeque<Vec<BlockNote>>>>,
    output: NoteOutput,
}

/// `(absolute frame, on, pitch)` as the receiving instrument heard them.
type Heard = Arc<Mutex<Vec<(u64, bool, u8)>>>;

struct Listener {
    heard: Heard,
    rendered: u64,
    batch: bool,
}

impl vibez_instruments::Instrument for NotePlayer {
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
    fn note_on(&mut self, _: u8, _: u8) {}
    fn note_off(&mut self, _: u8) {}
    fn render(&mut self, _: &mut [f32], _: usize) {
        self.output.clear();
        for note in self.script.lock().unwrap().pop_front().unwrap_or_default() {
            self.output.push(note);
        }
    }
    fn note_output(&self) -> Option<&NoteOutput> {
        Some(&self.output)
    }
    fn reset(&mut self) {}
    fn supports_batch_render(&self) -> bool {
        true
    }
}

impl vibez_instruments::Instrument for Listener {
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
        self.heard
            .lock()
            .unwrap()
            .push((self.rendered, true, pitch));
    }
    fn note_off(&mut self, pitch: u8) {
        self.heard
            .lock()
            .unwrap()
            .push((self.rendered, false, pitch));
    }
    fn note_on_at(&mut self, pitch: u8, _: u8, frame: u32) {
        let at = self.rendered + u64::from(frame);
        self.heard.lock().unwrap().push((at, true, pitch));
    }
    fn note_off_at(&mut self, pitch: u8, frame: u32) {
        let at = self.rendered + u64::from(frame);
        self.heard.lock().unwrap().push((at, false, pitch));
    }
    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        self.rendered += (buffer.len() / channels) as u64;
    }
    fn reset(&mut self) {}
    fn supports_batch_render(&self) -> bool {
        self.batch
    }
}

/// `(on, pitch, sample)` per input event.
type Inputs = Vec<(bool, u8, u64)>;

struct Rig {
    engine: AudioEngine,
    commands: rtrb::Producer<EngineCommand>,
    events: rtrb::Consumer<EngineEvent>,
    script: Arc<Mutex<VecDeque<Vec<BlockNote>>>>,
    heard: Heard,
    source: TrackId,
    receiver: TrackId,
}

impl Rig {
    /// The receiver is listed first, so only MIDI From can order the source ahead.
    fn channels(&self, routed: bool) -> Vec<RoutingChannel> {
        let mut receiving = routing_channel(self.receiver);
        receiving.midi_from = routed.then_some(self.source);
        vec![
            receiving,
            routing_channel(self.source),
            routing_channel(TrackId::MASTER),
        ]
    }

    fn new(batch: bool) -> Self {
        let (source, receiver) = (TrackId::new(), TrackId::new());
        let script = Arc::new(Mutex::new(VecDeque::new()));
        let heard = Heard::default();
        let mut rig = Self {
            engine: AudioEngine::new().0,
            commands: AudioEngine::new().1,
            events: AudioEngine::new().2,
            script: Arc::clone(&script),
            heard: Arc::clone(&heard),
            source,
            receiver,
        };
        (rig.engine, rig.commands, rig.events) = graph_engine(&rig.channels(true));
        for (track, instrument) in [
            (
                receiver,
                Box::new(Listener {
                    heard,
                    rendered: 0,
                    batch,
                }) as Box<dyn vibez_instruments::Instrument>,
            ),
            (
                source,
                Box::new(NotePlayer {
                    script,
                    output: NoteOutput::default(),
                }),
            ),
        ] {
            rig.commands.push(EngineCommand::add_track(track)).unwrap();
            rig.commands
                .push(EngineCommand::SetInstrument {
                    track_id: track,
                    instrument,
                })
                .unwrap();
        }
        rig
    }

    fn play(&mut self, blocks: Vec<Vec<BlockNote>>) {
        for block in blocks {
            self.script.lock().unwrap().push_back(block);
            self.engine.process(&mut [0.0; BLOCK * 2], 2);
        }
    }

    fn heard(&self) -> Vec<(u64, bool, u8)> {
        self.heard.lock().unwrap().clone()
    }

    /// `(on, pitch, sample)` for the receiver's recorded source input and
    /// for its heard instrument input.
    fn inputs(&mut self) -> (Inputs, Inputs) {
        let (mut source, mut heard) = (Vec::new(), Vec::new());
        while let Ok(event) = self.events.pop() {
            match event {
                EngineEvent::SourceNoteInput(input) if input.track_id == self.receiver => {
                    source.push((input.on, input.pitch, input.position.effective_at_samples));
                }
                EngineEvent::InstrumentNoteInput(input) if input.track_id == self.receiver => {
                    heard.push((input.on, input.pitch, input.effective_at_samples));
                }
                _ => {}
            }
        }
        (source, heard)
    }
}

#[test]
fn the_receiver_plays_routed_notes_at_their_frames_on_either_render_path() {
    for batch in [true, false] {
        let mut rig = Rig::new(batch);

        rig.play(vec![
            vec![BlockNote::on(10, 60, 100)],
            vec![BlockNote::off(44, 60)],
        ]);

        assert_eq!(
            rig.heard(),
            [(10, true, 60), (BLOCK as u64 + 44, false, 60)],
            "batch {batch}"
        );
    }
}

#[test]
fn routed_notes_play_while_the_transport_runs_on_either_render_path() {
    for batch in [true, false] {
        let mut rig = Rig::new(batch);
        rig.commands.push(EngineCommand::Play).unwrap();

        rig.play(vec![
            vec![BlockNote::on(10, 60, 100)],
            vec![BlockNote::off(44, 60)],
        ]);

        assert_eq!(
            rig.heard(),
            [(10, true, 60), (BLOCK as u64 + 44, false, 60)],
            "batch {batch}"
        );
    }
}

#[test]
fn removing_the_route_mid_note_releases_it_at_the_next_block_start() {
    let mut rig = Rig::new(true);
    rig.play(vec![vec![BlockNote::on(10, 60, 100)]]);
    let unrouted = routing_plan(&rig.channels(false));
    rig.commands
        .push(EngineCommand::SetRouting(unrouted))
        .unwrap();

    rig.play(vec![vec![BlockNote::off(44, 60)], vec![]]);

    assert_eq!(rig.heard(), [(10, true, 60), (BLOCK as u64, false, 60)]);
}

#[test]
fn stop_releases_a_routed_note_once() {
    let mut rig = Rig::new(true);
    rig.commands.push(EngineCommand::Play).unwrap();
    rig.play(vec![vec![BlockNote::on(10, 60, 100)]]);
    rig.commands.push(EngineCommand::Stop).unwrap();

    rig.play(vec![vec![], vec![BlockNote::off(44, 60)]]);

    assert_eq!(rig.heard(), [(10, true, 60), (BLOCK as u64, false, 60)]);
}

#[test]
fn routed_notes_reach_recording_as_the_receivers_input_at_their_samples() {
    let mut rig = Rig::new(true);
    rig.commands.push(EngineCommand::Play).unwrap();

    rig.play(vec![
        vec![BlockNote::on(10, 60, 100)],
        vec![BlockNote::off(44, 60)],
    ]);

    let expected = [(true, 60, 10), (false, 60, BLOCK as u64 + 44)];
    let (source, heard) = rig.inputs();
    assert_eq!(source, expected);
    assert_eq!(heard, expected);
}

/// Plays the same notes every block, without touching the allocator.
struct LoopingPlayer {
    notes: [BlockNote; 3],
    output: NoteOutput,
}

impl vibez_instruments::Instrument for LoopingPlayer {
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
    fn note_on(&mut self, _: u8, _: u8) {}
    fn note_off(&mut self, _: u8) {}
    fn render(&mut self, _: &mut [f32], _: usize) {
        self.output.clear();
        for note in self.notes {
            self.output.push(note);
        }
    }
    fn note_output(&self) -> Option<&NoteOutput> {
        Some(&self.output)
    }
    fn reset(&mut self) {}
}

#[test]
fn a_routed_block_allocates_nothing() {
    let (source, receiver) = (TrackId::new(), TrackId::new());
    let mut receiving = routing_channel(receiver);
    receiving.midi_from = Some(source);
    let (mut engine, mut commands, mut events) = graph_engine(&[
        receiving,
        routing_channel(source),
        routing_channel(TrackId::MASTER),
    ]);
    for (track, instrument) in [
        (
            receiver,
            Box::new(crate::test_support::DelayProbeInstrument::new(0, 1.0))
                as Box<dyn vibez_instruments::Instrument>,
        ),
        (
            source,
            Box::new(LoopingPlayer {
                notes: [
                    BlockNote::on(3, 60, 90),
                    BlockNote::on(9, 60, 90),
                    BlockNote::on(12, 64, 90),
                ],
                output: NoteOutput::default(),
            }),
        ),
    ] {
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands
            .push(EngineCommand::SetInstrument {
                track_id: track,
                instrument,
            })
            .unwrap();
    }
    commands.push(EngineCommand::Play).unwrap();
    let mut buffer = [0.0; BLOCK * 2];
    engine.process(&mut buffer, 2);
    while events.pop().is_ok() {}
    commands.push(EngineCommand::Stop).unwrap();

    let measured = crate::retirement::tests::allocations(|| {
        engine.process(&mut buffer, 2);
        engine.process(&mut buffer, 2);
    });

    assert_eq!(measured, (0, 0));
}

#[test]
fn a_muted_source_still_plays_its_notes_on_the_receiver() {
    let mut rig = Rig::new(true);
    let source = rig.source;
    rig.commands
        .push(EngineCommand::SetTrackMute(source, true))
        .unwrap();

    rig.play(vec![vec![BlockNote::on(10, 60, 100)]]);

    assert_eq!(rig.heard(), [(10, true, 60)]);
}

#[test]
fn replacing_the_sources_instrument_mid_note_releases_it_at_the_next_block_start() {
    let mut rig = Rig::new(true);
    rig.play(vec![vec![BlockNote::on(10, 60, 100)]]);
    rig.commands
        .push(EngineCommand::SetInstrument {
            track_id: rig.source,
            instrument: Box::new(NotePlayer {
                script: Arc::clone(&rig.script),
                output: NoteOutput::default(),
            }),
        })
        .unwrap();

    rig.play(vec![vec![], vec![]]);

    assert_eq!(rig.heard(), [(10, true, 60), (BLOCK as u64, false, 60)]);
}

#[test]
fn removing_the_source_track_mid_note_releases_it_at_the_next_block_start() {
    let mut rig = Rig::new(true);
    rig.play(vec![vec![BlockNote::on(10, 60, 100)]]);
    rig.commands
        .push(EngineCommand::RemoveTrack(rig.source))
        .unwrap();

    rig.play(vec![vec![], vec![]]);

    assert_eq!(rig.heard(), [(10, true, 60), (BLOCK as u64, false, 60)]);
}

#[test]
fn a_stop_the_engine_makes_itself_releases_a_routed_note_once() {
    let mut rig = Rig::new(true);
    rig.commands.push(EngineCommand::Play).unwrap();
    rig.play(vec![vec![BlockNote::on(10, 60, 100)]]);
    rig.commands.push(EngineCommand::UnloadAudio).unwrap();

    rig.play(vec![vec![], vec![BlockNote::off(44, 60)]]);

    assert_eq!(rig.heard(), [(10, true, 60), (BLOCK as u64, false, 60)]);
}

#[test]
fn stop_and_play_in_one_callback_still_release_a_routed_note() {
    let mut rig = Rig::new(true);
    rig.commands.push(EngineCommand::Play).unwrap();
    rig.play(vec![vec![BlockNote::on(10, 60, 100)]]);
    rig.commands.push(EngineCommand::Stop).unwrap();
    rig.commands.push(EngineCommand::Play).unwrap();

    rig.play(vec![vec![], vec![]]);

    assert_eq!(rig.heard(), [(10, true, 60), (BLOCK as u64, false, 60)]);
}

#[test]
fn removing_the_sources_instrument_mid_note_releases_it_at_the_next_block_start() {
    let mut rig = Rig::new(true);
    rig.play(vec![vec![BlockNote::on(10, 60, 100)]]);
    rig.commands
        .push(EngineCommand::RemoveTrackInstrument(rig.source))
        .unwrap();

    rig.play(vec![vec![], vec![]]);

    assert_eq!(rig.heard(), [(10, true, 60), (BLOCK as u64, false, 60)]);
}
