//! Domain modules: the architecture refactor's core pattern.
//!
//! Each domain owns a slice of application state (defined in
//! `state.rs`), its own message enum, and an `update` function that
//! can only touch its own slice plus the narrow interfaces below.
//! app.rs shrinks to a router. TypeScript mental model: these are
//! Redux slices; `EngineHandle` is an injected service interface.

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

/// Production implementation: wraps the real lock-free command queue.
pub struct EngineTx<'a>(pub &'a mut Option<rtrb::Producer<EngineCommand>>);

impl EngineHandle for EngineTx<'_> {
    fn send(&mut self, cmd: EngineCommand) {
        if let Some(tx) = self.0.as_mut() {
            let _ = tx.push(cmd);
        }
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
