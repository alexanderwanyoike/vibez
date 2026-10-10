//! Plan publications retain audio only for the same declared edge.
use super::*;
use vibez_core::routing::{NodeStage, RoutingEdge, RoutingNode};

fn graph(channels: &[TrackId]) -> RoutingGraph {
    let master = channels.len();
    RoutingGraph {
        nodes: channels
            .iter()
            .map(|&channel| RoutingNode {
                channel,
                stage: NodeStage::Source,
            })
            .chain(std::iter::once(RoutingNode {
                channel: TrackId::MASTER,
                stage: NodeStage::Sum,
            }))
            .collect(),
        edges: (0..master)
            .map(|from| RoutingEdge {
                from,
                to: master,
                kind: EdgeKind::Mix,
            })
            .collect(),
        order: (0..=master).collect(),
    }
}

fn plan(graph: &RoutingGraph, late: TrackId, latency: u32) -> CompensationPlan {
    let reports: Vec<_> = graph
        .nodes
        .iter()
        .map(|node| if node.channel == late { latency } else { 0 })
        .collect();
    CompensationPlan::prepare(graph, &reports, &[], 1).unwrap()
}

#[test]
fn reordered_nodes_and_edges_keep_only_their_own_audio() {
    let [a, b, late] = [TrackId::new(), TrackId::new(), TrackId::new()];
    let old_graph = graph(&[a, b, late]);
    let mut previous = plan(&old_graph, late, 4);
    previous.delay_edge(0, &mut [0.25; 8], 2);
    previous.delay_edge(1, &mut [0.75; 8], 2);
    let next_graph = graph(&[b, a, late]);
    let mut next = plan(&next_graph, late, 4);
    assert_eq!(
        crate::retirement::tests::allocations(|| next.retain_history_from(&mut previous)),
        (0, 0)
    );
    for (edge, value) in [(0, 0.75), (1, 0.25)] {
        let mut output = [0.0; 8];
        next.delay_edge(edge, &mut output, 2);
        assert_eq!(output, [value; 8]);
    }
}

#[test]
fn an_added_edge_starts_silent_and_existing_edges_keep_history() {
    let [a, added, late] = [TrackId::new(), TrackId::new(), TrackId::new()];
    let mut previous = plan(&graph(&[a, late]), late, 4);
    previous.delay_edge(0, &mut [0.5; 8], 2);
    let mut next = plan(&graph(&[added, a, late]), late, 4);
    next.retain_history_from(&mut previous);
    let mut fresh = [0.0; 8];
    next.delay_edge(0, &mut fresh, 2);
    assert_eq!(fresh, [0.0; 8]);
    let mut retained = [0.0; 8];
    next.delay_edge(1, &mut retained, 2);
    assert_eq!(retained, [0.5; 8]);
}

#[test]
fn changed_edge_kind_does_not_inherit_another_signal_path() {
    let [a, late] = [TrackId::new(), TrackId::new()];
    let mut previous = plan(&graph(&[a, late]), late, 4);
    previous.delay_edge(0, &mut [0.5; 8], 2);
    let mut changed = graph(&[a, late]);
    changed.edges[0].kind = EdgeKind::Send;
    let mut next = plan(&changed, late, 4);
    next.retain_history_from(&mut previous);
    let mut output = [0.0; 8];
    next.delay_edge(0, &mut output, 2);
    assert_eq!(output, [0.0; 8]);
}

#[test]
fn changed_retained_capacity_resets_the_matching_edge() {
    let [a, late] = [TrackId::new(), TrackId::new()];
    let g = graph(&[a, late]);
    let mut previous = plan(&g, late, 4);
    previous.delay_edge(0, &mut [0.5; 6], 2);
    let mut next = plan(&g, late, 2);
    let storage = next.storage_samples();
    assert_eq!(
        crate::retirement::tests::allocations(|| next.retain_history_from(&mut previous)),
        (0, 0)
    );
    assert_eq!(next.storage_samples(), storage);
    let mut output = [0.0; 8];
    next.delay_edge(0, &mut output, 2);
    assert_eq!(output, [0.0; 8]);
}

#[test]
fn changed_source_identity_does_not_replay_the_removed_source() {
    let [old_source, replacement, late] = [TrackId::new(), TrackId::new(), TrackId::new()];
    let mut previous = plan(&graph(&[old_source, late]), late, 4);
    previous.delay_edge(0, &mut [0.5; 8], 2);
    let mut next = plan(&graph(&[replacement, late]), late, 4);
    next.retain_history_from(&mut previous);
    let mut output = [0.0; 8];
    next.delay_edge(0, &mut output, 2);
    assert_eq!(output, [0.0; 8]);
}

#[test]
fn duplicate_edge_identity_is_rejected_before_retention_can_reuse_a_line() {
    let [a, late] = [TrackId::new(), TrackId::new()];
    let mut g = graph(&[a, late]);
    g.edges.push(g.edges[0]);
    let reports = [0, 4, 0];
    assert!(matches!(
        CompensationPlan::prepare(&g, &reports, &[], 1),
        Err(CompensationError::InvalidGraph)
    ));
}
