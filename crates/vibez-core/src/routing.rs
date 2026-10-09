use serde::{Deserialize, Serialize};

use crate::id::{EffectId, TrackId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExternalInputId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalInputDescriptor {
    pub id: ExternalInputId,
    pub name: String,
    pub channels: usize,
}

impl ExternalInputDescriptor {
    pub fn supported(&self) -> bool {
        matches!(self.channels, 1 | 2)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ExternalInputBlock<'a> {
    pub id: ExternalInputId,
    pub channels: usize,
    pub samples: &'a [f32],
    pub connected: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceTap {
    BeforeEffects,
    #[default]
    AfterEffects,
    AfterFader,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidechainAssignment {
    pub input_id: ExternalInputId,
    pub source: TrackId,
    pub source_name: String,
    #[serde(default)]
    pub tap: SourceTap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeStage {
    Source,
    Sum,
    Effect(EffectId),
    AfterEffects,
    AfterFader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RoutingNode {
    pub channel: TrackId,
    pub stage: NodeStage,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeKind {
    Main,
    External(ExternalInputId),
    Send(f32),
    Mix,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoutingEdge {
    pub from: usize,
    pub to: usize,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone)]
pub struct RoutingEffect {
    pub id: EffectId,
    pub inputs: Vec<ExternalInputDescriptor>,
    pub assignments: Vec<SidechainAssignment>,
}

#[derive(Debug, Clone)]
pub struct RoutingChannel {
    pub id: TrackId,
    pub is_bus: bool,
    pub effects: Vec<RoutingEffect>,
    pub sends: Vec<(TrackId, f32)>,
}

#[derive(Debug, Clone, Default)]
pub struct RoutingGraph {
    pub nodes: Vec<RoutingNode>,
    pub edges: Vec<RoutingEdge>,
    pub order: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingError {
    FeedbackLoop,
    DuplicateIdentity,
    MasterSource,
}

impl RoutingGraph {
    pub fn prepare(channels: &[RoutingChannel]) -> Result<Self, RoutingError> {
        let mut graph = Self::default();
        for channel in channels {
            let source = RoutingNode {
                channel: channel.id,
                stage: if channel.is_bus || channel.id.is_master() {
                    NodeStage::Sum
                } else {
                    NodeStage::Source
                },
            };
            if graph.nodes.contains(&source) {
                return Err(RoutingError::DuplicateIdentity);
            }
            let mut previous = graph.nodes.len();
            graph.nodes.push(source);
            for stage in channel
                .effects
                .iter()
                .map(|effect| NodeStage::Effect(effect.id))
                .chain([NodeStage::AfterEffects, NodeStage::AfterFader])
            {
                let next = graph.nodes.len();
                let node = RoutingNode {
                    channel: channel.id,
                    stage,
                };
                if graph.nodes.contains(&node) {
                    return Err(RoutingError::DuplicateIdentity);
                }
                graph.nodes.push(node);
                graph.edges.push(RoutingEdge {
                    from: previous,
                    to: next,
                    kind: EdgeKind::Main,
                });
                previous = next;
            }
        }
        for channel in channels {
            let after_fader = graph.index(channel.id, NodeStage::AfterFader).unwrap();
            if !channel.id.is_master() {
                if let Some(master) = graph.index(TrackId::MASTER, NodeStage::Sum) {
                    graph.edges.push(RoutingEdge {
                        from: after_fader,
                        to: master,
                        kind: EdgeKind::Mix,
                    });
                }
            }
            for &(bus, gain) in &channel.sends {
                if let Some(to) = graph.index(bus, NodeStage::Sum) {
                    graph.edges.push(RoutingEdge {
                        from: after_fader,
                        to,
                        kind: EdgeKind::Send(gain),
                    });
                }
            }
            for effect in &channel.effects {
                let to = graph
                    .index(channel.id, NodeStage::Effect(effect.id))
                    .unwrap();
                for route in &effect.assignments {
                    if !effect
                        .inputs
                        .iter()
                        .any(|input| input.id == route.input_id && input.supported())
                    {
                        continue;
                    }
                    if route.source.is_master() {
                        return Err(RoutingError::MasterSource);
                    }
                    let stage = match route.tap {
                        SourceTap::BeforeEffects => {
                            if channels
                                .iter()
                                .any(|source| source.id == route.source && source.is_bus)
                            {
                                NodeStage::Sum
                            } else {
                                NodeStage::Source
                            }
                        }
                        SourceTap::AfterEffects => NodeStage::AfterEffects,
                        SourceTap::AfterFader => NodeStage::AfterFader,
                    };
                    if let Some(from) = graph.index(route.source, stage) {
                        graph.edges.push(RoutingEdge {
                            from,
                            to,
                            kind: EdgeKind::External(route.input_id),
                        });
                    }
                }
            }
        }
        let mut pending: Vec<usize> = (0..graph.nodes.len())
            .map(|node| graph.edges.iter().filter(|edge| edge.to == node).count())
            .collect();
        for node in 0..graph.nodes.len() {
            if pending[node] == 0 {
                graph.order.push(node);
            }
        }
        let mut cursor = 0;
        while cursor < graph.order.len() {
            let node = graph.order[cursor];
            for edge in graph.edges.iter().filter(|edge| edge.from == node) {
                pending[edge.to] -= 1;
                if pending[edge.to] == 0 {
                    graph.order.push(edge.to);
                }
            }
            cursor += 1;
        }
        if graph.order.len() != graph.nodes.len() {
            return Err(RoutingError::FeedbackLoop);
        }
        Ok(graph)
    }

    pub fn index(&self, channel: TrackId, stage: NodeStage) -> Option<usize> {
        self.nodes
            .iter()
            .position(|node| node.channel == channel && node.stage == stage)
    }
}

pub fn adapt_channels(
    source: &[f32],
    source_channels: usize,
    destination: &mut [f32],
    destination_channels: usize,
) {
    destination.fill(0.0);
    if !matches!(source_channels, 1 | 2) || !matches!(destination_channels, 1 | 2) {
        return;
    }
    for (input, output) in source
        .chunks_exact(source_channels)
        .zip(destination.chunks_exact_mut(destination_channels))
    {
        match (source_channels, destination_channels) {
            (1, 2) => output.fill(input[0]),
            (2, 1) => output[0] = (input[0] + input[1]) * 0.5,
            _ => output.copy_from_slice(input),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: TrackId, effect: EffectId) -> RoutingChannel {
        RoutingChannel {
            id,
            is_bus: false,
            sends: vec![],
            effects: vec![RoutingEffect {
                id: effect,
                inputs: vec![ExternalInputDescriptor {
                    id: ExternalInputId(0),
                    name: "Detector".into(),
                    channels: 2,
                }],
                assignments: vec![],
            }],
        }
    }

    #[test]
    fn validates_actual_taps_and_bus_dependencies() {
        let id = TrackId::new();
        let mut channel = channel(id, EffectId::new());
        channel.effects[0].assignments.push(SidechainAssignment {
            input_id: ExternalInputId(0),
            source: id,
            source_name: "Self".into(),
            tap: SourceTap::BeforeEffects,
        });
        assert!(RoutingGraph::prepare(&[channel.clone()]).is_ok());
        channel.effects[0].assignments[0].tap = SourceTap::AfterEffects;
        assert_eq!(
            RoutingGraph::prepare(&[channel]).unwrap_err(),
            RoutingError::FeedbackLoop
        );
    }

    #[test]
    fn stereo_to_mono_cancels_and_mono_duplicates() {
        let mut mono = [1.0; 2];
        adapt_channels(&[0.5, -0.5, 1.0, 0.0], 2, &mut mono, 1);
        assert_eq!(mono, [0.0, 0.5]);
        let mut stereo = [0.0; 4];
        adapt_channels(&mono, 1, &mut stereo, 2);
        assert_eq!(stereo, [0.0, 0.0, 0.5, 0.5]);
    }
}
