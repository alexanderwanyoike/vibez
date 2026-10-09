use std::cell::UnsafeCell;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::{events::EngineEvent, mixer::EngineTrack};

pub(crate) const RETIREMENT_CAPACITY: usize = 32;
struct ChannelSlot {
    occupied: AtomicBool,
    owner: UnsafeCell<Option<EngineTrack>>,
}
// The audio producer writes only a free slot. Its unique event token publishes
// that owner to the UI, whose final Release makes the empty slot reusable.
unsafe impl Sync for ChannelSlot {}

pub(crate) struct ChannelRetirementPool {
    slots: Box<[ChannelSlot]>,
}
impl ChannelRetirementPool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            slots: (0..RETIREMENT_CAPACITY)
                .map(|_| ChannelSlot {
                    occupied: AtomicBool::new(false),
                    owner: UnsafeCell::new(None),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        })
    }
    pub fn has_capacity(&self) -> bool {
        self.slots
            .iter()
            .any(|slot| !slot.occupied.load(Ordering::Acquire))
    }
    fn retire(self: &Arc<Self>, track: EngineTrack) -> RetiredChannel {
        let index = self
            .slots
            .iter()
            .position(|slot| {
                slot.occupied
                    .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
            })
            .expect("command drain reserves channel retirement capacity");
        unsafe {
            *self.slots[index].owner.get() = Some(track);
        }
        RetiredChannel {
            pool: Arc::clone(self),
            index,
        }
    }
    fn take(&self, index: usize) -> Option<EngineTrack> {
        let slot = &self.slots[index];
        let owner = unsafe { (*slot.owner.get()).take() };
        slot.occupied.store(false, Ordering::Release);
        owner
    }
}

pub struct RetiredChannel {
    pool: Arc<ChannelRetirementPool>,
    index: usize,
}
impl std::fmt::Debug for RetiredChannel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RetiredChannel")
            .field("slot", &self.index)
            .finish()
    }
}
impl Drop for RetiredChannel {
    fn drop(&mut self) {
        drop(self.pool.take(self.index));
    }
}

impl crate::engine::AudioEngine {
    pub(super) fn flush_retirements(&mut self) {
        while let Some(event) = self.pending_retirements.pop() {
            if let Err(rtrb::PushError::Full(event)) = self.event_tx.push(event) {
                self.pending_retirements.push(event);
                break;
            }
        }
    }
    fn retire_event(&mut self, event: EngineEvent) {
        if let Err(rtrb::PushError::Full(event)) = self.event_tx.push(event) {
            self.pending_retirements.push(event);
        }
    }
    pub(super) fn dispose_effect(&mut self, mut effect: Box<dyn vibez_dsp::effect::AudioEffect>) {
        effect.stop_processing();
        self.retire_event(EngineEvent::DisposeEffect(
            crate::events::DisposalCell::new(effect),
        ));
    }
    pub(super) fn dispose_instrument(
        &mut self,
        mut instrument: Box<dyn vibez_instruments::Instrument>,
    ) {
        instrument.stop_processing();
        self.retire_event(EngineEvent::DisposeInstrument(
            crate::events::DisposalCell::new(instrument),
        ));
    }
    pub(super) fn dispose_channel(&mut self, mut channel: EngineTrack) {
        for slot in &mut channel.effects {
            slot.effect.stop_processing();
        }
        if let Some(instrument) = channel.instrument.as_mut() {
            instrument.stop_processing();
        }
        let token = self.channel_retirement.retire(channel);
        self.retire_event(EngineEvent::RetiredChannel(token));
    }
}

#[cfg(test)]
#[path = "retirement_tests.rs"]
mod tests;
