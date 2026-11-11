//! The pad and editor playhead follow a playing clip whose length changes.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;
use vibez_engine::events::EngineEvent;

use crate::domains::perform::clip_record::empty_midi_clip;
use crate::domains::perform::LauncherClip;

/// Eight samples a second at 120 BPM, so a beat is four samples.
const SPB: f64 = 4.0;

struct Playing {
    app: crate::app::App,
    events: rtrb::Producer<EngineEvent>,
    _commands: rtrb::Consumer<EngineCommand>,
    longer: LauncherClip,
}

/// A four-beat clip that began at sample 0, with an eight-beat version of
/// it in the store.
fn playing_clip() -> Playing {
    let mut app = crate::app::test_support::app();
    app.state.transport.sample_rate = 8;
    app.state.transport.bpm = 120.0;
    let (events, received) = rtrb::RingBuffer::new(8);
    app.event_rx = Some(received);
    let (commands, sent) = rtrb::RingBuffer::new(8);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(
        commands,
        vibez_engine::transport_ring::transport_ring().0,
    );
    let (id, track) = (ClipId::new(), TrackId::new());
    let short = empty_midi_clip(id, track, 0, "Take".into(), 4.0);
    let longer = empty_midi_clip(id, track, 0, "Take".into(), 8.0);
    Arc::make_mut(&mut app.state.perform.clips)
        .clips
        .push(longer.clone());
    let editor = &mut app.state.perform.clip_editor;
    editor.running = true;
    editor.playing.insert(track, short);
    editor.started_at.insert(track, 0);
    Playing {
        app,
        events,
        _commands: sent,
        longer,
    }
}

#[test]
fn an_edit_acknowledged_after_its_request_retired_still_lengthens_the_playhead() {
    let Playing {
        mut app,
        mut events,
        longer,
        ..
    } = playing_clip();
    let editor = &mut app.state.perform.clip_editor;
    editor.pending.insert(5, longer.clone());
    editor.pending.insert(6, longer.clone());

    // A latent track hears the refresh after both retirements arrive.
    for retired in [6, 5] {
        events
            .push(EngineEvent::ClipRequestRetired(
                longer.prepare(retired, SPB),
            ))
            .unwrap();
    }
    events
        .push(EngineEvent::ClipSourceRefreshed {
            track_id: longer.track_id,
            request_id: 5,
            position: 4,
            effective_at_samples: 20,
        })
        .unwrap();
    app.poll_engine_events();

    let editor = &app.state.perform.clip_editor;
    assert_eq!(editor.playhead_samples(longer.id, 36, SPB), Some(20));
}
