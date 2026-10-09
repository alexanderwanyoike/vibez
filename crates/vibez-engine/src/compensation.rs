//! One timing plan for the shared stage graph used by playback and Bounce.

use vibez_core::id::TrackId;
use vibez_core::routing::{EdgeKind, RoutingGraph};
use vibez_dsp::compensation_delay::{CompensationDelay, DelayPreparationError};

/// These are resource bounds, not a monitoring threshold. Reports outside the
/// bounds fail preparation rather than being clamped into incorrect alignment.
pub const MAX_DEVICE_LATENCY: u32 = 1 << 20;
pub const MAX_PATH_LATENCY: u32 = 1 << 22;
pub const MAX_STORAGE_SAMPLES: usize = 1 << 27;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompensationError {
    InvalidGraph,
    DeviceLatency { node: usize, samples: u32 },
    PathLatency { node: usize },
    StorageBudget,
    DelayStorage(DelayPreparationError),
}

#[derive(Debug)]
pub struct CompensationPlan {
    pub generation: u64,
    pub node_input_latency: Vec<u32>,
    pub node_output_latency: Vec<u32>,
    pub edge_delays: Vec<u32>,
    pub output_latency: u32,
    delays: Vec<CompensationDelay>,
}

impl CompensationPlan {
    pub fn storage_samples(&self) -> usize {
        self.delays
            .iter()
            .map(CompensationDelay::storage_samples)
            .sum()
    }

    /// The caller supplies cached device reports by graph node. Reduced
    /// monitoring exempts only eligible direct Mix edges, leaving detector
    /// alignment and compensated Send/return paths intact.
    pub fn prepare(
        graph: &RoutingGraph,
        device_latencies: &[u32],
        reduced_tracks: &[TrackId],
        generation: u64,
    ) -> Result<Self, CompensationError> {
        Self::prepare_with_budget(
            graph,
            device_latencies,
            reduced_tracks,
            generation,
            MAX_STORAGE_SAMPLES,
        )
    }

    pub fn prepare_with_budget(
        graph: &RoutingGraph,
        device_latencies: &[u32],
        reduced_tracks: &[TrackId],
        generation: u64,
        storage_budget: usize,
    ) -> Result<Self, CompensationError> {
        if device_latencies.len() != graph.nodes.len()
            || graph.order.len() != graph.nodes.len()
            || graph
                .edges
                .iter()
                .any(|edge| edge.from >= graph.nodes.len() || edge.to >= graph.nodes.len())
        {
            return Err(CompensationError::InvalidGraph);
        }
        let mut input_latency = vec![0u32; graph.nodes.len()];
        let mut output_latency = vec![0u32; graph.nodes.len()];
        let mut edge_delays = vec![0u32; graph.edges.len()];
        let mut visited = vec![false; graph.nodes.len()];
        for &node in &graph.order {
            if node >= visited.len() || visited[node] {
                return Err(CompensationError::InvalidGraph);
            }
            let report = device_latencies[node];
            if report > MAX_DEVICE_LATENCY {
                return Err(CompensationError::DeviceLatency {
                    node,
                    samples: report,
                });
            }
            let mut arrival = 0;
            for edge in graph.edges.iter().filter(|edge| edge.to == node) {
                if !visited[edge.from] {
                    return Err(CompensationError::InvalidGraph);
                }
                arrival = arrival.max(output_latency[edge.from]);
            }
            input_latency[node] = arrival;
            output_latency[node] = arrival
                .checked_add(report)
                .filter(|&samples| samples <= MAX_PATH_LATENCY)
                .ok_or(CompensationError::PathLatency { node })?;
            for (index, edge) in graph.edges.iter().enumerate().filter(|(_, e)| e.to == node) {
                let exempt = edge.kind == EdgeKind::Mix
                    && graph.nodes[edge.to].channel.is_master()
                    && reduced_tracks.contains(&graph.nodes[edge.from].channel)
                    && !graph.edges.iter().any(|dependency| {
                        matches!(dependency.kind, EdgeKind::External(_))
                            && graph.nodes[dependency.to].channel.is_master()
                    });
                edge_delays[index] = if exempt {
                    0
                } else {
                    arrival - output_latency[edge.from]
                };
            }
            visited[node] = true;
        }
        let total_latency = output_latency.iter().copied().max().unwrap_or(0);
        let mut budget = storage_budget.min(MAX_STORAGE_SAMPLES);
        let mut delays = Vec::with_capacity(edge_delays.len());
        for &frames in &edge_delays {
            let line = CompensationDelay::prepare(frames, 2, budget)
                .map_err(CompensationError::DelayStorage)?;
            budget = budget
                .checked_sub(line.storage_samples())
                .ok_or(CompensationError::StorageBudget)?;
            delays.push(line);
        }
        Ok(Self {
            generation,
            node_input_latency: input_latency,
            node_output_latency: output_latency,
            edge_delays,
            output_latency: total_latency,
            delays,
        })
    }

    pub fn delay_edge(&mut self, edge: usize, samples: &mut [f32], channels: usize) {
        self.delays[edge].process_layout(samples, channels);
    }

    pub fn clear_history(&mut self) {
        for line in &mut self.delays {
            line.clear();
        }
    }

    pub fn output_position(&self, rendered: u64) -> u64 {
        rendered.saturating_sub(self.output_latency as u64)
    }

    pub fn target_position(&self, node: usize, rendered: u64) -> u64 {
        rendered.saturating_sub(self.node_input_latency[node] as u64)
    }

    pub fn direct_path_latency(&self, graph: &RoutingGraph, track: TrackId) -> u32 {
        let omitted = graph
            .edges
            .iter()
            .enumerate()
            .find(|(_, edge)| {
                edge.kind == EdgeKind::Mix
                    && graph.nodes[edge.from].channel == track
                    && graph.nodes[edge.to].channel.is_master()
            })
            .map_or(0, |(index, edge)| {
                self.node_input_latency[edge.to]
                    - self.node_output_latency[edge.from]
                    - self.edge_delays[index]
            });
        self.output_latency.saturating_sub(omitted)
    }
}

#[cfg(test)]
#[path = "compensation_tests.rs"]
mod tests;
