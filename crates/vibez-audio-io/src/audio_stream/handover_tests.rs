//! A stream handover leaves exactly one stream driving the engine, or none.

use super::hand_over;
use crate::audio_stream::AudioStreamError;
use cpal::traits::StreamTrait;
use cpal::{PauseStreamError, PlayStreamError};
use std::cell::Cell;

#[derive(Default)]
struct FakeStream {
    playing: Cell<bool>,
    refuses_to_play: bool,
    dead: bool,
}

impl FakeStream {
    fn playing() -> Self {
        Self {
            playing: Cell::new(true),
            ..Self::default()
        }
    }

    /// A stream whose backend thread has exited, as WASAPI reports after its
    /// device goes away.
    fn dead() -> Self {
        Self {
            dead: true,
            ..Self::default()
        }
    }

    fn refusing_to_play() -> Self {
        Self {
            refuses_to_play: true,
            ..Self::default()
        }
    }
}

impl StreamTrait for FakeStream {
    fn play(&self) -> Result<(), PlayStreamError> {
        if self.refuses_to_play || self.dead {
            return Err(PlayStreamError::DeviceNotAvailable);
        }
        self.playing.set(true);
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        if self.dead {
            return Err(PauseStreamError::DeviceNotAvailable);
        }
        self.playing.set(false);
        Ok(())
    }
}

#[test]
fn replacement_takes_over_from_a_live_stream() {
    let mut outgoing = Some(FakeStream::playing());
    let replacement = FakeStream::default();
    let mut previous_stream_running = true;

    let result = hand_over(
        &mut outgoing,
        |stream| stream,
        &replacement,
        &mut previous_stream_running,
    );

    assert!(result.is_ok(), "{result:?}");
    assert!(replacement.playing.get());
    assert!(!outgoing.expect("a paused stream is kept").playing.get());
    assert!(!previous_stream_running);
}

#[test]
fn live_stream_resumes_when_the_replacement_cannot_start() {
    let mut outgoing = Some(FakeStream::playing());
    let replacement = FakeStream::refusing_to_play();
    let mut previous_stream_running = true;

    let result = hand_over(
        &mut outgoing,
        |stream| stream,
        &replacement,
        &mut previous_stream_running,
    );

    assert!(matches!(result, Err(AudioStreamError::StreamOpen(_))));
    assert!(outgoing.expect("the resumed stream is kept").playing.get());
    assert!(previous_stream_running);
}

#[test]
fn replacement_starts_when_nothing_was_playing() {
    let mut outgoing: Option<FakeStream> = None;
    let replacement = FakeStream::default();
    let mut previous_stream_running = false;

    let result = hand_over(
        &mut outgoing,
        |stream| stream,
        &replacement,
        &mut previous_stream_running,
    );

    assert!(result.is_ok(), "{result:?}");
    assert!(replacement.playing.get());
    assert!(!previous_stream_running);
}

#[test]
fn dead_outgoing_stream_does_not_block_the_replacement() {
    let mut outgoing = Some(FakeStream::dead());
    let replacement = FakeStream::default();
    let mut previous_stream_running = true;

    let result = hand_over(
        &mut outgoing,
        |stream| stream,
        &replacement,
        &mut previous_stream_running,
    );

    assert!(result.is_ok(), "{result:?}");
    assert!(replacement.playing.get());
    assert!(outgoing.is_none(), "a dead stream is dropped");
    assert!(!previous_stream_running);
}

#[test]
fn dead_outgoing_stream_is_not_reported_running_when_the_replacement_cannot_start() {
    let mut outgoing = Some(FakeStream::dead());
    let replacement = FakeStream::refusing_to_play();
    let mut previous_stream_running = true;

    let result = hand_over(
        &mut outgoing,
        |stream| stream,
        &replacement,
        &mut previous_stream_running,
    );

    assert!(
        matches!(result, Err(AudioStreamError::StreamOpen(_))),
        "{result:?}"
    );
    assert!(outgoing.is_none(), "a dead stream is dropped");
    assert!(!previous_stream_running);
}
