use std::collections::HashMap;
use vibez_core::{
    id::{EffectId, TrackId},
    routing::{ExternalInputId, NodeStage, RoutingChannel, RoutingGraph, SourceTap},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSourceChoice {
    pub source: TrackId,
    pub taps: Vec<SourceTap>,
}
pub type SidechainChoiceCache = HashMap<(EffectId, ExternalInputId), Vec<InputSourceChoice>>;

pub fn input_source_choices(channels: &[RoutingChannel]) -> SidechainChoiceCache {
    let mut choices = HashMap::new();
    for receiver in channels {
        for effect in &receiver.effects {
            for input in effect.inputs.iter().filter(|input| input.supported()) {
                let mut candidate = channels.to_vec();
                let owner = candidate
                    .iter_mut()
                    .find(|channel| channel.id == receiver.id)
                    .unwrap()
                    .effects
                    .iter_mut()
                    .find(|entry| entry.id == effect.id)
                    .unwrap();
                owner.assignments.retain(|route| route.input_id != input.id);
                let Ok(graph) = RoutingGraph::prepare(&candidate) else {
                    choices.insert((effect.id, input.id), vec![]);
                    continue;
                };
                let receiver_node = graph
                    .index(receiver.id, NodeStage::Effect(effect.id))
                    .unwrap();
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
                // Replacing one input removes its old edge first. A new edge
                // forms a cycle exactly when the receiver already reaches its tap.
                let mut sources = Vec::new();
                for source in channels.iter().filter(|source| !source.id.is_master()) {
                    let taps = [
                        SourceTap::BeforeEffects,
                        SourceTap::AfterEffects,
                        SourceTap::AfterFader,
                    ]
                    .into_iter()
                    .filter(|tap| {
                        let stage = match tap {
                            SourceTap::BeforeEffects => {
                                if source.is_bus {
                                    NodeStage::Sum
                                } else {
                                    NodeStage::Source
                                }
                            }
                            SourceTap::AfterEffects => NodeStage::AfterEffects,
                            SourceTap::AfterFader => NodeStage::AfterFader,
                        };
                        graph
                            .index(source.id, stage)
                            .is_some_and(|node| !downstream[node])
                    })
                    .collect::<Vec<_>>();
                    if !taps.is_empty() {
                        sources.push(super::InputSourceChoice {
                            source: source.id,
                            taps,
                        });
                    }
                }
                choices.insert((effect.id, input.id), sources);
            }
        }
    }
    choices
}
