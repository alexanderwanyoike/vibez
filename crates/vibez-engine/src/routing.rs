//! Prepared graph buffers and indexed channel bindings for callback delivery.

use vibez_core::routing::{ExternalInputDescriptor, RoutingChannel, RoutingError, RoutingGraph};

pub const MAX_EXTERNAL_INPUTS: usize = 64;
pub const MAX_ROUTING_FRAMES: usize = 65_536;
pub const MAX_ROUTING_STORAGE_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug)]
pub struct PreparedInput {
    pub descriptor: ExternalInputDescriptor,
    pub connected: bool,
    pub source: Option<usize>,
    pub samples: Vec<f32>,
}

#[derive(Debug)]
pub struct PreparedNode {
    pub channel: usize,
    pub incoming: Vec<usize>,
    pub samples: Vec<f32>,
    pub inputs: Vec<PreparedInput>,
}

#[derive(Debug)]
pub struct PreparedRouting {
    pub channels: Vec<PreparedChannel>,
    pub layout_output: Vec<f32>,
    pub layout_input: Vec<f32>,
    pub layout_capture: Vec<f32>,
    pub graph: RoutingGraph,
    pub nodes: Vec<PreparedNode>,
    pub max_frames: usize,
    pub detector_buses: Vec<vibez_core::id::TrackId>,
}

