//! Send edits respect the bus capacity without growing callback-owned storage.

use super::*;
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};

fn fixture(
    sends: usize,
) -> (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    rtrb::Consumer<EngineEvent>,
    TrackId,
    Vec<TrackId>,
) {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let buses: Vec<_> = (0..crate::mixer::CHANNEL_CAPACITY)
        .map(|_| TrackId::new())
        .collect();
    commands.push(EngineCommand::add_track(track)).unwrap();
    for (index, bus) in buses.iter().enumerate() {
        commands.push(EngineCommand::add_bus(*bus)).unwrap();
        if index < sends {
            commands
                .push(EngineCommand::SetSend {
                    track_id: track,
                    bus_id: *bus,
                    amount: 0.25,
                })
                .unwrap();
        }
    }
    engine.process(&mut [0.0; 32], 2);
    while events.pop().is_ok() {}
    (engine, commands, events, track, buses)
}

#[test]
fn sends_insert_and_update_without_callback_allocation_at_bus_boundaries() {
    for count in [0, 1, crate::mixer::CHANNEL_CAPACITY - 1, crate::mixer::CHANNEL_CAPACITY] {
        let (mut engine, mut commands, mut events, track, buses) = fixture(count);
        let target = buses[count % buses.len()];
        let storage = engine.tracks()[0].sends.as_ptr();
        for amount in [0.75, -1.0, 2.0] {
            commands
                .push(EngineCommand::SetSend {
                    track_id: track,
                    bus_id: target,
                    amount,
                })
                .unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
                (0, 0),
                "existing sends {count}"
            );
            let sends = &engine.tracks()[0].sends;
            assert_eq!(sends.as_ptr(), storage);
            assert_eq!(sends.len(), (count + 1).min(buses.len()));
            assert_eq!(sends.iter().find(|(id, _)| *id == target).unwrap().1, amount.clamp(0.0, 1.0));
            while events.pop().is_ok() {}
        }
    }
}

#[test]
fn sends_keep_existing_values_when_an_over_capacity_target_is_rejected() {
    let (mut engine, mut commands, _events, track, _) = fixture(crate::mixer::CHANNEL_CAPACITY);
    let before = engine.tracks()[0].sends.clone();
    commands
        .push(EngineCommand::SetSend {
            track_id: track,
            bus_id: TrackId::new(),
            amount: 1.0,
        })
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
        (0, 0)
    );
    assert_eq!(engine.tracks()[0].sends, before);
}

#[test]
fn sends_accept_unknown_targets_with_room_and_ignore_non_track_sources() {
    let (mut engine, mut commands, _events, track, buses) = fixture(0);
    let unknown = TrackId::new();
    for source in [track, buses[0], TrackId::MASTER, TrackId::new()] {
        commands
            .push(EngineCommand::SetSend {
                track_id: source,
                bus_id: unknown,
                amount: 0.5,
            })
            .unwrap();
    }
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
        (0, 0)
    );
    assert_eq!(engine.tracks()[0].sends, [(unknown, 0.5)]);
    assert!(engine.buses.iter().all(|bus| bus.sends.is_empty()));
    assert!(engine.master.sends.is_empty());
}

#[test]
fn sends_reuse_storage_after_bus_removal_with_full_ui_event_ring() {
    let (mut engine, mut commands, mut events, track, buses) = fixture(crate::mixer::CHANNEL_CAPACITY);
    let storage = engine.tracks()[0].sends.as_ptr();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    commands.push(EngineCommand::RemoveBus(buses[0])).unwrap();
    let replacement = TrackId::new();
    commands.push(EngineCommand::add_bus(replacement)).unwrap();
    commands
        .push(EngineCommand::SetSend {
            track_id: track,
            bus_id: replacement,
            amount: 0.5,
        })
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
        (0, 0)
    );
    assert_eq!(engine.pending_retirements.len(), 1);
    assert_eq!(engine.tracks()[0].sends.len(), buses.len());
    assert_eq!(engine.tracks()[0].sends.as_ptr(), storage);
    assert!(engine.tracks()[0].sends.contains(&(replacement, 0.5)));
    assert!(!engine.tracks()[0].sends.iter().any(|(id, _)| *id == buses[0]));
    while events.pop().is_ok() {}
    engine.flush_retirements();
    assert!(matches!(events.pop().unwrap(), EngineEvent::RetiredChannel(_)));
}

#[test]
fn sends_first_automation_value_uses_prepared_channel_storage() {
    let bus = TrackId::new();
    let mut lane = AutomationLane::new(AutomationTarget::Send { bus_id: bus });
    lane.insert_point(AutomationPoint { beat: 0.0, value: 0.75, curve: 0.0 });
    let source = crate::playback_source::PreparedPlaybackSource::new(Vec::new(), Vec::new(), vec![lane]);
    let mut channel = EngineTrack::with_playback_source(TrackId::new(), source);
    let storage = channel.sends.as_ptr();
    assert_eq!(
        crate::retirement::tests::allocations(|| { channel.apply_automation(0.0); }),
        (0, 0)
    );
    assert_eq!(channel.sends.as_ptr(), storage);
    assert_eq!(channel.sends, [(bus, 0.75)]);
}
