use super::*;
use vibez_core::id::EffectId;
use vibez_core::routing::{
    ExternalInputDescriptor, ExternalInputId, NodeStage, RoutingChannel, RoutingEffect,
    SidechainAssignment, SourceTap,
};

fn channel(id: TrackId, effects: &[EffectId], bus: bool) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: bus,
        effects: effects
            .iter()
            .map(|&id| RoutingEffect {
                inactive_inputs: vec![],
                id,
                inputs: vec![],
                assignments: vec![],
            })
            .collect(),
        sends: vec![],
    }
}

#[test]
fn series_parallel_bus_and_master_use_accumulated_arrivals() {
    let a = TrackId::new();
    let b = TrackId::new();
    let bus = TrackId::new();
    let effects = [EffectId::new(), EffectId::new(), EffectId::new()];
    let mut source = channel(a, &effects[..2], false);
    source.sends.push(bus);
    let mut dry = channel(b, &[], false);
    dry.sends.push(bus);
    let graph = RoutingGraph::prepare(&[
        channel(TrackId::MASTER, &effects[2..], true),
        channel(bus, &[], true),
        dry,
        source,
    ])
    .unwrap();
    let mut reports = vec![0; graph.nodes.len()];
    for (effect, delay) in effects.into_iter().zip([137, 521, 17]) {
        let node = graph
            .nodes
            .iter()
            .position(|n| n.stage == NodeStage::Effect(effect))
            .unwrap();
        reports[node] = delay;
    }
    let plan = CompensationPlan::prepare(&graph, &reports, &[], 1).unwrap();
    assert_eq!(plan.output_latency, 675);
    let dry_tap = graph.index(b, NodeStage::AfterFader).unwrap();
    for (edge, &delay) in graph.edges.iter().zip(&plan.edge_delays) {
        if edge.from == dry_tap && matches!(edge.kind, EdgeKind::Mix | EdgeKind::Send) {
            assert_eq!(delay, 658);
        }
    }
}

#[test]
fn receiver_aligns_main_and_detector_at_every_tap() {
    for tap in [
        SourceTap::BeforeEffects,
        SourceTap::AfterEffects,
        SourceTap::AfterFader,
    ] {
        let source_id = TrackId::new();
        let target_id = TrackId::new();
        let source_fx = EffectId::new();
        let target_fx = [EffectId::new(), EffectId::new()];
        let mut target = channel(target_id, &target_fx, false);
        target.effects[1].inputs.push(ExternalInputDescriptor {
            id: ExternalInputId(7),
            name: "Probe".into(),
            channels: 1,
        });
        target.effects[1].assignments.push(SidechainAssignment {
            input_id: ExternalInputId(7),
            input_name: "Probe".into(),
            source: source_id,
            source_name: "Source".into(),
            tap,
        });
        let graph = RoutingGraph::prepare(&[
            target,
            channel(source_id, &[source_fx], false),
            channel(TrackId::MASTER, &[], true),
        ])
        .unwrap();
        let mut reports = vec![0; graph.nodes.len()];
        reports[graph
            .index(source_id, NodeStage::Effect(source_fx))
            .unwrap()] = 521;
        reports[graph
            .index(target_id, NodeStage::Effect(target_fx[0]))
            .unwrap()] = 137;
        let plan = CompensationPlan::prepare(&graph, &reports, &[], 1).unwrap();
        let receiver = graph
            .index(target_id, NodeStage::Effect(target_fx[1]))
            .unwrap();
        let arrivals: Vec<_> = graph
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| edge.to == receiver)
            .map(|(index, edge)| plan.node_output_latency[edge.from] + plan.edge_delays[index])
            .collect();
        assert_eq!(arrivals.len(), 2);
        assert_eq!(arrivals[0], arrivals[1]);
        assert_eq!(
            arrivals[0],
            if tap == SourceTap::BeforeEffects {
                137
            } else {
                521
            }
        );
    }
}

#[test]
fn reduced_monitoring_exempts_direct_output_but_preserves_returns() {
    let monitored = TrackId::new();
    let slow = TrackId::new();
    let bus = TrackId::new();
    let fx = EffectId::new();
    let mut live = channel(monitored, &[], false);
    live.sends.push(bus);
    let graph = RoutingGraph::prepare(&[
        live,
        channel(slow, &[fx], false),
        channel(bus, &[EffectId::new()], true),
        channel(TrackId::MASTER, &[], true),
    ])
    .unwrap();
    let mut reports = vec![0; graph.nodes.len()];
    reports[graph.index(slow, NodeStage::Effect(fx)).unwrap()] = 521;
    let bus_fx = graph
        .nodes
        .iter()
        .position(|n| n.channel == bus && matches!(n.stage, NodeStage::Effect(_)))
        .unwrap();
    reports[bus_fx] = 137;
    let full = CompensationPlan::prepare(&graph, &reports, &[], 1).unwrap();
    let reduced = CompensationPlan::prepare(&graph, &reports, &[monitored], 2).unwrap();
    let direct = graph
        .edges
        .iter()
        .position(|e| e.kind == EdgeKind::Mix && graph.nodes[e.from].channel == monitored)
        .unwrap();
    assert_eq!(full.edge_delays[direct], 521);
    assert_eq!(reduced.edge_delays[direct], 0);
    for (index, edge) in graph.edges.iter().enumerate() {
        if index != direct {
            assert_eq!(
                reduced.edge_delays[index], full.edge_delays[index],
                "{edge:?}"
            );
        }
    }
    assert_eq!(reduced.output_latency, full.output_latency);
}

