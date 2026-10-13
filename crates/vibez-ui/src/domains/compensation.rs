//! Cached device reports and automation targets prepare full project compensation.

use vibez_core::id::TrackId;
use vibez_core::routing::{NodeStage, RoutingNode};

#[derive(Debug, Clone, PartialEq)]
pub struct TimingSignature {
    pub reports: Vec<(RoutingNode, u32)>,
    pub reduced_tracks: Vec<TrackId>,
    pub sample_rate: u32,
    pub controls: Vec<(TrackId, vibez_core::automation::AutomationTarget)>,
}

pub fn signature_for_state(state: &crate::state::AppState) -> TimingSignature {
    let mut timing = TimingSignature {
        reports: Vec::new(),
        reduced_tracks: Vec::new(),
        sample_rate: state.transport.sample_rate,
        controls: Vec::new(),
    };
    for track in state
        .project_tracks
        .tracks
        .iter()
        .chain(&state.project_tracks.buses)
        .chain(std::iter::once(&state.project_tracks.master))
    {
        timing.reports.push((
            RoutingNode {
                channel: track.id,
                stage: NodeStage::Source,
            },
            track.instrument_latency_samples.unwrap_or(0),
        ));
        for effect in &track.effects {
            if let Some(latency) = effect.latency_samples {
                timing.reports.push((
                    RoutingNode {
                        channel: track.id,
                        stage: NodeStage::Effect(effect.id),
                    },
                    latency,
                ));
            }
        }
    }
    let timelines = std::iter::once(state.arrangement.timeline.as_ref())
        .chain(
            state
                .perform
                .sections
                .sections
                .iter()
                .map(|section| section.timeline.as_ref()),
        )
        .chain(
            state
                .perform
                .clips
                .clips
                .iter()
                .map(|clip| clip.timeline.as_ref()),
        );
    for timeline in timelines {
        for (&track, content) in &timeline.by_track {
            for lane in &content.automation {
                timing.controls.push((track, lane.target));
            }
        }
    }
    if let Some(previous) = &state.devices.last_timing {
        if previous.reports == timing.reports
            && previous.reduced_tracks == timing.reduced_tracks
            && previous.sample_rate == timing.sample_rate
        {
            timing.controls.extend(previous.controls.iter().copied());
        }
    }
    timing
        .controls
        .sort_by_cached_key(|(track, target)| (track.raw(), format!("{target:?}")));
    timing.controls.dedup();
    timing
}

#[cfg(test)]
#[path = "compensation_bridge_tests.rs"]
mod bridge_tests;
