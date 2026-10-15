//! Runtime queue tests use the engine's existing callback allocation counter.

use super::*;
use crate::retirement::tests::allocations;
use std::cell::Cell;
use std::ops::ControlFlow;
use std::panic::{catch_unwind, AssertUnwindSafe};
use vibez_core::id::TrackId;

#[test]
fn queue_and_engine_are_send_while_allowances_and_permits_remain_callback_local() {
    fn assert_send<T: Send>() {}
    assert_send::<RetirementQueue>();
    assert_send::<crate::engine::AudioEngine>();
    trait NotSendIfUnambiguous<A> {
        fn witness() {}
    }
    impl<T: ?Sized> NotSendIfUnambiguous<()> for T {}
    struct SendWitness;
    impl<T: ?Sized + Send> NotSendIfUnambiguous<SendWitness> for T {}
    let _ = <RetirementAllowance as NotSendIfUnambiguous<_>>::witness;
    let _ = <RetirementPermit as NotSendIfUnambiguous<_>>::witness;
    trait NotSyncIfUnambiguous<A> {
        fn witness() {}
    }
    impl<T: ?Sized> NotSyncIfUnambiguous<()> for T {}
    struct SyncWitness;
    impl<T: ?Sized + Sync> NotSyncIfUnambiguous<SyncWitness> for T {}
    let _ = <RetirementAllowance as NotSyncIfUnambiguous<_>>::witness;
    let _ = <RetirementPermit as NotSyncIfUnambiguous<_>>::witness;
}

#[test]
fn direct_publication_consumes_one_slot_and_unused_allowance_returns_the_rest() {
    let mut queue = RetirementQueue::new();
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    let reason = String::from("prepared outside the callback");
    let pointer = reason.as_ptr();
    assert_eq!(
        allocations(|| {
            let mut allowance = queue.reserve(3).unwrap();
            assert_eq!(queue.available(), 29);
            let permit = allowance.next().unwrap();
            assert_eq!(
                queue
                    .publish(
                        &mut producer,
                        EngineEvent::CompensationFailed { reason },
                        permit
                    )
                    .unwrap(),
                Publication::Direct
            );
            assert_eq!(allowance.remaining(), 2);
            assert_eq!(queue.reserved(), 2);
            drop(allowance);
        }),
        (0, 0)
    );
    assert_eq!(
        (queue.len(), queue.reserved(), queue.available()),
        (0, 0, 32)
    );
    assert!(
        matches!(consumer.pop(), Ok(EngineEvent::CompensationFailed { reason }) if reason.as_ptr() == pointer)
    );
}

