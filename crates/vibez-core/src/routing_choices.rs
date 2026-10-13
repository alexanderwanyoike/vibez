//! Cycle-safe source choices for a supported receiving input.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSourceChoice {
    pub source: TrackId,
    pub taps: Vec<SourceTap>,
}

pub fn input_source_choices(
    channels: &[RoutingChannel],
    receiver: TrackId,
    effect_id: EffectId,
    input: ExternalInputId,
) -> Vec<InputSourceChoice> {
    let mut candidate = channels.to_vec();
    let Some(effect) = candidate
        .iter_mut()
        .find(|channel| channel.id == receiver)
        .and_then(|channel| {
            channel
                .effects
                .iter_mut()
                .find(|effect| effect.id == effect_id)
        })
    else {
        return vec![];
    };
    if !effect
        .inputs
        .iter()
        .any(|descriptor| descriptor.id == input && descriptor.supported())
    {
        return vec![];
    }
    effect.assignments.retain(|route| route.input_id != input);
    effect.inactive_inputs.retain(|id| *id != input);
    let Ok(graph) = RoutingGraph::prepare(&candidate) else {
        return vec![];
    };
    let Some(receiver_node) = graph.index(receiver, NodeStage::Effect(effect_id)) else {
        return vec![];
    };
    let mut downstream = vec![false; graph.nodes.len()];
    let mut pending = vec![receiver_node];
    downstream[receiver_node] = true;
    while let Some(node) = pending.pop() {
        for edge in graph.edges.iter().filter(|edge| edge.from == node) {
            if !downstream[edge.to] {
                downstream[edge.to] = true;
                pending.push(edge.to);
            }
        }
    }
    channels
        .iter()
        .filter(|source| !source.id.is_master())
        .filter_map(|source| {
            let taps: Vec<_> = [
                SourceTap::BeforeEffects,
                SourceTap::AfterEffects,
                SourceTap::AfterFader,
            ]
            .into_iter()
            .filter(|tap| {
                graph
                    .index(source.id, tap.stage(source.is_bus))
                    .is_some_and(|node| !downstream[node])
            })
            .collect();
            (!taps.is_empty()).then_some(InputSourceChoice {
                source: source.id,
                taps,
            })
        })
        .collect()
}

pub fn valid_input_taps(
    channels: &[RoutingChannel],
    receiver: TrackId,
    effect: EffectId,
    input: ExternalInputId,
    source: TrackId,
) -> Vec<SourceTap> {
    input_source_choices(channels, receiver, effect, input)
        .into_iter()
        .find(|choice| choice.source == source)
        .map_or_else(Vec::new, |choice| choice.taps)
}
