//! Checked accounting for all prepared routing audio and timing storage.

use super::*;
use crate::compensation::{CompensationTiming, MAX_STORAGE_SAMPLES};
use vibez_dsp::compensation_delay::CompensationDelay;

#[derive(Default)]
struct Samples(usize);
impl Samples {
    fn add(&mut self, samples: usize) -> Result<(), String> {
        self.0 = self
            .0
            .checked_add(samples)
            .ok_or("Routing storage size overflow")?;
        Ok(())
    }
    fn frames(&mut self, frames: usize, channels: usize) -> Result<(), String> {
        self.add(
            frames
                .checked_mul(channels)
                .ok_or("Routing storage size overflow")?,
        )
    }
    fn items<T>(&mut self, count: usize) -> Result<(), String> {
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or("Routing storage size overflow")?;
        self.bytes(bytes)
    }
    fn bytes(&mut self, bytes: usize) -> Result<(), String> {
        self.add(
            bytes
                .checked_add(std::mem::size_of::<f32>() - 1)
                .ok_or("Routing storage size overflow")?
                / std::mem::size_of::<f32>(),
        )
    }
}

pub(super) fn check_budget(samples: usize) -> Result<(), String> {
    if samples > MAX_STORAGE_SAMPLES {
        return Err("Routing exceeds the combined compensation storage budget".into());
    }
    Ok(())
}

pub(super) fn planned_samples(
    graph: &RoutingGraph,
    inputs: usize,
    channels: usize,
    frames: usize,
    timing: &CompensationTiming,
    latencies: &[u32],
) -> Result<usize, String> {
    let mut used = Samples::default();
    used.frames(
        frames,
        graph
            .nodes
            .len()
            .checked_mul(4)
            .ok_or("Routing storage size overflow")?,
    )?;
    used.frames(
        frames,
        graph
            .edges
            .len()
            .checked_mul(2)
            .ok_or("Routing storage size overflow")?,
    )?;
    used.frames(frames, inputs)?;
    used.frames(frames, 6)?;
    used.add(
        timing
            .storage_samples()
            .map_err(|error| format!("Compensation storage preparation failed: {error:?}"))?,
    )?;
    for (node, &latency) in graph.nodes.iter().zip(latencies) {
        if matches!(node.stage, vibez_core::routing::NodeStage::Effect(_)) {
            used.add(
                CompensationDelay::required_samples(latency, 2)
                    .map_err(|error| format!("Bypass storage size failed: {error:?}"))?,
            )?;
        }
    }
    let clock_count = crate::guarded_history::GuardedHistory::<u64>::required_count(
        timing.output_latency,
        frames,
    )
    .map_err(str::to_owned)?;
    used.items::<u64>(
        clock_count
            .checked_mul(channels)
            .ok_or("Channel clock size overflow")?,
    )?;
    used.items::<crate::compensation_clock::PresentationPosition>(
        crate::guarded_history::GuardedHistory::<crate::compensation_clock::PresentationPosition>::required_count(timing.output_latency, 0).map_err(str::to_owned)?,
    )?;
    Ok(used.0)
}

pub(super) fn allocated_samples(routing: &PreparedRouting) -> Result<usize, String> {
    let mut used = Samples::default();
    for values in [
        &routing.layout_input,
        &routing.layout_output,
        &routing.layout_capture,
    ] {
        used.add(values.len())?;
    }
    for node in &routing.nodes {
        used.add(node.samples.len())?;
        for input in &node.inputs {
            used.add(input.samples.len())?;
        }
    }
    for values in routing.edge_samples.iter().chain(&routing.bypass_samples) {
        used.add(values.len())?;
    }
    used.add(routing.compensation.storage_samples())?;
    for line in &routing.bypass_delays {
        used.add(line.storage_samples())?;
    }
    for clock in &routing.channel_clocks {
        used.bytes(clock.storage_bytes())?;
    }
    used.bytes(routing.presentation.storage_bytes())?;
    for control in &routing.automation_controls {
        used.add(control.storage_samples())?;
    }
    Ok(used.0)
}

pub(super) fn automation_samples(delay: u32, frames: usize) -> Result<usize, String> {
    crate::compensation_controls::PreparedAutomationControl::required_samples(delay, frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::{
        id::{EffectId, TrackId},
        routing::*,
    };

    #[test]
    fn partial_float_bytes_round_up_and_arithmetic_overflow_is_rejected() {
        let mut samples = Samples::default();
        samples.items::<[u8; 3]>(3).unwrap();
        assert_eq!(samples.0, 3);
        assert!(samples.bytes(usize::MAX).is_err());
        assert!(samples.frames(usize::MAX, 2).is_err());
        let mut samples = Samples(usize::MAX);
        assert!(samples.add(1).is_err());
    }

    #[test]
    fn forecast_equals_allocated_audio_and_timing_storage_with_external_input() {
        let source = TrackId::new();
        let receiver = TrackId::new();
        let effect = EffectId::new();
        let model = [
            RoutingChannel {
                id: source,
                is_bus: false,
                effects: vec![],
                sends: vec![],
            },
            RoutingChannel {
                id: receiver,
                is_bus: false,
                sends: vec![],
                effects: vec![RoutingEffect {
                    id: effect,
                    inactive_inputs: vec![],
                    inputs: vec![ExternalInputDescriptor {
                        id: ExternalInputId(7),
                        name: "Detector".into(),
                        channels: 1,
                    }],
                    assignments: vec![SidechainAssignment {
                        input_id: ExternalInputId(7),
                        input_name: "Detector".into(),
                        source,
                        source_name: "Source".into(),
                        tap: SourceTap::AfterEffects,
                    }],
                }],
            },
            RoutingChannel {
                id: TrackId::MASTER,
                is_bus: true,
                effects: vec![],
                sends: vec![],
            },
        ];
        for frames in [1, 17, 63] {
            let reports = [(
                RoutingNode {
                    channel: receiver,
                    stage: NodeStage::Effect(effect),
                },
                521,
            )];
            let routing =
                PreparedRouting::prepare_compensated(&model, frames, &reports, &[], 2).unwrap();
            let timing =
                CompensationTiming::prepare(&routing.graph, &routing.device_latencies, &[], 2)
                    .unwrap();
            assert_eq!(
                planned_samples(
                    &routing.graph,
                    1,
                    model.len(),
                    frames,
                    &timing,
                    &routing.device_latencies
                )
                .unwrap(),
                routing.storage_samples().unwrap()
            );
        }
    }
}
