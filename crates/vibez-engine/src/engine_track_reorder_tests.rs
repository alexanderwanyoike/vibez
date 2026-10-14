//! Track reordering preserves stable channel identity without callback allocation.

use super::*;

fn tracks(engine: &AudioEngine) -> Vec<TrackId> {
    engine.tracks().iter().map(|track| track.id).collect()
}

fn fixture(
    count: usize,
) -> (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    rtrb::Consumer<EngineEvent>,
    Vec<TrackId>,
) {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let ids: Vec<_> = (0..count).map(|_| TrackId::new()).collect();
    for id in &ids {
        commands.push(EngineCommand::add_track(*id)).unwrap();
    }
    engine.process(&mut [0.0; 32], 2);
    while events.pop().is_ok() {}
    (engine, commands, events, ids)
}

#[test]
fn reorder_tracks_is_allocation_free_at_channel_capacity_boundaries() {
    for count in [0, 1, 20, 21, crate::mixer::CHANNEL_CAPACITY] {
        let (mut engine, mut commands, mut events, ids) = fixture(count);
        let owners: Vec<_> = engine
            .tracks()
            .iter()
            .map(|track| (track.id, &**track as *const EngineTrack))
            .collect();
        let mut order = ids.clone();
        order.reverse();
        commands.push(EngineCommand::ReorderTracks(order)).unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
            (0, 0),
            "channel count {count}"
        );
        assert_eq!(tracks(&engine), ids.iter().rev().copied().collect::<Vec<_>>());
        for track in engine.tracks() {
            assert_eq!(
                owners.iter().find(|(id, _)| *id == track.id).unwrap().1,
                &**track as *const EngineTrack
            );
        }
        while events.pop().is_ok() {}
    }
}

#[test]
fn reorder_tracks_preserves_missing_tracks_and_ignores_duplicate_or_unknown_ids() {
    let (mut engine, mut commands, mut events, ids) = fixture(24);
    for order in [
        vec![TrackId::new(), ids[17], ids[2], ids[17], ids[23]],
        Vec::new(),
        vec![TrackId::new()],
    ] {
        let mut expected = tracks(&engine);
        // Missing IDs sort last in the existing command's stable ordering.
        expected.sort_by_key(|id| {
            order
                .iter()
                .position(|requested| requested == id)
                .unwrap_or(usize::MAX)
        });
        commands.push(EngineCommand::ReorderTracks(order)).unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
            (0, 0)
        );
        assert_eq!(tracks(&engine), expected);
        while events.pop().is_ok() {}
    }
}

#[test]
fn reorder_tracks_keeps_order_storage_until_full_event_ring_can_deliver_it() {
    let (mut engine, mut commands, mut events, ids) = fixture(24);
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..=crate::retirement::RETIREMENT_CAPACITY {
        let mut order = ids.clone();
        order.reverse();
        commands.push(EngineCommand::ReorderTracks(order)).unwrap();
    }
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
        (0, 0)
    );
    assert_eq!(
        engine.pending_retirements.len(),
        crate::retirement::RETIREMENT_CAPACITY
    );
    assert_eq!(engine.cmd_rx.slots(), 1);
    while events.pop().is_ok() {}
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
        (0, 0)
    );
    assert_eq!(engine.cmd_rx.slots(), 0);
    assert!(engine.pending_retirements.is_empty());
    assert_eq!(tracks(&engine), ids.iter().rev().copied().collect::<Vec<_>>());
}

#[test]
fn reorder_tracks_returns_the_original_vector_owner_with_or_without_ui_backpressure() {
    for full_ring in [false, true] {
        let (mut engine, mut commands, mut events, ids) = fixture(24);
        if full_ring {
            while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        }
        let mut order = ids.clone();
        order.reverse();
        let pointer = order.as_ptr();
        let capacity = order.capacity();
        commands.push(EngineCommand::ReorderTracks(order)).unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
            (0, 0)
        );
        if full_ring {
            assert_eq!(engine.pending_retirements.len(), 1);
            while events.pop().is_ok() {}
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.flush_retirements()),
                (0, 0)
            );
        }
        let returned = std::iter::from_fn(|| events.pop().ok())
            .find(|event| matches!(event, EngineEvent::RetiredTrackOrder(_)))
            .expect("the incoming order storage returns to the UI");
        assert_eq!(returned.time_domain(), crate::events::TimeDomain::Untimed);
        let EngineEvent::RetiredTrackOrder(owner) = returned else {
            unreachable!()
        };
        assert_eq!(owner.as_ptr(), pointer);
        assert_eq!(owner.capacity(), capacity);
        assert_eq!(owner, ids.iter().rev().copied().collect::<Vec<_>>());
    }
}
