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
}

impl FakeStream {
    fn playing() -> Self {
        Self {
            playing: Cell::new(true),
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
        if self.refuses_to_play {
            return Err(PlayStreamError::DeviceNotAvailable);
        }
        self.playing.set(true);
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        self.playing.set(false);
        Ok(())
    }
}

#[test]
fn replacement_takes_over_from_a_live_stream() {
    let mut outgoing = Some(FakeStream::playing());
    let replacement = FakeStream::default();
    let mut previous_stream_running = true;

    let result = hand_over(&mut outgoing, &replacement, &mut previous_stream_running);

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

    let result = hand_over(&mut outgoing, &replacement, &mut previous_stream_running);

    assert!(matches!(result, Err(AudioStreamError::StreamOpen(_))));
    assert!(outgoing.expect("the resumed stream is kept").playing.get());
    assert!(previous_stream_running);
}

#[test]
fn replacement_starts_when_nothing_was_playing() {
    let mut outgoing = None;
    let replacement = FakeStream::default();
    let mut previous_stream_running = false;

    let result = hand_over(&mut outgoing, &replacement, &mut previous_stream_running);

    assert!(result.is_ok(), "{result:?}");
    assert!(replacement.playing.get());
    assert!(!previous_stream_running);
}
