//! The clip launcher remembers what each track was playing when the
//! transport stops, and plays it again when the transport starts.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;
use vibez_engine::events::EngineEvent;

use crate::domains::transport::TransportMsg;
use crate::message::Message;
use crate::state::Workspace;

fn queued_clips(commands: &mut rtrb::Consumer<EngineCommand>) -> Vec<(TrackId, Option<ClipId>)> {
    let mut queued = Vec::new();
    while let Ok(command) = commands.pop() {
        if let EngineCommand::QueueClips { clips, .. } = command {
            queued.extend(clips.iter().map(|clip| (clip.track_id, clip.clip_id)));
        }
    }
    queued
}

#[test]
fn play_resumes_the_clips_that_were_playing_when_the_transport_stopped() {
    for play in [TransportMsg::TogglePlayback, TransportMsg::Play] {
        let mut app = super::test_support::app();
        let (commands, mut sent) = rtrb::RingBuffer::new(64);
        app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
        let (mut events, received) = rtrb::RingBuffer::new(16);
        app.event_rx = Some(received);
        app.state.view.workspace = Workspace::Perform;
        app.state.perform.layout = vibez_project::PerformLayout::Clips;
        let track = TrackId::new();
        let clip = crate::domains::perform::clip_record::empty_midi_clip(
            ClipId::new(),
            track,
            0,
            "Pattern 1".into(),
            4.0,
        );
        Arc::make_mut(&mut app.state.perform.clips)
            .clips
            .push(clip.clone());
        app.state.perform.clip_editor.running = true;
        app.state
            .perform
            .clip_editor
            .playing
            .insert(track, clip.clone());
        app.state.transport.playing = true;

        events.push(EngineEvent::PlaybackStopped).unwrap();
        app.poll_engine_events();
        assert!(app.state.perform.clip_editor.playing.is_empty());
        let _ = queued_clips(&mut sent);

        let _ = app.update(Message::Transport(play.clone()));
        assert_eq!(
            queued_clips(&mut sent),
            vec![(track, Some(clip.id))],
            "{play:?} plays again what was playing"
        );
    }
}