#[test]
fn full_capacity_and_retries_keep_exact_lifo_owners_without_new_reservations_or_heap_activity() {
    let mut queue = RetirementQueue::new();
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    producer.push(EngineEvent::PlaybackStarted).unwrap();
    let mut owners: [Option<EngineEvent>; 32] =
        std::array::from_fn(|_| Some(EngineEvent::RetiredTrackOrder(vec![TrackId::new()])));
    let pointers: Vec<_> = owners
        .iter()
        .map(|event| match event.as_ref().unwrap() {
            EngineEvent::RetiredTrackOrder(order) => order.as_ptr(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        allocations(|| {
            let mut allowance = queue.reserve(32).unwrap();
            for event in &mut owners {
                let permit = allowance.next().unwrap();
                assert_eq!(
                    queue
                        .publish(&mut producer, event.take().unwrap(), permit)
                        .unwrap(),
                    Publication::Retained
                );
                assert_eq!(queue.len() + queue.reserved(), 32);
            }
            assert!(allowance.next().is_none());
        }),
        (0, 0)
    );
    assert_eq!(
        (queue.len(), queue.reserved(), queue.available()),
        (32, 0, 0)
    );
    assert!(queue.reserve(1).is_none());
    assert_eq!(allocations(|| queue.flush(&mut producer)), (0, 0));
    assert_eq!((queue.len(), queue.reserved()), (32, 0));
    assert!(
        matches!(queue.as_slice().last(), Some(EngineEvent::RetiredTrackOrder(order)) if order.as_ptr() == pointers[31])
    );
    assert!(matches!(consumer.pop(), Ok(EngineEvent::PlaybackStarted)));
    for expected in pointers.into_iter().rev() {
        assert_eq!(allocations(|| queue.flush(&mut producer)), (0, 0));
        assert_eq!(queue.reserved(), 0);
        let EngineEvent::RetiredTrackOrder(owner) = consumer.pop().unwrap() else {
            panic!("original owner");
        };
        assert_eq!(owner.as_ptr(), expected);
        drop(owner);
    }
    assert!(queue.is_empty());
    assert_eq!(queue.capacity(), 32);
    assert_eq!(queue.available(), 32);
}

#[test]
fn overlapping_allowances_and_dropped_permits_release_only_their_own_capacity() {
    let mut queue = RetirementQueue::new();
    assert_eq!(
        allocations(|| {
            let mut outer = queue.reserve(20).unwrap();
            let inner = queue.reserve(12).unwrap();
            assert!(queue.reserve(1).is_none());
            let permit = outer.next().unwrap();
            drop(outer);
            assert_eq!(queue.reserved(), 13);
            drop(inner);
            assert_eq!(queue.reserved(), 1);
            drop(permit);
            assert_eq!(queue.reserved(), 0);
            assert!(queue.reserve(0).is_none());
            assert!(queue.reserve(33).is_none());
        }),
        (0, 0)
    );
    assert_eq!(queue.available(), 32);
}

#[test]
fn a_wrong_queue_returns_both_original_owner_and_unspent_permit() {
    let mut original = RetirementQueue::new();
    let mut wrong = RetirementQueue::new();
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    let order = vec![TrackId::new()];
    let pointer = order.as_ptr();
    assert_eq!(
        allocations(|| {
            let mut allowance = original.reserve(1).unwrap();
            let permit = allowance.next().unwrap();
            let rejected = wrong
                .publish(&mut producer, EngineEvent::RetiredTrackOrder(order), permit)
                .unwrap_err();
            assert_eq!(
                (wrong.len(), wrong.reserved(), original.reserved()),
                (0, 0, 1)
            );
            let (event, permit) = rejected.into_parts();
            assert_eq!(
                original.publish(&mut producer, event, permit).unwrap(),
                Publication::Direct
            );
        }),
        (0, 0)
    );
    assert_eq!(original.reserved(), 0);
    assert!(
        matches!(consumer.pop(), Ok(EngineEvent::RetiredTrackOrder(order)) if order.as_ptr() == pointer)
    );
}

fn nested_publish(
    queue: &mut RetirementQueue,
    producer: &mut rtrb::Producer<EngineEvent>,
    allowance: &mut RetirementAllowance,
) {
    queue
        .publish(
            producer,
            EngineEvent::PlaybackStopped,
            allowance.next().unwrap(),
        )
        .unwrap();
}

fn bounded_early_exit(
    queue: &mut RetirementQueue,
    producer: &mut rtrb::Producer<EngineEvent>,
) -> ControlFlow<()> {
    let mut allowance = queue.reserve(3).unwrap();
    nested_publish(queue, producer, &mut allowance);
    ControlFlow::Break(())
}

#[test]
fn nested_borrow_and_early_control_flow_return_unused_capacity() {
    let mut queue = RetirementQueue::new();
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    assert_eq!(
        allocations(|| assert_eq!(
            bounded_early_exit(&mut queue, &mut producer),
            ControlFlow::Break(())
        )),
        (0, 0)
    );
    assert_eq!(queue.reserved(), 0);
    assert!(matches!(consumer.pop(), Ok(EngineEvent::PlaybackStopped)));
}

#[test]
fn unused_allowance_drop_remains_allocation_free_during_unwind() {
    struct ObserveDrop<'a> {
        allowance: Option<RetirementAllowance>,
        observed: &'a Cell<Option<(usize, usize)>>,
    }
    impl Drop for ObserveDrop<'_> {
        fn drop(&mut self) {
            self.observed
                .set(Some(allocations(|| drop(self.allowance.take()))));
        }
    }
    let mut queue = RetirementQueue::new();
    let observed = Cell::new(None);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _guard = ObserveDrop {
            allowance: Some(queue.reserve(3).unwrap()),
            observed: &observed,
        };
        panic!("test-only unwind outside the measured allowance destructor");
    }));
    assert!(result.is_err());
    assert_eq!(observed.get(), Some((0, 0)));
    assert_eq!(queue.reserved(), 0);
    assert_eq!(queue.available(), 32);
}
