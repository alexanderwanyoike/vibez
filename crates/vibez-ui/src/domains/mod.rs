//! Domain modules: the architecture refactor's core pattern.
//!
//! Each domain owns a slice of application state (defined in
//! `state.rs`), its own message enum, and an `update` function that
//! can only touch its own slice plus the narrow interfaces below.
//! app.rs shrinks to a router. TypeScript mental model: these are
//! Redux slices; `EngineHandle` is an injected service interface.

use std::collections::VecDeque;

pub mod arrangement;
pub mod audio_recording;
pub mod audio_settings;
pub mod automation;
pub mod browser;
pub mod compensation;
pub mod compressor;
pub mod device_latency;
pub mod device_levels;
pub mod device_presets;
pub mod devices;
pub mod eq;
pub mod filter;
pub mod gain;
pub mod gate;
pub mod param_entry;
pub mod perform;
pub mod piano_roll;
pub mod project;
pub mod sidechain;
pub mod timeline_editor;
pub mod transport;
pub mod view;

use vibez_engine::commands::EngineCommand;

/// The one way domains talk to the audio engine. A trait (interface)
/// instead of the concrete channel so tests can inject a recorder
/// and assert on the exact commands a message produced.
pub trait EngineHandle {
    fn send(&mut self, cmd: EngineCommand);
    /// The rate the engine will run at when it drains the next command.
    fn sample_rate(&self) -> u32;

    fn insert_built_in_effect(
        &mut self,
        track_id: vibez_core::id::TrackId,
        effect_id: vibez_core::id::EffectId,
        effect_type: vibez_core::effect::EffectType,
    ) {
        let command = EngineCommand::insert_built_in_effect(
            track_id,
            effect_id,
            effect_type,
            self.sample_rate(),
        );
        self.send(command);
    }

    fn set_built_in_instrument(
        &mut self,
        track_id: vibez_core::id::TrackId,
        kind: vibez_core::midi::InstrumentKind,
    ) {
        let command = EngineCommand::set_built_in_instrument(track_id, kind, self.sample_rate());
        self.send(command);
    }
}

/// Ordered UI-side delivery for the bounded real-time engine queue.
///
/// Large edits such as a long Perform Capture can expand into more commands
/// than fit in one audio callback's ring buffer. Overflow stays on the UI
/// thread and is retried on later ticks; new commands join the back of the
/// same queue so transport actions cannot overtake unfinished project sync.
pub struct EngineCommandQueue {
    producer: Option<rtrb::Producer<EngineCommand>>,
    pending: VecDeque<EngineCommand>,
    device_owners: std::collections::HashMap<
        (vibez_core::id::TrackId, Option<vibez_core::id::EffectId>),
        usize,
    >,
    effect_chain_edited: bool,
    sample_rate: u32,
}

impl Default for EngineCommandQueue {
    fn default() -> Self {
        Self {
            producer: None,
            pending: VecDeque::new(),
            device_owners: Default::default(),
            effect_chain_edited: false,
            sample_rate: vibez_engine::engine::DEFAULT_SAMPLE_RATE,
        }
    }
}

impl EngineCommandQueue {
    pub fn new(producer: rtrb::Producer<EngineCommand>) -> Self {
        Self {
            producer: Some(producer),
            ..Self::default()
        }
    }

    /// Whether an effect-chain edit was sent since the last call; routing must
    /// be republished after one even when its model is unchanged.
    pub fn take_effect_chain_edit(&mut self) -> bool {
        std::mem::take(&mut self.effect_chain_edited)
    }

    pub fn owns_reconfiguration(
        &self,
        device: &vibez_engine::engine::reconfiguration::DeviceReconfiguration,
    ) -> bool {
        self.device_owners
            .get(&(device.track_id(), device.effect_id()))
            == Some(&device.owner_identity())
    }

    fn record_device_owner(&mut self, command: &EngineCommand) {
        use vibez_engine::engine::reconfiguration::{
            effect_owner_identity, instrument_owner_identity,
        };
        let invalidation = command.device_owner_invalidation();
        if invalidation != vibez_engine::command_ownership::DeviceOwnerInvalidation::None {
            self.device_owners
                .retain(|(track, effect), _| !invalidation.matches(*track, *effect));
        }
        match command {
            EngineCommand::InsertEffect {
                track_id,
                effect_id,
                effect,
                ..
            } => {
                self.device_owners.insert(
                    (*track_id, Some(*effect_id)),
                    effect_owner_identity(&**effect),
                );
            }
            EngineCommand::SetInstrument {
                track_id,
                instrument,
            } => {
                self.device_owners
                    .insert((*track_id, None), instrument_owner_identity(&**instrument));
            }
            _ => {}
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
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn send(&mut self, cmd: EngineCommand) {
        // Lifetime changes take effect in the UI even when the command ring is
        // full; a delayed native handoff cannot reactivate a replaced owner.
        self.record_device_owner(&cmd);
        self.effect_chain_edited |= cmd.edits_effect_chain();
        if let EngineCommand::SetSampleRate(rate) = &cmd {
            self.sample_rate = *rate;
        }
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
    fn sample_rate(&self) -> u32 {
        self.0.sample_rate()
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
    fn sample_rate(&self) -> u32 {
        vibez_engine::engine::DEFAULT_SAMPLE_RATE
    }
}

#[cfg(test)]
mod engine_command_queue_tests {
    use super::*;

    #[test]
    fn built_in_devices_use_the_rate_last_forwarded_to_the_engine() {
        let (producer, mut consumer) = rtrb::RingBuffer::new(4);
        let mut queue = EngineCommandQueue::new(producer);
        assert_eq!(
            queue.sample_rate(),
            vibez_engine::engine::DEFAULT_SAMPLE_RATE
        );
        queue.send(EngineCommand::SetSampleRate(96_000));
        assert_eq!(queue.sample_rate(), 96_000);
        queue.insert_built_in_effect(
            vibez_core::id::TrackId::new(),
            vibez_core::id::EffectId::new(),
            vibez_core::effect::EffectType::Reverb,
        );
        assert!(matches!(
            consumer.pop(),
            Ok(EngineCommand::SetSampleRate(96_000))
        ));
        let Ok(EngineCommand::InsertEffect { effect, .. }) = consumer.pop() else {
            panic!("expected a constructed built-in device");
        };
        assert_eq!(effect.effect_type(), vibez_core::effect::EffectType::Reverb);
    }

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
        fn sample_rate(&self) -> u32 {
            vibez_engine::engine::DEFAULT_SAMPLE_RATE
        }
    }
}
