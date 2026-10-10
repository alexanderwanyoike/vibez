//! Prepared storage rejects over-budget graphs before allocating audio history.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use vibez_core::{
    automation::AutomationTarget,
    id::{EffectId, TrackId},
    routing::*,
};
use vibez_engine::{compensation::MAX_DEVICE_LATENCY, routing::PreparedRouting};

thread_local! {static TRACK: Cell<bool> = const {Cell::new(false)}; static LARGEST: Cell<usize> = const {Cell::new(0)};}
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACK.get() {
            LARGEST.set(LARGEST.get().max(layout.size()));
        }
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if TRACK.get() {
            LARGEST.set(LARGEST.get().max(size));
        }
        System.realloc(ptr, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn largest_allocation<T>(action: impl FnOnce() -> T) -> (T, usize) {
    LARGEST.set(0);
    TRACK.set(true);
    let result = action();
    TRACK.set(false);
    (result, LARGEST.get())
}
fn channel(id: TrackId, effects: &[EffectId]) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: id.is_master(),
        sends: vec![],
        effects: effects
            .iter()
            .map(|&id| RoutingEffect {
                id,
                inputs: vec![],
                assignments: vec![],
                inactive_inputs: vec![],
            })
            .collect(),
    }
}

#[test]
fn combined_clock_and_delay_overflow_rejects_before_bulk_history_allocation() {
    let slow = TrackId::new();
    let effects = std::array::from_fn::<_, 4, _>(|_| EffectId::new());
    let mut model = vec![channel(slow, &effects), channel(TrackId::MASTER, &[])];
    model.extend((0..10).map(|_| channel(TrackId::new(), &[])));
    let reports: Vec<_> = effects
        .iter()
        .map(|&id| {
            (
                RoutingNode {
                    channel: slow,
                    stage: NodeStage::Effect(id),
                },
                MAX_DEVICE_LATENCY,
            )
        })
        .collect();
    let (result, largest) =
        largest_allocation(|| PreparedRouting::prepare_compensated(&model, 16, &reports, &[], 1));
    assert!(result.err().unwrap().contains("storage budget"));
    assert!(
        largest < 64 * 1024,
        "only metadata, not bulk history, may precede rejection: {largest}"
    );
}

#[test]
fn automation_stage_mapping_deduplication_and_storage_are_exact() {
    let track = TrackId::new();
    let effects = [EffectId::new(), EffectId::new()];
    let model = [channel(track, &effects), channel(TrackId::MASTER, &[])];
    let reports = [
        (
            RoutingNode {
                channel: track,
                stage: NodeStage::Effect(effects[0]),
            },
            137,
        ),
        (
            RoutingNode {
                channel: track,
                stage: NodeStage::Effect(effects[1]),
            },
            521,
        ),
    ];
    let mut routing = PreparedRouting::prepare_compensated(&model, 17, &reports, &[], 1).unwrap();
    let original = routing.storage_samples().unwrap();
    let targets = [
        AutomationTarget::InstrumentParam { param_index: 0 },
        AutomationTarget::PluginParam {
            effect_id: None,
            param_id: 0,
        },
        AutomationTarget::TrackSwingOffset,
        AutomationTarget::EffectParam {
            effect_id: effects[1],
            param_index: 0,
        },
        AutomationTarget::PluginParam {
            effect_id: Some(effects[1]),
            param_id: 0,
        },
        AutomationTarget::TrackGain,
        AutomationTarget::TrackPan,
        AutomationTarget::TrackMute,
        AutomationTarget::Send {
            bus_id: TrackId::new(),
        },
    ];
    let requested: Vec<_> = targets.iter().map(|&target| (track, target)).collect();
    routing.configure_automation(&requested).unwrap();
    for (index, control) in routing.automation_controls.iter().enumerate() {
        assert_eq!(
            routing.graph.nodes[control.node].stage,
            if index < 3 {
                NodeStage::Source
            } else if index < 5 {
                NodeStage::Effect(effects[1])
            } else {
                NodeStage::AfterFader
            }
        );
    }
    assert_eq!(
        routing.storage_samples().unwrap(),
        original + 9 * 17 + 2 * 137 + 4 * 658
    );
    routing.configure_automation(&requested).unwrap();
    assert_eq!(routing.automation_controls.len(), 9);
}

#[test]
fn an_over_budget_automation_batch_is_atomic_and_allocates_no_delay_storage() {
    let source = TrackId::new();
    let effects = std::array::from_fn::<_, 4, _>(|_| EffectId::new());
    let master_effect = EffectId::new();
    let model = [
        channel(source, &effects),
        channel(TrackId::MASTER, &[master_effect]),
    ];
    let reports: Vec<_> = effects
        .iter()
        .map(|&id| {
            (
                RoutingNode {
                    channel: source,
                    stage: NodeStage::Effect(id),
                },
                MAX_DEVICE_LATENCY / 16,
            )
        })
        .collect();
    let mut routing = PreparedRouting::prepare_compensated(&model, 16, &reports, &[], 1).unwrap();
    let targets: Vec<_> = (0..600)
        .map(|param_index| {
            (
                TrackId::MASTER,
                AutomationTarget::EffectParam {
                    effect_id: master_effect,
                    param_index,
                },
            )
        })
        .collect();
    routing
        .configure_automation(&[(TrackId::MASTER, AutomationTarget::TrackGain)])
        .unwrap();
    routing.automation_controls[0].values[0] = 0.75;
    let original = routing.storage_samples().unwrap();
    let (result, largest) = largest_allocation(|| routing.configure_automation(&targets));
    assert!(result.is_err());
    assert!(
        largest < 64 * 1024,
        "over-budget batch must not allocate any control history: {largest}"
    );
    assert_eq!(routing.automation_controls.len(), 1);
    assert_eq!(routing.automation_controls[0].values[0], 0.75);
    assert_eq!(routing.storage_samples().unwrap(), original);
}
