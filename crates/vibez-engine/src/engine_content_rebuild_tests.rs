//! Playback keeps running while undo or Capture re-adds a song's content.

use super::*;
use crate::clip_insertion::PreparedClipInsertion;
use crate::transport_ring::TransportSender;
use graph_test_support::{constant_clip, graph_engine, routing_channel};

const BLOCK: usize = 256;

struct Rebuild {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    transport: TransportSender,
    track_id: TrackId,
}

impl Rebuild {
    fn new() -> Self {
        let track_id = TrackId::new();
        let (mut engine, commands, events) =
            graph_engine(&[routing_channel(track_id), routing_channel(TrackId::MASTER)]);
        let transport = engine.take_transport_sender().unwrap();
        let mut rebuild = Self {
            engine,
            commands,
            events,
            transport,
            track_id,
        };
        rebuild.send(EngineCommand::add_track(track_id));
        rebuild
    }

    fn send(&mut self, command: EngineCommand) {
        self.transport.record_main_command(&command);
        assert!(self.commands.push(command).is_ok());
    }

    fn send_transport(&mut self, command: EngineCommand) {
        let stamped = self.transport.stamp(command);
        assert!(self.transport.push(stamped).is_ok());
    }

    fn add_clip(&mut self, frames: usize) -> vibez_core::id::ClipId {
        let clip = constant_clip(frames, 0.25);
        let clip_id = clip.id;
        self.send(EngineCommand::AddClip {
            insertion: PreparedClipInsertion::new(),
            track_id: self.track_id,
            clip_id,
            audio: clip.audio,
            position: clip.position,
            source_offset: clip.source_offset,
            start_marker: clip.start_marker,
            duration: clip.duration,
            loop_enabled: clip.loop_enabled,
            loop_start: clip.loop_start,
            loop_end: clip.loop_end,
            linear_gain: clip.linear_gain,
            fades: clip.fades,
            playback_direction: clip.playback_direction,
            warp_markers: clip.warp_markers,
        });
        clip_id
    }

    /// One callback, with the UI answering every storage request it raised.
    fn callback(&mut self) {
        self.engine
            .process_block(AudioProcessBlock::new(&mut [0.0; BLOCK * 2], 2));
        while let Ok(event) = self.events.pop() {
            if let EngineEvent::ClipInsertionStorageRequested(request) = event {
                request.fulfill();
            }
        }
    }

    fn play_from(&mut self, position: u64) {
        self.send_transport(EngineCommand::Seek(position));
        self.send_transport(EngineCommand::Play);
        self.callback();
        assert!(self.engine.transport().is_playing());
    }
}

#[test]
fn a_playhead_past_the_end_of_a_partly_re_added_song_keeps_playing() {
    let mut rebuild = Rebuild::new();
    rebuild.add_clip(100_000);
    rebuild.play_from(50_000);

    rebuild.send(EngineCommand::RemoveTrack(rebuild.track_id));
    rebuild.send(EngineCommand::add_track(rebuild.track_id));
    rebuild.add_clip(10_000);
    let before = rebuild.engine.transport().position();
    for _ in 0..4 {
        rebuild.callback();
    }

    assert!(rebuild.engine.transport().is_playing());
    assert_eq!(
        rebuild.engine.transport().position(),
        before + 4 * BLOCK as u64
    );
}

#[test]
fn the_song_end_does_not_stop_playback_until_a_content_rebuild_has_finished() {
    let mut rebuild = Rebuild::new();
    let long = rebuild.add_clip(100_000);
    rebuild.play_from(20_000);

    rebuild.send(EngineCommand::BeginContentRebuild);
    rebuild.send(EngineCommand::RemoveClip(rebuild.track_id, long));
    rebuild.add_clip(21_000);
    for _ in 0..8 {
        rebuild.callback();
    }
    assert!(
        rebuild.engine.transport().position() > 21_000,
        "the playhead crossed the end of what has been re-added so far"
    );
    assert!(rebuild.engine.transport().is_playing());

    rebuild.add_clip(100_000);
    rebuild.send(EngineCommand::EndContentRebuild);
    for _ in 0..4 {
        rebuild.callback();
    }
    assert!(rebuild.engine.transport().is_playing());
    assert_eq!(rebuild.engine.transport().audio_length(), Some(100_000));
}
