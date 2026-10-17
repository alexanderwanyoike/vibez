//! Ordinary fallback, channel tokens and nested cleanup have independent admission limits.

use super::*;
use crate::playback_source::{EngineClip, PreparedPlaybackSource};
use graph_test_support::{graph_engine, routing_channel, routing_effect};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::{ClipId, EffectId};
use vibez_core::perform::SwingAmount;

fn clip(id: ClipId, value: f32) -> EngineClip {
    EngineClip {
        id,
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![value; 4096]],
            sample_rate: DEFAULT_SAMPLE_RATE,
        }),
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 4096,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        linear_gain: 1.0,
        fades: Default::default(),
        playback_direction: Default::default(),
        warp_markers: Default::default(),
    }
}

fn fill_ring(engine: &mut AudioEngine) {
    assert_eq!(engine.event_tx.slots(), 1024);
    for sample in 0..1024 {
        engine
            .event_tx
            .push(EngineEvent::PlaybackPosition(sample))
            .unwrap();
    }
    assert!(engine.event_tx.is_full());
}

fn remove_ring_markers(events: &mut Consumer<EngineEvent>) {
    for expected in 0..1024 {
        assert!(
            matches!(events.pop(), Ok(EngineEvent::PlaybackPosition(actual)) if actual == expected)
        );
    }
    assert!(events.pop().is_err());
}

fn order_pressure(
    engine: &mut AudioEngine,
    commands: &mut Producer<EngineCommand>,
    track: TrackId,
    count: usize,
) -> Vec<usize> {
    let mut pointers = Vec::new();
    for _ in 0..count {
        let order = vec![track];
        pointers.push(order.as_ptr() as usize);
        commands.push(EngineCommand::ReorderTracks(order)).unwrap();
    }
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert_eq!(engine.pending_retirements.len(), count);
    assert!(engine.cmd_rx.peek().is_err());
    pointers
}

struct RequestedOwner {
    requested: Arc<AtomicBool>,
    drops: Arc<AtomicUsize>,
}
impl Drop for RequestedOwner {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}
impl vibez_dsp::effect::AudioEffect for RequestedOwner {
    fn reconfiguration_requested(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, _: &mut [f32], _: usize) {}
    fn reset(&mut self) {}
}