impl PreparedRouting {
    pub fn prepare(channels: &[RoutingChannel], max_frames: usize) -> Result<Box<Self>, String> {
        if max_frames == 0 || max_frames > MAX_ROUTING_FRAMES {
            return Err(format!(
                "Routing block capacity must be in 1..={MAX_ROUTING_FRAMES}"
            ));
        }
        let graph = RoutingGraph::prepare(channels).map_err(|error| match error {
            RoutingError::FeedbackLoop => "Routing creates a feedback loop",
            RoutingError::DuplicateIdentity => "Routing contains duplicate identities",
            RoutingError::MasterSource => "Master cannot provide an external input",
        })?;
        let input_channels: usize = channels
            .iter()
            .flat_map(|channel| &channel.effects)
            .flat_map(|effect| &effect.inputs)
            .filter(|input| input.supported())
            .map(|input| input.channels)
            .sum();
        let storage = graph
            .nodes
            .len()
            .checked_mul(2)
            .and_then(|count| count.checked_add(input_channels + 6))
            .and_then(|count| count.checked_mul(max_frames))
            .and_then(|count| count.checked_mul(std::mem::size_of::<f32>()))
            .ok_or("Routing storage arithmetic overflow")?;
        if storage > MAX_ROUTING_STORAGE_BYTES {
            return Err(format!(
                "Routing exceeds the {} MiB storage budget",
                MAX_ROUTING_STORAGE_BYTES / 1024 / 1024
            ));
        }
        let mut required = vec![false; graph.nodes.len()];
        for edge in &graph.edges {
            if matches!(edge.kind, vibez_core::routing::EdgeKind::External(_)) {
                required[edge.from] = true;
            }
        }
        for &node in graph.order.iter().rev() {
            if required[node] {
                for edge in graph.edges.iter().filter(|edge| edge.to == node) {
                    required[edge.from] = true;
                }
            }
        }
        let detector_buses = channels
            .iter()
            .filter(|channel| {
                channel.is_bus
                    && graph
                        .nodes
                        .iter()
                        .enumerate()
                        .any(|(index, node)| node.channel == channel.id && required[index])
            })
            .map(|channel| channel.id)
            .collect();
        let mut nodes = Vec::with_capacity(graph.nodes.len());
        for (index, node) in graph.nodes.iter().enumerate() {
            let mut inputs = Vec::new();
            if let vibez_core::routing::NodeStage::Effect(id) = node.stage {
                if let Some(effect) = channels
                    .iter()
                    .find(|channel| channel.id == node.channel)
                    .and_then(|channel| channel.effects.iter().find(|effect| effect.id == id))
                {
                    for descriptor in effect.inputs.iter().filter(|input| input.supported()) {
                        inputs.push(PreparedInput {
                            descriptor: descriptor.clone(),
                            connected: effect
                                .assignments
                                .iter()
                                .any(|route| route.matches(descriptor)),
                            source: graph
                                .edges
                                .iter()
                                .find(|edge| {
                                    edge.to == index
                                        && edge.kind
                                            == vibez_core::routing::EdgeKind::External(
                                                descriptor.id,
                                            )
                                })
                                .map(|edge| edge.from),
                            samples: vec![0.0; max_frames * descriptor.channels],
                        });
                    }
                }
            }
            if inputs.len() > MAX_EXTERNAL_INPUTS {
                return Err(format!(
                    "An effect exceeds the supported {MAX_EXTERNAL_INPUTS} external inputs"
                ));
            }
            nodes.push(PreparedNode {
                channel: channels
                    .iter()
                    .position(|channel| channel.id == node.channel)
                    .unwrap(),
                incoming: graph
                    .edges
                    .iter()
                    .enumerate()
                    .filter_map(|(edge_index, edge)| (edge.to == index).then_some(edge_index))
                    .collect(),
                samples: vec![0.0; max_frames * 2],
                inputs,
            });
        }
        Ok(Box::new(Self {
            channels: {
                let mut track = 0;
                let mut bus = 0;
                channels
                    .iter()
                    .map(|channel| {
                        let binding = if channel.id.is_master() {
                            ChannelIndex::Master
                        } else if channel.is_bus {
                            let index = bus;
                            bus += 1;
                            ChannelIndex::Bus(index)
                        } else {
                            let index = track;
                            track += 1;
                            ChannelIndex::Track(index)
                        };
                        PreparedChannel {
                            id: channel.id,
                            binding,
                            audible_return: false,
                        }
                    })
                    .collect()
            },
            layout_output: vec![0.0; max_frames * 2],
            layout_input: vec![0.0; max_frames * 2],
            layout_capture: vec![0.0; max_frames * 2],
            graph,
            nodes,
            max_frames,
            detector_buses,
        }))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ChannelIndex {
    Track(usize),
    Bus(usize),
    Master,
    Missing,
}
#[derive(Debug)]
pub struct PreparedChannel {
    pub id: vibez_core::id::TrackId,
    pub binding: ChannelIndex,
    pub(crate) audible_return: bool,
}

impl PreparedChannel {
    pub(crate) fn bind(
        &mut self,
        tracks: &[crate::mixer::EngineTrack],
        buses: &[crate::mixer::EngineTrack],
    ) {
        let valid = match self.binding {
            ChannelIndex::Track(index) => {
                tracks.get(index).is_some_and(|track| track.id == self.id)
            }
            ChannelIndex::Bus(index) => buses.get(index).is_some_and(|bus| bus.id == self.id),
            ChannelIndex::Master => true,
            ChannelIndex::Missing => false,
        };
        if !valid {
            self.binding = tracks
                .iter()
                .position(|track| track.id == self.id)
                .map(ChannelIndex::Track)
                .or_else(|| {
                    buses
                        .iter()
                        .position(|bus| bus.id == self.id)
                        .map(ChannelIndex::Bus)
                })
                .unwrap_or(ChannelIndex::Missing);
        }
    }
}
impl ChannelIndex {
    pub(crate) fn get<'a>(
        self,
        tracks: &'a [crate::mixer::EngineTrack],
        buses: &'a [crate::mixer::EngineTrack],
        master: &'a crate::mixer::EngineTrack,
    ) -> Option<&'a crate::mixer::EngineTrack> {
        match self {
            Self::Track(index) => tracks.get(index),
            Self::Bus(index) => buses.get(index),
            Self::Master => Some(master),
            Self::Missing => None,
        }
    }
    pub(crate) fn get_mut<'a>(
        self,
        tracks: &'a mut [crate::mixer::EngineTrack],
        buses: &'a mut [crate::mixer::EngineTrack],
        master: &'a mut crate::mixer::EngineTrack,
    ) -> Option<&'a mut crate::mixer::EngineTrack> {
        match self {
            Self::Track(index) => tracks.get_mut(index),
            Self::Bus(index) => buses.get_mut(index),
            Self::Master => Some(master),
            Self::Missing => None,
        }
    }
}

#[path = "routing_solo.rs"]
mod solo;
