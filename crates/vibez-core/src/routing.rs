//! Declared external inputs and the channel-stage routing contract.

use serde::{Deserialize, Serialize};

pub const SEND_SILENCE_THRESHOLD: f32 = 0.0005;

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
    #[serde(default)]
    pub input_name: String,
    pub source: TrackId,
    pub source_name: String,
    #[serde(default)]
    pub tap: SourceTap,
}

impl SidechainAssignment {
    pub fn matches(&self, input: &ExternalInputDescriptor) -> bool {
        input.supported()
            && self.input_id == input.id
            && (self.input_name.is_empty() || self.input_name == input.name)
    }
}

impl SourceTap {
    pub fn stage(self, is_bus: bool) -> NodeStage {
        match self {
            Self::BeforeEffects if is_bus => NodeStage::Sum,
            Self::BeforeEffects => NodeStage::Source,
            Self::AfterEffects => NodeStage::AfterEffects,
            Self::AfterFader => NodeStage::AfterFader,
        }
    }
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

#[derive(Debug, Clone, PartialEq)]
pub struct RoutingEffect {
    pub inactive_inputs: Vec<ExternalInputId>,
    pub id: EffectId,
    pub inputs: Vec<ExternalInputDescriptor>,
    pub assignments: Vec<SidechainAssignment>,
}

#[derive(Debug, Clone, PartialEq)]
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
                if graph.nodes.contains(&node)
                    || matches!(stage,NodeStage::Effect(id) if graph.nodes.iter().any(|existing| existing.stage==NodeStage::Effect(id)))
                {
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
                if gain <= SEND_SILENCE_THRESHOLD {
                    continue;
                }
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
                    if effect.inactive_inputs.contains(&route.input_id)
                        || !effect.inputs.iter().any(|input| route.matches(input))
                    {
                        continue;
                    }
                    if route.source.is_master() {
                        return Err(RoutingError::MasterSource);
                    }
                    let stage = route.tap.stage(
                        channels
                            .iter()
                            .any(|source| source.id == route.source && source.is_bus),
                    );
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
        for (node, dependencies) in pending.iter().enumerate() {
            if *dependencies == 0 {
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
        for (channel, sample) in output.iter_mut().enumerate() {
            *sample =
                adapt_channel_sample(source_channels, destination_channels, channel, |index| {
                    input[index]
                });
        }
    }
}

pub fn adapt_channel_sample(
    source_channels: usize,
    destination_channels: usize,
    channel: usize,
    source: impl Fn(usize) -> f32,
) -> f32 {
    match (source_channels, destination_channels) {
        (1, 1 | 2) => source(0),
        (2, 1) => (source(0) + source(1)) * 0.5,
        (2, 2) if channel < 2 => source(channel),
        _ => 0.0,
    }
}

/// A stereo project occupies the first L/R pair of a declared main layout.
/// Auxiliary layouts retain the strict mono/stereo law above.
pub fn adapt_main_channel_sample(
    source_channels: usize,
    destination_channels: usize,
    channel: usize,
    source: impl Fn(usize) -> f32,
) -> f32 {
    if channel >= 2 || channel >= destination_channels {
        return 0.0;
    }
    adapt_channel_sample(
        source_channels.min(2),
        destination_channels.min(2),
        channel,
        source,
    )
}

#[path = "routing_choices.rs"]
mod choices;
pub use choices::{input_source_choices, valid_input_taps, InputSourceChoice};
#[path = "routing_restore.rs"]
mod restore;
pub use restore::resolve_restored;

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
                inactive_inputs: vec![],
            }],
        }
    }

    #[test]
    fn validates_actual_taps_and_bus_dependencies() {
        let id = TrackId::new();
        let mut channel = channel(id, EffectId::new());
        channel.effects[0].assignments.push(SidechainAssignment {
            input_id: ExternalInputId(0),
            input_name: "Detector".into(),
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

    #[test]
    fn main_layouts_use_only_the_first_pair_and_preserve_mono_conversion() {
        let surround = [1.0, 3.0, 99.0, 99.0, 99.0, 99.0];
        assert_eq!(
            adapt_main_channel_sample(6, 2, 0, |index| surround[index]),
            1.0
        );
        assert_eq!(
            adapt_main_channel_sample(6, 2, 1, |index| surround[index]),
            3.0
        );
        assert_eq!(
            adapt_main_channel_sample(6, 1, 0, |index| surround[index]),
            2.0
        );
        for channel in 0..6 {
            assert_eq!(
                adapt_main_channel_sample(1, 6, channel, |_| 0.5),
                if channel < 2 { 0.5 } else { 0.0 }
            );
        }
        assert_eq!(adapt_channel_sample(6, 2, 0, |_| 99.0), 0.0);
    }
}