#[test]
fn current_reject_admission_preserves_all_three_owners_with_zero_through_three_free_slots() {
    for free in 0..=3 {
        let track = TrackId::new();
        let effect = EffectId::new();
        let requested = Arc::new(AtomicBool::new(false));
        let drops = Arc::new(AtomicUsize::new(0));
        let mut route = routing_channel(track);
        route.effects.push(routing_effect(effect));
        let (mut engine, mut commands, mut events) =
            graph_engine(&[route, routing_channel(TrackId::MASTER)]);
        let source_clip = clip(ClipId::new(), 0.25);
        let audio = Arc::downgrade(&source_clip.audio);
        let mut channel = EngineTrack::with_playback_source(
            track,
            PreparedPlaybackSource::new(vec![source_clip], vec![], vec![]),
        );
        channel.effects.push(EffectSlot {
            id: effect,
            bypass: false,
            effect: Box::new(RequestedOwner {
                requested: Arc::clone(&requested),
                drops: Arc::clone(&drops),
            }),
            levels: Default::default(),
        });
        commands
            .push(EngineCommand::AddTrack(Box::new(channel)))
            .unwrap();
        commands.push(EngineCommand::Play).unwrap();
        commands
            .push(EngineCommand::StartPerformanceCapture)
            .unwrap();
        let mut warm = [0.0];
        engine.process_block(AudioProcessBlock::new(&mut warm, 1));
        assert_eq!(warm, [0.25]);
        while events.pop().is_ok() {}
        requested.store(true, Ordering::Relaxed);
        engine.begin_device_reconfiguration();
        let EngineEvent::DeviceReconfiguration(mut device) = events.pop().unwrap() else {
            panic!("real handoff");
        };
        assert!(events.pop().is_err());
        let device_identity = device.owner_identity();
        assert!(engine.handoff_is_current(&device));
        let storage_identity = match &mut device {
            reconfiguration::DeviceReconfiguration::Effect {
                reserved_effects, ..
            } => {
                *reserved_effects = Vec::with_capacity(1);
                reserved_effects.as_ptr() as usize
            }
            _ => panic!("effect handoff"),
        };
        // The original regression's full restoration target returns storage,
        // the original device and the owned rejection cause in one command.
        let channel = &mut engine.tracks[0];
        assert!(channel.effects.capacity() > 1);
        while channel.effects.len() < channel.effects.capacity() {
            channel.effects.push(EffectSlot {
                id: EffectId::new(),
                bypass: false,
                effect: vibez_dsp::factory::create_effect(
                    EffectType::Gain,
                    DEFAULT_SAMPLE_RATE as f32,
                ),
                levels: Default::default(),
            });
        }
        fill_ring(&mut engine);
        let pressure_orders = order_pressure(&mut engine, &mut commands, track, 32 - free);
        let reason = String::from("actual current rejection under pressure");
        let reason_identity = reason.as_ptr() as usize;
        let swing = SwingAmount::new(0.625);
        commands
            .push(EngineCommand::RejectDeviceReconfiguration { device, reason })
            .unwrap();
        commands
            .push(EngineCommand::SetProjectSwing(swing))
            .unwrap();
        let original_swing = engine.project_swing;
        let before = (
            engine.output_position_samples(),
            engine.transport().position(),
        );
        let capture_boundary = engine.compensation_callback_heard_start;
        assert_eq!(capture_boundary, 0);
        let heap = crate::retirement::tests::allocations(|| engine.drain_commands());
        assert_eq!(heap, (0, 0), "free={free}");
        assert_eq!(engine.pending_retirements.capacity(), 32);
        assert_eq!(
            (
                engine.output_position_samples(),
                engine.transport().position()
            ),
            before
        );
        assert_eq!(audio.strong_count(), 1);
        assert_eq!(
            Arc::as_ptr(&engine.tracks()[0].playback_source.clips[0].audio),
            audio.as_ptr()
        );
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        assert_eq!(
            engine.project_swing, original_swing,
            "FIFO marker cannot overtake rejection"
        );
        if free < 3 {
            assert_eq!(engine.pending_retirements.len(), 32 - free);
            assert_eq!(engine.cmd_rx.slots(), 2);
            assert!(
                matches!(engine.cmd_rx.peek(), Ok(EngineCommand::RejectDeviceReconfiguration { device, reason }) if device.owner_identity() == device_identity && reason.as_ptr() as usize == reason_identity)
            );
            assert!(engine.device_handoff.pending().is_some());
            assert!(engine.capture_lifecycle.is_active());
        } else {
            assert_eq!(engine.pending_retirements.len(), 32);
            assert_eq!(engine.cmd_rx.slots(), 1);
            let returned = &engine.pending_retirements.as_slice()[29..];
            assert!(
                matches!(&returned[0], EngineEvent::RetiredEffectStorage(storage) if storage.0.as_ptr() as usize == storage_identity)
            );
            assert!(
                matches!(&returned[1], EngineEvent::DisposeEffect(owner) if reconfiguration::effect_owner_identity(&**owner) == device_identity)
            );
            assert!(
                matches!(&returned[2], EngineEvent::CompensationFailed { reason } if reason.as_ptr() as usize == reason_identity)
            );
            assert!(engine.device_handoff.pending().is_none());
            assert!(!engine.capture_lifecycle.is_active());
            assert_eq!(
                engine.presentation_state.terminals.capture_stop(),
                Some(capture_boundary)
            );
        }
        remove_ring_markers(&mut events);
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
        assert!(engine.cmd_rx.peek().is_err());
        assert_eq!(engine.project_swing, swing);
        assert!(engine.pending_retirements.is_empty());
        assert!(!engine.compensation_plan.applied_plan_is_valid());
        assert!(!engine.transport().is_playing());
        let returned: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
        let orders: Vec<_> = returned
            .iter()
            .filter_map(|event| match event {
                EngineEvent::RetiredTrackOrder(order) => Some(order.as_ptr() as usize),
                _ => None,
            })
            .collect();
        assert_eq!(orders.len(), pressure_orders.len());
        for pointer in pressure_orders {
            assert_eq!(
                orders.iter().filter(|actual| **actual == pointer).count(),
                1
            );
        }
        assert_eq!(returned.iter().filter(|event| matches!(event, EngineEvent::RetiredEffectStorage(storage) if storage.0.as_ptr() as usize == storage_identity)).count(), 1);
        assert_eq!(returned.iter().filter(|event| matches!(event, EngineEvent::DisposeEffect(owner) if reconfiguration::effect_owner_identity(&**owner) == device_identity)).count(), 1);
        assert_eq!(returned.iter().filter(|event| matches!(event, EngineEvent::CompensationFailed { reason } if reason.as_ptr() as usize == reason_identity)).count(), 1);
        assert_eq!(returned.iter().filter(|event| matches!(event, EngineEvent::PerformanceCaptureStopped { effective_at_samples } if *effective_at_samples == capture_boundary)).count(), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        drop(returned);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn full_channel_pool_holds_commands_even_when_ordinary_retirement_storage_is_empty() {
    let ids: Vec<_> = (0..33).map(|_| TrackId::new()).collect();
    let mut routes: Vec<_> = ids.iter().map(|id| routing_channel(*id)).collect();
    routes.push(routing_channel(TrackId::MASTER));
    let (mut engine, mut commands, mut events) = graph_engine(&routes);
    let mut owners = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let source = clip(ClipId::new(), if index == 32 { 0.25 } else { 0.0 });
        owners.push(Arc::downgrade(&source.audio));
        commands
            .push(EngineCommand::AddTrack(Box::new(
                EngineTrack::with_playback_source(
                    *id,
                    PreparedPlaybackSource::new(vec![source], vec![], vec![]),
                ),
            )))
            .unwrap();
    }
    commands.push(EngineCommand::Play).unwrap();
    let mut warm = [0.0];
    engine.process_block(AudioProcessBlock::new(&mut warm, 1));
    assert_eq!(warm, [0.25]);
    while events.pop().is_ok() {}
    for id in &ids[..32] {
        commands.push(EngineCommand::RemoveTrack(*id)).unwrap();
    }
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    let mut held = Vec::new();
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::RetiredChannel(token) => held.push(token),
            event => panic!("unexpected retirement {event:?}"),
        }
    }
    assert_eq!(held.len(), 32);
    assert!(!engine.channel_retirement.has_capacity());
    assert!(engine.pending_retirements.is_empty());
    assert_eq!(engine.tracks().len(), 1);
    assert!(owners.iter().all(|owner| owner.strong_count() == 1));
    fill_ring(&mut engine);
    let swing = SwingAmount::new(0.625);
    let original_swing = engine.project_swing;
    commands.push(EngineCommand::RemoveTrack(ids[32])).unwrap();
    commands
        .push(EngineCommand::SetProjectSwing(swing))
        .unwrap();
    let before = engine.transport().position();
    let mut output = [0.0; 8];
    assert_eq!(
        crate::retirement::tests::allocations(
            || engine.process_block(AudioProcessBlock::new(&mut output, 1))
        ),
        (0, 0)
    );
    assert_eq!(output, [0.25; 8]);
    assert_eq!(engine.transport().position(), before + 8);
    assert_eq!(engine.cmd_rx.slots(), 2);
    assert_eq!(engine.tracks()[0].id, ids[32]);
    assert_eq!(engine.project_swing, original_swing);
    assert!(engine.pending_retirements.is_empty());
    drop(held.pop().unwrap());
    assert!(engine.channel_retirement.has_capacity());
    assert_eq!(
        owners
            .iter()
            .filter(|owner| owner.strong_count() == 0)
            .count(),
        1
    );
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert!(engine.tracks().is_empty());
    assert_eq!(engine.pending_retirements.len(), 1);
    assert!(!engine.channel_retirement.has_capacity());
    assert_eq!(engine.cmd_rx.slots(), 1);
    assert_eq!(engine.project_swing, original_swing);
    assert_eq!(owners[32].strong_count(), 1);
    drop(held.pop().unwrap());
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert!(engine.cmd_rx.peek().is_err());
    assert_eq!(engine.project_swing, swing);
    drop(held);
    assert!(owners[..32].iter().all(|owner| owner.strong_count() == 0));
    remove_ring_markers(&mut events);
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.flush_retirements()),
        (0, 0)
    );
    let EngineEvent::RetiredChannel(token) = events.pop().unwrap() else {
        panic!("last exact channel owner");
    };
    assert_eq!(owners[32].strong_count(), 1);
    drop(token);
    assert_eq!(owners[32].strong_count(), 0);
    assert!(engine.channel_retirement.has_capacity());
    assert!(engine.pending_retirements.is_empty());
}

