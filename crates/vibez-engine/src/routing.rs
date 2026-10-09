use vibez_core::routing::{ExternalInputDescriptor, RoutingChannel, RoutingError, RoutingGraph};

pub const MAX_EXTERNAL_INPUTS: usize = 64;

#[derive(Debug)]
pub struct PreparedInput {
    pub descriptor: ExternalInputDescriptor,
    pub connected: bool,
    pub source: Option<usize>,
    pub samples: Vec<f32>,
}

#[derive(Debug)]
pub struct PreparedNode {
    pub samples: Vec<f32>,
    pub inputs: Vec<PreparedInput>,
}

#[derive(Debug)]
pub struct PreparedRouting {
    pub graph: RoutingGraph,
    pub nodes: Vec<PreparedNode>,
    pub max_frames: usize,
}

impl PreparedRouting {
    pub fn prepare(channels: &[RoutingChannel], max_frames: usize) -> Result<Box<Self>, String> {
        if max_frames == 0 {
            return Err("Routing block capacity must be positive".into());
        }
        let graph = RoutingGraph::prepare(channels).map_err(|error| match error {
            RoutingError::FeedbackLoop => "Routing creates a feedback loop",
            RoutingError::DuplicateIdentity => "Routing contains duplicate identities",
            RoutingError::MasterSource => "Master cannot provide an external input",
        })?;
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
                            connected: effect.assignments.iter().any(|route| {
                                route.input_id == descriptor.id
                                    && (route.input_name.is_empty()
                                        || route.input_name == descriptor.name)
                            }),
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
                samples: vec![0.0; max_frames * 2],
                inputs,
            });
        }
        Ok(Box::new(Self {
            graph,
            nodes,
            max_frames,
        }))
    }
}
