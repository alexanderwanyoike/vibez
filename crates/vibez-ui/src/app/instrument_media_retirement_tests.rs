//! Returned instrument media survives UI consumption until its final shared reference ends.

use super::*;
use std::sync::Arc;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_engine::events::EngineEvent;
use vibez_instruments::InstrumentMediaOwners;

fn sample() -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio { channels: vec![vec![0.25; 128]], sample_rate: 48_000 })
}

#[test]
fn returned_media_keeps_one_ui_guard_while_other_owners_live_and_reaps_afterward() {
    let mut app = test_support::app();
    let (mut events, receiver) = rtrb::RingBuffer::new(64);
    app.event_rx = Some(receiver);
    let voice = sample();
    let weak = Arc::downgrade(&voice);
    for _ in 0..32 {
        events.push(EngineEvent::RetiredInstrumentMedia(InstrumentMediaOwners::incoming(
            Arc::clone(&voice), "repeated-owner.wav".into(),
        ))).unwrap();
    }
    app.poll_engine_events();
    assert_eq!(weak.strong_count(), 2, "one voice and exactly one UI guard");
    app.clear_project_runtime();
    assert_eq!(weak.strong_count(), 2, "project reset cannot discard old-engine guards");
    app.poll_engine_events();
    assert_eq!(weak.strong_count(), 2);
    drop(voice);
    app.poll_engine_events();
    assert_eq!(weak.strong_count(), 0, "duplicate guards must not pin one another");
}

#[test]
fn equal_audio_buffers_are_distinct_guards_and_ui_cache_owners_delay_reaping() {
    let mut app = test_support::app();
    let (mut events, receiver) = rtrb::RingBuffer::new(8);
    app.event_rx = Some(receiver);
    let first = sample();
    let second = sample();
    let weak_first = Arc::downgrade(&first);
    let weak_second = Arc::downgrade(&second);
    let cached = Arc::clone(&first);
    for owner in [&first, &second] {
        events.push(EngineEvent::RetiredInstrumentMedia(InstrumentMediaOwners::incoming(
            Arc::clone(owner), "same-name-and-values.wav".into(),
        ))).unwrap();
    }
    app.poll_engine_events();
    assert_eq!(weak_first.strong_count(), 3);
    assert_eq!(weak_second.strong_count(), 2);
    drop(first);
    drop(second);
    app.poll_engine_events();
    assert_eq!(weak_first.strong_count(), 2, "a real cache owner is still present");
    assert_eq!(weak_second.strong_count(), 0);
    drop(cached);
    app.poll_engine_events();
    assert_eq!(weak_first.strong_count(), 0);
}
