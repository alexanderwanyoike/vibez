//! Domain modules: the architecture refactor's core pattern.
//!
//! Each domain owns a slice of application state (defined in
//! `state.rs`), its own message enum, and an `update` function that
//! can only touch its own slice plus the narrow interfaces below.
//! app.rs shrinks to a router. TypeScript mental model: these are
//! Redux slices; `EngineHandle` is an injected service interface.

use std::collections::VecDeque;

pub mod arrangement;
pub mod automation;
pub mod browser;
pub mod devices;
pub mod perform;
pub mod piano_roll;
pub mod project;
pub mod timeline_editor;
pub mod transport;
pub mod view;

use vibez_engine::commands::EngineCommand;

/// The one way domains talk to the audio engine. A trait (interface)
/// instead of the concrete channel so tests can inject a recorder
/// and assert on the exact commands a message produced.
pub trait EngineHandle {
    fn send(&mut self, cmd: EngineCommand);
}

/// Ordered UI-side delivery for the bounded real-time engine queue.
///
/// Large edits such as a long Perform Capture can expand into more commands
/// than fit in one audio callback's ring buffer. Overflow stays on the UI
/// thread and is retried on later ticks; new commands join the back of the
/// same queue so transport actions cannot overtake unfinished project sync.
#[derive(Default)]
pub struct EngineCommandQueue {
    producer: Option<rtrb::Producer<EngineCommand>>,
    pending: VecDeque<EngineCommand>,
}

impl EngineCommandQueue {
    pub fn new(producer: rtrb::Producer<EngineCommand>) -> Self {
        Self {
            producer: Some(producer),
            pending: VecDeque::new(),
        }
    }

    pub fn flush(&mut self) {
        let Some(producer) = self.producer.as_mut() else {
            self.pending.clear();
            return;
        };
        while let Some(command) = self.pending.pop_front() {
            match producer.push(command) {
                Ok(()) => {}
                Err(rtrb::PushError::Full(command)) => {
                    self.pending.push_front(command);
                    break;
                }
            }
        }
    }

    #[cfg(test)]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

impl EngineHandle for EngineCommandQueue {
    fn send(&mut self, cmd: EngineCommand) {
        let Some(producer) = self.producer.as_mut() else {
            return;
        };
        if !self.pending.is_empty() {
            self.pending.push_back(cmd);
            return;
        }
        if let Err(rtrb::PushError::Full(cmd)) = producer.push(cmd) {
            self.pending.push_back(cmd);
        }
    }
}

/// Production implementation: wraps the real lock-free command queue.
pub struct EngineTx<'a>(pub &'a mut EngineCommandQueue);

impl EngineHandle for EngineTx<'_> {
    fn send(&mut self, cmd: EngineCommand) {
        self.0.send(cmd);
    }
}

/// Editor target whose content is not currently resident in the audio engine.
///
/// Section authoring uses this until the Section playback adapter lands. The
/// shared editor still performs the same state transitions, but its Arrange
/// synchronization commands must not mutate the audible Arrange source.
#[derive(Default)]
pub struct DiscardingEngine;

impl EngineHandle for DiscardingEngine {
    fn send(&mut self, _cmd: EngineCommand) {}
}

#[cfg(test)]
mod engine_command_queue_tests {
    use super::*;

    #[test]
    fn overflow_and_new_commands_keep_fifo_order_across_flushes() {
        let (producer, mut consumer) = rtrb::RingBuffer::new(1);
        let mut queue = EngineCommandQueue::new(producer);

        queue.send(EngineCommand::SetBpm(100.0));
        queue.send(EngineCommand::SetBpm(110.0));
        queue.send(EngineCommand::Play);
        assert_eq!(queue.pending_len(), 2);

        assert!(matches!(consumer.pop(), Ok(EngineCommand::SetBpm(100.0))));
        queue.flush();
        assert_eq!(queue.pending_len(), 1);
        assert!(matches!(consumer.pop(), Ok(EngineCommand::SetBpm(110.0))));
        queue.flush();
        assert_eq!(queue.pending_len(), 0);
        assert!(matches!(consumer.pop(), Ok(EngineCommand::Play)));
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    /// Test double: records every command instead of sending it.
    #[derive(Default)]
    pub struct RecordingEngine(pub Vec<EngineCommand>);

    impl EngineHandle for RecordingEngine {
        fn send(&mut self, cmd: EngineCommand) {
            self.0.push(cmd);
        }
    }
}
