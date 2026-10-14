//! Selected native scope publication observes the real graph without callback heap traffic.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use vibez_core::{effect::EffectType, id::{EffectId, TrackId}, routing::{RoutingChannel, RoutingEffect, RoutingNode, NodeStage}};
use vibez_dsp::factory::create_effect_with_params;
use vibez_engine::{commands::EngineCommand, engine::{AudioEngine, AudioProcessBlock}, events::EngineEvent, routing::PreparedRouting};

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static DEALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.get() { ALLOCATIONS.set(ALLOCATIONS.get() + 1); }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if COUNTING.get() { DEALLOCATIONS.set(DEALLOCATIONS.get() + 1); }
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if COUNTING.get() {
            ALLOCATIONS.set(ALLOCATIONS.get() + 1);
            DEALLOCATIONS.set(DEALLOCATIONS.get() + 1);
        }
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn without_heap_traffic(action: impl FnOnce()) -> (usize, usize) {
    ALLOCATIONS.set(0);
    DEALLOCATIONS.set(0);
    COUNTING.set(true);
    action();
    COUNTING.set(false);
    (ALLOCATIONS.get(), DEALLOCATIONS.get())
}

#[test]
fn selected_native_sample_hold_packets_publish_actual_graph_samples_at_fifty_hertz() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
    commands.push(EngineCommand::add_track(track)).unwrap();
    commands.push(EngineCommand::InsertEffect {
        track_id: track,
        effect_id: effect,
        effect: create_effect_with_params(EffectType::Bitcrush, 48_000.0, &[2.0, 3.0, 0.25, 6.0]),
        position: None,
    }).unwrap();
    let channels = [
        RoutingChannel { id: track, is_bus: false, sends: vec![], effects: vec![RoutingEffect {
            id: effect, inputs: vec![], assignments: vec![], inactive_inputs: vec![],
        }] },
        RoutingChannel { id: TrackId::MASTER, is_bus: true, sends: vec![], effects: vec![] },
    ];
    let reports = [(RoutingNode { channel: track, stage: NodeStage::Effect(effect) }, 0)];
    let routing = PreparedRouting::prepare_compensated(&channels, 128, &reports, &[], 1).unwrap();
    commands.push(EngineCommand::SetRouting(routing)).unwrap();
    commands.push(EngineCommand::SetEffectMeterChannel { track_id: Some(track), generation: 17 }).unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [], 2));
    while events.pop().is_ok() {}

    let left = [-0.8, -0.6, -0.3, -0.2, 0.0, 0.2, 0.3, 0.6, 0.8, 1.2, 1.5, -1.25];
    let right = [0.91, -0.8, 0.6, -0.4, 0.2, 0.0, -0.2, 0.4, -0.6, 0.8, -1.0, 1.5];
    let held = [-1.0, -1.0, -1.0, -0.5, -0.5, -0.5, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0];
    let mut input = [0.0; 256];
    let mut output = [0.0; 256];
    let mut source_frame = 0;
    for frames in [128, 128, 128, 128, 128, 128, 128, 63, 1] {
        for frame in 0..frames {
            let position = (source_frame + frame) % left.len();
            input[frame * 2] = left[position];
            input[frame * 2 + 1] = right[position];
        }
        let traffic = without_heap_traffic(|| {
            engine.process_block(AudioProcessBlock::new(&mut output[..frames * 2], 2)
                .with_live_input(track.raw(), &input[..frames * 2]));
        });
        assert_eq!(traffic, (0, 0), "source_frame={source_frame}, frames={frames}");
        source_frame += frames;
        if source_frame < 960 {
            while let Ok(event) = events.pop() {
                assert!(!matches!(event, EngineEvent::EffectSampleHold { .. }),
                    "48 kHz scope publication must wait for960 processed frames");
            }
        }
    }
    let mut observed = None;
    while let Ok(event) = events.pop() {
        if let EngineEvent::EffectSampleHold { track_id, effect_id, generation, meter } = event {
            assert!(observed.is_none(), "one packet at the first50 Hz deadline");
            observed = Some((track_id, effect_id, generation, meter));
        }
    }
    let (observed_track, observed_effect, generation, meter) = observed.expect("selected native scope packet");
    assert_eq!((observed_track, observed_effect, generation), (track, effect, 17));
    assert_eq!(meter.count, 128);
    assert_eq!(meter.sample_rate, 48_000.0);
    assert_eq!(meter.first_source_frame, 832);
    assert_eq!(meter.latency_samples, 0);
    assert_eq!(meter.primary_interval_frames, 3);
    assert_eq!(meter.secondary_interval_frames, 0);
    assert!(!meter.transition_active);
    for point in 0..128 {
        let frame = 832 + point;
        let position = frame % left.len();
        assert_eq!(meter.input[point], left[position]);
        assert_eq!(meter.output[point], left[position] * 0.75 + held[position] * 0.25);
        let acquired = meter.primary_acquisitions[point / 64] & (1_u64 << (point % 64)) != 0;
        assert_eq!(acquired, frame.is_multiple_of(3));
    }
    assert_eq!(meter.secondary_acquisitions, [0; 2]);
    assert_eq!(meter.blending, [0; 2]);
}
