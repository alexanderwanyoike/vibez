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
    pub samples: Vec<f32>,
    pub inputs: Vec<PreparedInput>,
}

#[derive(Debug)]
pub struct PreparedRouting {
    pub graph: RoutingGraph,
    pub nodes: Vec<PreparedNode>,
    pub max_frames: usize,
    pub compensation: crate::compensation::CompensationPlan,
    pub capture_offsets: std::sync::Arc<[(vibez_core::id::TrackId, u32)]>,
    pub edge_samples: Vec<Vec<f32>>,
    pub bypass_delays: Vec<vibez_dsp::compensation_delay::CompensationDelay>,
    pub bypass_samples: Vec<Vec<f32>>,
    pub device_latencies: Vec<u32>,
    pub channel_clocks: Vec<crate::compensation_controls::ChannelClock>,
    pub automation_controls: Vec<crate::compensation_controls::PreparedAutomationControl>,
    pub presentation: crate::compensation_clock::PresentationHistory,
    pub presentation_start: Option<crate::compensation_clock::PresentationPosition>,
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
        let scratch_samples = preparation_samples(channels, &graph, max_frames)?;
        if scratch_samples > crate::compensation::MAX_STORAGE_SAMPLES {
            return Err("Routing exceeds the compensation storage budget".into());
        }
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
            .and_then(|count| count.checked_add(input_channels))
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
        let device_latencies = vec![0; graph.nodes.len()];
        let compensation =
            crate::compensation::CompensationPlan::prepare(&graph, &device_latencies, &[], 0)
                .map_err(|error| format!("Compensation preparation failed: {error:?}"))?;
        let edge_samples = graph
            .edges
            .iter()
            .map(|_| vec![0.0; max_frames * 2])
            .collect();
        let bypass_samples = graph
            .nodes
            .iter()
            .map(|_| vec![0.0; max_frames * 2])
            .collect();
        let bypass_delays = graph
            .nodes
            .iter()
            .map(|_| vibez_dsp::compensation_delay::CompensationDelay::prepare(0, 2, 0).unwrap())
            .collect();
        Ok(Box::new(Self {
            graph,
            nodes,
            max_frames,
            compensation,
            capture_offsets: std::sync::Arc::from([]),
            edge_samples,
            bypass_delays,
            bypass_samples,
            device_latencies,
            channel_clocks: channels
                .iter()
                .map(|channel| {
                    crate::compensation_controls::ChannelClock::prepare(channel.id, 0, max_frames)
                })
                .collect::<Result<_, _>>()?,
            automation_controls: Vec::new(),
            presentation: crate::compensation_clock::PresentationHistory::prepare(0)
                .map_err(str::to_owned)?,
            presentation_start: None,
            detector_buses,
        }))
    }

    pub fn prepare_compensated(
        channels: &[RoutingChannel],
        max_frames: usize,
        reports: &[(vibez_core::routing::RoutingNode, u32)],
        reduced_tracks: &[vibez_core::id::TrackId],
        generation: u64,
    ) -> Result<Box<Self>, String> {
        let mut routing = Self::prepare(channels, max_frames)?;
        for (index, node) in routing.graph.nodes.iter().enumerate() {
            routing.device_latencies[index] = reports
                .iter()
                .find(|(key, _)| key == node)
                .map_or(0, |(_, value)| *value);
        }
        routing.compensation = crate::compensation::CompensationPlan::prepare_with_budget(
            &routing.graph,
            &routing.device_latencies,
            reduced_tracks,
            generation,
            crate::compensation::MAX_STORAGE_SAMPLES
                .checked_sub(preparation_samples(channels, &routing.graph, max_frames)?)
                .ok_or("Routing scratch storage exceeds the compensation budget")?,
        )
        .map_err(|error| format!("Compensation preparation failed: {error:?}"))?;
        routing.capture_offsets = channels
            .iter()
            .filter(|channel| !channel.is_bus && !channel.id.is_master())
            .filter_map(|channel| {
                let omitted = routing.compensation.output_latency.saturating_sub(
                    routing
                        .compensation
                        .direct_path_latency(&routing.graph, channel.id),
                );
                (omitted > 0).then_some((channel.id, omitted))
            })
            .collect::<Vec<_>>()
            .into();
        let history_samples = (routing.compensation.output_latency as usize + 1)
            .checked_mul(std::mem::size_of::<
                crate::compensation_clock::PresentationPosition,
            >())
            .and_then(|bytes| bytes.checked_add(3))
            .map(|bytes| bytes / 4)
            .ok_or("Presentation storage size overflow")?;
        let used = preparation_samples(channels, &routing.graph, max_frames)?
            .checked_add(history_samples)
            .and_then(|samples| samples.checked_add(routing.compensation.storage_samples()))
            .ok_or("Compensation storage size overflow")?;
        let mut budget = crate::compensation::MAX_STORAGE_SAMPLES
            .checked_sub(used)
            .ok_or("Compensation exceeds the combined storage budget")?;
        let clock_samples = (routing.compensation.output_latency as usize)
            .checked_add(max_frames)
            .and_then(|samples| samples.checked_add(1))
            .and_then(|samples| samples.checked_mul(channels.len()))
            .and_then(|samples| samples.checked_mul(std::mem::size_of::<u64>() / 4))
            .ok_or("Channel clock history size overflow")?;
        budget = budget
            .checked_sub(clock_samples)
            .ok_or("Channel clock history exceeds the compensation storage budget")?;
        // Capture can start after a source changed at the render head, while
        // the previous source is still reaching the full mix.
        routing.channel_clocks = channels
            .iter()
            .map(|channel| {
                crate::compensation_controls::ChannelClock::prepare(
                    channel.id,
                    routing.compensation.output_latency,
                    max_frames,
                )
            })
            .collect::<Result<_, _>>()?;
        routing.presentation = crate::compensation_clock::PresentationHistory::prepare(
            routing.compensation.output_latency,
        )
        .map_err(str::to_owned)?;
        for (index, node) in routing.graph.nodes.iter().enumerate() {
            if matches!(node.stage, vibez_core::routing::NodeStage::Effect(_)) {
                let delay = vibez_dsp::compensation_delay::CompensationDelay::prepare(
                    routing.device_latencies[index],
                    2,
                    budget,
                )
                .map_err(|error| format!("Bypass delay preparation failed: {error:?}"))?;
                budget -= delay.storage_samples();
                routing.bypass_delays[index] = delay;
            }
        }
        Ok(routing)
    }

    pub fn configure_automation(
        &mut self,
        targets: &[(
            vibez_core::id::TrackId,
            vibez_core::automation::AutomationTarget,
        )],
    ) -> Result<(), String> {
        use vibez_core::automation::AutomationTarget;
        use vibez_core::routing::NodeStage;
        let mut used = self.compensation.storage_samples()
            + self
                .bypass_delays
                .iter()
                .map(|line| line.storage_samples())
                .sum::<usize>()
            + self
                .channel_clocks
                .iter()
                .map(|clock| clock.storage_bytes() / 4)
                .sum::<usize>()
            + (self.compensation.output_latency as usize + 1)
                * std::mem::size_of::<crate::compensation_clock::PresentationPosition>()
                / 4
            + (self.nodes.len() * 4 + self.graph.edges.len() * 2) * self.max_frames
            + self
                .nodes
                .iter()
                .flat_map(|node| &node.inputs)
                .map(|input| input.samples.len())
                .sum::<usize>();
        for &(track, target) in targets {
            if self
                .automation_controls
                .iter()
                .any(|control| control.track == track && control.target == target)
            {
                continue;
            }
            let stage = match target {
                AutomationTarget::EffectParam { effect_id, .. }
                | AutomationTarget::PluginParam {
                    effect_id: Some(effect_id),
                    ..
                } => NodeStage::Effect(effect_id),
                AutomationTarget::InstrumentParam { .. }
                | AutomationTarget::PluginParam {
                    effect_id: None, ..
                }
                | AutomationTarget::TrackSwingOffset => NodeStage::Source,
                _ => NodeStage::AfterFader,
            };
            if let Some(node) = self.graph.index(track, stage) {
                let budget = crate::compensation::MAX_STORAGE_SAMPLES
                    .checked_sub(used)
                    .and_then(|samples| samples.checked_sub(self.max_frames))
                    .ok_or("Automation exceeds the compensation storage budget")?;
                let control = crate::compensation_controls::PreparedAutomationControl::prepare(
                    track,
                    target,
                    node,
                    self.compensation.node_input_latency[node],
                    self.max_frames,
                    budget,
                )?;
                used += control.storage_samples();
                self.automation_controls.push(control);
            }
        }
        Ok(())
    }

    pub fn clear_history(&mut self) {
        self.compensation.clear_history();
        self.presentation.clear();
        for clock in &mut self.channel_clocks {
            clock.clear();
        }
        for control in &mut self.automation_controls {
            control.clear();
        }
        self.presentation_start = None;
        for delay in &mut self.bypass_delays {
            delay.clear();
        }
    }
}

fn preparation_samples(
    channels: &[RoutingChannel],
    graph: &RoutingGraph,
    max_frames: usize,
) -> Result<usize, String> {
    let inputs: usize = channels
        .iter()
        .flat_map(|channel| &channel.effects)
        .flat_map(|effect| &effect.inputs)
        .filter(|input| input.supported())
        .map(|input| input.channels)
        .sum();
    graph
        .nodes
        .len()
        .checked_mul(4)
        .and_then(|samples| {
            graph
                .edges
                .len()
                .checked_mul(2)
                .and_then(|edges| samples.checked_add(edges))
        })
        .and_then(|samples| samples.checked_add(inputs))
        .and_then(|samples| samples.checked_mul(max_frames))
        .ok_or_else(|| "Routing storage size overflow".to_owned())
}
