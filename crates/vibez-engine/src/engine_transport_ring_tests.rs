//! Play and Stop overtake content edits waiting for storage, never a control command sent before them.

use super::*;
use crate::clip_insertion::{PreparedNoteClipInsertion, PreparedNoteInsertion};
use crate::transport_ring::TransportSender;
use graph_test_support::{graph_engine, routing_channel};
use vibez_core::id::ClipId;
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;

struct Lanes {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    transport: TransportSender,
}

impl Lanes {
    fn send(&mut self, command: EngineCommand) {
        self.transport.record_main_command(&command);
        assert!(self.commands.push(command).is_ok());
    }

    fn send_transport(&mut self, command: EngineCommand) {
        let stamped = self.transport.stamp(command);
        assert!(self.transport.push(stamped).is_ok());
    }

    fn callback(&mut self) {
        self.engine
            .process_block(AudioProcessBlock::new(&mut [0.0; 256], 2));
    }

    fn serve_storage_requests(&mut self) -> usize {
        let mut served = 0;
        while let Ok(event) = self.events.pop() {
            if let EngineEvent::ClipInsertionStorageRequested(request) = event {
                request.fulfill();
                served += 1;
            }
        }
        served
    }
}

/// A note clip followed by `notes` single-note insertions. Each growth past
/// the prepared storage waits for the UI, so most of them stay queued.
fn lanes_with_pending_notes(notes: usize) -> Lanes {
    let track_id = TrackId::new();
    let (mut engine, commands, events) =
        graph_engine(&[routing_channel(track_id), routing_channel(TrackId::MASTER)]);
    let transport = engine.take_transport_sender().unwrap();
    let mut lanes = Lanes {
        engine,
        commands,
        events,
        transport,
    };
    lanes.send(EngineCommand::add_track(track_id));
    let clip_id = ClipId::new();
    lanes.send(EngineCommand::AddNoteClip {
        track_id,
        insertion: PreparedNoteClipInsertion::new(
            clip_id,
            0.0,
            4.0,
            0.0,
            false,
            0.0,
            0.0,
            GrooveGrid::Off,
        ),
    });
    for index in 0..notes {
        lanes.send(EngineCommand::AddNote {
            track_id,
            clip_id,
            insertion: PreparedNoteInsertion::new(MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: index as f64 / notes as f64,
                duration_beats: 0.1,
            }),
        });
    }
    lanes.callback();
    lanes
}

#[test]
fn play_and_stop_take_effect_in_the_next_callback_behind_pending_content_edits() {
    let pending = RING_BUFFER_CAPACITY - 2;
    let mut lanes = lanes_with_pending_notes(pending);
    assert!(
        lanes.commands.slots() > 0 && lanes.commands.slots() < 8,
        "the content edits are still queued behind the storage request"
    );

    lanes.send_transport(EngineCommand::Play);
    lanes.callback();
    assert!(lanes.engine.transport().is_playing());
    assert!(lanes.engine.transport().position() > 0);

    lanes.send_transport(EngineCommand::Stop);
    lanes.callback();
    assert!(!lanes.engine.transport().is_playing());
    assert!(
        lanes.commands.slots() < 8,
        "content edits are still pending"
    );
}

#[test]
fn a_transport_command_waits_for_a_control_command_sent_before_it() {
    let mut lanes = lanes_with_pending_notes(64);
    lanes.send(EngineCommand::SetBpm(90.0));
    lanes.send_transport(EngineCommand::Play);
    lanes.callback();
    assert!(
        !lanes.engine.transport().is_playing(),
        "Play must not overtake the tempo change queued before it"
    );

    for _ in 0..64 {
        if lanes.serve_storage_requests() == 0 && lanes.engine.transport().is_playing() {
            break;
        }
        lanes.callback();
    }
    assert!(lanes.engine.transport().is_playing());
    assert_eq!(lanes.engine.transport().bpm(), 90.0);
}
