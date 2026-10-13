//! Events reach the presentation queue only on the clock their consumer follows.

use super::*;

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "only heard-time events wait for output latency")]
fn a_source_event_cannot_wait_for_output_latency() {
    let (mut engine, _, _events) = AudioEngine::new();
    engine.present_event_after(
        EngineEvent::ClipRecordStarted {
            clip_id: vibez_core::id::ClipId::new(),
            at: 0,
        },
        64,
    );
}

#[test]
fn a_heard_event_may_wait_for_output_latency() {
    let (mut engine, _, mut events) = AudioEngine::new();
    engine.present_event_after(EngineEvent::PlaybackStopped, 64);
    assert!(events.pop().is_err(), "held until heard");
    engine.output_position = 64;
    engine.flush_presentation();
    assert!(matches!(events.pop(), Ok(EngineEvent::PlaybackStopped)));
}
