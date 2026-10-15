//! Channel-bound fallback permits reserve capacity before ownership transfer.

use crate::events::EngineEvent;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

const CAPACITY: usize = 32;

#[derive(Debug, Default)]
struct Ledger {
    reserved: AtomicUsize,
}

pub(super) struct RetirementQueue {
    queued: Vec<EngineEvent>,
    ledger: Arc<Ledger>,
}

#[derive(Debug)]
#[must_use = "unused allowance returns its reserved capacity on drop"]
pub(super) struct RetirementAllowance {
    ledger: Arc<Ledger>,
    remaining: usize,
    local: PhantomData<Rc<()>>,
}

#[derive(Debug)]
#[must_use = "consume this permit when publishing its original owner"]
pub(super) struct RetirementPermit {
    ledger: Arc<Ledger>,
    active: bool,
    local: PhantomData<Rc<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Publication {
    Direct,
    Retained,
}

#[derive(Debug)]
#[must_use = "a wrong-queue publication still owns both its event and permit"]
pub(super) struct WrongQueue {
    event: EngineEvent,
    permit: RetirementPermit,
}

impl WrongQueue {
    pub(super) fn into_parts(self) -> (EngineEvent, RetirementPermit) {
        (self.event, self.permit)
    }
}

impl RetirementQueue {
    pub(super) fn new() -> Self {
        Self {
            queued: Vec::with_capacity(CAPACITY),
            ledger: Arc::new(Ledger::default()),
        }
    }

    pub(super) fn as_slice(&self) -> &[EngineEvent] {
        &self.queued
    }
    pub(super) fn len(&self) -> usize {
        self.queued.len()
    }
    pub(super) fn is_empty(&self) -> bool {
        self.queued.is_empty()
    }
    pub(super) fn capacity(&self) -> usize {
        CAPACITY
    }
    pub(super) fn reserved(&self) -> usize {
        self.ledger.reserved.load(Ordering::Relaxed)
    }
    pub(super) fn available(&self) -> usize {
        CAPACITY - self.len() - self.reserved()
    }

    pub(super) fn reserve(&mut self, count: usize) -> Option<RetirementAllowance> {
        if count == 0 || count > self.available() {
            return None;
        }
        self.ledger.reserved.fetch_add(count, Ordering::Relaxed);
        Some(RetirementAllowance {
            ledger: Arc::clone(&self.ledger),
            remaining: count,
            local: PhantomData,
        })
    }

    pub(super) fn publish(
        &mut self,
        events: &mut rtrb::Producer<EngineEvent>,
        event: EngineEvent,
        permit: RetirementPermit,
    ) -> Result<Publication, WrongQueue> {
        if !Arc::ptr_eq(&self.ledger, &permit.ledger) {
            return Err(WrongQueue { event, permit });
        }
        Ok(self.publish_reserved(events, event, permit))
    }

    fn publish_reserved(
        &mut self,
        events: &mut rtrb::Producer<EngineEvent>,
        event: EngineEvent,
        mut permit: RetirementPermit,
    ) -> Publication {
        match events.push(event) {
            Ok(()) => {
                permit.consume();
                Publication::Direct
            }
            Err(rtrb::PushError::Full(event)) => {
                assert!(
                    self.queued.len() < CAPACITY,
                    "the permit already paid for this slot"
                );
                // Remove the reservation before installing its occupied slot.
                // The exclusive queue borrow prevents another producer between them.
                permit.consume();
                self.queued.push(event);
                Publication::Retained
            }
        }
    }

    pub(super) fn flush(&mut self, events: &mut rtrb::Producer<EngineEvent>) {
        while let Some(event) = self.queued.pop() {
            // Popping frees exactly this event's existing slot, even with no
            // otherwise available capacity. Full restores its original LIFO place.
            self.ledger.reserved.fetch_add(1, Ordering::Relaxed);
            let permit = RetirementPermit {
                ledger: Arc::clone(&self.ledger),
                active: true,
                local: PhantomData,
            };
            if self.publish_reserved(events, event, permit) == Publication::Retained {
                break;
            }
        }
    }
}

impl RetirementAllowance {
    pub(super) fn remaining(&self) -> usize {
        self.remaining
    }

    pub(super) fn next(&mut self) -> Option<RetirementPermit> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(RetirementPermit {
            ledger: Arc::clone(&self.ledger),
            active: true,
            local: PhantomData,
        })
    }
}

impl Drop for RetirementAllowance {
    fn drop(&mut self) {
        if self.remaining != 0 {
            let previous = self
                .ledger
                .reserved
                .fetch_sub(self.remaining, Ordering::Relaxed);
            debug_assert!(previous >= self.remaining);
        }
    }
}

impl RetirementPermit {
    fn consume(&mut self) {
        debug_assert!(self.active);
        let previous = self.ledger.reserved.fetch_sub(1, Ordering::Relaxed);
        debug_assert!(previous > 0);
        self.active = false;
    }
}

impl Drop for RetirementPermit {
    fn drop(&mut self) {
        if self.active {
            self.consume();
        }
    }
}

#[cfg(test)]
#[path = "retirement_queue_tests.rs"]
mod tests;
