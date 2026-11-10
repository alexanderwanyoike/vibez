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
    let mut previous = Some(FakeStream::playing());
    let replacement = FakeStream::default();

    let result = hand_over(&mut previous, |stream| stream, &replacement);

    assert!(result.is_ok(), "{result:?}");
    assert!(replacement.playing.get());
    assert!(!previous.expect("a paused stream is kept").playing.get());
}

#[test]
fn live_stream_resumes_when_the_replacement_cannot_start() {
    let mut previous = Some(FakeStream::playing());
    let replacement = FakeStream::refusing_to_play();

    let failure = hand_over(&mut previous, |stream| stream, &replacement)
        .expect_err("the replacement refuses to play");

    assert!(matches!(failure.error, AudioStreamError::StreamOpen(_)));
    assert!(failure.previous_running);
    assert!(previous.expect("the resumed stream is kept").playing.get());
}

#[test]
fn replacement_starts_when_nothing_was_playing() {
    let mut previous: Option<FakeStream> = None;
    let replacement = FakeStream::default();

    let result = hand_over(&mut previous, |stream| stream, &replacement);

    assert!(result.is_ok(), "{result:?}");
    assert!(replacement.playing.get());
}

#[test]
fn dead_previous_stream_does_not_block_the_replacement() {
    let mut previous = Some(FakeStream::dead());
    let replacement = FakeStream::default();

    let result = hand_over(&mut previous, |stream| stream, &replacement);

    assert!(result.is_ok(), "{result:?}");
    assert!(replacement.playing.get());
    assert!(previous.is_none(), "a dead stream is dropped");
}

#[test]
fn dead_previous_stream_is_not_reported_running_when_the_replacement_cannot_start() {
    let mut previous = Some(FakeStream::dead());
    let replacement = FakeStream::refusing_to_play();

    let failure = hand_over(&mut previous, |stream| stream, &replacement)
        .expect_err("the replacement refuses to play");

    assert!(matches!(failure.error, AudioStreamError::StreamOpen(_)));
    assert!(!failure.previous_running);
    assert!(previous.is_none(), "a dead stream is dropped");
}