#[test]
fn reduced_monitoring_keeps_master_detector_alignment() {
    let live = TrackId::new();
    let slow = TrackId::new();
    let slow_fx = EffectId::new();
    let master_fx = EffectId::new();
    let mut master = channel(TrackId::MASTER, &[master_fx], true);
    master.effects[0].inputs.push(ExternalInputDescriptor {
        id: ExternalInputId(0),
        name: "Detector".into(),
        channels: 2,
    });
    master.effects[0].assignments.push(SidechainAssignment {
        input_id: ExternalInputId(0),
        input_name: "Detector".into(),
        source: slow,
        source_name: "Slow".into(),
        tap: SourceTap::AfterEffects,
    });
    let graph = RoutingGraph::prepare(&[
        channel(live, &[], false),
        channel(slow, &[slow_fx], false),
        master,
    ])
    .unwrap();
    let mut reports = vec![0; graph.nodes.len()];
    reports[graph.index(slow, NodeStage::Effect(slow_fx)).unwrap()] = 521;
    let full = CompensationPlan::prepare(&graph, &reports, &[], 1).unwrap();
    let reduced = CompensationPlan::prepare(&graph, &reports, &[live], 2).unwrap();
    assert_eq!(reduced.edge_delays, full.edge_delays);
    assert_eq!(reduced.direct_path_latency(&graph, live), 521);
}

#[test]
fn excessive_reports_and_invalid_order_fail_without_clamping() {
    let graph = RoutingGraph::prepare(&[channel(TrackId::MASTER, &[], true)]).unwrap();
    let mut reports = vec![0; graph.nodes.len()];
    reports[0] = MAX_DEVICE_LATENCY + 1;
    assert!(matches!(
        CompensationPlan::prepare(&graph, &reports, &[], 1),
        Err(CompensationError::DeviceLatency { node: 0, .. })
    ));
    let mut broken = graph.clone();
    broken.order.reverse();
    reports[0] = 0;
    assert!(matches!(
        CompensationPlan::prepare(&broken, &reports, &[], 2),
        Err(CompensationError::InvalidGraph)
    ));
}

#[test]
fn sample_oracle_detects_uncompensated_and_wrong_reported_paths() {
    fn aligned(actual: u32, reported: u32, compensate: bool) -> bool {
        let mut slow = CompensationDelay::prepare(actual, 2, 4096).unwrap();
        let mut dry =
            CompensationDelay::prepare(if compensate { reported } else { 0 }, 2, 4096).unwrap();
        for offset in (0..1200).step_by(17) {
            let mut a = [0.0; 34];
            if offset == 0 {
                a[0] = 1.0;
                a[1] = -1.0;
            }
            let mut b = a;
            slow.process(&mut a);
            dry.process(&mut b);
            if a != b {
                return false;
            }
        }
        true
    }
    assert!(aligned(137, 137, true));
    assert!(aligned(521, 521, true));
    assert!(!aligned(137, 137, false));
    assert!(!aligned(137, 138, true));
}

#[test]
fn chained_bus_sends_align_each_sum_and_accumulate_all_processor_reports() {
    let source = TrackId::new();
    let dry = TrackId::new();
    let bus_a = TrackId::new();
    let bus_b = TrackId::new();
    let effects = [EffectId::new(), EffectId::new(), EffectId::new()];
    let mut input = channel(source, &effects[..1], false);
    input.sends.push(bus_a);
    let mut first_bus = channel(bus_a, &effects[1..2], true);
    first_bus.sends.push(bus_b);
    let mut dry = channel(dry, &[], false);
    dry.sends.push(bus_b);
    let graph = RoutingGraph::prepare(&[
        input,
        first_bus,
        channel(bus_b, &effects[2..], true),
        dry,
        channel(TrackId::MASTER, &[], true),
    ])
    .unwrap();
    let mut reports = vec![0; graph.nodes.len()];
    for (effect, latency) in effects.into_iter().zip([137, 521, 17]) {
        reports[graph
            .nodes
            .iter()
            .position(|node| node.stage == NodeStage::Effect(effect))
            .unwrap()] = latency;
    }
    let plan = CompensationPlan::prepare(&graph, &reports, &[], 1).unwrap();
    assert_eq!(plan.output_latency, 675);
    for bus in [bus_a, bus_b, TrackId::MASTER] {
        let node = graph.index(bus, NodeStage::Sum).unwrap();
        for (index, edge) in graph
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| edge.to == node)
        {
            assert_eq!(
                plan.node_output_latency[edge.from] + plan.edge_delays[index],
                plan.node_input_latency[node],
                "sum {bus:?}"
            );
        }
    }
}

#[test]
fn aggregate_delay_budget_is_rejected_before_any_individual_line_is_prepared() {
    let source = TrackId::new();
    let dry = TrackId::new();
    let effect = EffectId::new();
    let graph = RoutingGraph::prepare(&[
        channel(source, &[effect], false),
        channel(dry, &[], false),
        channel(TrackId::MASTER, &[], true),
    ])
    .unwrap();
    let mut reports = vec![0; graph.nodes.len()];
    reports[graph.index(source, NodeStage::Effect(effect)).unwrap()] = 137;
    let timing = CompensationTiming::prepare(&graph, &reports, &[], 1).unwrap();
    let needed = timing.storage_samples().unwrap();
    assert!(matches!(
        timing.allocate(needed - 1),
        Err(CompensationError::DelayStorage(
            DelayPreparationError::StorageBudget
        ))
    ));
    assert_eq!(
        CompensationPlan::prepare_with_budget(&graph, &reports, &[], 1, needed)
            .unwrap()
            .storage_samples(),
        needed
    );
}