#[test]
fn remove_bus_nested_lane_cleanup_uses_remaining_slots_after_staged_source_owners() {
    let track = TrackId::new();
    let bus = TrackId::new();
    let removed_id = ClipId::new();
    let mut source_clips = vec![clip(ClipId::new(), 0.25)];
    let surviving_audio = Arc::downgrade(&source_clips[0].audio);
    for _ in 0..4 {
        source_clips.push(clip(removed_id, 0.0));
    }
    let removed_audio: Vec<_> = source_clips[1..]
        .iter()
        .map(|clip| Arc::downgrade(&clip.audio))
        .collect();
    let mut lanes = Vec::new();
    for _ in 0..6 {
        let mut lane = AutomationLane::new(AutomationTarget::Send { bus_id: bus });
        lane.points.push(AutomationPoint {
            beat: 0.0,
            value: 0.0,
            curve: 0.0,
        });
        lanes.push(lane);
    }
    let lane_owners: Vec<_> = lanes
        .iter()
        .map(|lane| (lane.id, lane.points.as_ptr() as usize))
        .collect();
    let mut unrelated = AutomationLane::new(AutomationTarget::TrackGain);
    unrelated.points.push(AutomationPoint {
        beat: 0.0,
        value: 0.5,
        curve: 0.0,
    });
    let unrelated_identity = (unrelated.id, unrelated.points.as_ptr());
    lanes.push(unrelated);
    let mut track_route = routing_channel(track);
    track_route.sends.push(bus);
    let mut bus_route = routing_channel(bus);
    bus_route.is_bus = true;
    let (mut engine, mut commands, mut events) =
        graph_engine(&[track_route, bus_route, routing_channel(TrackId::MASTER)]);
    commands
        .push(EngineCommand::AddTrack(Box::new(
            EngineTrack::with_playback_source(
                track,
                PreparedPlaybackSource::new(source_clips, vec![], lanes),
            ),
        )))
        .unwrap();
    commands.push(EngineCommand::add_bus(bus)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut warm = [0.0];
    engine.process_block(AudioProcessBlock::new(&mut warm, 1));
    assert_eq!(warm, [0.25]);
    while events.pop().is_ok() {}
    fill_ring(&mut engine);
    let orders = order_pressure(&mut engine, &mut commands, track, 24);
    engine.tracks_mut_for_retirement()[0]
        .playback_source
        .remove_audio_clips(removed_id);
    let swing = SwingAmount::new(0.625);
    let original_swing = engine.project_swing;
    commands.push(EngineCommand::RemoveBus(bus)).unwrap();
    commands
        .push(EngineCommand::SetProjectSwing(swing))
        .unwrap();
    let before = engine.transport().position();
    let mut output = [0.0; 8];
    assert_eq!(
        crate::retirement::tests::allocations(
            || engine.process_block(AudioProcessBlock::new(&mut output, 1))
        ),
        (0, 0)
    );
    assert_eq!(output, [0.25; 8]);
    assert_eq!(engine.transport().position(), before + 8);
    assert_eq!(engine.pending_retirements.reserved(), 0);
    assert!(engine.pending_retirements.len() <= 32);
    assert_eq!(engine.pending_retirements.len(), 32);
    assert_eq!(engine.pending_retirements.capacity(), 32);
    assert_eq!(
        engine
            .pending_retirements
            .as_slice()
            .iter()
            .filter(|event| matches!(event, EngineEvent::RetiredTrackOrder(_)))
            .count(),
        24
    );
    assert_eq!(
        engine
            .pending_retirements
            .as_slice()
            .iter()
            .filter(|event| matches!(event, EngineEvent::RetiredAudioClip(_)))
            .count(),
        4
    );
    assert_eq!(
        engine
            .pending_retirements
            .as_slice()
            .iter()
            .filter(|event| matches!(event, EngineEvent::RetiredChannel(_)))
            .count(),
        1
    );
    assert_eq!(
        engine
            .pending_retirements
            .as_slice()
            .iter()
            .filter(|event| matches!(event, EngineEvent::RetiredAutomationLane(_)))
            .count(),
        3
    );
    assert!(engine.buses.is_empty());
    assert!(engine.pending_bus_cleanup.is_some());
    assert_eq!(engine.tracks()[0].playback_source.automation.len(), 4);
    assert_eq!(engine.cmd_rx.slots(), 1);
    assert_eq!(engine.project_swing, original_swing);
    assert!(removed_audio.iter().all(|owner| owner.strong_count() == 1));
    assert_eq!(surviving_audio.strong_count(), 1);
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert_eq!(engine.pending_retirements.len(), 32);
    assert_eq!(engine.cmd_rx.slots(), 1);
    assert_eq!(engine.pending_retirements.reserved(), 0);
    assert!(engine.pending_retirements.len() <= 32);
    remove_ring_markers(&mut events);
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert!(engine.cmd_rx.peek().is_err());
    assert_eq!(engine.project_swing, swing);
    assert!(engine.pending_bus_cleanup.is_none());
    assert!(engine.pending_retirements.is_empty());
    assert_eq!(engine.pending_retirements.reserved(), 0);
    assert!(engine.pending_retirements.len() <= 32);
    assert!(engine.tracks()[0].sends.is_empty());
    let retained = &engine.tracks()[0].playback_source.automation;
    assert_eq!(retained.len(), 1);
    assert_eq!(
        (retained[0].id, retained[0].points.as_ptr()),
        unrelated_identity
    );
    let mut returned_orders = Vec::new();
    let mut returned_audio = Vec::new();
    let mut returned_lanes = Vec::new();
    let mut returned_channels = 0;
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::RetiredTrackOrder(order) => returned_orders.push(order.as_ptr() as usize),
            EngineEvent::RetiredAudioClip(owner) => returned_audio.push(Arc::as_ptr(&owner.audio)),
            EngineEvent::RetiredAutomationLane(owner) => {
                returned_lanes.push((owner.id, owner.points.as_ptr() as usize))
            }
            EngineEvent::RetiredChannel(_) => returned_channels += 1,
            event => panic!("unexpected cleanup event {event:?}"),
        }
    }
    assert_eq!(returned_channels, 1);
    assert_eq!(returned_orders.len(), orders.len());
    for pointer in orders {
        assert_eq!(
            returned_orders
                .iter()
                .filter(|actual| **actual == pointer)
                .count(),
            1
        );
    }
    assert_eq!(returned_lanes.len(), 6);
    for identity in lane_owners {
        assert_eq!(
            returned_lanes
                .iter()
                .filter(|actual| **actual == identity)
                .count(),
            1
        );
    }
    assert_eq!(returned_audio.len(), 4);
    for owner in removed_audio {
        assert_eq!(
            returned_audio
                .iter()
                .filter(|pointer| **pointer == owner.as_ptr())
                .count(),
            1
        );
        assert_eq!(owner.strong_count(), 0);
    }
    assert_eq!(surviving_audio.strong_count(), 1);
    assert_eq!(
        Arc::as_ptr(&engine.tracks()[0].playback_source.clips[0].audio),
        surviving_audio.as_ptr()
    );
}
