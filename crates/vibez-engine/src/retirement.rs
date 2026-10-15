//! Bounded transfer of removed engine owners to the UI for destruction.

use std::cell::UnsafeCell;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::{events::EngineEvent, mixer::EngineTrack, retirement_queue::RetirementAllowance};

pub(crate) const RETIREMENT_CAPACITY: usize = 32;
struct ChannelSlot {
    occupied: AtomicBool,
    owner: UnsafeCell<Option<Box<EngineTrack>>>,
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
    fn retire(self: &Arc<Self>, track: Box<EngineTrack>) -> RetiredChannel {
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
    fn take(&self, index: usize) -> Option<Box<EngineTrack>> {
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
        // A newly stopped recording precedes older ordinary returns, matching
        // the existing LIFO retirement order without growing that store.
        if !self.flush_section_record_stop() {
            return;
        }
        if !self.flush_returning_section() {
            return;
        }
        self.pending_retirements.flush(&mut self.event_tx);
    }
    pub(super) fn flush_removed_clip_owners(&mut self) -> bool {
        for index in 0..self.tracks().len() {
            loop {
                let allowance = self.pending_retirements.reserve(1);
                let track = &mut self.tracks_mut_for_retirement()[index];
                let sources = [
                    track.playback_source.as_mut(),
                    track.section_playback_source.as_mut(),
                    track.launcher_source.as_mut(),
                    track.empty_launcher_source.as_mut(),
                ];
                let Some(source) = sources.into_iter().find(|source| {
                    source.has_removed_audio_clips() || source.has_removed_note_clips()
                }) else {
                    break;
                };
                // Take ownership only after admission, including when the UI ring stays full.
                let Some(mut allowance) = allowance else {
                    return false;
                };
                let event = if let Some(clip) = source.pop_removed_audio_clip() {
                    EngineEvent::RetiredAudioClip(clip)
                } else {
                    EngineEvent::RetiredNoteClip(
                        source
                            .pop_removed_note_clip()
                            .expect("pending note clip owner"),
                    )
                };
                self.retire_event(event, &mut allowance);
            }
        }
        true
    }

    pub(super) fn clean_removed_bus_automation(&mut self) {
        let Some((bus, mut track_index)) = self.pending_bus_cleanup else {
            return;
        };
        while track_index < self.tracks().len() {
            let Some(mut allowance) = self.pending_retirements.reserve(1) else {
                break;
            };
            let track = &mut self.tracks_mut_for_retirement()[track_index];
            track.sends.retain(|(id, _)| *id != bus);
            if let Some(index) = track.playback_source.automation.iter().position(|lane| {
                lane.target == vibez_core::automation::AutomationTarget::Send { bus_id: bus }
            }) {
                let lane = track.playback_source.automation.remove(index);
                self.retire_event(EngineEvent::RetiredAutomationLane(lane), &mut allowance);
            } else {
                track_index += 1;
            }
        }
        self.pending_bus_cleanup =
            (track_index < self.tracks().len()).then_some((bus, track_index));
    }

    pub(super) fn retire_event(&mut self, event: EngineEvent, allowance: &mut RetirementAllowance) {
        let permit = allowance.next().expect("the closed owner-return path stays within its admitted budget");
        self.pending_retirements
            .publish(&mut self.event_tx, event, permit)
            .expect("the engine's sole queue issued this allowance");
    }
    pub(super) fn dispose_effect(&mut self, mut effect: Box<dyn vibez_dsp::effect::AudioEffect>, allowance: &mut RetirementAllowance) {
        effect.stop_processing();
        self.retire_event(EngineEvent::DisposeEffect(
            crate::events::DisposalCell::new(effect),
        ), allowance);
    }
    pub(super) fn dispose_instrument(
        &mut self,
        mut instrument: Box<dyn vibez_instruments::Instrument>,
     allowance: &mut RetirementAllowance) {
        instrument.stop_processing();
        self.retire_event(EngineEvent::DisposeInstrument(
            crate::events::DisposalCell::new(instrument),
        ), allowance);
    }
    pub(super) fn dispose_channel(&mut self, mut channel: Box<EngineTrack>, allowance: &mut RetirementAllowance) {
        for slot in &mut channel.effects {
            slot.effect.stop_processing();
        }
        if let Some(instrument) = channel.instrument.as_mut() {
            instrument.stop_processing();
        }
        let token = self.channel_retirement.retire(channel);
        self.retire_event(EngineEvent::RetiredChannel(token), allowance);
    }
}

#[cfg(test)]
#[path = "retirement_tests.rs"]
pub(crate) mod tests;
